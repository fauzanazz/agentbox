use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod handlers;
mod routes;
mod state;

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

    let listen_addr = config.daemon.listen.clone();

    let vm_manager = Arc::new(agentbox_core::vm::VmManager::new(config.vm.clone()));
    let pool = Arc::new(agentbox_core::pool::Pool::new(
        config.pool.clone(),
        config.guest.clone(),
        vm_manager,
    ));
    let _pool_handle = pool.start().await?;

    let state = Arc::new(state::AppState::new(pool.clone(), Arc::new(config)));

    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("AgentBox daemon listening on {listen_addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Shutting down...");
    pool.shutdown().await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }
    #[cfg(not(unix))]
    ctrl_c.await.ok();
}
