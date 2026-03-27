# VM Manager + Snapshot Restore

## Context

Implements the Firecracker VM lifecycle in `agentbox-core`. Creates VMs from
snapshots using Firecracker's REST API over Unix Domain Socket. This is the
performance-critical path — snapshot restore with mmap achieves <300ms boot.

This task assumes `crates/agentbox-core/src/vm.rs` exists with `VmHandle` and
`VmManager` type stubs from FAU-67 (project scaffold).

AgentBox is a self-hosted sandbox infrastructure for AI agents.
See `docs/spec.md` and `docs/architecture.md` for full context.

## Requirements

- Create Firecracker VMs from snapshot (mmap memory backend)
- Copy-on-write rootfs per VM
- Wait for Firecracker API socket
- HTTP calls to Firecracker REST API via UDS
- Destroy VMs cleanly (kill process, cleanup temp files)
- Check if VM process is still running

## Implementation

### Modify `crates/agentbox-core/Cargo.toml`

Add these dependencies:
```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
hyper = { version = "1", features = ["client", "http1"] }
hyper-util = { version = "0.1", features = ["tokio", "client-legacy"] }
http-body-util = "0.1"
hyperlocal = "0.9"
tempfile = "3"
```

### Implement `crates/agentbox-core/src/vm.rs`

Replace all `todo!()` stubs with real implementations.

**`VmManager::create_from_snapshot(&self, config: &SandboxConfig) -> Result<VmHandle>`**:

```rust
pub async fn create_from_snapshot(&self, _config: &SandboxConfig) -> Result<VmHandle> {
    let vm_id = uuid::Uuid::new_v4().to_string()[..12].to_string();
    let work_dir = tempfile::tempdir()?.into_path();

    // 1. CoW copy rootfs
    let rootfs_dest = work_dir.join("rootfs.ext4");
    tokio::fs::copy(&self.config.rootfs_path, &rootfs_dest).await?;

    // 2. Spawn Firecracker process with cwd=work_dir
    let api_socket = work_dir.join("api.sock");
    let process = tokio::process::Command::new(&self.config.firecracker_bin)
        .arg("--api-sock")
        .arg("api.sock")
        .current_dir(&work_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AgentBoxError::VmCreation(format!("Failed to spawn firecracker: {e}")))?;

    // 3. Wait for API socket (poll existence, timeout 5s)
    wait_for_socket(&api_socket, std::time::Duration::from_secs(5)).await?;

    // 4. Restore snapshot via Firecracker API
    restore_snapshot(&api_socket, &self.config.snapshot_path).await?;

    let vsock_uds = work_dir.join("vsock.sock");

    Ok(VmHandle {
        id: vm_id,
        process,
        api_socket,
        vsock_uds,
        work_dir,
    })
}
```

**`wait_for_socket(path, timeout) -> Result<()>`** (private helper):
- Loop: check `path.exists()`, sleep 50ms between checks
- If timeout exceeded: return `Err(AgentBoxError::VmCreation("API socket did not appear"))`

**`restore_snapshot(api_socket, snapshot_dir) -> Result<()>`** (private helper):
- Build absolute paths: `snapshot_dir/vmstate.bin` and `snapshot_dir/memory.bin`
- Use `hyperlocal` to make HTTP PUT to Firecracker API via UDS at `api_socket`:
  ```
  PUT http://localhost/snapshot/load
  {
      "snapshot_path": "/absolute/path/to/vmstate.bin",
      "mem_backend": {
          "backend_path": "/absolute/path/to/memory.bin",
          "backend_type": "File"
      },
      "enable_diff_snapshots": false,
      "resume_vm": true
  }
  ```
- Check response status, return error if not 2xx

Implementation of the HTTP-over-UDS call using `hyperlocal`:
```rust
use hyperlocal::{UnixClientExt, Uri as UnixUri};
use hyper::body::Bytes;
use http_body_util::Full;

async fn fc_api_put(socket: &Path, path: &str, body: serde_json::Value) -> Result<()> {
    let client = hyper_util::client::legacy::Client::unix();
    let uri = UnixUri::new(socket, path).into();
    let body_bytes = serde_json::to_vec(&body)?;

    let req = hyper::Request::builder()
        .method("PUT")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .map_err(|e| AgentBoxError::VmCreation(e.to_string()))?;

    let resp = client.request(req).await
        .map_err(|e| AgentBoxError::VmCreation(format!("Firecracker API call failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = http_body_util::BodyExt::collect(resp.into_body()).await
            .map(|b| String::from_utf8_lossy(&b.to_bytes()).to_string())
            .unwrap_or_default();
        return Err(AgentBoxError::SnapshotLoad(format!("FC API {status}: {body}")));
    }
    Ok(())
}
```

Note: the exact `hyperlocal` API may vary by version. If `hyperlocal 0.9` doesn't compile
with `hyper 1.x`, use `hyper-util`'s Unix connector directly or fall back to a simple
`tokio::net::UnixStream` + raw HTTP request. The key is: HTTP PUT to `api.sock`.

**`VmManager::destroy(&self, mut vm: VmHandle) -> Result<()>`**:
```rust
pub async fn destroy(&self, mut vm: VmHandle) -> Result<()> {
    // Kill the Firecracker process
    let _ = vm.process.kill().await;
    // Wait with timeout
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        vm.process.wait()
    ).await;
    // Clean up temp directory
    let _ = tokio::fs::remove_dir_all(&vm.work_dir).await;
    tracing::info!(vm_id = %vm.id, "VM destroyed");
    Ok(())
}
```

**`VmManager::is_running(vm: &VmHandle) -> bool`**:
```rust
pub fn is_running(vm: &mut VmHandle) -> bool {
    // try_wait returns Ok(Some(status)) if exited, Ok(None) if still running
    matches!(vm.process.try_wait(), Ok(None))
}
```
Note: `try_wait` takes `&mut self`, so the signature may need `&mut VmHandle`.

### Implement `crates/agentbox-core/src/snapshot.rs`

Replace `todo!()` in `SnapshotManager::load`:
```rust
pub async fn load(&self, api_socket: &std::path::Path) -> Result<()> {
    let vmstate = self.snapshot_path.join("vmstate.bin").canonicalize()?;
    let memory = self.snapshot_path.join("memory.bin").canonicalize()?;

    let body = serde_json::json!({
        "snapshot_path": vmstate.to_str().unwrap(),
        "mem_backend": {
            "backend_path": memory.to_str().unwrap(),
            "backend_type": "File"
        },
        "enable_diff_snapshots": false,
        "resume_vm": true
    });

    fc_api_put(api_socket, "/snapshot/load", body).await
}
```

Move `fc_api_put` to a shared location (either `snapshot.rs` or a new `crates/agentbox-core/src/fc_api.rs` helper module). Update `vm.rs` to use the same helper.

## Testing Strategy

Run tests: `cargo test -p agentbox-core -- vm snapshot`

### Unit tests in `crates/agentbox-core/src/vm.rs`:
- `test_vm_manager_new` — create VmManager with valid config, verify fields
- `test_wait_for_socket_timeout` — call with nonexistent path, verify timeout error
- `test_wait_for_socket_success` — create file in background, verify success

### Unit tests in `crates/agentbox-core/src/snapshot.rs`:
- `test_snapshot_paths` — verify SnapshotManager builds correct vmstate/memory paths

### Integration tests (require KVM, skip in CI without it):
- `test_create_and_destroy_vm` — create from snapshot, verify process running, destroy, verify cleaned up
- Mark with `#[cfg(feature = "integration")]` or `#[ignore]` with comment

## Out of Scope

- Pool integration (Task E)
- Vsock communication to guest agent (Task D)
- Network configuration for VMs
- Resource limits (memory/CPU capping via Firecracker API)
- Reflink/CoW optimization detection
