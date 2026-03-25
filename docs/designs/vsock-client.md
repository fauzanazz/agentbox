# Vsock Client (Host-Side)

## Context

Implements the host-side vsock client in `agentbox-core`. This communicates with
the guest agent inside each Firecracker VM using a length-prefixed JSON protocol
over Firecracker's vsock UDS.

This task assumes `crates/agentbox-core/src/vsock.rs` exists with `VsockClient`
type stubs from FAU-67 (project scaffold). The protocol matches the guest agent
(FAU-68) — same 4-byte big-endian length prefix + JSON.

AgentBox is a self-hosted sandbox infrastructure for AI agents.
See `docs/spec.md` and `docs/architecture.md` for full context.

## Requirements

- Connect to guest agent via Firecracker vsock UDS
- Firecracker CONNECT handshake (send `"CONNECT {port}\n"`, expect `"OK ...\n"`)
- Ping guest agent
- Execute commands (blocking: wait for result)
- Execute with streaming (returns mpsc channels for stdout/stderr + stdin)
- Send signal to running process
- File read/write/list operations
- All operations with proper timeout handling

## Implementation

### Modify `crates/agentbox-core/Cargo.toml`

Add dependency (if not already present):
```toml
base64 = "0.22"
```

### Implement `crates/agentbox-core/src/vsock.rs`

Replace all `todo!()` stubs with real implementations.

**Firecracker vsock connection from host side:**
The host connects to a Unix Domain Socket (e.g., `work_dir/vsock.sock`). The protocol:
1. Connect to UDS
2. Write `"CONNECT {port}\n"` (e.g., `"CONNECT 5000\n"`)
3. Read a line — expect `"OK {cid}\n"`
4. Now the stream is connected to the guest agent

```rust
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use std::time::Duration;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

impl VsockClient {
    /// Establish a vsock connection to the guest agent.
    async fn connect(&self) -> crate::error::Result<UnixStream> {
        let stream = UnixStream::connect(&self.uds_path).await
            .map_err(|e| crate::error::AgentBoxError::VsockConnection(
                format!("Failed to connect to vsock UDS {:?}: {e}", self.uds_path)
            ))?;

        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        // Send CONNECT
        writer.write_all(format!("CONNECT {}\n", self.port).as_bytes()).await
            .map_err(|e| crate::error::AgentBoxError::VsockConnection(e.to_string()))?;
        writer.flush().await?;

        // Read OK response
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut response)).await
            .map_err(|_| crate::error::AgentBoxError::Timeout("vsock CONNECT handshake".into()))?
            .map_err(|e| crate::error::AgentBoxError::VsockConnection(e.to_string()))?;

        if !response.starts_with("OK") {
            return Err(crate::error::AgentBoxError::VsockConnection(
                format!("vsock CONNECT failed: {response}")
            ));
        }

        // Reunite the stream
        Ok(reader.into_inner().unsplit(writer))
    }
}
```

Note: `unsplit` requires the original `UnixStream`. Alternative approach: use the
stream directly without splitting for the handshake, then split after:
```rust
async fn connect(&self) -> Result<UnixStream> {
    let mut stream = UnixStream::connect(&self.uds_path).await?;
    stream.write_all(format!("CONNECT {}\n", self.port).as_bytes()).await?;
    stream.flush().await?;

    // Read response byte by byte until newline
    let mut buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        buf.push(byte[0]);
        if byte[0] == b'\n' { break; }
    }
    let response = String::from_utf8_lossy(&buf);
    if !response.starts_with("OK") {
        return Err(AgentBoxError::VsockConnection(format!("CONNECT failed: {response}")));
    }
    Ok(stream)
}
```

**Protocol helpers (private):**

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static REQ_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    REQ_ID.fetch_add(1, Ordering::Relaxed)
}

/// Send a request, receive a single response.
async fn request(
    stream: &mut UnixStream,
    method: &str,
    params: Option<serde_json::Value>,
) -> crate::error::Result<serde_json::Value> {
    let id = next_id();
    let req = serde_json::json!({ "id": id, "method": method, "params": params });

    // Write length-prefixed JSON
    let payload = serde_json::to_vec(&req)?;
    let len_bytes = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len_bytes).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;

    // Read length-prefixed JSON response
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let msg_len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; msg_len];
    stream.read_exact(&mut buf).await?;

    let resp: serde_json::Value = serde_json::from_slice(&buf)?;

    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(crate::error::AgentBoxError::ExecFailed(err.to_string()));
    }

    Ok(resp.get("result").cloned().unwrap_or(serde_json::Value::Null))
}
```

**Public methods — replace all `todo!()` stubs:**

**`ping(&self) -> Result<bool>`**:
```rust
pub async fn ping(&self) -> Result<bool> {
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        async {
            let mut stream = self.connect().await?;
            request(&mut stream, "ping", None).await
        }
    ).await;

    match result {
        Ok(Ok(val)) => Ok(val.get("status").and_then(|v| v.as_str()) == Some("ok")),
        _ => Ok(false),
    }
}
```

**`exec(&self, command, timeout) -> Result<ExecResult>`**:
```rust
pub async fn exec(&self, command: &str, timeout: Duration) -> Result<ExecResult> {
    let mut stream = self.connect().await?;
    let params = serde_json::json!({
        "command": command,
        "timeout": timeout.as_secs()
    });
    let result = tokio::time::timeout(
        timeout + Duration::from_secs(5), // extra buffer
        request(&mut stream, "exec", Some(params))
    ).await
    .map_err(|_| AgentBoxError::Timeout("exec timed out".into()))??;

    Ok(ExecResult {
        stdout: result.get("stdout").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        stderr: result.get("stderr").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        exit_code: result.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32,
    })
}
```

**`exec_stream(&self, command) -> Result<(Receiver<ExecEvent>, Sender<Vec<u8>>)>`**:
```rust
pub async fn exec_stream(&self, command: &str) -> Result<(
    tokio::sync::mpsc::Receiver<ExecEvent>,
    tokio::sync::mpsc::Sender<Vec<u8>>,
)> {
    let stream = self.connect().await?;
    let (mut reader, mut writer) = tokio::io::split(stream);

    let id = next_id();
    let req = serde_json::json!({
        "id": id, "method": "exec_stream",
        "params": { "command": command }
    });
    let payload = serde_json::to_vec(&req)?;
    writer.write_all(&(payload.len() as u32).to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<ExecEvent>(256);
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    // Reader task: read streaming messages
    tokio::spawn(async move {
        loop {
            let mut len_buf = [0u8; 4];
            if reader.read_exact(&mut len_buf).await.is_err() { break; }
            let msg_len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; msg_len];
            if reader.read_exact(&mut buf).await.is_err() { break; }

            let msg: serde_json::Value = match serde_json::from_slice(&buf) {
                Ok(v) => v,
                Err(_) => break,
            };

            // Check if this is a stream message or final result
            if let Some(stream_type) = msg.get("stream").and_then(|v| v.as_str()) {
                let data = msg.get("data").and_then(|v| v.as_str()).unwrap_or("");
                let decoded = B64.decode(data).unwrap_or_default();
                let event = match stream_type {
                    "stdout" => ExecEvent::Stdout(decoded),
                    "stderr" => ExecEvent::Stderr(decoded),
                    _ => continue,
                };
                if event_tx.send(event).await.is_err() { break; }
            } else if let Some(result) = msg.get("result") {
                let exit_code = result.get("exit_code")
                    .and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
                let _ = event_tx.send(ExecEvent::Exit(exit_code)).await;
                break;
            } else if let Some(err) = msg.get("error").and_then(|v| v.as_str()) {
                let _ = event_tx.send(ExecEvent::Error(err.to_string())).await;
                break;
            }
        }
    });

    // Writer task: forward stdin
    tokio::spawn(async move {
        while let Some(data) = stdin_rx.recv().await {
            let msg = serde_json::json!({
                "id": id, "method": "stdin",
                "params": { "data": B64.encode(&data) }
            });
            let payload = serde_json::to_vec(&msg).unwrap();
            let _ = writer.write_all(&(payload.len() as u32).to_be_bytes()).await;
            let _ = writer.write_all(&payload).await;
            let _ = writer.flush().await;
        }
    });

    Ok((event_rx, stdin_tx))
}
```

**`signal(&self, signal) -> Result<()>`**:
```rust
pub async fn signal(&self, signal: i32) -> Result<()> {
    let mut stream = self.connect().await?;
    request(&mut stream, "signal", Some(serde_json::json!({ "signal": signal }))).await?;
    Ok(())
}
```

**`read_file(&self, path) -> Result<Vec<u8>>`**:
```rust
pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
    let mut stream = self.connect().await?;
    let result = request(&mut stream, "read_file", Some(serde_json::json!({ "path": path }))).await?;
    let content_b64 = result.get("content").and_then(|v| v.as_str()).unwrap_or("");
    B64.decode(content_b64).map_err(|e| AgentBoxError::FileOp(e.to_string()))
}
```

**`write_file(&self, path, data) -> Result<()>`**:
```rust
pub async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
    let mut stream = self.connect().await?;
    let content = B64.encode(data);
    request(&mut stream, "write_file", Some(serde_json::json!({
        "path": path, "content": content
    }))).await?;
    Ok(())
}
```

**`list_files(&self, path) -> Result<Vec<FileEntry>>`**:
```rust
pub async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
    let mut stream = self.connect().await?;
    let result = request(&mut stream, "list_files", Some(serde_json::json!({ "path": path }))).await?;
    let entries = result.get("entries").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    entries.into_iter().map(|e| {
        Ok(FileEntry {
            name: e.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            size: e.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            is_dir: e.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false),
        })
    }).collect()
}
```

## Testing Strategy

Run tests: `cargo test -p agentbox-core -- vsock`

### Unit tests in `crates/agentbox-core/src/vsock.rs`:

- `test_vsock_client_new` — create client, verify fields
- `test_connect_handshake` — mock UDS server that responds "OK 3\n", verify connect succeeds
- `test_connect_handshake_reject` — mock server responds "ERR\n", verify error
- `test_request_response_roundtrip` — mock server that echoes back, verify protocol encoding

To mock the vsock connection for tests, create a helper that:
1. Binds a `tokio::net::UnixListener` on a temp path
2. Spawns a task that accepts one connection, sends "OK 3\n" after reading "CONNECT {port}\n"
3. Then acts as a mock guest agent (reads requests, sends responses)

```rust
#[cfg(test)]
async fn mock_guest_agent(listener: tokio::net::UnixListener) {
    let (mut stream, _) = listener.accept().await.unwrap();
    // Read CONNECT line
    let mut buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await.unwrap();
        buf.push(byte[0]);
        if byte[0] == b'\n' { break; }
    }
    // Send OK
    stream.write_all(b"OK 3\n").await.unwrap();
    // Read request, send response (for ping)
    // ... read length-prefixed message, respond with {"id": N, "result": {"status": "ok"}}
}
```

### Integration tests (require running guest agent):
- These are tested end-to-end in Task E. Skip here.

## Out of Scope

- Connection pooling (one connection per request for now)
- Reconnection logic
- Multiplexing multiple concurrent requests over one connection
- Integration with VmManager (that's Task E)
