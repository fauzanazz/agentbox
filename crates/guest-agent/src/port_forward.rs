use serde_json::Value;
use tokio::io::{self, AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::protocol::{write_message, Response};

pub async fn handle_port_forward_connect<R, W>(
    id: u64,
    params: Option<Value>,
    mut vsock_reader: R,
    mut vsock_writer: W,
) where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    let port = match params
        .as_ref()
        .and_then(|p| p.get("port"))
        .and_then(|v| v.as_u64())
    {
        Some(p) => p as u16,
        None => {
            let resp = Response {
                id,
                result: None,
                error: Some("Missing 'port' parameter".into()),
            };
            let _ = write_message(&mut vsock_writer, &resp).await;
            return;
        }
    };

    let target_stream = match TcpStream::connect(format!("127.0.0.1:{port}")).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("Port forward: failed to connect to 127.0.0.1:{port}: {e}");
            let resp = Response {
                id,
                result: None,
                error: Some(format!("Failed to connect to guest port {port}")),
            };
            let _ = write_message(&mut vsock_writer, &resp).await;
            return;
        }
    };

    let resp = Response {
        id,
        result: Some(serde_json::json!({"status": "connected"})),
        error: None,
    };
    if let Err(e) = write_message(&mut vsock_writer, &resp).await {
        tracing::error!("Failed to write port forward response: {e}");
        return;
    }

    let (mut target_reader, mut target_writer) = io::split(target_stream);

    tokio::select! {
        r = io::copy(&mut vsock_reader, &mut target_writer) => {
            if let Err(e) = r {
                tracing::debug!("Port forward vsock->target ended: {e}");
            }
        }
        r = io::copy(&mut target_reader, &mut vsock_writer) => {
            if let Err(e) = r {
                tracing::debug!("Port forward target->vsock ended: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn read_lp_message<R: AsyncReadExt + Unpin>(r: &mut R) -> Value {
        let mut len_buf = [0u8; 4];
        r.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf).await.unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    #[tokio::test]
    async fn missing_port_param_returns_error() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_reader, server_writer) = tokio::io::split(server);
        let (mut client_reader, _client_writer) = tokio::io::split(client);

        tokio::spawn(handle_port_forward_connect(
            1,
            None,
            server_reader,
            server_writer,
        ));

        let resp = read_lp_message(&mut client_reader).await;
        assert_eq!(resp["id"], 1);
        assert!(resp["error"].as_str().unwrap().contains("Missing"));
    }

    #[tokio::test]
    async fn connect_to_closed_port_returns_error() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_reader, server_writer) = tokio::io::split(server);
        let (mut client_reader, _client_writer) = tokio::io::split(client);

        let params = Some(serde_json::json!({"port": 19999}));
        tokio::spawn(handle_port_forward_connect(
            2,
            params,
            server_reader,
            server_writer,
        ));

        let resp = read_lp_message(&mut client_reader).await;
        assert_eq!(resp["id"], 2);
        assert!(resp["error"]
            .as_str()
            .unwrap()
            .contains("Failed to connect to guest port"));
    }

    #[tokio::test]
    async fn successful_port_forward_proxies_data() {
        // Start a TCP echo server on a random port
        let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_port = echo_listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = echo_listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                stream.write_all(&buf[..n]).await.unwrap();
            }
        });

        let (client, server) = tokio::io::duplex(4096);
        let (server_reader, server_writer) = tokio::io::split(server);

        let params = Some(serde_json::json!({"port": echo_port}));
        tokio::spawn(handle_port_forward_connect(
            3,
            params,
            server_reader,
            server_writer,
        ));

        // Read the handshake response
        let (mut client_reader, mut client_writer) = tokio::io::split(client);
        let resp = read_lp_message(&mut client_reader).await;
        assert_eq!(resp["id"], 3);
        assert_eq!(resp["result"]["status"], "connected");

        // Now send raw data through the tunnel
        client_writer.write_all(b"hello").await.unwrap();
        client_writer.flush().await.unwrap();

        let mut buf = [0u8; 5];
        client_reader.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }
}
