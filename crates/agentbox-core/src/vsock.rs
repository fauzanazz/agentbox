use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::sandbox::{ExecEvent, ExecResult, FileEntry};

#[derive(Debug)]
pub struct VsockClient {
    pub(crate) uds_path: PathBuf,
    pub(crate) port: u32,
}

impl VsockClient {
    pub fn new(uds_path: PathBuf, port: u32) -> Self {
        Self { uds_path, port }
    }

    pub async fn ping(&self) -> crate::error::Result<bool> {
        todo!()
    }

    pub async fn exec(
        &self,
        _command: &str,
        _timeout: Duration,
    ) -> crate::error::Result<ExecResult> {
        todo!()
    }

    pub async fn exec_stream(
        &self,
        _command: &str,
    ) -> crate::error::Result<(mpsc::Receiver<ExecEvent>, mpsc::Sender<Vec<u8>>)> {
        todo!()
    }

    pub async fn signal(&self, _signal: i32) -> crate::error::Result<()> {
        todo!()
    }

    pub async fn read_file(&self, _path: &str) -> crate::error::Result<Vec<u8>> {
        todo!()
    }

    pub async fn write_file(&self, _path: &str, _data: &[u8]) -> crate::error::Result<()> {
        todo!()
    }

    pub async fn list_files(&self, _path: &str) -> crate::error::Result<Vec<FileEntry>> {
        todo!()
    }
}
