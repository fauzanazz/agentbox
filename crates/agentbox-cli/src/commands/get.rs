use agentbox_core::sandbox::SandboxInfo;
use anyhow::Result;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(client: &AgentBoxClient, id: &str, output: &OutputMode) -> Result<()> {
    let info: SandboxInfo = client.get_json(&format!("/sandboxes/{id}")).await?;
    output.print_value(&info, || {
        println!("ID:      {}", info.id);
        println!("Status:  {:?}", info.status);
        println!("Memory:  {} MB", info.config.memory_mb);
        println!("vCPUs:   {}", info.config.vcpus);
        println!("Network: {}", info.config.network);
        println!("Timeout: {}s", info.config.timeout_secs);
        println!("Created: {}", info.created_at);
    })
}
