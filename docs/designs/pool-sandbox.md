# Pool + Sandbox Abstraction

## Context

Implements the warm VM pool and the high-level `Sandbox` API that ties together
VM creation, vsock communication, and lifecycle management. This is the main
public API of `agentbox-core` that the daemon and CLI use.

This task assumes the following exist from prior tasks:
- `crates/agentbox-core/src/vm.rs` — VmManager with create/destroy (FAU-69)
- `crates/agentbox-core/src/vsock.rs` — VsockClient with exec/files/ping (FAU-70)
- `crates/agentbox-core/src/sandbox.rs` — Sandbox type stubs (FAU-67)
- `crates/agentbox-core/src/pool.rs` — Pool type stubs (FAU-67)

AgentBox is a self-hosted sandbox infrastructure for AI agents.
See `docs/spec.md` and `docs/architecture.md` for full context.

## Requirements

- Sandbox wraps VmHandle + VsockClient into a clean high-level API
- Pool maintains warm pre-booted VMs for fast claim
- Background replenishment task keeps pool at min_size
- Claim/release lifecycle with proper state tracking
- Idle timeout for warm VMs in the pool
- List active sandboxes
- Graceful shutdown (destroy all VMs)

## Implementation

### Implement `crates/agentbox-core/src/sandbox.rs`

Replace all `todo!()` stubs. The Sandbox struct fields should be:
```rust
pub struct Sandbox {
    id: SandboxId,
    vm: crate::vm::VmHandle,
    vsock: crate::vsock::VsockClient,
    config: SandboxConfig,
    created_at: std::time::Instant,
}
```

**Constructor (crate-visible, called by Pool):**
```rust
impl Sandbox {
    pub(crate) fn new(
        vm: crate::vm::VmHandle,
        config: SandboxConfig,
        guest_config: &crate::config::GuestConfig,
    ) -> Self {
        let id = SandboxId(vm.id.clone());
        let vsock = crate::vsock::VsockClient::new(
            vm.vsock_uds.clone(),
            guest_config.vsock_port,
        );
        Self { id, vm, vsock, config, created_at: std::time::Instant::now() }
    }
}
```

**All public methods — delegate to vsock:**

```rust
impl Sandbox {
    pub fn id(&self) -> &SandboxId { &self.id }

    pub fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            status: SandboxStatus::Ready,
            config: self.config.clone(),
            created_at: format!("{:?}", self.created_at),
        }
    }

    pub async fn exec(&self, command: &str, timeout: Duration) -> Result<ExecResult> {
        self.vsock.exec(command, timeout).await
    }

    pub async fn exec_stream(&self, command: &str) -> Result<(
        tokio::sync::mpsc::Receiver<ExecEvent>,
        tokio::sync::mpsc::Sender<Vec<u8>>,
    )> {
        self.vsock.exec_stream(command).await
    }

    pub async fn send_signal(&self, signal: i32) -> Result<()> {
        self.vsock.signal(signal).await
    }

    pub async fn upload(&self, content: &[u8], remote_path: &str) -> Result<()> {
        self.vsock.write_file(remote_path, content).await
    }

    pub async fn download(&self, remote_path: &str) -> Result<Vec<u8>> {
        self.vsock.read_file(remote_path).await
    }

    pub async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
        self.vsock.list_files(path).await
    }

    pub async fn is_alive(&self) -> bool {
        self.vsock.ping().await.unwrap_or(false)
    }

    /// Consume the sandbox, returning the VM handle for destruction.
    pub(crate) fn into_vm(self) -> crate::vm::VmHandle {
        self.vm
    }
}
```

Note: `destroy` is NOT on Sandbox directly — the Pool handles destruction to
maintain consistent state tracking.

### Implement `crates/agentbox-core/src/pool.rs`

Replace all `todo!()` stubs.

```rust
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

use crate::config::{GuestConfig, PoolConfig};
use crate::error::{AgentBoxError, Result};
use crate::sandbox::*;
use crate::vm::VmManager;

struct PooledSandbox {
    sandbox: Sandbox,
    pooled_at: Instant,
}

pub struct Pool {
    config: PoolConfig,
    guest_config: GuestConfig,
    vm_manager: Arc<VmManager>,
    available: Arc<Mutex<VecDeque<PooledSandbox>>>,
    active: Arc<RwLock<HashMap<SandboxId, SandboxInfo>>>,
}

impl Pool {
    pub fn new(
        config: PoolConfig,
        guest_config: GuestConfig,
        vm_manager: Arc<VmManager>,
    ) -> Self {
        Self {
            config,
            guest_config,
            vm_manager,
            available: Arc::new(Mutex::new(VecDeque::new())),
            active: Arc::new(RwLock::new(HashMap::new())),
        }
    }
```

**`start(&self) -> Result<JoinHandle<()>>`** — spawn replenishment loop:
```rust
pub async fn start(&self) -> Result<tokio::task::JoinHandle<()>> {
    // Initial fill
    self.replenish().await;

    let available = self.available.clone();
    let vm_manager = self.vm_manager.clone();
    let config = self.config.clone();
    let guest_config = self.guest_config.clone();
    let active = self.active.clone();

    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            // Remove expired idle VMs from available pool
            {
                let idle_timeout = Duration::from_secs(config.idle_timeout_secs);
                let mut avail = available.lock().await;
                let mut expired = Vec::new();
                avail.retain(|ps| {
                    if ps.pooled_at.elapsed() > idle_timeout {
                        expired.push(ps); // can't move out of retain... see below
                        false
                    } else {
                        true
                    }
                });
                // Actually: use drain_filter or manual index loop to extract expired
            }

            // Replenish if below min_size
            let current_available = available.lock().await.len();
            let current_active = active.read().await.len();
            let total = current_available + current_active;

            if current_available < config.min_size && total < config.max_size {
                let defaults = SandboxConfig {
                    memory_mb: 2048, vcpus: 2, network: false, timeout_secs: 3600,
                };
                match vm_manager.create_from_snapshot(&defaults).await {
                    Ok(vm) => {
                        let sandbox = Sandbox::new(vm, defaults, &guest_config);
                        // Wait for guest agent to be ready
                        let timeout = Duration::from_millis(guest_config.ping_timeout_ms);
                        let ready = tokio::time::timeout(timeout, async {
                            loop {
                                if sandbox.is_alive().await { return true; }
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }).await.unwrap_or(false);

                        if ready {
                            available.lock().await.push_back(PooledSandbox {
                                sandbox,
                                pooled_at: Instant::now(),
                            });
                            tracing::debug!("Pool replenished: +1 VM");
                        } else {
                            tracing::warn!("New VM guest agent not ready, discarding");
                            let vm = sandbox.into_vm();
                            let _ = vm_manager.destroy(vm).await;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Pool replenishment failed: {e}");
                    }
                }
            }
        }
    });

    Ok(handle)
}
```

Note: The idle VM cleanup inside the replenishment loop needs careful handling.
Use a manual loop with index tracking to extract expired sandboxes, then destroy
their VMs outside the lock. Example:
```rust
let mut avail = available.lock().await;
let mut i = 0;
let mut to_destroy = Vec::new();
while i < avail.len() {
    if avail[i].pooled_at.elapsed() > idle_timeout {
        let ps = avail.remove(i).unwrap();
        to_destroy.push(ps.sandbox);
    } else {
        i += 1;
    }
}
drop(avail); // release lock before destroying
for sb in to_destroy {
    let _ = vm_manager.destroy(sb.into_vm()).await;
}
```

**`claim(&self, config: SandboxConfig) -> Result<Sandbox>`**:
```rust
pub async fn claim(&self, config: SandboxConfig) -> Result<Sandbox> {
    // Fast path: grab from pool
    let sandbox = {
        let mut avail = self.available.lock().await;
        avail.pop_front().map(|ps| ps.sandbox)
    };

    let sandbox = if let Some(sb) = sandbox {
        sb
    } else {
        // Pool empty — create on demand if under max
        let active_count = self.active.read().await.len();
        let avail_count = self.available.lock().await.len();
        if active_count + avail_count >= self.config.max_size {
            return Err(AgentBoxError::PoolExhausted);
        }
        let vm = self.vm_manager.create_from_snapshot(&config).await?;
        let sb = Sandbox::new(vm, config.clone(), &self.guest_config);
        // Wait for ready
        let timeout = Duration::from_millis(self.guest_config.ping_timeout_ms);
        let ready = tokio::time::timeout(timeout, async {
            loop {
                if sb.is_alive().await { return true; }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await.unwrap_or(false);
        if !ready {
            let vm = sb.into_vm();
            let _ = self.vm_manager.destroy(vm).await;
            return Err(AgentBoxError::Timeout("Guest agent not ready".into()));
        }
        sb
    };

    // Track as active
    let info = sandbox.info();
    self.active.write().await.insert(sandbox.id().clone(), info);

    Ok(sandbox)
}
```

**`release(&self, sandbox: Sandbox) -> Result<()>`**:
```rust
pub async fn release(&self, sandbox: Sandbox) -> Result<()> {
    let id = sandbox.id().clone();
    self.active.write().await.remove(&id);
    let vm = sandbox.into_vm();
    self.vm_manager.destroy(vm).await?;
    tracing::info!(sandbox_id = %id.0, "Sandbox released and destroyed");
    Ok(())
}
```

**`list_active(&self) -> Vec<SandboxInfo>`**:
```rust
pub fn list_active(&self) -> Vec<SandboxInfo> {
    // Use try_read to avoid async — or make this async
    // For simplicity, make it async:
    todo!("make this async or use blocking read")
}
// Better: pub async fn list_active(&self) -> Vec<SandboxInfo>
pub async fn list_active(&self) -> Vec<SandboxInfo> {
    self.active.read().await.values().cloned().collect()
}
```
Note: Update the method signature in the stub to be `async`.

**`shutdown(&self) -> Result<()>`**:
```rust
pub async fn shutdown(&self) -> Result<()> {
    // Destroy all available VMs
    let available: Vec<_> = {
        let mut avail = self.available.lock().await;
        avail.drain(..).map(|ps| ps.sandbox).collect()
    };
    for sb in available {
        let _ = self.vm_manager.destroy(sb.into_vm()).await;
    }

    // Active sandboxes can't be drained (they're in use),
    // but we log a warning
    let active_count = self.active.read().await.len();
    if active_count > 0 {
        tracing::warn!("{active_count} sandboxes still active during shutdown");
    }

    Ok(())
}
```

**Add `pool_status` method for health endpoint:**
```rust
pub async fn pool_status(&self) -> PoolStatus {
    PoolStatus {
        available: self.available.lock().await.len(),
        active: self.active.read().await.len(),
        max: self.config.max_size,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolStatus {
    pub available: usize,
    pub active: usize,
    pub max: usize,
}
```

### Update `crates/agentbox-core/src/lib.rs`

Make sure the `Pool` and `Sandbox` are properly re-exported:
```rust
pub use pool::{Pool, PoolStatus};
pub use sandbox::{Sandbox, SandboxConfig, SandboxId, SandboxInfo, SandboxStatus, ExecResult, ExecEvent, FileEntry};
pub use vm::VmManager;
pub use config::AgentBoxConfig;
pub use error::{AgentBoxError, Result};
```

## Testing Strategy

Run tests: `cargo test -p agentbox-core -- pool sandbox`

### Unit tests for Pool (with mock VmManager):

To test the pool without Firecracker, create a `MockVmManager` that returns
fake VmHandles. This requires either:
1. Making VmManager a trait (preferred for testability)
2. Or: using `cfg(test)` mock implementations

**Option 1 (recommended): Extract trait**

In `crates/agentbox-core/src/vm.rs`, add:
```rust
#[async_trait::async_trait]
pub trait VmBackend: Send + Sync {
    async fn create_from_snapshot(&self, config: &SandboxConfig) -> Result<VmHandle>;
    async fn destroy(&self, vm: VmHandle) -> Result<()>;
}
```

Add `async-trait = "0.1"` to Cargo.toml. Make Pool generic over `VmBackend` or
use `Arc<dyn VmBackend>`. Then implement `VmBackend` for `VmManager`.

If this is too much refactoring, test with `#[ignore]` integration tests instead.

### Test cases:

- `test_pool_new` — create pool, verify initial state
- `test_pool_claim_from_empty` — claim when pool is empty, verify on-demand creation
  (needs mock or real Firecracker)
- `test_pool_exhausted` — set max_size=0, claim, verify PoolExhausted error
- `test_pool_release` — release sandbox, verify removed from active
- `test_list_active` — claim a sandbox, list_active returns it
- `test_pool_shutdown` — add VMs, shutdown, verify all destroyed

### Integration tests (need KVM):
- `test_pool_e2e` — start pool, claim sandbox, exec "echo hello", release
- Mark with `#[ignore]` or feature flag

## Out of Scope

- Dynamic resource allocation per sandbox (all use defaults from pool config)
- Sandbox checkpointing / persistence
- Pool metrics / prometheus export
- Automatic sandbox timeout (active sandboxes that exceed timeout_secs)
