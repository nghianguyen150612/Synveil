#![forbid(unsafe_code)]

//! PostgreSQL metadata and migration foundation for Synveil.
//!
//! This crate owns SQLx row types, explicit domain mappings, focused
//! repositories, connection pooling, migration execution, and database
//! readiness. PostgreSQL is the only supported database scheme; there is no
//! SQLite implementation or fallback.

mod auth;
mod config;
mod errors;
mod files;
mod mapping;
mod migrations;
mod models;
mod pool;
mod readiness;
mod repository;
mod uploads;

pub use auth::{AuthRepository, BootstrapAttempt, BootstrapState};
pub use config::{DATABASE_URL_ENV, DatabaseConfig, PoolConfig};
pub use errors::{DatabaseConfigError, DatabaseError, DatabaseErrorKind, MetadataError};
pub use files::{
    DEFAULT_PAGE_LIMIT, FileMetadataBackend, FileMetadataError, FileMetadataService, LibraryPage,
    MAX_PAGE_LIMIT, NodePage,
};
pub use mapping::{
    MappingError, revision_from_decimal, revision_to_decimal, sequence_from_decimal,
    sequence_to_decimal,
};
pub use migrations::{MigrationRunner, MigrationStatus};
pub use models::{
    DeviceRow, FileVersionRow, LibraryRow, NodeRow, ObjectReplicaRow, ObjectRow, SessionRow,
    UploadSessionRow, UserCredentialRow, UserLoginRow, UserRow,
};
pub use pool::DatabasePool;
pub use readiness::DatabaseReadiness;
pub use repository::DomainRepository;
pub use uploads::{
    NewUploadSession, PostgresUploadRepository, UploadClaim, UploadCleanupCandidate,
    UploadCompletion, UploadDurabilityReceipt, UploadFinalization, UploadMetadataBackend,
    UploadSessionRecord,
};

pub use synveil_core;
