use std::sync::Arc;

use agentbox_core::config::AgentBoxConfig;

pub mod handlers;
pub mod port_forward;
pub mod routes;
pub mod state;
pub mod ws;

/// Run the AgentBox daemon with the given config.
pub async fn run_daemon(
    config: AgentBoxConfig,
    listen_override: Option<String>,
) -> anyhow::Result<()> {
    let listen_addr = listen_override.unwrap_or_else(|| config.daemon.listen.clone());

    let vm_manager = Arc::new(agentbox_core::vm::VmManager::new(config.vm.clone()));
    let pool = Arc::new(agentbox_core::pool::Pool::new(
        config.pool.clone(),
        config.guest.clone(),
        vm_manager,
    ));
    let _pool_handle = pool.start().await?;

    let tls_config = config.tls.clone();
    let state = Arc::new(state::AppState::new(pool.clone(), Arc::new(config)));

    let app = routes::build_router(state);

    if tls_config.is_configured() {
        let cert_path = tls_config.cert_path.as_ref().unwrap();
        let key_path = tls_config.key_path.as_ref().unwrap();

        let rustls_config =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;

        let addr: std::net::SocketAddr = listen_addr.parse()?;
        tracing::info!("AgentBox daemon listening on {listen_addr} (TLS)");

        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            shutdown_handle.graceful_shutdown(None);
        });

        axum_server::bind_rustls(addr, rustls_config)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
        tracing::info!("AgentBox daemon listening on {listen_addr}");

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    }

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
