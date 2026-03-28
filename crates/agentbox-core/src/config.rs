use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct AgentBoxConfig {
    pub daemon: DaemonConfig,
    pub vm: VmConfig,
    pub pool: PoolConfig,
    pub guest: GuestConfig,
    pub tls: TlsConfig,
    pub cors: CorsConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DaemonConfig {
    pub listen: String,
    pub log_level: String,
    pub api_key: Option<String>,
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
    pub disk_size_mb: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct PoolConfig {
    pub min_size: usize,
    pub max_size: usize,
    pub idle_timeout_secs: u64,
    pub network_min_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct GuestConfig {
    pub vsock_port: u32,
    pub ping_timeout_ms: u64,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct TlsConfig {
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
}

impl TlsConfig {
    /// Returns true if both cert and key paths are configured.
    pub fn is_configured(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CorsConfig {
    /// List of allowed origins. Use `["*"]` for permissive (development only).
    /// Default is empty, which means same-origin only.
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct RateLimitConfig {
    /// Max requests per second per IP address. 0 = disabled.
    pub requests_per_second: u64,
    /// Burst size (max tokens in bucket).
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 0,
            burst_size: 100,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".to_string(),
            log_level: "info".to_string(),
            api_key: None,
        }
    }
}

impl Default for VmDefaults {
    fn default() -> Self {
        Self {
            memory_mb: 2048,
            vcpus: 2,
            network: false,
            disk_size_mb: 512,
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
            network_min_size: 0,
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
        assert_eq!(p.network_min_size, 0);
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

    // ── TLS config ────────────────────────────────────────────────

    #[test]
    fn tls_default_is_unconfigured() {
        let tls = TlsConfig::default();
        assert!(!tls.is_configured());
        assert!(tls.cert_path.is_none());
        assert!(tls.key_path.is_none());
    }

    #[test]
    fn tls_is_configured_requires_both_paths() {
        let partial = TlsConfig {
            cert_path: Some(PathBuf::from("/etc/cert.pem")),
            key_path: None,
        };
        assert!(!partial.is_configured());

        let full = TlsConfig {
            cert_path: Some(PathBuf::from("/etc/cert.pem")),
            key_path: Some(PathBuf::from("/etc/key.pem")),
        };
        assert!(full.is_configured());
    }

    #[test]
    fn from_file_tls_section_parsed() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "[tls]\ncert_path = \"/etc/cert.pem\"\nkey_path = \"/etc/key.pem\"\n"
        )
        .unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert!(cfg.tls.is_configured());
        assert_eq!(cfg.tls.cert_path.unwrap(), PathBuf::from("/etc/cert.pem"));
    }

    #[test]
    fn from_file_without_tls_uses_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[daemon]\nlisten = \"0.0.0.0:9090\"\n").unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert!(!cfg.tls.is_configured());
    }

    // ── CORS config ───────────────────────────────────────────────

    #[test]
    fn cors_default_is_empty() {
        let cors = CorsConfig::default();
        assert!(cors.allowed_origins.is_empty());
    }

    #[test]
    fn from_file_cors_section_parsed() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "[cors]\nallowed_origins = [\"https://example.com\", \"https://app.example.com\"]\n"
        )
        .unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.cors.allowed_origins.len(), 2);
        assert_eq!(cfg.cors.allowed_origins[0], "https://example.com");
    }

    #[test]
    fn from_file_cors_wildcard() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[cors]\nallowed_origins = [\"*\"]\n").unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.cors.allowed_origins, vec!["*"]);
    }

    // ── Rate limit config ─────────────────────────────────────────

    #[test]
    fn rate_limit_default_is_disabled() {
        let rl = RateLimitConfig::default();
        assert_eq!(rl.requests_per_second, 0);
        assert_eq!(rl.burst_size, 100);
    }

    #[test]
    fn from_file_rate_limit_section_parsed() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "[rate_limit]\nrequests_per_second = 50\nburst_size = 200\n"
        )
        .unwrap();
        let cfg = AgentBoxConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.rate_limit.requests_per_second, 50);
        assert_eq!(cfg.rate_limit.burst_size, 200);
    }
}
