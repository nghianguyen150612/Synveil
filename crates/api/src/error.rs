use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use synveil_auth::AuthError;
use synveil_core::ErrorCode;
use synveil_storage::UploadError;

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
    PreconditionRequired,
    InvalidRequest,
    InvalidUploadOffset,
    PayloadTooLarge,
    UnsupportedMediaType,
    Unauthorized,
    Forbidden,
    BootstrapClosed,
    ReadinessUnavailable,
    Internal,
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
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::InvalidUploadOffset => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::BootstrapClosed => StatusCode::CONFLICT,
            Self::ReadinessUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
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
            Self::PreconditionRequired => "precondition_required",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidUploadOffset => "invalid_offset",
            Self::PayloadTooLarge => "payload_too_large",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::Unauthorized => "authentication_failed",
            Self::Forbidden => "permission_denied",
            Self::BootstrapClosed => "bootstrap_closed",
            Self::ReadinessUnavailable => "internal_dependency_unavailable",
            Self::Internal => "internal_error",
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
            | Self::PreconditionRequired
            | Self::InvalidRequest
            | Self::InvalidUploadOffset
            | Self::PayloadTooLarge
            | Self::UnsupportedMediaType
            | Self::Unauthorized
            | Self::Forbidden
            | Self::BootstrapClosed => false,
            Self::ReadinessUnavailable | Self::Internal => true,
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
            Self::PreconditionRequired => "An If-Match precondition is required.",
            Self::InvalidRequest => "The request is invalid.",
            Self::InvalidUploadOffset => {
                "Upload-Offset must be one canonical unsigned decimal value."
            }
            Self::PayloadTooLarge => "The request body exceeds the permitted limit.",
            Self::UnsupportedMediaType => {
                "The request content type must be application/octet-stream."
            }
            Self::Unauthorized => "Authentication is required.",
            Self::Forbidden => "The request could not be verified.",
            Self::BootstrapClosed => "Initial setup is no longer available.",
            Self::ReadinessUnavailable => "Required service dependencies are not ready.",
            Self::Internal => "Synveil could not complete the request.",
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
            _ => None,
        }
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
