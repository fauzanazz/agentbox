# CLI Management Tool

## Context

The CLI binary provides management commands for AgentBox and the ability to start
the daemon. Uses clap for argument parsing. When `--url` is not specified, it
talks to the daemon via HTTP on localhost:8080.

This task assumes the daemon HTTP API exists from FAU-72.
See `docs/architecture.md` for the CLI command reference.

## Requirements

- `agentbox serve` — start the daemon
- `agentbox list` — list active sandboxes
- `agentbox exec <sandbox-id> "command"` — run a command in a sandbox
- `agentbox stop <sandbox-id>` — destroy a sandbox
- `agentbox status` — health check + pool stats
- `agentbox version` — print version
- `--url` flag for remote daemon connection
- Pretty-printed output for humans

## Implementation

### `crates/agentbox-cli/Cargo.toml`

Verify/update (should exist from scaffold FAU-67):
```toml
[package]
name = "agentbox-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "agentbox"
path = "src/main.rs"

[dependencies]
agentbox-core = { path = "../agentbox-core" }
agentbox-daemon = { path = "../agentbox-daemon" }
clap = { version = "4", features = ["derive"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
reqwest = { version = "0.12", features = ["json"] }
tabled = "0.17"
anyhow = "1"
```

Note: Add `agentbox-daemon` as dependency so `serve` can embed the daemon.
The daemon's `main.rs` logic needs to be extractable as a library function.

### Make daemon startable from CLI

**Modify `crates/agentbox-daemon/src/lib.rs`** (create this file):
```rust
pub mod handlers;
pub mod routes;
pub mod state;
pub mod ws;

use std::sync::Arc;

pub async fn run_daemon(config: agentbox_core::AgentBoxConfig) -> anyhow::Result<()> {
    let listen_addr = config.daemon.listen.clone();
    let vm_manager = Arc::new(agentbox_core::VmManager::new(config.vm.clone()));
    let pool = Arc::new(agentbox_core::Pool::new(
        config.pool.clone(), config.guest.clone(), vm_manager,
    ));
    let _pool_handle = pool.start().await?;

    let state = Arc::new(state::AppState::new(pool.clone(), Arc::new(config)));
    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("AgentBox daemon listening on {listen_addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    pool.shutdown().await?;
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
```

Update `crates/agentbox-daemon/Cargo.toml` to also be a library:
```toml
[lib]
name = "agentbox_daemon"
path = "src/lib.rs"

[[bin]]
name = "agentbox-daemon"
path = "src/main.rs"
```

Update `crates/agentbox-daemon/src/main.rs` to use the lib:
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info".into()))
        .init();
    let config = agentbox_core::AgentBoxConfig::default();
    agentbox_daemon::run_daemon(config).await
}
```

### `crates/agentbox-cli/src/main.rs`

```rust
use clap::{Parser, Subcommand};

mod client;
mod commands;

#[derive(Parser)]
#[command(name = "agentbox", about = "Self-hosted sandbox infrastructure for AI agents")]
#[command(version)]
struct Cli {
    /// AgentBox daemon URL (default: http://localhost:8080)
    #[arg(long, global = true, env = "AGENTBOX_URL")]
    url: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the AgentBox daemon
    Serve {
        /// Config file path
        #[arg(long, default_value = "/var/lib/agentbox/config.toml")]
        config: String,
        /// Listen address override
        #[arg(long)]
        port: Option<u16>,
    },
    /// List active sandboxes
    List,
    /// Execute a command in a sandbox
    Exec {
        /// Sandbox ID
        id: String,
        /// Command to execute
        command: String,
        /// Timeout in seconds
        #[arg(long, default_value = "30")]
        timeout: u64,
    },
    /// Destroy a sandbox
    Stop {
        /// Sandbox ID
        id: String,
    },
    /// Show daemon health and pool status
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config, port } => commands::serve::run(config, port).await,
        Commands::List => commands::list::run(&cli.url).await,
        Commands::Exec { id, command, timeout } => commands::exec::run(&cli.url, &id, &command, timeout).await,
        Commands::Stop { id } => commands::stop::run(&cli.url, &id).await,
        Commands::Status => commands::status::run(&cli.url).await,
    }
}
```

### `crates/agentbox-cli/src/client.rs`

```rust
use reqwest::Client;
use serde::de::DeserializeOwned;

pub struct DaemonClient {
    client: Client,
    base_url: String,
}

impl DaemonClient {
    pub fn new(url: &Option<String>) -> Self {
        let base_url = url.clone()
            .or_else(|| std::env::var("AGENTBOX_URL").ok())
            .unwrap_or_else(|| "http://localhost:8080".to_string());
        Self {
            client: Client::new(),
            base_url,
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let resp = self.client.get(format!("{}{path}", self.base_url)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {status}: {body}");
        }
        Ok(resp.json().await?)
    }

    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> anyhow::Result<T> {
        let resp = self.client.post(format!("{}{path}", self.base_url))
            .json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {status}: {body}");
        }
        Ok(resp.json().await?)
    }

    pub async fn delete(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self.client.delete(format!("{}{path}", self.base_url)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {status}: {body}");
        }
        Ok(resp.json().await?)
    }
}
```

### `crates/agentbox-cli/src/commands/mod.rs`

```rust
pub mod serve;
pub mod list;
pub mod exec;
pub mod stop;
pub mod status;
```

### `crates/agentbox-cli/src/commands/serve.rs`

```rust
pub async fn run(config_path: String, port: Option<u16>) -> anyhow::Result<()> {
    let mut config = if std::path::Path::new(&config_path).exists() {
        agentbox_core::AgentBoxConfig::from_file(std::path::Path::new(&config_path))?
    } else {
        tracing::warn!("Config not found at {config_path}, using defaults");
        agentbox_core::AgentBoxConfig::default()
    };

    if let Some(p) = port {
        config.daemon.listen = format!("127.0.0.1:{p}");
    }

    println!("Starting AgentBox daemon on {}", config.daemon.listen);
    agentbox_daemon::run_daemon(config).await
}
```

### `crates/agentbox-cli/src/commands/list.rs`

```rust
use crate::client::DaemonClient;
use tabled::{Table, Tabled};

#[derive(Tabled, serde::Deserialize)]
struct SandboxRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Created")]
    created_at: String,
}

pub async fn run(url: &Option<String>) -> anyhow::Result<()> {
    let client = DaemonClient::new(url);
    let sandboxes: Vec<SandboxRow> = client.get("/sandboxes").await?;

    if sandboxes.is_empty() {
        println!("No active sandboxes.");
    } else {
        let table = Table::new(&sandboxes).to_string();
        println!("{table}");
    }
    Ok(())
}
```

### `crates/agentbox-cli/src/commands/exec.rs`

```rust
use crate::client::DaemonClient;

pub async fn run(url: &Option<String>, id: &str, command: &str, timeout: u64) -> anyhow::Result<()> {
    let client = DaemonClient::new(url);
    let result: serde_json::Value = client.post(
        &format!("/sandboxes/{id}/exec"),
        serde_json::json!({"command": command, "timeout": timeout}),
    ).await?;

    if let Some(stdout) = result.get("stdout").and_then(|v| v.as_str()) {
        if !stdout.is_empty() { print!("{stdout}"); }
    }
    if let Some(stderr) = result.get("stderr").and_then(|v| v.as_str()) {
        if !stderr.is_empty() { eprint!("{stderr}"); }
    }
    if let Some(code) = result.get("exit_code").and_then(|v| v.as_i64()) {
        if code != 0 {
            std::process::exit(code as i32);
        }
    }
    Ok(())
}
```

### `crates/agentbox-cli/src/commands/stop.rs`

```rust
use crate::client::DaemonClient;

pub async fn run(url: &Option<String>, id: &str) -> anyhow::Result<()> {
    let client = DaemonClient::new(url);
    client.delete(&format!("/sandboxes/{id}")).await?;
    println!("Sandbox {id} destroyed.");
    Ok(())
}
```

### `crates/agentbox-cli/src/commands/status.rs`

```rust
use crate::client::DaemonClient;

pub async fn run(url: &Option<String>) -> anyhow::Result<()> {
    let client = DaemonClient::new(url);
    let health: serde_json::Value = client.get("/health").await?;

    println!("AgentBox Daemon Status");
    println!("======================");
    println!("Status: {}", health.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"));

    if let Some(pool) = health.get("pool") {
        println!("\nPool:");
        println!("  Available: {}", pool.get("available").and_then(|v| v.as_u64()).unwrap_or(0));
        println!("  Active:    {}", pool.get("active").and_then(|v| v.as_u64()).unwrap_or(0));
        println!("  Max:       {}", pool.get("max").and_then(|v| v.as_u64()).unwrap_or(0));
    }
    Ok(())
}
```

## Testing Strategy

Run tests: `cargo test -p agentbox-cli`

### Unit tests:
- `test_cli_parsing` — verify clap parses all subcommands correctly
- `test_daemon_client_url_resolution` — verify URL priority: flag > env > default

### Integration tests (need daemon running):
- `test_status_command` — run status against live daemon, verify output
- Mark with `#[ignore]`

## Out of Scope

- Direct core library usage for local operations (always go through HTTP for MVP)
- Shell completions
- Interactive mode
- Logs command (would need daemon-side log storage)
