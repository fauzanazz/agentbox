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

// ── Handlers ───────────────────────────────────────────────────────

pub async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSandboxRequest>,
) -> Result<impl IntoResponse, AppError> {
    let defaults = &state.config.vm.defaults;
    let config = SandboxConfig {
        memory_mb: req.memory_mb.unwrap_or(defaults.memory_mb),
        vcpus: req.vcpus.unwrap_or(defaults.vcpus),
        network: req.network.unwrap_or(defaults.network),
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
    let sandbox_id = SandboxId(id);
    let sandbox = state
        .remove_sandbox(&sandbox_id)
        .await
        .map_err(|e| match e {
            RemoveSandboxError::NotFound => AppError::NotFound("Sandbox not found".into()),
            RemoveSandboxError::InUse => {
                AppError::BadRequest("Sandbox is currently in use by another request".into())
            }
        })?;
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
}
