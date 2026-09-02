use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use synveil_auth::AuthError;
use synveil_core::{ConflictLifecycle, ConflictResolutionId, ErrorCode, SyncConflictId, Timestamp};
use synveil_metadata::MutationConflict;
use synveil_storage::{ContentReadError, UploadError};

/// Stable public error envelope matching the reviewed API contract.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub request_id: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Map<String, serde_json::Value>>,
}

/// Transport-only errors. Core error values are mapped here, keeping HTTP
/// status codes outside `synveil-core`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiError {
    Core(ErrorCode),
    VersionConflict {
        current_revision: Option<String>,
        current_etag: Option<String>,
    },
    IdempotencyConflict,
    MutationIdConflict,
    MutationConflict {
        conflict_id: SyncConflictId,
        conflict: Box<MutationConflict>,
        replayed: bool,
    },
    InvalidConflictResolution,
    ResolutionIdConflict,
    ConflictNotOpen {
        lifecycle: ConflictLifecycle,
    },
    ResolutionConflict {
        resolution_id: ConflictResolutionId,
        conflict: Box<MutationConflict>,
        completed_at: Timestamp,
        replayed: bool,
    },
    ConflictDependencyUnavailable,
    ConflictInvalidPersistedData,
    PreconditionRequired,
    InvalidRequest,
    InvalidMutation,
    InvalidUploadOffset,
    PayloadTooLarge,
    UnsupportedMediaType,
    Unauthorized,
    InvalidEnrollment,
    DeviceRevoked,
    Forbidden,
    BootstrapClosed,
    SyncInvalidLimit,
    SyncInvalidAckToken,
    SyncCheckpointConflict,
    RebaselineInvalidLimit,
    RebaselineInvalidCursor,
    RebaselineInvalidBootstrapToken,
    RebaselineBootstrapExpired,
    RebaselineBootstrapConflict,
    RebaselineDependencyUnavailable,
    RebaselineInvalidPersistedData,
    MutationDependencyUnavailable,
    MutationInvalidPersistedData,
    SyncRebaselineRequired {
        reason: &'static str,
        current_epoch: String,
        minimum_retained_sequence: String,
    },
    ReadinessUnavailable,
    Internal,
    RangeNotSatisfiable {
        length: u64,
    },
    Upload(UploadError),
}

impl From<ErrorCode> for ApiError {
    fn from(error: ErrorCode) -> Self {
        Self::Core(error)
    }
}

impl From<UploadError> for ApiError {
    fn from(error: UploadError) -> Self {
        Self::Upload(error)
    }
}

/// Explicit API-boundary mapping for domain/core errors.
#[must_use]
pub const fn map_core_error(error: ErrorCode) -> ApiError {
    ApiError::Core(error)
}

impl ApiError {
    #[must_use]
    pub const fn status_code(&self) -> StatusCode {
        match self {
            Self::Core(error) => match error {
                ErrorCode::AuthenticationFailed => StatusCode::UNAUTHORIZED,
                ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
                ErrorCode::NotFound => StatusCode::NOT_FOUND,
                ErrorCode::VersionConflict => StatusCode::CONFLICT,
                ErrorCode::InvalidState => StatusCode::CONFLICT,
                ErrorCode::InvalidCursor => StatusCode::BAD_REQUEST,
                ErrorCode::UploadExpired => StatusCode::GONE,
                ErrorCode::ChecksumMismatch => StatusCode::PRECONDITION_FAILED,
                ErrorCode::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::QuotaExceeded => StatusCode::INSUFFICIENT_STORAGE,
                ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Self::VersionConflict { .. } => StatusCode::CONFLICT,
            Self::IdempotencyConflict => StatusCode::CONFLICT,
            Self::MutationIdConflict
            | Self::MutationConflict { .. }
            | Self::ResolutionIdConflict
            | Self::ConflictNotOpen { .. }
            | Self::ResolutionConflict { .. } => StatusCode::CONFLICT,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::InvalidMutation | Self::InvalidConflictResolution => StatusCode::BAD_REQUEST,
            Self::InvalidUploadOffset => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::InvalidEnrollment | Self::DeviceRevoked => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::BootstrapClosed => StatusCode::CONFLICT,
            Self::SyncInvalidLimit
            | Self::SyncInvalidAckToken
            | Self::RebaselineInvalidLimit
            | Self::RebaselineInvalidCursor
            | Self::RebaselineInvalidBootstrapToken => StatusCode::BAD_REQUEST,
            Self::RebaselineBootstrapExpired => StatusCode::GONE,
            Self::SyncCheckpointConflict
            | Self::RebaselineBootstrapConflict
            | Self::SyncRebaselineRequired { .. } => StatusCode::CONFLICT,
            Self::ReadinessUnavailable
            | Self::RebaselineDependencyUnavailable
            | Self::MutationDependencyUnavailable
            | Self::ConflictDependencyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal
            | Self::RebaselineInvalidPersistedData
            | Self::MutationInvalidPersistedData
            | Self::ConflictInvalidPersistedData => StatusCode::INTERNAL_SERVER_ERROR,
            Self::RangeNotSatisfiable { .. } => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::Upload(error) => match error {
                UploadError::NotFound => StatusCode::NOT_FOUND,
                UploadError::InvalidRequest => StatusCode::BAD_REQUEST,
                UploadError::InvalidOffset { .. }
                | UploadError::SizeMismatch
                | UploadError::InvalidState { .. }
                | UploadError::VersionConflict { .. }
                | UploadError::CompletionConflict
                | UploadError::InProgress
                | UploadError::Failed { .. } => StatusCode::CONFLICT,
                UploadError::ChunkTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                UploadError::HashMismatch => StatusCode::PRECONDITION_FAILED,
                UploadError::UploadExpired => StatusCode::GONE,
                UploadError::StorageUnavailable | UploadError::DatabaseUnavailable => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                UploadError::CapacityUnavailable => StatusCode::INSUFFICIENT_STORAGE,
                UploadError::InvalidPersistedData => StatusCode::INTERNAL_SERVER_ERROR,
            },
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Core(error) => error.as_str(),
            Self::VersionConflict { .. } => "version_conflict",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::MutationIdConflict => "mutation_id_conflict",
            Self::MutationConflict { .. } => "mutation_conflict",
            Self::InvalidConflictResolution => "invalid_conflict_resolution",
            Self::ResolutionIdConflict => "resolution_id_conflict",
            Self::ConflictNotOpen { .. } => "conflict_not_open",
            Self::ResolutionConflict { .. } => "resolution_conflict",
            Self::ConflictDependencyUnavailable => "dependency_unavailable",
            Self::ConflictInvalidPersistedData => "invalid_persisted_data",
            Self::PreconditionRequired => "precondition_required",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidMutation => "invalid_mutation",
            Self::InvalidUploadOffset => "invalid_offset",
            Self::PayloadTooLarge => "payload_too_large",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::Unauthorized => "authentication_failed",
            Self::InvalidEnrollment => "invalid_enrollment",
            Self::DeviceRevoked => "device_revoked",
            Self::Forbidden => "permission_denied",
            Self::BootstrapClosed => "bootstrap_closed",
            Self::SyncInvalidLimit => "invalid_limit",
            Self::SyncInvalidAckToken => "invalid_ack_token",
            Self::SyncCheckpointConflict => "checkpoint_conflict",
            Self::RebaselineInvalidLimit => "invalid_limit",
            Self::RebaselineInvalidCursor => "invalid_cursor",
            Self::RebaselineInvalidBootstrapToken => "invalid_bootstrap_token",
            Self::RebaselineBootstrapExpired => "bootstrap_expired",
            Self::RebaselineBootstrapConflict => "bootstrap_conflict",
            Self::RebaselineDependencyUnavailable => "dependency_unavailable",
            Self::RebaselineInvalidPersistedData => "invalid_persisted_data",
            Self::MutationDependencyUnavailable => "dependency_unavailable",
            Self::MutationInvalidPersistedData => "invalid_persisted_data",
            Self::SyncRebaselineRequired { .. } => "sync_rebaseline_required",
            Self::ReadinessUnavailable => "internal_dependency_unavailable",
            Self::Internal => "internal_error",
            Self::RangeNotSatisfiable { .. } => "invalid_range",
            Self::Upload(error) => match error {
                UploadError::ChunkTooLarge => "payload_too_large",
                UploadError::DatabaseUnavailable => "internal_dependency_unavailable",
                UploadError::InvalidPersistedData => "internal_error",
                _ => error.as_str(),
            },
        }
    }

    #[must_use]
    pub const fn retryable(&self) -> bool {
        match self {
            Self::Core(error) => error.retryable(),
            Self::VersionConflict { .. }
            | Self::IdempotencyConflict
            | Self::MutationIdConflict
            | Self::MutationConflict { .. }
            | Self::InvalidConflictResolution
            | Self::ResolutionIdConflict
            | Self::ConflictNotOpen { .. }
            | Self::ResolutionConflict { .. }
            | Self::PreconditionRequired
            | Self::InvalidRequest
            | Self::InvalidMutation
            | Self::InvalidUploadOffset
            | Self::PayloadTooLarge
            | Self::UnsupportedMediaType
            | Self::Unauthorized
            | Self::InvalidEnrollment
            | Self::DeviceRevoked
            | Self::Forbidden
            | Self::BootstrapClosed
            | Self::SyncInvalidLimit
            | Self::SyncInvalidAckToken
            | Self::SyncCheckpointConflict
            | Self::RebaselineInvalidLimit
            | Self::RebaselineInvalidCursor
            | Self::RebaselineInvalidBootstrapToken
            | Self::RebaselineBootstrapExpired
            | Self::RebaselineBootstrapConflict
            | Self::RebaselineInvalidPersistedData
            | Self::MutationInvalidPersistedData
            | Self::ConflictInvalidPersistedData
            | Self::SyncRebaselineRequired { .. }
            | Self::RangeNotSatisfiable { .. } => false,
            Self::ReadinessUnavailable
            | Self::RebaselineDependencyUnavailable
            | Self::MutationDependencyUnavailable
            | Self::ConflictDependencyUnavailable
            | Self::Internal => true,
            Self::Upload(error) => error.retryable(),
        }
    }

    #[must_use]
    pub const fn message(&self) -> &'static str {
        match self {
            Self::Core(error) => match error {
                ErrorCode::AuthenticationFailed => "Authentication failed.",
                ErrorCode::PermissionDenied => "You are not allowed to perform this action.",
                ErrorCode::NotFound => "The requested resource was not found.",
                ErrorCode::VersionConflict => {
                    "The resource changed before this request was applied."
                }
                ErrorCode::InvalidState => {
                    "The requested metadata operation is not valid for the resource state."
                }
                ErrorCode::InvalidCursor => "The supplied cursor is invalid.",
                ErrorCode::UploadExpired => "The upload session has expired.",
                ErrorCode::ChecksumMismatch => "The supplied content checksum does not match.",
                ErrorCode::StorageUnavailable => "Required storage is temporarily unavailable.",
                ErrorCode::QuotaExceeded => "The configured storage quota has been exceeded.",
                ErrorCode::RateLimited => "Too many requests; try again later.",
                ErrorCode::InternalError => "Synveil could not complete the request.",
            },
            Self::VersionConflict { .. } => "The resource changed before this request was applied.",
            Self::IdempotencyConflict => {
                "The idempotency key was already used for a different restore request."
            }
            Self::MutationIdConflict => {
                "The mutation ID was already used for a different mutation."
            }
            Self::MutationConflict { .. } => "The mutation conflicts with newer canonical state.",
            Self::InvalidConflictResolution => "The conflict resolution request is invalid.",
            Self::ResolutionIdConflict => {
                "The resolution ID was already used for a different decision."
            }
            Self::ConflictNotOpen { .. } => "The conflict is no longer open.",
            Self::ResolutionConflict { .. } => {
                "The canonical resource changed before the resolution was applied."
            }
            Self::ConflictDependencyUnavailable => {
                "A required conflict-management dependency is unavailable."
            }
            Self::ConflictInvalidPersistedData => {
                "Conflict management encountered invalid persisted data."
            }
            Self::PreconditionRequired => "An If-Match precondition is required.",
            Self::InvalidRequest => "The request is invalid.",
            Self::InvalidMutation => "The client mutation is invalid.",
            Self::InvalidUploadOffset => {
                "Upload-Offset must be one canonical unsigned decimal value."
            }
            Self::PayloadTooLarge => "The request body exceeds the permitted limit.",
            Self::UnsupportedMediaType => {
                "The request content type must be application/octet-stream."
            }
            Self::Unauthorized => "Authentication is required.",
            Self::InvalidEnrollment => "The enrollment grant is invalid or no longer available.",
            Self::DeviceRevoked => "The device credential is no longer active.",
            Self::Forbidden => "The request could not be verified.",
            Self::BootstrapClosed => "Initial setup is no longer available.",
            Self::SyncInvalidLimit => "The synchronization feed limit is invalid.",
            Self::SyncInvalidAckToken => "The synchronization acknowledgment token is invalid.",
            Self::SyncCheckpointConflict => {
                "The synchronization checkpoint changed; fetch the feed again."
            }
            Self::RebaselineInvalidLimit => "The snapshot bootstrap page limit is invalid.",
            Self::RebaselineInvalidCursor => "The snapshot bootstrap cursor is invalid.",
            Self::RebaselineInvalidBootstrapToken => {
                "The snapshot bootstrap completion token is invalid."
            }
            Self::RebaselineBootstrapExpired => "The snapshot bootstrap session has expired.",
            Self::RebaselineBootstrapConflict => {
                "The snapshot bootstrap conflicts with newer synchronization progress."
            }
            Self::RebaselineDependencyUnavailable => {
                "A required snapshot bootstrap dependency is unavailable."
            }
            Self::RebaselineInvalidPersistedData => {
                "The snapshot bootstrap encountered invalid persisted data."
            }
            Self::MutationDependencyUnavailable => {
                "A required client mutation dependency is unavailable."
            }
            Self::MutationInvalidPersistedData => {
                "The client mutation encountered invalid persisted data."
            }
            Self::SyncRebaselineRequired { .. } => {
                "The synchronization checkpoint requires rebaseline."
            }
            Self::ReadinessUnavailable => "Required service dependencies are not ready.",
            Self::Internal => "Synveil could not complete the request.",
            Self::RangeNotSatisfiable { .. } => "The requested byte range cannot be satisfied.",
            Self::Upload(error) => match error {
                UploadError::NotFound => "The upload session was not found.",
                UploadError::InvalidRequest => "The upload request is invalid.",
                UploadError::InvalidOffset { .. } => {
                    "The supplied upload offset does not match the authoritative offset."
                }
                UploadError::ChunkTooLarge => "The upload chunk exceeds the permitted limit.",
                UploadError::SizeMismatch => {
                    "The uploaded byte length does not match the declared length."
                }
                UploadError::HashMismatch => {
                    "The uploaded content checksum does not match the declared checksum."
                }
                UploadError::InvalidState { .. } => {
                    "The upload operation is not valid for the current session state."
                }
                UploadError::UploadExpired => "The upload session has expired.",
                UploadError::VersionConflict { .. } => {
                    "The target changed before the upload was committed."
                }
                UploadError::CompletionConflict => {
                    "The upload completion conflicts with an existing result."
                }
                UploadError::InProgress => "The upload is already being completed.",
                UploadError::StorageUnavailable => "Required storage is temporarily unavailable.",
                UploadError::CapacityUnavailable => {
                    "The upload cannot be admitted within current capacity."
                }
                UploadError::DatabaseUnavailable => "Required service dependencies are not ready.",
                UploadError::InvalidPersistedData => "Synveil could not complete the request.",
                UploadError::Failed { .. } => "The upload session has failed.",
            },
        }
    }

    #[must_use]
    pub(crate) fn response(&self) -> Response {
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code().to_owned(),
                message: self.message().to_owned(),
                request_id: "pending".to_owned(),
                retryable: self.retryable(),
                details: self.details(),
            },
        };
        let mut response = (self.status_code(), Json(body)).into_response();
        if self.status_code() == StatusCode::SERVICE_UNAVAILABLE
            || self.status_code() == StatusCode::TOO_MANY_REQUESTS
        {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("1"),
            );
        }
        if let Self::Upload(UploadError::InvalidOffset { current_offset }) = self {
            response.headers_mut().insert(
                crate::uploads::UPLOAD_OFFSET_HEADER_NAME,
                HeaderValue::from_str(&current_offset.to_string())
                    .expect("a decimal upload offset is valid HTTP header data"),
            );
        }
        if let Self::RangeNotSatisfiable { length } = self {
            response.headers_mut().insert(
                axum::http::header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{length}"))
                    .expect("content range is valid HTTP header data"),
            );
        }
        response
    }

    fn details(&self) -> Option<Map<String, serde_json::Value>> {
        match self {
            Self::VersionConflict {
                current_revision,
                current_etag,
            } => {
                let mut details = Map::new();
                details.insert(
                    "resource_type".to_owned(),
                    serde_json::Value::String("node".to_owned()),
                );
                if let Some(revision) = current_revision {
                    details.insert(
                        "current_revision".to_owned(),
                        serde_json::Value::String(revision.clone()),
                    );
                }
                if let Some(etag) = current_etag {
                    details.insert("etag".to_owned(), serde_json::Value::String(etag.clone()));
                }
                Some(details)
            }
            Self::MutationConflict {
                conflict_id,
                conflict,
                replayed,
            } => {
                let mut details = Map::new();
                details.insert(
                    "outcome".to_owned(),
                    serde_json::Value::String("CONFLICT".to_owned()),
                );
                details.insert("replayed".to_owned(), serde_json::Value::Bool(*replayed));
                details.insert(
                    "conflict_id".to_owned(),
                    serde_json::Value::String(conflict_id.to_string()),
                );
                details.insert(
                    "reason".to_owned(),
                    serde_json::Value::String(conflict.reason().as_str().to_owned()),
                );
                details.insert(
                    "resource_id".to_owned(),
                    serde_json::Value::String(conflict.resource_id().to_string()),
                );
                if let Some(revision) = conflict.expected_revision() {
                    details.insert(
                        "expected_revision".to_owned(),
                        serde_json::Value::String(revision.to_string()),
                    );
                }
                if let Some(revision) = conflict.current_revision() {
                    details.insert(
                        "current_revision".to_owned(),
                        serde_json::Value::String(revision.to_string()),
                    );
                }
                if let Some(state) = conflict.current_state() {
                    details.insert(
                        "current_state".to_owned(),
                        serde_json::Value::String(state.as_str().to_owned()),
                    );
                }
                if let Some(parent_id) = conflict.current_parent_id() {
                    details.insert(
                        "current_parent_id".to_owned(),
                        serde_json::Value::String(parent_id.to_string()),
                    );
                }
                if let Some(name) = conflict.current_name() {
                    details.insert(
                        "current_name".to_owned(),
                        serde_json::Value::String(name.as_str().to_owned()),
                    );
                }
                details.insert(
                    "server_epoch".to_owned(),
                    serde_json::Value::String(conflict.server_epoch().to_string()),
                );
                details.insert(
                    "server_sequence".to_owned(),
                    serde_json::Value::String(conflict.server_sequence().to_string()),
                );
                Some(details)
            }
            Self::ConflictNotOpen { lifecycle } => {
                let mut details = Map::new();
                details.insert(
                    "lifecycle".to_owned(),
                    serde_json::Value::String(lifecycle.as_str().to_owned()),
                );
                Some(details)
            }
            Self::ResolutionConflict {
                resolution_id,
                conflict,
                completed_at,
                replayed,
            } => {
                let mut details = mutation_conflict_details(conflict);
                details.insert(
                    "resolution_id".to_owned(),
                    serde_json::Value::String(resolution_id.to_string()),
                );
                details.insert("replayed".to_owned(), serde_json::Value::Bool(*replayed));
                details.insert(
                    "completed_at".to_owned(),
                    serde_json::Value::String(completed_at.to_string()),
                );
                Some(details)
            }
            Self::Upload(UploadError::InvalidOffset { current_offset }) => {
                let mut details = Map::new();
                details.insert(
                    "resource_type".to_owned(),
                    serde_json::Value::String("upload_session".to_owned()),
                );
                details.insert(
                    "current_offset".to_owned(),
                    serde_json::Value::String(current_offset.to_string()),
                );
                Some(details)
            }
            Self::Upload(UploadError::VersionConflict { current_revision }) => {
                let mut details = Map::new();
                details.insert(
                    "resource_type".to_owned(),
                    serde_json::Value::String("node".to_owned()),
                );
                if let Some(revision) = current_revision {
                    details.insert(
                        "current_revision".to_owned(),
                        serde_json::Value::String(revision.to_string()),
                    );
                }
                Some(details)
            }
            Self::Upload(UploadError::InvalidState { state }) => {
                let mut details = Map::new();
                details.insert(
                    "current_state".to_owned(),
                    serde_json::Value::String(state.to_string()),
                );
                Some(details)
            }
            Self::SyncRebaselineRequired {
                reason,
                current_epoch,
                minimum_retained_sequence,
            } => {
                let mut details = Map::new();
                details.insert(
                    "reason".to_owned(),
                    serde_json::Value::String((*reason).to_owned()),
                );
                details.insert(
                    "current_epoch".to_owned(),
                    serde_json::Value::String(current_epoch.clone()),
                );
                details.insert(
                    "minimum_retained_sequence".to_owned(),
                    serde_json::Value::String(minimum_retained_sequence.clone()),
                );
                Some(details)
            }
            _ => None,
        }
    }
}

fn mutation_conflict_details(conflict: &MutationConflict) -> Map<String, serde_json::Value> {
    let mut details = Map::new();
    details.insert(
        "reason".to_owned(),
        serde_json::Value::String(conflict.reason().as_str().to_owned()),
    );
    details.insert(
        "resource_id".to_owned(),
        serde_json::Value::String(conflict.resource_id().to_string()),
    );
    if let Some(revision) = conflict.expected_revision() {
        details.insert(
            "expected_revision".to_owned(),
            serde_json::Value::String(revision.to_string()),
        );
    }
    if let Some(revision) = conflict.current_revision() {
        details.insert(
            "current_revision".to_owned(),
            serde_json::Value::String(revision.to_string()),
        );
    }
    if let Some(state) = conflict.current_state() {
        details.insert(
            "current_state".to_owned(),
            serde_json::Value::String(state.as_str().to_owned()),
        );
    }
    if let Some(parent_id) = conflict.current_parent_id() {
        details.insert(
            "current_parent_id".to_owned(),
            serde_json::Value::String(parent_id.to_string()),
        );
    }
    if let Some(name) = conflict.current_name() {
        details.insert(
            "current_name".to_owned(),
            serde_json::Value::String(name.as_str().to_owned()),
        );
    }
    details.insert(
        "server_epoch".to_owned(),
        serde_json::Value::String(conflict.server_epoch().to_string()),
    );
    details.insert(
        "server_sequence".to_owned(),
        serde_json::Value::String(conflict.server_sequence().to_string()),
    );
    details
}

/// Map transport-neutral immutable content-read failures to the stable HTTP
/// error registry. A range failure carries the already-resolved length so the
/// caller receives RFC-compatible `Content-Range: bytes */length` without
/// exposing storage details.
#[must_use]
pub(crate) fn map_content_read_error(
    error: ContentReadError,
    resolved_length: Option<u64>,
) -> ApiError {
    match error {
        ContentReadError::ContentNotFound | ContentReadError::VersionNotFound => {
            ApiError::Core(ErrorCode::NotFound)
        }
        ContentReadError::NotAFile => ApiError::Core(ErrorCode::InvalidState),
        ContentReadError::ContentUnavailable | ContentReadError::StorageUnavailable => {
            ApiError::Core(ErrorCode::StorageUnavailable)
        }
        ContentReadError::IntegrityMismatch => ApiError::Internal,
        ContentReadError::InvalidRange => resolved_length.map_or(ApiError::Internal, |length| {
            ApiError::RangeNotSatisfiable { length }
        }),
    }
}

/// Map transport-neutral authentication failures to the reviewed public
/// error registry without exposing credential, persistence, or configuration
/// details.
#[must_use]
pub const fn map_auth_error(error: AuthError) -> ApiError {
    match error {
        AuthError::InvalidCredentials
        | AuthError::InvalidSession
        | AuthError::CredentialNotFound => ApiError::Unauthorized,
        AuthError::BootstrapClosed => ApiError::BootstrapClosed,
        AuthError::Persistence(_) => ApiError::ReadinessUnavailable,
        AuthError::Password(_)
        | AuthError::InvalidPersistedData
        | AuthError::BootstrapStateInvalid
        | AuthError::SessionConfiguration(_) => ApiError::Internal,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.response()
    }
}
