use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::{
    config::{GuestConfig, PoolConfig},
    sandbox::*,
    vm::VmManager,
};

pub struct Pool {
    config: PoolConfig,
    guest_config: GuestConfig,
    vm_manager: Arc<VmManager>,
    available: Arc<Mutex<VecDeque<Sandbox>>>,
    active: Arc<RwLock<HashMap<SandboxId, SandboxInfo>>>,
}

impl Pool {
    pub fn new(
        config: PoolConfig,
        guest_config: GuestConfig,
        vm_manager: Arc<VmManager>,
    ) -> Self {
        Self {
            config,
            guest_config,
            vm_manager,
            available: Arc::new(Mutex::new(VecDeque::new())),
            active: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(&self) -> crate::error::Result<tokio::task::JoinHandle<()>> {
        todo!()
    }

    pub async fn claim(&self, _config: SandboxConfig) -> crate::error::Result<Sandbox> {
        todo!()
    }

    pub async fn release(&self, _sandbox: Sandbox) -> crate::error::Result<()> {
        todo!()
    }

    pub fn list_active(&self) -> Vec<SandboxInfo> {
        todo!()
    }

    pub async fn shutdown(&self) -> crate::error::Result<()> {
        todo!()
    }
}
