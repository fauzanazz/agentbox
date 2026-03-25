use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentBoxError {
    #[error("VM creation failed: {0}")]
    VmCreation(String),

    #[error("VM not found: {0}")]
    VmNotFound(String),

    #[error("Vsock connection error: {0}")]
    VsockConnection(String),

    #[error("Execution failed: {0}")]
    ExecFailed(String),

    #[error("File operation error: {0}")]
    FileOp(String),

    #[error("Pool exhausted: no available sandboxes")]
    PoolExhausted,

    #[error("Snapshot load error: {0}")]
    SnapshotLoad(String),

    #[error("Operation timed out: {0}")]
    Timeout(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AgentBoxError>;
