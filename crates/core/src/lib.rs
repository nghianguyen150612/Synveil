#![forbid(unsafe_code)]

//! Platform-neutral foundation boundary for Synveil.
//!
//! This crate contains portable domain primitives only. It must remain free of
//! HTTP, database, operating-system, and storage-backend dependencies.

mod config;
mod device_secrets;
mod domain;
mod errors;
mod hashes;
mod ids;
mod numbers;
mod time;
mod tokens;

pub use config::{
    CacheDir, ConfigDir, DEFAULT_GC_WORKER_CYCLE_INTERVAL, DEFAULT_GC_WORKER_ENABLED,
    DEFAULT_GC_WORKER_MAX_ACTIVE_OPERATIONS, DEFAULT_GC_WORKER_MAX_ATTEMPTS,
    DEFAULT_GC_WORKER_MAX_CANDIDATE_CLAIMS, DEFAULT_GC_WORKER_MAX_CONCURRENT_EXECUTIONS,
    DEFAULT_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES, DEFAULT_GC_WORKER_MAX_REPLICA_ACTIONS,
    DEFAULT_GC_WORKER_RETRY_BASE, DEFAULT_GC_WORKER_RETRY_MAX, DEFAULT_GC_WORKER_SHUTDOWN_TIMEOUT,
    DEFAULT_OBJECT_GC_GRACE_PERIOD, DEFAULT_OBJECT_GC_LEASE_DURATION,
    DEFAULT_OBJECT_GC_MAX_BATCH_SIZE, DEFAULT_TRASH_RETENTION, DataDir,
    GC_WORKER_CYCLE_INTERVAL_SECONDS_ENV, GC_WORKER_ENABLED_ENV,
    GC_WORKER_MAX_ACTIVE_OPERATIONS_ENV, GC_WORKER_MAX_ATTEMPTS_ENV,
    GC_WORKER_MAX_CANDIDATE_CLAIMS_ENV, GC_WORKER_MAX_CONCURRENT_EXECUTIONS_ENV,
    GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES_ENV, GC_WORKER_MAX_REPLICA_ACTIONS_ENV,
    GC_WORKER_RETRY_BASE_SECONDS_ENV, GC_WORKER_RETRY_MAX_SECONDS_ENV,
    GC_WORKER_SHUTDOWN_TIMEOUT_SECONDS_ENV, GcWorkerConfig, GcWorkerConfigError,
    GcWorkerRetryPolicy, GcWorkerRetryPolicyError, MAX_GC_WORKER_ACTIVE_OPERATIONS,
    MAX_GC_WORKER_ATTEMPTS, MAX_GC_WORKER_CANDIDATE_CLAIMS, MAX_GC_WORKER_CONCURRENT_EXECUTIONS,
    MAX_GC_WORKER_CONCURRENT_REPLICA_DELETES, MAX_GC_WORKER_CYCLE_INTERVAL,
    MAX_GC_WORKER_REPLICA_ACTIONS, MAX_GC_WORKER_RETRY_DELAY, MAX_GC_WORKER_SHUTDOWN_TIMEOUT,
    MAX_OBJECT_GC_BATCH_SIZE, OBJECT_GC_GRACE_SECONDS_ENV, OBJECT_GC_LEASE_SECONDS_ENV,
    OBJECT_GC_MAX_BATCH_SIZE_ENV, ObjectGcPolicy, ObjectGcPolicyError, RuntimeDir,
    TRASH_RETENTION_SECONDS_ENV, TrashRetentionPolicy, TrashRetentionPolicyError,
};
pub use device_secrets::{
    DEVICE_SECRET_ENCODED_BYTES, DEVICE_SECRET_ENTROPY_BYTES, DeviceCredentialSecret,
    DeviceSecretError, EnrollmentSecret,
};
pub use domain::{
    BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION, BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION,
    BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION, BACKUP_SCHEDULE_FINGERPRINT_VERSION,
    BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION, BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION,
    BACKUP_SNAPSHOT_EXPIRY_EXECUTION_FINGERPRINT_VERSION,
    BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION,
    BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION, BackupMaintenanceRun,
    BackupMaintenanceRunIdempotencyFingerprint, BackupMaintenanceRunPreflightIssue,
    BackupMaintenanceRunRequest, BackupMaintenanceRunState, BackupMaintenanceRunStateParseError,
    BackupManifestContent, BackupOperationKind, BackupOperationKindParseError,
    BackupPruneExecution, BackupPruneExecutionPreflightIssue, BackupPruneImpact,
    BackupPruneImpactParseError, BackupPrunePlan, BackupPrunePlanEntry,
    BackupPrunePlanIdempotencyFingerprint, BackupPrunePlanRequest, BackupPrunePlanState,
    BackupPrunePlanStateParseError, BackupPrunePreflightIssue, BackupRestoreAction,
    BackupRestoreActionParseError, BackupRestoreExecution, BackupRestoreExecutionEntry,
    BackupRestorePlan, BackupRestorePlanEntry, BackupRestorePlanIdempotencyFingerprint,
    BackupRestorePlanRequest, BackupRestorePlanState, BackupRestorePlanStateParseError,
    BackupRestorePreflightIssue, BackupSchedule, BackupScheduleConfig,
    BackupScheduleIdempotencyFingerprint, BackupScheduleLocalTime, BackupScheduleMisfireMode,
    BackupScheduleMisfireModeParseError, BackupScheduleMisfireSkip, BackupScheduleOccurrence,
    BackupScheduleOccurrenceHandoff, BackupScheduleOccurrenceHandoffResult,
    BackupScheduleOccurrenceMaterializationResult, BackupScheduleOccurrenceNotEffectiveReason,
    BackupScheduleRecurrenceKind, BackupScheduleRecurrenceKindParseError, BackupScheduleRequest,
    BackupScheduleRevision, BackupScheduleRevisionNumber, BackupScheduleTimezone,
    BackupScheduleTimezoneParseError, BackupScheduleWeekday, BackupScheduleWeekdayParseError,
    BackupScheduledMaintenanceClaim, BackupScheduledMaintenanceClaimOutcome,
    BackupScheduledMaintenanceStepResult, BackupSchedulerSkipOutcome, BackupSchedulerTickOutcome,
    BackupSchedulerTickResult, BackupSet, BackupSetState, BackupSetStateParseError, BackupSnapshot,
    BackupSnapshotExpiryBasisEntry, BackupSnapshotExpiryBasisFingerprint,
    BackupSnapshotExpiryDecision, BackupSnapshotExpiryDecisionParseError,
    BackupSnapshotExpiryExecution, BackupSnapshotExpiryExecutionEntry,
    BackupSnapshotExpiryExecutionPreflightIssue, BackupSnapshotExpiryPlan,
    BackupSnapshotExpiryPlanEntry, BackupSnapshotExpiryPlanIdempotencyFingerprint,
    BackupSnapshotExpiryPlanRequest, BackupSnapshotExpiryPlanState,
    BackupSnapshotExpiryPlanStateParseError, BackupSnapshotExpiryPreflightIssue,
    BackupSnapshotNode, BackupSnapshotRetentionPolicyConfig,
    BackupSnapshotRetentionPolicyIdempotencyFingerprint, BackupSnapshotRetentionPolicyRequest,
    BackupSnapshotRetentionPolicyRevision, BackupSnapshotRetentionPolicyRevisionNumber,
    BackupSource, BackupSourceParseError, CLIENT_MUTATION_FINGERPRINT_VERSION,
    CONFLICT_RESOLUTION_FINGERPRINT_VERSION, ChangeEvent, ChangeKind, ChangeKindParseError,
    ChangeResourceKind, ChangeResourceKindParseError, ClientMutation, ClientMutationFingerprint,
    ClientMutationKind, ClientMutationKindParseError, ClientMutationRequest, ConflictLifecycle,
    ConflictLifecycleParseError, ConflictResolutionAction, ConflictResolutionActionParseError,
    ConflictResolutionFingerprint, ConflictResolutionRequest,
    DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS,
    DEFAULT_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS, Device, DeviceStatus, DeviceSyncCheckpoint,
    DomainError, FileVersion, Library, LibraryStatus, LogicalName, LogicalSnapshot,
    LogicalSnapshotError, LogicalSnapshotNode, LogicalSnapshotNodeError, LoginIdentifier,
    MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS, MAX_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS,
    MAX_BACKUP_SNAPSHOT_RETENTION_SECONDS, MAX_LOGICAL_NAME_BYTES,
    MIN_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS, MIN_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS, Node,
    NodeKind, NodeState, ObjectReference, PlannedScheduleOccurrence, RebaselineSnapshotPageCursor,
    SnapshotState, SnapshotStateParseError, SyncBootstrap, SyncBootstrapState,
    SyncBootstrapStateParseError, UploadOperation, UploadOperationParseError, UploadSessionState,
    UploadSessionStateParseError, UploadStateTransitionError, User, UserStatus,
    is_claimable_scheduled_maintenance_state,
};
pub use errors::{CoreError, ErrorCode, UnknownErrorCode};
pub use hashes::{Hash, HashParseError, Sha256Digest};
pub use ids::{
    BackupMaintenanceRunId, BackupPruneExecutionId, BackupPrunePlanId, BackupRestoreExecutionId,
    BackupRestorePlanId, BackupScheduleId, BackupScheduleMisfireSkipId, BackupScheduleOccurrenceId,
    BackupScheduleRevisionId, BackupScheduledMaintenanceClaimId,
    BackupScheduledMaintenanceLeaseToken, BackupScheduledMaintenanceWorkerId, BackupSetId,
    BackupSnapshotExpiryExecutionId, BackupSnapshotExpiryPlanId,
    BackupSnapshotRetentionPolicyRevisionId, ChangeEventId, ClientMutationId, ConflictResolutionId,
    DedupDomainId, DeviceCredentialId, DeviceEnrollmentGrantId, DeviceId, FileVersionId,
    IdParseError, LibraryId, NodeId, ObjectGcOperationId, ObjectId, ObjectReplicaId,
    OutboundIntentId, RebaselineSnapshotId, ShareId, SnapshotId, SyncBootstrapId, SyncConflictId,
    UploadSessionId, UserId,
};

/// Compatibility spelling for the immutable identity of one journal entry.
/// The accepted domain vocabulary calls the entry a `ChangeEvent`.
pub type ChangeJournalEntryId = ChangeEventId;

/// Compatibility spelling for the immutable journal fact contract.
pub type ChangeJournalEntry = ChangeEvent;
pub use numbers::{DecimalValueError, Revision, Sequence};
pub use time::{Timestamp, TimestampParseError};
pub use tokens::{ETag, Etag, OpaqueCursor, TokenError, VersionToken};

pub use domain::{
    last_occurrence_before, latest_occurrence_in_window, next_occurrence_after,
    occurrence_on_local_date, oldest_occurrence_in_window, scheduled_maintenance_resulting_state,
    validate_scheduled_maintenance_lease_duration,
};
