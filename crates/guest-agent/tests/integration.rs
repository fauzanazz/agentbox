use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;

#[derive(Debug, Serialize)]
struct Request {
    id: u64,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Response {
    id: u64,
    result: Option<Value>,
    error: Option<String>,
    // For stream messages
    stream: Option<String>,
    data: Option<String>,
}

async fn write_message<W: AsyncWriteExt + Unpin, T: Serialize>(
    writer: &mut W,
    msg: &T,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    let len = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_response<R: AsyncReadExt + Unpin>(reader: &mut R) -> std::io::Result<Response> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

struct TestServer {
    #[allow(dead_code)]
    child: tokio::process::Child,
    port: u16,
}

impl TestServer {
    async fn start() -> Self {
        // Find a free port
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let child = Command::new(env!("CARGO_BIN_EXE_guest-agent"))
            .args(["--tcp", "--port", &port.to_string()])
            .env("RUST_LOG", "debug")
            .kill_on_drop(true)
            .spawn()
            .expect("Failed to start guest-agent");

        // Wait for the server to start
        for _ in 0..50 {
            if TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .is_ok()
            {
                return TestServer { child, port };
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("Guest agent did not start in time");
    }

    async fn connect(&self) -> TcpStream {
        TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .await
            .expect("Failed to connect")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // kill_on_drop handles cleanup
    }
}

#[tokio::test]
async fn test_ping() {
    let server = TestServer::start().await;
    let mut stream = server.connect().await;

    let req = Request {
        id: 1,
        method: "ping".to_string(),
        params: None,
    };
    write_message(&mut stream, &req).await.unwrap();

    let resp = read_response(&mut stream).await.unwrap();
    assert_eq!(resp.id, 1);
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["status"], "ok");
}

#[tokio::test]
async fn test_exec() {
    let server = TestServer::start().await;
    let mut stream = server.connect().await;

    let req = Request {
        id: 2,
        method: "exec".to_string(),
        params: Some(serde_json::json!({"command": "echo integration_test"})),
    };
    write_message(&mut stream, &req).await.unwrap();

    let resp = read_response(&mut stream).await.unwrap();
    assert_eq!(resp.id, 2);
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["stdout"], "integration_test\n");
    assert_eq!(result["exit_code"], 0);
}

#[tokio::test]
async fn test_file_write_and_read() {
    let dir = tempfile::tempdir().unwrap();
    // Set workspace to temp dir so path validation passes in the spawned agent
    std::env::set_var("AGENTBOX_WORKSPACE_DIR", dir.path().to_str().unwrap());
    let server = TestServer::start().await;
    let mut stream = server.connect().await;

    let file_path = dir.path().join("integration_test.txt");
    let content = b"hello from integration test";
    let encoded = base64::engine::general_purpose::STANDARD.encode(content);

    // Write file
    let req = Request {
        id: 3,
        method: "write_file".to_string(),
        params: Some(serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "content": encoded,
        })),
    };
    write_message(&mut stream, &req).await.unwrap();

    let resp = read_response(&mut stream).await.unwrap();
    assert_eq!(resp.id, 3);
    assert!(resp.error.is_none());

    // Read file
    let req = Request {
        id: 4,
        method: "read_file".to_string(),
        params: Some(serde_json::json!({
            "path": file_path.to_str().unwrap(),
        })),
    };
    write_message(&mut stream, &req).await.unwrap();

    let resp = read_response(&mut stream).await.unwrap();
    assert_eq!(resp.id, 4);
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let read_content = base64::engine::general_purpose::STANDARD
        .decode(result["content"].as_str().unwrap())
        .unwrap();
    assert_eq!(read_content, content);
}

#[tokio::test]
async fn test_unknown_method() {
    let server = TestServer::start().await;
    let mut stream = server.connect().await;

    let req = Request {
        id: 5,
        method: "nonexistent".to_string(),
        params: None,
    };
    write_message(&mut stream, &req).await.unwrap();

    let resp = read_response(&mut stream).await.unwrap();
    assert_eq!(resp.id, 5);
    assert!(resp.error.is_some());
    assert!(resp.error.unwrap().contains("Unknown method"));
}
