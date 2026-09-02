use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use synveil_core::ErrorCode;

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
    InvalidRequest,
    PayloadTooLarge,
    Unauthorized,
    ReadinessUnavailable,
    Internal,
}

impl From<ErrorCode> for ApiError {
    fn from(error: ErrorCode) -> Self {
        Self::Core(error)
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
                ErrorCode::InvalidCursor => StatusCode::BAD_REQUEST,
                ErrorCode::UploadExpired => StatusCode::GONE,
                ErrorCode::ChecksumMismatch => StatusCode::PRECONDITION_FAILED,
                ErrorCode::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::QuotaExceeded => StatusCode::INSUFFICIENT_STORAGE,
                ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::ReadinessUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Core(error) => error.as_str(),
            Self::InvalidRequest => "invalid_request",
            Self::PayloadTooLarge => "payload_too_large",
            Self::Unauthorized => "authentication_failed",
            Self::ReadinessUnavailable => "internal_dependency_unavailable",
            Self::Internal => "internal_error",
        }
    }

    #[must_use]
    pub const fn retryable(&self) -> bool {
        match self {
            Self::Core(error) => error.retryable(),
            Self::InvalidRequest | Self::PayloadTooLarge | Self::Unauthorized => false,
            Self::ReadinessUnavailable | Self::Internal => true,
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
                ErrorCode::InvalidCursor => "The supplied cursor is invalid.",
                ErrorCode::UploadExpired => "The upload session has expired.",
                ErrorCode::ChecksumMismatch => "The supplied content checksum does not match.",
                ErrorCode::StorageUnavailable => "Required storage is temporarily unavailable.",
                ErrorCode::QuotaExceeded => "The configured storage quota has been exceeded.",
                ErrorCode::RateLimited => "Too many requests; try again later.",
                ErrorCode::InternalError => "Synveil could not complete the request.",
            },
            Self::InvalidRequest => "The request is invalid.",
            Self::PayloadTooLarge => "The request body exceeds the permitted limit.",
            Self::Unauthorized => "Authentication is required.",
            Self::ReadinessUnavailable => "Required service dependencies are not ready.",
            Self::Internal => "Synveil could not complete the request.",
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
                details: None,
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
        response
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.response()
    }
}
