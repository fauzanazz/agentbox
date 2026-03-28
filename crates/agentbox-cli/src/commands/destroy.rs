use anyhow::Result;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(client: &AgentBoxClient, id: &str, output: &OutputMode) -> Result<()> {
    let resp: serde_json::Value = client.delete_json(&format!("/sandboxes/{id}")).await?;
    output.print_value(&resp, || {
        println!("Sandbox {id} destroyed.");
    })
}
