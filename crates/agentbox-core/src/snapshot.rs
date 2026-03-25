use std::path::PathBuf;

pub struct SnapshotManager {
    pub(crate) snapshot_path: PathBuf,
}

impl SnapshotManager {
    pub fn new(snapshot_path: PathBuf) -> Self {
        Self { snapshot_path }
    }

    pub async fn load(&self, _api_socket: &std::path::Path) -> crate::error::Result<()> {
        todo!()
    }
}
