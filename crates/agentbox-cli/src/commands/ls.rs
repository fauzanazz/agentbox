use agentbox_core::sandbox::FileEntry;
use anyhow::Result;

use crate::client::AgentBoxClient;
use crate::output::OutputMode;

pub async fn run(client: &AgentBoxClient, id: &str, path: &str, output: &OutputMode) -> Result<()> {
    let entries: Vec<FileEntry> = client
        .get_json_with_query(
            &format!("/sandboxes/{id}/files"),
            &[("list", "true"), ("path", path)],
        )
        .await?;

    output.print_value(&entries, || {
        if entries.is_empty() {
            println!("(empty directory)");
            return;
        }
        println!("{:<5} {:>10} NAME", "TYPE", "SIZE");
        for e in &entries {
            let kind = if e.is_dir { "dir" } else { "file" };
            let size = if e.is_dir {
                "-".to_string()
            } else {
                format_size(e.size)
            };
            let name = if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            println!("{:<5} {:>10} {}", kind, size, name);
        }
    })
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
