use std::sync::Arc;

use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Json;
use axum::Router;
use percent_encoding::percent_decode_str;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};

use crate::handlers;
use crate::metrics;
use crate::state::AppState;
use crate::ws;

/// Bearer token auth middleware. Rejects requests without a valid API key.
/// Checks `Authorization: Bearer <token>` header first, then falls back to
/// `?token=<token>` query parameter (needed for WebSocket connections where
/// custom headers are not supported by browser/Node WebSocket APIs).
async fn require_api_key(
    state: axum::extract::State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let expected = match state.config.daemon.api_key {
        Some(ref key) => key,
        None => return next.run(req).await, // no key configured = open access
    };

    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid or missing API key"})),
        )
            .into_response()
    };

    // Check Authorization: Bearer header
    if let Some(header) = req.headers().get("authorization") {
        if let Ok(value) = header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                if token == expected {
                    return next.run(req).await;
                }
            }
        }
    }

    // Fallback: check ?token= query parameter (for WebSocket connections)
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(raw_token) = pair.strip_prefix("token=") {
                // Percent-decode the token (SDKs encode with encodeURIComponent)
                if let Ok(token) = percent_decode_str(raw_token).decode_utf8() {
                    if *token == **expected {
                        return next.run(req).await;
                    }
                }
            }
        }
    }

    unauthorized()
}

pub fn build_router(
    state: Arc<AppState>,
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
) -> Router {
    // Protected routes — require API key
    let protected = Router::new()
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
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    // Public routes — no auth required
    let metrics_state = state.clone();
    let public = Router::new()
        .route("/health", get(handlers::health_check))
        .route(
            "/metrics",
            get(move || async move {
                metrics::record_pool_gauges(&metrics_state);
                prometheus_handle.render()
            }),
        );

    // Use path-only spans to avoid leaking query params (e.g., ?token=) in logs
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|req: &axum::http::Request<_>| {
            tracing::info_span!(
                "request",
                method = %req.method(),
                uri = %req.uri().path(),
            )
        })
        .on_response(DefaultOnResponse::new());

    // CORS — configurable origins, defaults to same-origin only
    let cors_layer = build_cors_layer(&state.config.cors);

    let mut router = protected
        .merge(public)
        .layer(middleware::from_fn(metrics::track_metrics))
        .layer(cors_layer)
        .layer(trace_layer);

    // Rate limiting — disabled when requests_per_second = 0
    if state.config.rate_limit.requests_per_second > 0 {
        let governor_conf = Arc::new(
            tower_governor::governor::GovernorConfigBuilder::default()
                .per_second(state.config.rate_limit.requests_per_second)
                .burst_size(state.config.rate_limit.burst_size)
                .finish()
                .expect("valid rate limit config"),
        );
        router = router.layer(tower_governor::GovernorLayer {
            config: governor_conf,
        });
    }

    router.with_state(state)
}

fn build_cors_layer(config: &agentbox_core::config::CorsConfig) -> CorsLayer {
    if config.allowed_origins.iter().any(|o| o == "*") {
        return CorsLayer::permissive();
    }

    if config.allowed_origins.is_empty() {
        // Same-origin only: no Access-Control-Allow-Origin header sent,
        // which means browsers block cross-origin requests.
        return CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers(tower_http::cors::Any);
    }

    let origins: Vec<HeaderValue> = config
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(tower_http::cors::Any)
}
