use base64::Engine;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::protocol::{write_message, Response, StreamMessage};

pub async fn handle_exec(id: u64, params: Option<Value>) -> Response {
    let (command, timeout_secs) = match extract_exec_params(&params) {
        Ok(v) => v,
        Err(e) => {
            return Response {
                id,
                result: None,
                error: Some(e),
            }
        }
    };

    let mut child = match tokio::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Response {
                id,
                result: None,
                error: Some(format!("Failed to spawn process: {e}")),
            }
        }
    };

    let timeout = Duration::from_secs(timeout_secs);

    // Take pipes before wait so we can read them
    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();

    let io_and_wait = async {
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let (r1, r2) = tokio::join!(
            stdout_pipe.read_to_end(&mut stdout_buf),
            stderr_pipe.read_to_end(&mut stderr_buf),
        );
        r1?;
        r2?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((stdout_buf, stderr_buf, status))
    };

    match tokio::time::timeout(timeout, io_and_wait).await {
        Ok(Ok((stdout_buf, stderr_buf, status))) => {
            let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
            let stderr = String::from_utf8_lossy(&stderr_buf).to_string();
            let exit_code = status.code().unwrap_or(-1);
            Response {
                id,
                result: Some(serde_json::json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code,
                })),
                error: None,
            }
        }
        Ok(Err(e)) => Response {
            id,
            result: None,
            error: Some(format!("Process error: {e}")),
        },
        Err(_) => {
            // child was moved into the async block, but on timeout it's dropped
            // which will kill the process
            Response {
                id,
                result: None,
                error: Some("command timed out".to_string()),
            }
        }
    }
}

pub async fn handle_exec_stream<W: AsyncWriteExt + Unpin>(
    id: u64,
    params: Option<Value>,
    writer: &mut W,
) {
    let command = match params
        .as_ref()
        .and_then(|p| p.get("command"))
        .and_then(|v| v.as_str())
    {
        Some(c) => c.to_string(),
        None => {
            let resp = Response {
                id,
                result: None,
                error: Some("Missing 'command' parameter".to_string()),
            };
            let _ = write_message(writer, &resp).await;
            return;
        }
    };

    let mut child = match tokio::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let resp = Response {
                id,
                result: None,
                error: Some(format!("Failed to spawn process: {e}")),
            };
            let _ = write_message(writer, &resp).await;
            return;
        }
    };

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamMessage>(64);

    let tx_stdout = tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let data =
                        base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    let msg = StreamMessage {
                        id,
                        stream: "stdout".to_string(),
                        data,
                    };
                    if tx_stdout.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let tx_stderr = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let data =
                        base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    let msg = StreamMessage {
                        id,
                        stream: "stderr".to_string(),
                        data,
                    };
                    if tx_stderr.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Drop our copy of tx so rx closes when both tasks finish
    drop(tx);

    // Forward stream messages to the writer
    while let Some(msg) = rx.recv().await {
        if let Err(e) = write_message(writer, &msg).await {
            tracing::error!("Failed to write stream message: {e}");
            break;
        }
    }

    // Wait for tasks and process
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let exit_code = match child.wait().await {
        Ok(status) => status.code().unwrap_or(-1),
        Err(_) => -1,
    };

    let final_resp = Response {
        id,
        result: Some(serde_json::json!({ "exit_code": exit_code })),
        error: None,
    };
    let _ = write_message(writer, &final_resp).await;
}

fn extract_exec_params(params: &Option<Value>) -> Result<(String, u64), String> {
    let params = params
        .as_ref()
        .ok_or_else(|| "Missing params".to_string())?;
    let command = params
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'command' parameter".to_string())?
        .to_string();
    let timeout = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);
    Ok((command, timeout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_exec_simple() {
        let params = Some(serde_json::json!({"command": "echo hello"}));
        let resp = handle_exec(1, params).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["stdout"], "hello\n");
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_exec_exit_code() {
        let params = Some(serde_json::json!({"command": "exit 42"}));
        let resp = handle_exec(2, params).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["exit_code"], 42);
    }

    #[tokio::test]
    async fn test_exec_stderr() {
        let params = Some(serde_json::json!({"command": "echo error >&2"}));
        let resp = handle_exec(3, params).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["stderr"], "error\n");
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_exec_timeout() {
        let params = Some(serde_json::json!({"command": "sleep 100", "timeout": 1}));
        let resp = handle_exec(4, params).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap(), "command timed out");
    }
}
