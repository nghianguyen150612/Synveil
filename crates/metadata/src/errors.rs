use std::fmt;

use crate::mapping::MappingError;

/// Safe configuration failures. Connection URLs and credentials are never
/// carried in these values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseConfigError {
    MissingEnvironmentVariable { variable: &'static str },
    EmptyUrl,
    UnsupportedScheme,
    InvalidPoolConfiguration,
}

impl fmt::Display for DatabaseConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironmentVariable { variable } => {
                write!(
                    formatter,
                    "database configuration variable {variable} is missing"
                )
            }
            Self::EmptyUrl => formatter.write_str("database URL is empty"),
            Self::UnsupportedScheme => {
                formatter.write_str("database URL must use the PostgreSQL scheme")
            }
            Self::InvalidPoolConfiguration => {
                formatter.write_str("database pool configuration is invalid")
            }
        }
    }
}

impl std::error::Error for DatabaseConfigError {}

/// Stable database failure classes exposed above the SQLx adapter boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseErrorKind {
    ConnectionUnavailable,
    QueryFailed,
    MigrationFailed,
    MigrationStateUnavailable,
    ReadinessCheckFailed,
}

impl DatabaseErrorKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConnectionUnavailable => "database_connection_unavailable",
            Self::QueryFailed => "database_query_failed",
            Self::MigrationFailed => "database_migration_failed",
            Self::MigrationStateUnavailable => "database_migration_state_unavailable",
            Self::ReadinessCheckFailed => "database_readiness_check_failed",
        }
    }
}

/// Database error translation boundary. The underlying SQLx error is consumed
/// and intentionally not retained, preventing credentials or connection URLs
/// from leaking through normal error formatting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseError {
    Configuration(DatabaseConfigError),
    Failure(DatabaseErrorKind),
}

impl DatabaseError {
    #[must_use]
    pub const fn kind(self) -> Option<DatabaseErrorKind> {
        match self {
            Self::Configuration(_) => None,
            Self::Failure(kind) => Some(kind),
        }
    }

    pub(crate) fn connection(_error: sqlx::Error) -> Self {
        Self::Failure(DatabaseErrorKind::ConnectionUnavailable)
    }

    pub(crate) fn query(_error: sqlx::Error) -> Self {
        Self::Failure(DatabaseErrorKind::QueryFailed)
    }

    pub(crate) fn migration(_error: sqlx::migrate::MigrateError) -> Self {
        Self::Failure(DatabaseErrorKind::MigrationFailed)
    }

    pub(crate) fn migration_state(_error: sqlx::Error) -> Self {
        Self::Failure(DatabaseErrorKind::MigrationStateUnavailable)
    }

    pub(crate) const fn migration_state_decode() -> Self {
        Self::Failure(DatabaseErrorKind::MigrationStateUnavailable)
    }

    pub(crate) fn readiness(_error: sqlx::Error) -> Self {
        Self::Failure(DatabaseErrorKind::ReadinessCheckFailed)
    }
}

impl From<DatabaseConfigError> for DatabaseError {
    fn from(error: DatabaseConfigError) -> Self {
        Self::Configuration(error)
    }
}

impl From<sqlx::Error> for DatabaseError {
    fn from(error: sqlx::Error) -> Self {
        Self::query(error)
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(error) => error.fmt(formatter),
            Self::Failure(kind) => formatter.write_str(kind.as_str()),
        }
    }
}

impl std::error::Error for DatabaseError {}

/// Safe error boundary for row repositories. SQLx details are translated to
/// the existing sanitized database error classes, while invalid persisted
/// values remain explicit mapping failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataError {
    Database(DatabaseError),
    Mapping(MappingError),
    CapacityUnavailable,
}

impl MetadataError {
    #[must_use]
    pub const fn database_kind(self) -> Option<DatabaseErrorKind> {
        match self {
            Self::Database(error) => error.kind(),
            Self::Mapping(_) | Self::CapacityUnavailable => None,
        }
    }
}

impl From<DatabaseError> for MetadataError {
    fn from(error: DatabaseError) -> Self {
        Self::Database(error)
    }
}

impl From<MappingError> for MetadataError {
    fn from(error: MappingError) -> Self {
        Self::Mapping(error)
    }
}

impl From<synveil_core::DomainError> for MetadataError {
    fn from(error: synveil_core::DomainError) -> Self {
        Self::Mapping(MappingError::Domain(error))
    }
}

impl From<sqlx::Error> for MetadataError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(DatabaseError::query(error))
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => error.fmt(formatter),
            Self::Mapping(error) => error.fmt(formatter),
            Self::CapacityUnavailable => formatter.write_str("metadata capacity is unavailable"),
        }
    }
}

impl std::error::Error for MetadataError {}

#[cfg(test)]
mod tests {
    use super::{DatabaseConfigError, DatabaseError, DatabaseErrorKind};

    #[test]
    fn database_error_display_is_stable_and_does_not_include_connection_data() {
        let error = DatabaseError::Failure(DatabaseErrorKind::ConnectionUnavailable);

        assert_eq!(error.to_string(), "database_connection_unavailable");
        assert!(!error.to_string().contains("postgres"));
        assert_eq!(
            DatabaseError::Configuration(DatabaseConfigError::UnsupportedScheme).to_string(),
            "database URL must use the PostgreSQL scheme"
        );
    }
}
