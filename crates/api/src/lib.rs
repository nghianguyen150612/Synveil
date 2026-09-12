#![cfg_attr(not(test), forbid(unsafe_code))]

//! HTTP transport and runtime composition boundary for Synveil.
//!
//! Axum types, request handling, response envelopes, and middleware live in
//! this crate. The domain/core crates remain independent of HTTP and status
//! codes; application/domain errors are translated at this boundary.

mod auth;
mod backups;
mod conflicts;
mod cookies;
mod csrf;
mod device_auth;
mod downloads;
mod error;
mod etag;
mod files;
mod health;
mod middleware;
mod mutations;
mod rebaseline;
mod rebaseline_snapshot;
mod request_id;
mod router;
mod runtime_database_credential;
mod state;
mod sync;
mod telemetry;
mod uploads;
mod versions;

pub use auth::{
    AuthenticatedPrincipal, AuthenticationBackend, BootstrapStatus, IssuedSession,
    PostgresAuthenticationBackend,
};
pub use conflicts::{
    CONFLICT_RESOLUTION_BODY_LIMIT_BYTES, ConflictCursorError, ConflictCursorKey,
    MAX_CONFLICT_CURSOR_BYTES, PostgresConflictManagementBackend,
};
pub use cookies::{CSRF_COOKIE_NAME, CookieConfig, CookieSameSite, SESSION_COOKIE_NAME};
pub use csrf::CsrfKey;
pub use device_auth::{
    DEVICE_ENROLLMENT_BODY_LIMIT_BYTES, DeviceAuthenticationBackend,
    PostgresDeviceAuthenticationBackend,
};
pub use downloads::{DownloadBackend, DownloadMetadata, DownloadRead};
pub use error::{ApiError, ErrorBody, ErrorResponse, map_auth_error, map_core_error};
pub use etag::EtagKey;
pub use files::FILE_METADATA_BODY_LIMIT_BYTES;
pub use mutations::{CLIENT_MUTATION_BODY_LIMIT_BYTES, PostgresClientMutationBackend};
pub use rebaseline::{
    MAX_REBASELINE_COMPLETION_TOKEN_BYTES, MAX_REBASELINE_CURSOR_BYTES, PostgresRebaselineBackend,
    REBASELINE_BODY_LIMIT_BYTES, RebaselineBackend, RebaselineTokenError, RebaselineTokenKey,
    RebaselineTokenKeyParseError,
};
pub use rebaseline_snapshot::{
    DurableSnapshotBackend, PostgresDurableSnapshotBackend, SNAPSHOT_CREATE_BODY_LIMIT_BYTES,
};
pub use request_id::{REQUEST_ID_HEADER, RequestContext, RequestId};
pub use router::{
    API_VERSION_PREFIX, BACKUP_MUTATION_BODY_LIMIT_BYTES, BOOTSTRAP_BODY_LIMIT_BYTES,
    DEFAULT_BODY_LIMIT_BYTES, LOGIN_BODY_LIMIT_BYTES, router,
};
pub use runtime_database_credential::{
    CREDENTIAL_FILE_ENV, CREDENTIAL_ID, CREDENTIALS_DIRECTORY_ENV, MAX_CREDENTIAL_FILE_SIZE,
    RuntimeDatabaseCredentialError, credential_file_path_from_env, database_config_from_runtime,
    load_database_url_from_runtime_source,
};
pub use state::{
    ApiState, DenySystemHealth, PlatformReadiness, ReadinessProbe, ReadinessSnapshot,
    StaticReadiness, SystemHealthAuthorizer,
};
pub use sync::{
    MAX_SYNC_ACK_TOKEN_BYTES, PostgresSyncFeedBackend, SYNC_ACK_BODY_LIMIT_BYTES, SyncAckKey,
    SyncAckTokenError, SyncFeedBackend,
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
