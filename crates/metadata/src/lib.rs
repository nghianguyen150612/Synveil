#![forbid(unsafe_code)]

//! PostgreSQL metadata and migration foundation for Synveil.
//!
//! This crate owns SQLx row types, explicit domain mappings, focused
//! repositories, connection pooling, migration execution, and database
//! readiness. PostgreSQL is the only supported database scheme; there is no
//! SQLite implementation or fallback.

mod auth;
mod config;
mod content;
mod errors;
mod files;
mod mapping;
mod migrations;
mod models;
mod pool;
mod purge;
mod readiness;
mod repository;
mod uploads;
mod versions;

pub use auth::{AuthRepository, BootstrapAttempt, BootstrapState};
pub use config::{DATABASE_URL_ENV, DatabaseConfig, PoolConfig};
pub use content::{
    AuthorizedContent, ContentReadMetadataBackend, ContentReadResolution,
    PostgresContentReadRepository,
};
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
pub use purge::{
    DEFAULT_PURGE_CANDIDATE_LIMIT, MAX_PURGE_CANDIDATE_LIMIT, PurgeCandidate, PurgeCandidatePage,
    PurgeError, PurgeExecutionResult, TrashRetentionService,
};
pub use readiness::DatabaseReadiness;
pub use repository::DomainRepository;
pub use uploads::{
    NewUploadSession, PostgresUploadRepository, UploadClaim, UploadCleanupCandidate,
    UploadCompletion, UploadDurabilityReceipt, UploadFinalization, UploadMetadataBackend,
    UploadSessionRecord,
};
pub use versions::{
    FileVersionMetadata, FileVersionPage, MAX_RESTORE_IDEMPOTENCY_KEY_BYTES,
    MIN_RESTORE_IDEMPOTENCY_KEY_BYTES, RestoredFileVersion, VersionHistoryBackend,
    VersionHistoryError, VersionHistoryService, VersionRestoreBackend, VersionRestoreError,
    VersionRestoreService,
};

pub use synveil_core;
