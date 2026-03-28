use std::path::{Component, PathBuf};

/// Shared mutex for tests that modify the AGENTBOX_WORKSPACE_DIR env var.
/// Import this in all test modules that call `set_var("AGENTBOX_WORKSPACE_DIR", ...)`.
#[cfg(test)]
pub static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Return the workspace base directory.
/// Reads `AGENTBOX_WORKSPACE_DIR` env var, defaulting to `/workspace`.
pub fn workspace_base() -> String {
    std::env::var("AGENTBOX_WORKSPACE_DIR").unwrap_or_else(|_| "/workspace".to_string())
}

/// Validate that a requested path resolves to within the allowed base directory.
/// Returns the normalized absolute path if valid.
///
/// - Rejects null bytes
/// - Resolves relative paths against `base`
/// - Lexically normalizes `..` and `.` components
/// - Checks result starts with `base` (component-aware)
///
/// NOTE: This is lexical-only validation. It does NOT resolve symlinks.
/// A symlink inside the workspace pointing outside it would pass this check.
/// This is acceptable because the guest agent runs inside a Firecracker VM —
/// symlink creation requires code already executing inside the sandbox, which
/// is within the trust boundary. The primary threat model is external callers
/// supplying malicious path strings (e.g., `../../etc/passwd`).
pub fn validate_path(requested: &str, base: &str) -> Result<PathBuf, String> {
    if requested.contains('\0') {
        return Err("Path contains null byte".to_string());
    }

    let target = if std::path::Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        PathBuf::from(base).join(requested)
    };

    // Lexically normalize: resolve ".." and "." without touching filesystem
    let mut normalized = PathBuf::new();
    for component in target.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }

    let base_path = PathBuf::from(base);
    if !normalized.starts_with(&base_path) {
        return Err(format!(
            "Path '{}' resolves outside allowed directory '{}'",
            requested, base
        ));
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "/workspace";

    #[test]
    fn valid_absolute_path() {
        let r = validate_path("/workspace/foo/bar.txt", BASE);
        assert_eq!(r.unwrap(), PathBuf::from("/workspace/foo/bar.txt"));
    }

    #[test]
    fn valid_workspace_root() {
        let r = validate_path("/workspace", BASE);
        assert_eq!(r.unwrap(), PathBuf::from("/workspace"));
    }

    #[test]
    fn dot_dot_escape_rejected() {
        let r = validate_path("/workspace/../etc/passwd", BASE);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("resolves outside"));
    }

    #[test]
    fn multiple_dot_dots_rejected() {
        let r = validate_path("/workspace/a/../../etc/passwd", BASE);
        assert!(r.is_err());
    }

    #[test]
    fn relative_path_resolved() {
        let r = validate_path("foo/bar", BASE);
        assert_eq!(r.unwrap(), PathBuf::from("/workspace/foo/bar"));
    }

    #[test]
    fn relative_dot_dot_escape_rejected() {
        let r = validate_path("../etc/passwd", BASE);
        assert!(r.is_err());
    }

    #[test]
    fn absolute_outside_base_rejected() {
        let r = validate_path("/etc/passwd", BASE);
        assert!(r.is_err());
    }

    #[test]
    fn null_byte_rejected() {
        let r = validate_path("/workspace/foo\0bar", BASE);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("null byte"));
    }

    #[test]
    fn workspace_prefix_not_confused() {
        // /workspace2 should NOT match /workspace
        let r = validate_path("/workspace2/evil", BASE);
        assert!(r.is_err());
    }

    #[test]
    fn dot_resolves_to_base() {
        let r = validate_path(".", BASE);
        assert_eq!(r.unwrap(), PathBuf::from("/workspace"));
    }

    #[test]
    fn trailing_slash_passes() {
        let r = validate_path("/workspace/", BASE);
        assert_eq!(r.unwrap(), PathBuf::from("/workspace"));
    }

    #[test]
    fn deeply_nested_valid_path() {
        let r = validate_path("/workspace/a/b/c/d/e.txt", BASE);
        assert_eq!(r.unwrap(), PathBuf::from("/workspace/a/b/c/d/e.txt"));
    }

    #[test]
    fn custom_base_dir() {
        let r = validate_path("/data/files/test.txt", "/data/files");
        assert_eq!(r.unwrap(), PathBuf::from("/data/files/test.txt"));
    }

    #[test]
    fn custom_base_escape_rejected() {
        let r = validate_path("/data/files/../secret", "/data/files");
        assert!(r.is_err());
    }
}
