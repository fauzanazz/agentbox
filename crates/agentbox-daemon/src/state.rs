use std::collections::HashMap;
use std::sync::Arc;

use agentbox_core::config::AgentBoxConfig;
use agentbox_core::pool::Pool;
use agentbox_core::sandbox::{Sandbox, SandboxId};
use tokio::sync::Mutex;

pub struct AppState {
    pub pool: Arc<Pool>,
    pub config: Arc<AgentBoxConfig>,
    pub sandboxes: Mutex<HashMap<SandboxId, Arc<Mutex<Sandbox>>>>,
}

impl AppState {
    pub fn new(pool: Arc<Pool>, config: Arc<AgentBoxConfig>) -> Self {
        Self {
            pool,
            config,
            sandboxes: Mutex::new(HashMap::new()),
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

pub enum RemoveSandboxError {
    NotFound,
    InUse,
}
