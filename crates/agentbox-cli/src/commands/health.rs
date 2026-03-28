use anyhow::Result;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(client: &AgentBoxClient, output: &OutputMode) -> Result<()> {
    let resp: serde_json::Value = client.get_json("/health").await?;
    output.print_value(&resp, || {
        let status = resp["status"].as_str().unwrap_or("unknown");
        let active = resp["pool"]["active"].as_u64().unwrap_or(0);
        let max = resp["pool"]["max_size"].as_u64().unwrap_or(0);
        println!("Daemon is healthy ({status}). Active sandboxes: {active}, Pool max: {max}");
    })
}
