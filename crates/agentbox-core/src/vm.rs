use std::path::PathBuf;
use tokio::process::Child;

#[derive(Debug)]
pub struct VmHandle {
    pub id: String,
    pub process: Child,
    pub api_socket: PathBuf,
    pub vsock_uds: PathBuf,
    pub work_dir: PathBuf,
}

pub struct VmManager {
    pub(crate) config: crate::config::VmConfig,
}

impl VmManager {
    pub fn new(config: crate::config::VmConfig) -> Self {
        Self { config }
    }

    pub async fn create_from_snapshot(
        &self,
        _config: &crate::sandbox::SandboxConfig,
    ) -> crate::error::Result<VmHandle> {
        todo!()
    }

    pub async fn destroy(&self, _vm: VmHandle) -> crate::error::Result<()> {
        todo!()
    }

    pub fn is_running(_vm: &VmHandle) -> bool {
        todo!()
    }
}
