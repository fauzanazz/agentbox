use anyhow::{Context, Result};
use reqwest::multipart;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(
    client: &AgentBoxClient,
    id: &str,
    local: &str,
    remote: &str,
    output: &OutputMode,
) -> Result<()> {
    let data = tokio::fs::read(local)
        .await
        .with_context(|| format!("Failed to read local file: {local}"))?;

    let file_part = multipart::Part::bytes(data).file_name(
        std::path::Path::new(local)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
    );
    let form = multipart::Form::new()
        .text("path", remote.to_string())
        .part("file", file_part);

    let resp: serde_json::Value = client
        .post_multipart(&format!("/sandboxes/{id}/files"), form)
        .await?;

    output.print_value(&resp, || {
        let path = resp["path"].as_str().unwrap_or(remote);
        let size = resp["size"].as_u64().unwrap_or(0);
        println!("Uploaded {path} ({size} bytes)");
    })
}
