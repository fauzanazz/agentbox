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
    pub stream: String,
    pub data: String,
}

/// Read a length-prefixed JSON message from a reader.
/// Format: [4 bytes big-endian length][JSON payload]
pub async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> std::io::Result<Request> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Write a length-prefixed JSON message to a writer.
pub async fn write_message<W: AsyncWriteExt + Unpin, T: Serialize>(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_write_roundtrip() {
        // Write a request, read it back as a request
        let req_json = serde_json::json!({
            "id": 42,
            "method": "ping",
            "params": {"key": "value"}
        });
        let payload = serde_json::to_vec(&req_json).unwrap();
        let len = (payload.len() as u32).to_be_bytes();
        let mut buf = Vec::new();
        buf.extend_from_slice(&len);
        buf.extend_from_slice(&payload);

        let mut cursor = std::io::Cursor::new(buf);
        let request = read_message(&mut cursor).await.unwrap();
        assert_eq!(request.id, 42);
        assert_eq!(request.method, "ping");
        assert!(request.params.is_some());
    }

    #[tokio::test]
    async fn test_read_write_roundtrip_with_request() {
        let req_json = serde_json::json!({
            "id": 1,
            "method": "ping",
            "params": null
        });
        let payload = serde_json::to_vec(&req_json).unwrap();
        let len = (payload.len() as u32).to_be_bytes();
        let mut buf = Vec::new();
        buf.extend_from_slice(&len);
        buf.extend_from_slice(&payload);

        let mut cursor = std::io::Cursor::new(buf);
        let request = read_message(&mut cursor).await.unwrap();
        assert_eq!(request.id, 1);
        assert_eq!(request.method, "ping");
    }

    #[tokio::test]
    async fn test_read_invalid_json() {
        let invalid = b"not json at all";
        let len = (invalid.len() as u32).to_be_bytes();
        let mut buf = Vec::new();
        buf.extend_from_slice(&len);
        buf.extend_from_slice(invalid);

        let mut cursor = std::io::Cursor::new(buf);
        let result = read_message(&mut cursor).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn test_large_message() {
        let large_data = "x".repeat(100_000);
        let req_json = serde_json::json!({
            "id": 99,
            "method": "exec",
            "params": {"command": large_data}
        });
        let payload = serde_json::to_vec(&req_json).unwrap();
        let len = (payload.len() as u32).to_be_bytes();
        let mut buf = Vec::new();
        buf.extend_from_slice(&len);
        buf.extend_from_slice(&payload);

        let mut cursor = std::io::Cursor::new(buf);
        let request = read_message(&mut cursor).await.unwrap();
        assert_eq!(request.id, 99);
        assert_eq!(request.method, "exec");
    }
}
