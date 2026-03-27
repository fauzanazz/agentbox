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

    #[error("API transport error: {0}")]
    ApiTransport(String),

    #[error("Operation timed out: {0}")]
    Timeout(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AgentBoxError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    // ── Display strings ──────────────────────────────────────────

    #[test]
    fn display_vm_creation() {
        let e = AgentBoxError::VmCreation("disk full".into());
        assert_eq!(e.to_string(), "VM creation failed: disk full");
    }

    #[test]
    fn display_vm_not_found() {
        let e = AgentBoxError::VmNotFound("abc123".into());
        assert_eq!(e.to_string(), "VM not found: abc123");
    }

    #[test]
    fn display_vsock_connection() {
        let e = AgentBoxError::VsockConnection("refused".into());
        assert_eq!(e.to_string(), "Vsock connection error: refused");
    }

    #[test]
    fn display_exec_failed() {
        let e = AgentBoxError::ExecFailed("signal 9".into());
        assert_eq!(e.to_string(), "Execution failed: signal 9");
    }

    #[test]
    fn display_file_op() {
        let e = AgentBoxError::FileOp("permission denied".into());
        assert_eq!(e.to_string(), "File operation error: permission denied");
    }

    #[test]
    fn display_pool_exhausted() {
        let e = AgentBoxError::PoolExhausted;
        assert_eq!(e.to_string(), "Pool exhausted: no available sandboxes");
    }

    #[test]
    fn display_snapshot_load() {
        let e = AgentBoxError::SnapshotLoad("corrupt".into());
        assert_eq!(e.to_string(), "Snapshot load error: corrupt");
    }

    #[test]
    fn display_timeout() {
        let e = AgentBoxError::Timeout("5s exceeded".into());
        assert_eq!(e.to_string(), "Operation timed out: 5s exceeded");
    }

    #[test]
    fn display_config() {
        let e = AgentBoxError::Config("missing field".into());
        assert_eq!(e.to_string(), "Configuration error: missing field");
    }

    // ── From trait conversions ───────────────────────────────────

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let e: AgentBoxError = io_err.into();
        assert!(matches!(e, AgentBoxError::Io(_)));
        assert!(e.to_string().contains("gone"));
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<String>("{{bad").unwrap_err();
        let e: AgentBoxError = json_err.into();
        assert!(matches!(e, AgentBoxError::Json(_)));
        assert!(e.to_string().starts_with("JSON error:"));
    }

    // ── Error trait source ───────────────────────────────────────

    #[test]
    fn io_variant_has_source() {
        let e = AgentBoxError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x"));
        assert!(e.source().is_some());
    }

    #[test]
    fn json_variant_has_source() {
        let json_err = serde_json::from_str::<String>("bad").unwrap_err();
        let e = AgentBoxError::Json(json_err);
        assert!(e.source().is_some());
    }

    #[test]
    fn string_variants_have_no_source() {
        let cases: Vec<AgentBoxError> = vec![
            AgentBoxError::VmCreation("x".into()),
            AgentBoxError::VmNotFound("x".into()),
            AgentBoxError::VsockConnection("x".into()),
            AgentBoxError::ExecFailed("x".into()),
            AgentBoxError::FileOp("x".into()),
            AgentBoxError::PoolExhausted,
            AgentBoxError::SnapshotLoad("x".into()),
            AgentBoxError::ApiTransport("x".into()),
            AgentBoxError::Timeout("x".into()),
            AgentBoxError::Config("x".into()),
        ];
        for e in &cases {
            assert!(e.source().is_none(), "Expected no source for: {e}");
        }
    }
}
