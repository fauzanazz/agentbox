use agentbox_core::sandbox::ExecResult;
use anyhow::Result;
use std::io::Write;

use crate::client::AgentBoxClient;

pub async fn run(
    client: &AgentBoxClient,
    id: &str,
    command: &[String],
    timeout: Option<u64>,
    json: bool,
) -> Result<i32> {
    let cmd = command.join(" ");
    let body = serde_json::json!({
        "command": cmd,
        "timeout": timeout,
    });
    let result: ExecResult = client
        .post_json(&format!("/sandboxes/{id}/exec"), &body)
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        if !result.stdout.is_empty() {
            std::io::stdout().write_all(result.stdout.as_bytes())?;
        }
        if !result.stderr.is_empty() {
            std::io::stderr().write_all(result.stderr.as_bytes())?;
        }
    }

    Ok(result.exit_code)
}
