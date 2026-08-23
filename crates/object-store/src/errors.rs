use std::fmt;

use crate::StorageCapability;

/// Stable, backend-neutral failures from the object-store boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectStoreError {
    InvalidKey,
    InvalidRange,
    NotFound,
    AlreadyExists,
    PreconditionFailed,
    StagingNotFound,
    StagingConflict,
    UnsupportedCapability(StorageCapability),
    StorageUnavailable,
    IntegrityMismatch,
    InvalidRequest,
}

impl ObjectStoreError {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidKey => "invalid_storage_key",
            Self::InvalidRange => "invalid_range",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::PreconditionFailed => "precondition_failed",
            Self::StagingNotFound => "staging_not_found",
            Self::StagingConflict => "staging_conflict",
            Self::UnsupportedCapability(_) => "unsupported_storage_capability",
            Self::StorageUnavailable => "storage_unavailable",
            Self::IntegrityMismatch => "object_corrupt",
            Self::InvalidRequest => "invalid_storage_request",
        }
    }

    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(self, Self::StorageUnavailable)
    }
}

impl fmt::Display for ObjectStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCapability(capability) => {
                write!(formatter, "{}: {}", self.as_str(), capability.as_str())
            }
            _ => formatter.write_str(self.as_str()),
        }
    }
}

impl std::error::Error for ObjectStoreError {}
