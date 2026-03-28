use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::io;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use agentbox_core::error::AgentBoxError;
use agentbox_core::vsock::VsockClient;

const MAX_CONNECTIONS_PER_FORWARD: usize = 128;
const MAX_FORWARDS_PER_SANDBOX: usize = 16;

pub struct PortForwardEntry {
    pub guest_port: u16,
    pub host_port: u16,
    pub listener_handle: JoinHandle<()>,
    pub cancel: CancellationToken,
}

impl PortForwardEntry {
    pub fn info(&self) -> PortForwardInfo {
        PortForwardInfo {
            guest_port: self.guest_port,
            host_port: self.host_port,
            local_address: format!("0.0.0.0:{}", self.host_port),
        }
    }

    pub fn stop(self) {
        self.cancel.cancel();
        self.listener_handle.abort();
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PortForwardInfo {
    pub guest_port: u16,
    pub host_port: u16,
    pub local_address: String,
}

pub async fn start_forward(
    vsock_uds_path: PathBuf,
    vsock_port: u32,
    guest_port: u16,
) -> Result<PortForwardEntry, AgentBoxError> {
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .map_err(|e| AgentBoxError::PortForward(format!("Failed to bind TCP listener: {e}")))?;

    let host_port = listener
        .local_addr()
        .map_err(|e| AgentBoxError::PortForward(format!("Failed to get local address: {e}")))?
        .port();

    let cancel = CancellationToken::new();
    let handle = tokio::spawn(accept_loop(
        listener,
        vsock_uds_path,
        vsock_port,
        guest_port,
        cancel.clone(),
    ));

    Ok(PortForwardEntry {
        guest_port,
        host_port,
        listener_handle: handle,
        cancel,
    })
}

pub fn max_forwards_per_sandbox() -> usize {
    MAX_FORWARDS_PER_SANDBOX
}

async fn accept_loop(
    listener: TcpListener,
    vsock_uds_path: PathBuf,
    vsock_port: u32,
    guest_port: u16,
    cancel: CancellationToken,
) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS_PER_FORWARD));

    loop {
        let tcp_stream = tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        tracing::debug!(
                            "Port forward: accepted TCP from {peer} for guest port {guest_port}"
                        );
                        stream
                    }
                    Err(e) => {
                        tracing::error!("Port forward accept error: {e}");
                        break;
                    }
                }
            }
            _ = cancel.cancelled() => {
                tracing::debug!("Port forward listener cancelled for guest port {guest_port}");
                break;
            }
        };

        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break, // semaphore closed
        };

        let uds_path = vsock_uds_path.clone();
        let child_cancel = cancel.child_token();

        tokio::spawn(async move {
            let _permit = permit;
            tokio::select! {
                result = proxy_connection(tcp_stream, uds_path, vsock_port, guest_port) => {
                    if let Err(e) = result {
                        tracing::debug!("Port forward proxy ended: {e}");
                    }
                }
                _ = child_cancel.cancelled() => {
                    tracing::debug!("Port forward proxy cancelled for guest port {guest_port}");
                }
            }
        });
    }
}

async fn proxy_connection(
    tcp_stream: tokio::net::TcpStream,
    vsock_uds_path: PathBuf,
    vsock_port: u32,
    guest_port: u16,
) -> Result<(), AgentBoxError> {
    let client = VsockClient::new(vsock_uds_path, vsock_port);
    let vsock_stream = client.open_port_forward(guest_port).await?;

    let (mut vsock_reader, mut vsock_writer) = io::split(vsock_stream);
    let (mut tcp_reader, mut tcp_writer) = io::split(tcp_stream);

    tokio::select! {
        r = io::copy(&mut tcp_reader, &mut vsock_writer) => {
            if let Err(e) = r {
                tracing::debug!("Port forward tcp->vsock ended: {e}");
            }
        }
        r = io::copy(&mut vsock_reader, &mut tcp_writer) => {
            if let Err(e) = r {
                tracing::debug!("Port forward vsock->tcp ended: {e}");
            }
        }
    }

    Ok(())
}
