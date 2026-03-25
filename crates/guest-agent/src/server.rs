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
