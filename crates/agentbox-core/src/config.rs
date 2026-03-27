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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AgentBoxError;
    use std::io::Write;

    // ── Default values ───────────────────────────────────────────

    #[test]
    fn defaults_daemon() {
        let d = DaemonConfig::default();
        assert_eq!(d.listen, "127.0.0.1:8080");
        assert_eq!(d.log_level, "info");
    }

    #[test]
    fn defaults_vm() {
        let v = VmConfig::default();
        assert_eq!(
            v.firecracker_bin,
            PathBuf::from("/usr/local/bin/firecracker")
        );
        assert_eq!(v.kernel_path, PathBuf::from("/var/lib/agentbox/vmlinux"));
        assert_eq!(
            v.rootfs_path,
            PathBuf::from("/var/lib/agentbox/rootfs.ext4")
        );
        assert_eq!(v.snapshot_path, PathBuf::from("/var/lib/agentbox/snapshot"));
        assert_eq!(v.defaults.memory_mb, 2048);
        assert_eq!(v.defaults.vcpus, 2);
        assert!(!v.defaults.network);
        assert_eq!(v.defaults.timeout_secs, 3600);
    }

    #[test]
    fn defaults_pool() {
        let p = PoolConfig::default();
        assert_eq!(p.min_size, 2);
        assert_eq!(p.max_size, 10);
        assert_eq!(p.idle_timeout_secs, 3600);
    }

    #[test]
    fn defaults_guest() {
        let g = GuestConfig::default();
        assert_eq!(g.vsock_port, 5000);
        assert_eq!(g.ping_timeout_ms, 5000);
    }

    #[test]
    fn defaults_composite() {
        let c = AgentBoxConfig::default();
        assert_eq!(c.daemon.listen, DaemonConfig::default().listen);
        assert_eq!(c.vm.defaults.memory_mb, VmDefaults::default().memory_mb);
        assert_eq!(c.pool.max_size, PoolConfig::default().max_size);
        assert_eq!(c.guest.vsock_port, GuestConfig::default().vsock_port);
    }

    // ── from_file happy paths ────────────────────────────────────

    #[test]
    fn from_file_empty_toml_yields_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "").unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.daemon.listen, "127.0.0.1:8080");
        assert_eq!(cfg.pool.max_size, 10);
    }

    #[test]
    fn from_file_partial_override() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[daemon]\nlisten = \"0.0.0.0:9090\"\n").unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.daemon.listen, "0.0.0.0:9090");
        assert_eq!(cfg.daemon.log_level, "info"); // untouched default
        assert_eq!(cfg.pool.max_size, 10); // other section at default
    }

    #[test]
    fn from_file_nested_partial_override() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[vm.defaults]\nmemory_mb = 4096\n").unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.vm.defaults.memory_mb, 4096);
        assert_eq!(cfg.vm.defaults.vcpus, 2); // untouched default
    }

    // ── from_file error paths ────────────────────────────────────

    #[test]
    fn from_file_nonexistent_returns_io_error() {
        let result = AgentBoxConfig::from_file(std::path::Path::new(
            "/tmp/does_not_exist_agentbox_test.toml",
        ));
        assert!(matches!(result, Err(AgentBoxError::Io(_))));
    }

    #[test]
    fn from_file_invalid_toml_returns_config_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[daemon\nlisten = \"bad\"").unwrap();
        let result = AgentBoxConfig::from_file(f.path());
        assert!(matches!(result, Err(AgentBoxError::Config(_))));
    }

    #[test]
    fn from_file_wrong_type_returns_config_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[pool]\nmin_size = \"not_a_number\"\n").unwrap();
        let result = AgentBoxConfig::from_file(f.path());
        assert!(matches!(result, Err(AgentBoxError::Config(_))));
    }
}
