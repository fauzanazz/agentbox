use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::config::VmConfig;
use crate::error::{AgentBoxError, Result};
use crate::fc_api::fc_api_put;
use crate::sandbox::SandboxConfig;

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub tap_device: String,
    pub host_ip: String,
    pub guest_ip: String,
    pub subnet_cidr: String,
}

#[derive(Debug)]
pub struct VmHandle {
    pub id: String,
    pub process: tokio::process::Child,
    pub api_socket: PathBuf,
    pub vsock_uds: PathBuf,
    pub work_dir: PathBuf,
    pub network: Option<NetworkInfo>,
}

pub struct VmManager {
    pub(crate) config: VmConfig,
    next_subnet_id: AtomicU32,
}

impl VmManager {
    pub fn new(config: VmConfig) -> Self {
        Self {
            config,
            next_subnet_id: AtomicU32::new(0),
        }
    }

    /// Create a VM from snapshot (fast path — <300ms).
    /// If `config.network` is true, falls back to fresh boot with TAP networking
    /// since the base snapshot does not include a network interface.
    pub async fn create_from_snapshot(&self, config: &SandboxConfig) -> Result<VmHandle> {
        if config.network {
            return self.create_fresh(config).await;
        }

        let vm_id = uuid::Uuid::new_v4().to_string()[..12].to_string();
        let work_dir = tempfile::tempdir()?.keep();

        // 1. CoW copy rootfs
        let rootfs_dest = work_dir.join("rootfs.ext4");
        cow_copy(&self.config.rootfs_path, &rootfs_dest).await?;

        // 2. Spawn Firecracker process
        let api_socket = work_dir.join("api.sock");
        let process = tokio::process::Command::new(&self.config.firecracker_bin)
            .arg("--api-sock")
            .arg("api.sock")
            .current_dir(&work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AgentBoxError::VmCreation(format!("Failed to spawn firecracker: {e}")))?;

        // 3. Wait for API socket
        wait_for_socket(&api_socket, Duration::from_secs(5)).await?;

        // 4. Restore snapshot
        restore_snapshot(&api_socket, &self.config.snapshot_path).await?;

        let vsock_uds = work_dir.join("vsock.sock");

        Ok(VmHandle {
            id: vm_id,
            process,
            api_socket,
            vsock_uds,
            work_dir,
            network: None,
        })
    }

    /// Fresh-boot a VM with full Firecracker configuration (slower than snapshot restore).
    /// Used when networking is required, since the base snapshot has no network interface.
    async fn create_fresh(&self, config: &SandboxConfig) -> Result<VmHandle> {
        let vm_id = uuid::Uuid::new_v4().to_string()[..12].to_string();
        let work_dir = tempfile::tempdir()?.keep();

        let rootfs_dest = work_dir.join("rootfs.ext4");
        cow_copy(&self.config.rootfs_path, &rootfs_dest).await?;

        // Set up host networking before configuring Firecracker
        let network = if config.network {
            Some(self.setup_host_network(&vm_id).await?)
        } else {
            None
        };

        let api_socket = work_dir.join("api.sock");
        let process = tokio::process::Command::new(&self.config.firecracker_bin)
            .arg("--api-sock")
            .arg("api.sock")
            .current_dir(&work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AgentBoxError::VmCreation(format!("Failed to spawn firecracker: {e}")))?;

        if let Err(e) = self
            .configure_and_boot(&vm_id, &api_socket, config, &network)
            .await
        {
            // Clean up TAP device on failure; process is dropped automatically
            if let Some(ref net) = network {
                teardown_host_network(net).await;
            }
            return Err(e);
        }

        let vsock_uds = work_dir.join("vsock.sock");

        Ok(VmHandle {
            id: vm_id,
            process,
            api_socket,
            vsock_uds,
            work_dir,
            network,
        })
    }

    /// Configure Firecracker via API and boot the VM.
    async fn configure_and_boot(
        &self,
        vm_id: &str,
        api_socket: &std::path::Path,
        config: &SandboxConfig,
        network: &Option<NetworkInfo>,
    ) -> Result<()> {
        wait_for_socket(api_socket, Duration::from_secs(5)).await?;

        let kernel_path = self
            .config
            .kernel_path
            .canonicalize()
            .map_err(|e| AgentBoxError::VmCreation(format!("kernel path not found: {e}")))?;

        fc_api_put(
            api_socket,
            "/boot-source",
            serde_json::json!({
                "kernel_image_path": kernel_path.to_str().unwrap(),
                "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
            }),
        )
        .await?;

        fc_api_put(
            api_socket,
            "/drives/rootfs",
            serde_json::json!({
                "drive_id": "rootfs",
                "path_on_host": "rootfs.ext4",
                "is_root_device": true,
                "is_read_only": false
            }),
        )
        .await?;

        fc_api_put(
            api_socket,
            "/vsock",
            serde_json::json!({
                "guest_cid": 3,
                "uds_path": "vsock.sock"
            }),
        )
        .await?;

        if let Some(ref net) = network {
            fc_api_put(
                api_socket,
                "/network-interfaces/eth0",
                serde_json::json!({
                    "iface_id": "eth0",
                    "guest_mac": generate_mac(vm_id),
                    "host_dev_name": &net.tap_device
                }),
            )
            .await?;
        }

        fc_api_put(
            api_socket,
            "/machine-config",
            serde_json::json!({
                "vcpu_count": config.vcpus,
                "mem_size_mib": config.memory_mb
            }),
        )
        .await?;

        fc_api_put(
            api_socket,
            "/actions",
            serde_json::json!({"action_type": "InstanceStart"}),
        )
        .await?;

        Ok(())
    }

    /// Create a TAP device and configure host networking for a VM.
    async fn setup_host_network(&self, vm_id: &str) -> Result<NetworkInfo> {
        let subnet_id = self.next_subnet_id.fetch_add(1, Ordering::Relaxed);
        let tap_name = format!("tap_{}", &vm_id[..8]);

        let base = subnet_id * 4;
        let third_octet = base / 256;
        let fourth_base = base % 256;

        let host_ip = format!("172.16.{}.{}", third_octet, fourth_base + 1);
        let guest_ip = format!("172.16.{}.{}", third_octet, fourth_base + 2);
        let subnet_cidr = format!("172.16.{}.{}/30", third_octet, fourth_base);

        run_cmd("ip", &["tuntap", "add", "dev", &tap_name, "mode", "tap"])
            .await
            .map_err(|e| AgentBoxError::VmCreation(format!("TAP create failed: {e}")))?;

        run_cmd(
            "ip",
            &["addr", "add", &format!("{host_ip}/30"), "dev", &tap_name],
        )
        .await
        .map_err(|e| AgentBoxError::VmCreation(format!("TAP configure failed: {e}")))?;

        run_cmd("ip", &["link", "set", &tap_name, "up"])
            .await
            .map_err(|e| AgentBoxError::VmCreation(format!("TAP up failed: {e}")))?;

        // Idempotent — safe to call multiple times
        let _ = run_cmd("sysctl", &["-w", "net.ipv4.ip_forward=1"]).await;

        run_cmd(
            "iptables",
            &[
                "-t",
                "nat",
                "-A",
                "POSTROUTING",
                "-s",
                &subnet_cidr,
                "-j",
                "MASQUERADE",
            ],
        )
        .await
        .map_err(|e| AgentBoxError::VmCreation(format!("NAT rule failed: {e}")))?;

        Ok(NetworkInfo {
            tap_device: tap_name,
            host_ip,
            guest_ip,
            subnet_cidr,
        })
    }

    /// Destroy a VM (kill process, cleanup network + files).
    pub async fn destroy(&self, mut vm: VmHandle) -> Result<()> {
        if let Some(ref net) = vm.network {
            teardown_host_network(net).await;
        }
        let _ = vm.process.kill().await;
        let _ = tokio::time::timeout(Duration::from_secs(5), vm.process.wait()).await;
        let _ = tokio::fs::remove_dir_all(&vm.work_dir).await;
        tracing::info!(vm_id = %vm.id, "VM destroyed");
        Ok(())
    }

    /// Check if VM process is still running.
    pub fn is_running(vm: &mut VmHandle) -> bool {
        matches!(vm.process.try_wait(), Ok(None))
    }
}

/// Copy a file using CoW (reflink) when supported, falling back to regular copy.
async fn cow_copy(src: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    let status = tokio::process::Command::new("cp")
        .arg("--reflink=auto")
        .arg(src)
        .arg(dest)
        .status()
        .await;

    match status {
        Ok(s) if s.success() => Ok(()),
        _ => {
            // Fallback: regular copy (macOS dev, or cp doesn't support --reflink)
            tokio::fs::copy(src, dest)
                .await
                .map_err(|e| AgentBoxError::VmCreation(format!("failed to copy rootfs: {e}")))?;
            Ok(())
        }
    }
}

/// Poll for a Unix socket to appear on disk, with timeout.
async fn wait_for_socket(path: &std::path::Path, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        if path.exists() {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(AgentBoxError::VmCreation(
                "API socket did not appear within timeout".to_string(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Restore a Firecracker snapshot by calling the Firecracker REST API.
async fn restore_snapshot(
    api_socket: &std::path::Path,
    snapshot_dir: &std::path::Path,
) -> Result<()> {
    let vmstate = snapshot_dir
        .join("vmstate.bin")
        .canonicalize()
        .map_err(|e| {
            AgentBoxError::SnapshotLoad(format!("vmstate.bin not found in {:?}: {e}", snapshot_dir))
        })?;
    let memory = snapshot_dir
        .join("memory.bin")
        .canonicalize()
        .map_err(|e| {
            AgentBoxError::SnapshotLoad(format!("memory.bin not found in {:?}: {e}", snapshot_dir))
        })?;

    let vmstate_str = vmstate
        .to_str()
        .ok_or_else(|| AgentBoxError::SnapshotLoad("vmstate path is not valid UTF-8".into()))?;
    let memory_str = memory
        .to_str()
        .ok_or_else(|| AgentBoxError::SnapshotLoad("memory path is not valid UTF-8".into()))?;

    let body = serde_json::json!({
        "snapshot_path": vmstate_str,
        "mem_backend": {
            "backend_path": memory_str,
            "backend_type": "File"
        },
        "enable_diff_snapshots": false,
        "resume_vm": true
    });

    fc_api_put(api_socket, "/snapshot/load", body).await
}

/// Remove a TAP device and its associated iptables NAT rule (best-effort).
async fn teardown_host_network(net: &NetworkInfo) {
    let _ = run_cmd(
        "iptables",
        &[
            "-t",
            "nat",
            "-D",
            "POSTROUTING",
            "-s",
            &net.subnet_cidr,
            "-j",
            "MASQUERADE",
        ],
    )
    .await;
    let _ = run_cmd("ip", &["link", "del", &net.tap_device]).await;
}

/// Run a shell command and return an error if it fails.
async fn run_cmd(cmd: &str, args: &[&str]) -> std::result::Result<(), String> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("{cmd} failed to execute: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{cmd} {} failed: {stderr}", args.join(" ")));
    }
    Ok(())
}

/// Generate a locally-administered MAC address from a VM ID.
fn generate_mac(vm_id: &str) -> String {
    let bytes: Vec<u8> = vm_id.as_bytes().iter().copied().take(3).collect();
    format!(
        "AA:FC:00:{:02x}:{:02x}:{:02x}",
        bytes.first().copied().unwrap_or(0),
        bytes.get(1).copied().unwrap_or(0),
        bytes.get(2).copied().unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VmDefaults;

    #[test]
    fn test_vm_manager_new() {
        let config = VmConfig {
            firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
            kernel_path: PathBuf::from("/var/lib/agentbox/vmlinux"),
            rootfs_path: PathBuf::from("/var/lib/agentbox/rootfs.ext4"),
            snapshot_path: PathBuf::from("/var/lib/agentbox/snapshot"),
            defaults: VmDefaults::default(),
        };
        let manager = VmManager::new(config);
        assert_eq!(
            manager.config.firecracker_bin,
            PathBuf::from("/usr/bin/firecracker")
        );
        assert_eq!(
            manager.config.snapshot_path,
            PathBuf::from("/var/lib/agentbox/snapshot")
        );
    }

    #[tokio::test]
    async fn test_wait_for_socket_timeout() {
        let path = PathBuf::from("/tmp/nonexistent-agentbox-test-socket.sock");
        let result = wait_for_socket(&path, Duration::from_millis(100)).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("API socket did not appear"));
    }

    #[tokio::test]
    async fn test_wait_for_socket_success() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let sock_path_clone = sock_path.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tokio::fs::write(&sock_path_clone, b"").await.unwrap();
        });

        let result = wait_for_socket(&sock_path, Duration::from_secs(2)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cow_copy_creates_destination_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.txt");
        let dest = dir.path().join("destination.txt");

        tokio::fs::write(&src, b"hello agentbox").await.unwrap();

        let result = cow_copy(&src, &dest).await;
        assert!(result.is_ok());

        let content = tokio::fs::read_to_string(&dest).await.unwrap();
        assert_eq!(content, "hello agentbox");
    }

    #[tokio::test]
    async fn test_cow_copy_nonexistent_source_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("nonexistent.txt");
        let dest = dir.path().join("destination.txt");

        let result = cow_copy(&src, &dest).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_destroy_cleans_up_work_dir() {
        let work_dir = tempfile::tempdir().unwrap().keep();
        assert!(work_dir.exists());

        let process = tokio::process::Command::new("sleep")
            .arg("3600")
            .kill_on_drop(true)
            .spawn()
            .unwrap();

        let vm = VmHandle {
            id: "test-destroy".to_string(),
            process,
            api_socket: work_dir.join("api.sock"),
            vsock_uds: work_dir.join("vsock.sock"),
            work_dir: work_dir.clone(),
            network: None,
        };

        let manager = VmManager::new(VmConfig::default());
        let result = manager.destroy(vm).await;
        assert!(result.is_ok());
        assert!(!work_dir.exists());
    }

    #[tokio::test]
    async fn test_is_running_true_for_live_process() {
        let process = tokio::process::Command::new("sleep")
            .arg("3600")
            .kill_on_drop(true)
            .spawn()
            .unwrap();

        let mut vm = VmHandle {
            id: "test-running".to_string(),
            process,
            api_socket: PathBuf::from("/tmp/test-running.sock"),
            vsock_uds: PathBuf::from("/tmp/test-running-vsock.sock"),
            work_dir: PathBuf::from("/tmp"),
            network: None,
        };

        assert!(VmManager::is_running(&mut vm));

        // Clean up: kill the process
        let _ = vm.process.kill().await;
    }

    #[tokio::test]
    async fn test_is_running_false_after_kill() {
        let process = tokio::process::Command::new("sleep")
            .arg("3600")
            .kill_on_drop(true)
            .spawn()
            .unwrap();

        let mut vm = VmHandle {
            id: "test-killed".to_string(),
            process,
            api_socket: PathBuf::from("/tmp/test-killed.sock"),
            vsock_uds: PathBuf::from("/tmp/test-killed-vsock.sock"),
            work_dir: PathBuf::from("/tmp"),
            network: None,
        };

        vm.process.kill().await.unwrap();
        vm.process.wait().await.unwrap();

        assert!(!VmManager::is_running(&mut vm));
    }
}
