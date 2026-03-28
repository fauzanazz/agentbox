use std::collections::HashMap;
use std::sync::Arc;

use agentbox_core::config::AgentBoxConfig;
use agentbox_core::pool::Pool;
use agentbox_core::sandbox::{Sandbox, SandboxId};
use tokio::sync::Mutex;

use crate::port_forward::PortForwardEntry;

pub struct AppState {
    pub pool: Arc<Pool>,
    pub config: Arc<AgentBoxConfig>,
    pub sandboxes: Mutex<HashMap<SandboxId, Arc<Mutex<Sandbox>>>>,
    pub port_forwards: Mutex<HashMap<String, HashMap<u16, PortForwardEntry>>>,
}

impl AppState {
    pub fn new(pool: Arc<Pool>, config: Arc<AgentBoxConfig>) -> Self {
        Self {
            pool,
            config,
            sandboxes: Mutex::new(HashMap::new()),
            port_forwards: Mutex::new(HashMap::new()),
        }
    }

    pub async fn register_sandbox(&self, sandbox: Sandbox) {
        let id = sandbox.id().clone();
        self.sandboxes
            .lock()
            .await
            .insert(id, Arc::new(Mutex::new(sandbox)));
    }

    pub async fn get_sandbox(&self, id: &SandboxId) -> Option<Arc<Mutex<Sandbox>>> {
        self.sandboxes.lock().await.get(id).cloned()
    }

    pub async fn remove_sandbox(&self, id: &SandboxId) -> Result<Sandbox, RemoveSandboxError> {
        let mut sandboxes = self.sandboxes.lock().await;
        let sb_arc = sandboxes.remove(id).ok_or(RemoveSandboxError::NotFound)?;
        match Arc::try_unwrap(sb_arc) {
            Ok(mutex) => Ok(mutex.into_inner()),
            Err(arc) => {
                sandboxes.insert(id.clone(), arc);
                Err(RemoveSandboxError::InUse)
            }
        }
    }
}

#[derive(Debug)]
pub enum RemoveSandboxError {
    NotFound,
    InUse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentbox_core::config::{AgentBoxConfig, GuestConfig, PoolConfig, VmConfig};
    use agentbox_core::pool::Pool;
    use agentbox_core::sandbox::SandboxConfig;
    use agentbox_core::vm::{VmHandle, VmManager};
    use std::path::PathBuf;

    fn make_state() -> Arc<AppState> {
        let vm_manager = Arc::new(VmManager::new(VmConfig::default()));
        let pool = Arc::new(Pool::new(
            PoolConfig::default(),
            GuestConfig::default(),
            vm_manager,
        ));
        Arc::new(AppState::new(pool, Arc::new(AgentBoxConfig::default())))
    }

    async fn dummy_sandbox(id: &str) -> Sandbox {
        let child = tokio::process::Command::new("sleep")
            .arg("3600")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let vm = VmHandle {
            id: id.into(),
            process: child,
            api_socket: PathBuf::from("/dev/null"),
            vsock_uds: PathBuf::from("/dev/null"),
            work_dir: PathBuf::from("/tmp"),
        };
        let config = SandboxConfig {
            memory_mb: 128,
            vcpus: 1,
            network: false,
            timeout_secs: 60,
        };
        let guest_config = GuestConfig::default();
        Sandbox::new(vm, config, &guest_config)
    }

    // ── Registration & retrieval ─────────────────────────────────

    #[tokio::test]
    async fn get_returns_none_when_empty() {
        let state = make_state();
        assert!(state.get_sandbox(&SandboxId("x".into())).await.is_none());
    }

    #[tokio::test]
    async fn register_then_get_returns_some() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("abc").await).await;
        assert!(state.get_sandbox(&SandboxId("abc".into())).await.is_some());
    }

    #[tokio::test]
    async fn get_wrong_id_returns_none() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("abc").await).await;
        assert!(state.get_sandbox(&SandboxId("xyz".into())).await.is_none());
    }

    #[tokio::test]
    async fn register_multiple_distinct_ids() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("a").await).await;
        state.register_sandbox(dummy_sandbox("b").await).await;
        state.register_sandbox(dummy_sandbox("c").await).await;
        assert!(state.get_sandbox(&SandboxId("a".into())).await.is_some());
        assert!(state.get_sandbox(&SandboxId("b".into())).await.is_some());
        assert!(state.get_sandbox(&SandboxId("c".into())).await.is_some());
    }

    // ── Removal happy path ───────────────────────────────────────

    #[tokio::test]
    async fn remove_returns_sandbox_and_clears_map() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("abc").await).await;
        let sb = state.remove_sandbox(&SandboxId("abc".into())).await;
        assert!(sb.is_ok());
        assert_eq!(sb.unwrap().id, SandboxId("abc".into()));
        assert!(state.get_sandbox(&SandboxId("abc".into())).await.is_none());
    }

    #[tokio::test]
    async fn remove_then_remove_again_returns_not_found() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("abc").await).await;
        assert!(state.remove_sandbox(&SandboxId("abc".into())).await.is_ok());
        assert!(matches!(
            state.remove_sandbox(&SandboxId("abc".into())).await,
            Err(RemoveSandboxError::NotFound)
        ));
    }

    // ── Removal error paths ──────────────────────────────────────

    #[tokio::test]
    async fn remove_nonexistent_returns_not_found() {
        let state = make_state();
        assert!(matches!(
            state.remove_sandbox(&SandboxId("nope".into())).await,
            Err(RemoveSandboxError::NotFound)
        ));
    }

    #[tokio::test]
    async fn remove_while_arc_held_returns_in_use() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("abc").await).await;
        let _held = state.get_sandbox(&SandboxId("abc".into())).await.unwrap();
        assert!(matches!(
            state.remove_sandbox(&SandboxId("abc".into())).await,
            Err(RemoveSandboxError::InUse)
        ));
        // Sandbox should still be in the map after InUse
        assert!(state.get_sandbox(&SandboxId("abc".into())).await.is_some());
    }

    #[tokio::test]
    async fn remove_succeeds_after_clone_dropped() {
        let state = make_state();
        state.register_sandbox(dummy_sandbox("abc").await).await;
        {
            let _held = state.get_sandbox(&SandboxId("abc".into())).await.unwrap();
            // _held dropped here
        }
        assert!(state.remove_sandbox(&SandboxId("abc".into())).await.is_ok());
    }
}
