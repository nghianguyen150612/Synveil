#![forbid(unsafe_code)]

//! PostgreSQL metadata and migration foundation for Synveil.
//!
//! This crate owns SQLx row types, explicit domain mappings, focused
//! repositories, connection pooling, migration execution, and database
//! readiness. PostgreSQL is the only supported database scheme; there is no
//! SQLite implementation or fallback.

mod auth;
mod backup;
mod config;
mod conflicts;
mod content;
mod device_credentials;
mod errors;
mod files;
mod gc;
mod gc_worker;
mod journal;
mod mapping;
mod migrations;
mod models;
mod mutations;
mod physical_gc;
mod pool;
mod purge;
mod readiness;
mod rebaseline;
mod repository;
mod sync;
mod uploads;
mod versions;

pub use auth::{AuthRepository, BootstrapAttempt, BootstrapState};
pub use backup::{
    BackupError, BackupMaintenanceRunRow, BackupPruneExecutionObjectResultRow,
    BackupPruneExecutionRow, BackupPrunePlanEntryRow, BackupPrunePlanRow,
    BackupRestoreExecutionEntryRow, BackupRestoreExecutionRow, BackupRestorePlanEntryRow,
    BackupRestorePlanRow, BackupService, BackupSetRow, BackupSnapshotExpiryExecutionEntryRow,
    BackupSnapshotExpiryExecutionRow, BackupSnapshotExpiryPlanEntryRow,
    BackupSnapshotExpiryPlanRow, BackupSnapshotNodeRow, BackupSnapshotRetentionPolicyRevisionRow,
    BackupSnapshotRow, DEFAULT_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT,
    DEFAULT_BACKUP_RESTORE_PLAN_ENTRY_PAGE_LIMIT, DEFAULT_BACKUP_SET_PAGE_LIMIT,
    DEFAULT_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT, DEFAULT_BACKUP_SNAPSHOT_PAGE_LIMIT,
    MAX_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT, MAX_BACKUP_RESTORE_PLAN_ENTRY_PAGE_LIMIT,
    MAX_BACKUP_SET_PAGE_LIMIT, MAX_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT,
    MAX_BACKUP_SNAPSHOT_PAGE_LIMIT,
};
pub use config::{DATABASE_URL_ENV, DatabaseConfig, PoolConfig};
pub use conflicts::{
    ConflictManagementBackend, ConflictManagementError, ConflictManagementService, ConflictPage,
    ConflictPagePosition, ConflictResolutionResult, ConflictTerminalResolution,
    DEFAULT_CONFLICT_PAGE_LIMIT, MAX_CONFLICT_PAGE_LIMIT, SyncConflictRecord,
};
pub use content::{
    AuthorizedContent, ContentReadMetadataBackend, ContentReadResolution,
    PostgresContentReadRepository,
};
pub use device_credentials::{
    ConsumedDeviceEnrollment, DeviceCredentialRepository, DeviceCredentialRepositoryError,
    NewDeviceEnrollmentGrant, StoredDeviceCredential,
};
pub use errors::{DatabaseConfigError, DatabaseError, DatabaseErrorKind, MetadataError};
pub use files::{
    DEFAULT_PAGE_LIMIT, FileMetadataBackend, FileMetadataError, FileMetadataService, LibraryPage,
    MAX_PAGE_LIMIT, NodePage,
};
pub use gc::{
    GcLeaseId, ObjectGcCandidate, ObjectGcCandidateState, ObjectGcError, ObjectGcLease,
    ObjectGcLeaseReleaseResult, ObjectGcPlanResult, ObjectGcPlanningService,
};
pub use gc_worker::{
    MAX_GC_WORKER_RECONCILIATION_LIMIT, ObjectGcReconciliationReport, ObjectGcRecoveryClaim,
    ObjectGcWorkerMetadataError, PostgresObjectGcWorkerRepository,
};
pub use journal::{
    ChangeJournalPage, ChangeJournalService, DEFAULT_JOURNAL_PAGE_LIMIT, JournalCursor,
    JournalCursorError, JournalError, JournalHighWatermark, JournalReadService,
    MAX_JOURNAL_CURSOR_BYTES, MAX_JOURNAL_PAGE_LIMIT,
};
pub use mapping::{
    MappingError, revision_from_decimal, revision_to_decimal, sequence_from_decimal,
    sequence_to_decimal,
};
pub use migrations::{MigrationRunner, MigrationStatus};
pub use models::{
    DeviceRow, DeviceSyncCheckpointRow, FileVersionRow, LibraryRow, NodeRow, ObjectReplicaRow,
    ObjectRow, SessionRow, UploadSessionRow, UserCredentialRow, UserLoginRow, UserRow,
};
pub use mutations::{
    ClientMutationBackend, ClientMutationError, ClientMutationResult, ClientMutationService,
    MutationConflict, MutationConflictReason, MutationConflictReasonParseError,
};
pub use physical_gc::{
    ObjectGcExecutionMetadataBackend, ObjectGcExecutionMetadataError, ObjectGcExecutionState,
    ObjectGcOperation, ObjectGcReplicaAction, ObjectGcReplicaActionState, ObjectGcReplicaDirective,
    ObjectGcReplicaObservation, PostgresObjectGcExecutionRepository,
};
pub use pool::DatabasePool;
pub use purge::{
    DEFAULT_PURGE_CANDIDATE_LIMIT, MAX_PURGE_CANDIDATE_LIMIT, PurgeCandidate, PurgeCandidatePage,
    PurgeError, PurgeExecutionResult, TrashRetentionService,
};
pub use readiness::DatabaseReadiness;
pub use rebaseline::{
    BootstrapCompletion, BootstrapCompletionEvidence, BootstrapPagePosition,
    BootstrapTerminalEvidence, DEFAULT_SYNC_BOOTSTRAP_PAGE_LIMIT,
    DEFAULT_SYNC_BOOTSTRAP_TTL_SECONDS, MAX_SYNC_BOOTSTRAP_CLEANUP_LIMIT,
    MAX_SYNC_BOOTSTRAP_PAGE_LIMIT, RebaselineError, SnapshotNodePage, SyncBootstrapService,
};
pub use repository::DomainRepository;
pub use sync::{
    DEFAULT_SYNC_FEED_LIMIT, DeviceSyncService, MAX_SYNC_FEED_LIMIT, RebaselineReason,
    SyncAckEvidence, SyncError, SyncFeedPage,
};
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
