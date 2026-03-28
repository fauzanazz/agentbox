mod exec;
mod files;
mod path_security;
mod port_forward;
mod protocol;
mod server;

use tokio::net::TcpListener;

/// Bring up the loopback interface so port forwarding (which connects to
/// 127.0.0.1 inside the guest) works even when full networking is disabled.
#[cfg(target_os = "linux")]
fn bring_up_loopback() {
    use std::ffi::CString;
    use std::mem;

    unsafe {
        let sock = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if sock < 0 {
            tracing::warn!("Failed to create socket for loopback setup");
            return;
        }

        let ifname = CString::new("lo").unwrap();
        let mut ifr: libc::ifreq = mem::zeroed();
        libc::strncpy(
            ifr.ifr_name.as_mut_ptr(),
            ifname.as_ptr(),
            libc::IFNAMSIZ - 1,
        );

        // Get current flags
        if libc::ioctl(sock, libc::SIOCGIFFLAGS as _, &mut ifr) < 0 {
            tracing::warn!("Failed to get loopback flags: {}", std::io::Error::last_os_error());
            libc::close(sock);
            return;
        }

        // Set IFF_UP
        ifr.ifr_ifru.ifru_flags |= libc::IFF_UP as i16;
        if libc::ioctl(sock, libc::SIOCSIFFLAGS as _, &ifr) < 0 {
            tracing::warn!("Failed to bring up loopback: {}", std::io::Error::last_os_error());
        } else {
            tracing::info!("Loopback interface (lo) is up");
        }

        libc::close(sock);
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    #[cfg(target_os = "linux")]
    bring_up_loopback();

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
        use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
        use tokio::io::unix::AsyncFd;

        const AF_VSOCK: libc::c_int = 40;
        const VMADDR_CID_ANY: u32 = u32::MAX;

        #[repr(C)]
        struct SockaddrVm {
            svm_family: u16,
            svm_reserved1: u16,
            svm_port: u32,
            svm_cid: u32,
            svm_zero: [u8; 4],
        }

        let fd = unsafe { libc::socket(AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
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

        /// Wrapper to make a raw fd usable with tokio's AsyncFd.
        struct VsockListener(RawFd);
        impl AsRawFd for VsockListener {
            fn as_raw_fd(&self) -> RawFd {
                self.0
            }
        }
        impl Drop for VsockListener {
            fn drop(&mut self) {
                unsafe { libc::close(self.0) };
            }
        }

        let listener = AsyncFd::new(VsockListener(fd))?;
        tracing::info!("Guest agent listening on vsock port {port}");

        let shutdown = tokio::signal::ctrl_c();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                guard = listener.readable() => {
                    let mut guard = guard?;
                    let conn_fd = unsafe {
                        libc::accept4(
                            fd,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            libc::SOCK_NONBLOCK,
                        )
                    };
                    if conn_fd < 0 {
                        let err = std::io::Error::last_os_error();
                        if err.kind() == std::io::ErrorKind::WouldBlock {
                            guard.clear_ready();
                            continue;
                        }
                        tracing::error!("Accept error: {err}");
                        continue;
                    }
                    guard.clear_ready();
                    tracing::debug!("Accepted vsock connection (fd={conn_fd})");
                    // Wrap accepted vsock fd as a UnixStream for async I/O
                    let std_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(conn_fd) };
                    match tokio::net::UnixStream::from_std(std_stream) {
                        Ok(stream) => {
                            tokio::spawn(server::handle_connection(stream));
                        }
                        Err(e) => {
                            tracing::error!("Failed to wrap vsock stream: {e}");
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
