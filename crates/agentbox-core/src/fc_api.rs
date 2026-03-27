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
