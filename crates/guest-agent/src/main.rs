mod exec;
mod files;
mod path_security;
mod port_forward;
mod protocol;
mod server;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mut port: u16 = 5000;
    let mut tcp_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                if i < args.len() {
                    port = args[i].parse().expect("Invalid port number");
                }
            }
            "--tcp" => {
                tcp_mode = true;
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if tcp_mode {
        run_tcp(port).await
    } else {
        run_vsock(port).await
    }
}

async fn run_tcp(port: u16) -> std::io::Result<()> {
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Guest agent listening on TCP {addr}");

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        tracing::debug!("Accepted connection from {peer}");
                        tokio::spawn(server::handle_connection(stream));
                    }
                    Err(e) => {
                        tracing::error!("Accept error: {e}");
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn run_vsock(port: u16) -> std::io::Result<()> {
    // vsock requires Linux with KVM — use raw socket API
    // AF_VSOCK = 40, VMADDR_CID_ANY = u32::MAX
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::FromRawFd;

        const AF_VSOCK: libc::c_int = 40;
        const VMADDR_CID_ANY: u32 = u32::MAX;

        let fd = unsafe { libc::socket(AF_VSOCK, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        // sockaddr_vm layout: family (2 bytes), reserved (2 bytes), port (4 bytes), cid (4 bytes)
        #[repr(C)]
        struct SockaddrVm {
            svm_family: u16,
            svm_reserved1: u16,
            svm_port: u32,
            svm_cid: u32,
            svm_zero: [u8; 4],
        }

        let addr = SockaddrVm {
            svm_family: AF_VSOCK as u16,
            svm_reserved1: 0,
            svm_port: port as u32,
            svm_cid: VMADDR_CID_ANY,
            svm_zero: [0; 4],
        };

        let ret = unsafe {
            libc::bind(
                fd,
                &addr as *const SockaddrVm as *const libc::sockaddr,
                std::mem::size_of::<SockaddrVm>() as libc::socklen_t,
            )
        };
        if ret < 0 {
            unsafe { libc::close(fd) };
            return Err(std::io::Error::last_os_error());
        }

        let ret = unsafe { libc::listen(fd, 128) };
        if ret < 0 {
            unsafe { libc::close(fd) };
            return Err(std::io::Error::last_os_error());
        }

        // Set non-blocking for tokio
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let std_listener = unsafe { std::net::TcpListener::from_raw_fd(fd) };
        let listener = TcpListener::from_std(std_listener)?;
        tracing::info!("Guest agent listening on vsock port {port}");

        let shutdown = tokio::signal::ctrl_c();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            tracing::debug!("Accepted vsock connection");
                            tokio::spawn(server::handle_connection(stream));
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {e}");
                        }
                    }
                }
                _ = &mut shutdown => {
                    tracing::info!("Shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = port;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "vsock is only supported on Linux. Use --tcp for development.",
        ))
    }
}
