use axum::{Router, routing::get};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};
use tracing::{Span, info};

use crate::{ApiError, ApiState, health, middleware};

/// Stable product API prefix for versioned resources.
pub const API_VERSION_PREFIX: &str = "/api/v1";

/// Conservative default for the transport foundation. Feature endpoints must
/// apply stricter operation-specific limits when they are introduced.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Construct the Axum application router.
pub fn router(state: ApiState) -> Router {
    let api = Router::new().route("/system/health", get(health::system_health));

    Router::new()
        // Requested short probe names and the OpenAPI blueprint's canonical
        // aliases are intentionally unversioned deployment signals.
        .route("/live", get(health::live))
        .route("/ready", get(health::ready))
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .nest(crate::API_VERSION_PREFIX, api)
        .fallback(not_found)
        .layer(RequestBodyLimitLayer::new(state.body_limit_bytes()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let context = request.extensions().get::<crate::RequestContext>();
                    let request_id = context
                        .map(|context| context.request_id().to_string())
                        .unwrap_or_else(|| "unassigned".to_owned());
                    let trace_id = context
                        .map(|context| context.trace_id().to_string())
                        .unwrap_or_else(|| "unassigned".to_owned());
                    tracing::info_span!(
                        "http.transport",
                        request_id = %request_id,
                        trace_id = %trace_id,
                        method = %request.method(),
                        path = %request.uri().path(),
                        status_code = tracing::field::Empty,
                    )
                })
                .on_request(|request: &axum::http::Request<_>, span: &Span| {
                    info!(
                        parent: span,
                        method = %request.method(),
                        path = %request.uri().path(),
                        "request started"
                    );
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     span: &Span| {
                        info!(
                            parent: span,
                            status = %response.status(),
                            latency_ms = latency.as_millis(),
                            "request completed"
                        );
                    },
                ),
        )
        .layer(axum::middleware::from_fn(middleware::request_context))
        .with_state(state)
}

async fn not_found() -> ApiError {
    ApiError::from(synveil_core::ErrorCode::NotFound)
}
