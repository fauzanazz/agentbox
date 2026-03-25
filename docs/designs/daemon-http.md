# Daemon HTTP API

## Context

Implements the axum-based HTTP server that wraps `agentbox-core`. This is what the
SDKs talk to. REST endpoints for sandbox CRUD, command execution (non-streaming),
file operations, and health checks.

This task assumes `agentbox-core` has working Pool + Sandbox from FAU-71.
See `docs/architecture.md` for the full API design.

## Requirements

- axum server on configurable port (default 8080)
- REST endpoints for sandbox lifecycle (create, list, get, destroy)
- Command execution endpoint (non-streaming, wait for result)
- File operations (upload, download, list)
- Health check with pool status
- JSON request/response bodies
- Proper error handling with HTTP status codes
- Graceful shutdown on SIGTERM/SIGINT
- CORS middleware (permissive for development)

## Implementation

### `crates/agentbox-daemon/Cargo.toml`

Verify/update dependencies (should exist from scaffold FAU-67):
```toml
[package]
name = "agentbox-daemon"
version.workspace = true
edition.workspace = true

[dependencies]
agentbox-core = { path = "../agentbox-core" }
axum = { version = "0.8", features = ["multipart"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
tower-http = { version = "0.6", features = ["cors", "trace"] }
```

Note: Do NOT add `ws` feature to axum here — WebSocket is Task G (FAU-72).

### `crates/agentbox-daemon/src/main.rs`

```rust
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod handlers;
mod routes;
mod state;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Load config
    let config_path = std::env::args().nth(1)
        .unwrap_or_else(|| "/var/lib/agentbox/config.toml".to_string());
    let config = if std::path::Path::new(&config_path).exists() {
        agentbox_core::AgentBoxConfig::from_file(std::path::Path::new(&config_path))?
    } else {
        tracing::warn!("Config file not found at {config_path}, using defaults");
        agentbox_core::AgentBoxConfig::default()
    };

    let listen_addr = config.daemon.listen.clone();

    // Create VM manager, pool, start pool
    let vm_manager = Arc::new(agentbox_core::VmManager::new(config.vm.clone()));
    let pool = Arc::new(agentbox_core::Pool::new(
        config.pool.clone(),
        config.guest.clone(),
        vm_manager,
    ));
    let _pool_handle = pool.start().await?;

    let state = Arc::new(state::AppState {
        pool: pool.clone(),
        config: Arc::new(config),
    });

    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("AgentBox daemon listening on {listen_addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Shutting down...");
    pool.shutdown().await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm.recv() => {},
    }
    #[cfg(not(unix))]
    ctrl_c.await.ok();
}
```

Add `anyhow = "1"` to Cargo.toml dependencies.

### `crates/agentbox-daemon/src/state.rs`

```rust
use std::sync::Arc;
use agentbox_core::{Pool, AgentBoxConfig};

pub struct AppState {
    pub pool: Arc<Pool>,
    pub config: Arc<AgentBoxConfig>,
}
```

### `crates/agentbox-daemon/src/routes.rs`

```rust
use std::sync::Arc;
use axum::{Router, routing::{get, post, delete}};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use crate::{handlers, state::AppState};

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sandboxes", post(handlers::create_sandbox))
        .route("/sandboxes", get(handlers::list_sandboxes))
        .route("/sandboxes/{id}", get(handlers::get_sandbox))
        .route("/sandboxes/{id}", delete(handlers::destroy_sandbox))
        .route("/sandboxes/{id}/exec", post(handlers::exec_command))
        .route("/sandboxes/{id}/files", post(handlers::upload_file))
        .route("/sandboxes/{id}/files", get(handlers::handle_files_get))
        .route("/health", get(handlers::health_check))
        // WebSocket route will be added by Task G (FAU-72):
        // .route("/sandboxes/{id}/ws", get(handlers::ws_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

### `crates/agentbox-daemon/src/handlers.rs`

```rust
use std::sync::Arc;
use axum::{
    extract::{Path, Query, State, Multipart},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use agentbox_core::{SandboxConfig, SandboxId};
use crate::state::AppState;
```

**`POST /sandboxes` — create_sandbox:**
```rust
#[derive(Deserialize)]
pub struct CreateSandboxRequest {
    pub memory_mb: Option<u32>,
    pub vcpus: Option<u32>,
    pub network: Option<bool>,
    pub timeout: Option<u64>,
}

pub async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSandboxRequest>,
) -> Result<impl IntoResponse, AppError> {
    let config = SandboxConfig {
        memory_mb: req.memory_mb.unwrap_or(state.config.vm.defaults.memory_mb),
        vcpus: req.vcpus.unwrap_or(state.config.vm.defaults.vcpus),
        network: req.network.unwrap_or(state.config.vm.defaults.network),
        timeout_secs: req.timeout.unwrap_or(state.config.vm.defaults.timeout_secs),
    };
    let sandbox = state.pool.claim(config).await?;
    let info = sandbox.info();

    // Store sandbox for later access — need a way to look up by ID
    // Add a sandbox registry to AppState (HashMap<SandboxId, Arc<Mutex<Sandbox>>>)
    state.register_sandbox(sandbox).await;

    Ok((StatusCode::CREATED, Json(info)))
}
```

**Important: Sandbox registry.** The Pool tracks active sandbox IDs, but we need
to store the actual `Sandbox` object to call `.exec()` on it later. Add to AppState:

```rust
use tokio::sync::Mutex;
use std::collections::HashMap;

pub struct AppState {
    pub pool: Arc<Pool>,
    pub config: Arc<AgentBoxConfig>,
    pub sandboxes: Mutex<HashMap<SandboxId, Arc<tokio::sync::Mutex<agentbox_core::Sandbox>>>>,
}

impl AppState {
    pub async fn register_sandbox(&self, sandbox: agentbox_core::Sandbox) {
        let id = sandbox.id().clone();
        self.sandboxes.lock().await.insert(id, Arc::new(tokio::sync::Mutex::new(sandbox)));
    }

    pub async fn get_sandbox(&self, id: &SandboxId) -> Option<Arc<tokio::sync::Mutex<agentbox_core::Sandbox>>> {
        self.sandboxes.lock().await.get(id).cloned()
    }

    pub async fn remove_sandbox(&self, id: &SandboxId) -> Option<agentbox_core::Sandbox> {
        let sb_arc = self.sandboxes.lock().await.remove(id)?;
        Some(Arc::try_unwrap(sb_arc).ok()?.into_inner())
    }
}
```

**`GET /sandboxes` — list_sandboxes:**
```rust
pub async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sandboxes = state.pool.list_active().await;
    Json(sandboxes)
}
```

**`GET /sandboxes/{id}` — get_sandbox:**
```rust
pub async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state.get_sandbox(&sandbox_id).await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let info = sb.lock().await.info();
    Ok(Json(info))
}
```

**`DELETE /sandboxes/{id}` — destroy_sandbox:**
```rust
pub async fn destroy_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sandbox = state.remove_sandbox(&sandbox_id).await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    state.pool.release(sandbox).await?;
    Ok(Json(serde_json::json!({"status": "destroyed"})))
}
```

**`POST /sandboxes/{id}/exec` — exec_command:**
```rust
#[derive(Deserialize)]
pub struct ExecRequest {
    pub command: String,
    pub timeout: Option<u64>,
}

pub async fn exec_command(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state.get_sandbox(&sandbox_id).await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let timeout = std::time::Duration::from_secs(req.timeout.unwrap_or(30));
    let result = sb.lock().await.exec(&req.command, timeout).await?;
    Ok(Json(result))
}
```

**`POST /sandboxes/{id}/files` — upload_file:**
```rust
pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state.get_sandbox(&sandbox_id).await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;

    let mut path = String::new();
    let mut content = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::BadRequest(e.to_string()))? {
        match field.name() {
            Some("path") => path = field.text().await.unwrap_or_default(),
            Some("file") => content = field.bytes().await.unwrap_or_default().to_vec(),
            _ => {}
        }
    }

    if path.is_empty() { path = "/workspace/upload".to_string(); }
    let size = content.len();
    sb.lock().await.upload(&content, &path).await?;
    Ok(Json(serde_json::json!({"path": path, "size": size})))
}
```

**`GET /sandboxes/{id}/files` — handle_files_get (dual: list or download):**
```rust
#[derive(Deserialize)]
pub struct FilesQuery {
    pub path: Option<String>,
    pub list: Option<bool>,
}

pub async fn handle_files_get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<Response, AppError> {
    let sandbox_id = SandboxId(id);
    let sb = state.get_sandbox(&sandbox_id).await
        .ok_or(AppError::NotFound("Sandbox not found".into()))?;
    let path = query.path.unwrap_or_else(|| "/workspace".to_string());

    if query.list.unwrap_or(false) {
        let files = sb.lock().await.list_files(&path).await?;
        Ok(Json(files).into_response())
    } else {
        let data = sb.lock().await.download(&path).await?;
        Ok((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            data
        ).into_response())
    }
}
```

**`GET /health` — health_check:**
```rust
pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let pool_status = state.pool.pool_status().await;
    Json(serde_json::json!({
        "status": "ok",
        "pool": pool_status,
    }))
}
```

**Error handling:**
```rust
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    ServiceUnavailable(String),
    Internal(String),
}

impl From<agentbox_core::AgentBoxError> for AppError {
    fn from(e: agentbox_core::AgentBoxError) -> Self {
        match e {
            agentbox_core::AgentBoxError::PoolExhausted => AppError::ServiceUnavailable(e.to_string()),
            agentbox_core::AgentBoxError::VmNotFound(_) => AppError::NotFound(e.to_string()),
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
```

## Testing Strategy

Run tests: `cargo test -p agentbox-daemon`

### Unit tests using axum test utilities:

Create a helper that builds a test app with a mock pool:
```rust
#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::body::Body;
    use tower::ServiceExt;

    // Test health endpoint
    #[tokio::test]
    async fn test_health_check() {
        // Build app with real (default config) state
        // GET /health
        // Assert 200, body contains "status": "ok"
    }

    // Test create sandbox returns 503 when pool exhausted
    // Test list sandboxes returns empty array
    // Test get nonexistent sandbox returns 404
    // Test destroy nonexistent sandbox returns 404
}
```

### Integration tests (need KVM):
- Full flow: create → exec → download → destroy
- Mark with `#[ignore]`

## Out of Scope

- WebSocket endpoint (Task G)
- Authentication / authorization
- Rate limiting
- Request logging beyond tracing middleware
