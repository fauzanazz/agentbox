# Project Scaffold + Core Types

## Context

Bootstrap the AgentBox Cargo workspace with all 4 crates and define the core type
system. This is the foundation task — downstream tasks compile against these types.

AgentBox is a self-hosted sandbox infrastructure for AI agents (like E2B but
self-hosted). See `docs/spec.md` and `docs/architecture.md` for full context.

## Requirements

- Cargo workspace with 4 crates: agentbox-core, agentbox-daemon, agentbox-cli, guest-agent
- All core types, error types, and config types defined in agentbox-core
- Stub implementations (`todo!()`) for all public methods
- Project compiles with `cargo check`

## Implementation

### Files to create

**`Cargo.toml`** (workspace root — replace existing if any)
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
repository = "https://github.com/fauzanazz/agentbox"

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

**`crates/agentbox-core/src/error.rs`** — Define `AgentBoxError` enum with variants:
- `VmCreation(String)`, `VmNotFound(String)`, `VsockConnection(String)`
- `ExecFailed(String)`, `FileOp(String)`, `PoolExhausted`
- `SnapshotLoad(String)`, `Timeout(String)`, `Config(String)`
- `Io(#[from] std::io::Error)`
- Define `pub type Result<T> = std::result::Result<T, AgentBoxError>;`

**`crates/agentbox-core/src/config.rs`** — Configuration types:
```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct AgentBoxConfig {
    pub daemon: DaemonConfig,
    pub vm: VmConfig,
    pub pool: PoolConfig,
    pub guest: GuestConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DaemonConfig {
    pub listen: String,        // default "127.0.0.1:8080"
    pub log_level: String,     // default "info"
}

#[derive(Debug, Deserialize, Clone)]
pub struct VmConfig {
    pub firecracker_bin: PathBuf,
    pub kernel_path: PathBuf,
    pub rootfs_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub defaults: VmDefaults,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VmDefaults {
    pub memory_mb: u32,        // default 2048
    pub vcpus: u32,            // default 2
    pub network: bool,         // default false
    pub timeout_secs: u64,     // default 3600
}

#[derive(Debug, Deserialize, Clone)]
pub struct PoolConfig {
    pub min_size: usize,       // default 2
    pub max_size: usize,       // default 10
    pub idle_timeout_secs: u64, // default 3600
}

#[derive(Debug, Deserialize, Clone)]
pub struct GuestConfig {
    pub vsock_port: u32,       // default 5000
    pub ping_timeout_ms: u64,  // default 5000
}

impl AgentBoxConfig {
    pub fn from_file(path: &std::path::Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| crate::error::AgentBoxError::Config(e.to_string()))
    }
}
```
Implement `Default` for all config types with the default values shown in comments.

**`crates/agentbox-core/src/sandbox.rs`** — Sandbox types:
```rust
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub memory_mb: u32,
    pub vcpus: u32,
    pub network: bool,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub id: SandboxId,
    pub status: SandboxStatus,
    pub config: SandboxConfig,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxStatus { Creating, Ready, Busy, Destroying }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug)]
pub enum ExecEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}
```

Define `Sandbox` struct with fields: `id: SandboxId`, `vm: crate::vm::VmHandle`, `vsock: crate::vsock::VsockClient`, `config: SandboxConfig`.

Implement all methods on Sandbox with `todo!()`:
- `pub async fn exec(&self, command: &str, timeout: Duration) -> crate::error::Result<ExecResult>`
- `pub async fn exec_stream(&self, command: &str) -> crate::error::Result<(mpsc::Receiver<ExecEvent>, mpsc::Sender<Vec<u8>>)>`
- `pub async fn send_signal(&self, signal: i32) -> crate::error::Result<()>`
- `pub async fn upload(&self, content: &[u8], remote_path: &str) -> crate::error::Result<()>`
- `pub async fn download(&self, remote_path: &str) -> crate::error::Result<Vec<u8>>`
- `pub async fn list_files(&self, path: &str) -> crate::error::Result<Vec<FileEntry>>`
- `pub async fn is_alive(&self) -> bool`
- `pub async fn destroy(self) -> crate::error::Result<()>`
- `pub fn id(&self) -> &SandboxId`
- `pub fn info(&self) -> SandboxInfo`

**`crates/agentbox-core/src/vm.rs`**:
```rust
use std::path::PathBuf;
use tokio::process::Child;

pub struct VmHandle {
    pub id: String,
    pub process: Child,
    pub api_socket: PathBuf,
    pub vsock_uds: PathBuf,
    pub work_dir: PathBuf,
}

pub struct VmManager {
    pub(crate) config: crate::config::VmConfig,
}

impl VmManager {
    pub fn new(config: crate::config::VmConfig) -> Self { Self { config } }
    pub async fn create_from_snapshot(&self, config: &crate::sandbox::SandboxConfig) -> crate::error::Result<VmHandle> { todo!() }
    pub async fn destroy(&self, vm: VmHandle) -> crate::error::Result<()> { todo!() }
    pub fn is_running(vm: &VmHandle) -> bool { todo!() }
}
```

**`crates/agentbox-core/src/pool.rs`**:
```rust
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use crate::{vm::VmManager, sandbox::*, config::{PoolConfig, GuestConfig}};

pub struct Pool {
    config: PoolConfig,
    guest_config: GuestConfig,
    vm_manager: Arc<VmManager>,
    available: Arc<Mutex<VecDeque<Sandbox>>>,
    active: Arc<RwLock<HashMap<SandboxId, SandboxInfo>>>,
}

impl Pool {
    pub fn new(config: PoolConfig, guest_config: GuestConfig, vm_manager: Arc<VmManager>) -> Self { todo!() }
    pub async fn start(&self) -> crate::error::Result<tokio::task::JoinHandle<()>> { todo!() }
    pub async fn claim(&self, config: SandboxConfig) -> crate::error::Result<Sandbox> { todo!() }
    pub async fn release(&self, sandbox: Sandbox) -> crate::error::Result<()> { todo!() }
    pub fn list_active(&self) -> Vec<SandboxInfo> { todo!() }
    pub async fn shutdown(&self) -> crate::error::Result<()> { todo!() }
}
```

**`crates/agentbox-core/src/vsock.rs`**:
```rust
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use crate::sandbox::{ExecResult, ExecEvent, FileEntry};

pub struct VsockClient {
    pub(crate) uds_path: PathBuf,
    pub(crate) port: u32,
}

impl VsockClient {
    pub fn new(uds_path: PathBuf, port: u32) -> Self { Self { uds_path, port } }
    pub async fn ping(&self) -> crate::error::Result<bool> { todo!() }
    pub async fn exec(&self, command: &str, timeout: Duration) -> crate::error::Result<ExecResult> { todo!() }
    pub async fn exec_stream(&self, command: &str) -> crate::error::Result<(mpsc::Receiver<ExecEvent>, mpsc::Sender<Vec<u8>>)> { todo!() }
    pub async fn signal(&self, signal: i32) -> crate::error::Result<()> { todo!() }
    pub async fn read_file(&self, path: &str) -> crate::error::Result<Vec<u8>> { todo!() }
    pub async fn write_file(&self, path: &str, data: &[u8]) -> crate::error::Result<()> { todo!() }
    pub async fn list_files(&self, path: &str) -> crate::error::Result<Vec<FileEntry>> { todo!() }
}
```

**`crates/agentbox-core/src/snapshot.rs`**:
```rust
use std::path::PathBuf;

pub struct SnapshotManager {
    pub(crate) snapshot_path: PathBuf,
}

impl SnapshotManager {
    pub fn new(snapshot_path: PathBuf) -> Self { Self { snapshot_path } }
    pub async fn load(&self, api_socket: &std::path::Path) -> crate::error::Result<()> { todo!() }
}
```

**`crates/agentbox-daemon/Cargo.toml`**:
```toml
[package]
name = "agentbox-daemon"
version.workspace = true
edition.workspace = true

[dependencies]
agentbox-core = { path = "../agentbox-core" }
axum = { version = "0.8", features = ["ws", "multipart"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
tower-http = { version = "0.6", features = ["cors", "trace"] }
```

**`crates/agentbox-daemon/src/main.rs`** — `fn main() { println!("agentbox-daemon: not yet implemented"); }`

**`crates/agentbox-cli/Cargo.toml`**:
```toml
[package]
name = "agentbox-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "agentbox"
path = "src/main.rs"

[dependencies]
agentbox-core = { path = "../agentbox-core" }
clap = { version = "4", features = ["derive"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
reqwest = { version = "0.12", features = ["json"] }
```

**`crates/agentbox-cli/src/main.rs`** — `fn main() { println!("agentbox: not yet implemented"); }`

**`crates/guest-agent/Cargo.toml`**:
```toml
[package]
name = "guest-agent"
version.workspace = true
edition.workspace = true

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
nix = { version = "0.29", features = ["process", "signal", "pty", "fs"] }
base64 = "0.22"
uuid = { workspace = true }
```

**`crates/guest-agent/src/main.rs`** — `fn main() { println!("guest-agent: not yet implemented"); }`

**Also create:**
- `.gitignore` with: `target/`, `*.swp`, `.env`, `artifacts/output/`
- `config.example.toml` — example config matching all config types with default values

## Testing Strategy

- `cargo check` must pass for all workspace members
- `cargo test` must pass (no tests exist yet, but zero compile errors)
- Run `cargo check` at the end to verify

## Out of Scope

- No implementations — only type definitions and `todo!()` stubs
- No guest-agent logic (Task B)
- No actual Firecracker interaction (Task C)
- No SDK code
