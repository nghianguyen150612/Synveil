#![forbid(unsafe_code)]

//! HTTP transport and runtime composition boundary for Synveil.
//!
//! Axum types, request handling, response envelopes, and middleware live in
//! this crate. The domain/core crates remain independent of HTTP and status
//! codes; application/domain errors are translated at this boundary.

mod error;
mod health;
mod middleware;
mod request_id;
mod router;
mod state;
mod telemetry;

pub use error::{ApiError, ErrorBody, ErrorResponse, map_core_error};
pub use request_id::{REQUEST_ID_HEADER, RequestContext, RequestId};
pub use router::{API_VERSION_PREFIX, DEFAULT_BODY_LIMIT_BYTES, router};
pub use state::{
    ApiState, DenySystemHealth, PlatformReadiness, ReadinessProbe, ReadinessSnapshot,
    StaticReadiness, SystemHealthAuthorizer,
};
pub use telemetry::init_tracing;

/// Build the application router using the supplied runtime/application state.
pub fn app(state: ApiState) -> axum::Router {
    router(state)
}

/// Compatibility spelling for composition roots that prefer an explicit
/// builder name.
pub fn build_router(state: ApiState) -> axum::Router {
    router(state)
}

/// Stable application-state name for embedding runtimes.
pub type AppState = ApiState;

#[cfg(test)]
mod tests;
