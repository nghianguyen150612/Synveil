use std::{fmt, str::FromStr};

/// Stable, transport-neutral error codes from the reviewed core registry.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ErrorCode {
    AuthenticationFailed,
    PermissionDenied,
    NotFound,
    VersionConflict,
    InvalidState,
    InvalidCursor,
    UploadExpired,
    ChecksumMismatch,
    StorageUnavailable,
    QuotaExceeded,
    RateLimited,
    InternalError,
}

/// The core error taxonomy is intentionally independent of HTTP response types.
pub type CoreError = ErrorCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownErrorCode;

impl fmt::Display for UnknownErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown error code")
    }
}

impl std::error::Error for UnknownErrorCode {}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationFailed => "authentication_failed",
            Self::PermissionDenied => "permission_denied",
            Self::NotFound => "not_found",
            Self::VersionConflict => "version_conflict",
            Self::InvalidState => "invalid_state",
            Self::InvalidCursor => "invalid_cursor",
            Self::UploadExpired => "upload_expired",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::StorageUnavailable => "storage_unavailable",
            Self::QuotaExceeded => "quota_exceeded",
            Self::RateLimited => "rate_limited",
            Self::InternalError => "internal_error",
        }
    }

    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::StorageUnavailable | Self::RateLimited | Self::InternalError
        )
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for ErrorCode {}

impl FromStr for ErrorCode {
    type Err = UnknownErrorCode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let code = match value {
            "authentication_failed" => Self::AuthenticationFailed,
            "permission_denied" => Self::PermissionDenied,
            "not_found" => Self::NotFound,
            "version_conflict" => Self::VersionConflict,
            "invalid_state" => Self::InvalidState,
            "invalid_cursor" => Self::InvalidCursor,
            "upload_expired" => Self::UploadExpired,
            "checksum_mismatch" => Self::ChecksumMismatch,
            "storage_unavailable" => Self::StorageUnavailable,
            "quota_exceeded" => Self::QuotaExceeded,
            "rate_limited" => Self::RateLimited,
            "internal_error" => Self::InternalError,
            _ => return Err(UnknownErrorCode),
        };

        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{ErrorCode, UnknownErrorCode};

    #[test]
    fn error_codes_are_stable_and_round_trip() {
        let codes = [
            (ErrorCode::AuthenticationFailed, "authentication_failed"),
            (ErrorCode::PermissionDenied, "permission_denied"),
            (ErrorCode::NotFound, "not_found"),
            (ErrorCode::VersionConflict, "version_conflict"),
            (ErrorCode::InvalidState, "invalid_state"),
            (ErrorCode::InvalidCursor, "invalid_cursor"),
            (ErrorCode::UploadExpired, "upload_expired"),
            (ErrorCode::ChecksumMismatch, "checksum_mismatch"),
            (ErrorCode::StorageUnavailable, "storage_unavailable"),
            (ErrorCode::QuotaExceeded, "quota_exceeded"),
            (ErrorCode::RateLimited, "rate_limited"),
            (ErrorCode::InternalError, "internal_error"),
        ];

        for (code, serialized) in codes {
            assert_eq!(code.as_str(), serialized);
            assert_eq!(ErrorCode::from_str(serialized), Ok(code));
            assert_eq!(code.to_string(), serialized);
        }
    }

    #[test]
    fn unknown_error_codes_do_not_echo_untrusted_input() {
        assert_eq!(
            ErrorCode::from_str("unknown\nsecret"),
            Err(UnknownErrorCode)
        );
        assert!(ErrorCode::InternalError.retryable());
        assert!(!ErrorCode::NotFound.retryable());
    }
}
