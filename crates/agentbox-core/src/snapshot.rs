use std::path::PathBuf;

use crate::error::{AgentBoxError, Result};
use crate::fc_api::fc_api_put;

pub struct SnapshotManager {
    pub(crate) snapshot_path: PathBuf,
}

impl SnapshotManager {
    pub fn new(snapshot_path: PathBuf) -> Self {
        Self { snapshot_path }
    }

    /// Load a snapshot into a Firecracker VM via its API socket.
    pub async fn load(&self, api_socket: &std::path::Path) -> Result<()> {
        let vmstate = self
            .snapshot_path
            .join("vmstate.bin")
            .canonicalize()
            .map_err(|e| {
                AgentBoxError::SnapshotLoad(format!(
                    "vmstate.bin not found in {:?}: {e}",
                    self.snapshot_path
                ))
            })?;
        let memory = self
            .snapshot_path
            .join("memory.bin")
            .canonicalize()
            .map_err(|e| {
                AgentBoxError::SnapshotLoad(format!(
                    "memory.bin not found in {:?}: {e}",
                    self.snapshot_path
                ))
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_paths() {
        let mgr = SnapshotManager::new(PathBuf::from("/var/lib/agentbox/snapshot"));
        assert_eq!(
            mgr.snapshot_path,
            PathBuf::from("/var/lib/agentbox/snapshot")
        );
        let vmstate = mgr.snapshot_path.join("vmstate.bin");
        let memory = mgr.snapshot_path.join("memory.bin");
        assert_eq!(
            vmstate,
            PathBuf::from("/var/lib/agentbox/snapshot/vmstate.bin")
        );
        assert_eq!(
            memory,
            PathBuf::from("/var/lib/agentbox/snapshot/memory.bin")
        );
    }
}
