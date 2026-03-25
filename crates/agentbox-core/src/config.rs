use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct AgentBoxConfig {
    pub daemon: DaemonConfig,
    pub vm: VmConfig,
    pub pool: PoolConfig,
    pub guest: GuestConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DaemonConfig {
    pub listen: String,
    pub log_level: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct VmConfig {
    pub firecracker_bin: PathBuf,
    pub kernel_path: PathBuf,
    pub rootfs_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub defaults: VmDefaults,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct VmDefaults {
    pub memory_mb: u32,
    pub vcpus: u32,
    pub network: bool,
    pub timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct PoolConfig {
    pub min_size: usize,
    pub max_size: usize,
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct GuestConfig {
    pub vsock_port: u32,
    pub ping_timeout_ms: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".to_string(),
            log_level: "info".to_string(),
        }
    }
}

impl Default for VmDefaults {
    fn default() -> Self {
        Self {
            memory_mb: 2048,
            vcpus: 2,
            network: false,
            timeout_secs: 3600,
        }
    }
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            firecracker_bin: PathBuf::from("/usr/local/bin/firecracker"),
            kernel_path: PathBuf::from("/var/lib/agentbox/vmlinux"),
            rootfs_path: PathBuf::from("/var/lib/agentbox/rootfs.ext4"),
            snapshot_path: PathBuf::from("/var/lib/agentbox/snapshot"),
            defaults: VmDefaults::default(),
        }
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_size: 2,
            max_size: 10,
            idle_timeout_secs: 3600,
        }
    }
}

impl Default for GuestConfig {
    fn default() -> Self {
        Self {
            vsock_port: 5000,
            ping_timeout_ms: 5000,
        }
    }
}

impl AgentBoxConfig {
    pub fn from_file(path: &std::path::Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| crate::error::AgentBoxError::Config(e.to_string()))
    }
}
