mod client;
mod commands;
mod output;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use client::AgentBoxClient;
use output::OutputMode;

#[derive(Parser)]
#[command(
    name = "agentbox",
    version,
    about = "Self-hosted sandbox infrastructure for AI agents"
)]
struct Cli {
    /// Daemon URL
    #[arg(
        long,
        global = true,
        env = "AGENTBOX_URL",
        default_value = "http://127.0.0.1:8080"
    )]
    host: String,

    /// Output as JSON instead of human-readable format
    #[arg(long, global = true)]
    json: bool,

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

    /// Create a new sandbox
    Create {
        /// Memory in MB
        #[arg(long)]
        memory: Option<u32>,

        /// Number of vCPUs
        #[arg(long)]
        vcpus: Option<u32>,

        /// Enable networking
        #[arg(long)]
        network: bool,

        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },

    /// List all sandboxes
    List,

    /// Get sandbox info
    Get {
        /// Sandbox ID
        id: String,
    },

    /// Destroy a sandbox
    Destroy {
        /// Sandbox ID
        id: String,
    },

    /// Execute a command in a sandbox
    Exec {
        /// Sandbox ID
        id: String,

        /// Command to execute
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,

        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },

    /// Upload a file to a sandbox
    Upload {
        /// Sandbox ID
        id: String,

        /// Local file path
        local: String,

        /// Remote path in sandbox
        #[arg(long, default_value = "/workspace/upload")]
        remote: String,
    },

    /// Download a file from a sandbox
    Download {
        /// Sandbox ID
        id: String,

        /// Remote path in sandbox
        remote: String,

        /// Local output path (defaults to stdout)
        #[arg(long, short)]
        output: Option<String>,
    },

    /// List files in a sandbox
    #[command(name = "ls")]
    Ls {
        /// Sandbox ID
        id: String,

        /// Path to list
        #[arg(default_value = "/workspace")]
        path: String,
    },

    /// Check daemon health
    Health,
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
        Commands::Exec {
            id,
            command,
            timeout,
        } => {
            let client = AgentBoxClient::new(&cli.host);
            let code = commands::exec::run(&client, &id, &command, timeout, cli.json).await?;
            std::process::exit(code);
        }
        other => {
            let client = AgentBoxClient::new(&cli.host);
            let output = OutputMode::new(cli.json);
            match other {
                Commands::Health => commands::health::run(&client, &output).await,
                Commands::Create {
                    memory,
                    vcpus,
                    network,
                    timeout,
                } => commands::create::run(&client, memory, vcpus, network, timeout, &output).await,
                Commands::List => commands::list::run(&client, &output).await,
                Commands::Get { id } => commands::get::run(&client, &id, &output).await,
                Commands::Destroy { id } => commands::destroy::run(&client, &id, &output).await,
                Commands::Upload { id, local, remote } => {
                    commands::upload::run(&client, &id, &local, &remote, &output).await
                }
                Commands::Download {
                    id,
                    remote,
                    output: out,
                } => commands::download::run(&client, &id, &remote, out.as_deref()).await,
                Commands::Ls { id, path } => commands::ls::run(&client, &id, &path, &output).await,
                Commands::Serve { .. } | Commands::Exec { .. } => unreachable!(),
            }
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
            _ => panic!("Expected Serve"),
        }
    }

    #[test]
    fn cli_parses_serve_with_listen() {
        let cli = Cli::try_parse_from(["agentbox", "serve", "--listen", "0.0.0.0:9090"]).unwrap();
        match cli.command {
            Commands::Serve { listen, .. } => assert_eq!(listen.unwrap(), "0.0.0.0:9090"),
            _ => panic!("Expected Serve"),
        }
    }

    #[test]
    fn cli_rejects_unknown_command() {
        assert!(Cli::try_parse_from(["agentbox", "unknown"]).is_err());
    }

    #[test]
    fn cli_parses_health() {
        let cli = Cli::try_parse_from(["agentbox", "health"]).unwrap();
        assert!(matches!(cli.command, Commands::Health));
    }

    #[test]
    fn cli_parses_create_with_flags() {
        let cli = Cli::try_parse_from([
            "agentbox",
            "create",
            "--memory",
            "4096",
            "--vcpus",
            "4",
            "--network",
        ])
        .unwrap();
        match cli.command {
            Commands::Create {
                memory,
                vcpus,
                network,
                ..
            } => {
                assert_eq!(memory, Some(4096));
                assert_eq!(vcpus, Some(4));
                assert!(network);
            }
            _ => panic!("Expected Create"),
        }
    }

    #[test]
    fn cli_parses_exec() {
        let cli = Cli::try_parse_from(["agentbox", "exec", "abc123", "ls", "-la"]).unwrap();
        match cli.command {
            Commands::Exec { id, command, .. } => {
                assert_eq!(id, "abc123");
                assert_eq!(command, vec!["ls", "-la"]);
            }
            _ => panic!("Expected Exec"),
        }
    }

    #[test]
    fn cli_parses_list() {
        let cli = Cli::try_parse_from(["agentbox", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::List));
    }

    #[test]
    fn cli_parses_get() {
        let cli = Cli::try_parse_from(["agentbox", "get", "abc123"]).unwrap();
        match cli.command {
            Commands::Get { id } => assert_eq!(id, "abc123"),
            _ => panic!("Expected Get"),
        }
    }

    #[test]
    fn cli_parses_destroy() {
        let cli = Cli::try_parse_from(["agentbox", "destroy", "abc123"]).unwrap();
        match cli.command {
            Commands::Destroy { id } => assert_eq!(id, "abc123"),
            _ => panic!("Expected Destroy"),
        }
    }

    #[test]
    fn cli_parses_upload() {
        let cli = Cli::try_parse_from([
            "agentbox",
            "upload",
            "abc123",
            "./file.txt",
            "--remote",
            "/workspace/file.txt",
        ])
        .unwrap();
        match cli.command {
            Commands::Upload { id, local, remote } => {
                assert_eq!(id, "abc123");
                assert_eq!(local, "./file.txt");
                assert_eq!(remote, "/workspace/file.txt");
            }
            _ => panic!("Expected Upload"),
        }
    }

    #[test]
    fn cli_parses_download() {
        let cli = Cli::try_parse_from([
            "agentbox",
            "download",
            "abc123",
            "/workspace/out.txt",
            "--output",
            "out.txt",
        ])
        .unwrap();
        match cli.command {
            Commands::Download {
                id, remote, output, ..
            } => {
                assert_eq!(id, "abc123");
                assert_eq!(remote, "/workspace/out.txt");
                assert_eq!(output, Some("out.txt".to_string()));
            }
            _ => panic!("Expected Download"),
        }
    }

    #[test]
    fn cli_parses_ls() {
        let cli = Cli::try_parse_from(["agentbox", "ls", "abc123", "/workspace/src"]).unwrap();
        match cli.command {
            Commands::Ls { id, path } => {
                assert_eq!(id, "abc123");
                assert_eq!(path, "/workspace/src");
            }
            _ => panic!("Expected Ls"),
        }
    }

    #[test]
    fn cli_global_host_flag() {
        let cli =
            Cli::try_parse_from(["agentbox", "--host", "http://10.0.0.5:9090", "health"]).unwrap();
        assert_eq!(cli.host, "http://10.0.0.5:9090");
    }

    #[test]
    fn cli_global_json_flag() {
        let cli = Cli::try_parse_from(["agentbox", "--json", "list"]).unwrap();
        assert!(cli.json);
    }
}
