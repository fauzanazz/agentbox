# Guest Agent Binary

## Context

The guest agent is a small Rust binary that runs inside each Firecracker microVM.
It listens on a vsock port and handles commands from the host: command execution,
file operations, and process management.

This is part of AgentBox — a self-hosted sandbox infrastructure for AI agents.
See `docs/spec.md` and `docs/architecture.md` for full context.

This is a standalone crate with no workspace dependencies on agentbox-core.
It can be built independently.

## Requirements

- vsock-compatible server listening on port 5000 (use TCP for development/testing)
- Length-prefixed JSON codec (4-byte big-endian length prefix)
- Command execution with stdout/stderr capture (non-streaming)
- Streaming exec with PTY allocation
- File read/write/list operations
- Signal forwarding to running processes
- Ping/health check endpoint

## Implementation

### `crates/guest-agent/Cargo.toml`

This file already exists from the scaffold task (FAU-67). Verify it has:
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

### `crates/guest-agent/src/main.rs`

- Parse optional CLI arg: `--port <PORT>` (default 5000), `--tcp` flag for dev mode
- Init tracing subscriber with env filter
- In production: bind vsock listener on `VMADDR_CID_ANY:5000`
  - Use raw socket: `socket(AF_VSOCK, SOCK_STREAM, 0)` + bind + listen
  - Wrap in tokio `TcpListener` via `from_std` after setting non-blocking
- In `--tcp` mode: bind TCP on `0.0.0.0:{port}` (for development without KVM)
- For each accepted connection: `tokio::spawn(server::handle_connection(stream))`
- Graceful shutdown on SIGTERM via `tokio::signal`

### `crates/guest-agent/src/protocol.rs`

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StreamMessage {
    pub id: u64,
    pub stream: String,   // "stdout" or "stderr"
    pub data: String,      // base64 encoded
}

/// Read a length-prefixed JSON message from a reader.
/// Format: [4 bytes big-endian length][JSON payload]
pub async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> std::io::Result<Request> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Write a length-prefixed JSON message to a writer.
pub async fn write_message<W: AsyncWriteExt + Unpin, T: Serialize>(writer: &mut W, msg: &T) -> std::io::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    let len = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}
```

### `crates/guest-agent/src/server.rs`

```rust
use tokio::io::{AsyncRead, AsyncWrite};
use crate::protocol::{read_message, write_message, Response};
use crate::{exec, files};

pub async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(stream: S) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    loop {
        let request = match read_message(&mut reader).await {
            Ok(req) => req,
            Err(e) => {
                // Connection closed or read error — exit cleanly
                tracing::debug!("Connection ended: {e}");
                break;
            }
        };

        let response = match request.method.as_str() {
            "ping" => Response {
                id: request.id,
                result: Some(serde_json::json!({"status": "ok"})),
                error: None,
            },
            "exec" => exec::handle_exec(request.id, request.params).await,
            "exec_stream" => {
                // Streaming: sends multiple messages, then a final Response
                exec::handle_exec_stream(request.id, request.params, &mut writer).await;
                continue; // final response already sent by handle_exec_stream
            },
            "read_file" => files::handle_read(request.id, request.params).await,
            "write_file" => files::handle_write(request.id, request.params).await,
            "list_files" => files::handle_list(request.id, request.params).await,
            _ => Response {
                id: request.id,
                result: None,
                error: Some(format!("Unknown method: {}", request.method)),
            },
        };

        if let Err(e) = write_message(&mut writer, &response).await {
            tracing::error!("Failed to write response: {e}");
            break;
        }
    }
}
```

### `crates/guest-agent/src/exec.rs`

**`handle_exec(id, params) -> Response`**:
- Extract `command: String` and `timeout: u64` (default 30) from params
- Use `tokio::process::Command::new("/bin/sh").args(["-c", &command])`
- Set `.stdout(Stdio::piped()).stderr(Stdio::piped())`
- Wrap in `tokio::time::timeout(Duration::from_secs(timeout), child.wait_with_output())`
- On success: return Response with `{ stdout, stderr, exit_code }`
- On timeout: kill child, return Response with error "command timed out"

**`handle_exec_stream(id, params, writer) -> ()`**:
- Extract `command: String` from params
- Open PTY with `nix::pty::openpty(None, None)`
  - If PTY fails, fall back to pipe-based streaming
- Fork process: `tokio::process::Command::new("/bin/sh").args(["-c", &command])`
  - With PTY: use unsafe to set slave as stdin/stdout/stderr, setsid, set controlling terminal
  - Without PTY: pipe stdout and stderr
- Read loop on PTY master (or pipes):
  - Read chunks (up to 4096 bytes)
  - Base64 encode
  - Send `StreamMessage { id, stream: "stdout", data }` via `write_message`
- Wait for process exit
- Send final `Response { id, result: { exit_code }, error: None }`

For MVP, the pipe-based approach is sufficient. PTY can be simplified:
- Spawn with `.stdout(Stdio::piped()).stderr(Stdio::piped())`
- Spawn two read tasks: one for stdout, one for stderr
- Each task reads in a loop, sends StreamMessage for each chunk
- When both tasks complete and process exits, send final Response

### `crates/guest-agent/src/files.rs`

**`handle_read(id, params) -> Response`**:
- Extract `path: String` from params
- `tokio::fs::read(&path).await`
- Base64 encode content
- Return `Response { result: { content: "<base64>" } }`
- On error: return Response with error field

**`handle_write(id, params) -> Response`**:
- Extract `path: String` and `content: String` (base64) from params
- Base64 decode content
- Create parent directories: `tokio::fs::create_dir_all(parent).await`
- `tokio::fs::write(&path, &decoded).await`
- Return `Response { result: { bytes_written: N } }`

**`handle_list(id, params) -> Response`**:
- Extract `path: String` from params (default "/workspace")
- `tokio::fs::read_dir(&path).await`
- For each entry: get name, metadata (size, is_dir)
- Return `Response { result: { entries: [{name, size, is_dir}] } }`

## Testing Strategy

Run all tests with: `cargo test -p guest-agent`

### Unit tests in `crates/guest-agent/src/protocol.rs`:
- `test_read_write_roundtrip` — write a Response, read it back, verify equality
- `test_read_invalid_json` — write invalid JSON, verify error
- `test_large_message` — write a message > 64KB, verify roundtrip

### Unit tests in `crates/guest-agent/src/exec.rs`:
- `test_exec_simple` — run `echo hello`, verify stdout = "hello\n"
- `test_exec_exit_code` — run `exit 42`, verify exit_code = 42
- `test_exec_stderr` — run command that writes to stderr
- `test_exec_timeout` — run `sleep 100` with timeout 1, verify timeout error

### Unit tests in `crates/guest-agent/src/files.rs`:
- `test_write_and_read` — write file, read back, verify content matches
- `test_read_nonexistent` — read missing file, verify error
- `test_list_files` — create temp dir with files, list, verify entries
- `test_write_creates_dirs` — write to nested path, verify parent dirs created

### Integration test in `crates/guest-agent/tests/integration.rs`:
- Start guest-agent in `--tcp` mode on random port
- Connect TCP client, send ping, verify "ok"
- Send exec request, verify stdout
- Send write_file, then read_file, verify roundtrip

## Out of Scope

- vsock listener (use TCP with `--tcp` flag for all testing; vsock only matters in real VM)
- PTY resize handling
- stdin forwarding (handle_exec_stream accepts it but not tested here)
- Signal forwarding implementation (stub only)
- Process group management
