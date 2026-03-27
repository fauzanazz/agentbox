use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::{read_message, write_message, Response};
use crate::{exec, files};

pub async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(stream: S) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    loop {
        let request = match read_message(&mut reader).await {
            Ok(req) => req,
            Err(e) => {
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
                exec::handle_exec_stream(request.id, request.params, &mut writer).await;
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Write a length-prefixed JSON request to a writer.
    async fn send_request<W: AsyncWriteExt + Unpin>(
        w: &mut W,
        id: u64,
        method: &str,
        params: Option<Value>,
    ) {
        let req = serde_json::json!({ "id": id, "method": method, "params": params });
        let payload = serde_json::to_vec(&req).unwrap();
        let len = (payload.len() as u32).to_be_bytes();
        w.write_all(&len).await.unwrap();
        w.write_all(&payload).await.unwrap();
        w.flush().await.unwrap();
    }

    /// Read a length-prefixed JSON response from a reader.
    async fn recv_response<R: AsyncReadExt + Unpin>(r: &mut R) -> Value {
        let mut len_buf = [0u8; 4];
        r.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf).await.unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    // ── Ping ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn ping_returns_status_ok() {
        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(&mut w, 1, "ping", None).await;
        let resp = recv_response(&mut r).await;

        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["status"], "ok");
        assert!(resp.get("error").is_none() || resp["error"].is_null());

        drop(w);
        drop(r);
        handle.await.unwrap();
    }

    // ── Unknown method ───────────────────────────────────────────

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(&mut w, 7, "nonexistent", None).await;
        let resp = recv_response(&mut r).await;

        assert_eq!(resp["id"], 7);
        assert_eq!(resp["error"], "Unknown method: nonexistent");

        drop(w);
        drop(r);
        handle.await.unwrap();
    }

    // ── Multiple sequential requests ─────────────────────────────

    #[tokio::test]
    async fn multiple_requests_on_same_connection() {
        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(&mut w, 1, "ping", None).await;
        let r1 = recv_response(&mut r).await;
        assert_eq!(r1["id"], 1);

        send_request(&mut w, 2, "ping", None).await;
        let r2 = recv_response(&mut r).await;
        assert_eq!(r2["id"], 2);

        drop(w);
        drop(r);
        handle.await.unwrap();
    }

    // ── EOF closes cleanly ───────────────────────────────────────

    #[tokio::test]
    async fn eof_closes_connection_cleanly() {
        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(&mut w, 1, "ping", None).await;
        let _ = recv_response(&mut r).await;

        // Reunite and drop the whole stream to trigger EOF on server side
        let client = r.unsplit(w);
        drop(client);

        handle.await.unwrap(); // should return without panic
    }

    // ── Exec integration ─────────────────────────────────────────

    #[tokio::test]
    async fn exec_echo_returns_stdout() {
        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(
            &mut w,
            1,
            "exec",
            Some(serde_json::json!({"command":"echo test"})),
        )
        .await;
        let resp = recv_response(&mut r).await;

        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["stdout"], "test\n");
        assert_eq!(resp["result"]["exit_code"], 0);

        drop(w);
        drop(r);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn exec_missing_params_returns_error() {
        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(&mut w, 1, "exec", None).await;
        let resp = recv_response(&mut r).await;

        assert_eq!(resp["id"], 1);
        assert!(resp["error"].as_str().unwrap().contains("Missing"));

        drop(w);
        drop(r);
        handle.await.unwrap();
    }

    // ── File operations ──────────────────────────────────────────

    #[tokio::test]
    async fn write_then_read_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt").to_str().unwrap().to_string();
        let content = base64::engine::general_purpose::STANDARD.encode(b"hello world");

        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        // Write
        send_request(
            &mut w,
            1,
            "write_file",
            Some(serde_json::json!({"path": path, "content": content})),
        )
        .await;
        let write_resp = recv_response(&mut r).await;
        assert!(write_resp.get("error").is_none() || write_resp["error"].is_null());

        // Read back
        send_request(
            &mut w,
            2,
            "read_file",
            Some(serde_json::json!({"path": path})),
        )
        .await;
        let read_resp = recv_response(&mut r).await;
        assert_eq!(read_resp["result"]["content"], content);

        drop(w);
        drop(r);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn list_files_on_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let dir_path = dir.path().to_str().unwrap().to_string();

        let (client, server) = tokio::io::duplex(4096);
        let handle = tokio::spawn(handle_connection(server));
        let (mut r, mut w) = tokio::io::split(client);

        send_request(
            &mut w,
            1,
            "list_files",
            Some(serde_json::json!({"path": dir_path})),
        )
        .await;
        let resp = recv_response(&mut r).await;
        let entries = resp["result"]["entries"].as_array().unwrap();
        let names: Vec<&str> = entries
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.txt"));

        drop(w);
        drop(r);
        handle.await.unwrap();
    }
}
