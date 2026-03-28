use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::state::AppState;
use crate::ws;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sandboxes", post(handlers::create_sandbox))
        .route("/sandboxes", get(handlers::list_sandboxes))
        .route("/sandboxes/{id}", get(handlers::get_sandbox))
        .route("/sandboxes/{id}", delete(handlers::destroy_sandbox))
        .route("/sandboxes/{id}/exec", post(handlers::exec_command))
        .route("/sandboxes/{id}/files", post(handlers::upload_file))
        .route("/sandboxes/{id}/files", get(handlers::handle_files_get))
        .route(
            "/sandboxes/{id}/files",
            delete(handlers::delete_file).put(handlers::mkdir),
        )
        .route("/sandboxes/{id}/signal", post(handlers::send_signal))
        .route(
            "/sandboxes/{id}/ports",
            post(handlers::create_port_forward).get(handlers::list_port_forwards),
        )
        .route(
            "/sandboxes/{id}/ports/{guest_port}",
            delete(handlers::remove_port_forward),
        )
        .route("/sandboxes/{id}/ws", get(ws::ws_handler))
        .route("/pool/status", get(handlers::pool_status))
        .route("/health", get(handlers::health_check))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
