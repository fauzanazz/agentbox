use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub memory_mb: u32,
    pub vcpus: u32,
    pub network: bool,
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
}

impl Sandbox {
    pub async fn exec(&self, _command: &str, _timeout: Duration) -> crate::error::Result<ExecResult> {
        todo!()
    }

    pub async fn exec_stream(
        &self,
        _command: &str,
    ) -> crate::error::Result<(mpsc::Receiver<ExecEvent>, mpsc::Sender<Vec<u8>>)> {
        todo!()
    }

    pub async fn send_signal(&self, _signal: i32) -> crate::error::Result<()> {
        todo!()
    }

    pub async fn upload(&self, _content: &[u8], _remote_path: &str) -> crate::error::Result<()> {
        todo!()
    }

    pub async fn download(&self, _remote_path: &str) -> crate::error::Result<Vec<u8>> {
        todo!()
    }

    pub async fn list_files(&self, _path: &str) -> crate::error::Result<Vec<FileEntry>> {
        todo!()
    }

    pub async fn is_alive(&self) -> bool {
        todo!()
    }

    pub async fn destroy(self) -> crate::error::Result<()> {
        todo!()
    }

    pub fn id(&self) -> &SandboxId {
        &self.id
    }

    pub fn info(&self) -> SandboxInfo {
        todo!()
    }
}
