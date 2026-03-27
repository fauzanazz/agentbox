use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "agentbox",
    version,
    about = "Self-hosted sandbox infrastructure for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the AgentBox daemon
    Serve {
        /// Path to config file
        #[arg(long, default_value = "/var/lib/agentbox/config.toml")]
        config: String,

        /// Override listen address (e.g. 127.0.0.1:8080)
        #[arg(long)]
        listen: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config, listen } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
                )
                .init();

            let cfg = if std::path::Path::new(&config).exists() {
                agentbox_core::config::AgentBoxConfig::from_file(std::path::Path::new(&config))?
            } else {
                tracing::warn!("Config file not found at {config}, using defaults");
                agentbox_core::config::AgentBoxConfig::default()
            };

            agentbox_daemon::run_daemon(cfg, listen).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_serve() {
        let cli = Cli::try_parse_from(["agentbox", "serve"]).unwrap();
        assert!(matches!(cli.command, Commands::Serve { .. }));
    }

    #[test]
    fn cli_parses_serve_with_config() {
        let cli = Cli::try_parse_from(["agentbox", "serve", "--config", "/tmp/test.toml"]).unwrap();
        match cli.command {
            Commands::Serve { config, .. } => assert_eq!(config, "/tmp/test.toml"),
        }
    }

    #[test]
    fn cli_parses_serve_with_listen() {
        let cli = Cli::try_parse_from(["agentbox", "serve", "--listen", "0.0.0.0:9090"]).unwrap();
        match cli.command {
            Commands::Serve { listen, .. } => assert_eq!(listen.unwrap(), "0.0.0.0:9090"),
        }
    }

    #[test]
    fn cli_rejects_unknown_command() {
        assert!(Cli::try_parse_from(["agentbox", "unknown"]).is_err());
    }
}
