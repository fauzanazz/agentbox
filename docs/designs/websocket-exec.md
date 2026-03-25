# WebSocket Exec Handler

## Context

Adds WebSocket support to the daemon for streaming exec. This enables real-time
stdout/stderr streaming and interactive sessions (stdin + signals) over a single
persistent connection.

This task assumes `crates/agentbox-daemon/` has the HTTP API working from FAU-72,
with `routes.rs`, `handlers.rs`, and `state.rs` already in place.

See `docs/architecture.md` for the WebSocket protocol specification.

## Requirements

- WebSocket endpoint at `/sandboxes/{id}/ws`
- Client sends: exec commands, stdin data, signals
- Server sends: stdout/stderr chunks, exit codes, errors, ready signal
- Base64 encoding for binary data in JSON messages
- Multiple sequential commands per WebSocket connection
- Clean disconnect handling

## Implementation

### Update `crates/agentbox-daemon/Cargo.toml`

Add the `ws` feature to axum:
```toml
axum = { version = "0.8", features = ["ws", "multipart"] }
```

### Create `crates/agentbox-daemon/src/ws.rs`

```rust
use std::sync::Arc;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::response::Response;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use agentbox_core::{SandboxId, ExecEvent};
use crate::state::AppState;
use crate::handlers::AppError;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "exec")]
    Exec { command: String, timeout: Option<u64> },
    #[serde(rename = "stdin")]
    Stdin { data: String },  // base64 encoded
    #[serde(rename = "signal")]
    Signal { signal: i32 },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "stdout")]
    Stdout { data: String },  // base64 encoded
    #[serde(rename = "stderr")]
    Stderr { data: String },  // base64 encoded
    #[serde(rename = "exit")]
    Exit { code: i32 },
    #[serde(rename = "error")]
    Error { message: String },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state.get_sandbox(&sandbox_id).await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, sb)))
}

async fn handle_ws(
    mut socket: WebSocket,
    sandbox: Arc<tokio::sync::Mutex<agentbox_core::Sandbox>>,
) {
    // Send ready message
    let ready = serde_json::to_string(&ServerMessage::Ready).unwrap();
    if socket.send(Message::Text(ready.into())).await.is_err() { return; }

    // Track active stdin sender for current exec
    let mut stdin_tx: Option<tokio::sync::mpsc::Sender<Vec<u8>>> = None;

    loop {
        let msg = match socket.recv().await {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(_))) | None => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                let err = serde_json::to_string(&ServerMessage::Error {
                    message: format!("Invalid message: {e}"),
                }).unwrap();
                let _ = socket.send(Message::Text(err.into())).await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::Exec { command, timeout: _timeout } => {
                // Start streaming exec
                let sb = sandbox.lock().await;
                match sb.exec_stream(&command).await {
                    Ok((mut event_rx, new_stdin_tx)) => {
                        stdin_tx = Some(new_stdin_tx);
                        drop(sb); // release lock

                        // Forward events to WebSocket
                        while let Some(event) = event_rx.recv().await {
                            let server_msg = match event {
                                ExecEvent::Stdout(data) => ServerMessage::Stdout {
                                    data: B64.encode(&data),
                                },
                                ExecEvent::Stderr(data) => ServerMessage::Stderr {
                                    data: B64.encode(&data),
                                },
                                ExecEvent::Exit(code) => {
                                    stdin_tx = None;
                                    ServerMessage::Exit { code }
                                },
                                ExecEvent::Error(msg) => {
                                    stdin_tx = None;
                                    ServerMessage::Error { message: msg }
                                },
                            };
                            let json = serde_json::to_string(&server_msg).unwrap();
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                return;
                            }
                            // Break after exit/error
                            if matches!(server_msg, ServerMessage::Exit { .. } | ServerMessage::Error { .. }) {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        drop(sb);
                        let err = serde_json::to_string(&ServerMessage::Error {
                            message: e.to_string(),
                        }).unwrap();
                        let _ = socket.send(Message::Text(err.into())).await;
                    }
                }
            }
            ClientMessage::Stdin { data } => {
                if let Some(ref tx) = stdin_tx {
                    if let Ok(decoded) = B64.decode(&data) {
                        let _ = tx.send(decoded).await;
                    }
                }
            }
            ClientMessage::Signal { signal } => {
                let sb = sandbox.lock().await;
                if let Err(e) = sb.send_signal(signal).await {
                    let err = serde_json::to_string(&ServerMessage::Error {
                        message: e.to_string(),
                    }).unwrap();
                    drop(sb);
                    let _ = socket.send(Message::Text(err.into())).await;
                }
            }
        }
    }
}
```

Add `base64 = "0.22"` to `crates/agentbox-daemon/Cargo.toml`.

### Update `crates/agentbox-daemon/src/routes.rs`

Add the WebSocket route:
```rust
// Add import
use crate::ws;

// In build_router, add this route:
.route("/sandboxes/{id}/ws", get(ws::ws_handler))
```

### Update `crates/agentbox-daemon/src/main.rs`

Add module declaration:
```rust
mod ws;
```

## Testing Strategy

Run tests: `cargo test -p agentbox-daemon -- ws`

### Unit tests in `crates/agentbox-daemon/src/ws.rs`:

- `test_client_message_deserialization` — verify Exec, Stdin, Signal parse correctly
- `test_server_message_serialization` — verify Ready, Stdout, Exit serialize with correct `type` tag

### Integration tests (need KVM or mock):

- `test_ws_exec_streaming` — connect WebSocket, send exec, verify stdout chunks arrive, verify exit
- `test_ws_stdin_forwarding` — send exec for interactive command, send stdin, verify response
- `test_ws_invalid_sandbox` — connect to nonexistent sandbox ID, verify error
- Use `tokio-tungstenite` client for WebSocket tests:
  ```toml
  [dev-dependencies]
  tokio-tungstenite = "0.24"
  ```

## Out of Scope

- PTY resize messages
- Multiple concurrent execs per WebSocket (one at a time for now)
- Binary WebSocket frames (JSON + base64 only)
- WebSocket authentication
- Heartbeat/ping-pong keepalive
