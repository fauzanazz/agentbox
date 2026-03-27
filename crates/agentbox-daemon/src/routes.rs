use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::state::AppState;

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
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
