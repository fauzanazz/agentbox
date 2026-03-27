use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::error::{AgentBoxError, Result};
use crate::sandbox::{ExecEvent, ExecResult, FileEntry};

/// Global monotonic request ID counter.
static REQ_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    REQ_ID.fetch_add(1, Ordering::Relaxed)
}

/// Host-side vsock client that communicates with the guest agent inside a
/// Firecracker microVM using a length-prefixed JSON protocol over a UDS.
#[derive(Debug)]
pub struct VsockClient {
    pub(crate) uds_path: PathBuf,
    pub(crate) port: u32,
}

impl VsockClient {
    /// Create a new vsock client targeting the given UDS path and guest port.
    pub fn new(uds_path: PathBuf, port: u32) -> Self {
        Self { uds_path, port }
    }

    /// Establish a vsock connection to the guest agent.
    ///
    /// Performs the Firecracker CONNECT handshake:
    /// 1. Connect to the UDS
    /// 2. Send `"CONNECT {port}\n"`
    /// 3. Read response — expect `"OK {cid}\n"`
    async fn connect(&self) -> Result<UnixStream> {
        let mut stream = UnixStream::connect(&self.uds_path).await.map_err(|e| {
            AgentBoxError::VsockConnection(format!(
                "Failed to connect to vsock UDS {:?}: {e}",
                self.uds_path
            ))
        })?;

        // Send CONNECT handshake
        stream
            .write_all(format!("CONNECT {}\n", self.port).as_bytes())
            .await
            .map_err(|e| AgentBoxError::VsockConnection(e.to_string()))?;
        stream.flush().await?;

        // Read response byte-by-byte until newline
        let mut buf = Vec::new();
        let response = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let mut byte = [0u8; 1];
                stream.read_exact(&mut byte).await?;
                buf.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Ok::<_, std::io::Error>(())
        })
        .await
        .map_err(|_| AgentBoxError::Timeout("vsock CONNECT handshake".into()))?
        .map_err(|e| AgentBoxError::VsockConnection(e.to_string()));

        response?;

        let response_str = String::from_utf8_lossy(&buf);
        if !response_str.starts_with("OK") {
            return Err(AgentBoxError::VsockConnection(format!(
                "vsock CONNECT failed: {response_str}"
            )));
        }

        Ok(stream)
    }

    /// Ping the guest agent. Returns `true` if the agent responds, `false` otherwise.
    pub async fn ping(&self) -> Result<bool> {
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = self.connect().await?;
            request(&mut stream, "ping", None).await
        })
        .await;

        match result {
            Ok(Ok(val)) => Ok(val.get("status").and_then(|v| v.as_str()) == Some("ok")),
            _ => Ok(false),
        }
    }

    /// Execute a command and wait for the result.
    pub async fn exec(&self, command: &str, timeout: Duration) -> Result<ExecResult> {
        let mut stream = self.connect().await?;
        let params = serde_json::json!({
            "command": command,
            "timeout": timeout.as_secs()
        });
        let result = tokio::time::timeout(
            timeout + Duration::from_secs(5),
            request(&mut stream, "exec", Some(params)),
        )
        .await
        .map_err(|_| AgentBoxError::Timeout("exec timed out".into()))??;

        Ok(ExecResult {
            stdout: result
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            stderr: result
                .get("stderr")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            exit_code: result
                .get("exit_code")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1) as i32,
        })
    }

    /// Execute a command with streaming output.
    ///
    /// Returns a receiver for output events and a sender for stdin data.
    pub async fn exec_stream(
        &self,
        command: &str,
    ) -> Result<(
        tokio::sync::mpsc::Receiver<ExecEvent>,
        tokio::sync::mpsc::Sender<Vec<u8>>,
    )> {
        let stream = self.connect().await?;
        let (mut reader, mut writer) = tokio::io::split(stream);

        let id = next_id();
        let req = serde_json::json!({
            "id": id,
            "method": "exec_stream",
            "params": { "command": command }
        });
        let payload = serde_json::to_vec(&req)?;
        writer
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;

        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<ExecEvent>(256);
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        // Reader task: read streaming messages from the guest agent
        tokio::spawn(async move {
            loop {
                let mut len_buf = [0u8; 4];
                if reader.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let msg_len = u32::from_be_bytes(len_buf) as usize;
                let mut buf = vec![0u8; msg_len];
                if reader.read_exact(&mut buf).await.is_err() {
                    break;
                }

                let msg: serde_json::Value = match serde_json::from_slice(&buf) {
                    Ok(v) => v,
                    Err(_) => break,
                };

                if let Some(stream_type) = msg.get("stream").and_then(|v| v.as_str()) {
                    let data = msg.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    let decoded = B64.decode(data).unwrap_or_default();
                    let event = match stream_type {
                        "stdout" => ExecEvent::Stdout(decoded),
                        "stderr" => ExecEvent::Stderr(decoded),
                        _ => continue,
                    };
                    if event_tx.send(event).await.is_err() {
                        break;
                    }
                } else if let Some(result) = msg.get("result") {
                    let exit_code = result
                        .get("exit_code")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(-1) as i32;
                    let _ = event_tx.send(ExecEvent::Exit(exit_code)).await;
                    break;
                } else if let Some(err) = msg.get("error").and_then(|v| v.as_str()) {
                    let _ = event_tx.send(ExecEvent::Error(err.to_string())).await;
                    break;
                }
            }
        });

        // Writer task: forward stdin data to the guest agent
        tokio::spawn(async move {
            while let Some(data) = stdin_rx.recv().await {
                let msg = serde_json::json!({
                    "id": id,
                    "method": "stdin",
                    "params": { "data": B64.encode(&data) }
                });
                let payload = match serde_json::to_vec(&msg) {
                    Ok(p) => p,
                    Err(_) => break,
                };
                if writer
                    .write_all(&(payload.len() as u32).to_be_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                if writer.write_all(&payload).await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        Ok((event_rx, stdin_tx))
    }

    /// Send a signal to a running process in the guest.
    pub async fn signal(&self, signal: i32) -> Result<()> {
        let mut stream = self.connect().await?;
        request(
            &mut stream,
            "signal",
            Some(serde_json::json!({ "signal": signal })),
        )
        .await?;
        Ok(())
    }

    /// Read a file from the guest. Returns raw bytes.
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let mut stream = self.connect().await?;
        let result = request(
            &mut stream,
            "read_file",
            Some(serde_json::json!({ "path": path })),
        )
        .await?;
        let content_b64 = result.get("content").and_then(|v| v.as_str()).unwrap_or("");
        B64.decode(content_b64)
            .map_err(|e| AgentBoxError::FileOp(e.to_string()))
    }

    /// Write a file to the guest.
    pub async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let mut stream = self.connect().await?;
        let content = B64.encode(data);
        request(
            &mut stream,
            "write_file",
            Some(serde_json::json!({
                "path": path,
                "content": content
            })),
        )
        .await?;
        Ok(())
    }

    /// List files in a directory on the guest.
    pub async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
        let mut stream = self.connect().await?;
        let result = request(
            &mut stream,
            "list_files",
            Some(serde_json::json!({ "path": path })),
        )
        .await?;
        let entries = result
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        entries
            .into_iter()
            .map(|e| {
                Ok(FileEntry {
                    name: e
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    size: e.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                    is_dir: e.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false),
                })
            })
            .collect()
    }
}

/// Send a length-prefixed JSON request and read a single length-prefixed JSON response.
async fn request(
    stream: &mut UnixStream,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
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
        return Err(AgentBoxError::ExecFailed(err.to_string()));
    }

    Ok(resp
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    /// Create a temporary UDS path for testing.
    fn temp_uds_path() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        // Leak the tempdir so it isn't cleaned up while the test runs.
        let path = dir.path().join("test.sock");
        std::mem::forget(dir);
        path
    }

    /// Read a line (until \n) from a stream byte-by-byte.
    async fn read_line(stream: &mut UnixStream) -> String {
        let mut buf = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).await.unwrap();
            buf.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        String::from_utf8(buf).unwrap()
    }

    /// Read a length-prefixed JSON message from the stream.
    async fn read_lp_message(stream: &mut UnixStream) -> serde_json::Value {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.unwrap();
        let msg_len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; msg_len];
        stream.read_exact(&mut buf).await.unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    /// Write a length-prefixed JSON message to the stream.
    async fn write_lp_message(stream: &mut UnixStream, msg: &serde_json::Value) {
        let payload = serde_json::to_vec(msg).unwrap();
        stream
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await
            .unwrap();
        stream.write_all(&payload).await.unwrap();
        stream.flush().await.unwrap();
    }

    /// Mock guest agent that handles the CONNECT handshake then processes one request.
    async fn mock_guest_agent(
        listener: UnixListener,
        handler: impl FnOnce(serde_json::Value) -> serde_json::Value + Send + 'static,
    ) {
        let (mut stream, _) = listener.accept().await.unwrap();

        // Read CONNECT line
        let connect_line = read_line(&mut stream).await;
        assert!(connect_line.starts_with("CONNECT "));

        // Send OK
        stream.write_all(b"OK 3\n").await.unwrap();

        // Read request
        let req = read_lp_message(&mut stream).await;

        // Send response
        let resp = handler(req);
        write_lp_message(&mut stream, &resp).await;
    }

    #[test]
    fn test_vsock_client_new() {
        let path = PathBuf::from("/tmp/test.sock");
        let client = VsockClient::new(path.clone(), 5000);
        assert_eq!(client.uds_path, path);
        assert_eq!(client.port, 5000);
    }

    #[tokio::test]
    async fn test_connect_handshake() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        // Spawn a mock server that accepts the CONNECT and responds OK
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let line = read_line(&mut stream).await;
            assert_eq!(line, "CONNECT 5000\n");
            stream.write_all(b"OK 3\n").await.unwrap();
        });

        let stream = client.connect().await;
        assert!(stream.is_ok());
    }

    #[tokio::test]
    async fn test_connect_handshake_reject() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _line = read_line(&mut stream).await;
            stream.write_all(b"ERR connection refused\n").await.unwrap();
        });

        let result = client.connect().await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("CONNECT failed"));
    }

    #[tokio::test]
    async fn test_ping() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(mock_guest_agent(listener, |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "ping");
            serde_json::json!({ "id": id, "result": { "status": "ok" } })
        }));

        let result = client.ping().await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_exec() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(mock_guest_agent(listener, |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "exec");
            let params = req.get("params").unwrap();
            assert_eq!(
                params.get("command").unwrap().as_str().unwrap(),
                "echo hello"
            );
            serde_json::json!({
                "id": id,
                "result": {
                    "stdout": "hello\n",
                    "stderr": "",
                    "exit_code": 0
                }
            })
        }));

        let result = client
            .exec("echo hello", Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_request_response_roundtrip() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(mock_guest_agent(listener, |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            let method = req.get("method").unwrap().as_str().unwrap().to_string();
            let params = req.get("params").cloned();
            // Echo the method and params back as the result
            serde_json::json!({
                "id": id,
                "result": {
                    "method": method,
                    "params": params,
                }
            })
        }));

        // Use the private connect + request to verify protocol encoding
        let mut stream = client.connect().await.unwrap();
        let result = request(
            &mut stream,
            "test_method",
            Some(serde_json::json!({"key": "value"})),
        )
        .await
        .unwrap();

        assert_eq!(
            result.get("method").unwrap().as_str().unwrap(),
            "test_method"
        );
    }

    #[tokio::test]
    async fn test_read_file() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);
        let file_content = b"hello world";

        tokio::spawn(mock_guest_agent(listener, move |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "read_file");
            let encoded = B64.encode(file_content);
            serde_json::json!({
                "id": id,
                "result": { "content": encoded }
            })
        }));

        let data = client.read_file("/workspace/test.txt").await.unwrap();
        assert_eq!(data, b"hello world");
    }

    #[tokio::test]
    async fn test_write_file() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(mock_guest_agent(listener, |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "write_file");
            let params = req.get("params").unwrap();
            assert_eq!(
                params.get("path").unwrap().as_str().unwrap(),
                "/workspace/out.txt"
            );
            // Verify content is valid base64
            let content = params.get("content").unwrap().as_str().unwrap();
            let decoded = B64.decode(content).unwrap();
            assert_eq!(decoded, b"file data");
            serde_json::json!({ "id": id, "result": {} })
        }));

        client
            .write_file("/workspace/out.txt", b"file data")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_list_files() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(mock_guest_agent(listener, |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "list_files");
            serde_json::json!({
                "id": id,
                "result": {
                    "entries": [
                        { "name": "file.txt", "size": 123, "is_dir": false },
                        { "name": "subdir", "size": 0, "is_dir": true },
                    ]
                }
            })
        }));

        let entries = client.list_files("/workspace").await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "file.txt");
        assert_eq!(entries[0].size, 123);
        assert!(!entries[0].is_dir);
        assert_eq!(entries[1].name, "subdir");
        assert!(entries[1].is_dir);
    }

    #[tokio::test]
    async fn test_signal() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(mock_guest_agent(listener, |req| {
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "signal");
            let params = req.get("params").unwrap();
            assert_eq!(params.get("signal").unwrap().as_i64().unwrap(), 2);
            serde_json::json!({ "id": id, "result": {} })
        }));

        client.signal(2).await.unwrap();
    }

    #[tokio::test]
    async fn test_exec_stream() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let client = VsockClient::new(sock_path, 5000);

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            // CONNECT handshake
            let _line = read_line(&mut stream).await;
            stream.write_all(b"OK 3\n").await.unwrap();

            // Read the exec_stream request
            let req = read_lp_message(&mut stream).await;
            let id = req.get("id").unwrap().as_u64().unwrap();
            assert_eq!(req.get("method").unwrap().as_str().unwrap(), "exec_stream");

            // Send streaming stdout
            let stdout_data = B64.encode(b"hello\n");
            write_lp_message(
                &mut stream,
                &serde_json::json!({ "id": id, "stream": "stdout", "data": stdout_data }),
            )
            .await;

            // Send streaming stderr
            let stderr_data = B64.encode(b"warn\n");
            write_lp_message(
                &mut stream,
                &serde_json::json!({ "id": id, "stream": "stderr", "data": stderr_data }),
            )
            .await;

            // Send exit
            write_lp_message(
                &mut stream,
                &serde_json::json!({ "id": id, "result": { "exit_code": 0 } }),
            )
            .await;
        });

        let (mut rx, _stdin_tx) = client.exec_stream("echo hello").await.unwrap();

        // Collect events
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            let is_exit = matches!(event, ExecEvent::Exit(_));
            events.push(event);
            if is_exit {
                break;
            }
        }

        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], ExecEvent::Stdout(d) if d == b"hello\n"));
        assert!(matches!(&events[1], ExecEvent::Stderr(d) if d == b"warn\n"));
        assert!(matches!(&events[2], ExecEvent::Exit(0)));
    }
}
