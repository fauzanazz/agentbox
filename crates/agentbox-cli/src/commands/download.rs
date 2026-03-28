use anyhow::{Context, Result};
use std::io::Write;

use crate::client::AgentBoxClient;

pub async fn run(
    client: &AgentBoxClient,
    id: &str,
    remote: &str,
    output_path: Option<&str>,
) -> Result<()> {
    let data = client
        .get_bytes(&format!("/sandboxes/{id}/files"), &[("path", remote)])
        .await?;

    if let Some(path) = output_path {
        tokio::fs::write(path, &data)
            .await
            .with_context(|| format!("Failed to write to {path}"))?;
        eprintln!("Downloaded {remote} ({} bytes) -> {path}", data.len());
    } else {
        std::io::stdout().write_all(&data)?;
    }

    Ok(())
}
