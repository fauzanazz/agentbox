use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(pub String);

impl std::fmt::Display for SandboxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub memory_mb: u32,
    pub vcpus: u32,
    pub network: bool,
    pub disk_size_mb: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub id: SandboxId,
    pub status: SandboxStatus,
    pub config: SandboxConfig,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxStatus {
    Creating,
    Ready,
    Busy,
    Destroying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug)]
pub enum ExecEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Debug)]
pub struct Sandbox {
    pub id: SandboxId,
    pub vm: crate::vm::VmHandle,
    pub vsock: crate::vsock::VsockClient,
    pub config: SandboxConfig,
    created_at: Instant,
}

impl Sandbox {
    pub fn new(
        vm: crate::vm::VmHandle,
        config: SandboxConfig,
        guest_config: &crate::config::GuestConfig,
    ) -> Self {
        let id = SandboxId(vm.id.clone());
        let vsock = crate::vsock::VsockClient::new(vm.vsock_uds.clone(), guest_config.vsock_port);
        Self {
            id,
            vm,
            vsock,
            config,
            created_at: Instant::now(),
        }
    }

    pub fn id(&self) -> &SandboxId {
        &self.id
    }

    pub fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            status: SandboxStatus::Ready,
            config: self.config.clone(),
            created_at: format!("{}s ago", self.created_at.elapsed().as_secs()),
        }
    }

    pub async fn exec(&self, command: &str, timeout: Duration) -> crate::error::Result<ExecResult> {
        self.vsock.exec(command, timeout).await
    }

    pub async fn exec_stream(
        &self,
        command: &str,
    ) -> crate::error::Result<(mpsc::Receiver<ExecEvent>, mpsc::Sender<Vec<u8>>)> {
        self.vsock.exec_stream(command).await
    }

    pub async fn send_signal(&self, signal: i32) -> crate::error::Result<()> {
        self.vsock.signal(signal).await
    }

    pub async fn upload(&self, content: &[u8], remote_path: &str) -> crate::error::Result<()> {
        self.vsock.write_file(remote_path, content).await
    }

    pub async fn download(&self, remote_path: &str) -> crate::error::Result<Vec<u8>> {
        self.vsock.read_file(remote_path).await
    }

    pub async fn list_files(&self, path: &str) -> crate::error::Result<Vec<FileEntry>> {
        self.vsock.list_files(path).await
    }

    pub async fn delete_file(&self, path: &str) -> crate::error::Result<()> {
        self.vsock.delete_file(path).await
    }

    pub async fn mkdir(&self, path: &str) -> crate::error::Result<()> {
        self.vsock.mkdir(path).await
    }

    pub async fn is_alive(&self) -> bool {
        self.vsock.ping().await.unwrap_or(false)
    }

    /// Configure networking inside the guest after a fresh boot with TAP.
    /// Sets up eth0 with the assigned IP, default route, and DNS.
    pub async fn setup_guest_network(&self) -> crate::error::Result<()> {
        let net = match self.vm.network {
            Some(ref n) => n,
            None => return Ok(()),
        };
        let cmd = format!(
            "ip addr add {}/30 dev eth0 && ip link set eth0 up && ip route add default via {} && echo 'nameserver 8.8.8.8' > /etc/resolv.conf",
            net.guest_ip, net.host_ip
        );
        let result = self.exec(&cmd, Duration::from_secs(10)).await?;
        if result.exit_code != 0 {
            return Err(crate::error::AgentBoxError::VmCreation(format!(
                "Guest network setup failed (exit {}): {}",
                result.exit_code, result.stderr
            )));
        }
        tracing::debug!(sandbox_id = %self.id, "Guest network configured: {}", net.guest_ip);
        Ok(())
    }

    pub async fn destroy(self) -> crate::error::Result<()> {
        // VM process is cleaned up when Child is dropped
        Ok(())
    }

    /// Extract the VM handle for destruction by Pool.
    pub(crate) fn into_vm(self) -> crate::vm::VmHandle {
        self.vm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // ── SandboxId ────────────────────────────────────────────────

    #[test]
    fn sandbox_id_serializes_as_string() {
        let id = SandboxId("abc-123".into());
        assert_eq!(serde_json::to_value(&id).unwrap(), json!("abc-123"));
    }

    #[test]
    fn sandbox_id_roundtrip() {
        let id = SandboxId("test-id".into());
        let json = serde_json::to_string(&id).unwrap();
        let back: SandboxId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn sandbox_id_hash_consistent() {
        let a = SandboxId("same".into());
        let b = SandboxId("same".into());
        let hash = |v: &SandboxId| {
            let mut h = DefaultHasher::new();
            v.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&a), hash(&b));
    }

    // ── SandboxConfig ────────────────────────────────────────────

    #[test]
    fn sandbox_config_roundtrip() {
        let cfg = SandboxConfig {
            memory_mb: 512,
            vcpus: 1,
            network: true,
            disk_size_mb: 1024,
            timeout_secs: 60,
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(
            json,
            json!({"memory_mb":512,"vcpus":1,"network":true,"disk_size_mb":1024,"timeout_secs":60})
        );
        let back: SandboxConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.memory_mb, 512);
        assert_eq!(back.vcpus, 1);
    }

    #[test]
    fn sandbox_config_zero_values() {
        let cfg = SandboxConfig {
            memory_mb: 0,
            vcpus: 0,
            network: false,
            disk_size_mb: 0,
            timeout_secs: 0,
        };
        let back: SandboxConfig =
            serde_json::from_value(serde_json::to_value(&cfg).unwrap()).unwrap();
        assert_eq!(back.memory_mb, 0);
        assert_eq!(back.timeout_secs, 0);
    }

    // ── SandboxStatus ────────────────────────────────────────────

    #[test]
    fn sandbox_status_serializes_to_variant_names() {
        assert_eq!(
            serde_json::to_value(SandboxStatus::Creating).unwrap(),
            json!("Creating")
        );
        assert_eq!(
            serde_json::to_value(SandboxStatus::Ready).unwrap(),
            json!("Ready")
        );
        assert_eq!(
            serde_json::to_value(SandboxStatus::Busy).unwrap(),
            json!("Busy")
        );
        assert_eq!(
            serde_json::to_value(SandboxStatus::Destroying).unwrap(),
            json!("Destroying")
        );
    }

    #[test]
    fn sandbox_status_roundtrip_all_variants() {
        for status in [
            SandboxStatus::Creating,
            SandboxStatus::Ready,
            SandboxStatus::Busy,
            SandboxStatus::Destroying,
        ] {
            let json = serde_json::to_value(&status).unwrap();
            let back: SandboxStatus = serde_json::from_value(json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn sandbox_status_invalid_string_fails() {
        let result = serde_json::from_value::<SandboxStatus>(json!("Invalid"));
        assert!(result.is_err());
    }

    // ── ExecResult ───────────────────────────────────────────────

    #[test]
    fn exec_result_roundtrip() {
        let r = ExecResult {
            stdout: "hello\n".into(),
            stderr: String::new(),
            exit_code: 0,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["stdout"], "hello\n");
        assert_eq!(json["exit_code"], 0);
        let back: ExecResult = serde_json::from_value(json).unwrap();
        assert_eq!(back.stdout, "hello\n");
    }

    #[test]
    fn exec_result_negative_exit_code() {
        let r = ExecResult {
            stdout: String::new(),
            stderr: "killed".into(),
            exit_code: -1,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["exit_code"], -1);
        let back: ExecResult = serde_json::from_value(json).unwrap();
        assert_eq!(back.exit_code, -1);
    }

    // ── FileEntry ────────────────────────────────────────────────

    #[test]
    fn file_entry_file_vs_dir() {
        let file = FileEntry {
            name: "test.txt".into(),
            size: 1024,
            is_dir: false,
        };
        let dir = FileEntry {
            name: "src".into(),
            size: 0,
            is_dir: true,
        };
        let fj = serde_json::to_value(&file).unwrap();
        let dj = serde_json::to_value(&dir).unwrap();
        assert_eq!(fj, json!({"name":"test.txt","size":1024,"is_dir":false}));
        assert_eq!(dj, json!({"name":"src","size":0,"is_dir":true}));
    }

    #[test]
    fn file_entry_max_size() {
        let f = FileEntry {
            name: "big".into(),
            size: u64::MAX,
            is_dir: false,
        };
        let json = serde_json::to_value(&f).unwrap();
        let back: FileEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back.size, u64::MAX);
    }
}
