use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::Response;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use agentbox_core::sandbox::{ExecEvent, Sandbox, SandboxId};

use crate::handlers::AppError;
use crate::state::AppState;

// ── Wire types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "exec")]
    Exec {
        command: String,
        #[allow(dead_code)]
        timeout: Option<u64>,
    },
    #[serde(rename = "stdin")]
    Stdin { data: String },
    #[serde(rename = "signal")]
    Signal { signal: i32 },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "stdout")]
    Stdout { data: String },
    #[serde(rename = "stderr")]
    Stderr { data: String },
    #[serde(rename = "exit")]
    Exit { code: i32 },
    #[serde(rename = "error")]
    Error { message: String },
}

// ── Handler ────────────────────────────────────────────────────────

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, sb)))
}

async fn handle_ws(mut socket: WebSocket, sandbox: Arc<Mutex<Sandbox>>) {
    if send_msg(&mut socket, &ServerMessage::Ready).await.is_err() {
        return;
    }

    let mut stdin_tx: Option<tokio::sync::mpsc::Sender<Vec<u8>>> = None;

    loop {
        let text = match socket.recv().await {
            Some(Ok(Message::Text(t))) => t,
            Some(Ok(Message::Close(_))) | None => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = send_msg(
                    &mut socket,
                    &ServerMessage::Error {
                        message: format!("Invalid message: {e}"),
                    },
                )
                .await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::Exec { command, .. } => {
                let sb = sandbox.lock().await;
                match sb.exec_stream(&command).await {
                    Ok((mut event_rx, new_stdin_tx)) => {
                        stdin_tx = Some(new_stdin_tx);
                        drop(sb);

                        while let Some(event) = event_rx.recv().await {
                            let (msg, is_terminal) = match event {
                                ExecEvent::Stdout(data) => (
                                    ServerMessage::Stdout {
                                        data: B64.encode(&data),
                                    },
                                    false,
                                ),
                                ExecEvent::Stderr(data) => (
                                    ServerMessage::Stderr {
                                        data: B64.encode(&data),
                                    },
                                    false,
                                ),
                                ExecEvent::Exit(code) => {
                                    stdin_tx = None;
                                    (ServerMessage::Exit { code }, true)
                                }
                                ExecEvent::Error(msg) => {
                                    stdin_tx = None;
                                    (ServerMessage::Error { message: msg }, true)
                                }
                            };

                            if send_msg(&mut socket, &msg).await.is_err() {
                                return;
                            }
                            if is_terminal {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        drop(sb);
                        let _ = send_msg(
                            &mut socket,
                            &ServerMessage::Error {
                                message: e.to_string(),
                            },
                        )
                        .await;
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
                    drop(sb);
                    let _ = send_msg(
                        &mut socket,
                        &ServerMessage::Error {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }
    }
}

async fn send_msg(socket: &mut WebSocket, msg: &ServerMessage) -> Result<(), ()> {
    let json = serde_json::to_string(msg).unwrap();
    socket.send(Message::Text(json.into())).await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ClientMessage deserialization ─────────────────────────────────

    #[test]
    fn test_client_message_exec_deserialization() {
        let with_timeout: ClientMessage =
            serde_json::from_str(r#"{"type":"exec","command":"ls -la","timeout":30}"#).unwrap();
        assert!(matches!(
            with_timeout,
            ClientMessage::Exec { ref command, timeout: Some(30) } if command == "ls -la"
        ));

        let without_timeout: ClientMessage =
            serde_json::from_str(r#"{"type":"exec","command":"pwd"}"#).unwrap();
        assert!(matches!(
            without_timeout,
            ClientMessage::Exec { ref command, timeout: None } if command == "pwd"
        ));
    }

    #[test]
    fn test_client_message_stdin_deserialization() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"stdin","data":"aGVsbG8="}"#).unwrap();
        assert!(matches!(msg, ClientMessage::Stdin { ref data } if data == "aGVsbG8="));
    }

    #[test]
    fn test_client_message_signal_deserialization() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"signal","signal":9}"#).unwrap();
        assert!(matches!(msg, ClientMessage::Signal { signal: 9 }));
    }

    #[test]
    fn test_client_message_invalid_type() {
        let result = serde_json::from_str::<ClientMessage>(r#"{"type":"unknown"}"#);
        assert!(result.is_err());
    }

    // ── ServerMessage serialization ──────────────────────────────────

    #[test]
    fn test_server_message_ready_serialization() {
        let val = serde_json::to_value(&ServerMessage::Ready).unwrap();
        assert_eq!(val, json!({"type": "ready"}));
    }

    #[test]
    fn test_server_message_stdout_serialization() {
        let val = serde_json::to_value(&ServerMessage::Stdout {
            data: "aGVsbG8=".into(),
        })
        .unwrap();
        assert_eq!(val, json!({"type": "stdout", "data": "aGVsbG8="}));
    }

    #[test]
    fn test_server_message_stderr_serialization() {
        let val = serde_json::to_value(&ServerMessage::Stderr {
            data: "ZXJyb3I=".into(),
        })
        .unwrap();
        assert_eq!(val, json!({"type": "stderr", "data": "ZXJyb3I="}));
    }

    #[test]
    fn test_server_message_exit_serialization() {
        let val = serde_json::to_value(&ServerMessage::Exit { code: 0 }).unwrap();
        assert_eq!(val, json!({"type": "exit", "code": 0}));
    }

    #[test]
    fn test_server_message_error_serialization() {
        let val = serde_json::to_value(&ServerMessage::Error {
            message: "something went wrong".into(),
        })
        .unwrap();
        assert_eq!(val, json!({"type": "error", "message": "something went wrong"}));
    }
}
