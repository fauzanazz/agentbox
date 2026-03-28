use std::path::Path;

use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::body::Bytes;
use hyperlocal::{UnixClientExt, Uri as UnixUri};

use crate::error::{AgentBoxError, Result};

/// Make an HTTP PUT request to the Firecracker API via Unix Domain Socket.
pub async fn fc_api_put(socket: &Path, path: &str, body: serde_json::Value) -> Result<()> {
    let client = hyper_util::client::legacy::Client::unix();
    let uri: hyper::Uri = UnixUri::new(socket, path).into();
    let body_bytes = serde_json::to_vec(&body)?;

    let req = hyper::Request::builder()
        .method("PUT")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .map_err(|e| AgentBoxError::ApiTransport(format!("failed to build request: {e}")))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| AgentBoxError::ApiTransport(format!("Firecracker API call failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = BodyExt::collect(resp.into_body())
            .await
            .map(|b| String::from_utf8_lossy(&b.to_bytes()).to_string())
            .unwrap_or_default();
        return Err(AgentBoxError::ApiTransport(format!(
            "FC API {status}: {body}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::server::conn::http1::Builder as Http1Builder;
    use hyper::service::service_fn;
    use hyper::{Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use tokio::net::UnixListener;
    use tokio::sync::oneshot;

    /// Create a temporary UDS path for testing.
    fn temp_uds_path() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fc.sock");
        std::mem::forget(dir);
        path
    }

    /// Spawn a one-shot HTTP server on a Unix socket that responds with the given
    /// status and body. Returns a channel that receives the captured request
    /// (method, uri, headers, body bytes) if `capture_tx` is provided.
    #[allow(clippy::type_complexity)]
    async fn mock_fc_server(
        listener: UnixListener,
        status: StatusCode,
        response_body: &'static str,
        capture_tx: Option<oneshot::Sender<(hyper::Method, String, hyper::HeaderMap, Vec<u8>)>>,
    ) {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);

        let capture_tx = std::sync::Arc::new(tokio::sync::Mutex::new(capture_tx));

        Http1Builder::new()
            .serve_connection(
                io,
                service_fn(move |req: Request<hyper::body::Incoming>| {
                    let capture_tx = capture_tx.clone();
                    async move {
                        let method = req.method().clone();
                        let uri = req.uri().to_string();
                        let headers = req.headers().clone();
                        let body_bytes = BodyExt::collect(req.into_body())
                            .await
                            .unwrap()
                            .to_bytes()
                            .to_vec();

                        if let Some(tx) = capture_tx.lock().await.take() {
                            let _ = tx.send((method, uri, headers, body_bytes));
                        }

                        Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(status)
                                .body(Full::new(Bytes::from(response_body)))
                                .unwrap(),
                        )
                    }
                }),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_fc_api_put_success_200() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        tokio::spawn(mock_fc_server(listener, StatusCode::OK, "", None));

        let result = fc_api_put(
            &sock_path,
            "/machine-config",
            serde_json::json!({"vcpu_count": 2}),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fc_api_put_success_204() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        tokio::spawn(mock_fc_server(listener, StatusCode::NO_CONTENT, "", None));

        let result = fc_api_put(
            &sock_path,
            "/actions",
            serde_json::json!({"action_type": "InstanceStart"}),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fc_api_put_error_400() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        tokio::spawn(mock_fc_server(
            listener,
            StatusCode::BAD_REQUEST,
            "invalid vcpu_count",
            None,
        ));

        let result = fc_api_put(
            &sock_path,
            "/machine-config",
            serde_json::json!({"vcpu_count": -1}),
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AgentBoxError::ApiTransport(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("FC API 400"),
            "expected 'FC API 400' in: {msg}"
        );
        assert!(
            msg.contains("invalid vcpu_count"),
            "expected error body in: {msg}"
        );
    }

    #[tokio::test]
    async fn test_fc_api_put_error_500() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        tokio::spawn(mock_fc_server(
            listener,
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal error",
            None,
        ));

        let result = fc_api_put(
            &sock_path,
            "/machine-config",
            serde_json::json!({"vcpu_count": 2}),
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AgentBoxError::ApiTransport(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("FC API 500"),
            "expected 'FC API 500' in: {msg}"
        );
    }

    #[tokio::test]
    async fn test_fc_api_put_connection_refused() {
        let sock_path = temp_uds_path();
        // No listener bound — connection should fail.

        let result = fc_api_put(
            &sock_path,
            "/machine-config",
            serde_json::json!({"vcpu_count": 2}),
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AgentBoxError::ApiTransport(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("Firecracker API call failed"),
            "expected 'Firecracker API call failed' in: {msg}"
        );
    }

    #[tokio::test]
    async fn test_fc_api_put_sends_correct_request() {
        let sock_path = temp_uds_path();
        let listener = UnixListener::bind(&sock_path).unwrap();

        let (tx, rx) = oneshot::channel();

        tokio::spawn(mock_fc_server(listener, StatusCode::OK, "", Some(tx)));

        let body = serde_json::json!({"vcpu_count": 4, "mem_size_mib": 256});
        fc_api_put(&sock_path, "/machine-config", body.clone())
            .await
            .unwrap();

        let (method, _uri, headers, req_body) = rx.await.unwrap();

        // Assert method is PUT
        assert_eq!(method, hyper::Method::PUT);

        // Assert Content-Type header
        assert_eq!(
            headers.get("content-type").unwrap().to_str().unwrap(),
            "application/json"
        );

        // Assert body matches the JSON we sent
        let received: serde_json::Value = serde_json::from_slice(&req_body).unwrap();
        assert_eq!(received, body);
    }
}
