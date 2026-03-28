use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/var/lib/agentbox/config.toml".to_string());

    // Load config BEFORE tracing init so we can use config.daemon.log_level.
    // This is pure file I/O + TOML parsing — no logging needed.
    let (config, config_warning) = if std::path::Path::new(&config_path).exists() {
        match agentbox_core::config::AgentBoxConfig::from_file(std::path::Path::new(&config_path)) {
            Ok(c) => (c, None),
            Err(e) => (
                agentbox_core::config::AgentBoxConfig::default(),
                Some(format!(
                    "Failed to parse config at {config_path}: {e}. Using defaults."
                )),
            ),
        }
    } else {
        (
            agentbox_core::config::AgentBoxConfig::default(),
            Some(format!(
                "Config file not found at {config_path}, using defaults"
            )),
        )
    };

    // RUST_LOG env var takes precedence; otherwise use config value
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.daemon.log_level));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    if let Some(warning) = config_warning {
        tracing::warn!("{warning}");
    }

    agentbox_daemon::run_daemon(config, None).await
}
