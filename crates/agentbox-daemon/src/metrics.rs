use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use metrics::{counter, gauge, histogram};
use std::sync::Arc;
use std::time::Instant;

use crate::state::AppState;

/// Middleware that records HTTP request metrics.
pub async fn track_metrics(req: Request, next: Next) -> Response {
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let method = req.method().to_string();

    let start = Instant::now();
    let response = next.run(req).await;
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    counter!("agentbox_http_requests_total", "method" => method.clone(), "path" => path.clone(), "status" => status)
        .increment(1);
    histogram!("agentbox_http_request_duration_seconds", "method" => method, "path" => path)
        .record(duration);

    response
}

/// Record pool gauge metrics from current pool state.
pub fn record_pool_gauges(state: &Arc<AppState>) {
    let status = state.pool.status();
    gauge!("agentbox_sandboxes_active").set(status.active_sandboxes as f64);
    gauge!("agentbox_pool_warm_vms").set(status.warm_vms as f64);
    gauge!("agentbox_pool_network_warm_vms").set(status.warm_network_vms as f64);
    gauge!("agentbox_pool_max_size").set(status.config.max_size as f64);
}

/// Install the global Prometheus recorder. Returns the handle for rendering.
pub fn install_recorder() -> metrics_exporter_prometheus::PrometheusHandle {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}
