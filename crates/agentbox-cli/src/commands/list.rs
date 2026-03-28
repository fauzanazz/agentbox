use agentbox_core::sandbox::SandboxInfo;
use anyhow::Result;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(client: &AgentBoxClient, output: &OutputMode) -> Result<()> {
    let sandboxes: Vec<SandboxInfo> = client.get_json("/sandboxes").await?;
    output.print_value(&sandboxes, || {
        if sandboxes.is_empty() {
            println!("No sandboxes found.");
            return;
        }
        println!(
            "{:<40} {:<12} {:>8} {:>6} CREATED",
            "ID", "STATUS", "MEMORY", "VCPUS"
        );
        for sb in &sandboxes {
            let status = format!("{:?}", sb.status);
            println!(
                "{:<40} {:<12} {:>6}MB {:>6} {}",
                sb.id, status, sb.config.memory_mb, sb.config.vcpus, sb.created_at
            );
        }
    })
}
