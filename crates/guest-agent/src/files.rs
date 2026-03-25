use base64::Engine;
use serde_json::Value;
use std::path::Path;

use crate::protocol::Response;

pub async fn handle_read(id: u64, params: Option<Value>) -> Response {
    let path = match params
        .as_ref()
        .and_then(|p| p.get("path"))
        .and_then(|v| v.as_str())
    {
        Some(p) => p.to_string(),
        None => {
            return Response {
                id,
                result: None,
                error: Some("Missing 'path' parameter".to_string()),
            }
        }
    };

    match tokio::fs::read(&path).await {
        Ok(content) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
            Response {
                id,
                result: Some(serde_json::json!({ "content": encoded })),
                error: None,
            }
        }
        Err(e) => Response {
            id,
            result: None,
            error: Some(format!("Failed to read file: {e}")),
        },
    }
}

pub async fn handle_write(id: u64, params: Option<Value>) -> Response {
    let params = match params {
        Some(p) => p,
        None => {
            return Response {
                id,
                result: None,
                error: Some("Missing params".to_string()),
            }
        }
    };

    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            return Response {
                id,
                result: None,
                error: Some("Missing 'path' parameter".to_string()),
            }
        }
    };

    let content_b64 = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            return Response {
                id,
                result: None,
                error: Some("Missing 'content' parameter".to_string()),
            }
        }
    };

    let decoded = match base64::engine::general_purpose::STANDARD.decode(&content_b64) {
        Ok(d) => d,
        Err(e) => {
            return Response {
                id,
                result: None,
                error: Some(format!("Invalid base64 content: {e}")),
            }
        }
    };

    if let Some(parent) = Path::new(&path).parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Response {
                id,
                result: None,
                error: Some(format!("Failed to create parent directories: {e}")),
            };
        }
    }

    let bytes_written = decoded.len();
    match tokio::fs::write(&path, &decoded).await {
        Ok(()) => Response {
            id,
            result: Some(serde_json::json!({ "bytes_written": bytes_written })),
            error: None,
        },
        Err(e) => Response {
            id,
            result: None,
            error: Some(format!("Failed to write file: {e}")),
        },
    }
}

pub async fn handle_list(id: u64, params: Option<Value>) -> Response {
    let path = params
        .as_ref()
        .and_then(|p| p.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("/workspace")
        .to_string();

    let mut entries = Vec::new();
    let mut dir = match tokio::fs::read_dir(&path).await {
        Ok(d) => d,
        Err(e) => {
            return Response {
                id,
                result: None,
                error: Some(format!("Failed to read directory: {e}")),
            }
        }
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        entries.push(serde_json::json!({
            "name": name,
            "size": metadata.len(),
            "is_dir": metadata.is_dir(),
        }));
    }

    Response {
        id,
        result: Some(serde_json::json!({ "entries": entries })),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let dir = temp_dir();
        let file_path = dir.path().join("test.txt");
        let content = b"hello world";
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);

        let write_params = Some(serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "content": encoded,
        }));
        let resp = handle_write(1, write_params).await;
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["bytes_written"], content.len());

        let read_params = Some(serde_json::json!({
            "path": file_path.to_str().unwrap(),
        }));
        let resp = handle_read(2, read_params).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let read_content = base64::engine::general_purpose::STANDARD
            .decode(result["content"].as_str().unwrap())
            .unwrap();
        assert_eq!(read_content, content);
    }

    #[tokio::test]
    async fn test_read_nonexistent() {
        let params = Some(serde_json::json!({
            "path": "/tmp/nonexistent_agentbox_test_file_xyz.txt",
        }));
        let resp = handle_read(3, params).await;
        assert!(resp.error.is_some());
        assert!(resp.error.unwrap().contains("Failed to read file"));
    }

    #[tokio::test]
    async fn test_list_files() {
        let dir = temp_dir();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();

        let params = Some(serde_json::json!({
            "path": dir.path().to_str().unwrap(),
        }));
        let resp = handle_list(4, params).await;
        assert!(resp.error.is_none());
        let entries = resp.result.unwrap()["entries"].as_array().unwrap().clone();
        assert_eq!(entries.len(), 3);

        let names: Vec<String> = entries
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"a.txt".to_string()));
        assert!(names.contains(&"b.txt".to_string()));
        assert!(names.contains(&"subdir".to_string()));

        let subdir_entry = entries.iter().find(|e| e["name"] == "subdir").unwrap();
        assert_eq!(subdir_entry["is_dir"], true);
    }

    #[tokio::test]
    async fn test_write_creates_dirs() {
        let dir = temp_dir();
        let nested_path = dir.path().join("a/b/c/test.txt");
        let content = b"nested";
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);

        let params = Some(serde_json::json!({
            "path": nested_path.to_str().unwrap(),
            "content": encoded,
        }));
        let resp = handle_write(5, params).await;
        assert!(resp.error.is_none());

        let written = fs::read(&nested_path).unwrap();
        assert_eq!(written, content);
    }
}
