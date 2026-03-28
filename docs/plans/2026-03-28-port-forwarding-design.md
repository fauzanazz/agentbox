# Design: Port Forwarding for AgentBox

**Date:** 2026-03-28
**Status:** Draft

## Overview

Guest-to-host TCP port forwarding via vsock. Each forwarded port gets a TCP listener on the host. Each incoming TCP connection opens a fresh vsock connection to the guest agent, exchanges a single JSON handshake, then switches to raw bidirectional byte proxying.

## Data Flow

```
External TCP Client
       │
       ▼
Host TCP Listener (127.0.0.1:<allocated_port>)
       │
       ▼
Daemon: open vsock connection to guest agent
       │
       ▼
JSON handshake:
  → {"id":N, "method":"port_forward_connect", "params":{"port":3000}}
  ← {"id":N, "result":{"status":"connected"}}
       │
       ▼
Raw bidirectional byte proxy:
  TCP client ←→ UnixStream (vsock) ←→ Guest Agent ←→ localhost:3000 (inside VM)
```

## API Design

### Create Port Forward
```
POST /sandboxes/{id}/ports
Body: {"guest_port": 3000}
Response 201: {"guest_port": 3000, "host_port": 49152, "local_address": "127.0.0.1:49152"}
```

### List Port Forwards
```
GET /sandboxes/{id}/ports
Response 200: {"ports": [{"guest_port": 3000, "host_port": 49152, "local_address": "127.0.0.1:49152"}]}
```

### Remove Port Forward
```
DELETE /sandboxes/{id}/ports/{guest_port}
Response 204
```

## Component Changes

### 1. agentbox-core

**error.rs** — Add variant:
```rust
#[error("Port forward error: {0}")]
PortForward(String),
```

**vsock.rs** — Add method:
```rust
/// Opens a vsock connection for port forwarding.
/// Sends the handshake, then returns the raw stream for bidirectional proxying.
pub async fn open_port_forward(&self, guest_port: u16) -> Result<UnixStream>
```

**sandbox.rs** — Add type:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardInfo {
    pub guest_port: u16,
    pub host_port: u16,
    pub local_address: String,
}
```

### 2. guest-agent

**port_forward.rs** (new) — Handler:
```rust
/// Handles port_forward_connect: connects to localhost:port inside the VM,
/// sends success response, then proxies bytes between vsock and TCP streams.
pub async fn handle_port_forward_connect<R, W>(
    id: u64,
    params: Option<Value>,
    vsock_reader: R,
    vsock_writer: W,
) where R: AsyncRead + Unpin + Send, W: AsyncWrite + Unpin + Send
```

**server.rs** — Add dispatch:
```rust
"port_forward_connect" => {
    port_forward::handle_port_forward_connect(
        request.id, request.params, reader, writer
    ).await;
    return; // Connection consumed by proxy
}
```

### 3. agentbox-daemon

**port_forward.rs** (new) — Manager:
```rust
pub struct PortForwardEntry {
    pub guest_port: u16,
    pub host_port: u16,
    pub listener_handle: JoinHandle<()>,
}
```

Key functions:
- `start_forward(vsock_client, guest_port) -> Result<PortForwardEntry>`: Binds TCP listener on 127.0.0.1:0, spawns accept loop
- Accept loop: for each TCP connection, opens vsock port forward, spawns bidirectional copy task
- `stop_forward(entry)`: Aborts listener task

**state.rs** — Add to AppState:
```rust
pub port_forwards: Mutex<HashMap<String, HashMap<u16, PortForwardEntry>>>,
// outer key: sandbox_id, inner key: guest_port
```

**handlers.rs** — Add handlers:
```rust
pub async fn create_port_forward(State, Path(id), Json) -> Result<impl IntoResponse, AppError>
pub async fn list_port_forwards(State, Path(id)) -> Result<impl IntoResponse, AppError>
pub async fn remove_port_forward(State, Path(id), Path(guest_port)) -> Result<impl IntoResponse, AppError>
```

**routes.rs** — Add routes:
```rust
.route("/sandboxes/{id}/ports", post(handlers::create_port_forward))
.route("/sandboxes/{id}/ports", get(handlers::list_port_forwards))
.route("/sandboxes/{id}/ports/{guest_port}", delete(handlers::remove_port_forward))
```

### 4. Cleanup

When `destroy_sandbox` is called, iterate and abort all port forward listeners for that sandbox before removing it.

## Decisions

- **Host bind address**: Always `127.0.0.1` (localhost only) for security
- **Port allocation**: Let OS allocate via bind to port 0
- **Active connections on remove**: Listener stops; existing proxy tasks run until their connections close naturally
- **No connection limits** in v1
- **No guest-side port validation** — connection fails naturally when TCP client connects if guest port isn't listening
