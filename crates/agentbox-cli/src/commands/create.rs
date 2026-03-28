use agentbox_core::sandbox::SandboxInfo;
use anyhow::Result;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(
    client: &AgentBoxClient,
    memory: Option<u32>,
    vcpus: Option<u32>,
    network: bool,
    timeout: Option<u64>,
    output: &OutputMode,
) -> Result<()> {
    let body = serde_json::json!({
        "memory_mb": memory,
        "vcpus": vcpus,
        "network": network,
        "timeout": timeout,
    });
    let info: SandboxInfo = client.post_json("/sandboxes", &body).await?;
    output.print_value(&info, || {
        println!("Sandbox created: {}", info.id);
        println!("  Memory: {} MB", info.config.memory_mb);
        println!("  vCPUs:  {}", info.config.vcpus);
        println!("  Status: {:?}", info.status);
    })
}
