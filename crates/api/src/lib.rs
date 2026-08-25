#![forbid(unsafe_code)]

//! HTTP transport and runtime composition boundary for Synveil.
//!
//! Axum types, request handling, response envelopes, and middleware live in
//! this crate. The domain/core crates remain independent of HTTP and status
//! codes; application/domain errors are translated at this boundary.

mod auth;
mod cookies;
mod csrf;
mod downloads;
mod error;
mod etag;
mod files;
mod health;
mod middleware;
mod request_id;
mod router;
mod state;
mod telemetry;
mod uploads;
mod versions;

pub use auth::{
    AuthenticationBackend, BootstrapStatus, IssuedSession, PostgresAuthenticationBackend,
};
pub use cookies::{CSRF_COOKIE_NAME, CookieConfig, CookieSameSite, SESSION_COOKIE_NAME};
pub use csrf::CsrfKey;
pub use downloads::{DownloadBackend, DownloadMetadata, DownloadRead};
pub use error::{ApiError, ErrorBody, ErrorResponse, map_auth_error, map_core_error};
pub use etag::EtagKey;
pub use files::FILE_METADATA_BODY_LIMIT_BYTES;
pub use request_id::{REQUEST_ID_HEADER, RequestContext, RequestId};
pub use router::{
    API_VERSION_PREFIX, BOOTSTRAP_BODY_LIMIT_BYTES, DEFAULT_BODY_LIMIT_BYTES,
    LOGIN_BODY_LIMIT_BYTES, router,
};
pub use state::{
    ApiState, DenySystemHealth, PlatformReadiness, ReadinessProbe, ReadinessSnapshot,
    StaticReadiness, SystemHealthAuthorizer,
};
pub use telemetry::init_tracing;
pub use uploads::{UPLOAD_JSON_BODY_LIMIT_BYTES, UPLOAD_OFFSET_HEADER_NAME, UploadBackend};

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
