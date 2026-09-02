use std::fmt;

use synveil_metadata::{DatabaseError, MetadataError};

use crate::{PasswordError, SessionConfigError, SessionTokenError};

/// Stable, transport-neutral failures for authentication foundation flows.
/// Values that could contain passwords or PHC strings are never carried here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthError {
    Password(PasswordError),
    Persistence(DatabaseError),
    InvalidPersistedData,
    BootstrapClosed,
    BootstrapStateInvalid,
    CredentialNotFound,
    InvalidCredentials,
    InvalidSession,
    SessionConfiguration(SessionConfigError),
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Password(error) => return error.fmt(formatter),
            Self::Persistence(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "authentication_persisted_data_invalid",
            Self::BootstrapClosed => "bootstrap_closed",
            Self::BootstrapStateInvalid => "authentication_bootstrap_state_invalid",
            Self::CredentialNotFound => "password_credential_not_found",
            Self::InvalidCredentials => "invalid_credentials",
            Self::InvalidSession => "invalid_session",
            Self::SessionConfiguration(error) => return error.fmt(formatter),
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AuthError {}

impl From<PasswordError> for AuthError {
    fn from(error: PasswordError) -> Self {
        Self::Password(error)
    }
}

impl From<SessionConfigError> for AuthError {
    fn from(error: SessionConfigError) -> Self {
        Self::SessionConfiguration(error)
    }
}

impl From<SessionTokenError> for AuthError {
    fn from(_error: SessionTokenError) -> Self {
        // Raw-token syntax is deliberately not distinguished from an unknown,
        // expired, or revoked session at the service boundary.
        Self::InvalidSession
    }
}

impl From<MetadataError> for AuthError {
    fn from(error: MetadataError) -> Self {
        match error {
            MetadataError::Database(error) => Self::Persistence(error),
            MetadataError::Mapping(_) => Self::InvalidPersistedData,
        }
    }
}
