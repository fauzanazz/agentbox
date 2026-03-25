# AgentBox — Implementation Plan

## Task Breakdown (Wave Model)

This plan splits into 6 waves. Each wave's tasks are independent and can run in
parallel. Each task is ~30 min of coding work.

```
Wave 1: [A] Project scaffold + core types     ~30 min
         [B] Guest agent binary                ~30 min

Wave 2: [C] VM Manager + snapshot restore      ~30 min  (needs A)
         [D] Vsock client (host-side)          ~30 min  (needs A, B for protocol)

Wave 3: [E] Pool + sandbox abstraction         ~30 min  (needs C, D)

Wave 4: [F] Daemon HTTP API                    ~30 min  (needs E)
         [G] WebSocket exec handler            ~30 min  (needs E)

Wave 5: [H] Python SDK                         ~30 min  (needs F, G)
         [I] CLI (clap)                        ~30 min  (needs E, F)

Wave 6: [J] Build pipeline (Makefile)          ~30 min  (needs B)
         [K] Install script + CI               ~30 min  (needs J, and all binaries)
```

---

## Task A: Project Scaffold + Core Types

**Wave 1 — No dependencies**

### Context
Bootstrap the Cargo workspace with all 4 crates and define the core type system
that everything else depends on. This is the foundation — get the types and traits
right so downstream tasks can compile against them.

### Requirements
- Cargo workspace with 4 crates
- All core types, error types, and trait definitions
- Compiles with `cargo check` (no implementations yet, use `todo!()`)

### Implementation

#### Files to create:

**`Cargo.toml`** (workspace root)
```toml
[workspace]
resolver = "2"
members = [
    "crates/agentbox-core",
    "crates/agentbox-daemon",
    "crates/agentbox-cli",
    "crates/guest-agent",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
repository = "https://github.com/ORG/agentbox"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

**`crates/agentbox-core/Cargo.toml`**
```toml
[package]
name = "agentbox-core"
version.workspace = true
edition.workspace = true

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
toml = "0.8"
```

**`crates/agentbox-core/src/lib.rs`**
```rust
pub mod config;
pub mod error;
pub mod pool;
pub mod sandbox;
pub mod snapshot;
pub mod vm;
pub mod vsock;
```

**`crates/agentbox-core/src/error.rs`**
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentBoxError {
    #[error("VM creation failed: {0}")]
    VmCreation(String),

    #[error("VM not found: {0}")]
    VmNotFound(String),

    #[error("Vsock connection failed: {0}")]
    VsockConnection(String),

    #[error("Command execution failed: {0}")]
    ExecFailed(String),

    #[error("File operation failed: {0}")]
    FileOp(String),

    #[error("Pool exhausted: no available sandboxes")]
    PoolExhausted,

    #[error("Snapshot load failed: {0}")]
    SnapshotLoad(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AgentBoxError>;
```

**`crates/agentbox-core/src/config.rs`**
- `DaemonConfig` — listen address, log level
- `VmConfig` — firecracker_bin, kernel/rootfs/snapshot paths
- `VmDefaults` — default memory_mb, vcpus, network, timeout
- `PoolConfig` — min_size, max_size, idle_timeout
- `GuestConfig` — vsock_port, ping_timeout
- `AgentBoxConfig` — top-level, contains all sub-configs
- `impl AgentBoxConfig` — `from_file(path)` and `from_env()` functions

**`crates/agentbox-core/src/sandbox.rs`**
- `SandboxId` — newtype around `String`
- `SandboxConfig` — memory_mb, vcpus, network, timeout_secs
- `SandboxInfo` — id, status, config, created_at
- `SandboxStatus` — enum: Creating, Ready, Busy, Destroying
- `ExecResult` — stdout, stderr, exit_code
- `ExecEvent` — enum: Stdout(Vec<u8>), Stderr(Vec<u8>), Exit(i32), Error(String)
- `ExecStream` — wraps mpsc::Receiver<ExecEvent> + mpsc::Sender<Vec<u8>> for stdin
- `FileEntry` — name, size, is_dir
- `Sandbox` struct — id, vm handle, vsock client, config
- `impl Sandbox` — all methods with `todo!()` bodies

**`crates/agentbox-core/src/vm.rs`**
- `VmHandle` — id, process (Option for now), api_socket, vsock_uds, work_dir
- `VmConfig` struct (if different from config.rs VmConfig)
- `VmManager` struct — config
- `impl VmManager` — `create_from_snapshot()`, `destroy()`, `is_running()` with `todo!()`

**`crates/agentbox-core/src/pool.rs`**
- `Pool` struct — config, vm_manager, available (Mutex<VecDeque>), active (RwLock<HashMap>)
- `impl Pool` — `new()`, `start()`, `claim()`, `release()`, `list_active()`, `shutdown()` with `todo!()`

**`crates/agentbox-core/src/vsock.rs`**
- `VsockClient` struct — uds_path, port
- `impl VsockClient` — `new()`, `ping()`, `exec()`, `exec_stream()`, `signal()`, `read_file()`, `write_file()`, `list_files()` with `todo!()`

**`crates/agentbox-core/src/snapshot.rs`**
- `SnapshotManager` struct — snapshot_path
- `impl SnapshotManager` — `load(api_socket)` with `todo!()`

**`crates/agentbox-daemon/Cargo.toml`** — depends on agentbox-core, axum, tokio-tungstenite
**`crates/agentbox-daemon/src/main.rs`** — minimal `fn main() { todo!() }`

**`crates/agentbox-cli/Cargo.toml`** — depends on agentbox-core, clap
**`crates/agentbox-cli/src/main.rs`** — minimal `fn main() { todo!() }`

**`crates/guest-agent/Cargo.toml`** — standalone (tokio, serde, serde_json, nix)
**`crates/guest-agent/src/main.rs`** — minimal `fn main() { todo!() }`

**Also create:**
- `LICENSE` — Apache-2.0
- `README.md` — project overview (can be brief for now)
- `config.example.toml` — example config with all sections
- `.gitignore` — standard Rust + target/, artifacts/output/

### Testing
- `cargo check` passes for all workspace members
- `cargo test` passes (no tests yet, but no compile errors)

### Out of scope
- No implementations — only type definitions and `todo!()` stubs
- No guest-agent protocol implementation
- No actual Firecracker interaction

---

## Task B: Guest Agent Binary

**Wave 1 — No dependencies (standalone crate)**

### Context
The guest agent is a small Rust binary that runs inside each Firecracker VM. It
listens on a vsock port and handles commands from the host: exec, file operations,
and process management. This is a standalone crate with no workspace dependencies.

The protocol is length-prefixed JSON over vsock, proven in the claude-harness project.

### Requirements
- vsock server listening on port 5000
- Length-prefixed JSON codec (4-byte big-endian length prefix)
- Command execution with stdout/stderr capture
- Streaming exec with PTY allocation
- File read/write/list operations
- Signal forwarding to running processes
- Ping/health check

### Implementation

**`crates/guest-agent/Cargo.toml`**
```toml
[package]
name = "guest-agent"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
nix = { version = "0.29", features = ["process", "signal", "pty", "fs"] }
base64 = "0.22"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
```

**`crates/guest-agent/src/main.rs`**
- Parse CLI args (optional vsock port override, default 5000)
- Init tracing subscriber
- Bind vsock listener: `tokio::net::UnixListener` on `/tmp/guest-agent.sock`
  (Firecracker exposes guest vsock as CID 3, host connects via UDS)
- Actually: guest uses `vsock::VsockListener` — use `tokio-vsock` crate or raw
  socket with `AF_VSOCK`. Bind to `VMADDR_CID_ANY:5000`.
- For each connection: spawn task, read length-prefixed messages, dispatch to handler
- Graceful shutdown on SIGTERM

**`crates/guest-agent/src/protocol.rs`**
- `Request` struct: `{ id: u64, method: String, params: Option<serde_json::Value> }`
- `Response` struct: `{ id: u64, result: Option<Value>, error: Option<String> }`
- `StreamMessage` struct: `{ id: u64, stream: String, data: String }`
- `async fn read_message(reader) -> Result<Request>` — read 4-byte len + JSON
- `async fn write_message(writer, response) -> Result<()>` — write 4-byte len + JSON
- `async fn write_stream(writer, msg) -> Result<()>` — write streaming message

**`crates/guest-agent/src/server.rs`**
- `async fn handle_connection(stream)` — read messages in loop, dispatch by method:
  - `"ping"` → `{"status": "ok"}`
  - `"exec"` → `exec::run_command(params)`
  - `"exec_stream"` → `exec::run_stream(params, writer)`
  - `"stdin"` → forward to active PTY
  - `"signal"` → forward signal to active process
  - `"read_file"` → `files::read(params)`
  - `"write_file"` → `files::write(params)`
  - `"list_files"` → `files::list(params)`

**`crates/guest-agent/src/exec.rs`**
- `async fn run_command(command: &str, timeout: u64) -> ExecResult`
  - Use `tokio::process::Command` with `/bin/sh -c`
  - Capture stdout + stderr as strings
  - Enforce timeout with `tokio::time::timeout`
  - Return `ExecResult { stdout, stderr, exit_code }`

- `async fn run_stream(command: &str, writer, id: u64) -> Result<()>`
  - Open PTY with `nix::pty::openpty()`
  - Fork process with PTY as controlling terminal
  - Read from PTY master in a loop, send `StreamMessage` for each chunk
  - Handle stdin messages by writing to PTY master
  - On process exit, send final `Response` with exit_code

**`crates/guest-agent/src/files.rs`**
- `async fn read_file(path: &str) -> Result<String>`
  - Read file, base64 encode, return `{ content: "<base64>" }`
- `async fn write_file(path: &str, content_b64: &str) -> Result<Value>`
  - Base64 decode, write to path, return `{ bytes_written: N }`
- `async fn list_files(path: &str) -> Result<Value>`
  - Read directory, return `{ entries: [{ name, size, is_dir }] }`

### Testing
- Unit tests for protocol codec (read/write messages with in-memory buffers)
- Unit tests for file operations (use temp dirs)
- Unit test for command execution (run simple commands)
- `cargo test -p guest-agent`

### Out of scope
- vsock listener (will use TCP for testing, vsock for production)
- PTY resize handling (future)
- Process group management

---

## Task C: VM Manager + Snapshot Restore

**Wave 2 — Depends on: Task A (core types)**

### Context
Implements the Firecracker VM lifecycle in `agentbox-core`. Creates VMs from
snapshots using Firecracker's REST API over Unix Domain Socket. This is the
performance-critical path — snapshot restore with mmap should achieve <300ms boot.

### Requirements
- Create a Firecracker VM from snapshot (mmap memory backend)
- Copy-on-write rootfs per VM
- Wait for API socket and manage the Firecracker process
- Destroy VMs cleanly (kill process, cleanup temp files)
- Check if VM is running

### Implementation

**Modify `crates/agentbox-core/Cargo.toml`** — add dependencies:
```toml
hyper = { version = "1", features = ["client", "http1"] }
hyper-util = { version = "0.1", features = ["tokio", "client-legacy"] }
http-body-util = "0.1"
hyperlocal = "0.9"    # Unix domain socket HTTP client
tempfile = "3"
```

**Implement `crates/agentbox-core/src/vm.rs`**

`VmManager::create_from_snapshot(config: &SandboxConfig) -> Result<VmHandle>`:
1. Generate vm_id: `uuid::Uuid::new_v4().to_string()[..12]`
2. Create temp dir: `tempfile::tempdir_in("/tmp")` named `agentbox-{vm_id}`
3. Copy rootfs with reflink: try `std::fs::copy` (on btrfs/xfs this does CoW)
   - Source: `self.config.rootfs_path`
   - Dest: `{work_dir}/rootfs.ext4`
4. Spawn Firecracker process:
   ```rust
   tokio::process::Command::new(&self.config.firecracker_bin)
       .arg("--api-sock").arg("api.sock")
       .current_dir(&work_dir)
       .stdout(Stdio::piped())
       .stderr(Stdio::piped())
       .spawn()
   ```
5. Wait for socket: poll `work_dir/api.sock` existence, timeout 5s
6. Restore snapshot via Firecracker API:
   - PUT `http://localhost/snapshot/load` via UDS at `api.sock`
   - Body: `{ snapshot_path, mem_backend: { backend_path, backend_type: "File" }, enable_diff_snapshots: false, resume_vm: true }`
   - Use absolute paths for snapshot files, relative path for rootfs (cwd=work_dir)
7. Return `VmHandle { id, process, api_socket, vsock_uds: work_dir/vsock.sock, work_dir }`

`VmManager::destroy(vm: VmHandle) -> Result<()>`:
1. Kill the Firecracker process: `vm.process.kill().await`
2. Wait with timeout: `tokio::time::timeout(5s, vm.process.wait())`
3. Remove temp dir: `tokio::fs::remove_dir_all(vm.work_dir)`

`VmManager::is_running(vm: &VmHandle) -> bool`:
- Check `vm.process.try_wait()` — if `Ok(None)`, still running

**Implement `crates/agentbox-core/src/snapshot.rs`**

Helper for making Firecracker API calls over UDS:
- `async fn fc_api_call(socket: &Path, method: &str, path: &str, body: Value) -> Result<()>`
- Uses `hyperlocal` to connect to Unix domain socket
- Sends JSON body, checks response status

### Testing
- Unit test: VmManager config validation
- Unit test: temp dir creation and cleanup
- Integration test (needs KVM): create VM from snapshot, verify process running, destroy
- `cargo test -p agentbox-core -- vm`

### Out of scope
- Pool integration (that's Task E)
- Vsock communication (that's Task D)
- Network configuration for VMs

---

## Task D: Vsock Client (Host-Side)

**Wave 2 — Depends on: Task A (core types), Task B (guest agent protocol)**

### Context
Implements the host-side vsock client in `agentbox-core`. This communicates with
the guest agent inside each VM using the same length-prefixed JSON protocol defined
in Task B. Connects via Firecracker's vsock UDS path.

### Requirements
- Connect to guest agent via Firecracker vsock UDS
- Implement the vsock CONNECT handshake (Firecracker-specific)
- Ping guest agent
- Execute commands (blocking and streaming)
- File operations (read, write, list)
- Send signals to running processes

### Implementation

**Implement `crates/agentbox-core/src/vsock.rs`**

The Firecracker vsock protocol from the host side:
1. Connect to UDS at `vsock_uds_path`
2. Send `"CONNECT {port}\n"`
3. Read response, expect `"OK {cid}\n"`
4. Now bidirectional stream to guest agent

`VsockClient::new(uds_path: PathBuf, port: u32) -> Self`

`async fn connect(&self) -> Result<(OwnedReadHalf, OwnedWriteHalf)>`:
- `tokio::net::UnixStream::connect(&self.uds_path)`
- Write `CONNECT {port}\n`, read `OK` response
- Return split stream halves

`async fn request(&self, method: &str, params: Option<Value>) -> Result<Value>`:
- Connect, build `{ id, method, params }` JSON
- Write 4-byte big-endian length + JSON payload
- Read 4-byte length + JSON response
- Check for error field, return result field

`async fn ping(&self) -> Result<bool>`:
- `self.request("ping", None)` — check `result.status == "ok"`
- Wrap in timeout (configurable, default 5s)

`async fn exec(&self, command: &str, timeout: Duration) -> Result<ExecResult>`:
- `self.request("exec", Some(json!({ "command": command, "timeout": timeout.as_secs() })))`
- Parse result into `ExecResult { stdout, stderr, exit_code }`

`async fn exec_stream(&self, command: &str) -> Result<(Receiver<ExecEvent>, Sender<Vec<u8>>)>`:
- Connect, send `exec_stream` request
- Spawn read task: loop reading length-prefixed messages
  - `stream: "stdout"` → `ExecEvent::Stdout(base64_decode(data))`
  - `stream: "stderr"` → `ExecEvent::Stderr(base64_decode(data))`
  - `result` with `exit_code` → `ExecEvent::Exit(code)` → break
- Spawn write task: receive from stdin Sender, send as `stdin` messages
- Return (Receiver, Sender)

`async fn signal(&self, signal: i32) -> Result<()>`:
- `self.request("signal", Some(json!({ "signal": signal })))`

`async fn read_file(&self, path: &str) -> Result<Vec<u8>>`:
- Request `read_file`, base64 decode `result.content`

`async fn write_file(&self, path: &str, data: &[u8]) -> Result<()>`:
- Base64 encode data, request `write_file`

`async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>`:
- Request `list_files`, parse `result.entries`

### Testing
- Unit test: protocol encoding/decoding with in-memory streams
- Unit test: CONNECT handshake with mock server
- Integration test: connect to real guest agent in a Firecracker VM
- `cargo test -p agentbox-core -- vsock`

### Out of scope
- Reconnection logic (if connection drops, create a new one)
- Connection pooling (one connection per request is fine for now)
- Multiplexing multiple concurrent execs over one connection

---

## Task E: Pool + Sandbox Abstraction

**Wave 3 — Depends on: Task C (VmManager), Task D (VsockClient)**

### Context
Implements the warm VM pool and the high-level `Sandbox` API that ties together
VM creation, vsock communication, and lifecycle management. This is the main
public API of `agentbox-core` that the daemon and CLI use.

### Requirements
- Sandbox wraps VmHandle + VsockClient into a clean API
- Pool maintains warm pre-booted VMs
- Background replenishment task
- Claim/release lifecycle
- Idle timeout for warm VMs
- List active sandboxes

### Implementation

**Implement `crates/agentbox-core/src/sandbox.rs`**

Replace all `todo!()` with real implementations:

`Sandbox::new(vm: VmHandle, config: SandboxConfig, guest_config: GuestConfig) -> Self`:
- Build `VsockClient` from `vm.vsock_uds` and `guest_config.vsock_port`
- Store id from vm.id, store config

`Sandbox::exec()` — delegate to `self.vsock.exec()`
`Sandbox::exec_stream()` — delegate to `self.vsock.exec_stream()`, wrap in ExecStream
`Sandbox::send_stdin()` — delegate to ExecStream's stdin sender
`Sandbox::send_signal()` — delegate to `self.vsock.signal()`
`Sandbox::upload()` — delegate to `self.vsock.write_file()`
`Sandbox::download()` — delegate to `self.vsock.read_file()`
`Sandbox::list_files()` — delegate to `self.vsock.list_files()`
`Sandbox::is_alive()` — delegate to `self.vsock.ping()`
`Sandbox::destroy()` — takes ownership, consumes self

`Sandbox::into_vm(self) -> VmHandle` — for pool to destroy the underlying VM

**Implement `crates/agentbox-core/src/pool.rs`**

`Pool::new(config: PoolConfig, vm_manager: Arc<VmManager>, guest_config: GuestConfig)`:
- Init available VecDeque, active HashMap
- Store vm_manager and configs

`Pool::start(&self) -> Result<JoinHandle<()>>`:
- Spawn tokio task: replenishment loop
  - Every 1 second: check if `available.len() < config.min_size`
  - If yes: `vm_manager.create_from_snapshot()` → wait for ping → push to available
  - Also: check idle timeouts on available VMs, destroy expired ones
  - Also: check timeout on active VMs, destroy expired ones

`Pool::claim(&self, config: SandboxConfig) -> Result<Sandbox>`:
- Lock available, pop front
- If empty: try to create on-demand (if under max_size)
- If at max: return `Err(AgentBoxError::PoolExhausted)`
- Wait for guest agent ping (should already be up in warm VMs)
- Move to active map
- Return Sandbox wrapping the VM

`Pool::release(&self, sandbox: Sandbox) -> Result<()>`:
- Remove from active
- Destroy VM (Firecracker VMs are not recyclable — no clean reset)
- Replenishment loop will create a replacement

`Pool::list_active(&self) -> Vec<SandboxInfo>`:
- Read-lock active, return info for all entries

`Pool::shutdown(&self) -> Result<()>`:
- Destroy all available + active VMs

### Testing
- Unit test: Pool claim/release with mock VmManager
- Unit test: Pool exhaustion returns correct error
- Unit test: list_active returns correct info
- Integration test: Pool with real Firecracker (claim → exec → release)
- `cargo test -p agentbox-core -- pool sandbox`

### Out of scope
- Dynamic resource allocation (all VMs use default config for now)
- Sandbox persistence / checkpointing
- Pool metrics / observability

---

## Task F: Daemon HTTP API

**Wave 4 — Depends on: Task E (Pool + Sandbox)**

### Context
Implements the axum-based HTTP server that wraps `agentbox-core`. This is what the
SDKs talk to. REST endpoints for sandbox CRUD, command execution (non-streaming),
file operations, and health checks.

### Requirements
- axum server on configurable port (default 8080)
- All REST endpoints from the architecture doc
- JSON request/response bodies
- Proper error handling with HTTP status codes
- Graceful shutdown

### Implementation

**`crates/agentbox-daemon/Cargo.toml`**
```toml
[dependencies]
agentbox-core = { path = "../agentbox-core" }
axum = { version = "0.8", features = ["multipart"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
tower-http = { version = "0.6", features = ["cors", "trace"] }
```

**`crates/agentbox-daemon/src/main.rs`**
- Load config (from CLI arg or default path)
- Init tracing
- Create VmManager, Pool, start pool
- Build AppState, build router, start axum server
- Graceful shutdown on SIGTERM/SIGINT

**`crates/agentbox-daemon/src/state.rs`**
```rust
pub struct AppState {
    pub pool: Arc<Pool>,
    pub config: Arc<AgentBoxConfig>,
}
```

**`crates/agentbox-daemon/src/routes.rs`**
- Build axum Router with all routes
- Apply CORS middleware (permissive for dev)
- Apply tracing middleware

**`crates/agentbox-daemon/src/handlers.rs`**

`POST /sandboxes` → `create_sandbox`:
- Parse `CreateSandboxRequest { memory_mb, vcpus, network, timeout }`
- `pool.claim(config)` → return `{ id, status, created_at }`
- Error 503 if pool exhausted

`GET /sandboxes` → `list_sandboxes`:
- `pool.list_active()` → return array

`GET /sandboxes/{id}` → `get_sandbox`:
- Look up in active, return info or 404

`DELETE /sandboxes/{id}` → `destroy_sandbox`:
- `pool.release(sandbox)` → return `{ status: "destroyed" }`
- 404 if not found

`POST /sandboxes/{id}/exec` → `exec_command`:
- Parse `ExecRequest { command, timeout }`
- `sandbox.exec(command, timeout)` → return `ExecResult`
- 404 if sandbox not found

`POST /sandboxes/{id}/files` → `upload_file`:
- Multipart: file content + path field
- `sandbox.upload(content, path)` → return `{ path, size }`

`GET /sandboxes/{id}/files?path=...` → `download_file`:
- `sandbox.download(path)` → return binary with correct Content-Type

`GET /sandboxes/{id}/files?list&path=...` → `list_files`:
- `sandbox.list_files(path)` → return `[FileEntry]`

`GET /health` → `health_check`:
- Return `{ status: "ok", pool: { available, active, max } }`

### Testing
- Unit test: all handlers with mock pool (using axum test utilities)
- Integration test: start server, call endpoints with reqwest
- `cargo test -p agentbox-daemon`

### Out of scope
- WebSocket (that's Task G)
- Authentication
- Rate limiting

---

## Task G: WebSocket Exec Handler

**Wave 4 — Depends on: Task E (Pool + Sandbox)**

### Context
Adds WebSocket support to the daemon for streaming exec. This enables real-time
stdout/stderr streaming and interactive sessions (stdin + signals).

### Requirements
- WebSocket endpoint at `/sandboxes/{id}/ws`
- Bidirectional: client sends commands/stdin/signals, server sends stdout/stderr/exit
- Base64 encoding for binary data
- Multiple sequential commands per WebSocket connection
- Clean disconnect handling

### Implementation

**Add to `crates/agentbox-daemon/Cargo.toml`**:
```toml
axum = { version = "0.8", features = ["ws", "multipart"] }
```

**`crates/agentbox-daemon/src/ws.rs`**

`async fn ws_handler(ws: WebSocketUpgrade, state, sandbox_id) -> Response`:
- Look up sandbox in pool, 404 if not found
- Upgrade to WebSocket: `ws.on_upgrade(|socket| handle_ws(socket, sandbox))`

`async fn handle_ws(mut socket: WebSocket, sandbox: Arc<Sandbox>)`:
- Send `{"type": "ready"}` message
- Loop:
  - Select on:
    - `socket.recv()` → parse client message
    - `exec_rx.recv()` → forward exec event to client

  - Client message `{"type": "exec", "command": "...", "timeout": N}`:
    - Call `sandbox.exec_stream(command)`
    - Spawn task to forward ExecEvents → WebSocket messages
    - Track active exec state

  - Client message `{"type": "stdin", "data": "<base64>"}`:
    - Forward to active exec's stdin sender

  - Client message `{"type": "signal", "signal": N}`:
    - Forward to `sandbox.send_signal(N)`

  - ExecEvent::Stdout(data):
    - Send `{"type": "stdout", "data": "<base64>"}`

  - ExecEvent::Stderr(data):
    - Send `{"type": "stderr", "data": "<base64>"}`

  - ExecEvent::Exit(code):
    - Send `{"type": "exit", "code": N}`
    - Clean up active exec state

- On disconnect: clean up any active exec

**Add route to `routes.rs`**:
- `GET /sandboxes/{id}/ws` → `ws_handler`

### Testing
- Unit test: WebSocket message parsing/serialization
- Integration test: connect WebSocket, send exec, receive streaming output
- Integration test: stdin forwarding
- `cargo test -p agentbox-daemon -- ws`

### Out of scope
- PTY resize (future)
- Multiple concurrent execs per WebSocket
- Binary WebSocket frames (use base64 JSON for now)

---

## Task H: Python SDK

**Wave 5 — Depends on: Task F (HTTP API), Task G (WebSocket)**

### Context
The Python SDK is the primary integration surface. A thin HTTP/WebSocket client
that wraps the daemon API into a clean `Sandbox` class. Also ships pre-built
tool definitions for OpenAI and Anthropic function calling formats.

### Requirements
- `Sandbox.create()` / `.exec()` / `.upload()` / `.download()` / `.destroy()`
- Context manager support (`with Sandbox.create() as sb:`)
- Async variant (`AsyncSandbox`)
- Streaming exec via WebSocket
- Pre-built tool definitions for OpenAI and Anthropic formats
- `handle_tool_call()` helper for agent loop integration

### Implementation

**`sdks/python/pyproject.toml`**
```toml
[project]
name = "agentbox"
version = "0.1.0"
description = "Self-hosted sandbox infrastructure for AI agents"
requires-python = ">=3.10"
dependencies = [
    "httpx>=0.27",
    "pydantic>=2.0",
    "websockets>=12.0",
]

[project.optional-dependencies]
dev = ["pytest", "pytest-asyncio", "respx"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
```

**`sdks/python/agentbox/__init__.py`**
- Export: `Sandbox`, `AsyncSandbox`, `ExecResult`, `FileEntry`

**`sdks/python/agentbox/types.py`**
- `ExecResult(BaseModel)` — stdout, stderr, exit_code
- `FileEntry(BaseModel)` — name, size, is_dir
- `SandboxInfo(BaseModel)` — id, status, created_at, memory_mb, vcpus
- `CreateSandboxRequest(BaseModel)` — memory_mb, vcpus, network, timeout

**`sdks/python/agentbox/client.py`**
- `AgentBoxClient` class
  - `__init__(url: str = None)` — defaults to `AGENTBOX_URL` env or `http://localhost:8080`
  - Sync methods using `httpx.Client`: `get`, `post`, `delete`, `get_bytes`
  - `ws(path)` → WebSocket connection context manager

**`sdks/python/agentbox/sandbox.py`**
- Full `Sandbox` implementation as shown in the architecture doc
- `create()`, `exec()`, `exec_stream()`, `upload()`, `download()`, `list_files()`, `destroy()`
- `__enter__` / `__exit__` for context manager
- `tool_definitions(format)` and `handle_tool_call(tool_call)`

**`sdks/python/agentbox/tools.py`**
- `SANDBOX_TOOLS` list — execute_code, write_file, read_file schemas
- `get_tool_definitions(format, sandbox_id)` — format for openai/anthropic/generic
- `handle_tool_call(sandbox, tool_call)` — dispatch tool calls to sandbox methods

### Testing
- Unit test: `ExecResult`, `FileEntry` serialization
- Unit test: `get_tool_definitions()` returns correct format for each provider
- Unit test: `handle_tool_call()` dispatches correctly
- Unit test: `AgentBoxClient` with mocked HTTP (respx)
- Integration test: full flow against running daemon
- `cd sdks/python && uv run pytest`

### Out of scope
- AsyncSandbox (can add later, sync-first for simplicity)
- Retry logic / connection pooling
- Logging integration

---

## Task I: CLI (clap)

**Wave 5 — Depends on: Task E (core), Task F (HTTP API)**

### Context
The CLI binary provides management commands and the ability to start the daemon.
When `--url` is not specified, it detects if a local daemon is running and routes
accordingly.

### Requirements
- `agentbox serve` — start daemon (delegates to agentbox-daemon logic)
- `agentbox list` — list active sandboxes
- `agentbox exec <id> "command"` — run a command in a sandbox
- `agentbox stop <id>` — destroy a sandbox
- `agentbox status` — health check
- `agentbox version` — print version
- `--url` flag for remote daemon

### Implementation

**`crates/agentbox-cli/Cargo.toml`**
```toml
[dependencies]
agentbox-core = { path = "../agentbox-core" }
clap = { version = "4", features = ["derive"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
reqwest = { version = "0.12", features = ["json"] }
tabled = "0.17"    # Pretty table output
```

**`crates/agentbox-cli/src/main.rs`**
- Define top-level `Cli` struct with clap derive
- `--url` optional global flag
- Subcommands: Serve, List, Exec, Stop, Status, Version

**`crates/agentbox-cli/src/commands/serve.rs`**
- Load config, start daemon (reuse agentbox-daemon's startup logic)
- Alternatively: depend on `agentbox-daemon` as a library

**`crates/agentbox-cli/src/commands/list.rs`**
- GET `/sandboxes` → print table with id, status, created_at, memory, vcpus

**`crates/agentbox-cli/src/commands/exec.rs`**
- POST `/sandboxes/{id}/exec` → print stdout/stderr
- Or: WebSocket for streaming output to terminal

**`crates/agentbox-cli/src/commands/stop.rs`**
- DELETE `/sandboxes/{id}` → confirm destroyed

**`crates/agentbox-cli/src/commands/status.rs`**
- GET `/health` → print formatted status

**`crates/agentbox-cli/src/client.rs`**
- HTTP client wrapper using reqwest
- URL detection: check `--url`, else `AGENTBOX_URL`, else `http://localhost:8080`

### Testing
- Unit test: CLI argument parsing
- Integration test: CLI against running daemon
- `cargo test -p agentbox-cli`

### Out of scope
- Direct core library usage (always go through HTTP for MVP simplicity)
- Shell completions
- Interactive mode

---

## Task J: Build Pipeline (Artifacts Makefile)

**Wave 6 — Depends on: Task B (guest-agent binary)**

### Context
The Makefile builds all artifacts needed for Firecracker VMs: kernel, rootfs,
guest agent binary, and snapshot. These are built in CI and published as release
tarballs.

### Requirements
- Build minimal Linux kernel (vmlinux)
- Build Alpine rootfs with Python, Node.js, dev tools
- Cross-compile guest-agent for VM architecture
- Bake base snapshot (boot VM, wait for agent, snapshot)
- Support x86_64 and aarch64

### Implementation

**`artifacts/Makefile`**
```makefile
ARCH ?= $(shell uname -m)
OUTPUT = output/$(ARCH)

all: kernel rootfs guest-agent snapshot

kernel: $(OUTPUT)/vmlinux
rootfs: $(OUTPUT)/rootfs.ext4
guest-agent: $(OUTPUT)/guest-agent
snapshot: $(OUTPUT)/snapshot/vmstate.bin

# ... targets for each step
```

**`artifacts/kernel/build.sh`**
- Download Linux kernel source (6.x)
- Apply minimal config for Firecracker (virtio, ext4, vsock, no modules)
- Build vmlinux
- Copy to `output/{arch}/vmlinux`

**`artifacts/rootfs/build.sh`**
- Create ext4 image (512MB)
- Bootstrap Alpine Linux minimal
- Install packages: python3, nodejs, npm, git, ripgrep, jq, curl, build-base
- Copy guest-agent binary to `/usr/local/bin/guest-agent`
- Copy OpenRC init script for guest-agent (auto-start on boot)
- Create `/workspace` directory (working directory for sandboxes)

**`artifacts/rootfs/overlay/etc/init.d/guest-agent`**
- OpenRC init script that starts guest-agent on boot

**`artifacts/snapshot/bake.sh`**
- Start Firecracker with kernel + rootfs (fresh boot, no snapshot)
- Wait for guest-agent to respond to ping via vsock
- Pause the VM
- Take snapshot (vmstate.bin + memory.bin via Firecracker API)
- Stop Firecracker
- Output to `output/{arch}/snapshot/`

### Testing
- Test that `make all` completes without errors on a KVM machine
- Test that resulting snapshot can be restored and guest-agent pings
- Tested in CI (requires KVM runner)

### Out of scope
- Custom rootfs images (one standard image for MVP)
- Kernel modules
- Optimized kernel config per architecture

---

## Task K: Install Script + CI

**Wave 6 — Depends on: Task J (artifacts), all binary crates**

### Context
The install script (`install.sh`) and GitHub Actions CI pipeline. The CI builds
all binaries and artifacts on every tagged release, publishes to GitHub Releases.
The install script downloads the latest release.

### Requirements
- `install.sh` that works on any KVM-enabled Linux machine
- GitHub Actions workflow for CI (test on every PR)
- GitHub Actions workflow for releases (build + publish on tags)
- Pre-built binaries for x86_64 and aarch64

### Implementation

**`install.sh`** — as designed in the architecture doc:
- Detect arch, verify KVM, download binary + artifacts, install systemd service
- Idempotent, clear error messages

**`.github/workflows/ci.yml`**
- Trigger: push to main, PRs
- Steps: cargo check, cargo test, cargo clippy, cargo fmt --check
- Matrix: x86_64 (standard runner), aarch64 (if available)
- Note: integration tests skipped in CI without KVM

**`.github/workflows/release.yml`**
- Trigger: push tag `v*`
- Steps:
  1. Build `agentbox-cli` and `agentbox-daemon` for linux-x86_64 and linux-aarch64
     (use `cross` for cross-compilation)
  2. Build `guest-agent` for both architectures
  3. On a KVM-enabled runner: run `make all` in artifacts/ to build kernel + rootfs + snapshot
  4. Package: `agentbox-linux-{arch}` binary + `agentbox-artifacts-{arch}.tar.gz`
  5. Create GitHub Release, upload all assets
  6. Publish Python SDK to PyPI
  7. Publish TypeScript SDK to npm

### Testing
- Test install.sh on a clean Ubuntu VM (manual or CI)
- Verify release artifacts can be downloaded and installed
- Verify systemd service starts correctly

### Out of scope
- Homebrew formula
- Docker image distribution
- Automatic update mechanism
