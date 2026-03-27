use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/var/lib/agentbox/config.toml".to_string());

    let config = if std::path::Path::new(&config_path).exists() {
        agentbox_core::config::AgentBoxConfig::from_file(std::path::Path::new(&config_path))?
    } else {
        tracing::warn!("Config file not found at {config_path}, using defaults");
        agentbox_core::config::AgentBoxConfig::default()
    };

    agentbox_daemon::run_daemon(config, None).await
}
