use std::sync::Arc;

use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use agentbox_core::error::AgentBoxError;
use agentbox_core::sandbox::{SandboxConfig, SandboxId};

use crate::state::{AppState, RemoveSandboxError};

// ── Request / query types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSandboxRequest {
    pub memory_mb: Option<u32>,
    pub vcpus: Option<u32>,
    pub network: Option<bool>,
    pub disk_size_mb: Option<u32>,
    pub timeout: Option<u64>,
}

#[derive(Deserialize)]
pub struct ExecRequest {
    pub command: String,
    pub timeout: Option<u64>,
}

#[derive(Deserialize)]
pub struct FilesQuery {
    pub path: Option<String>,
    pub list: Option<bool>,
}

#[derive(Deserialize)]
pub struct SignalRequest {
    pub signal: i32,
}

#[derive(Deserialize)]
pub struct CreatePortForwardRequest {
    pub guest_port: u16,
}

// ── Path validation ───────────────────────────────────────────────

/// Validate that a file path is safe to forward to the guest agent.
/// Defense-in-depth: the guest-agent also validates, but we reject
/// obviously malicious paths before they cross the vsock boundary.
fn validate_sandbox_path(path: &str) -> Result<(), AppError> {
    if path.contains('\0') {
        return Err(AppError::BadRequest("Path contains null byte".into()));
    }

    let target = std::path::Path::new(path);
    let mut normalized = std::path::PathBuf::new();
    for component in target.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other),
        }
    }

    if !normalized.starts_with("/workspace") {
        return Err(AppError::BadRequest(format!(
            "Path must be under /workspace, got: {path}"
        )));
    }

    Ok(())
}

// ── Handlers ───────────────────────────────────────────────────────

pub async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSandboxRequest>,
) -> Result<impl IntoResponse, AppError> {
    let defaults = &state.config.vm.defaults;
    let disk_size_mb = req
        .disk_size_mb
        .unwrap_or(defaults.disk_size_mb)
        .clamp(512, 2048);
    let config = SandboxConfig {
        memory_mb: req.memory_mb.unwrap_or(defaults.memory_mb),
        vcpus: req.vcpus.unwrap_or(defaults.vcpus),
        network: req.network.unwrap_or(defaults.network),
        disk_size_mb,
        timeout_secs: req.timeout.unwrap_or(defaults.timeout_secs),
    };

    let sandbox = state.pool.claim(config).await?;
    let info = sandbox.info();
    state.register_sandbox(sandbox).await;

    Ok((StatusCode::CREATED, Json(info)))
}

pub async fn list_sandboxes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sandboxes = state.pool.list_active();
    Json(sandboxes)
}

pub async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let info = sb.lock().await.info();
    Ok(Json(info))
}

pub async fn destroy_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id.clone());

    // Remove sandbox first — only clean up port forwards on success.
    let sandbox = state
        .remove_sandbox(&sandbox_id)
        .await
        .map_err(|e| match e {
            RemoveSandboxError::NotFound => AppError::NotFound("Sandbox not found".into()),
            RemoveSandboxError::InUse => {
                AppError::BadRequest("Sandbox is currently in use by another request".into())
            }
        })?;

    // Clean up port forwards after confirmed removal.
    if let Some(forwards) = state.port_forwards.lock().await.remove(&id) {
        for (_, entry) in forwards {
            entry.stop();
        }
    }

    state.pool.release(sandbox).await?;
    Ok(Json(serde_json::json!({"status": "destroyed"})))
}

pub async fn exec_command(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let timeout = std::time::Duration::from_secs(req.timeout.unwrap_or(30));
    let result = sb.lock().await.exec(&req.command, timeout).await?;
    Ok(Json(result))
}

pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    let mut path = String::new();
    let mut content = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        match field.name() {
            Some("path") => {
                path = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
            }
            Some("file") => {
                content = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?
                    .to_vec();
            }
            _ => {}
        }
    }

    if path.is_empty() {
        path = "/workspace/upload".to_string();
    }
    validate_sandbox_path(&path)?;

    let size = content.len();
    sb.lock().await.upload(&content, &path).await?;
    Ok(Json(serde_json::json!({"path": path, "size": size})))
}

pub async fn handle_files_get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<Response, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let path = query.path.unwrap_or_else(|| "/workspace".to_string());
    validate_sandbox_path(&path)?;

    if query.list.unwrap_or(false) {
        let files = sb.lock().await.list_files(&path).await?;
        Ok(Json(files).into_response())
    } else {
        let data = sb.lock().await.download(&path).await?;
        Ok((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            data,
        )
            .into_response())
    }
}

pub async fn pool_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = state.pool.status();
    Json(status)
}

pub async fn send_signal(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SignalRequest>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    sb.lock().await.send_signal(req.signal).await?;
    Ok(Json(
        serde_json::json!({"status": "signal_sent", "signal": req.signal}),
    ))
}

pub async fn delete_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let path = query.path.ok_or(AppError::BadRequest(
        "Missing 'path' query parameter".into(),
    ))?;
    validate_sandbox_path(&path)?;
    sb.lock().await.delete_file(&path).await?;
    Ok(Json(serde_json::json!({"status": "deleted", "path": path})))
}

pub async fn mkdir(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let path = query.path.ok_or(AppError::BadRequest(
        "Missing 'path' query parameter".into(),
    ))?;
    validate_sandbox_path(&path)?;
    sb.lock().await.mkdir(&path).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"status": "created", "path": path})),
    ))
}

pub async fn create_port_forward(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CreatePortForwardRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.guest_port == 0 {
        return Err(AppError::BadRequest(
            "guest_port must be between 1 and 65535".into(),
        ));
    }

    let sandbox_id = SandboxId(id.clone());
    let sb = state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    let sandbox = sb.lock().await;
    let uds_path = sandbox.vsock.uds_path().to_path_buf();
    let vsock_port = sandbox.vsock.port();
    drop(sandbox);

    // Hold the lock across check-and-insert to prevent TOCTOU races.
    let mut forwards = state.port_forwards.lock().await;
    let sandbox_forwards = forwards.entry(id.clone()).or_default();

    if sandbox_forwards.contains_key(&req.guest_port) {
        return Err(AppError::BadRequest(format!(
            "Port {} is already forwarded",
            req.guest_port
        )));
    }

    let max = crate::port_forward::max_forwards_per_sandbox();
    if sandbox_forwards.len() >= max {
        return Err(AppError::BadRequest(format!(
            "Maximum of {max} port forwards per sandbox reached"
        )));
    }

    let entry = crate::port_forward::start_forward(uds_path, vsock_port, req.guest_port).await?;
    let info = entry.info();
    sandbox_forwards.insert(req.guest_port, entry);

    Ok((StatusCode::CREATED, Json(info)))
}

pub async fn list_port_forwards(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id.clone());
    state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    let forwards = state.port_forwards.lock().await;
    let ports: Vec<_> = forwards
        .get(&id)
        .map(|m| m.values().map(|e| e.info()).collect())
        .unwrap_or_default();

    Ok(Json(serde_json::json!({"ports": ports})))
}

pub async fn remove_port_forward(
    State(state): State<Arc<AppState>>,
    Path((id, guest_port)): Path<(String, u16)>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id.clone());
    state
        .get_sandbox(&sandbox_id)
        .await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    let mut forwards = state.port_forwards.lock().await;
    let entry = forwards
        .get_mut(&id)
        .and_then(|m| m.remove(&guest_port))
        .ok_or(AppError::NotFound(format!(
            "Port forward for port {guest_port} not found"
        )))?;

    entry.stop();

    Ok(StatusCode::NO_CONTENT)
}

pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let active = state.pool.list_active();
    Json(serde_json::json!({
        "status": "ok",
        "pool": {
            "active": active.len(),
            "max_size": state.config.pool.max_size,
        },
    }))
}

// ── Error handling ─────────────────────────────────────────────────

pub enum AppError {
    NotFound(String),
    BadRequest(String),
    ServiceUnavailable(String),
    Internal(String),
}

impl From<AgentBoxError> for AppError {
    fn from(e: AgentBoxError) -> Self {
        match e {
            AgentBoxError::PoolExhausted => AppError::ServiceUnavailable(e.to_string()),
            AgentBoxError::VmNotFound(_) => AppError::NotFound(e.to_string()),
            AgentBoxError::PathTraversal(msg) => AppError::BadRequest(msg),
            _ => AppError::Internal(e.to_string()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app() -> axum::Router {
        use agentbox_core::config::AgentBoxConfig;
        use agentbox_core::pool::Pool;
        use agentbox_core::vm::VmManager;

        let vm_manager = Arc::new(VmManager::new(agentbox_core::config::VmConfig::default()));
        let pool = Arc::new(Pool::new(
            agentbox_core::config::PoolConfig::default(),
            agentbox_core::config::GuestConfig::default(),
            vm_manager,
        ));
        let state = Arc::new(crate::state::AppState::new(
            pool,
            Arc::new(AgentBoxConfig::default()),
        ));
        crate::routes::build_router(state)
    }

    #[tokio::test]
    async fn test_get_nonexistent_sandbox_returns_404() {
        let app = test_app();
        let req = Request::builder()
            .uri("/sandboxes/nonexistent")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_destroy_nonexistent_sandbox_returns_404() {
        let app = test_app();
        let req = Request::builder()
            .uri("/sandboxes/nonexistent")
            .method("DELETE")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_exec_nonexistent_sandbox_returns_404() {
        let app = test_app();
        let req = Request::builder()
            .uri("/sandboxes/nonexistent/exec")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"command":"ls"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_app_error_not_found_response() {
        let err = AppError::NotFound("test".into());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_app_error_bad_request_response() {
        let err = AppError::BadRequest("bad input".into());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_app_error_service_unavailable_response() {
        let err = AppError::ServiceUnavailable("overloaded".into());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_app_error_internal_response() {
        let err = AppError::Internal("something broke".into());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ── AgentBoxError → AppError mapping ─────────────────────────

    #[test]
    fn pool_exhausted_maps_to_service_unavailable() {
        let e = AppError::from(AgentBoxError::PoolExhausted);
        assert!(matches!(e, AppError::ServiceUnavailable(_)));
    }

    #[test]
    fn vm_not_found_maps_to_not_found() {
        let e = AppError::from(AgentBoxError::VmNotFound("abc".into()));
        assert!(matches!(e, AppError::NotFound(_)));
    }

    #[test]
    fn vm_creation_maps_to_internal() {
        let e = AppError::from(AgentBoxError::VmCreation("fail".into()));
        assert!(matches!(e, AppError::Internal(_)));
    }

    #[test]
    fn timeout_maps_to_internal() {
        let e = AppError::from(AgentBoxError::Timeout("too slow".into()));
        assert!(matches!(e, AppError::Internal(_)));
    }

    // ── AppError body format ─────────────────────────────────────

    #[tokio::test]
    async fn app_error_body_is_json_with_error_key() {
        let err = AppError::NotFound("sandbox xyz not found".into());
        let resp = err.into_response();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "sandbox xyz not found");
    }

    // ── 404 for upload & files_get ───────────────────────────────

    #[tokio::test]
    async fn upload_file_nonexistent_sandbox_returns_404() {
        let app = test_app();
        let boundary = "----testboundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nhello\r\n--{boundary}--\r\n"
        );
        let req = Request::builder()
            .uri("/sandboxes/nonexistent/files")
            .method("POST")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn files_get_nonexistent_sandbox_returns_404() {
        let app = test_app();
        let req = Request::builder()
            .uri("/sandboxes/nonexistent/files?list=true")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Request validation ───────────────────────────────────────

    #[tokio::test]
    async fn create_sandbox_invalid_json_returns_4xx() {
        let app = test_app();
        let req = Request::builder()
            .uri("/sandboxes")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from("not json"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.status().is_client_error());
    }

    // ── Path validation ──────────────────────────────────────────

    #[test]
    fn validate_sandbox_path_valid() {
        assert!(validate_sandbox_path("/workspace/foo.txt").is_ok());
    }

    #[test]
    fn validate_sandbox_path_traversal_rejected() {
        assert!(validate_sandbox_path("/workspace/../etc/passwd").is_err());
    }

    #[test]
    fn validate_sandbox_path_outside_rejected() {
        assert!(validate_sandbox_path("/etc/passwd").is_err());
    }

    #[test]
    fn validate_sandbox_path_null_byte_rejected() {
        assert!(validate_sandbox_path("/workspace/foo\0bar").is_err());
    }

    #[test]
    fn validate_sandbox_path_prefix_not_confused() {
        // /workspace2 must NOT match /workspace
        assert!(validate_sandbox_path("/workspace2/evil").is_err());
    }

    #[test]
    fn validate_sandbox_path_relative_rejected() {
        // Relative paths don't start with /workspace and must be rejected
        assert!(validate_sandbox_path("workspace/foo.txt").is_err());
        assert!(validate_sandbox_path("foo.txt").is_err());
    }

    // ── List & health (no VMs needed) ────────────────────────────

    #[tokio::test]
    async fn test_list_sandboxes_returns_empty_array() {
        let app = test_app();
        let req = Request::builder()
            .uri("/sandboxes")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_health_check_returns_expected_format() {
        let app = test_app();
        let req = Request::builder()
            .uri("/health")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["pool"]["active"].is_number());
        assert_eq!(json["pool"]["active"], 0);
    }

    // ── Error body consistency ───────────────────────────────────

    #[tokio::test]
    async fn all_error_variants_produce_json_error_key() {
        let variants = vec![
            AppError::NotFound("nf".into()),
            AppError::BadRequest("br".into()),
            AppError::ServiceUnavailable("su".into()),
            AppError::Internal("int".into()),
        ];
        for err in variants {
            let resp = err.into_response();
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(
                json.get("error").is_some(),
                "All errors must have 'error' key"
            );
            assert!(json["error"].is_string());
        }
    }
}
