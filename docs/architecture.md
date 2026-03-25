# AgentBox — Architecture Plan

## Overview

AgentBox is a Rust Cargo workspace with 4 crates, 2 SDK packages, a build pipeline
for VM artifacts, and a `curl|sh` installer. The architecture follows a library-first
design: all VM management logic lives in `agentbox-core`, and both the daemon and CLI
are thin wrappers.

```
┌─────────────────────────────────────────────────────────────────┐
│                     Developer's AI App                          │
│  (Python/TS agent loop using any LLM)                          │
└───────────────┬─────────────────────────────────────────────────┘
                │  SDK (HTTP + WebSocket)
                ▼
┌─────────────────────────────────────────────────────────────────┐
│  agentbox-daemon  (axum HTTP/WS server)                        │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  agentbox-core  (library)                                 │  │
│  │  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌────────────┐  │  │
│  │  │ Sandbox  │ │   Pool   │ │ VmManager │ │VsockClient │  │  │
│  │  └──────────┘ └──────────┘ └───────────┘ └────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
└──────────────────────────┬──────────────────────────────────────┘
                           │  Firecracker API (UDS) + vsock
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│  Firecracker microVM                                            │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  guest-agent  (Rust, vsock server)                        │  │
│  │  ┌──────┐ ┌───────┐ ┌───────┐ ┌──────────┐               │  │
│  │  │ Exec │ │  PTY  │ │ Files │ │ Signals  │               │  │
│  │  └──────┘ └───────┘ └───────┘ └──────────┘               │  │
│  └───────────────────────────────────────────────────────────┘  │
│  Alpine Linux · Python · Node.js · git · ripgrep · jq          │
└─────────────────────────────────────────────────────────────────┘
```

## Repository Structure

```
agentbox/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── config.example.toml           # Example daemon config
├── install.sh                    # The curl|sh installer
├── README.md
├── LICENSE                       # Apache-2.0
│
├── crates/
│   ├── agentbox-core/            # Library: all VM logic
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # Public API re-exports
│   │       ├── sandbox.rs        # Sandbox high-level abstraction
│   │       ├── vm.rs             # Firecracker VM lifecycle
│   │       ├── pool.rs           # Warm VM pool
│   │       ├── vsock.rs          # Vsock client (host → guest)
│   │       ├── snapshot.rs       # Snapshot load/restore
│   │       ├── config.rs         # Configuration types + TOML parsing
│   │       └── error.rs          # Error types (thiserror)
│   │
│   ├── agentbox-daemon/          # Binary: HTTP/WS daemon
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # Entry point, signal handling
│   │       ├── state.rs          # AppState (wraps core Pool + config)
│   │       ├── routes.rs         # axum router setup
│   │       ├── handlers.rs       # HTTP endpoint handlers
│   │       └── ws.rs             # WebSocket exec handler
│   │
│   ├── agentbox-cli/             # Binary: management CLI
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # clap entry point
│   │       ├── commands/
│   │       │   ├── mod.rs
│   │       │   ├── serve.rs      # Start daemon (embeds daemon logic)
│   │       │   ├── list.rs       # List active sandboxes
│   │       │   ├── exec.rs       # Execute command in sandbox
│   │       │   ├── stop.rs       # Destroy sandbox
│   │       │   ├── logs.rs       # Tail sandbox logs
│   │       │   └── status.rs     # Daemon health + pool stats
│   │       └── client.rs         # HTTP/WS client for remote daemon
│   │
│   └── guest-agent/              # Binary: runs inside each VM
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # Entry point, vsock listen
│           ├── server.rs         # Request dispatcher
│           ├── exec.rs           # Command execution + PTY
│           ├── files.rs          # File operations
│           └── protocol.rs       # Length-prefixed JSON codec
│
├── sdks/
│   ├── python/
│   │   ├── pyproject.toml        # uv/pip installable
│   │   ├── agentbox/
│   │   │   ├── __init__.py       # from agentbox import Sandbox
│   │   │   ├── sandbox.py        # Sandbox class (main API)
│   │   │   ├── client.py         # HTTP + WebSocket client
│   │   │   ├── tools.py          # Pre-built LLM tool definitions
│   │   │   └── types.py          # Pydantic models (ExecResult, etc.)
│   │   └── tests/
│   │       ├── test_sandbox.py
│   │       └── test_tools.py
│   │
│   └── typescript/
│       ├── package.json
│       ├── tsconfig.json
│       ├── src/
│       │   ├── index.ts          # export { Sandbox }
│       │   ├── sandbox.ts        # Sandbox class
│       │   ├── client.ts         # HTTP + WebSocket client
│       │   ├── tools.ts          # Pre-built LLM tool definitions
│       │   └── types.ts          # TypeScript interfaces
│       └── tests/
│           ├── sandbox.test.ts
│           └── tools.test.ts
│
├── artifacts/
│   ├── Makefile                  # kernel → rootfs → guest-agent → snapshot
│   ├── kernel/
│   │   ├── config-x86_64        # Minimal kernel config
│   │   ├── config-aarch64
│   │   └── build.sh             # Download + compile vmlinux
│   ├── rootfs/
│   │   ├── build.sh             # Alpine rootfs builder
│   │   └── overlay/             # Files to copy into rootfs
│   │       ├── etc/
│   │       │   └── init.d/
│   │       │       └── guest-agent  # OpenRC init script
│   │       └── usr/local/bin/   # Guest agent binary goes here
│   └── snapshot/
│       └── bake.sh              # Boot VM, wait for agent, snapshot
│
└── .github/
    └── workflows/
        ├── ci.yml               # Test on every PR
        └── release.yml          # Build binaries + artifacts on tag
```

## Crate Design

### agentbox-core (library)

This is the heart of the project. Every other component depends on it.

#### Key Types

```rust
// === sandbox.rs — The main user-facing type ===

pub struct Sandbox {
    id: SandboxId,
    vm: VmHandle,
    vsock: VsockClient,
    config: SandboxConfig,
}

pub struct SandboxConfig {
    pub memory_mb: u32,        // default: 2048
    pub vcpus: u32,            // default: 2
    pub network: bool,         // default: false
    pub timeout_secs: u64,     // default: 3600 (1 hour)
}

pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

impl Sandbox {
    /// Execute a command. Returns result after completion.
    pub async fn exec(&self, command: &str, timeout: Duration) -> Result<ExecResult>;

    /// Execute a command with streaming output via channel.
    pub async fn exec_stream(&self, command: &str) -> Result<ExecStream>;

    /// Send data to stdin of a running exec.
    pub async fn send_stdin(&self, data: &[u8]) -> Result<()>;

    /// Send a signal to the running process.
    pub async fn send_signal(&self, signal: i32) -> Result<()>;

    /// Upload a file into the sandbox.
    pub async fn upload(&self, content: &[u8], remote_path: &str) -> Result<()>;

    /// Download a file from the sandbox.
    pub async fn download(&self, remote_path: &str) -> Result<Vec<u8>>;

    /// List files at a path.
    pub async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;

    /// Destroy the sandbox and its VM.
    pub async fn destroy(self) -> Result<()>;

    /// Check if sandbox is still alive.
    pub async fn is_alive(&self) -> bool;
}

/// Streaming exec output
pub struct ExecStream {
    rx: tokio::sync::mpsc::Receiver<ExecEvent>,
    tx_stdin: tokio::sync::mpsc::Sender<Vec<u8>>,
}

pub enum ExecEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
    Error(String),
}
```

```rust
// === pool.rs — Warm VM pool ===

pub struct Pool {
    config: PoolConfig,
    vm_manager: Arc<VmManager>,
    /// Ready-to-claim sandboxes
    available: Arc<Mutex<VecDeque<Sandbox>>>,
    /// Currently in-use sandboxes
    active: Arc<RwLock<HashMap<SandboxId, SandboxInfo>>>,
}

pub struct PoolConfig {
    pub min_size: usize,       // default: 2
    pub max_size: usize,       // default: 10
    pub idle_timeout: Duration, // default: 1 hour
}

impl Pool {
    pub fn new(config: PoolConfig, vm_manager: VmManager) -> Self;

    /// Start background replenishment task.
    pub async fn start(&self) -> Result<()>;

    /// Claim a sandbox from the pool. Fast path: grab from available queue.
    pub async fn claim(&self, config: SandboxConfig) -> Result<Sandbox>;

    /// Release a sandbox. Destroys the VM (Firecracker VMs are not recyclable).
    pub async fn release(&self, sandbox: Sandbox) -> Result<()>;

    /// List all active sandboxes.
    pub fn list_active(&self) -> Vec<SandboxInfo>;

    /// Shutdown: destroy all VMs.
    pub async fn shutdown(&self) -> Result<()>;
}
```

```rust
// === vm.rs — Firecracker VM lifecycle ===

pub struct VmManager {
    config: VmConfig,
}

pub struct VmConfig {
    pub firecracker_bin: PathBuf,
    pub snapshot_path: PathBuf,    // directory with vmstate.bin + memory.bin
    pub rootfs_path: PathBuf,      // base rootfs.ext4
    pub kernel_path: PathBuf,      // vmlinux (for non-snapshot boot)
}

pub struct VmHandle {
    pub id: String,
    pub process: tokio::process::Child,
    pub api_socket: PathBuf,
    pub vsock_uds: PathBuf,
    pub work_dir: PathBuf,         // temp dir for this VM's files
}

impl VmManager {
    /// Create a VM from snapshot (fast path — <300ms).
    pub async fn create_from_snapshot(&self, config: &SandboxConfig) -> Result<VmHandle>;

    /// Destroy a VM (kill process, cleanup files).
    pub async fn destroy(&self, vm: VmHandle) -> Result<()>;

    /// Check if VM process is still running.
    pub fn is_running(vm: &VmHandle) -> bool;
}
```

```rust
// === vsock.rs — Host-side vsock client ===

pub struct VsockClient {
    uds_path: PathBuf,
    port: u32,
}

impl VsockClient {
    pub fn new(uds_path: PathBuf, port: u32) -> Self;

    /// Ping the guest agent.
    pub async fn ping(&self) -> Result<bool>;

    /// Execute a command (wait for completion).
    pub async fn exec(&self, command: &str, timeout: Duration) -> Result<ExecResult>;

    /// Execute with streaming (returns channels).
    pub async fn exec_stream(&self, command: &str) -> Result<(
        tokio::sync::mpsc::Receiver<ExecEvent>,
        tokio::sync::mpsc::Sender<Vec<u8>>,  // stdin
    )>;

    /// Send signal to running process.
    pub async fn signal(&self, signal: i32) -> Result<()>;

    /// Read a file from the guest.
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>>;

    /// Write a file to the guest.
    pub async fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;

    /// List files in a directory.
    pub async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
}
```

### agentbox-daemon (HTTP/WS server)

Thin wrapper around `agentbox-core`. The daemon is started by the CLI's `serve` command
or by systemd.

#### HTTP API

| Method | Path | Description | Response |
|--------|------|-------------|----------|
| `POST` | `/sandboxes` | Create sandbox | `{id, status, created_at}` |
| `GET` | `/sandboxes` | List sandboxes | `[{id, status, created_at, ...}]` |
| `GET` | `/sandboxes/{id}` | Get sandbox info | `{id, status, memory, vcpus, ...}` |
| `DELETE` | `/sandboxes/{id}` | Destroy sandbox | `{status: "destroyed"}` |
| `POST` | `/sandboxes/{id}/exec` | Execute (non-streaming) | `{stdout, stderr, exit_code}` |
| `WS` | `/sandboxes/{id}/ws` | WebSocket (streaming exec) | Bidirectional stream |
| `POST` | `/sandboxes/{id}/files` | Upload file | `{path, size}` |
| `GET` | `/sandboxes/{id}/files?path=...` | Download file | Binary content |
| `GET` | `/sandboxes/{id}/files?list&path=...` | List files | `[{name, size, is_dir}]` |
| `GET` | `/health` | Health check | `{status, pool: {available, active, max}}` |

#### WebSocket Protocol

```
// Client → Server
{"type": "exec", "command": "python script.py", "timeout": 30}
{"type": "stdin", "data": "cHJpbnQoJ2hlbGxvJyk="}  // base64
{"type": "signal", "signal": 2}   // SIGINT
{"type": "resize", "cols": 80, "rows": 24}

// Server → Client
{"type": "stdout", "data": "UHJvY2Vzc2luZy4uLg=="}  // base64
{"type": "stderr", "data": "V2FybmluZzogLi4u"}
{"type": "exit", "code": 0}
{"type": "error", "message": "sandbox not found"}
{"type": "ready"}  // sandbox is ready for commands
```

Binary data (stdout/stderr/stdin) is base64-encoded in JSON messages to avoid
framing issues. For high-throughput scenarios, a future optimization could use
WebSocket binary frames.

### guest-agent (inside VM)

#### Vsock Protocol (length-prefixed JSON)

```
┌──────────┬─────────────────────┐
│ 4 bytes  │ N bytes             │
│ (big-    │ (JSON payload)      │
│  endian  │                     │
│  length) │                     │
└──────────┴─────────────────────┘
```

#### Request/Response Format

```json
// Request
{"id": 1, "method": "exec", "params": {"command": "ls -la", "timeout": 30}}
{"id": 2, "method": "exec_stream", "params": {"command": "python script.py"}}
{"id": 2, "method": "stdin", "params": {"data": "<base64>"}}
{"id": 2, "method": "signal", "params": {"signal": 2}}
{"id": 3, "method": "read_file", "params": {"path": "/workspace/output.txt"}}
{"id": 4, "method": "write_file", "params": {"path": "/workspace/data.csv", "content": "<base64>"}}
{"id": 5, "method": "list_files", "params": {"path": "/workspace"}}
{"id": 6, "method": "ping"}

// Response (non-streaming)
{"id": 1, "result": {"stdout": "...", "stderr": "", "exit_code": 0}}
{"id": 3, "result": {"content": "<base64>"}}
{"id": 6, "result": {"status": "ok"}}

// Streaming response (exec_stream) — multiple messages with same id
{"id": 2, "stream": "stdout", "data": "<base64>"}
{"id": 2, "stream": "stderr", "data": "<base64>"}
{"id": 2, "result": {"exit_code": 0}}  // final message for this id
```

The guest agent allocates a PTY for `exec_stream` commands, enabling interactive
sessions. For non-streaming `exec`, it uses simple pipe-based command execution.

### agentbox-cli

The CLI binary wraps both `agentbox-core` (for local operations) and an HTTP client
(for remote operations).

```
agentbox
├── serve [--port 8080] [--config config.toml]   # Start daemon
├── list [--url http://remote:8080]               # List sandboxes
├── exec <sandbox-id> "command" [--url ...]       # Run command
├── stop <sandbox-id> [--url ...]                 # Destroy sandbox
├── logs <sandbox-id> [--url ...]                 # Tail logs
├── status [--url ...]                            # Health + pool stats
└── version                                       # Version info
```

When `--url` is not provided:
1. Check if local daemon is running (pid file or port check)
2. If yes: use HTTP client to talk to local daemon
3. If no: use `agentbox-core` directly (start pool in-process)

When `--url` is provided: always use HTTP client.

## VM Lifecycle

```
                    ┌─────────────────────────┐
                    │     Daemon Start        │
                    │  Pool.start()           │
                    └───────────┬─────────────┘
                                │
                    ┌───────────▼─────────────┐
                    │  Replenish Loop         │
                    │  while available < min  │
                    │    VmManager.create()   │◄──── Background task
                    │    push to available    │       (runs continuously)
                    └───────────┬─────────────┘
                                │
        SDK calls               │
   Sandbox.create() ──────►┌───▼────────────────┐
                            │  Pool.claim()       │
                            │  pop from available  │
                            │  → Sandbox ready     │
                            └───┬────────────────┘
                                │
                    ┌───────────▼─────────────┐
                    │  sandbox.exec()         │
                    │  VsockClient → guest    │
                    │  agent → PTY → process  │
                    │  ← streaming output     │
                    └───────────┬─────────────┘
                                │
                    ┌───────────▼─────────────┐
   sandbox.destroy() ──────►│  Pool.release()     │
                            │  VmManager.destroy() │
                            │  kill FC process     │
                            │  rm temp dir         │
                            └─────────────────────┘
```

### VM Creation (from snapshot)

1. Create temp directory: `/tmp/agentbox-{vm_id}/`
2. Copy rootfs: `cp --reflink=auto base/rootfs.ext4 → temp/rootfs.ext4`
   (CoW copy on filesystems that support it, regular copy otherwise)
3. Start Firecracker process: `firecracker --api-sock temp/api.sock`
4. Wait for API socket to appear
5. PUT `/snapshot/load` with snapshot path + mmap memory backend
6. Wait for guest agent ping via vsock
7. VM is ready

Target time budget:
- Step 2 (rootfs copy): ~50ms (CoW) or ~200ms (full copy for 500MB)
- Step 3-4 (FC start): ~30ms
- Step 5 (snapshot load): ~100ms (mmap, no memory copy)
- Step 6 (agent ping): ~50ms
- **Total: ~230ms** (CoW path) or ~380ms (full copy path)

## Build Pipeline (artifacts/)

The Makefile builds everything needed for Firecracker VMs:

```makefile
# artifacts/Makefile

all: kernel rootfs guest-agent snapshot

kernel:           # Download Linux source, apply minimal config, build vmlinux
rootfs:           # Build Alpine rootfs with Python, Node.js, dev tools
guest-agent:      # Cross-compile guest-agent crate for the target VM arch
snapshot:         # Boot a VM, wait for guest agent, take snapshot
clean:            # Remove all build artifacts

# Output structure:
# artifacts/output/
#   ├── vmlinux
#   ├── rootfs.ext4
#   ├── guest-agent  (binary, also embedded in rootfs)
#   └── snapshot/
#       ├── vmstate.bin
#       └── memory.bin
```

The CI pipeline (GitHub Actions) runs this Makefile on a KVM-enabled runner to produce
release artifacts. Pre-built artifacts are uploaded to GitHub Releases as tarballs.

## Install Script (install.sh)

```bash
#!/bin/sh
set -eu

REPO="your-org/agentbox"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/agentbox"

# 1. Detect architecture
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)  ARCH_SUFFIX="x86_64" ;;
  aarch64) ARCH_SUFFIX="aarch64" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# 2. Verify KVM
if [ ! -e /dev/kvm ]; then
  echo "ERROR: /dev/kvm not found."
  echo "AgentBox requires KVM. Run on bare-metal Linux or enable nested virt."
  exit 1
fi

# 3. Get latest release
VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep tag_name | cut -d'"' -f4)

# 4. Download binary
curl -fsSL "https://github.com/$REPO/releases/download/$VERSION/agentbox-linux-$ARCH_SUFFIX" \
  -o "$INSTALL_DIR/agentbox"
chmod +x "$INSTALL_DIR/agentbox"

# 5. Download artifacts
mkdir -p "$DATA_DIR"
curl -fsSL "https://github.com/$REPO/releases/download/$VERSION/agentbox-artifacts-$ARCH_SUFFIX.tar.gz" \
  | tar xz -C "$DATA_DIR"

# 6. Write config
cat > "$DATA_DIR/config.toml" <<EOF
[daemon]
listen = "127.0.0.1:8080"

[vm]
firecracker_bin = "$DATA_DIR/firecracker"
kernel_path = "$DATA_DIR/vmlinux"
rootfs_path = "$DATA_DIR/rootfs.ext4"
snapshot_path = "$DATA_DIR/snapshot"

[pool]
min_size = 2
max_size = 10
EOF

# 7. Install systemd service (if systemd is available)
if command -v systemctl >/dev/null 2>&1; then
  cat > /etc/systemd/system/agentbox.service <<EOF
[Unit]
Description=AgentBox Sandbox Daemon
After=network.target

[Service]
ExecStart=$INSTALL_DIR/agentbox serve --config $DATA_DIR/config.toml
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
  systemctl daemon-reload
  systemctl enable --now agentbox
  echo "AgentBox daemon started (systemd)."
else
  echo "No systemd found. Start manually: agentbox serve --config $DATA_DIR/config.toml"
fi

echo ""
echo "AgentBox $VERSION installed successfully!"
echo "Daemon running on http://127.0.0.1:8080"
echo ""
echo "Next: pip install agentbox"
```

## SDK Design

### Python SDK

```python
# agentbox/sandbox.py

import httpx
import websockets  # or websocket-client
from typing import AsyncIterator
from pydantic import BaseModel

class ExecResult(BaseModel):
    stdout: str
    stderr: str
    exit_code: int

class FileEntry(BaseModel):
    name: str
    size: int
    is_dir: bool

class Sandbox:
    """A sandboxed environment for executing code."""

    def __init__(self, id: str, client: "AgentBoxClient"):
        self.id = id
        self._client = client

    @classmethod
    def create(
        cls,
        url: str = None,     # default: AGENTBOX_URL env or localhost:8080
        memory_mb: int = 2048,
        vcpus: int = 2,
        network: bool = False,
        timeout: int = 3600,
    ) -> "Sandbox":
        """Create a new sandbox. Boots a microVM in <300ms."""
        client = AgentBoxClient(url)
        data = client.post("/sandboxes", json={
            "memory_mb": memory_mb,
            "vcpus": vcpus,
            "network": network,
            "timeout": timeout,
        })
        return cls(id=data["id"], client=client)

    def exec(self, command: str, timeout: int = 30) -> ExecResult:
        """Execute a command and wait for completion."""
        data = self._client.post(
            f"/sandboxes/{self.id}/exec",
            json={"command": command, "timeout": timeout},
        )
        return ExecResult(**data)

    async def exec_stream(self, command: str) -> AsyncIterator[dict]:
        """Execute with streaming output via WebSocket."""
        async with self._client.ws(f"/sandboxes/{self.id}/ws") as ws:
            await ws.send_json({"type": "exec", "command": command})
            async for msg in ws:
                yield msg
                if msg.get("type") == "exit":
                    break

    def upload(self, local_path: str, remote_path: str) -> None:
        """Upload a file to the sandbox."""
        with open(local_path, "rb") as f:
            self._client.post(
                f"/sandboxes/{self.id}/files",
                files={"file": f},
                data={"path": remote_path},
            )

    def download(self, remote_path: str) -> bytes:
        """Download a file from the sandbox."""
        return self._client.get_bytes(
            f"/sandboxes/{self.id}/files",
            params={"path": remote_path},
        )

    def list_files(self, path: str = "/workspace") -> list[FileEntry]:
        """List files in the sandbox."""
        data = self._client.get(
            f"/sandboxes/{self.id}/files",
            params={"list": True, "path": path},
        )
        return [FileEntry(**f) for f in data]

    def destroy(self) -> None:
        """Destroy the sandbox and its VM."""
        self._client.delete(f"/sandboxes/{self.id}")

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.destroy()

    # === LLM Tool Definitions ===

    def tool_definitions(self, format: str = "openai") -> list[dict]:
        """Return tool schemas for LLM function calling."""
        from .tools import get_tool_definitions
        return get_tool_definitions(format, self.id)

    def handle_tool_call(self, tool_call: dict) -> dict:
        """Execute an LLM tool call against this sandbox."""
        from .tools import handle_tool_call
        return handle_tool_call(self, tool_call)
```

### Tool Definitions (tools.py)

```python
# agentbox/tools.py

SANDBOX_TOOLS = [
    {
        "name": "execute_code",
        "description": "Execute a bash command or script in the sandbox. Use this to run code, install packages, or perform any shell operation.",
        "parameters": {
            "command": {"type": "string", "description": "The bash command to execute"},
        },
        "required": ["command"],
    },
    {
        "name": "write_file",
        "description": "Write content to a file in the sandbox.",
        "parameters": {
            "path": {"type": "string", "description": "Absolute path in the sandbox"},
            "content": {"type": "string", "description": "File content to write"},
        },
        "required": ["path", "content"],
    },
    {
        "name": "read_file",
        "description": "Read the contents of a file in the sandbox.",
        "parameters": {
            "path": {"type": "string", "description": "Absolute path in the sandbox"},
        },
        "required": ["path"],
    },
]

def get_tool_definitions(format: str, sandbox_id: str) -> list[dict]:
    if format == "openai":
        return [{"type": "function", "function": {
            "name": t["name"],
            "description": t["description"],
            "parameters": {"type": "object", "properties": t["parameters"], "required": t["required"]},
        }} for t in SANDBOX_TOOLS]
    elif format == "anthropic":
        return [{"name": t["name"], "description": t["description"], "input_schema": {
            "type": "object", "properties": t["parameters"], "required": t["required"],
        }} for t in SANDBOX_TOOLS]
    else:
        return SANDBOX_TOOLS

def handle_tool_call(sandbox: "Sandbox", tool_call: dict) -> dict:
    name = tool_call.get("name") or tool_call.get("function", {}).get("name")
    args = tool_call.get("arguments") or tool_call.get("input", {})
    if isinstance(args, str):
        import json
        args = json.loads(args)

    if name == "execute_code":
        result = sandbox.exec(args["command"])
        return {"stdout": result.stdout, "stderr": result.stderr, "exit_code": result.exit_code}
    elif name == "write_file":
        sandbox.upload_content(args["content"].encode(), args["path"])
        return {"status": "written", "path": args["path"]}
    elif name == "read_file":
        content = sandbox.download(args["path"])
        return {"content": content.decode()}
    else:
        return {"error": f"Unknown tool: {name}"}
```

## Configuration (config.toml)

```toml
[daemon]
listen = "127.0.0.1:8080"     # Daemon listen address
log_level = "info"             # trace, debug, info, warn, error

[vm]
firecracker_bin = "/var/lib/agentbox/firecracker"
kernel_path = "/var/lib/agentbox/vmlinux"
rootfs_path = "/var/lib/agentbox/rootfs.ext4"
snapshot_path = "/var/lib/agentbox/snapshot"

[vm.defaults]
memory_mb = 2048               # Default per-sandbox
vcpus = 2
network = false
timeout_secs = 3600            # Auto-destroy after this

[pool]
min_size = 2                   # Warm VMs to keep ready
max_size = 10                  # Max concurrent VMs
idle_timeout_secs = 3600       # Destroy idle warm VMs after this

[guest]
vsock_port = 5000              # Guest agent vsock port
ping_timeout_ms = 5000         # Time to wait for agent readiness
```

## Dependency Graph

```
agentbox-cli ──────────► agentbox-core ◄──────── agentbox-daemon
     │                        │                        │
     │                        │                        │
     ▼                        ▼                        ▼
   clap                    tokio                     axum
                           serde                   tokio-tungstenite
                           thiserror
                           uuid
                           toml

guest-agent (standalone, no workspace deps)
     │
     ▼
   tokio, serde, nix (for PTY), uuid
```

## Testing Strategy

| Layer | Test Type | What | How |
|-------|-----------|------|-----|
| `agentbox-core` | Unit | Pool logic, config parsing | Mock VmManager |
| `agentbox-core` | Unit | Vsock protocol (codec) | In-memory streams |
| `agentbox-daemon` | Unit | HTTP handlers | axum::test helpers |
| `agentbox-daemon` | Integration | Full API flow | Real Firecracker (CI with KVM) |
| `guest-agent` | Unit | Protocol parsing, file ops | Mock vsock |
| `guest-agent` | Integration | Exec + PTY | Real VM |
| Python SDK | Unit | Client, tool definitions | Mock HTTP |
| Python SDK | Integration | Full flow | Real daemon |
| TS SDK | Unit | Client, tool definitions | Mock HTTP |
| TS SDK | Integration | Full flow | Real daemon |

Integration tests require KVM access. CI uses self-hosted runners or cloud instances
with nested virtualization (e.g., GCP N2 with `--enable-nested-virtualization`).
