//! Durable backup domain and immutable snapshot manifest repository/service.
//!
//! The repository writes the owner-scoped `backup_sets` config, the immutable
//! `backup_snapshots` capture lifecycle, immutable `backup_snapshot_nodes`
//! manifest rows, and private `backup_snapshot_content_pins`. Snapshot capture
//! acquires the existing per-library namespace guard and copies the current
//! logical Node projection in one transaction, so a later rename, move,
//! Trash/purge, or content replacement cannot mix into the captured cut. Each
//! manifest content row receives a server-internal Object retention pin before
//! the snapshot can transition to `COMPLETED`; the public manifest never gains
//! physical identity.

use std::str::FromStr;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
};

use async_trait::async_trait;
use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION, BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION,
    BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION, BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION,
    BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION,
    BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION, BackupMaintenanceRun,
    BackupMaintenanceRunId, BackupMaintenanceRunIdempotencyFingerprint,
    BackupMaintenanceRunPreflightIssue, BackupMaintenanceRunRequest, BackupMaintenanceRunState,
    BackupManifestContent, BackupOperationKind, BackupPruneExecution, BackupPruneExecutionId,
    BackupPruneExecutionPreflightIssue, BackupPruneImpact, BackupPrunePlan, BackupPrunePlanEntry,
    BackupPrunePlanId, BackupPrunePlanIdempotencyFingerprint, BackupPrunePlanRequest,
    BackupPrunePlanState, BackupPrunePreflightIssue, BackupRestoreAction, BackupRestoreExecution,
    BackupRestoreExecutionEntry, BackupRestoreExecutionId, BackupRestorePlan,
    BackupRestorePlanEntry, BackupRestorePlanId, BackupRestorePlanIdempotencyFingerprint,
    BackupRestorePlanRequest, BackupRestorePlanState, BackupRestorePreflightIssue, BackupSet,
    BackupSetId, BackupSetState, BackupSnapshot, BackupSnapshotExpiryBasisEntry,
    BackupSnapshotExpiryBasisFingerprint, BackupSnapshotExpiryDecision,
    BackupSnapshotExpiryExecution, BackupSnapshotExpiryExecutionEntry,
    BackupSnapshotExpiryExecutionId, BackupSnapshotExpiryExecutionPreflightIssue,
    BackupSnapshotExpiryPlan, BackupSnapshotExpiryPlanEntry, BackupSnapshotExpiryPlanId,
    BackupSnapshotExpiryPlanIdempotencyFingerprint, BackupSnapshotExpiryPlanRequest,
    BackupSnapshotExpiryPlanState, BackupSnapshotExpiryPreflightIssue, BackupSnapshotNode,
    BackupSnapshotRetentionPolicyConfig, BackupSnapshotRetentionPolicyIdempotencyFingerprint,
    BackupSnapshotRetentionPolicyRequest, BackupSnapshotRetentionPolicyRevision,
    BackupSnapshotRetentionPolicyRevisionId, BackupSnapshotRetentionPolicyRevisionNumber,
    BackupSource, ChangeKind, DedupDomainId, FileVersion, FileVersionId, Library, LibraryId,
    LibraryStatus, LogicalName, Node, NodeId, NodeKind, NodeState, ObjectId, ObjectReference,
    Revision, Sequence, Sha256Digest, SnapshotId, SnapshotState, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    DatabaseError, DatabaseErrorKind, DatabasePool, LibraryRow, MappingError, MetadataError,
    NodeRow,
    journal::{JournalChange, acquire_namespace_guard, append_changes},
    repository::DomainRepository,
};

pub const DEFAULT_BACKUP_SNAPSHOT_PAGE_LIMIT: u32 = 200;
pub const MAX_BACKUP_SNAPSHOT_PAGE_LIMIT: u32 = 1_000;
pub const DEFAULT_BACKUP_SET_PAGE_LIMIT: u32 = 100;
pub const MAX_BACKUP_SET_PAGE_LIMIT: u32 = 500;
pub const DEFAULT_BACKUP_RESTORE_PLAN_ENTRY_PAGE_LIMIT: u32 = 200;
pub const MAX_BACKUP_RESTORE_PLAN_ENTRY_PAGE_LIMIT: u32 = 1_000;
pub const DEFAULT_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT: u32 = 200;
pub const MAX_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT: u32 = 1_000;
pub const DEFAULT_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT: u32 = 200;
pub const MAX_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT: u32 = 1_000;
pub const DEFAULT_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT: u32 = 200;
pub const MAX_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT: u32 = 1_000;
pub const DEFAULT_BACKUP_OPERATION_PAGE_LIMIT: u32 = 100;
pub const MAX_BACKUP_OPERATION_PAGE_LIMIT: u32 = 500;

const MIN_OPERATION_KEY_BYTES: usize = 8;
const MAX_OPERATION_KEY_BYTES: usize = 256;

/// Immutable keyset boundary for the owner/set snapshot listing. The
/// timestamp is the canonical chronological sort key: committed snapshots use
/// `committed_at`, while unfinished/failed observations use `created_at`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotPagePosition {
    sort_at: Timestamp,
    snapshot_id: SnapshotId,
}

impl BackupSnapshotPagePosition {
    #[must_use]
    pub const fn new(sort_at: Timestamp, snapshot_id: SnapshotId) -> Self {
        Self {
            sort_at,
            snapshot_id,
        }
    }

    #[must_use]
    pub const fn sort_at(self) -> Timestamp {
        self.sort_at
    }

    #[must_use]
    pub const fn snapshot_id(self) -> SnapshotId {
        self.snapshot_id
    }
}

/// Immutable keyset boundary for the owner/set maintenance-run listing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupMaintenanceRunPagePosition {
    created_at: Timestamp,
    run_id: BackupMaintenanceRunId,
}

/// Typed identity for one semantic item in the unified backup activity feed.
/// Restore/prune execution receipts intentionally cannot be represented here:
/// they are child evidence for their parent plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupOperationId {
    Maintenance(BackupMaintenanceRunId),
    Restore(BackupRestorePlanId),
    Prune(BackupPrunePlanId),
}

impl BackupOperationId {
    #[must_use]
    pub const fn kind(self) -> BackupOperationKind {
        match self {
            Self::Maintenance(_) => BackupOperationKind::Maintenance,
            Self::Restore(_) => BackupOperationKind::Restore,
            Self::Prune(_) => BackupOperationKind::Prune,
        }
    }

    #[must_use]
    pub const fn into_uuid(self) -> Uuid {
        match self {
            Self::Maintenance(value) => value.into_uuid(),
            Self::Restore(value) => value.into_uuid(),
            Self::Prune(value) => value.into_uuid(),
        }
    }
}

/// Opaque keyset boundary for the heterogeneous backup activity feed. The
/// transport includes the backup-set scope in its encoded cursor; this value
/// contains only the ordering tuple consumed by the metadata query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupOperationPagePosition {
    created_at: Timestamp,
    operation_id: BackupOperationId,
}

impl BackupOperationPagePosition {
    #[must_use]
    pub const fn new(created_at: Timestamp, operation_id: BackupOperationId) -> Self {
        Self {
            created_at,
            operation_id,
        }
    }

    #[must_use]
    pub const fn created_at(self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn operation_id(self) -> BackupOperationId {
        self.operation_id
    }
}

/// Canonical operation-specific state carried by the read projection. It is
/// intentionally not flattened to a generic RUNNING/DONE vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupOperationState {
    Maintenance(BackupMaintenanceRunState),
    Restore(BackupRestorePlanState),
    Prune(BackupPrunePlanState),
}

impl BackupOperationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Maintenance(state) => state.as_str(),
            Self::Restore(state) => state.as_str(),
            Self::Prune(state) => state.as_str(),
        }
    }
}

/// Compact, safe activity-feed projection. The step count is derived from
/// durable child references and receipt state; it is not a live percentage or
/// process-progress estimate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupOperationSummary {
    operation_kind: BackupOperationKind,
    operation_id: BackupOperationId,
    backup_set_id: BackupSetId,
    snapshot_id: Option<SnapshotId>,
    state: BackupOperationState,
    completed_steps: u32,
    total_steps: u32,
    created_at: Timestamp,
    last_transition_at: Timestamp,
    completed_at: Option<Timestamp>,
}

impl BackupOperationSummary {
    #[must_use]
    pub const fn operation_kind(&self) -> BackupOperationKind {
        self.operation_kind
    }

    #[must_use]
    pub const fn operation_id(&self) -> BackupOperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> Option<SnapshotId> {
        self.snapshot_id
    }

    #[must_use]
    pub const fn state(&self) -> BackupOperationState {
        self.state
    }

    #[must_use]
    pub const fn completed_steps(&self) -> u32 {
        self.completed_steps
    }

    #[must_use]
    pub const fn total_steps(&self) -> u32 {
        self.total_steps
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn last_transition_at(&self) -> Timestamp {
        self.last_transition_at
    }

    #[must_use]
    pub const fn completed_at(&self) -> Option<Timestamp> {
        self.completed_at
    }
}

/// Rich, kind-specific durable operation projection used by the detail
/// endpoint. All fields are logical metadata; no Object/replica identity is
/// reachable from this boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupOperationDetail {
    Maintenance(BackupMaintenanceRun),
    Restore {
        plan: BackupRestorePlan,
        execution: Option<BackupRestoreExecution>,
    },
    Prune {
        plan: BackupPrunePlan,
        execution: Option<BackupPruneExecution>,
    },
}

impl BackupOperationDetail {
    pub fn summary(&self) -> Result<BackupOperationSummary, BackupError> {
        match self {
            Self::Maintenance(run) => maintenance_operation_summary(run),
            Self::Restore { plan, execution } => {
                restore_operation_summary(plan, execution.as_ref())
            }
            Self::Prune { plan, execution } => prune_operation_summary(plan, execution.as_ref()),
        }
    }
}

fn maintenance_operation_summary(
    run: &BackupMaintenanceRun,
) -> Result<BackupOperationSummary, BackupError> {
    let (completed_steps, last_transition_at) = match run.state() {
        BackupMaintenanceRunState::Created => (0, run.created_at()),
        BackupMaintenanceRunState::SnapshotCaptured => (
            1,
            run.snapshot_captured_at()
                .ok_or(BackupError::InvalidPersistedData)?,
        ),
        BackupMaintenanceRunState::ExpiryPlanned => (
            2,
            run.expiry_planned_at()
                .ok_or(BackupError::InvalidPersistedData)?,
        ),
        BackupMaintenanceRunState::Completed => (
            3,
            run.maintenance_completed_at()
                .ok_or(BackupError::InvalidPersistedData)?,
        ),
        BackupMaintenanceRunState::Stale => {
            // The durable stale shape deliberately preserves references to
            // work already committed before the run became unusable. Reject
            // partial references rather than resetting visible progress.
            let captured = match (run.captured_snapshot_id(), run.snapshot_captured_at()) {
                (None, None) => false,
                (Some(_), Some(_)) => true,
                _ => return Err(BackupError::InvalidPersistedData),
            };
            let planned = match (run.expiry_plan_id(), run.expiry_planned_at()) {
                (None, None) => false,
                (Some(_), Some(_)) if captured => true,
                _ => return Err(BackupError::InvalidPersistedData),
            };
            if run.expiry_execution_id().is_some() || run.maintenance_completed_at().is_some() {
                return Err(BackupError::InvalidPersistedData);
            }
            (
                if planned {
                    2
                } else if captured {
                    1
                } else {
                    0
                },
                run.stale_at().ok_or(BackupError::InvalidPersistedData)?,
            )
        }
    };

    Ok(BackupOperationSummary {
        operation_kind: BackupOperationKind::Maintenance,
        operation_id: BackupOperationId::Maintenance(run.id()),
        backup_set_id: run.backup_set_id(),
        snapshot_id: run.captured_snapshot_id(),
        state: BackupOperationState::Maintenance(run.state()),
        completed_steps,
        total_steps: 3,
        created_at: run.created_at(),
        last_transition_at,
        completed_at: run.maintenance_completed_at(),
    })
}

fn restore_operation_summary(
    plan: &BackupRestorePlan,
    execution: Option<&BackupRestoreExecution>,
) -> Result<BackupOperationSummary, BackupError> {
    validate_restore_operation_shape(plan, execution)?;

    let (completed_steps, last_transition_at, completed_at) = match plan.state() {
        BackupRestorePlanState::Planned => (1, plan.created_at(), None),
        BackupRestorePlanState::Stale => (
            1,
            plan.stale_at().ok_or(BackupError::InvalidPersistedData)?,
            None,
        ),
        BackupRestorePlanState::Executed => {
            let executed_at = execution
                .map(BackupRestoreExecution::executed_at)
                .ok_or(BackupError::InvalidPersistedData)?;
            (2, executed_at, Some(executed_at))
        }
    };

    Ok(BackupOperationSummary {
        operation_kind: BackupOperationKind::Restore,
        operation_id: BackupOperationId::Restore(plan.id()),
        backup_set_id: plan.backup_set_id(),
        snapshot_id: Some(plan.snapshot_id()),
        state: BackupOperationState::Restore(plan.state()),
        completed_steps,
        total_steps: 2,
        created_at: plan.created_at(),
        last_transition_at,
        completed_at,
    })
}

fn prune_operation_summary(
    plan: &BackupPrunePlan,
    execution: Option<&BackupPruneExecution>,
) -> Result<BackupOperationSummary, BackupError> {
    validate_prune_operation_shape(plan, execution)?;

    let (completed_steps, last_transition_at, completed_at) = match plan.state() {
        BackupPrunePlanState::Planned => (1, plan.created_at(), None),
        BackupPrunePlanState::Stale => (
            1,
            plan.stale_at().ok_or(BackupError::InvalidPersistedData)?,
            None,
        ),
        BackupPrunePlanState::Executed => {
            let executed_at = execution
                .map(BackupPruneExecution::executed_at)
                .ok_or(BackupError::InvalidPersistedData)?;
            (2, executed_at, Some(executed_at))
        }
    };

    Ok(BackupOperationSummary {
        operation_kind: BackupOperationKind::Prune,
        operation_id: BackupOperationId::Prune(plan.id()),
        backup_set_id: plan.backup_set_id(),
        snapshot_id: Some(plan.snapshot_id()),
        state: BackupOperationState::Prune(plan.state()),
        completed_steps,
        total_steps: 2,
        created_at: plan.created_at(),
        last_transition_at,
        completed_at,
    })
}

impl BackupMaintenanceRunPagePosition {
    #[must_use]
    pub const fn new(created_at: Timestamp, run_id: BackupMaintenanceRunId) -> Self {
        Self { created_at, run_id }
    }

    #[must_use]
    pub const fn created_at(self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn run_id(self) -> BackupMaintenanceRunId {
        self.run_id
    }
}

/// Read-only metadata port used by the HTTP backup inspection surface. The
/// trait intentionally contains no capture, retention mutation, restore/prune
/// execution, expiry, maintenance-advance, pin, or object-store operation.
#[async_trait]
pub trait BackupReadBackend: Send + Sync {
    async fn list_backup_sets(
        &self,
        owner_user_id: UserId,
        after: Option<BackupSetId>,
        limit: u32,
    ) -> Result<(Vec<BackupSet>, bool), BackupError>;

    async fn get_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSet, BackupError>;

    async fn list_backup_snapshots(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        state: Option<SnapshotState>,
        after: Option<BackupSnapshotPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshot>, bool), BackupError>;

    async fn get_backup_snapshot(
        &self,
        owner_user_id: UserId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupSnapshot, BackupError>;

    async fn list_backup_snapshot_nodes(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        parent_node_id: Option<NodeId>,
        after: Option<NodeId>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotNode>, bool), BackupError>;

    async fn get_current_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError>;

    async fn list_backup_maintenance_runs(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        after: Option<BackupMaintenanceRunPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupMaintenanceRun>, bool), BackupError>;

    async fn get_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError>;

    /// Read one owner-scoped restore plan without revalidating or mutating its state.
    async fn get_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestorePlan, BackupError>;

    /// Read one owner-scoped committed restore-execution receipt.
    async fn get_restore_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupRestoreExecutionId,
    ) -> Result<BackupRestoreExecution, BackupError>;

    /// Read one owner-scoped prune plan without revalidating or mutating it.
    async fn get_prune_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
    ) -> Result<BackupPrunePlan, BackupError>;

    /// Read one owner-scoped committed prune-execution receipt.
    async fn get_prune_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupPruneExecutionId,
    ) -> Result<BackupPruneExecution, BackupError>;

    /// List one owner- and backup-set-scoped heterogeneous activity page.
    async fn list_backup_operations(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        kind: Option<BackupOperationKind>,
        after: Option<BackupOperationPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupOperationSummary>, bool), BackupError>;

    /// Read one semantic operation by its kind-qualified identity. A UUID
    /// present in another operation table is intentionally not a match.
    async fn get_backup_operation(
        &self,
        owner_user_id: UserId,
        kind: BackupOperationKind,
        operation_id: BackupOperationId,
    ) -> Result<BackupOperationDetail, BackupError>;
}

/// Narrow application port for the public manual backup mutation workflow.
/// Implementations must preserve the canonical service's ownership,
/// idempotency, retention, and maintenance-orchestration invariants.
#[async_trait]
pub trait BackupMutationBackend: Send + Sync {
    async fn create_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        name: LogicalName,
        source_library_id: LibraryId,
        observed_at: Timestamp,
    ) -> Result<BackupSet, BackupError>;

    async fn configure_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        keep_latest_completed: u64,
        expire_after_seconds: u64,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError>;

    async fn create_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
    ) -> Result<BackupMaintenanceRun, BackupError>;

    async fn advance_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError>;

    /// Create a durable, non-destructive restore plan.
    #[allow(clippy::too_many_arguments)]
    async fn create_restore_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        target_library_id: LibraryId,
        target_parent_node_id: NodeId,
        destination_name: LogicalName,
    ) -> Result<BackupRestorePlan, BackupError>;

    /// Execute one persisted PLANNED restore plan as one authoritative metadata transaction.
    async fn execute_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestoreExecution, BackupError>;

    /// Create a durable, non-destructive prune plan for one EXPIRED snapshot.
    async fn create_prune_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupPrunePlan, BackupError>;

    /// Execute exactly one persisted prune plan through the canonical atomic
    /// retention-release service. This never performs physical ObjectStore I/O.
    async fn execute_prune_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
    ) -> Result<BackupPruneExecution, BackupError>;
}

/// Stable failures for the backup domain application service. Inaccessible
/// owner/set/library scopes intentionally collapse to `NotFound` so IDs cannot
/// enumerate resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupError {
    NotFound,
    InvalidRequest,
    InvalidLimit,
    BackupSetConflict,
    BackupSetOperationConflict,
    BackupSetDisabled,
    SnapshotConflict,
    SnapshotAlreadyBuilding,
    InvalidState,
    OwnerScopeMismatch,
    CrossOwnerBackupSet,
    RestorePlanConflict,
    RestorePreflight(BackupRestorePreflightIssue),
    PrunePlanConflict,
    PruneAlreadyPlanned,
    PrunePreflight(BackupPrunePreflightIssue),
    PruneExecutionPreflight(BackupPruneExecutionPreflightIssue),
    SnapshotAlreadyPruned,
    RetentionPolicyConflict,
    ExpiryPlanConflict,
    ExpiryPreflight(BackupSnapshotExpiryPreflightIssue),
    ExpiryExecutionPreflight(BackupSnapshotExpiryExecutionPreflightIssue),
    MaintenanceRunPreflight(BackupMaintenanceRunPreflightIssue),
    DependencyUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
    InternalError,
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "backup resource was not found",
            Self::InvalidRequest => "backup request is invalid",
            Self::InvalidLimit => "backup page limit is invalid",
            Self::BackupSetConflict => "backup set already exists",
            Self::BackupSetOperationConflict => {
                "backup set operation identity conflicts with the request"
            }
            Self::BackupSetDisabled => "backup set is disabled",
            Self::SnapshotConflict => "backup snapshot conflicts with newer progress",
            Self::SnapshotAlreadyBuilding => "a backup snapshot is already in progress",
            Self::InvalidState => "backup resource is in an invalid state",
            Self::OwnerScopeMismatch => "backup resource owner scope does not match",
            Self::CrossOwnerBackupSet => "backup set does not belong to the owner",
            Self::RestorePlanConflict => {
                "restore plan operation identity conflicts with the request"
            }
            Self::RestorePreflight(issue) => return issue.fmt(formatter),
            Self::PrunePlanConflict => "prune plan operation identity conflicts with the request",
            Self::PruneAlreadyPlanned => "backup snapshot already has an active prune plan",
            Self::PrunePreflight(issue) => return issue.fmt(formatter),
            Self::PruneExecutionPreflight(issue) => return issue.fmt(formatter),
            Self::SnapshotAlreadyPruned => "snapshot retention has already been released",
            Self::RetentionPolicyConflict => {
                "retention policy operation identity conflicts with the request"
            }
            Self::ExpiryPlanConflict => {
                "snapshot expiry plan operation identity conflicts with the request"
            }
            Self::ExpiryPreflight(issue) => return issue.fmt(formatter),
            Self::ExpiryExecutionPreflight(issue) => return issue.fmt(formatter),
            Self::MaintenanceRunPreflight(issue) => return issue.fmt(formatter),
            Self::DependencyUnavailable => "backup dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "backup persisted data is invalid",
            Self::InternalError => "backup internal error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for BackupError {}

/// PostgreSQL row for one owner-scoped backup set.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSetRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub name: String,
    pub source_library_id: Uuid,
    pub source: String,
    pub retention_days: Option<i32>,
    pub state: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revision: String,
}

impl BackupSetRow {
    pub fn from_domain(value: &BackupSet) -> Result<Self, MappingError> {
        Ok(Self {
            id: value.id().into_uuid(),
            owner_user_id: value.owner_user_id().into_uuid(),
            name: value.name().as_str().to_owned(),
            source_library_id: value.source_library_id().into_uuid(),
            source: value.source().as_str().to_owned(),
            retention_days: value
                .retention_days()
                .map(i32::try_from)
                .transpose()
                .map_err(|_| MappingError::InvalidDecimal {
                    field: "backup_sets.retention_days",
                })?,
            state: value.state().as_str().to_owned(),
            created_at: encode_timestamp(value.created_at(), "backup_sets.created_at")?,
            updated_at: encode_timestamp(value.updated_at(), "backup_sets.updated_at")?,
            revision: value.revision().get().to_string(),
        })
    }

    pub fn try_into_domain(self) -> Result<BackupSet, MappingError> {
        let id = decode_id(self.id, "backup_sets.id")?;
        let owner_user_id = decode_id(self.owner_user_id, "backup_sets.owner_user_id")?;
        let source_library_id = decode_id(self.source_library_id, "backup_sets.source_library_id")?;
        let name = LogicalName::new(self.name).map_err(|_| MappingError::InvalidName {
            field: "backup_sets.name",
        })?;
        let _ = BackupSource::from_str(&self.source).map_err(|_| MappingError::InvalidEnum {
            field: "backup_sets.source",
        })?;
        let state =
            BackupSetState::from_str(&self.state).map_err(|_| MappingError::InvalidEnum {
                field: "backup_sets.state",
            })?;
        let retention_days = self
            .retention_days
            .map(|value| {
                u32::try_from(value).map_err(|_| MappingError::InvalidDecimal {
                    field: "backup_sets.retention_days",
                })
            })
            .transpose()?;
        let revision = revision_from_str(&self.revision, "backup_sets.revision")?;
        Ok(BackupSet::rehydrate(
            id,
            owner_user_id,
            name,
            source_library_id,
            retention_days,
            state,
            Timestamp::from_offset_datetime(self.created_at),
            Timestamp::from_offset_datetime(self.updated_at),
            revision,
        )?)
    }
}

/// PostgreSQL row for one immutable backup snapshot.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotRow {
    pub id: Uuid,
    pub backup_set_id: Uuid,
    pub owner_user_id: Uuid,
    pub source_library_id: Uuid,
    pub operation_id: String,
    pub snapshot_epoch: i64,
    pub snapshot_resume_sequence: i64,
    pub manifest_item_count: i64,
    pub terminal_node_id: Option<Uuid>,
    pub content_reference_count: i64,
    pub state: String,
    pub created_at: OffsetDateTime,
    pub committed_at: Option<OffsetDateTime>,
    pub expired_at: Option<OffsetDateTime>,
}

impl BackupSnapshotRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshot, MappingError> {
        let id = decode_id(self.id, "backup_snapshots.id")?;
        let backup_set_id = decode_id(self.backup_set_id, "backup_snapshots.backup_set_id")?;
        let owner_user_id = decode_id(self.owner_user_id, "backup_snapshots.owner_user_id")?;
        let source_library_id =
            decode_id(self.source_library_id, "backup_snapshots.source_library_id")?;
        let snapshot_epoch =
            decode_sequence(self.snapshot_epoch, "backup_snapshots.snapshot_epoch")?;
        let snapshot_resume_sequence = decode_sequence(
            self.snapshot_resume_sequence,
            "backup_snapshots.snapshot_resume_sequence",
        )?;
        let manifest_item_count =
            u64::try_from(self.manifest_item_count).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_snapshots.manifest_item_count",
            })?;
        let terminal_node_id = self
            .terminal_node_id
            .map(|value| decode_id(value, "backup_snapshots.terminal_node_id"))
            .transpose()?;
        let content_reference_count =
            u64::try_from(self.content_reference_count).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_snapshots.content_reference_count",
                }
            })?;
        let state =
            SnapshotState::from_str(&self.state).map_err(|_| MappingError::InvalidEnum {
                field: "backup_snapshots.state",
            })?;
        Ok(BackupSnapshot::new(
            id,
            backup_set_id,
            owner_user_id,
            source_library_id,
            snapshot_epoch,
            snapshot_resume_sequence,
            manifest_item_count,
            terminal_node_id,
            content_reference_count,
            state,
            Timestamp::from_offset_datetime(self.created_at),
            self.committed_at.map(Timestamp::from_offset_datetime),
            self.expired_at.map(Timestamp::from_offset_datetime),
        ))
    }
}

/// PostgreSQL row for one immutable manifest node.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotNodeRow {
    pub snapshot_id: Uuid,
    pub node_id: Uuid,
    pub parent_node_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub state: String,
    pub revision: String,
    pub current_version_id: Option<Uuid>,
    pub content_length: Option<String>,
    pub content_sha256: Option<Vec<u8>>,
    pub node_created_at: OffsetDateTime,
    pub node_updated_at: OffsetDateTime,
}

impl BackupSnapshotNodeRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshotNode, MappingError> {
        let node_id = decode_id(self.node_id, "backup_snapshot_nodes.node_id")?;
        let parent_node_id = self
            .parent_node_id
            .map(|value| decode_id(value, "backup_snapshot_nodes.parent_node_id"))
            .transpose()?;
        let name = LogicalName::new(self.name).map_err(|_| MappingError::InvalidName {
            field: "backup_snapshot_nodes.name",
        })?;
        let kind = decode_node_kind(&self.kind)?;
        let state = decode_public_node_state(&self.state)?;
        let revision = revision_from_str(&self.revision, "backup_snapshot_nodes.revision")?;
        let content = match (
            self.current_version_id,
            self.content_length,
            self.content_sha256,
        ) {
            (Some(current_version_id), Some(content_length), Some(content_sha256)) => {
                let current_version_id = decode_id(
                    current_version_id,
                    "backup_snapshot_nodes.current_version_id",
                )?;
                let byte_length =
                    u64::from_str(&content_length).map_err(|_| MappingError::InvalidDecimal {
                        field: "backup_snapshot_nodes.content_length",
                    })?;
                let sha256 = Sha256Digest::try_from(content_sha256.as_slice()).map_err(|_| {
                    MappingError::InvalidDigest {
                        field: "backup_snapshot_nodes.content_sha256",
                    }
                })?;
                Some(BackupManifestContent::new(
                    current_version_id,
                    byte_length,
                    sha256,
                ))
            }
            (None, None, None) => None,
            _ => {
                return Err(MappingError::RelationMismatch {
                    relation: "backup_snapshot_nodes.content",
                });
            }
        };
        BackupSnapshotNode::new(
            node_id,
            parent_node_id,
            name,
            kind,
            state,
            revision,
            content,
            Timestamp::from_offset_datetime(self.node_created_at),
            Timestamp::from_offset_datetime(self.node_updated_at),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one append-only snapshot-retention policy revision.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotRetentionPolicyRevisionRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub backup_set_id: Uuid,
    pub revision_number: i64,
    pub operation_id: String,
    pub fingerprint_version: i16,
    pub request_fingerprint: Vec<u8>,
    pub keep_latest_completed: i64,
    pub expire_after_seconds: i64,
    pub created_at: OffsetDateTime,
}

impl BackupSnapshotRetentionPolicyRevisionRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshotRetentionPolicyRevision, MappingError> {
        let id = decode_id(self.id, "backup_snapshot_retention_policy_revisions.id")?;
        let owner_user_id = decode_id(
            self.owner_user_id,
            "backup_snapshot_retention_policy_revisions.owner_user_id",
        )?;
        let backup_set_id = decode_id(
            self.backup_set_id,
            "backup_snapshot_retention_policy_revisions.backup_set_id",
        )?;
        let revision_number = BackupSnapshotRetentionPolicyRevisionNumber::new(
            u64::try_from(self.revision_number).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_snapshot_retention_policy_revisions.revision_number",
            })?,
        )
        .map_err(MappingError::from)?;
        let config = BackupSnapshotRetentionPolicyConfig::new(
            u64::try_from(self.keep_latest_completed).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_snapshot_retention_policy_revisions.keep_latest_completed",
                }
            })?,
            u64::try_from(self.expire_after_seconds).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_snapshot_retention_policy_revisions.expire_after_seconds",
            })?,
        )
        .map_err(MappingError::from)?;
        let fingerprint_version =
            u16::try_from(self.fingerprint_version).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_snapshot_retention_policy_revisions.fingerprint_version",
            })?;
        let fingerprint_bytes =
            <[u8; 32]>::try_from(self.request_fingerprint.as_slice()).map_err(|_| {
                MappingError::InvalidDigest {
                    field: "backup_snapshot_retention_policy_revisions.request_fingerprint",
                }
            })?;
        let fingerprint = BackupSnapshotRetentionPolicyIdempotencyFingerprint::new(
            fingerprint_version,
            fingerprint_bytes,
        );
        let request = BackupSnapshotRetentionPolicyRequest::new(backup_set_id, config);
        if fingerprint_version != BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION
            || request.fingerprint() != fingerprint
        {
            return Err(MappingError::RelationMismatch {
                relation: "backup_snapshot_retention_policy_revisions.request_fingerprint",
            });
        }
        BackupSnapshotRetentionPolicyRevision::new(
            id,
            owner_user_id,
            backup_set_id,
            revision_number,
            self.operation_id,
            fingerprint,
            config,
            Timestamp::from_offset_datetime(self.created_at),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for durable, public-safe expiry-plan provenance.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotExpiryPlanRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub backup_set_id: Uuid,
    pub policy_revision_id: Uuid,
    pub policy_revision_number: i64,
    pub operation_id: String,
    pub fingerprint_version: i16,
    pub request_fingerprint: Vec<u8>,
    pub evaluated_at: OffsetDateTime,
    pub cutoff_at: OffsetDateTime,
    pub snapshot_basis_fingerprint_version: i16,
    pub snapshot_basis_fingerprint: Vec<u8>,
    pub evaluated_completed_snapshot_count: i64,
    pub expire_candidate_count: i64,
    pub keep_latest_count: i64,
    pub keep_recent_count: i64,
    pub blocked_active_restore_count: i64,
    pub state: String,
    pub created_at: OffsetDateTime,
    pub stale_at: Option<OffsetDateTime>,
}

impl BackupSnapshotExpiryPlanRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshotExpiryPlan, MappingError> {
        let backup_set_id = decode_id(
            self.backup_set_id,
            "backup_snapshot_expiry_plans.backup_set_id",
        )?;
        let request = BackupSnapshotExpiryPlanRequest::new(backup_set_id);
        let fingerprint_version =
            u16::try_from(self.fingerprint_version).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_snapshot_expiry_plans.fingerprint_version",
            })?;
        let fingerprint_bytes =
            <[u8; 32]>::try_from(self.request_fingerprint.as_slice()).map_err(|_| {
                MappingError::InvalidDigest {
                    field: "backup_snapshot_expiry_plans.request_fingerprint",
                }
            })?;
        let request_fingerprint = BackupSnapshotExpiryPlanIdempotencyFingerprint::new(
            fingerprint_version,
            fingerprint_bytes,
        );
        if fingerprint_version != BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION
            || request.fingerprint() != request_fingerprint
        {
            return Err(MappingError::RelationMismatch {
                relation: "backup_snapshot_expiry_plans.request_fingerprint",
            });
        }
        let basis_version =
            u16::try_from(self.snapshot_basis_fingerprint_version).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_snapshot_expiry_plans.snapshot_basis_fingerprint_version",
                }
            })?;
        let basis_bytes = <[u8; 32]>::try_from(self.snapshot_basis_fingerprint.as_slice())
            .map_err(|_| MappingError::InvalidDigest {
                field: "backup_snapshot_expiry_plans.snapshot_basis_fingerprint",
            })?;
        if basis_version != BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION {
            return Err(MappingError::RelationMismatch {
                relation: "backup_snapshot_expiry_plans.snapshot_basis_fingerprint_version",
            });
        }
        let state = BackupSnapshotExpiryPlanState::from_str(&self.state).map_err(|_| {
            MappingError::InvalidEnum {
                field: "backup_snapshot_expiry_plans.state",
            }
        })?;
        BackupSnapshotExpiryPlan::new(
            decode_id(self.id, "backup_snapshot_expiry_plans.id")?,
            decode_id(
                self.owner_user_id,
                "backup_snapshot_expiry_plans.owner_user_id",
            )?,
            self.operation_id,
            request_fingerprint,
            backup_set_id,
            decode_id(
                self.policy_revision_id,
                "backup_snapshot_expiry_plans.policy_revision_id",
            )?,
            BackupSnapshotRetentionPolicyRevisionNumber::new(
                u64::try_from(self.policy_revision_number).map_err(|_| {
                    MappingError::InvalidDecimal {
                        field: "backup_snapshot_expiry_plans.policy_revision_number",
                    }
                })?,
            )
            .map_err(MappingError::from)?,
            Timestamp::from_offset_datetime(self.evaluated_at),
            Timestamp::from_offset_datetime(self.cutoff_at),
            BackupSnapshotExpiryBasisFingerprint::new(basis_version, basis_bytes),
            decode_nonnegative_count(
                self.evaluated_completed_snapshot_count,
                "backup_snapshot_expiry_plans.evaluated_completed_snapshot_count",
            )?,
            decode_nonnegative_count(
                self.expire_candidate_count,
                "backup_snapshot_expiry_plans.expire_candidate_count",
            )?,
            decode_nonnegative_count(
                self.keep_latest_count,
                "backup_snapshot_expiry_plans.keep_latest_count",
            )?,
            decode_nonnegative_count(
                self.keep_recent_count,
                "backup_snapshot_expiry_plans.keep_recent_count",
            )?,
            decode_nonnegative_count(
                self.blocked_active_restore_count,
                "backup_snapshot_expiry_plans.blocked_active_restore_count",
            )?,
            state,
            Timestamp::from_offset_datetime(self.created_at),
            self.stale_at.map(Timestamp::from_offset_datetime),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one immutable COMPLETED-snapshot decision.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotExpiryPlanEntryRow {
    pub plan_id: Uuid,
    pub snapshot_id: Uuid,
    pub committed_at: OffsetDateTime,
    pub recency_rank: i64,
    pub decision: String,
}

impl BackupSnapshotExpiryPlanEntryRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshotExpiryPlanEntry, MappingError> {
        BackupSnapshotExpiryPlanEntry::new(
            decode_id(self.plan_id, "backup_snapshot_expiry_plan_entries.plan_id")?,
            decode_id(
                self.snapshot_id,
                "backup_snapshot_expiry_plan_entries.snapshot_id",
            )?,
            Timestamp::from_offset_datetime(self.committed_at),
            u64::try_from(self.recency_rank).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_snapshot_expiry_plan_entries.recency_rank",
            })?,
            BackupSnapshotExpiryDecision::from_str(&self.decision).map_err(|_| {
                MappingError::InvalidEnum {
                    field: "backup_snapshot_expiry_plan_entries.decision",
                }
            })?,
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one durable, public-safe expiry execution receipt.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotExpiryExecutionRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub expiry_plan_id: Uuid,
    pub backup_set_id: Uuid,
    pub policy_revision_id: Uuid,
    pub evaluated_at: OffsetDateTime,
    pub evaluated_snapshot_count: i64,
    pub expired_snapshot_count: i64,
    pub unchanged_snapshot_count: i64,
    pub state: String,
    pub executed_at: OffsetDateTime,
}

impl BackupSnapshotExpiryExecutionRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshotExpiryExecution, MappingError> {
        if self.state != "COMMITTED" {
            return Err(MappingError::InvalidEnum {
                field: "backup_snapshot_expiry_executions.state",
            });
        }
        BackupSnapshotExpiryExecution::new(
            decode_id(self.id, "backup_snapshot_expiry_executions.id")?,
            decode_id(
                self.owner_user_id,
                "backup_snapshot_expiry_executions.owner_user_id",
            )?,
            decode_id(
                self.expiry_plan_id,
                "backup_snapshot_expiry_executions.expiry_plan_id",
            )?,
            decode_id(
                self.backup_set_id,
                "backup_snapshot_expiry_executions.backup_set_id",
            )?,
            decode_id(
                self.policy_revision_id,
                "backup_snapshot_expiry_executions.policy_revision_id",
            )?,
            Timestamp::from_offset_datetime(self.evaluated_at),
            decode_nonnegative_count(
                self.evaluated_snapshot_count,
                "backup_snapshot_expiry_executions.evaluated_snapshot_count",
            )?,
            decode_nonnegative_count(
                self.expired_snapshot_count,
                "backup_snapshot_expiry_executions.expired_snapshot_count",
            )?,
            decode_nonnegative_count(
                self.unchanged_snapshot_count,
                "backup_snapshot_expiry_executions.unchanged_snapshot_count",
            )?,
            Timestamp::from_offset_datetime(self.executed_at),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one immutable expiry-execution entry.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupSnapshotExpiryExecutionEntryRow {
    pub execution_id: Uuid,
    pub expiry_plan_id: Uuid,
    pub snapshot_id: Uuid,
    pub original_decision: String,
    pub transitioned: bool,
}

impl BackupSnapshotExpiryExecutionEntryRow {
    pub fn try_into_domain(self) -> Result<BackupSnapshotExpiryExecutionEntry, MappingError> {
        Ok(BackupSnapshotExpiryExecutionEntry::new(
            decode_id(
                self.execution_id,
                "backup_snapshot_expiry_execution_entries.execution_id",
            )?,
            decode_id(
                self.expiry_plan_id,
                "backup_snapshot_expiry_execution_entries.expiry_plan_id",
            )?,
            decode_id(
                self.snapshot_id,
                "backup_snapshot_expiry_execution_entries.snapshot_id",
            )?,
            BackupSnapshotExpiryDecision::from_str(&self.original_decision).map_err(|_| {
                MappingError::InvalidEnum {
                    field: "backup_snapshot_expiry_execution_entries.original_decision",
                }
            })?,
            self.transitioned,
        ))
    }
}

/// PostgreSQL row for one durable prune-plan provenance record. The row and
/// its domain mapping expose aggregate, logical-safe evidence only; distinct
/// canonical Object accounting stays private below this service boundary.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupPrunePlanRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub backup_set_id: Uuid,
    pub snapshot_id: Uuid,
    pub operation_id: String,
    pub fingerprint_version: i16,
    pub request_fingerprint: Vec<u8>,
    pub snapshot_manifest_item_count: i64,
    pub snapshot_content_reference_count: i64,
    pub planned_pin_release_count: i64,
    pub distinct_retained_content_count: i64,
    pub retained_after_release_count: i64,
    pub would_become_unreferenced_count: i64,
    pub state: String,
    pub created_at: OffsetDateTime,
    pub stale_at: Option<OffsetDateTime>,
}

impl BackupPrunePlanRow {
    pub fn try_into_domain(self) -> Result<BackupPrunePlan, MappingError> {
        let id = decode_id(self.id, "backup_prune_plans.id")?;
        let owner_user_id = decode_id(self.owner_user_id, "backup_prune_plans.owner_user_id")?;
        let backup_set_id = decode_id(self.backup_set_id, "backup_prune_plans.backup_set_id")?;
        let snapshot_id = decode_id(self.snapshot_id, "backup_prune_plans.snapshot_id")?;
        let fingerprint_version =
            u16::try_from(self.fingerprint_version).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_plans.fingerprint_version",
            })?;
        let fingerprint_bytes =
            <[u8; 32]>::try_from(self.request_fingerprint.as_slice()).map_err(|_| {
                MappingError::InvalidDigest {
                    field: "backup_prune_plans.request_fingerprint",
                }
            })?;
        let request = BackupPrunePlanRequest::new(backup_set_id, snapshot_id);
        let fingerprint =
            BackupPrunePlanIdempotencyFingerprint::new(fingerprint_version, fingerprint_bytes);
        if fingerprint_version != BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION
            || request.fingerprint() != fingerprint
        {
            return Err(MappingError::RelationMismatch {
                relation: "backup_prune_plans.request_fingerprint",
            });
        }
        let snapshot_manifest_item_count = u64::try_from(self.snapshot_manifest_item_count)
            .map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_plans.snapshot_manifest_item_count",
            })?;
        let snapshot_content_reference_count = u64::try_from(self.snapshot_content_reference_count)
            .map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_plans.snapshot_content_reference_count",
            })?;
        let planned_pin_release_count =
            u64::try_from(self.planned_pin_release_count).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_prune_plans.planned_pin_release_count",
                }
            })?;
        let distinct_retained_content_count = u64::try_from(self.distinct_retained_content_count)
            .map_err(|_| MappingError::InvalidDecimal {
            field: "backup_prune_plans.distinct_retained_content_count",
        })?;
        let retained_after_release_count = u64::try_from(self.retained_after_release_count)
            .map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_plans.retained_after_release_count",
            })?;
        let would_become_unreferenced_count = u64::try_from(self.would_become_unreferenced_count)
            .map_err(|_| MappingError::InvalidDecimal {
            field: "backup_prune_plans.would_become_unreferenced_count",
        })?;
        let state =
            BackupPrunePlanState::from_str(&self.state).map_err(|_| MappingError::InvalidEnum {
                field: "backup_prune_plans.state",
            })?;
        BackupPrunePlan::new(
            id,
            owner_user_id,
            self.operation_id,
            fingerprint,
            request,
            snapshot_manifest_item_count,
            snapshot_content_reference_count,
            planned_pin_release_count,
            distinct_retained_content_count,
            retained_after_release_count,
            would_become_unreferenced_count,
            state,
            Timestamp::from_offset_datetime(self.created_at),
            self.stale_at.map(Timestamp::from_offset_datetime),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one durable prune execution receipt.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupPruneExecutionRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub prune_plan_id: Uuid,
    pub backup_set_id: Uuid,
    pub snapshot_id: Uuid,
    pub released_pin_count: i64,
    pub distinct_object_count: i64,
    pub retained_by_other_reference_count: i64,
    pub gc_handoff_object_count: i64,
    pub state: String,
    pub executed_at: OffsetDateTime,
}

impl BackupPruneExecutionRow {
    pub fn try_into_domain(self) -> Result<BackupPruneExecution, MappingError> {
        if self.state != "COMMITTED" {
            return Err(MappingError::InvalidEnum {
                field: "backup_prune_executions.state",
            });
        }
        BackupPruneExecution::new(
            decode_id(self.id, "backup_prune_executions.id")?,
            decode_id(self.owner_user_id, "backup_prune_executions.owner_user_id")?,
            decode_id(self.prune_plan_id, "backup_prune_executions.prune_plan_id")?,
            decode_id(self.backup_set_id, "backup_prune_executions.backup_set_id")?,
            decode_id(self.snapshot_id, "backup_prune_executions.snapshot_id")?,
            u64::try_from(self.released_pin_count).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_executions.released_pin_count",
            })?,
            u64::try_from(self.distinct_object_count).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_prune_executions.distinct_object_count",
                }
            })?,
            u64::try_from(self.retained_by_other_reference_count).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_prune_executions.retained_by_other_reference_count",
                }
            })?,
            u64::try_from(self.gc_handoff_object_count).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_prune_executions.gc_handoff_object_count",
                }
            })?,
            Timestamp::from_offset_datetime(self.executed_at),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one server-internal per-Object execution result.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupPruneExecutionObjectResultRow {
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub object_id: Uuid,
    pub object_dedup_domain_id: Uuid,
    pub target_snapshot_pin_count: i64,
    pub post_release_live_file_version_count: i64,
    pub post_release_other_snapshot_pin_count: i64,
    pub post_release_authoritative_reference_count: i64,
    pub gc_candidate_created_or_reused: bool,
    pub gc_candidate_id: Option<Uuid>,
    pub gc_candidate_generation: Option<String>,
}

/// PostgreSQL row for one immutable, public-safe logical release entry.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupPrunePlanEntryRow {
    pub plan_id: Uuid,
    pub ordinal: i64,
    pub source_snapshot_node_id: Uuid,
    pub file_version_id: Uuid,
    pub content_length: String,
    pub content_sha256: Vec<u8>,
}

impl BackupPrunePlanEntryRow {
    pub fn try_into_domain(self) -> Result<BackupPrunePlanEntry, MappingError> {
        let byte_length =
            u64::from_str(&self.content_length).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_plan_entries.content_length",
            })?;
        let sha256 = Sha256Digest::try_from(self.content_sha256.as_slice()).map_err(|_| {
            MappingError::InvalidDigest {
                field: "backup_prune_plan_entries.content_sha256",
            }
        })?;
        Ok(BackupPrunePlanEntry::new(
            decode_id(self.plan_id, "backup_prune_plan_entries.plan_id")?,
            u64::try_from(self.ordinal).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_prune_plan_entries.ordinal",
            })?,
            decode_id(
                self.source_snapshot_node_id,
                "backup_prune_plan_entries.source_snapshot_node_id",
            )?,
            BackupManifestContent::new(
                decode_id(
                    self.file_version_id,
                    "backup_prune_plan_entries.file_version_id",
                )?,
                byte_length,
                sha256,
            ),
        ))
    }
}

/// PostgreSQL row for one durable restore-plan provenance record. The row is
/// logical metadata only; physical Object/replica identity is intentionally
/// absent.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupRestorePlanRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub backup_set_id: Uuid,
    pub snapshot_id: Uuid,
    pub target_library_id: Uuid,
    pub target_parent_node_id: Uuid,
    pub operation_id: String,
    pub fingerprint_version: i16,
    pub request_fingerprint: Vec<u8>,
    pub destination_name: String,
    pub base_journal_epoch: i64,
    pub base_journal_head: i64,
    pub item_count: i64,
    pub content_item_count: i64,
    pub state: String,
    pub created_at: OffsetDateTime,
    pub stale_at: Option<OffsetDateTime>,
}

impl BackupRestorePlanRow {
    pub fn try_into_domain(self) -> Result<BackupRestorePlan, MappingError> {
        let id = decode_id(self.id, "backup_restore_plans.id")?;
        let owner_user_id = decode_id(self.owner_user_id, "backup_restore_plans.owner_user_id")?;
        let backup_set_id = decode_id(self.backup_set_id, "backup_restore_plans.backup_set_id")?;
        let snapshot_id = decode_id(self.snapshot_id, "backup_restore_plans.snapshot_id")?;
        let target_library_id = decode_id(
            self.target_library_id,
            "backup_restore_plans.target_library_id",
        )?;
        let target_parent_node_id = decode_id(
            self.target_parent_node_id,
            "backup_restore_plans.target_parent_node_id",
        )?;
        let fingerprint_version =
            u16::try_from(self.fingerprint_version).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_restore_plans.fingerprint_version",
            })?;
        let fingerprint_bytes =
            <[u8; 32]>::try_from(self.request_fingerprint.as_slice()).map_err(|_| {
                MappingError::InvalidDigest {
                    field: "backup_restore_plans.request_fingerprint",
                }
            })?;
        let request = BackupRestorePlanRequest::new(
            backup_set_id,
            snapshot_id,
            target_library_id,
            target_parent_node_id,
            LogicalName::new(self.destination_name).map_err(|_| MappingError::InvalidName {
                field: "backup_restore_plans.destination_name",
            })?,
        );
        let fingerprint =
            BackupRestorePlanIdempotencyFingerprint::new(fingerprint_version, fingerprint_bytes);
        if fingerprint_version != BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION
            || request.fingerprint() != fingerprint
        {
            return Err(MappingError::RelationMismatch {
                relation: "backup_restore_plans.request_fingerprint",
            });
        }
        let base_journal_epoch = decode_sequence(
            self.base_journal_epoch,
            "backup_restore_plans.base_journal_epoch",
        )?;
        let base_journal_head = decode_sequence(
            self.base_journal_head,
            "backup_restore_plans.base_journal_head",
        )?;
        let item_count =
            u64::try_from(self.item_count).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_restore_plans.item_count",
            })?;
        let content_item_count =
            u64::try_from(self.content_item_count).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_restore_plans.content_item_count",
            })?;
        let state = BackupRestorePlanState::from_str(&self.state).map_err(|_| {
            MappingError::InvalidEnum {
                field: "backup_restore_plans.state",
            }
        })?;
        BackupRestorePlan::new(
            id,
            owner_user_id,
            self.operation_id,
            fingerprint,
            request,
            base_journal_epoch,
            base_journal_head,
            item_count,
            content_item_count,
            state,
            Timestamp::from_offset_datetime(self.created_at),
            self.stale_at.map(Timestamp::from_offset_datetime),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one immutable logical restore-plan entry.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupRestorePlanEntryRow {
    pub plan_id: Uuid,
    pub ordinal: i64,
    pub planned_node_id: Uuid,
    pub planned_parent_node_id: Uuid,
    pub source_snapshot_node_id: Option<Uuid>,
    pub source_parent_node_id: Option<Uuid>,
    pub source_state: String,
    pub action: String,
    pub kind: String,
    pub name: String,
    pub source_revision: Option<String>,
    pub file_version_id: Option<Uuid>,
    pub content_length: Option<String>,
    pub content_sha256: Option<Vec<u8>>,
}

impl BackupRestorePlanEntryRow {
    pub fn try_into_domain(self) -> Result<BackupRestorePlanEntry, MappingError> {
        let plan_id = decode_id(self.plan_id, "backup_restore_plan_entries.plan_id")?;
        let ordinal = u64::try_from(self.ordinal).map_err(|_| MappingError::InvalidDecimal {
            field: "backup_restore_plan_entries.ordinal",
        })?;
        let planned_node_id = decode_id(
            self.planned_node_id,
            "backup_restore_plan_entries.planned_node_id",
        )?;
        let planned_parent_node_id = decode_id(
            self.planned_parent_node_id,
            "backup_restore_plan_entries.planned_parent_node_id",
        )?;
        let source_snapshot_node_id = self
            .source_snapshot_node_id
            .map(|value| decode_id(value, "backup_restore_plan_entries.source_snapshot_node_id"))
            .transpose()?;
        let source_parent_node_id = self
            .source_parent_node_id
            .map(|value| decode_id(value, "backup_restore_plan_entries.source_parent_node_id"))
            .transpose()?;
        let source_state = decode_public_node_state(&self.source_state)?;
        let action =
            BackupRestoreAction::from_str(&self.action).map_err(|_| MappingError::InvalidEnum {
                field: "backup_restore_plan_entries.action",
            })?;
        let kind = decode_node_kind(&self.kind)?;
        let source_revision = self
            .source_revision
            .as_deref()
            .map(|value| revision_from_str(value, "backup_restore_plan_entries.source_revision"))
            .transpose()?;
        let content = match (
            self.file_version_id,
            self.content_length,
            self.content_sha256,
        ) {
            (Some(file_version_id), Some(content_length), Some(content_sha256)) => {
                let file_version_id = decode_id(
                    file_version_id,
                    "backup_restore_plan_entries.file_version_id",
                )?;
                let byte_length =
                    u64::from_str(&content_length).map_err(|_| MappingError::InvalidDecimal {
                        field: "backup_restore_plan_entries.content_length",
                    })?;
                let sha256 = Sha256Digest::try_from(content_sha256.as_slice()).map_err(|_| {
                    MappingError::InvalidDigest {
                        field: "backup_restore_plan_entries.content_sha256",
                    }
                })?;
                Some(BackupManifestContent::new(
                    file_version_id,
                    byte_length,
                    sha256,
                ))
            }
            (None, None, None) => None,
            _ => {
                return Err(MappingError::RelationMismatch {
                    relation: "backup_restore_plan_entries.content",
                });
            }
        };
        BackupRestorePlanEntry::new(
            plan_id,
            ordinal,
            planned_node_id,
            planned_parent_node_id,
            source_snapshot_node_id,
            source_parent_node_id,
            source_state,
            action,
            kind,
            LogicalName::new(self.name).map_err(|_| MappingError::InvalidName {
                field: "backup_restore_plan_entries.name",
            })?,
            source_revision,
            content,
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one committed restore receipt. It contains only logical
/// execution evidence; retained physical Object identity stays in private
/// preflight state and is never serialized here.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupRestoreExecutionRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub restore_plan_id: Uuid,
    pub target_library_id: Uuid,
    pub journal_first_sequence: i64,
    pub journal_last_sequence: i64,
    pub created_node_count: i64,
    pub created_file_version_count: i64,
    pub state: String,
    pub executed_at: OffsetDateTime,
}

impl BackupRestoreExecutionRow {
    pub fn try_into_domain(self) -> Result<BackupRestoreExecution, MappingError> {
        if self.state != "COMMITTED" {
            return Err(MappingError::InvalidEnum {
                field: "backup_restore_executions.state",
            });
        }
        BackupRestoreExecution::new(
            decode_id(self.id, "backup_restore_executions.id")?,
            decode_id(
                self.owner_user_id,
                "backup_restore_executions.owner_user_id",
            )?,
            decode_id(
                self.restore_plan_id,
                "backup_restore_executions.restore_plan_id",
            )?,
            decode_id(
                self.target_library_id,
                "backup_restore_executions.target_library_id",
            )?,
            decode_sequence(
                self.journal_first_sequence,
                "backup_restore_executions.journal_first_sequence",
            )?,
            decode_sequence(
                self.journal_last_sequence,
                "backup_restore_executions.journal_last_sequence",
            )?,
            u64::try_from(self.created_node_count).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_restore_executions.created_node_count",
            })?,
            u64::try_from(self.created_file_version_count).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_restore_executions.created_file_version_count",
                }
            })?,
            Timestamp::from_offset_datetime(self.executed_at),
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one immutable logical execution-entry mapping.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupRestoreExecutionEntryRow {
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub ordinal: i64,
    pub destination_node_id: Uuid,
    pub destination_file_version_id: Option<Uuid>,
    pub node_created_journal_sequence: Option<i64>,
    pub file_content_committed_journal_sequence: Option<i64>,
}

impl BackupRestoreExecutionEntryRow {
    pub fn try_into_domain(self) -> Result<BackupRestoreExecutionEntry, MappingError> {
        BackupRestoreExecutionEntry::new(
            decode_id(
                self.execution_id,
                "backup_restore_execution_entries.execution_id",
            )?,
            decode_id(self.plan_id, "backup_restore_execution_entries.plan_id")?,
            u64::try_from(self.ordinal).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_restore_execution_entries.ordinal",
            })?,
            decode_id(
                self.destination_node_id,
                "backup_restore_execution_entries.destination_node_id",
            )?,
            self.destination_file_version_id
                .map(|value| {
                    decode_id(
                        value,
                        "backup_restore_execution_entries.destination_file_version_id",
                    )
                })
                .transpose()?,
            self.node_created_journal_sequence
                .map(|value| {
                    decode_sequence(
                        value,
                        "backup_restore_execution_entries.node_created_journal_sequence",
                    )
                })
                .transpose()?,
            self.file_content_committed_journal_sequence
                .map(|value| {
                    decode_sequence(
                        value,
                        "backup_restore_execution_entries.file_content_committed_journal_sequence",
                    )
                })
                .transpose()?,
        )
        .map_err(MappingError::from)
    }
}

/// PostgreSQL row for one durable manual maintenance-run coordinator.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct BackupMaintenanceRunRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub backup_set_id: Uuid,
    pub policy_revision_id: Uuid,
    pub policy_revision_number: i64,
    pub operation_id: String,
    pub fingerprint_version: i16,
    pub request_fingerprint: Vec<u8>,
    pub capture_operation_id: String,
    pub expiry_plan_operation_id: String,
    pub state: String,
    pub captured_snapshot_id: Option<Uuid>,
    pub expiry_plan_id: Option<Uuid>,
    pub expiry_execution_id: Option<Uuid>,
    pub snapshot_captured_at: Option<OffsetDateTime>,
    pub expiry_planned_at: Option<OffsetDateTime>,
    pub maintenance_completed_at: Option<OffsetDateTime>,
    pub stale_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

/// Joined read row for a restore plan and its optional committed execution
/// receipt. The joined shape is used only by the heterogeneous activity list;
/// all nullable receipt columns must be present together or the mapping fails
/// closed.
#[derive(Clone, Debug, FromRow)]
struct BackupRestoreOperationRow {
    id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    snapshot_id: Uuid,
    target_library_id: Uuid,
    target_parent_node_id: Uuid,
    operation_id: String,
    fingerprint_version: i16,
    request_fingerprint: Vec<u8>,
    destination_name: String,
    base_journal_epoch: i64,
    base_journal_head: i64,
    item_count: i64,
    content_item_count: i64,
    state: String,
    created_at: OffsetDateTime,
    stale_at: Option<OffsetDateTime>,
    execution_id: Option<Uuid>,
    execution_owner_user_id: Option<Uuid>,
    execution_restore_plan_id: Option<Uuid>,
    execution_target_library_id: Option<Uuid>,
    execution_journal_first_sequence: Option<i64>,
    execution_journal_last_sequence: Option<i64>,
    execution_created_node_count: Option<i64>,
    execution_created_file_version_count: Option<i64>,
    execution_state: Option<String>,
    execution_executed_at: Option<OffsetDateTime>,
}

impl BackupRestoreOperationRow {
    fn try_into_detail(self) -> Result<BackupOperationDetail, BackupError> {
        let plan = BackupRestorePlanRow {
            id: self.id,
            owner_user_id: self.owner_user_id,
            backup_set_id: self.backup_set_id,
            snapshot_id: self.snapshot_id,
            target_library_id: self.target_library_id,
            target_parent_node_id: self.target_parent_node_id,
            operation_id: self.operation_id,
            fingerprint_version: self.fingerprint_version,
            request_fingerprint: self.request_fingerprint,
            destination_name: self.destination_name,
            base_journal_epoch: self.base_journal_epoch,
            base_journal_head: self.base_journal_head,
            item_count: self.item_count,
            content_item_count: self.content_item_count,
            state: self.state,
            created_at: self.created_at,
            stale_at: self.stale_at,
        }
        .try_into_domain()?;
        let execution = optional_restore_execution(
            self.execution_id,
            self.execution_owner_user_id,
            self.execution_restore_plan_id,
            self.execution_target_library_id,
            self.execution_journal_first_sequence,
            self.execution_journal_last_sequence,
            self.execution_created_node_count,
            self.execution_created_file_version_count,
            self.execution_state,
            self.execution_executed_at,
        )?;
        validate_restore_operation_shape(&plan, execution.as_ref())?;
        Ok(BackupOperationDetail::Restore { plan, execution })
    }
}

/// Joined read row for a prune plan and its optional committed execution
/// receipt. No physical columns are selected into this projection.
#[derive(Clone, Debug, FromRow)]
struct BackupPruneOperationRow {
    id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    snapshot_id: Uuid,
    operation_id: String,
    fingerprint_version: i16,
    request_fingerprint: Vec<u8>,
    snapshot_manifest_item_count: i64,
    snapshot_content_reference_count: i64,
    planned_pin_release_count: i64,
    distinct_retained_content_count: i64,
    retained_after_release_count: i64,
    would_become_unreferenced_count: i64,
    state: String,
    created_at: OffsetDateTime,
    stale_at: Option<OffsetDateTime>,
    execution_id: Option<Uuid>,
    execution_owner_user_id: Option<Uuid>,
    execution_prune_plan_id: Option<Uuid>,
    execution_backup_set_id: Option<Uuid>,
    execution_snapshot_id: Option<Uuid>,
    execution_released_pin_count: Option<i64>,
    execution_distinct_object_count: Option<i64>,
    execution_retained_by_other_reference_count: Option<i64>,
    execution_gc_handoff_object_count: Option<i64>,
    execution_state: Option<String>,
    execution_executed_at: Option<OffsetDateTime>,
}

impl BackupPruneOperationRow {
    fn try_into_detail(self) -> Result<BackupOperationDetail, BackupError> {
        let plan = BackupPrunePlanRow {
            id: self.id,
            owner_user_id: self.owner_user_id,
            backup_set_id: self.backup_set_id,
            snapshot_id: self.snapshot_id,
            operation_id: self.operation_id,
            fingerprint_version: self.fingerprint_version,
            request_fingerprint: self.request_fingerprint,
            snapshot_manifest_item_count: self.snapshot_manifest_item_count,
            snapshot_content_reference_count: self.snapshot_content_reference_count,
            planned_pin_release_count: self.planned_pin_release_count,
            distinct_retained_content_count: self.distinct_retained_content_count,
            retained_after_release_count: self.retained_after_release_count,
            would_become_unreferenced_count: self.would_become_unreferenced_count,
            state: self.state,
            created_at: self.created_at,
            stale_at: self.stale_at,
        }
        .try_into_domain()?;
        let execution = optional_prune_execution(
            self.execution_id,
            self.execution_owner_user_id,
            self.execution_prune_plan_id,
            self.execution_backup_set_id,
            self.execution_snapshot_id,
            self.execution_released_pin_count,
            self.execution_distinct_object_count,
            self.execution_retained_by_other_reference_count,
            self.execution_gc_handoff_object_count,
            self.execution_state,
            self.execution_executed_at,
        )?;
        validate_prune_operation_shape(&plan, execution.as_ref())?;
        Ok(BackupOperationDetail::Prune { plan, execution })
    }
}

#[allow(clippy::too_many_arguments)]
fn optional_restore_execution(
    id: Option<Uuid>,
    owner_user_id: Option<Uuid>,
    restore_plan_id: Option<Uuid>,
    target_library_id: Option<Uuid>,
    journal_first_sequence: Option<i64>,
    journal_last_sequence: Option<i64>,
    created_node_count: Option<i64>,
    created_file_version_count: Option<i64>,
    state: Option<String>,
    executed_at: Option<OffsetDateTime>,
) -> Result<Option<BackupRestoreExecution>, BackupError> {
    let values = (
        id,
        owner_user_id,
        restore_plan_id,
        target_library_id,
        journal_first_sequence,
        journal_last_sequence,
        created_node_count,
        created_file_version_count,
        state,
        executed_at,
    );
    if values.0.is_none()
        && values.1.is_none()
        && values.2.is_none()
        && values.3.is_none()
        && values.4.is_none()
        && values.5.is_none()
        && values.6.is_none()
        && values.7.is_none()
        && values.8.is_none()
        && values.9.is_none()
    {
        return Ok(None);
    }
    let (
        Some(id),
        Some(owner_user_id),
        Some(restore_plan_id),
        Some(target_library_id),
        Some(journal_first_sequence),
        Some(journal_last_sequence),
        Some(created_node_count),
        Some(created_file_version_count),
        Some(state),
        Some(executed_at),
    ) = values
    else {
        return Err(BackupError::InvalidPersistedData);
    };
    Ok(Some(
        BackupRestoreExecutionRow {
            id,
            owner_user_id,
            restore_plan_id,
            target_library_id,
            journal_first_sequence,
            journal_last_sequence,
            created_node_count,
            created_file_version_count,
            state,
            executed_at,
        }
        .try_into_domain()?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn optional_prune_execution(
    id: Option<Uuid>,
    owner_user_id: Option<Uuid>,
    prune_plan_id: Option<Uuid>,
    backup_set_id: Option<Uuid>,
    snapshot_id: Option<Uuid>,
    released_pin_count: Option<i64>,
    distinct_object_count: Option<i64>,
    retained_by_other_reference_count: Option<i64>,
    gc_handoff_object_count: Option<i64>,
    state: Option<String>,
    executed_at: Option<OffsetDateTime>,
) -> Result<Option<BackupPruneExecution>, BackupError> {
    let values = (
        id,
        owner_user_id,
        prune_plan_id,
        backup_set_id,
        snapshot_id,
        released_pin_count,
        distinct_object_count,
        retained_by_other_reference_count,
        gc_handoff_object_count,
        state,
        executed_at,
    );
    if values.0.is_none()
        && values.1.is_none()
        && values.2.is_none()
        && values.3.is_none()
        && values.4.is_none()
        && values.5.is_none()
        && values.6.is_none()
        && values.7.is_none()
        && values.8.is_none()
        && values.9.is_none()
        && values.10.is_none()
    {
        return Ok(None);
    }
    let (
        Some(id),
        Some(owner_user_id),
        Some(prune_plan_id),
        Some(backup_set_id),
        Some(snapshot_id),
        Some(released_pin_count),
        Some(distinct_object_count),
        Some(retained_by_other_reference_count),
        Some(gc_handoff_object_count),
        Some(state),
        Some(executed_at),
    ) = values
    else {
        return Err(BackupError::InvalidPersistedData);
    };
    Ok(Some(
        BackupPruneExecutionRow {
            id,
            owner_user_id,
            prune_plan_id,
            backup_set_id,
            snapshot_id,
            released_pin_count,
            distinct_object_count,
            retained_by_other_reference_count,
            gc_handoff_object_count,
            state,
            executed_at,
        }
        .try_into_domain()?,
    ))
}

fn validate_restore_operation_shape(
    plan: &BackupRestorePlan,
    execution: Option<&BackupRestoreExecution>,
) -> Result<(), BackupError> {
    if let Some(execution) = execution {
        if plan.state() != BackupRestorePlanState::Executed
            || execution.owner_user_id() != plan.owner_user_id()
            || execution.plan_id() != plan.id()
            || execution.target_library_id() != plan.target_library_id()
        {
            return Err(BackupError::InvalidPersistedData);
        }
    } else if plan.state() == BackupRestorePlanState::Executed {
        return Err(BackupError::InvalidPersistedData);
    }
    Ok(())
}

fn validate_prune_operation_shape(
    plan: &BackupPrunePlan,
    execution: Option<&BackupPruneExecution>,
) -> Result<(), BackupError> {
    if let Some(execution) = execution {
        if plan.state() != BackupPrunePlanState::Executed
            || execution.owner_user_id() != plan.owner_user_id()
            || execution.prune_plan_id() != plan.id()
            || execution.backup_set_id() != plan.backup_set_id()
            || execution.snapshot_id() != plan.snapshot_id()
        {
            return Err(BackupError::InvalidPersistedData);
        }
    } else if plan.state() == BackupPrunePlanState::Executed {
        return Err(BackupError::InvalidPersistedData);
    }
    Ok(())
}

impl BackupMaintenanceRunRow {
    pub fn try_into_domain(self) -> Result<BackupMaintenanceRun, MappingError> {
        let id = decode_id(self.id, "backup_maintenance_runs.id")?;
        let owner_user_id = decode_id(self.owner_user_id, "backup_maintenance_runs.owner_user_id")?;
        let backup_set_id = decode_id(self.backup_set_id, "backup_maintenance_runs.backup_set_id")?;
        let policy_revision_id = decode_id(
            self.policy_revision_id,
            "backup_maintenance_runs.policy_revision_id",
        )?;
        let policy_revision_number = BackupSnapshotRetentionPolicyRevisionNumber::new(
            u64::try_from(self.policy_revision_number).map_err(|_| {
                MappingError::InvalidDecimal {
                    field: "backup_maintenance_runs.policy_revision_number",
                }
            })?,
        )
        .map_err(MappingError::from)?;
        let fingerprint_version =
            u16::try_from(self.fingerprint_version).map_err(|_| MappingError::InvalidDecimal {
                field: "backup_maintenance_runs.fingerprint_version",
            })?;
        let fingerprint_bytes =
            <[u8; 32]>::try_from(self.request_fingerprint.as_slice()).map_err(|_| {
                MappingError::InvalidDigest {
                    field: "backup_maintenance_runs.request_fingerprint",
                }
            })?;
        let request = BackupMaintenanceRunRequest::new(backup_set_id);
        let fingerprint =
            BackupMaintenanceRunIdempotencyFingerprint::new(fingerprint_version, fingerprint_bytes);
        if fingerprint_version != BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION
            || request.fingerprint() != fingerprint
        {
            return Err(MappingError::RelationMismatch {
                relation: "backup_maintenance_runs.request_fingerprint",
            });
        }
        let state = BackupMaintenanceRunState::from_str(&self.state).map_err(|_| {
            MappingError::InvalidEnum {
                field: "backup_maintenance_runs.state",
            }
        })?;

        BackupMaintenanceRun::new(
            id,
            owner_user_id,
            self.operation_id,
            fingerprint,
            backup_set_id,
            policy_revision_id,
            policy_revision_number,
            self.capture_operation_id,
            self.expiry_plan_operation_id,
            state,
            self.captured_snapshot_id
                .map(|value| decode_id(value, "backup_maintenance_runs.captured_snapshot_id"))
                .transpose()?,
            self.expiry_plan_id
                .map(|value| decode_id(value, "backup_maintenance_runs.expiry_plan_id"))
                .transpose()?,
            self.expiry_execution_id
                .map(|value| decode_id(value, "backup_maintenance_runs.expiry_execution_id"))
                .transpose()?,
            self.snapshot_captured_at
                .map(Timestamp::from_offset_datetime),
            self.expiry_planned_at.map(Timestamp::from_offset_datetime),
            self.maintenance_completed_at
                .map(Timestamp::from_offset_datetime),
            self.stale_at.map(Timestamp::from_offset_datetime),
            Timestamp::from_offset_datetime(self.created_at),
        )
        .map_err(MappingError::from)
    }
}

/// The PostgreSQL-backed backup domain service. Snapshot capture acquires the
/// per-library namespace guard and copies the current logical Node projection in
/// one transaction so live mutations cannot mix into the captured cut.
#[derive(Clone)]
pub struct BackupService {
    pool: DatabasePool,
}

impl BackupService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Create one durable explicit maintenance run. Creation binds the current
    /// snapshot-retention policy revision and persists immutable child
    /// operation identities for canonical replay across crashes.
    pub async fn create_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        validate_operation_key(&operation_id)?;
        let request = BackupMaintenanceRunRequest::new(backup_set_id);
        let fingerprint = request.fingerprint();
        let run_id = BackupMaintenanceRunId::new();
        let capture_operation_id = format!("maint:{run_id}:capture");
        let expiry_plan_operation_id = format!("maint:{run_id}:expiry-plan");
        validate_operation_key(&capture_operation_id)?;
        validate_operation_key(&expiry_plan_operation_id)?;

        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        if let Some(existing) =
            load_maintenance_run_by_operation(&mut transaction, owner_user_id, &operation_id, true)
                .await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::MaintenanceRunPreflight(
                    BackupMaintenanceRunPreflightIssue::OperationConflict,
                ));
            }
            return Ok(existing);
        }

        lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id).await?;

        if let Some(existing) =
            load_maintenance_run_by_operation(&mut transaction, owner_user_id, &operation_id, true)
                .await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::MaintenanceRunPreflight(
                    BackupMaintenanceRunPreflightIssue::OperationConflict,
                ));
            }
            return Ok(existing);
        }

        let current_policy = load_current_retention_policy_for_update(
            &mut transaction,
            owner_user_id,
            backup_set_id,
        )
        .await?
        .ok_or(BackupError::MaintenanceRunPreflight(
            BackupMaintenanceRunPreflightIssue::RetentionPolicyNotConfigured,
        ))?
        .try_into_domain()?;

        let active_run = sqlx::query_scalar::<_, Uuid>(
            "SELECT id
             FROM backup_maintenance_runs
             WHERE backup_set_id = $1
               AND state IN ('CREATED', 'SNAPSHOT_CAPTURED', 'EXPIRY_PLANNED')
             ORDER BY id ASC
             LIMIT 1
             FOR SHARE",
        )
        .bind(backup_set_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?;
        if active_run.is_some() {
            transaction.commit().await?;
            return Err(BackupError::MaintenanceRunPreflight(
                BackupMaintenanceRunPreflightIssue::MaintenanceAlreadyRunning,
            ));
        }

        let inserted = sqlx::query_as::<_, BackupMaintenanceRunRow>(
            "INSERT INTO backup_maintenance_runs
                (id, owner_user_id, backup_set_id, policy_revision_id,
                 policy_revision_number, operation_id, fingerprint_version,
                 request_fingerprint, capture_operation_id,
                 expiry_plan_operation_id, state, captured_snapshot_id,
                 expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                 expiry_planned_at, maintenance_completed_at, stale_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                     'CREATED', NULL, NULL, NULL, NULL, NULL, NULL, NULL,
                     clock_timestamp())
             ON CONFLICT DO NOTHING
             RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                       policy_revision_number, operation_id, fingerprint_version,
                       request_fingerprint, capture_operation_id,
                       expiry_plan_operation_id, state, captured_snapshot_id,
                       expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                       expiry_planned_at, maintenance_completed_at, stale_at,
                       created_at",
        )
        .bind(run_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(current_policy.id().into_uuid())
        .bind(
            i64::try_from(current_policy.revision_number().get())
                .map_err(|_| BackupError::InvalidPersistedData)?,
        )
        .bind(&operation_id)
        .bind(i16::try_from(fingerprint.version()).map_err(|_| BackupError::InvalidRequest)?)
        .bind(fingerprint.as_bytes().as_slice())
        .bind(&capture_operation_id)
        .bind(&expiry_plan_operation_id)
        .fetch_optional(&mut *transaction)
        .await?;

        let row = if let Some(row) = inserted {
            row
        } else if let Some(row) =
            load_maintenance_run_by_operation(&mut transaction, owner_user_id, &operation_id, true)
                .await?
        {
            row
        } else {
            let active_run = sqlx::query_scalar::<_, Uuid>(
                "SELECT id
                 FROM backup_maintenance_runs
                 WHERE backup_set_id = $1
                   AND state IN ('CREATED', 'SNAPSHOT_CAPTURED', 'EXPIRY_PLANNED')
                 ORDER BY id ASC
                 LIMIT 1
                 FOR SHARE",
            )
            .bind(backup_set_id.into_uuid())
            .fetch_optional(&mut *transaction)
            .await?;
            if active_run.is_some() {
                transaction.commit().await?;
                return Err(BackupError::MaintenanceRunPreflight(
                    BackupMaintenanceRunPreflightIssue::MaintenanceAlreadyRunning,
                ));
            }
            return Err(BackupError::InvalidPersistedData);
        };
        let run = row.try_into_domain()?;
        transaction.commit().await?;
        if run.request_fingerprint() != fingerprint {
            return Err(BackupError::MaintenanceRunPreflight(
                BackupMaintenanceRunPreflightIssue::OperationConflict,
            ));
        }
        Ok(run)
    }

    /// Read one owner-scoped maintenance run. Missing and foreign runs are both
    /// concealed as `NotFound`.
    pub async fn get_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        load_maintenance_run(self.pool.sqlx_pool(), owner_user_id, run_id)
            .await?
            .ok_or(BackupError::NotFound)
            .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// Explicitly advance one manual maintenance run. This coordinator never
    /// holds one transaction across child capability calls; it recovers through
    /// durable child operation identities and canonical child replay.
    pub async fn advance_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        loop {
            let run = self
                .get_backup_maintenance_run(owner_user_id, run_id)
                .await?;
            match run.state() {
                BackupMaintenanceRunState::Completed => return Ok(run),
                BackupMaintenanceRunState::Stale => {
                    return Err(BackupError::MaintenanceRunPreflight(
                        BackupMaintenanceRunPreflightIssue::InvalidRunState,
                    ));
                }
                BackupMaintenanceRunState::Created => {
                    let snapshot = self
                        .capture_snapshot(
                            owner_user_id,
                            run.backup_set_id(),
                            SnapshotId::new(),
                            run.capture_operation_id().to_owned(),
                        )
                        .await?;
                    persist_maintenance_snapshot_progress(
                        self.pool.sqlx_pool(),
                        owner_user_id,
                        run.id(),
                        snapshot.id(),
                        snapshot
                            .committed_at()
                            .ok_or(BackupError::InvalidPersistedData)?,
                    )
                    .await?;
                }
                BackupMaintenanceRunState::SnapshotCaptured => {
                    if maintenance_has_foreign_active_expiry_plan(
                        self.pool.sqlx_pool(),
                        run.backup_set_id(),
                        run.expiry_plan_operation_id(),
                    )
                    .await?
                    {
                        stale_maintenance_run(self.pool.sqlx_pool(), owner_user_id, run.id())
                            .await?;
                        return Err(BackupError::MaintenanceRunPreflight(
                            BackupMaintenanceRunPreflightIssue::ExpiryPlanStale,
                        ));
                    }
                    let plan = match self
                        .create_snapshot_expiry_plan_with_expected_policy(
                            owner_user_id,
                            run.expiry_plan_operation_id().to_owned(),
                            run.backup_set_id(),
                            Some((run.policy_revision_id(), run.policy_revision_number())),
                        )
                        .await
                    {
                        Ok(plan) => plan,
                        Err(BackupError::ExpiryPreflight(
                            BackupSnapshotExpiryPreflightIssue::ExpiryAlreadyPlanned,
                        )) => {
                            stale_maintenance_run(self.pool.sqlx_pool(), owner_user_id, run.id())
                                .await?;
                            return Err(BackupError::MaintenanceRunPreflight(
                                BackupMaintenanceRunPreflightIssue::ExpiryPlanStale,
                            ));
                        }
                        Err(BackupError::MaintenanceRunPreflight(
                            BackupMaintenanceRunPreflightIssue::PolicyChanged,
                        )) => {
                            stale_maintenance_run(self.pool.sqlx_pool(), owner_user_id, run.id())
                                .await?;
                            return Err(BackupError::MaintenanceRunPreflight(
                                BackupMaintenanceRunPreflightIssue::PolicyChanged,
                            ));
                        }
                        Err(error) => return Err(error),
                    };
                    if plan.policy_revision_id() != run.policy_revision_id()
                        || plan.policy_revision_number() != run.policy_revision_number()
                    {
                        stale_maintenance_run(self.pool.sqlx_pool(), owner_user_id, run.id())
                            .await?;
                        return Err(BackupError::MaintenanceRunPreflight(
                            BackupMaintenanceRunPreflightIssue::PolicyChanged,
                        ));
                    }
                    persist_maintenance_expiry_plan_progress(
                        self.pool.sqlx_pool(),
                        owner_user_id,
                        run.id(),
                        plan.id(),
                        plan.created_at(),
                    )
                    .await?;
                }
                BackupMaintenanceRunState::ExpiryPlanned => {
                    let plan_id = run
                        .expiry_plan_id()
                        .ok_or(BackupError::InvalidPersistedData)?;
                    let execution = match self
                        .execute_snapshot_expiry_plan(owner_user_id, plan_id)
                        .await
                    {
                        Ok(execution) => execution,
                        Err(BackupError::ExpiryExecutionPreflight(
                            BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
                        )) => {
                            stale_maintenance_run(self.pool.sqlx_pool(), owner_user_id, run.id())
                                .await?;
                            return Err(BackupError::MaintenanceRunPreflight(
                                BackupMaintenanceRunPreflightIssue::ExpiryPlanStale,
                            ));
                        }
                        Err(error) => return Err(error),
                    };
                    persist_maintenance_completion(
                        self.pool.sqlx_pool(),
                        owner_user_id,
                        run.id(),
                        execution.id(),
                        execution.executed_at(),
                    )
                    .await?;
                }
            }
        }
    }

    /// Create or replay one owner-scoped backup set. The source library must be
    /// owned by the authenticated user; name uniqueness is enforced per owner.
    /// The caller-supplied set ID is the durable creation operation identity:
    /// identical semantics replay the row and changed semantics conflict.
    pub async fn create_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        name: LogicalName,
        source_library_id: LibraryId,
        retention_days: Option<u32>,
        observed_at: Timestamp,
    ) -> Result<BackupSet, BackupError> {
        let set = BackupSet::new(
            backup_set_id,
            owner_user_id,
            name,
            source_library_id,
            retention_days,
            observed_at,
        )?;
        let row = BackupSetRow::from_domain(&set)?;
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM libraries
                WHERE id = $1 AND owner_user_id = $2
             )",
        )
        .bind(source_library_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if !exists {
            return Err(BackupError::NotFound);
        }

        let inserted = sqlx::query(
            "INSERT INTO backup_sets
                (id, owner_user_id, name, source_library_id, source,
                 retention_days, state, created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::NUMERIC)
             ON CONFLICT DO NOTHING",
        )
        .bind(row.id)
        .bind(row.owner_user_id)
        .bind(&row.name)
        .bind(row.source_library_id)
        .bind(&row.source)
        .bind(row.retention_days)
        .bind(&row.state)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            == 1;

        if !inserted {
            let existing = sqlx::query_as::<_, BackupSetRow>(
                "SELECT id, owner_user_id, name, source_library_id, source,
                        retention_days, state, created_at, updated_at,
                        revision::TEXT AS revision
                 FROM backup_sets
                 WHERE id = $1 AND owner_user_id = $2
                 FOR UPDATE",
            )
            .bind(backup_set_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .fetch_optional(&mut *transaction)
            .await?;
            let Some(existing) = existing else {
                transaction.commit().await?;
                return Err(BackupError::BackupSetConflict);
            };
            let existing = existing.try_into_domain()?;
            let request_matches = existing.name() == set.name()
                && existing.source_library_id() == set.source_library_id()
                && existing.retention_days() == set.retention_days();
            transaction.commit().await?;
            if !request_matches {
                return Err(BackupError::BackupSetOperationConflict);
            }
            return Ok(existing);
        }

        transaction.commit().await?;

        Ok(set)
    }

    /// Begin one building snapshot for an owned backup set. The owner-scope and
    /// set-state are validated, the per-library namespace guard is acquired, and
    /// the current logical Node projection is copied in one transaction. The
    /// `operation_id` makes the capture retry-idempotent: a replay with the same
    /// key returns the original snapshot instead of creating a duplicate.
    pub async fn capture_snapshot(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        operation_id: String,
    ) -> Result<BackupSnapshot, BackupError> {
        validate_operation_key(&operation_id)?;
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        let set_row = sqlx::query_as::<_, BackupSetRow>(
            "SELECT id, owner_user_id, name, source_library_id, source,
                    retention_days, state, created_at, updated_at, revision::TEXT AS revision
             FROM backup_sets
             WHERE id = $1 AND owner_user_id = $2
             FOR UPDATE",
        )
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;

        let set = set_row.try_into_domain()?;
        if set.state() == BackupSetState::Disabled {
            return Err(BackupError::BackupSetDisabled);
        }

        let existing = sqlx::query_as::<_, BackupSnapshotRow>(
            "SELECT id, backup_set_id, owner_user_id, source_library_id, operation_id,
                    snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
                    terminal_node_id, content_reference_count, state, created_at,
                    committed_at, expired_at
             FROM backup_snapshots
             WHERE owner_user_id = $1 AND backup_set_id = $2 AND operation_id = $3",
        )
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(&operation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(existing) = existing {
            transaction.commit().await?;
            return Ok(existing.try_into_domain()?);
        }

        acquire_namespace_guard(&mut transaction, set.source_library_id()).await?;

        let head = sqlx::query_as::<_, (i64, i64)>(
            "SELECT journal_epoch, sync_head
             FROM libraries
             WHERE id = $1 AND owner_user_id = $2
             FOR UPDATE",
        )
        .bind(set.source_library_id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;
        if head.0 <= 0 || head.1 < 0 {
            return Err(BackupError::InvalidPersistedData);
        }

        let inserted = sqlx::query(
            "INSERT INTO backup_snapshots
                (id, backup_set_id, owner_user_id, source_library_id, operation_id,
                 snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
                 terminal_node_id, content_reference_count, state, created_at,
                 committed_at, expired_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, NULL, 0, 'BUILDING',
                     CURRENT_TIMESTAMP, NULL, NULL)
             ON CONFLICT (owner_user_id, backup_set_id, operation_id)
             DO NOTHING",
        )
        .bind(snapshot_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(set.source_library_id().into_uuid())
        .bind(&operation_id)
        .bind(head.0)
        .bind(head.1)
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            == 1;

        if !inserted {
            return Err(BackupError::SnapshotConflict);
        }

        let copied = sqlx::query(
            "INSERT INTO backup_snapshot_nodes
                (snapshot_id, node_id, parent_node_id, name, kind, state, revision,
                 current_version_id, content_length, content_sha256,
                 node_created_at, node_updated_at)
             SELECT $1, n.id, n.parent_node_id, n.name, n.kind, n.state, n.revision,
                    n.current_version_id, o.plaintext_length, o.canonical_hash,
                    n.created_at, n.updated_at
             FROM nodes AS n
             LEFT JOIN file_versions AS v
               ON v.id = n.current_version_id
              AND v.library_id = n.library_id
             LEFT JOIN objects AS o
               ON o.id = v.object_id
              AND o.dedup_domain_id = v.object_dedup_domain_id
             WHERE n.library_id = $2
               AND n.state IN ('ACTIVE', 'TRASHED')",
        )
        .bind(snapshot_id.into_uuid())
        .bind(set.source_library_id().into_uuid())
        .execute(&mut *transaction)
        .await?;
        let manifest_item_count =
            i64::try_from(copied.rows_affected()).map_err(|_| BackupError::InvalidPersistedData)?;
        let content_reference_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
                 FROM backup_snapshot_nodes
                 WHERE snapshot_id = $1 AND current_version_id IS NOT NULL",
        )
        .bind(snapshot_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;

        // Retention pins use the existing GC lock order: all candidate rows,
        // then canonical Object locks, then pin insertion. This makes a
        // snapshot capture a normal durable-reference creator: it clears a
        // stale candidate while holding the same Object fence that planning,
        // physical GC, and metadata purge use. No pin can commit after GC has
        // made its Object non-referenceable, and no completed snapshot can
        // commit before every manifest content row has its pin.
        lock_backup_pin_candidate_rows(&mut transaction, snapshot_id).await?;
        let pin_objects = lock_backup_pin_objects(&mut transaction, snapshot_id).await?;
        if pin_objects
            .iter()
            .any(|lifecycle_state| lifecycle_state != "AVAILABLE")
        {
            return Err(BackupError::SnapshotConflict);
        }
        clear_backup_pin_candidates(&mut transaction, snapshot_id).await?;

        let inserted_pins = sqlx::query(
            "INSERT INTO backup_snapshot_content_pins
                (snapshot_id, manifest_node_id, file_version_id,
                 object_id, object_dedup_domain_id, created_at)
             SELECT manifest.snapshot_id, manifest.node_id,
                    version.id, version.object_id, version.object_dedup_domain_id,
                    CURRENT_TIMESTAMP
             FROM backup_snapshot_nodes AS manifest
             INNER JOIN file_versions AS version
               ON version.id = manifest.current_version_id
             WHERE manifest.snapshot_id = $1
               AND manifest.current_version_id IS NOT NULL
             ORDER BY manifest.node_id ASC
             ON CONFLICT (snapshot_id, manifest_node_id) DO NOTHING",
        )
        .bind(snapshot_id.into_uuid())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let inserted_pins =
            i64::try_from(inserted_pins).map_err(|_| BackupError::InvalidPersistedData)?;
        if inserted_pins != content_reference_count {
            return Err(BackupError::InvalidPersistedData);
        }

        let persisted_pin_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
             FROM backup_snapshot_content_pins
             WHERE snapshot_id = $1",
        )
        .bind(snapshot_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if persisted_pin_count != content_reference_count {
            return Err(BackupError::InvalidPersistedData);
        }
        let terminal_node_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT node_id
             FROM backup_snapshot_nodes
             WHERE snapshot_id = $1
             ORDER BY node_id DESC
             LIMIT 1",
        )
        .bind(snapshot_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?;

        let row = sqlx::query_as::<_, BackupSnapshotRow>(
            "UPDATE backup_snapshots
             SET manifest_item_count = $2, terminal_node_id = $3,
                 content_reference_count = $4, state = 'COMPLETED',
                 committed_at = CURRENT_TIMESTAMP
             WHERE id = $1
             RETURNING id, backup_set_id, owner_user_id, source_library_id, operation_id,
                       snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
                       terminal_node_id, content_reference_count, state, created_at,
                       committed_at, expired_at",
        )
        .bind(snapshot_id.into_uuid())
        .bind(manifest_item_count)
        .bind(terminal_node_id)
        .bind(content_reference_count)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;

        transaction.commit().await?;

        Ok(row.try_into_domain()?)
    }

    /// List owner backup sets with bounded keyset pagination by backup set ID.
    pub async fn list_backup_sets(
        &self,
        owner_user_id: UserId,
        after: Option<BackupSetId>,
        limit: u32,
    ) -> Result<(Vec<BackupSet>, bool), BackupError> {
        validate_page_limit_set(limit)?;
        let rows = sqlx::query_as::<_, BackupSetRow>(
            "SELECT id, owner_user_id, name, source_library_id, source,
                    retention_days, state, created_at, updated_at, revision::TEXT AS revision
             FROM backup_sets
             WHERE owner_user_id = $1
               AND ($2::UUID IS NULL OR id > $2)
             ORDER BY id ASC
             LIMIT $3",
        )
        .bind(owner_user_id.into_uuid())
        .bind(after.map(BackupSetId::into_uuid))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;

        let has_more = rows.len() > limit as usize;
        let sets = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupSetRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((sets, has_more))
    }

    /// Read one owner-scoped backup set. Missing and foreign sets are both
    /// concealed as `NotFound`.
    pub async fn get_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSet, BackupError> {
        sqlx::query_as::<_, BackupSetRow>(
            "SELECT id, owner_user_id, name, source_library_id, source,
                    retention_days, state, created_at, updated_at,
                    revision::TEXT AS revision
             FROM backup_sets
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)
        .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// List owner-scoped snapshots in deterministic newest-first chronological
    /// order. Committed/expired snapshots use `committed_at`; snapshots that
    /// have not committed yet use `created_at` as the stable fallback key.
    pub async fn list_backup_snapshots(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        state: Option<SnapshotState>,
        after: Option<BackupSnapshotPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshot>, bool), BackupError> {
        validate_page_limit_node(limit)?;
        self.get_backup_set(owner_user_id, backup_set_id).await?;

        let rows = sqlx::query_as::<_, BackupSnapshotRow>(
            "SELECT snapshot.id, snapshot.backup_set_id, snapshot.owner_user_id,
                    snapshot.source_library_id, snapshot.operation_id,
                    snapshot.snapshot_epoch, snapshot.snapshot_resume_sequence,
                    snapshot.manifest_item_count, snapshot.terminal_node_id,
                    snapshot.content_reference_count, snapshot.state,
                    snapshot.created_at, snapshot.committed_at, snapshot.expired_at
             FROM backup_snapshots AS snapshot
             WHERE snapshot.owner_user_id = $1
               AND snapshot.backup_set_id = $2
               AND (
                    $3::TIMESTAMPTZ IS NULL
                    OR COALESCE(snapshot.committed_at, snapshot.created_at) < $3
                    OR (
                        COALESCE(snapshot.committed_at, snapshot.created_at) = $3
                        AND snapshot.id < $4
                    )
               )
               AND ($5::TEXT IS NULL OR snapshot.state = $5)
             ORDER BY COALESCE(snapshot.committed_at, snapshot.created_at) DESC,
                      snapshot.id DESC
             LIMIT $6",
        )
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(after.map(|position| position.sort_at().as_offset_datetime()))
        .bind(after.map(|position| position.snapshot_id().into_uuid()))
        .bind(state.map(SnapshotState::as_str))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;

        let has_more = rows.len() > limit as usize;
        let snapshots = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupSnapshotRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((snapshots, has_more))
    }

    /// Read one owner-scoped snapshot summary. The immutable manifest remains
    /// readable for the full logical snapshot history, including `EXPIRED`
    /// snapshots whose retention pins have already been released by pruning.
    pub async fn get_backup_snapshot(
        &self,
        owner_user_id: UserId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupSnapshot, BackupError> {
        sqlx::query_as::<_, BackupSnapshotRow>(
            "SELECT snapshot.id, snapshot.backup_set_id, snapshot.owner_user_id,
                    snapshot.source_library_id, snapshot.operation_id,
                    snapshot.snapshot_epoch, snapshot.snapshot_resume_sequence,
                    snapshot.manifest_item_count, snapshot.terminal_node_id,
                    snapshot.content_reference_count, snapshot.state,
                    snapshot.created_at, snapshot.committed_at, snapshot.expired_at
             FROM backup_snapshots AS snapshot
             INNER JOIN backup_sets AS backup_set
                ON backup_set.id = snapshot.backup_set_id
               AND backup_set.owner_user_id = snapshot.owner_user_id
             WHERE snapshot.id = $1
               AND snapshot.owner_user_id = $2",
        )
        .bind(snapshot_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)
        .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// List immutable manifest nodes for one completed or expired snapshot.
    pub async fn list_snapshot_nodes(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        after: Option<NodeId>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotNode>, bool), BackupError> {
        validate_page_limit_node(limit)?;
        let row = sqlx::query_as::<_, BackupSnapshotRow>(
            "SELECT id, backup_set_id, owner_user_id, source_library_id, operation_id,
                    snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
                    terminal_node_id, content_reference_count, state, created_at,
                    committed_at, expired_at
             FROM backup_snapshots
             WHERE id = $1 AND owner_user_id = $2 AND backup_set_id = $3",
        )
        .bind(snapshot_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)?;
        let snapshot = row.try_into_domain()?;
        if !matches!(
            snapshot.state(),
            SnapshotState::Completed | SnapshotState::Expired
        ) {
            return Err(BackupError::InvalidState);
        }

        let rows = sqlx::query_as::<_, BackupSnapshotNodeRow>(
            "SELECT snapshot_id, node_id, parent_node_id, name, kind, state,
                    revision::TEXT AS revision, current_version_id,
                    content_length::TEXT AS content_length, content_sha256,
                    node_created_at, node_updated_at
             FROM backup_snapshot_nodes
             WHERE snapshot_id = $1
               AND ($2::UUID IS NULL OR node_id > $2)
             ORDER BY node_id ASC
             LIMIT $3",
        )
        .bind(snapshot_id.into_uuid())
        .bind(after.map(NodeId::into_uuid))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;

        let has_more = rows.len() > limit as usize;
        let nodes = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupSnapshotNodeRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((nodes, has_more))
    }

    /// List direct children of one immutable snapshot manifest parent. When
    /// `parent_node_id` is omitted, the single manifest root is returned. The
    /// service validates the parent belongs to this owner-scoped snapshot and
    /// is a directory before listing its children.
    pub async fn list_backup_snapshot_nodes(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        parent_node_id: Option<NodeId>,
        after: Option<NodeId>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotNode>, bool), BackupError> {
        validate_page_limit_node(limit)?;
        let snapshot = sqlx::query_as::<_, BackupSnapshotRow>(
            "SELECT snapshot.id, snapshot.backup_set_id, snapshot.owner_user_id,
                    snapshot.source_library_id, snapshot.operation_id,
                    snapshot.snapshot_epoch, snapshot.snapshot_resume_sequence,
                    snapshot.manifest_item_count, snapshot.terminal_node_id,
                    snapshot.content_reference_count, snapshot.state,
                    snapshot.created_at, snapshot.committed_at, snapshot.expired_at
             FROM backup_snapshots AS snapshot
             INNER JOIN backup_sets AS backup_set
                ON backup_set.id = snapshot.backup_set_id
               AND backup_set.owner_user_id = snapshot.owner_user_id
             WHERE snapshot.id = $1
               AND snapshot.owner_user_id = $2
               AND snapshot.backup_set_id = $3",
        )
        .bind(snapshot_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)?
        .try_into_domain()?;
        if !matches!(
            snapshot.state(),
            SnapshotState::Completed | SnapshotState::Expired
        ) {
            return Err(BackupError::InvalidState);
        }

        if let Some(parent_node_id) = parent_node_id {
            let parent_kind = sqlx::query_scalar::<_, String>(
                "SELECT kind
                 FROM backup_snapshot_nodes
                 WHERE snapshot_id = $1 AND node_id = $2",
            )
            .bind(snapshot_id.into_uuid())
            .bind(parent_node_id.into_uuid())
            .fetch_optional(self.pool.sqlx_pool())
            .await?
            .ok_or(BackupError::NotFound)?;
            if parent_kind != "DIRECTORY" {
                return Err(BackupError::InvalidState);
            }
        }

        let rows = sqlx::query_as::<_, BackupSnapshotNodeRow>(
            "SELECT snapshot_id, node_id, parent_node_id, name, kind, state,
                    revision::TEXT AS revision, current_version_id,
                    content_length::TEXT AS content_length, content_sha256,
                    node_created_at, node_updated_at
             FROM backup_snapshot_nodes
             WHERE snapshot_id = $1
               AND (
                    ($2::UUID IS NULL AND parent_node_id IS NULL)
                    OR parent_node_id = $2
               )
               AND ($3::UUID IS NULL OR node_id > $3)
             ORDER BY node_id ASC
             LIMIT $4",
        )
        .bind(snapshot_id.into_uuid())
        .bind(parent_node_id.map(NodeId::into_uuid))
        .bind(after.map(NodeId::into_uuid))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;

        let has_more = rows.len() > limit as usize;
        let nodes = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupSnapshotNodeRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((nodes, has_more))
    }

    /// List historical maintenance-run evidence for one owned backup set in
    /// deterministic newest-first creation order. This is observational only;
    /// it never recovers or advances a run.
    pub async fn list_backup_maintenance_runs(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        after: Option<BackupMaintenanceRunPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupMaintenanceRun>, bool), BackupError> {
        validate_page_limit_maintenance(limit)?;
        self.get_backup_set(owner_user_id, backup_set_id).await?;

        let rows = sqlx::query_as::<_, BackupMaintenanceRunRow>(
            "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                    policy_revision_number, operation_id, fingerprint_version,
                    request_fingerprint, capture_operation_id,
                    expiry_plan_operation_id, state, captured_snapshot_id,
                    expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                    expiry_planned_at, maintenance_completed_at, stale_at,
                    created_at
             FROM backup_maintenance_runs
             WHERE owner_user_id = $1
               AND backup_set_id = $2
               AND (
                    $3::TIMESTAMPTZ IS NULL
                    OR created_at < $3
                    OR (created_at = $3 AND id < $4)
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $5",
        )
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(after.map(|position| position.created_at().as_offset_datetime()))
        .bind(after.map(|position| position.run_id().into_uuid()))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;

        let has_more = rows.len() > limit as usize;
        let runs = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupMaintenanceRunRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((runs, has_more))
    }

    /// Append one immutable, owner-scoped retention-policy revision. The
    /// backup-set row serializes revision allocation with snapshot capture and
    /// other policy writers; policy configuration has no snapshot side effect.
    pub async fn configure_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        keep_latest_completed: u64,
        expire_after_seconds: u64,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        validate_operation_key(&operation_id)?;
        let config =
            BackupSnapshotRetentionPolicyConfig::new(keep_latest_completed, expire_after_seconds)
                .map_err(|_| BackupError::InvalidRequest)?;
        let request = BackupSnapshotRetentionPolicyRequest::new(backup_set_id, config);
        let fingerprint = request.fingerprint();
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        if let Some(existing) =
            load_retention_policy_by_operation(&mut transaction, owner_user_id, &operation_id)
                .await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::RetentionPolicyConflict);
            }
            return Ok(existing);
        }

        lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id).await?;

        // Recheck after the set fence: a concurrent replay may have committed
        // while this transaction waited.
        if let Some(existing) =
            load_retention_policy_by_operation(&mut transaction, owner_user_id, &operation_id)
                .await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::RetentionPolicyConflict);
            }
            return Ok(existing);
        }

        let latest_revision = sqlx::query_scalar::<_, i64>(
            "SELECT revision_number
             FROM backup_snapshot_retention_policy_revisions
             WHERE backup_set_id = $1
             ORDER BY revision_number DESC
             LIMIT 1",
        )
        .bind(backup_set_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?;
        let revision_number = match latest_revision {
            Some(value) => BackupSnapshotRetentionPolicyRevisionNumber::new(
                u64::try_from(value).map_err(|_| BackupError::InvalidPersistedData)?,
            )
            .map_err(|_| BackupError::InvalidPersistedData)?
            .checked_next()
            .map_err(|_| BackupError::InvalidPersistedData)?,
            None => BackupSnapshotRetentionPolicyRevisionNumber::new(1)
                .map_err(|_| BackupError::InternalError)?,
        };
        let revision_id = BackupSnapshotRetentionPolicyRevisionId::new();
        let inserted = sqlx::query_as::<_, BackupSnapshotRetentionPolicyRevisionRow>(
            "INSERT INTO backup_snapshot_retention_policy_revisions
                (id, owner_user_id, backup_set_id, revision_number, operation_id,
                 fingerprint_version, request_fingerprint, keep_latest_completed,
                 expire_after_seconds, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, clock_timestamp())
             ON CONFLICT (owner_user_id, operation_id) DO NOTHING
             RETURNING id, owner_user_id, backup_set_id, revision_number,
                       operation_id, fingerprint_version, request_fingerprint,
                       keep_latest_completed, expire_after_seconds, created_at",
        )
        .bind(revision_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(i64::try_from(revision_number.get()).map_err(|_| BackupError::InvalidRequest)?)
        .bind(&operation_id)
        .bind(i16::try_from(fingerprint.version()).map_err(|_| BackupError::InvalidRequest)?)
        .bind(fingerprint.as_bytes().as_slice())
        .bind(
            i64::try_from(config.keep_latest_completed())
                .map_err(|_| BackupError::InvalidRequest)?,
        )
        .bind(
            i64::try_from(config.expire_after_seconds())
                .map_err(|_| BackupError::InvalidRequest)?,
        )
        .fetch_optional(&mut *transaction)
        .await?;

        let row = if let Some(inserted) = inserted {
            inserted
        } else {
            load_retention_policy_by_operation(&mut transaction, owner_user_id, &operation_id)
                .await?
                .ok_or(BackupError::InvalidPersistedData)?
        };
        let revision = row.try_into_domain()?;
        if revision.request_fingerprint() != fingerprint {
            transaction.commit().await?;
            return Err(BackupError::RetentionPolicyConflict);
        }
        transaction.commit().await?;
        Ok(revision)
    }

    /// Return the highest committed policy revision for an owned backup set.
    pub async fn get_current_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        let visible = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM backup_sets
                WHERE id = $1 AND owner_user_id = $2
             )",
        )
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_one(self.pool.sqlx_pool())
        .await?;
        if !visible {
            return Err(BackupError::NotFound);
        }
        load_current_retention_policy(self.pool.sqlx_pool(), owner_user_id, backup_set_id)
            .await?
            .ok_or(BackupError::ExpiryPreflight(
                BackupSnapshotExpiryPreflightIssue::RetentionPolicyNotConfigured,
            ))?
            .try_into_domain()
            .map_err(Into::into)
    }

    /// Create one durable policy observation and decision entry per currently
    /// COMPLETED snapshot. This transaction is metadata-only and grants no
    /// expiry, pruning, pin-release, GC, or ObjectStore authority.
    pub async fn create_snapshot_expiry_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSnapshotExpiryPlan, BackupError> {
        self.create_snapshot_expiry_plan_with_expected_policy(
            owner_user_id,
            operation_id,
            backup_set_id,
            None,
        )
        .await
    }

    async fn create_snapshot_expiry_plan_with_expected_policy(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        expected_policy: Option<(
            BackupSnapshotRetentionPolicyRevisionId,
            BackupSnapshotRetentionPolicyRevisionNumber,
        )>,
    ) -> Result<BackupSnapshotExpiryPlan, BackupError> {
        validate_operation_key(&operation_id)?;
        let request = BackupSnapshotExpiryPlanRequest::new(backup_set_id);
        let request_fingerprint = request.fingerprint();
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        if let Some(existing) =
            load_expiry_plan_by_operation(&mut transaction, owner_user_id, &operation_id).await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != request_fingerprint {
                return Err(BackupError::ExpiryPlanConflict);
            }
            return Ok(existing);
        }

        // Lock order: owned backup set -> current policy -> completed cohort
        // -> active restore-plan rows. Capture and policy creation take the
        // same first fence; restore insertion's FK key lock serializes against
        // it, producing a coherent before/after race outcome.
        lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id).await?;

        if let Some(existing) =
            load_expiry_plan_by_operation(&mut transaction, owner_user_id, &operation_id).await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != request_fingerprint {
                return Err(BackupError::ExpiryPlanConflict);
            }
            return Ok(existing);
        }

        let policy_row = load_current_retention_policy_for_update(
            &mut transaction,
            owner_user_id,
            backup_set_id,
        )
        .await?
        .ok_or(BackupError::ExpiryPreflight(
            BackupSnapshotExpiryPreflightIssue::RetentionPolicyNotConfigured,
        ))?;
        let policy = policy_row.try_into_domain()?;
        if expected_policy.is_some_and(|(expected_id, expected_number)| {
            policy.id() != expected_id || policy.revision_number() != expected_number
        }) {
            transaction.commit().await?;
            return Err(BackupError::MaintenanceRunPreflight(
                BackupMaintenanceRunPreflightIssue::PolicyChanged,
            ));
        }

        let active_plan = sqlx::query_scalar::<_, Uuid>(
            "SELECT id
             FROM backup_snapshot_expiry_plans
             WHERE backup_set_id = $1 AND state = 'PLANNED'
             ORDER BY id ASC
             LIMIT 1
             FOR SHARE",
        )
        .bind(backup_set_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?;
        if active_plan.is_some() {
            transaction.commit().await?;
            return Err(BackupError::ExpiryPreflight(
                BackupSnapshotExpiryPreflightIssue::ExpiryAlreadyPlanned,
            ));
        }

        let evaluated_at = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await?;
        let evaluated_at = Timestamp::from_offset_datetime(evaluated_at);
        let cutoff_at = evaluated_at
            .checked_sub_std(policy.config().expire_after())
            .ok_or(BackupError::ExpiryPreflight(
                BackupSnapshotExpiryPreflightIssue::DurationUnrepresentable,
            ))?;
        let cohort =
            load_completed_expiry_cohort(&mut transaction, owner_user_id, backup_set_id).await?;
        let blockers =
            load_active_restore_blockers(&mut transaction, owner_user_id, backup_set_id).await?;
        let plan_id = BackupSnapshotExpiryPlanId::new();
        let observation = build_expiry_observation(
            plan_id,
            backup_set_id,
            policy.id(),
            evaluated_at,
            cutoff_at,
            policy.config(),
            &cohort,
            &blockers,
        )?;

        let inserted = sqlx::query_as::<_, BackupSnapshotExpiryPlanRow>(
            "INSERT INTO backup_snapshot_expiry_plans
                (id, owner_user_id, backup_set_id, policy_revision_id,
                 policy_revision_number, operation_id, fingerprint_version,
                 request_fingerprint, evaluated_at, cutoff_at,
                 snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                 evaluated_completed_snapshot_count, expire_candidate_count,
                 keep_latest_count, keep_recent_count, blocked_active_restore_count,
                 state, created_at, stale_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                     $13, $14, $15, $16, $17, 'ASSEMBLING', clock_timestamp(), NULL)
             ON CONFLICT (owner_user_id, operation_id) DO NOTHING
             RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                       policy_revision_number, operation_id, fingerprint_version,
                       request_fingerprint, evaluated_at, cutoff_at,
                       snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                       evaluated_completed_snapshot_count, expire_candidate_count,
                       keep_latest_count, keep_recent_count, blocked_active_restore_count,
                       state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(policy.id().into_uuid())
        .bind(
            i64::try_from(policy.revision_number().get())
                .map_err(|_| BackupError::InvalidPersistedData)?,
        )
        .bind(&operation_id)
        .bind(
            i16::try_from(request_fingerprint.version())
                .map_err(|_| BackupError::InvalidRequest)?,
        )
        .bind(request_fingerprint.as_bytes().as_slice())
        .bind(encode_timestamp(
            evaluated_at,
            "backup_snapshot_expiry_plans.evaluated_at",
        )?)
        .bind(encode_timestamp(
            cutoff_at,
            "backup_snapshot_expiry_plans.cutoff_at",
        )?)
        .bind(
            i16::try_from(observation.basis_fingerprint.version())
                .map_err(|_| BackupError::InvalidRequest)?,
        )
        .bind(observation.basis_fingerprint.as_bytes().as_slice())
        .bind(observation.counts.evaluated_completed_snapshot_count)
        .bind(observation.counts.expire_candidate_count)
        .bind(observation.counts.keep_latest_count)
        .bind(observation.counts.keep_recent_count)
        .bind(observation.counts.blocked_active_restore_count)
        .fetch_optional(&mut *transaction)
        .await?;

        if inserted.is_none() {
            let existing =
                load_expiry_plan_by_operation(&mut transaction, owner_user_id, &operation_id)
                    .await?
                    .ok_or(BackupError::InvalidPersistedData)?
                    .try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != request_fingerprint {
                return Err(BackupError::ExpiryPlanConflict);
            }
            return Ok(existing);
        }

        for entry in &observation.entries {
            sqlx::query(
                "INSERT INTO backup_snapshot_expiry_plan_entries
                    (plan_id, snapshot_id, committed_at, recency_rank, decision)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(entry.plan_id().into_uuid())
            .bind(entry.snapshot_id().into_uuid())
            .bind(encode_timestamp(
                entry.committed_at(),
                "backup_snapshot_expiry_plan_entries.committed_at",
            )?)
            .bind(i64::try_from(entry.recency_rank()).map_err(|_| BackupError::InvalidRequest)?)
            .bind(entry.decision().as_str())
            .execute(&mut *transaction)
            .await?;
        }
        verify_expiry_plan_entry_counts(&mut transaction, plan_id, observation.counts).await?;

        let finalized = sqlx::query_as::<_, BackupSnapshotExpiryPlanRow>(
            "UPDATE backup_snapshot_expiry_plans
             SET state = 'PLANNED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'ASSEMBLING'
             RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                       policy_revision_number, operation_id, fingerprint_version,
                       request_fingerprint, evaluated_at, cutoff_at,
                       snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                       evaluated_completed_snapshot_count, expire_candidate_count,
                       keep_latest_count, keep_recent_count, blocked_active_restore_count,
                       state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        let plan = finalized.try_into_domain()?;
        transaction.commit().await?;
        Ok(plan)
    }

    /// Read one owner-scoped expiry plan without re-evaluating it.
    pub async fn get_snapshot_expiry_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupSnapshotExpiryPlanId,
    ) -> Result<BackupSnapshotExpiryPlan, BackupError> {
        load_expiry_plan(self.pool.sqlx_pool(), owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?
            .try_into_domain()
            .map_err(Into::into)
    }

    /// List immutable decisions by recency-rank keyset.
    pub async fn list_snapshot_expiry_plan_entries(
        &self,
        owner_user_id: UserId,
        plan_id: BackupSnapshotExpiryPlanId,
        after: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotExpiryPlanEntry>, bool), BackupError> {
        validate_expiry_plan_entry_limit(limit)?;
        let _ = load_expiry_plan(self.pool.sqlx_pool(), owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?
            .try_into_domain()?;
        let after = after
            .map(|value| i64::try_from(value).map_err(|_| BackupError::InvalidRequest))
            .transpose()?;
        let rows = sqlx::query_as::<_, BackupSnapshotExpiryPlanEntryRow>(
            "SELECT plan_id, snapshot_id, committed_at, recency_rank, decision
             FROM backup_snapshot_expiry_plan_entries
             WHERE plan_id = $1
               AND ($2::BIGINT IS NULL OR recency_rank > $2)
             ORDER BY recency_rank ASC
             LIMIT $3",
        )
        .bind(plan_id.into_uuid())
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;
        let has_more = rows.len() > limit as usize;
        let entries = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupSnapshotExpiryPlanEntryRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, has_more))
    }

    /// Recompute the immutable policy decision basis with the original
    /// `evaluated_at`. Current wall-clock passage alone is deliberately absent
    /// from validation. Any policy/cohort/blocker/evidence drift only marks the
    /// plan STALE; entries and provenance are never rewritten.
    pub async fn validate_snapshot_expiry_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupSnapshotExpiryPlanId,
    ) -> Result<BackupSnapshotExpiryPlan, BackupError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let initial =
            load_expiry_plan_in_transaction(&mut transaction, owner_user_id, plan_id, false)
                .await?
                .ok_or(BackupError::NotFound)?;
        let initial_set_id = decode_id(
            initial.backup_set_id,
            "backup_snapshot_expiry_plans.backup_set_id",
        )?;
        lock_owned_backup_set(&mut transaction, owner_user_id, initial_set_id).await?;
        let current =
            load_expiry_plan_in_transaction(&mut transaction, owner_user_id, plan_id, true)
                .await?
                .ok_or(BackupError::NotFound)?;
        let plan = current.try_into_domain()?;
        if plan.state() == BackupSnapshotExpiryPlanState::Stale {
            transaction.commit().await?;
            return Ok(plan);
        }

        let current_policy = load_current_retention_policy_for_update(
            &mut transaction,
            owner_user_id,
            plan.backup_set_id(),
        )
        .await?;
        let Some(policy_row) = current_policy else {
            let stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Ok(stale);
        };
        let policy = policy_row.try_into_domain()?;
        if policy.id() != plan.policy_revision_id()
            || policy.revision_number() != plan.policy_revision_number()
        {
            let stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Ok(stale);
        }

        let expected_cutoff = plan
            .evaluated_at()
            .checked_sub_std(policy.config().expire_after());
        let cohort =
            load_completed_expiry_cohort(&mut transaction, owner_user_id, plan.backup_set_id())
                .await?;
        let blockers =
            load_active_restore_blockers(&mut transaction, owner_user_id, plan.backup_set_id())
                .await?;
        let observation = expected_cutoff
            .filter(|cutoff| *cutoff == plan.cutoff_at())
            .map(|cutoff| {
                build_expiry_observation(
                    plan.id(),
                    plan.backup_set_id(),
                    policy.id(),
                    plan.evaluated_at(),
                    cutoff,
                    policy.config(),
                    &cohort,
                    &blockers,
                )
            })
            .transpose()?;
        let stored_entries = load_all_expiry_plan_entries(&mut transaction, plan_id).await;
        let evidence_matches = observation.as_ref().is_some_and(|observation| {
            plan.snapshot_basis_fingerprint() == observation.basis_fingerprint
                && observation.counts.matches_plan(&plan)
                && stored_entries
                    .as_ref()
                    .is_ok_and(|entries| entries == &observation.entries)
        });
        if evidence_matches {
            transaction.commit().await?;
            return Ok(plan);
        }

        let stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
        transaction.commit().await?;
        Ok(stale)
    }

    /// Execute one valid PLANNED snapshot-expiry plan atomically, transitioning
    /// every EXPIRE decision entry's snapshot from `COMPLETED` to `EXPIRED`.
    /// The plan must still be PLANNED, its immutable Prompt 47 planning basis
    /// must exactly match current authoritative state, and no entry's snapshot
    /// may have an active PLANNED restore plan. The entire plan basis is
    /// revalidated inside a single transactional commit; if any invariant
    /// fails the transaction rolls back leaving zero snapshot mutations and zero
    /// execution evidence. Returns the canonical immutable execution receipt.
    pub async fn execute_snapshot_expiry_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupSnapshotExpiryPlanId,
    ) -> Result<BackupSnapshotExpiryExecution, BackupError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        // Owner-concealment read: a foreign or missing plan is NotFound and
        // reveals nothing about another owner's expiry decisions.
        let initial =
            load_expiry_plan_in_transaction(&mut transaction, owner_user_id, plan_id, false)
                .await?
                .ok_or(BackupError::NotFound)?;
        let initial_set_id = decode_id(
            initial.backup_set_id,
            "backup_snapshot_expiry_plans.backup_set_id",
        )?;

        // The plan row is locked first, serializing duplicate concurrent
        // executions of the same plan onto one authoritative lock owner.
        lock_owned_backup_set(&mut transaction, owner_user_id, initial_set_id).await?;

        let plan_row =
            load_expiry_plan_in_transaction(&mut transaction, owner_user_id, plan_id, true)
                .await?
                .ok_or(BackupError::NotFound)?;
        let plan = plan_row.try_into_domain()?;

        // Terminal/replay fast path must resolve before any revalidation of
        // the immutable planning basis. Returns the committed receipt if the
        // plan is already EXECUTED.
        match plan.state() {
            BackupSnapshotExpiryPlanState::Executed => {
                let receipt =
                    load_expiry_execution_for_plan(&mut transaction, owner_user_id, plan.id())
                        .await?
                        .ok_or(BackupError::InvalidPersistedData)?
                        .try_into_domain()?;
                transaction.commit().await?;
                return Ok(receipt);
            }
            BackupSnapshotExpiryPlanState::Stale => {
                transaction.commit().await?;
                return Err(BackupError::ExpiryExecutionPreflight(
                    BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
                ));
            }
            BackupSnapshotExpiryPlanState::Planned => {}
        }

        // Current policy revision revalidation: the snapshot retention policy
        // revision must still be the exact revision the plan was authored against.
        let current_policy = load_current_retention_policy_for_update(
            &mut transaction,
            owner_user_id,
            plan.backup_set_id(),
        )
        .await?;
        let Some(policy_row) = current_policy else {
            let _stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::ExpiryExecutionPreflight(
                BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
            ));
        };
        let policy = policy_row.try_into_domain()?;
        if policy.id() != plan.policy_revision_id()
            || policy.revision_number() != plan.policy_revision_number()
        {
            let _stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::ExpiryExecutionPreflight(
                BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
            ));
        }

        // Cohort and blocker revalidation using the ORIGINAL evaluated_at.
        // Wall-clock passage alone is deliberately absent from staleness.
        let expected_cutoff = plan
            .evaluated_at()
            .checked_sub_std(policy.config().expire_after());
        let cohort =
            load_completed_expiry_cohort(&mut transaction, owner_user_id, plan.backup_set_id())
                .await?;
        let blockers =
            load_active_restore_blockers(&mut transaction, owner_user_id, plan.backup_set_id())
                .await?;
        let observation = expected_cutoff
            .filter(|cutoff| *cutoff == plan.cutoff_at())
            .map(|cutoff| {
                build_expiry_observation(
                    plan.id(),
                    plan.backup_set_id(),
                    policy.id(),
                    plan.evaluated_at(),
                    cutoff,
                    policy.config(),
                    &cohort,
                    &blockers,
                )
            })
            .transpose()?;
        let stored_entries = load_all_expiry_plan_entries(&mut transaction, plan_id).await;
        let evidence_matches = observation.as_ref().is_some_and(|observation| {
            plan.snapshot_basis_fingerprint() == observation.basis_fingerprint
                && observation.counts.matches_plan(&plan)
                && stored_entries
                    .as_ref()
                    .is_ok_and(|entries| entries == &observation.entries)
        });
        if !evidence_matches {
            let _stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::ExpiryExecutionPreflight(
                BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
            ));
        }

        // Validate every EXPIRE snapshot is still COMPLETED immediately before
        // mutation. The ordered SELECT FOR UPDATE of the relevant snapshots in
        // deterministic plan-entry order establishes the consistent lock
        // ordering against concurrent restore-plan creation and snapshot
        // completion, guaranteeing exactly one outcome for each race.
        let entries = observation
            .as_ref()
            .expect("observation must be present after evidence_matches")
            .entries
            .clone();
        let expire_snapshot_ids: Vec<SnapshotId> = entries
            .iter()
            .filter(|entry| entry.decision() == BackupSnapshotExpiryDecision::Expire)
            .map(|entry| entry.snapshot_id())
            .collect();
        let expire_snapshot_row_ids = verify_snapshots_still_completed_for_expiry(
            &mut transaction,
            owner_user_id,
            plan.backup_set_id(),
            &expire_snapshot_ids,
        )
        .await?;
        if expire_snapshot_row_ids.len() != expire_snapshot_ids.len() {
            // At least one snapshot no longer COMPLETED under owner/set scope.
            // This may be legitimate lifecycle drift, but the immutable plan
            // basis changed, so mark it STALE rather than partially expiring.
            let _stale = stale_expiry_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::ExpiryExecutionPreflight(
                BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
            ));
        }

        // Authorized expiry phase. Everything below commits or rolls back as
        // one atomic unit.
        let executed_at = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .map(Timestamp::from_offset_datetime)?;
        let execution_id = BackupSnapshotExpiryExecutionId::new();
        let evaluated_snapshot_count = i64::try_from(plan.evaluated_completed_snapshot_count())
            .map_err(|_| {
                BackupError::ExpiryExecutionPreflight(
                    BackupSnapshotExpiryExecutionPreflightIssue::PlanCorruption,
                )
            })?;
        let expire_count = i64::try_from(plan.expire_candidate_count()).map_err(|_| {
            BackupError::ExpiryExecutionPreflight(
                BackupSnapshotExpiryExecutionPreflightIssue::PlanCorruption,
            )
        })?;
        let unchanged_count = {
            let total = &evaluated_snapshot_count;
            let expire = &expire_count;
            total
                .checked_sub(*expire)
                .ok_or(BackupError::ExpiryExecutionPreflight(
                    BackupSnapshotExpiryExecutionPreflightIssue::PlanCorruption,
                ))?
        };

        // 1. Persist the transaction-local ASSEMBLING execution receipt. The
        //    identity trigger binds every count to the immutable plan basis.
        sqlx::query(
            "INSERT INTO backup_snapshot_expiry_executions
                (id, owner_user_id, expiry_plan_id, backup_set_id,
                 policy_revision_id, evaluated_at, evaluated_snapshot_count,
                 expired_snapshot_count, unchanged_snapshot_count,
                 state, executed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'ASSEMBLING', $10)",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(plan.id().into_uuid())
        .bind(plan.backup_set_id().into_uuid())
        .bind(plan.policy_revision_id().into_uuid())
        .bind(encode_timestamp(
            plan.evaluated_at(),
            "backup_snapshot_expiry_executions.evaluated_at",
        )?)
        .bind(evaluated_snapshot_count)
        .bind(expire_count)
        .bind(unchanged_count)
        .bind(encode_timestamp(
            executed_at,
            "backup_snapshot_expiry_executions.executed_at",
        )?)
        .execute(&mut *transaction)
        .await?;

        // 2. Insert immutable per-entry execution evidence. Each evidence row
        //    is bound by a trigger to the exact plan entry.
        for entry in &entries {
            let transitioned = entry.decision() == BackupSnapshotExpiryDecision::Expire;
            sqlx::query(
                "INSERT INTO backup_snapshot_expiry_execution_entries
                    (execution_id, expiry_plan_id, snapshot_id,
                     original_decision, transitioned)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(execution_id.into_uuid())
            .bind(entry.plan_id().into_uuid())
            .bind(entry.snapshot_id().into_uuid())
            .bind(entry.decision().as_str())
            .bind(transitioned)
            .execute(&mut *transaction)
            .await?;
        }

        // 3. Transition every EXPIRE snapshot through the narrow authorized
        //    function. The function validates the exact plan entry, current
        //    snapshot lifecycle, and owner scope before touching the row.
        let mut expired_count = 0_i64;
        for &snapshot_id in &expire_snapshot_row_ids {
            let affected = sqlx::query_scalar::<_, i64>(
                "SELECT synveil_authorized_snapshot_expiry_transition($1, $2)",
            )
            .bind(plan.id().into_uuid())
            .bind(snapshot_id)
            .fetch_one(&mut *transaction)
            .await?;
            if affected != 1 {
                return Err(BackupError::InvalidPersistedData);
            }
            expired_count += 1;
        }
        if expired_count != expire_count {
            return Err(BackupError::InvalidPersistedData);
        }

        // 4. Verify the evidence entry count before sealing.
        let evidence_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_snapshot_expiry_execution_entries
             WHERE execution_id = $1",
        )
        .bind(execution_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        let total_entry_count =
            i64::try_from(plan.evaluated_completed_snapshot_count()).map_err(|_| {
                BackupError::ExpiryExecutionPreflight(
                    BackupSnapshotExpiryExecutionPreflightIssue::PlanCorruption,
                )
            })?;
        if evidence_count != total_entry_count {
            return Err(BackupError::InvalidPersistedData);
        }

        // 5. Seal the receipt and transition the plan. Both are deferred-checked
        //    at commit: no EXECUTED plan may commit without its COMMITTED
        //    receipt and no ASSEMBLING execution may commit.
        sqlx::query(
            "UPDATE backup_snapshot_expiry_executions
             SET state = 'COMMITTED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'ASSEMBLING'",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "UPDATE backup_snapshot_expiry_plans
             SET state = 'EXECUTED', stale_at = NULL
             WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'",
        )
        .bind(plan.id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .execute(&mut *transaction)
        .await?;

        let receipt = load_expiry_execution_for_plan(&mut transaction, owner_user_id, plan.id())
            .await?
            .ok_or(BackupError::InvalidPersistedData)?
            .try_into_domain()?;
        transaction.commit().await?;
        Ok(receipt)
    }

    /// Read one owner-scoped committed expiry execution receipt. Inaccessible
    /// executions are intentionally indistinguishable from missing executions.
    pub async fn get_snapshot_expiry_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupSnapshotExpiryExecutionId,
    ) -> Result<BackupSnapshotExpiryExecution, BackupError> {
        sqlx::query_as::<_, BackupSnapshotExpiryExecutionRow>(
            "SELECT execution.id, execution.owner_user_id, execution.expiry_plan_id,
                    execution.backup_set_id, execution.policy_revision_id,
                    execution.evaluated_at, execution.evaluated_snapshot_count,
                    execution.expired_snapshot_count, execution.unchanged_snapshot_count,
                    execution.state, execution.executed_at
             FROM backup_snapshot_expiry_executions AS execution
             WHERE execution.id = $1
               AND execution.owner_user_id = $2
               AND execution.state = 'COMMITTED'",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)
        .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// List immutable expiry execution entries for one committed receipt.
    pub async fn list_snapshot_expiry_execution_entries(
        &self,
        owner_user_id: UserId,
        execution_id: BackupSnapshotExpiryExecutionId,
        after: Option<SnapshotId>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotExpiryExecutionEntry>, bool), BackupError> {
        validate_expiry_plan_entry_limit(limit)?;
        // Verify the execution receipt exists and is committed under owner scope.
        let _ = sqlx::query_as::<_, BackupSnapshotExpiryExecutionRow>(
            "SELECT id, owner_user_id, expiry_plan_id, backup_set_id,
                    policy_revision_id, evaluated_at, evaluated_snapshot_count,
                    expired_snapshot_count, unchanged_snapshot_count,
                    state, executed_at
             FROM backup_snapshot_expiry_executions
             WHERE id = $1 AND owner_user_id = $2 AND state = 'COMMITTED'",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)?;
        let after_uuid = after.map(SnapshotId::into_uuid);
        let rows = sqlx::query_as::<_, BackupSnapshotExpiryExecutionEntryRow>(
            "SELECT execution_id, expiry_plan_id, snapshot_id,
                    original_decision, transitioned
             FROM backup_snapshot_expiry_execution_entries
             WHERE execution_id = $1
               AND ($2::UUID IS NULL OR snapshot_id > $2)
             ORDER BY snapshot_id ASC
             LIMIT $3",
        )
        .bind(execution_id.into_uuid())
        .bind(after_uuid)
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;
        let has_more = rows.len() > limit as usize;
        let entries = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupSnapshotExpiryExecutionEntryRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, has_more))
    }

    /// Create one durable, non-destructive restore plan. The target namespace
    /// guard and library clock row are held only for this metadata transaction;
    /// no live Node/FileVersion row, journal row, Object row, replica, pin, or
    /// ObjectStore is written or read.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_restore_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        target_library_id: LibraryId,
        target_parent_node_id: NodeId,
        destination_name: LogicalName,
    ) -> Result<BackupRestorePlan, BackupError> {
        validate_operation_key(&operation_id)?;
        let request = BackupRestorePlanRequest::new(
            backup_set_id,
            snapshot_id,
            target_library_id,
            target_parent_node_id,
            destination_name,
        );
        let fingerprint = request.fingerprint();
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        // A committed operation identity is the canonical replay result. Do
        // this lookup before taking a target-library lock so replaying a plan
        // after target drift remains idempotent and cannot create a new plan.
        if let Some(existing) =
            load_restore_plan_by_operation(&mut transaction, owner_user_id, &operation_id, false)
                .await?
        {
            let existing = existing.try_into_domain()?;
            if existing.request_fingerprint() != fingerprint {
                transaction.commit().await?;
                return Err(BackupError::RestorePlanConflict);
            }
            transaction.commit().await?;
            return Ok(existing);
        }

        // Planning uses the same cooperative namespace fence as metadata
        // mutations and snapshot capture. It is acquired before target Node
        // reads and before the target library clock row is locked.
        acquire_namespace_guard(&mut transaction, target_library_id).await?;
        let target = load_restore_target(
            &mut transaction,
            owner_user_id,
            target_library_id,
            target_parent_node_id,
            request.destination_name(),
        )
        .await?;
        let Some(target) = target else {
            return Err(BackupError::NotFound);
        };
        if !target.parent_is_valid {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::InvalidTargetParent,
            ));
        }
        if target.destination_exists {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::DestinationNameConflict,
            ));
        }
        if target.journal_epoch <= 0 || target.sync_head < 0 {
            return Err(BackupError::InvalidPersistedData);
        }

        let snapshot_row = sqlx::query_as::<_, BackupSnapshotRow>(
            "SELECT snapshot.id, snapshot.backup_set_id, snapshot.owner_user_id,
                    snapshot.source_library_id, snapshot.operation_id,
                    snapshot.snapshot_epoch, snapshot.snapshot_resume_sequence,
                    snapshot.manifest_item_count, snapshot.terminal_node_id,
                    snapshot.content_reference_count, snapshot.state,
                    snapshot.created_at, snapshot.committed_at, snapshot.expired_at
             FROM backup_snapshots AS snapshot
             INNER JOIN backup_sets AS backup_set
                ON backup_set.id = snapshot.backup_set_id
               AND backup_set.owner_user_id = snapshot.owner_user_id
             WHERE snapshot.id = $1
               AND snapshot.backup_set_id = $2
               AND snapshot.owner_user_id = $3
               AND backup_set.owner_user_id = $3
             FOR SHARE OF snapshot",
        )
        .bind(snapshot_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;
        let snapshot = snapshot_row.try_into_domain()?;
        let source_library_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT source_library_id
             FROM backup_sets
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if source_library_id != snapshot.source_library_id().into_uuid() {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        }
        if !snapshot.is_restorable() {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotNotRestorable,
            ));
        }

        let manifest_rows = load_all_snapshot_nodes(&mut transaction, snapshot_id).await?;
        let manifest_nodes = manifest_rows
            .into_iter()
            .map(BackupSnapshotNodeRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt)
            })?;
        validate_snapshot_tree(&snapshot, &manifest_nodes)?;
        validate_retained_content(&mut transaction, &snapshot, &manifest_nodes).await?;

        let entries =
            build_restore_plan_entries(&request, BackupRestorePlanId::new(), &manifest_nodes)?;
        let item_count = i64::try_from(entries.len()).map_err(|_| BackupError::InvalidRequest)?;
        let content_item_count = i64::try_from(
            entries
                .iter()
                .filter(|entry| entry.content().is_some())
                .count(),
        )
        .map_err(|_| BackupError::InvalidRequest)?;
        let plan_id = entries
            .first()
            .map(BackupRestorePlanEntry::plan_id)
            .ok_or(BackupError::InvalidPersistedData)?;

        let inserted_plan = sqlx::query_as::<_, BackupRestorePlanRow>(
            "INSERT INTO backup_restore_plans
                (id, owner_user_id, backup_set_id, snapshot_id,
                 target_library_id, target_parent_node_id, operation_id,
                 fingerprint_version, request_fingerprint, destination_name,
                 base_journal_epoch, base_journal_head, item_count,
                 content_item_count, state, created_at, stale_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                     $11, $12, $13, $14, 'ASSEMBLING', CURRENT_TIMESTAMP, NULL)
             ON CONFLICT (owner_user_id, operation_id) DO NOTHING
             RETURNING id, owner_user_id, backup_set_id, snapshot_id,
                       target_library_id, target_parent_node_id, operation_id,
                       fingerprint_version, request_fingerprint, destination_name,
                       base_journal_epoch, base_journal_head, item_count,
                       content_item_count, state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(snapshot_id.into_uuid())
        .bind(target_library_id.into_uuid())
        .bind(target_parent_node_id.into_uuid())
        .bind(&operation_id)
        .bind(i16::try_from(fingerprint.version()).map_err(|_| BackupError::InvalidRequest)?)
        .bind(fingerprint.sha256().as_slice())
        .bind(request.destination_name().as_str())
        .bind(target.journal_epoch)
        .bind(target.sync_head)
        .bind(item_count)
        .bind(content_item_count)
        .fetch_optional(&mut *transaction)
        .await?;

        let Some(_inserted_plan) = inserted_plan else {
            let existing = load_restore_plan_by_operation(
                &mut transaction,
                owner_user_id,
                &operation_id,
                true,
            )
            .await?
            .ok_or(BackupError::InvalidPersistedData)?
            .try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::RestorePlanConflict);
            }
            return Ok(existing);
        };

        for entry in &entries {
            let content = entry.content();
            let content_sha256 = content.map(|value| value.sha256().into_bytes().to_vec());
            sqlx::query(
                "INSERT INTO backup_restore_plan_entries
                    (plan_id, ordinal, planned_node_id, planned_parent_node_id,
                     source_snapshot_node_id, source_parent_node_id, source_state,
                     action, kind, name, source_revision, file_version_id,
                     content_length, content_sha256)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::NUMERIC,
                         $12, $13::NUMERIC, $14)",
            )
            .bind(entry.plan_id().into_uuid())
            .bind(i64::try_from(entry.ordinal()).map_err(|_| BackupError::InvalidRequest)?)
            .bind(entry.planned_node_id().into_uuid())
            .bind(entry.planned_parent_node_id().into_uuid())
            .bind(entry.source_snapshot_node_id().map(NodeId::into_uuid))
            .bind(entry.source_parent_node_id().map(NodeId::into_uuid))
            .bind(entry.source_state().as_str())
            .bind(entry.action().as_str())
            .bind(entry.kind().as_str())
            .bind(entry.name().as_str())
            .bind(entry.source_revision().map(|value| value.get().to_string()))
            .bind(content.map(|value| value.file_version_id().into_uuid()))
            .bind(content.map(|value| value.byte_length().to_string()))
            .bind(content_sha256)
            .execute(&mut *transaction)
            .await?;
        }

        let persisted_counts = sqlx::query_as::<_, (i64, i64)>(
            "SELECT count(*)::BIGINT,
                    count(*) FILTER (WHERE file_version_id IS NOT NULL)::BIGINT
             FROM backup_restore_plan_entries
             WHERE plan_id = $1",
        )
        .bind(plan_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if persisted_counts.0 != item_count || persisted_counts.1 != content_item_count {
            return Err(BackupError::InvalidPersistedData);
        }

        let finalized_plan = sqlx::query_as::<_, BackupRestorePlanRow>(
            "UPDATE backup_restore_plans
             SET state = 'PLANNED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'ASSEMBLING'
             RETURNING id, owner_user_id, backup_set_id, snapshot_id,
                       target_library_id, target_parent_node_id, operation_id,
                       fingerprint_version, request_fingerprint, destination_name,
                       base_journal_epoch, base_journal_head, item_count,
                       content_item_count, state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        let plan = finalized_plan.try_into_domain()?;
        transaction.commit().await?;
        Ok(plan)
    }

    /// Read one owner-scoped restore plan without revalidating or mutating its
    /// state. Staleness is an explicit operation of `validate_restore_plan`.
    pub async fn get_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestorePlan, BackupError> {
        load_restore_plan(self.pool.sqlx_pool(), owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)
            .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// List immutable plan entries by ordinal keyset. `after` is the last
    /// ordinal already delivered and is deliberately not an offset.
    pub async fn list_restore_plan_entries(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
        after: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<BackupRestorePlanEntry>, bool), BackupError> {
        validate_restore_plan_entry_limit(limit)?;
        let visible = load_restore_plan(self.pool.sqlx_pool(), owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let _ = visible.try_into_domain()?;
        let after = after
            .map(|value| i64::try_from(value).map_err(|_| BackupError::InvalidRequest))
            .transpose()?;
        let rows = sqlx::query_as::<_, BackupRestorePlanEntryRow>(
            "SELECT plan_id, ordinal, planned_node_id, planned_parent_node_id,
                    source_snapshot_node_id, source_parent_node_id, source_state,
                    action, kind, name, source_revision::TEXT AS source_revision,
                    file_version_id, content_length::TEXT AS content_length,
                    content_sha256
             FROM backup_restore_plan_entries
             WHERE plan_id = $1
               AND ($2::BIGINT IS NULL OR ordinal > $2)
             ORDER BY ordinal ASC
             LIMIT $3",
        )
        .bind(plan_id.into_uuid())
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;
        let has_more = rows.len() > limit as usize;
        let entries = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupRestorePlanEntryRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, has_more))
    }

    /// Execute one persisted PLANNED restore plan as one authoritative
    /// metadata transaction. The plan row is the semantic idempotency fence;
    /// an already EXECUTED plan returns its immutable receipt without touching
    /// the target namespace, Object metadata, or journal.
    pub async fn execute_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        // This initial owner-scoped read is only used to choose the namespace
        // guard. It intentionally reveals nothing for a foreign plan.
        let target_library_uuid = sqlx::query_scalar::<_, Uuid>(
            "SELECT target_library_id
             FROM backup_restore_plans
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;
        let target_library_id = decode_id(
            target_library_uuid,
            "backup_restore_plans.target_library_id",
        )?;

        acquire_namespace_guard(&mut transaction, target_library_id).await?;
        let plan_row = load_restore_plan_for_update(&mut transaction, owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let plan = plan_row.try_into_domain()?;

        match plan.state() {
            BackupRestorePlanState::Executed => {
                let receipt =
                    load_restore_execution_for_plan(&mut transaction, owner_user_id, plan.id())
                        .await?
                        .ok_or(BackupError::InvalidPersistedData)?
                        .try_into_domain()?;
                transaction.commit().await?;
                return Ok(receipt);
            }
            BackupRestorePlanState::Stale => {
                transaction.commit().await?;
                return Err(BackupError::RestorePreflight(
                    BackupRestorePreflightIssue::TargetChanged,
                ));
            }
            BackupRestorePlanState::Planned => {}
        }

        let target = load_execution_target(
            &mut transaction,
            owner_user_id,
            plan.target_library_id(),
            plan.target_parent_node_id(),
            plan.destination_name(),
        )
        .await?;
        let Some(target) = target else {
            mark_restore_plan_stale(&mut transaction, owner_user_id, plan.id()).await?;
            transaction.commit().await?;
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::InvalidTargetParent,
            ));
        };
        if !target.parent_is_valid {
            mark_restore_plan_stale(&mut transaction, owner_user_id, plan.id()).await?;
            transaction.commit().await?;
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::InvalidTargetParent,
            ));
        }
        if target.journal_epoch
            != i64::try_from(plan.base_journal_epoch().get()).unwrap_or(i64::MIN)
            || target.sync_head != i64::try_from(plan.base_journal_head().get()).unwrap_or(i64::MIN)
        {
            mark_restore_plan_stale(&mut transaction, owner_user_id, plan.id()).await?;
            transaction.commit().await?;
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::TargetChanged,
            ));
        }
        if target.destination_exists {
            mark_restore_plan_stale(&mut transaction, owner_user_id, plan.id()).await?;
            transaction.commit().await?;
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::DestinationNameConflict,
            ));
        }

        let plan_entries = load_all_restore_plan_entries(&mut transaction, plan.id()).await?;
        let plan_entries = plan_entries
            .into_iter()
            .map(BackupRestorePlanEntryRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt)
            })?;

        let snapshot_row = load_restore_snapshot(
            &mut transaction,
            owner_user_id,
            plan.backup_set_id(),
            plan.snapshot_id(),
        )
        .await?
        .ok_or(BackupError::NotFound)?;
        let snapshot = snapshot_row.try_into_domain()?;
        let source_library_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT source_library_id
             FROM backup_sets
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(plan.backup_set_id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;
        if source_library_id != snapshot.source_library_id().into_uuid() {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        }
        if !snapshot.is_restorable() {
            if snapshot.state() == SnapshotState::Expired {
                mark_restore_plan_stale(&mut transaction, owner_user_id, plan.id()).await?;
                transaction.commit().await?;
            }
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotNotRestorable,
            ));
        }

        let manifest_rows = load_all_snapshot_nodes(&mut transaction, snapshot.id()).await?;
        let manifest_nodes = manifest_rows
            .into_iter()
            .map(BackupSnapshotNodeRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt)
            })?;
        validate_snapshot_tree(&snapshot, &manifest_nodes)?;
        validate_restore_plan_structure(&plan, &plan_entries, &manifest_nodes)?;
        validate_planned_destination_ids_free(&mut transaction, &plan_entries).await?;
        let retained =
            load_retained_restore_content(&mut transaction, &snapshot, &manifest_nodes).await?;
        validate_restore_object_compatibility(
            &mut transaction,
            &target.library,
            &plan_entries,
            &retained,
        )
        .await?;

        let executed_at = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .map(Timestamp::from_offset_datetime)?;
        let event_count =
            i64::try_from(plan_entries.len()).map_err(|_| BackupError::InvalidPersistedData)?;
        let base_head = i64::try_from(plan.base_journal_head().get())
            .map_err(|_| BackupError::InvalidPersistedData)?;
        let journal_last_sequence = base_head
            .checked_add(event_count)
            .ok_or(BackupError::InvalidPersistedData)?;
        let journal_first_sequence = base_head
            .checked_add(1)
            .ok_or(BackupError::InvalidPersistedData)?;
        if journal_last_sequence <= 0 {
            return Err(BackupError::InvalidPersistedData);
        }

        let execution_id = BackupRestoreExecutionId::new();
        sqlx::query(
            "INSERT INTO backup_restore_executions
                (id, owner_user_id, restore_plan_id, target_library_id,
                 journal_first_sequence, journal_last_sequence,
                 created_node_count, created_file_version_count, state, executed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'ASSEMBLING', $9)",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(plan.id().into_uuid())
        .bind(target.library.id().into_uuid())
        .bind(journal_first_sequence)
        .bind(journal_last_sequence)
        .bind(event_count)
        .bind(
            i64::try_from(plan.content_item_count())
                .map_err(|_| BackupError::InvalidPersistedData)?,
        )
        .bind(executed_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?;

        let mut created_nodes = HashMap::with_capacity(plan_entries.len() + 1);
        created_nodes.insert(target.parent.id(), target.parent.clone());
        let mut journal_changes = Vec::with_capacity(plan_entries.len());
        let mut mutations = Vec::with_capacity(plan_entries.len());

        for entry in &plan_entries {
            let parent = created_nodes.get(&entry.planned_parent_node_id()).ok_or(
                BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt),
            )?;
            let node = Node::new_child(
                entry.planned_node_id(),
                target.library.id(),
                parent,
                entry.kind(),
                entry.name().clone(),
                executed_at,
            )?;
            DomainRepository::insert_node_row_in_transaction(
                &mut transaction,
                &NodeRow::from_domain(&node)?,
            )
            .await?;

            let (node, file_version_id, change_kind) =
                if entry.action() == BackupRestoreAction::CreateFile {
                    let source_node_id =
                        entry
                            .source_snapshot_node_id()
                            .ok_or(BackupError::RestorePreflight(
                                BackupRestorePreflightIssue::SnapshotCorrupt,
                            ))?;
                    let retained =
                        retained
                            .get(&source_node_id)
                            .ok_or(BackupError::RestorePreflight(
                                BackupRestorePreflightIssue::ContentUnavailable,
                            ))?;
                    let version = FileVersion::new(
                        FileVersionId::new(),
                        &target.library,
                        &node,
                        retained.object,
                        None,
                        executed_at,
                    )?;
                    DomainRepository::insert_file_version_in_transaction(&mut transaction, version)
                        .await?;
                    let node = node.with_current_version(&version, executed_at)?;
                    DomainRepository::update_node_in_transaction(&mut transaction, &node).await?;
                    (node, Some(version.id()), ChangeKind::FileContentCommitted)
                } else {
                    (node, None, ChangeKind::NodeCreated)
                };

            journal_changes.push(JournalChange::from_node(change_kind, &node));
            mutations.push(ExecutedRestoreMutation {
                ordinal: entry.ordinal(),
                destination_node_id: node.id(),
                destination_file_version_id: file_version_id,
                change_kind,
            });
            created_nodes.insert(node.id(), node);
        }

        let mut journal_sequences = Vec::with_capacity(journal_changes.len());
        for changes in journal_changes.chunks(64) {
            let events = append_changes(
                &mut transaction,
                owner_user_id,
                target.library.id(),
                changes,
            )
            .await?;
            journal_sequences.extend(events.into_iter().map(|event| event.sequence()));
        }
        if journal_sequences.len() != mutations.len()
            || journal_sequences.first().map(|value| value.get())
                != Some(
                    u64::try_from(journal_first_sequence)
                        .map_err(|_| BackupError::InvalidPersistedData)?,
                )
            || journal_sequences.last().map(|value| value.get())
                != Some(
                    u64::try_from(journal_last_sequence)
                        .map_err(|_| BackupError::InvalidPersistedData)?,
                )
        {
            return Err(BackupError::InvalidPersistedData);
        }

        for (mutation, sequence) in mutations.iter().zip(journal_sequences.iter().copied()) {
            let (node_created_sequence, file_content_sequence) = match mutation.change_kind {
                ChangeKind::NodeCreated => (Some(sequence), None),
                ChangeKind::FileContentCommitted => (None, Some(sequence)),
                _ => return Err(BackupError::InvalidPersistedData),
            };
            sqlx::query(
                "INSERT INTO backup_restore_execution_entries
                    (execution_id, plan_id, ordinal, destination_node_id,
                     destination_file_version_id, node_created_journal_sequence,
                     file_content_committed_journal_sequence)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(execution_id.into_uuid())
            .bind(plan.id().into_uuid())
            .bind(i64::try_from(mutation.ordinal).map_err(|_| BackupError::InvalidPersistedData)?)
            .bind(mutation.destination_node_id.into_uuid())
            .bind(
                mutation
                    .destination_file_version_id
                    .map(FileVersionId::into_uuid),
            )
            .bind(node_created_sequence.map(|value| value.get() as i64))
            .bind(file_content_sequence.map(|value| value.get() as i64))
            .execute(&mut *transaction)
            .await?;
        }

        let execution_entry_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
             FROM backup_restore_execution_entries
             WHERE execution_id = $1",
        )
        .bind(execution_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if execution_entry_count != event_count {
            return Err(BackupError::InvalidPersistedData);
        }

        let mut created_node_count = 0_i64;
        let mut created_file_version_count = 0_i64;
        for mutation in &mutations {
            let node_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(
                     SELECT 1 FROM nodes
                     WHERE id = $1 AND library_id = $2
                 )",
            )
            .bind(mutation.destination_node_id.into_uuid())
            .bind(target.library.id().into_uuid())
            .fetch_one(&mut *transaction)
            .await?;
            if !node_exists {
                return Err(BackupError::InvalidPersistedData);
            }
            created_node_count = created_node_count
                .checked_add(1)
                .ok_or(BackupError::InvalidPersistedData)?;
            if let Some(file_version_id) = mutation.destination_file_version_id {
                let version_matches = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(
                         SELECT 1
                         FROM file_versions
                         WHERE id = $1
                           AND library_id = $2
                           AND node_id = $3
                     )",
                )
                .bind(file_version_id.into_uuid())
                .bind(target.library.id().into_uuid())
                .bind(mutation.destination_node_id.into_uuid())
                .fetch_one(&mut *transaction)
                .await?;
                if !version_matches {
                    return Err(BackupError::InvalidPersistedData);
                }
                let current_version_matches = sqlx::query_scalar::<_, bool>(
                    "SELECT current_version_id IS NOT DISTINCT FROM $1
                     FROM nodes
                     WHERE id = $2 AND library_id = $3",
                )
                .bind(file_version_id.into_uuid())
                .bind(mutation.destination_node_id.into_uuid())
                .bind(target.library.id().into_uuid())
                .fetch_one(&mut *transaction)
                .await?;
                if !current_version_matches {
                    return Err(BackupError::InvalidPersistedData);
                }
                created_file_version_count = created_file_version_count
                    .checked_add(1)
                    .ok_or(BackupError::InvalidPersistedData)?;
            }
        }
        if created_node_count != event_count
            || created_file_version_count
                != i64::try_from(plan.content_item_count())
                    .map_err(|_| BackupError::InvalidPersistedData)?
        {
            return Err(BackupError::InvalidPersistedData);
        }

        let receipt_row = sqlx::query_as::<_, BackupRestoreExecutionRow>(
            "UPDATE backup_restore_executions
             SET state = 'COMMITTED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'ASSEMBLING'
             RETURNING id, owner_user_id, restore_plan_id, target_library_id,
                       journal_first_sequence, journal_last_sequence,
                       created_node_count, created_file_version_count, state,
                       executed_at",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;

        let finalized_plan = sqlx::query_as::<_, BackupRestorePlanRow>(
            "UPDATE backup_restore_plans
             SET state = 'EXECUTED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'
             RETURNING id, owner_user_id, backup_set_id, snapshot_id,
                       target_library_id, target_parent_node_id, operation_id,
                       fingerprint_version, request_fingerprint, destination_name,
                       base_journal_epoch, base_journal_head, item_count,
                       content_item_count, state, created_at, stale_at",
        )
        .bind(plan.id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        let finalized_plan = finalized_plan.try_into_domain()?;
        if finalized_plan.state() != BackupRestorePlanState::Executed {
            return Err(BackupError::InvalidPersistedData);
        }

        let receipt = receipt_row.try_into_domain()?;
        transaction.commit().await?;
        Ok(receipt)
    }

    /// Read one owner-scoped committed receipt. Inaccessible executions are
    /// intentionally indistinguishable from missing executions.
    pub async fn get_restore_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupRestoreExecutionId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        sqlx::query_as::<_, BackupRestoreExecutionRow>(
            "SELECT execution.id, execution.owner_user_id, execution.restore_plan_id,
                    execution.target_library_id, execution.journal_first_sequence,
                    execution.journal_last_sequence, execution.created_node_count,
                    execution.created_file_version_count, execution.state,
                    execution.executed_at
             FROM backup_restore_executions AS execution
             WHERE execution.id = $1
               AND execution.owner_user_id = $2
               AND execution.state = 'COMMITTED'",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)
        .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// Read immutable per-entry evidence for one owner-scoped committed
    /// execution. This is metadata evidence only and contains no Object ID.
    pub async fn list_restore_execution_entries(
        &self,
        owner_user_id: UserId,
        execution_id: BackupRestoreExecutionId,
    ) -> Result<Vec<BackupRestoreExecutionEntry>, BackupError> {
        let visible = self
            .get_restore_execution(owner_user_id, execution_id)
            .await?;
        let rows = sqlx::query_as::<_, BackupRestoreExecutionEntryRow>(
            "SELECT entry.execution_id, entry.plan_id, entry.ordinal,
                    entry.destination_node_id, entry.destination_file_version_id,
                    entry.node_created_journal_sequence,
                    entry.file_content_committed_journal_sequence
             FROM backup_restore_execution_entries AS entry
             INNER JOIN backup_restore_executions AS execution
               ON execution.id = entry.execution_id
              AND execution.owner_user_id = $2
              AND execution.state = 'COMMITTED'
             WHERE entry.execution_id = $1
             ORDER BY entry.ordinal ASC",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_all(self.pool.sqlx_pool())
        .await?;
        let entries = rows
            .into_iter()
            .map(BackupRestoreExecutionEntryRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        if entries.len() as u64 != visible.created_node_count() {
            return Err(BackupError::InvalidPersistedData);
        }
        Ok(entries)
    }

    /// Revalidate the target namespace observation for a plan. A changed
    /// epoch/head, an invalidated parent, or a newly occupied destination
    /// performs the one permitted transition `PLANNED -> STALE`; the original
    /// planning basis is never rewritten.
    pub async fn validate_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestorePlan, BackupError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let initial = sqlx::query_as::<_, BackupRestorePlanRow>(
            "SELECT id, owner_user_id, backup_set_id, snapshot_id,
                    target_library_id, target_parent_node_id, operation_id,
                    fingerprint_version, request_fingerprint, destination_name,
                    base_journal_epoch, base_journal_head, item_count,
                    content_item_count, state, created_at, stale_at
             FROM backup_restore_plans
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;
        let target_library_id = initial.target_library_id;

        acquire_namespace_guard(
            &mut transaction,
            decode_id(target_library_id, "backup_restore_plans.target_library_id")?,
        )
        .await?;
        let current = sqlx::query_as::<_, BackupRestorePlanRow>(
            "SELECT id, owner_user_id, backup_set_id, snapshot_id,
                    target_library_id, target_parent_node_id, operation_id,
                    fingerprint_version, request_fingerprint, destination_name,
                    base_journal_epoch, base_journal_head, item_count,
                    content_item_count, state, created_at, stale_at
             FROM backup_restore_plans
             WHERE id = $1 AND owner_user_id = $2
             FOR UPDATE",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::NotFound)?;
        let plan = current.try_into_domain()?;
        if matches!(
            plan.state(),
            BackupRestorePlanState::Stale | BackupRestorePlanState::Executed
        ) {
            transaction.commit().await?;
            return Ok(plan);
        }

        let target = load_restore_target(
            &mut transaction,
            owner_user_id,
            plan.target_library_id(),
            plan.target_parent_node_id(),
            plan.destination_name(),
        )
        .await?;
        let stale = target.as_ref().is_none_or(|target| {
            !target.parent_is_valid
                || target.journal_epoch
                    != i64::try_from(plan.base_journal_epoch().get()).unwrap_or(i64::MIN)
                || target.sync_head
                    != i64::try_from(plan.base_journal_head().get()).unwrap_or(i64::MIN)
                || target.destination_exists
        });
        if !stale {
            transaction.commit().await?;
            return Ok(plan);
        }

        let row = sqlx::query_as::<_, BackupRestorePlanRow>(
            "UPDATE backup_restore_plans
             SET state = 'STALE', stale_at = CURRENT_TIMESTAMP
             WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'
             RETURNING id, owner_user_id, backup_set_id, snapshot_id,
                       target_library_id, target_parent_node_id, operation_id,
                       fingerprint_version, request_fingerprint, destination_name,
                       base_journal_epoch, base_journal_head, item_count,
                       content_item_count, state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        let row = row.try_into_domain()?;
        transaction.commit().await?;
        Ok(row)
    }

    /// Create one durable, immutable retention-release preflight for exactly
    /// one EXPIRED snapshot. This observes PostgreSQL metadata only: it never
    /// releases a pin, mutates snapshot/manifest/live metadata, touches GC, or
    /// performs ObjectStore I/O.
    pub async fn create_prune_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupPrunePlan, BackupError> {
        validate_operation_key(&operation_id)?;
        let request = BackupPrunePlanRequest::new(backup_set_id, snapshot_id);
        let fingerprint = request.fingerprint();
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        if let Some(existing) =
            load_prune_plan_by_operation(&mut transaction, owner_user_id, &operation_id).await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::PrunePlanConflict);
            }
            return Ok(existing);
        }

        // The source snapshot row is the single-snapshot coordination fence.
        // All normal plan creators take it before inspecting active plans, so
        // competing operation identities cannot race into duplicate plans.
        let snapshot_row = load_prune_snapshot_for_update(
            &mut transaction,
            owner_user_id,
            backup_set_id,
            snapshot_id,
        )
        .await?
        .ok_or(BackupError::NotFound)?;
        let snapshot = snapshot_row
            .try_into_domain()
            .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::SnapshotCorrupt))?;
        validate_prune_snapshot_set(&mut transaction, owner_user_id, &snapshot).await?;

        // A concurrent same-operation creator may have committed while this
        // transaction waited for the source snapshot row.
        if let Some(existing) =
            load_prune_plan_by_operation(&mut transaction, owner_user_id, &operation_id).await?
        {
            let existing = existing.try_into_domain()?;
            transaction.commit().await?;
            if existing.request_fingerprint() != fingerprint {
                return Err(BackupError::PrunePlanConflict);
            }
            return Ok(existing);
        }

        if snapshot.state() != SnapshotState::Expired {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::SnapshotNotExpired,
            ));
        }
        // SnapshotAlreadyPruned check: a successfully executed prune plan for this
        // snapshot means its dedicated retention was already released. Explicit
        // rejection avoids leaking that the pins are legitimately missing vs.
        // generic corruption.
        let executed_plan = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_prune_plans
             WHERE snapshot_id = $1 AND state = 'EXECUTED'",
        )
        .bind(snapshot_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if executed_plan > 0 {
            transaction.commit().await?;
            return Err(BackupError::SnapshotAlreadyPruned);
        }
        if load_active_prune_plan_for_snapshot(&mut transaction, snapshot.id())
            .await?
            .is_some()
        {
            transaction.commit().await?;
            return Err(BackupError::PruneAlreadyPlanned);
        }

        let source = collect_prune_source_observation(&mut transaction, &snapshot).await?;

        // Lock every affected canonical Object in stable identity order before
        // reading reference counts. Reference creators serialize on the same
        // Object row, so the observation is entirely before or after each
        // concurrent live-version/pin mutation, never a mixed basis.
        lock_prune_source_objects(&mut transaction, snapshot.id()).await?;
        let impacts = observe_prune_object_impacts(&mut transaction, snapshot.id()).await?;
        let plan_id = BackupPrunePlanId::new();
        let entries = source.logical_entries(plan_id)?;
        let counts = PrunePlanCounts::from_source_and_impacts(&source, &impacts)?;

        let inserted = sqlx::query_as::<_, BackupPrunePlanRow>(
            "INSERT INTO backup_prune_plans
                (id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                 fingerprint_version, request_fingerprint,
                 snapshot_manifest_item_count, snapshot_content_reference_count,
                 planned_pin_release_count, distinct_retained_content_count,
                 retained_after_release_count, would_become_unreferenced_count,
                 state, created_at, stale_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                     'ASSEMBLING', CURRENT_TIMESTAMP, NULL)
             ON CONFLICT DO NOTHING
             RETURNING id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                       fingerprint_version, request_fingerprint,
                       snapshot_manifest_item_count, snapshot_content_reference_count,
                       planned_pin_release_count, distinct_retained_content_count,
                       retained_after_release_count, would_become_unreferenced_count,
                       state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(snapshot_id.into_uuid())
        .bind(&operation_id)
        .bind(i16::try_from(fingerprint.version()).map_err(|_| BackupError::InvalidRequest)?)
        .bind(fingerprint.sha256().as_slice())
        .bind(counts.snapshot_manifest_item_count)
        .bind(counts.snapshot_content_reference_count)
        .bind(counts.planned_pin_release_count)
        .bind(counts.distinct_retained_content_count)
        .bind(counts.retained_after_release_count)
        .bind(counts.would_become_unreferenced_count)
        .fetch_optional(&mut *transaction)
        .await?;

        if inserted.is_none() {
            if let Some(existing) =
                load_prune_plan_by_operation(&mut transaction, owner_user_id, &operation_id).await?
            {
                let existing = existing.try_into_domain()?;
                transaction.commit().await?;
                if existing.request_fingerprint() != fingerprint {
                    return Err(BackupError::PrunePlanConflict);
                }
                return Ok(existing);
            }
            if load_active_prune_plan_for_snapshot(&mut transaction, snapshot.id())
                .await?
                .is_some()
            {
                transaction.commit().await?;
                return Err(BackupError::PruneAlreadyPlanned);
            }
            return Err(BackupError::InvalidPersistedData);
        }

        for entry in &entries {
            sqlx::query(
                "INSERT INTO backup_prune_plan_entries
                    (plan_id, ordinal, source_snapshot_node_id, file_version_id,
                     content_length, content_sha256)
                 VALUES ($1, $2, $3, $4, $5::NUMERIC, $6)",
            )
            .bind(entry.plan_id().into_uuid())
            .bind(i64::try_from(entry.ordinal()).map_err(|_| BackupError::InvalidRequest)?)
            .bind(entry.source_snapshot_node_id().into_uuid())
            .bind(entry.content().file_version_id().into_uuid())
            .bind(entry.content().byte_length().to_string())
            .bind(entry.content().sha256().as_bytes())
            .execute(&mut *transaction)
            .await?;
        }
        for impact in &impacts {
            sqlx::query(
                "INSERT INTO backup_prune_plan_object_impacts
                    (plan_id, object_id, object_dedup_domain_id,
                     target_snapshot_pin_count,
                     surviving_live_file_version_reference_count,
                     surviving_other_snapshot_pin_count,
                     predicted_post_release_reference_count, impact)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(plan_id.into_uuid())
            .bind(impact.object_id)
            .bind(impact.object_dedup_domain_id)
            .bind(impact.target_snapshot_pin_count)
            .bind(impact.surviving_live_file_version_reference_count)
            .bind(impact.surviving_other_snapshot_pin_count)
            .bind(impact.predicted_post_release_reference_count)
            .bind(impact.impact.as_str())
            .execute(&mut *transaction)
            .await?;
        }
        verify_prune_plan_persisted_counts(&mut transaction, plan_id, snapshot.id(), &counts)
            .await?;

        let finalized = sqlx::query_as::<_, BackupPrunePlanRow>(
            "UPDATE backup_prune_plans
             SET state = 'PLANNED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'ASSEMBLING'
             RETURNING id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                       fingerprint_version, request_fingerprint,
                       snapshot_manifest_item_count, snapshot_content_reference_count,
                       planned_pin_release_count, distinct_retained_content_count,
                       retained_after_release_count, would_become_unreferenced_count,
                       state, created_at, stale_at",
        )
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        let plan = finalized.try_into_domain()?;
        transaction.commit().await?;
        Ok(plan)
    }

    /// Read an immutable owner-scoped prune plan without revalidating it.
    pub async fn get_prune_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
    ) -> Result<BackupPrunePlan, BackupError> {
        load_prune_plan(self.pool.sqlx_pool(), owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)
            .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// List immutable logical release entries by ordinal keyset. No Object or
    /// replica identity is projected across this boundary.
    pub async fn list_prune_plan_entries(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
        after: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<BackupPrunePlanEntry>, bool), BackupError> {
        validate_prune_plan_entry_limit(limit)?;
        let visible = load_prune_plan(self.pool.sqlx_pool(), owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let _ = visible.try_into_domain()?;
        let after = after
            .map(|value| i64::try_from(value).map_err(|_| BackupError::InvalidRequest))
            .transpose()?;
        let rows = sqlx::query_as::<_, BackupPrunePlanEntryRow>(
            "SELECT plan_id, ordinal, source_snapshot_node_id, file_version_id,
                    content_length::TEXT AS content_length, content_sha256
             FROM backup_prune_plan_entries
             WHERE plan_id = $1
               AND ($2::BIGINT IS NULL OR ordinal > $2)
             ORDER BY ordinal ASC
             LIMIT $3",
        )
        .bind(plan_id.into_uuid())
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await?;
        let has_more = rows.len() > limit as usize;
        let entries = rows
            .into_iter()
            .take(limit as usize)
            .map(BackupPrunePlanEntryRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, has_more))
    }

    /// Revalidate a planned retention-release observation against current
    /// authoritative references. Any source/pin/entry/impact drift performs
    /// the one allowed lifecycle change, `PLANNED -> STALE`; original counts
    /// and classifications are never recalculated in place.
    pub async fn validate_prune_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
    ) -> Result<BackupPrunePlan, BackupError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let initial = load_prune_plan_for_read(&mut transaction, owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let initial_backup_set_id =
            decode_id(initial.backup_set_id, "backup_prune_plans.backup_set_id")?;
        let initial_snapshot_id = decode_id(initial.snapshot_id, "backup_prune_plans.snapshot_id")?;

        // Match creation's source-snapshot-first lock order. We deliberately
        // do not lock the plan until after this row, avoiding a plan/snapshot
        // cycle with a concurrent creator.
        let snapshot_row = load_prune_snapshot_for_update(
            &mut transaction,
            owner_user_id,
            initial_backup_set_id,
            initial_snapshot_id,
        )
        .await?;
        let Some(snapshot_row) = snapshot_row else {
            // A source row that no longer resolves through the owner/set fence
            // is not a usable release basis. Mark the otherwise-valid plan
            // stale rather than recalculating or treating the missing source
            // as a new plan request. No snapshot data is changed here.
            let current = load_prune_plan_for_update(&mut transaction, owner_user_id, plan_id)
                .await?
                .ok_or(BackupError::NotFound)?;
            let plan = current.try_into_domain()?;
            if plan.state() == BackupPrunePlanState::Stale {
                transaction.commit().await?;
                return Ok(plan);
            }
            let stale = stale_prune_plan(&mut transaction, owner_user_id, plan_id).await?;
            transaction.commit().await?;
            return Ok(stale);
        };
        let current = load_prune_plan_for_update(&mut transaction, owner_user_id, plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let plan = current.try_into_domain()?;
        if plan.state() == BackupPrunePlanState::Stale {
            transaction.commit().await?;
            return Ok(plan);
        }

        let stale = match snapshot_row.try_into_domain() {
            Err(_) => true,
            Ok(snapshot) => {
                match validate_prune_snapshot_set(&mut transaction, owner_user_id, &snapshot).await
                {
                    Ok(()) => {
                        match collect_prune_source_observation(&mut transaction, &snapshot).await {
                            Ok(source) => {
                                if !prune_plan_matches_source(&plan, &source) {
                                    true
                                } else {
                                    let evidence = async {
                                        lock_prune_source_objects(&mut transaction, snapshot.id())
                                            .await?;
                                        let current_impacts = observe_prune_object_impacts(
                                            &mut transaction,
                                            snapshot.id(),
                                        )
                                        .await?;
                                        let entries = load_all_prune_plan_entries(
                                            &mut transaction,
                                            plan.id(),
                                        )
                                        .await?;
                                        let stored_impacts = load_all_prune_plan_object_impacts(
                                            &mut transaction,
                                            plan.id(),
                                        )
                                        .await?;
                                        Ok::<bool, BackupError>(prune_plan_evidence_matches(
                                            &plan,
                                            &source,
                                            &entries,
                                            &current_impacts,
                                            &stored_impacts,
                                        ))
                                    }
                                    .await;
                                    match evidence {
                                        Ok(matches) => !matches,
                                        Err(error)
                                            if prune_observation_error_makes_plan_stale(&error) =>
                                        {
                                            true
                                        }
                                        Err(error) => return Err(error),
                                    }
                                }
                            }
                            Err(error) if prune_observation_error_makes_plan_stale(&error) => true,
                            Err(error) => return Err(error),
                        }
                    }
                    Err(error) if prune_observation_error_makes_plan_stale(&error) => true,
                    Err(error) => return Err(error),
                }
            }
        };
        if !stale {
            transaction.commit().await?;
            return Ok(plan);
        }
        let stale = stale_prune_plan(&mut transaction, owner_user_id, plan_id).await?;
        transaction.commit().await?;
        Ok(stale)
    }

    /// Atomically execute one valid PLANNED prune plan for exactly one EXPIRED
    /// owner-scoped snapshot. This is the first authorized retention release
    /// protocol: it releases ALL target-snapshot pins, verifies post-release
    /// authoritative references, hands every newly unreferenced Object to the
    /// canonical GC candidate pipeline, persists an immutable execution
    /// receipt, and transitions the plan PLANNED -> EXECUTED in one
    /// PostgreSQL transaction.
    ///
    /// Outcomes are binary. On success, all pins are released exactly once and
    /// the canonical receipt commits. On any failure, the transaction rolls
    /// back: no pins are released, no execution receipt exists, and a clean
    /// retry is safe. Reference drift commits only the STALE transition.
    pub async fn execute_prune_plan(
        &self,
        owner_user_id: UserId,
        prune_plan_id: BackupPrunePlanId,
    ) -> Result<BackupPruneExecution, BackupError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        // Owner-concealment read: a foreign or missing plan is NotFound and
        // reveals nothing about another owner's snapshot or retention.
        let initial = load_prune_plan_for_read(&mut transaction, owner_user_id, prune_plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let backup_set_id = decode_id(initial.backup_set_id, "backup_prune_plans.backup_set_id")?;
        let snapshot_id = decode_id(initial.snapshot_id, "backup_prune_plans.snapshot_id")?;

        // Source-snapshot-first lock order, matching creation and validation.
        // This makes concurrent execution against the same snapshot serialize
        // on one authoritative row instead of racing into duplicate releases.
        let snapshot_row = load_prune_snapshot_for_update(
            &mut transaction,
            owner_user_id,
            backup_set_id,
            snapshot_id,
        )
        .await?;

        let current = load_prune_plan_for_update(&mut transaction, owner_user_id, prune_plan_id)
            .await?
            .ok_or(BackupError::NotFound)?;
        let plan = current.try_into_domain()?;

        // Terminal/replay fast paths must resolve before any revalidation of
        // the now-absent released pins.
        match plan.state() {
            BackupPrunePlanState::Executed => {
                let receipt =
                    load_prune_execution_for_plan(&mut transaction, owner_user_id, plan.id())
                        .await?
                        .ok_or(BackupError::InvalidPersistedData)?
                        .try_into_domain()?;
                transaction.commit().await?;
                return Ok(receipt);
            }
            BackupPrunePlanState::Stale => {
                transaction.commit().await?;
                return Err(BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanStale,
                ));
            }
            BackupPrunePlanState::Planned => {}
        }

        // A source row that no longer resolves through the owner/set fence is
        // not a usable release basis. Commit only the STALE transition and do
        // not release anything.
        let Some(snapshot_row) = snapshot_row else {
            let _ = stale_prune_plan(&mut transaction, owner_user_id, prune_plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::PruneExecutionPreflight(
                BackupPruneExecutionPreflightIssue::SnapshotNotExpired,
            ));
        };
        let snapshot = snapshot_row.try_into_domain().map_err(|_| {
            BackupError::PruneExecutionPreflight(BackupPruneExecutionPreflightIssue::PlanCorruption)
        })?;
        match validate_prune_snapshot_set(&mut transaction, owner_user_id, &snapshot).await {
            Ok(()) => {}
            Err(error) if prune_observation_error_makes_plan_stale(&error) => {
                let _ = stale_prune_plan(&mut transaction, owner_user_id, prune_plan_id).await?;
                transaction.commit().await?;
                return Err(BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanCorruption,
                ));
            }
            Err(error) => return Err(error),
        }
        if snapshot.state() != SnapshotState::Expired {
            // Legitimate lifecycle drift: the snapshot is no longer EXPIRED.
            // Commit only the STALE transition; never release retention.
            let _ = stale_prune_plan(&mut transaction, owner_user_id, prune_plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::PruneExecutionPreflight(
                BackupPruneExecutionPreflightIssue::SnapshotNotExpired,
            ));
        }

        // Revalidate the complete immutable basis against current
        // authoritative references exactly like Prompt 45 validation: source
        // observation, exact logical entries, exact pin mappings and counts,
        // and every distinct-Object impact. Structural corruption fails
        // closed (0 release, 0 handoff, 0 receipt); legitimate drift commits
        // the one allowed STALE transition.
        let revalidation = async {
            // Existing candidate rows for the affected Objects are locked
            // before the canonical Object rows, preserving the GC/purge lock
            // order. Missing candidates are handled after the Object locks by
            // the idempotent handoff insert inside the same transaction.
            lock_prune_candidate_rows(&mut transaction, snapshot.id()).await?;
            lock_prune_source_objects(&mut transaction, snapshot.id()).await?;
            let source = collect_prune_source_observation(&mut transaction, &snapshot).await?;
            let entries = load_all_prune_plan_entries(&mut transaction, plan.id()).await?;
            let current_impacts =
                observe_prune_object_impacts(&mut transaction, snapshot.id()).await?;
            let stored_impacts =
                load_all_prune_plan_object_impacts(&mut transaction, plan.id()).await?;
            let source_at_plan = collect_prune_source_observation(&mut transaction, &snapshot)
                .await
                .map_err(|_| BackupError::InvalidPersistedData)?;
            let _ = source_at_plan;
            Ok::<bool, BackupError>(
                prune_plan_matches_source(&plan, &source)
                    && prune_plan_evidence_matches(
                        &plan,
                        &source,
                        &entries,
                        &current_impacts,
                        &stored_impacts,
                    ),
            )
        }
        .await;
        let matches = match revalidation {
            Ok(matches) => matches,
            Err(error) if prune_observation_error_makes_plan_stale(&error) => false,
            Err(error) => return Err(error),
        };
        if !matches {
            // The immutable basis changed since planning. Commit only the
            // STALE transition with 0 release / 0 handoff / 0 receipt.
            let _ = stale_prune_plan(&mut transaction, owner_user_id, prune_plan_id).await?;
            transaction.commit().await?;
            return Err(BackupError::PruneExecutionPreflight(
                BackupPruneExecutionPreflightIssue::PlanReferenceDrift,
            ));
        }

        // Authorized release phase. Everything below commits or rolls back as
        // one atomic unit.
        let executed_at = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .map(Timestamp::from_offset_datetime)?;
        let execution_id = BackupPruneExecutionId::new();
        let planned_pin_release_count =
            i64::try_from(plan.planned_pin_release_count()).map_err(|_| {
                BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanCorruption,
                )
            })?;
        let distinct_object_count =
            i64::try_from(plan.distinct_retained_content_count()).map_err(|_| {
                BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanCorruption,
                )
            })?;
        let retained_count = i64::try_from(plan.retained_after_release_count()).map_err(|_| {
            BackupError::PruneExecutionPreflight(BackupPruneExecutionPreflightIssue::PlanCorruption)
        })?;
        let unreferenced_count =
            i64::try_from(plan.would_become_unreferenced_count()).map_err(|_| {
                BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanCorruption,
                )
            })?;

        // 1. Persist the transaction-local ASSEMBLING execution receipt. The
        //    identity trigger binds every count to the immutable plan basis.
        sqlx::query(
            "INSERT INTO backup_prune_executions
                (id, owner_user_id, prune_plan_id, backup_set_id, snapshot_id,
                 released_pin_count, distinct_object_count,
                 retained_by_other_reference_count, gc_handoff_object_count,
                 state, executed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'ASSEMBLING', $10)",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(plan.id().into_uuid())
        .bind(plan.backup_set_id().into_uuid())
        .bind(plan.snapshot_id().into_uuid())
        .bind(planned_pin_release_count)
        .bind(distinct_object_count)
        .bind(retained_count)
        .bind(unreferenced_count)
        .bind(executed_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?;

        // 2. Authorized release of every target-snapshot pin through the one
        //    narrow protocol. The SECURITY DEFINER function verifies one
        //    PLANNED owner plan, the EXPIRED source snapshot, and the exact
        //    snapshot ownership before deleting; the pin DELETE trigger
        //    independently re-checks the same facts.
        let released =
            sqlx::query_scalar::<_, i64>("SELECT synveil_authorized_prune_pin_release($1, $2)")
                .bind(prune_plan_id.into_uuid())
                .bind(snapshot_id.into_uuid())
                .fetch_one(&mut *transaction)
                .await?;
        if released != planned_pin_release_count {
            return Err(BackupError::InvalidPersistedData);
        }
        let remaining_pins = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
        )
        .bind(snapshot_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if remaining_pins != 0 {
            return Err(BackupError::InvalidPersistedData);
        }

        // 3. Post-release accounting and canonical GC handoff per distinct
        //    Object, in deterministic canonical identity order. Objects are
        //    already locked; a missing candidate row is either absent or
        //    locked for this transaction now, and the idempotent INSERT
        //    closes the absent-at-first-read window.
        let impact_rows = sqlx::query_as::<_, PruneObjectImpactRow>(
            "SELECT object_id, object_dedup_domain_id, target_snapshot_pin_count,
                    surviving_live_file_version_reference_count,
                    surviving_other_snapshot_pin_count,
                    predicted_post_release_reference_count, impact
             FROM backup_prune_plan_object_impacts
             WHERE plan_id = $1
             ORDER BY object_id ASC, object_dedup_domain_id ASC",
        )
        .bind(plan.id().into_uuid())
        .fetch_all(&mut *transaction)
        .await?;
        let expected_impact_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_prune_plan_object_impacts WHERE plan_id = $1",
        )
        .bind(plan.id().into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if expected_impact_count != distinct_object_count {
            return Err(BackupError::InvalidPersistedData);
        }

        let mut actual_retained = 0_i64;
        let mut actual_unreferenced = 0_i64;
        for row in impact_rows {
            let object_id = row.object_id;
            let object_dedup_domain_id = row.object_dedup_domain_id;
            decode_id::<synveil_core::ObjectId>(
                object_id,
                "backup_prune_plan_object_impacts.object_id",
            )
            .map_err(|_| {
                BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanCorruption,
                )
            })?;
            decode_id::<synveil_core::DedupDomainId>(
                object_dedup_domain_id,
                "backup_prune_plan_object_impacts.object_dedup_domain_id",
            )
            .map_err(|_| {
                BackupError::PruneExecutionPreflight(
                    BackupPruneExecutionPreflightIssue::PlanCorruption,
                )
            })?;

            DomainRepository::lock_gc_candidate_row_for_reference(
                &mut transaction,
                ObjectId::try_from_uuid(object_id).map_err(|reason| {
                    MetadataError::Mapping(MappingError::InvalidId {
                        field: "backup_prune_execution_object_results.object_id",
                        reason,
                    })
                })?,
                DedupDomainId::try_from_uuid(object_dedup_domain_id).map_err(|reason| {
                    MetadataError::Mapping(MappingError::InvalidId {
                        field: "backup_prune_execution_object_results.object_dedup_domain_id",
                        reason,
                    })
                })?,
            )
            .await?;
            let post_release: (i64, i64) = sqlx::query_as(
                "SELECT
                    (SELECT count(*)::BIGINT
                     FROM file_versions
                     WHERE object_id = $1 AND object_dedup_domain_id = $2),
                    (SELECT count(*)::BIGINT
                     FROM backup_snapshot_content_pins
                     WHERE object_id = $1
                       AND object_dedup_domain_id = $2
                       AND snapshot_id <> $3)",
            )
            .bind(object_id)
            .bind(object_dedup_domain_id)
            .bind(snapshot_id.into_uuid())
            .fetch_one(&mut *transaction)
            .await?;
            let post_live = post_release.0;
            let post_other = post_release.1;
            let post_total = post_live
                .checked_add(post_other)
                .ok_or(BackupError::InvalidPersistedData)?;

            let (candidate_used, candidate_id, candidate_generation) = if post_total == 0 {
                sqlx::query(
                    "INSERT INTO object_gc_candidates
                        (object_id, object_dedup_domain_id, unreferenced_at, source)
                     VALUES ($1, $2, $3, 'METADATA_PURGE')
                     ON CONFLICT (object_id, object_dedup_domain_id) DO NOTHING",
                )
                .bind(object_id)
                .bind(object_dedup_domain_id)
                .bind(executed_at.as_offset_datetime())
                .execute(&mut *transaction)
                .await?;
                let candidate = sqlx::query_as::<_, (Uuid, String)>(
                    "SELECT object_id, lease_generation::TEXT
                     FROM object_gc_candidates
                     WHERE object_id = $1
                       AND object_dedup_domain_id = $2
                       AND source = 'METADATA_PURGE'",
                )
                .bind(object_id)
                .bind(object_dedup_domain_id)
                .fetch_one(&mut *transaction)
                .await?;
                actual_unreferenced = actual_unreferenced
                    .checked_add(1)
                    .ok_or(BackupError::InvalidPersistedData)?;
                (true, Some(candidate.0), Some(candidate.1))
            } else {
                actual_retained = actual_retained
                    .checked_add(1)
                    .ok_or(BackupError::InvalidPersistedData)?;
                (false, None, None)
            };

            sqlx::query(
                "INSERT INTO backup_prune_execution_object_results
                    (execution_id, plan_id, object_id, object_dedup_domain_id,
                     target_snapshot_pin_count,
                     post_release_live_file_version_count,
                     post_release_other_snapshot_pin_count,
                     post_release_authoritative_reference_count,
                     gc_candidate_created_or_reused,
                     gc_candidate_id,
                     gc_candidate_generation)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::NUMERIC)",
            )
            .bind(execution_id.into_uuid())
            .bind(plan.id().into_uuid())
            .bind(object_id)
            .bind(object_dedup_domain_id)
            .bind(row.target_snapshot_pin_count)
            .bind(post_live)
            .bind(post_other)
            .bind(post_total)
            .bind(candidate_used)
            .bind(candidate_id)
            .bind(candidate_generation)
            .execute(&mut *transaction)
            .await?;
        }

        // 4. Verify execution-count invariants before the receipt is sealed.
        if actual_retained != retained_count
            || actual_unreferenced != unreferenced_count
            || actual_retained
                .checked_add(actual_unreferenced)
                .ok_or(BackupError::InvalidPersistedData)?
                != distinct_object_count
        {
            return Err(BackupError::InvalidPersistedData);
        }
        let evidence_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_prune_execution_object_results
             WHERE execution_id = $1",
        )
        .bind(execution_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        if evidence_count != distinct_object_count {
            return Err(BackupError::InvalidPersistedData);
        }

        // 5. Seal the receipt and transition the plan. Both are deferred-checked
        //    at commit: no EXECUTED plan may commit without its COMMITTED
        //    receipt and no ASSEMBLING execution may commit.
        let receipt_row = sqlx::query_as::<_, BackupPruneExecutionRow>(
            "UPDATE backup_prune_executions
             SET state = 'COMMITTED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'ASSEMBLING'
             RETURNING id, owner_user_id, prune_plan_id, backup_set_id,
                       snapshot_id, released_pin_count, distinct_object_count,
                       retained_by_other_reference_count, gc_handoff_object_count,
                       state, executed_at",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;

        let finalized_plan = sqlx::query_as::<_, BackupPrunePlanRow>(
            "UPDATE backup_prune_plans
             SET state = 'EXECUTED'
             WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'
             RETURNING id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                       fingerprint_version, request_fingerprint,
                       snapshot_manifest_item_count, snapshot_content_reference_count,
                       planned_pin_release_count, distinct_retained_content_count,
                       retained_after_release_count, would_become_unreferenced_count,
                       state, created_at, stale_at",
        )
        .bind(plan.id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        if finalized_plan.state != "EXECUTED" {
            return Err(BackupError::InvalidPersistedData);
        }

        let receipt = receipt_row.try_into_domain()?;
        transaction.commit().await?;
        Ok(receipt)
    }

    /// Read one owner-scoped committed prune execution receipt. Inaccessible
    /// executions are intentionally indistinguishable from missing executions.
    pub async fn get_prune_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupPruneExecutionId,
    ) -> Result<BackupPruneExecution, BackupError> {
        sqlx::query_as::<_, BackupPruneExecutionRow>(
            "SELECT execution.id, execution.owner_user_id, execution.prune_plan_id,
                    execution.backup_set_id, execution.snapshot_id,
                    execution.released_pin_count, execution.distinct_object_count,
                    execution.retained_by_other_reference_count,
                    execution.gc_handoff_object_count, execution.state,
                    execution.executed_at
             FROM backup_prune_executions AS execution
             WHERE execution.id = $1
               AND execution.owner_user_id = $2
               AND execution.state = 'COMMITTED'",
        )
        .bind(execution_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupError::NotFound)
        .and_then(|row| row.try_into_domain().map_err(Into::into))
    }

    /// List the owner- and backup-set-scoped semantic backup operations in a
    /// deterministic heterogeneous keyset order. This is a pure metadata
    /// projection: every returned state comes from the canonical lifecycle
    /// rows and committed child receipts.
    pub async fn list_backup_operations(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        kind: Option<BackupOperationKind>,
        after: Option<BackupOperationPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupOperationSummary>, bool), BackupError> {
        validate_page_limit_operation(limit)?;
        if kind.is_some_and(|kind| after.is_some_and(|after| after.operation_id().kind() != kind)) {
            return Err(BackupError::InvalidRequest);
        }
        // Preserve the existing concealment convention: an unknown/foreign
        // set is not an empty activity feed.
        self.get_backup_set(owner_user_id, backup_set_id).await?;

        let mut details = Vec::new();
        if kind.is_none_or(|kind| kind == BackupOperationKind::Maintenance) {
            details.extend(
                list_maintenance_operation_details(
                    self.pool.sqlx_pool(),
                    owner_user_id,
                    backup_set_id,
                    after,
                    limit,
                )
                .await?,
            );
        }
        if kind.is_none_or(|kind| kind == BackupOperationKind::Restore) {
            details.extend(
                list_restore_operation_details(
                    self.pool.sqlx_pool(),
                    owner_user_id,
                    backup_set_id,
                    after,
                    limit,
                )
                .await?,
            );
        }
        if kind.is_none_or(|kind| kind == BackupOperationKind::Prune) {
            details.extend(
                list_prune_operation_details(
                    self.pool.sqlx_pool(),
                    owner_user_id,
                    backup_set_id,
                    after,
                    limit,
                )
                .await?,
            );
        }

        let mut summaries = details
            .iter()
            .map(BackupOperationDetail::summary)
            .collect::<Result<Vec<_>, _>>()?;
        summaries.sort_by(compare_backup_operation_summaries);
        let has_more = summaries.len() > limit as usize;
        summaries.truncate(limit as usize);
        Ok((summaries, has_more))
    }

    /// Read one kind-qualified semantic operation. The operation kind selects
    /// the canonical table, so a UUID from another kind is concealed as
    /// `NotFound` rather than being probed across tables.
    pub async fn get_backup_operation(
        &self,
        owner_user_id: UserId,
        kind: BackupOperationKind,
        operation_id: BackupOperationId,
    ) -> Result<BackupOperationDetail, BackupError> {
        if operation_id.kind() != kind {
            return Err(BackupError::NotFound);
        }
        match operation_id {
            BackupOperationId::Maintenance(run_id) => Ok(BackupOperationDetail::Maintenance(
                self.get_backup_maintenance_run(owner_user_id, run_id)
                    .await?,
            )),
            BackupOperationId::Restore(plan_id) => {
                let plan = self.get_restore_plan(owner_user_id, plan_id).await?;
                let execution = load_restore_execution_for_operation(
                    self.pool.sqlx_pool(),
                    owner_user_id,
                    plan.id(),
                )
                .await?
                .map(BackupRestoreExecutionRow::try_into_domain)
                .transpose()?;
                validate_restore_operation_shape(&plan, execution.as_ref())?;
                Ok(BackupOperationDetail::Restore { plan, execution })
            }
            BackupOperationId::Prune(plan_id) => {
                let plan = self.get_prune_plan(owner_user_id, plan_id).await?;
                let execution = load_prune_execution_for_operation(
                    self.pool.sqlx_pool(),
                    owner_user_id,
                    plan.id(),
                )
                .await?
                .map(BackupPruneExecutionRow::try_into_domain)
                .transpose()?;
                validate_prune_operation_shape(&plan, execution.as_ref())?;
                Ok(BackupOperationDetail::Prune { plan, execution })
            }
        }
    }
}

fn compare_backup_operation_summaries(
    left: &BackupOperationSummary,
    right: &BackupOperationSummary,
) -> std::cmp::Ordering {
    right
        .created_at()
        .cmp(&left.created_at())
        .then_with(|| {
            left.operation_kind()
                .rank()
                .cmp(&right.operation_kind().rank())
        })
        .then_with(|| {
            right
                .operation_id()
                .into_uuid()
                .cmp(&left.operation_id().into_uuid())
        })
}

#[async_trait]
impl BackupReadBackend for BackupService {
    async fn list_backup_sets(
        &self,
        owner_user_id: UserId,
        after: Option<BackupSetId>,
        limit: u32,
    ) -> Result<(Vec<BackupSet>, bool), BackupError> {
        BackupService::list_backup_sets(self, owner_user_id, after, limit).await
    }

    async fn get_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSet, BackupError> {
        BackupService::get_backup_set(self, owner_user_id, backup_set_id).await
    }

    async fn list_backup_snapshots(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        state: Option<SnapshotState>,
        after: Option<BackupSnapshotPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshot>, bool), BackupError> {
        BackupService::list_backup_snapshots(
            self,
            owner_user_id,
            backup_set_id,
            state,
            after,
            limit,
        )
        .await
    }

    async fn get_backup_snapshot(
        &self,
        owner_user_id: UserId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupSnapshot, BackupError> {
        BackupService::get_backup_snapshot(self, owner_user_id, snapshot_id).await
    }

    async fn list_backup_snapshot_nodes(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        parent_node_id: Option<NodeId>,
        after: Option<NodeId>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotNode>, bool), BackupError> {
        BackupService::list_backup_snapshot_nodes(
            self,
            owner_user_id,
            backup_set_id,
            snapshot_id,
            parent_node_id,
            after,
            limit,
        )
        .await
    }

    async fn get_current_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        BackupService::get_current_snapshot_retention_policy(self, owner_user_id, backup_set_id)
            .await
    }

    async fn list_backup_maintenance_runs(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        after: Option<BackupMaintenanceRunPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupMaintenanceRun>, bool), BackupError> {
        BackupService::list_backup_maintenance_runs(
            self,
            owner_user_id,
            backup_set_id,
            after,
            limit,
        )
        .await
    }

    async fn get_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        BackupService::get_backup_maintenance_run(self, owner_user_id, run_id).await
    }

    async fn get_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestorePlan, BackupError> {
        BackupService::get_restore_plan(self, owner_user_id, plan_id).await
    }

    async fn get_restore_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupRestoreExecutionId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        BackupService::get_restore_execution(self, owner_user_id, execution_id).await
    }

    async fn get_prune_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
    ) -> Result<BackupPrunePlan, BackupError> {
        BackupService::get_prune_plan(self, owner_user_id, plan_id).await
    }

    async fn get_prune_execution(
        &self,
        owner_user_id: UserId,
        execution_id: BackupPruneExecutionId,
    ) -> Result<BackupPruneExecution, BackupError> {
        BackupService::get_prune_execution(self, owner_user_id, execution_id).await
    }

    async fn list_backup_operations(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        kind: Option<BackupOperationKind>,
        after: Option<BackupOperationPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupOperationSummary>, bool), BackupError> {
        BackupService::list_backup_operations(
            self,
            owner_user_id,
            backup_set_id,
            kind,
            after,
            limit,
        )
        .await
    }

    async fn get_backup_operation(
        &self,
        owner_user_id: UserId,
        kind: BackupOperationKind,
        operation_id: BackupOperationId,
    ) -> Result<BackupOperationDetail, BackupError> {
        BackupService::get_backup_operation(self, owner_user_id, kind, operation_id).await
    }
}

#[async_trait]
impl BackupMutationBackend for BackupService {
    async fn create_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        name: LogicalName,
        source_library_id: LibraryId,
        observed_at: Timestamp,
    ) -> Result<BackupSet, BackupError> {
        BackupService::create_backup_set(
            self,
            owner_user_id,
            backup_set_id,
            name,
            source_library_id,
            None,
            observed_at,
        )
        .await
    }

    async fn configure_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        keep_latest_completed: u64,
        expire_after_seconds: u64,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        BackupService::configure_snapshot_retention_policy(
            self,
            owner_user_id,
            operation_id,
            backup_set_id,
            keep_latest_completed,
            expire_after_seconds,
        )
        .await
    }

    async fn create_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        BackupService::create_backup_maintenance_run(
            self,
            owner_user_id,
            operation_id,
            backup_set_id,
        )
        .await
    }

    async fn advance_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        BackupService::advance_backup_maintenance_run(self, owner_user_id, run_id).await
    }

    async fn create_restore_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        target_library_id: LibraryId,
        target_parent_node_id: NodeId,
        destination_name: LogicalName,
    ) -> Result<BackupRestorePlan, BackupError> {
        BackupService::create_restore_plan(
            self,
            owner_user_id,
            operation_id,
            backup_set_id,
            snapshot_id,
            target_library_id,
            target_parent_node_id,
            destination_name,
        )
        .await
    }

    async fn execute_restore_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        BackupService::execute_restore_plan(self, owner_user_id, plan_id).await
    }

    async fn create_prune_plan(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupPrunePlan, BackupError> {
        BackupService::create_prune_plan(
            self,
            owner_user_id,
            operation_id,
            backup_set_id,
            snapshot_id,
        )
        .await
    }

    async fn execute_prune_plan(
        &self,
        owner_user_id: UserId,
        plan_id: BackupPrunePlanId,
    ) -> Result<BackupPruneExecution, BackupError> {
        BackupService::execute_prune_plan(self, owner_user_id, plan_id).await
    }
}

async fn list_maintenance_operation_details(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    after: Option<BackupOperationPagePosition>,
    limit: u32,
) -> Result<Vec<BackupOperationDetail>, BackupError> {
    let rows = sqlx::query_as::<_, BackupMaintenanceRunRow>(
        "SELECT run.id, run.owner_user_id, run.backup_set_id, run.policy_revision_id,
                run.policy_revision_number, run.operation_id, run.fingerprint_version,
                run.request_fingerprint, run.capture_operation_id,
                run.expiry_plan_operation_id, run.state, run.captured_snapshot_id,
                run.expiry_plan_id, run.expiry_execution_id, run.snapshot_captured_at,
                run.expiry_planned_at, run.maintenance_completed_at, run.stale_at,
                run.created_at
         FROM backup_maintenance_runs AS run
         WHERE run.owner_user_id = $1
           AND run.backup_set_id = $2
           AND (
                $3::TIMESTAMPTZ IS NULL
                OR run.created_at < $3
                OR (run.created_at = $3 AND (
                    1 > $4::SMALLINT
                    OR (1 = $4::SMALLINT AND run.id < $5)
                ))
           )
         ORDER BY run.created_at DESC, run.id DESC
         LIMIT $6",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(after.map(|position| position.created_at().as_offset_datetime()))
    .bind(after.map(|position| i16::from(position.operation_id().kind().rank())))
    .bind(after.map(|position| position.operation_id().into_uuid()))
    .bind(i64::from(limit) + 1)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            row.try_into_domain()
                .map(BackupOperationDetail::Maintenance)
                .map_err(Into::into)
        })
        .collect()
}

async fn list_restore_operation_details(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    after: Option<BackupOperationPagePosition>,
    limit: u32,
) -> Result<Vec<BackupOperationDetail>, BackupError> {
    let rows = sqlx::query_as::<_, BackupRestoreOperationRow>(
        "SELECT plan.id, plan.owner_user_id, plan.backup_set_id, plan.snapshot_id,
                plan.target_library_id, plan.target_parent_node_id,
                plan.operation_id, plan.fingerprint_version, plan.request_fingerprint,
                plan.destination_name, plan.base_journal_epoch, plan.base_journal_head,
                plan.item_count, plan.content_item_count, plan.state, plan.created_at,
                plan.stale_at,
                execution.id AS execution_id,
                execution.owner_user_id AS execution_owner_user_id,
                execution.restore_plan_id AS execution_restore_plan_id,
                execution.target_library_id AS execution_target_library_id,
                execution.journal_first_sequence AS execution_journal_first_sequence,
                execution.journal_last_sequence AS execution_journal_last_sequence,
                execution.created_node_count AS execution_created_node_count,
                execution.created_file_version_count AS execution_created_file_version_count,
                execution.state AS execution_state,
                execution.executed_at AS execution_executed_at
         FROM backup_restore_plans AS plan
         INNER JOIN backup_snapshots AS source_snapshot
            ON source_snapshot.id = plan.snapshot_id
           AND source_snapshot.owner_user_id = plan.owner_user_id
           AND source_snapshot.backup_set_id = plan.backup_set_id
         LEFT JOIN backup_restore_executions AS execution
            ON execution.restore_plan_id = plan.id
           AND execution.owner_user_id = plan.owner_user_id
           AND execution.state = 'COMMITTED'
         WHERE plan.owner_user_id = $1
           AND plan.backup_set_id = $2
           AND (
                $3::TIMESTAMPTZ IS NULL
                OR plan.created_at < $3
                OR (plan.created_at = $3 AND (
                    2 > $4::SMALLINT
                    OR (2 = $4::SMALLINT AND plan.id < $5)
                ))
           )
         ORDER BY plan.created_at DESC, plan.id DESC
         LIMIT $6",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(after.map(|position| position.created_at().as_offset_datetime()))
    .bind(after.map(|position| i16::from(position.operation_id().kind().rank())))
    .bind(after.map(|position| position.operation_id().into_uuid()))
    .bind(i64::from(limit) + 1)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(BackupRestoreOperationRow::try_into_detail)
        .collect()
}

async fn list_prune_operation_details(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    after: Option<BackupOperationPagePosition>,
    limit: u32,
) -> Result<Vec<BackupOperationDetail>, BackupError> {
    let rows = sqlx::query_as::<_, BackupPruneOperationRow>(
        "SELECT plan.id, plan.owner_user_id, plan.backup_set_id, plan.snapshot_id,
                plan.operation_id, plan.fingerprint_version, plan.request_fingerprint,
                plan.snapshot_manifest_item_count, plan.snapshot_content_reference_count,
                plan.planned_pin_release_count, plan.distinct_retained_content_count,
                plan.retained_after_release_count, plan.would_become_unreferenced_count,
                plan.state, plan.created_at, plan.stale_at,
                execution.id AS execution_id,
                execution.owner_user_id AS execution_owner_user_id,
                execution.prune_plan_id AS execution_prune_plan_id,
                execution.backup_set_id AS execution_backup_set_id,
                execution.snapshot_id AS execution_snapshot_id,
                execution.released_pin_count AS execution_released_pin_count,
                execution.distinct_object_count AS execution_distinct_object_count,
                execution.retained_by_other_reference_count
                    AS execution_retained_by_other_reference_count,
                execution.gc_handoff_object_count AS execution_gc_handoff_object_count,
                execution.state AS execution_state,
                execution.executed_at AS execution_executed_at
         FROM backup_prune_plans AS plan
         INNER JOIN backup_snapshots AS target_snapshot
            ON target_snapshot.id = plan.snapshot_id
           AND target_snapshot.owner_user_id = plan.owner_user_id
           AND target_snapshot.backup_set_id = plan.backup_set_id
         LEFT JOIN backup_prune_executions AS execution
            ON execution.prune_plan_id = plan.id
           AND execution.owner_user_id = plan.owner_user_id
           AND execution.state = 'COMMITTED'
         WHERE plan.owner_user_id = $1
           AND plan.backup_set_id = $2
           AND (
                $3::TIMESTAMPTZ IS NULL
                OR plan.created_at < $3
                OR (plan.created_at = $3 AND (
                    3 > $4::SMALLINT
                    OR (3 = $4::SMALLINT AND plan.id < $5)
                ))
           )
         ORDER BY plan.created_at DESC, plan.id DESC
         LIMIT $6",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(after.map(|position| position.created_at().as_offset_datetime()))
    .bind(after.map(|position| i16::from(position.operation_id().kind().rank())))
    .bind(after.map(|position| position.operation_id().into_uuid()))
    .bind(i64::from(limit) + 1)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(BackupPruneOperationRow::try_into_detail)
        .collect()
}

async fn load_restore_execution_for_operation(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    plan_id: BackupRestorePlanId,
) -> Result<Option<BackupRestoreExecutionRow>, BackupError> {
    sqlx::query_as::<_, BackupRestoreExecutionRow>(
        "SELECT id, owner_user_id, restore_plan_id, target_library_id,
                journal_first_sequence, journal_last_sequence,
                created_node_count, created_file_version_count, state, executed_at
         FROM backup_restore_executions
         WHERE restore_plan_id = $1
           AND owner_user_id = $2
           AND state = 'COMMITTED'",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn load_prune_execution_for_operation(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    plan_id: BackupPrunePlanId,
) -> Result<Option<BackupPruneExecutionRow>, BackupError> {
    sqlx::query_as::<_, BackupPruneExecutionRow>(
        "SELECT id, owner_user_id, prune_plan_id, backup_set_id, snapshot_id,
                released_pin_count, distinct_object_count,
                retained_by_other_reference_count, gc_handoff_object_count,
                state, executed_at
         FROM backup_prune_executions
         WHERE prune_plan_id = $1
           AND owner_user_id = $2
           AND state = 'COMMITTED'",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

#[derive(Clone, Copy, Debug, FromRow)]
struct ExpiryCohortRow {
    snapshot_id: Uuid,
    committed_at: OffsetDateTime,
}

impl ExpiryCohortRow {
    fn snapshot_id(self) -> Result<SnapshotId, BackupError> {
        decode_id(self.snapshot_id, "backup_snapshots.id").map_err(Into::into)
    }

    fn committed_at(self) -> Timestamp {
        Timestamp::from_offset_datetime(self.committed_at)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpiryPlanCounts {
    evaluated_completed_snapshot_count: i64,
    expire_candidate_count: i64,
    keep_latest_count: i64,
    keep_recent_count: i64,
    blocked_active_restore_count: i64,
}

impl ExpiryPlanCounts {
    fn matches_plan(self, plan: &BackupSnapshotExpiryPlan) -> bool {
        u64::try_from(self.evaluated_completed_snapshot_count)
            .is_ok_and(|value| value == plan.evaluated_completed_snapshot_count())
            && u64::try_from(self.expire_candidate_count)
                .is_ok_and(|value| value == plan.expire_candidate_count())
            && u64::try_from(self.keep_latest_count)
                .is_ok_and(|value| value == plan.keep_latest_count())
            && u64::try_from(self.keep_recent_count)
                .is_ok_and(|value| value == plan.keep_recent_count())
            && u64::try_from(self.blocked_active_restore_count)
                .is_ok_and(|value| value == plan.blocked_active_restore_count())
    }
}

#[derive(Clone, Debug)]
struct ExpiryObservation {
    basis_fingerprint: BackupSnapshotExpiryBasisFingerprint,
    entries: Vec<BackupSnapshotExpiryPlanEntry>,
    counts: ExpiryPlanCounts,
}

#[allow(clippy::too_many_arguments)]
fn build_expiry_observation(
    plan_id: BackupSnapshotExpiryPlanId,
    backup_set_id: BackupSetId,
    policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
    evaluated_at: Timestamp,
    cutoff_at: Timestamp,
    config: BackupSnapshotRetentionPolicyConfig,
    cohort: &[ExpiryCohortRow],
    blockers: &BTreeSet<SnapshotId>,
) -> Result<ExpiryObservation, BackupError> {
    let mut basis = Vec::with_capacity(cohort.len());
    let mut entries = Vec::with_capacity(cohort.len());
    let mut counts = ExpiryPlanCounts {
        evaluated_completed_snapshot_count: i64::try_from(cohort.len())
            .map_err(|_| BackupError::InvalidPersistedData)?,
        expire_candidate_count: 0,
        keep_latest_count: 0,
        keep_recent_count: 0,
        blocked_active_restore_count: 0,
    };
    for (index, row) in cohort.iter().copied().enumerate() {
        let snapshot_id = row.snapshot_id()?;
        let committed_at = row.committed_at();
        let blocked = blockers.contains(&snapshot_id);
        basis.push(BackupSnapshotExpiryBasisEntry::new(
            snapshot_id,
            committed_at,
            blocked,
        ));
        let recency_rank = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(BackupError::InvalidPersistedData)?;
        let decision = if recency_rank <= config.keep_latest_completed() {
            counts.keep_latest_count = counts
                .keep_latest_count
                .checked_add(1)
                .ok_or(BackupError::InvalidPersistedData)?;
            BackupSnapshotExpiryDecision::KeepLatest
        } else if committed_at > cutoff_at {
            counts.keep_recent_count = counts
                .keep_recent_count
                .checked_add(1)
                .ok_or(BackupError::InvalidPersistedData)?;
            BackupSnapshotExpiryDecision::KeepRecent
        } else if blocked {
            counts.blocked_active_restore_count = counts
                .blocked_active_restore_count
                .checked_add(1)
                .ok_or(BackupError::InvalidPersistedData)?;
            BackupSnapshotExpiryDecision::BlockedActiveRestorePlan
        } else {
            counts.expire_candidate_count = counts
                .expire_candidate_count
                .checked_add(1)
                .ok_or(BackupError::InvalidPersistedData)?;
            BackupSnapshotExpiryDecision::Expire
        };
        entries.push(BackupSnapshotExpiryPlanEntry::new(
            plan_id,
            snapshot_id,
            committed_at,
            recency_rank,
            decision,
        )?);
    }
    let basis_fingerprint = BackupSnapshotExpiryBasisFingerprint::calculate(
        backup_set_id,
        policy_revision_id,
        evaluated_at,
        &basis,
    );
    Ok(ExpiryObservation {
        basis_fingerprint,
        entries,
        counts,
    })
}

fn select_maintenance_run_columns(lock: bool) -> &'static str {
    if lock {
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, capture_operation_id,
                expiry_plan_operation_id, state, captured_snapshot_id,
                expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                expiry_planned_at, maintenance_completed_at, stale_at, created_at
         FROM backup_maintenance_runs
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE"
    } else {
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, capture_operation_id,
                expiry_plan_operation_id, state, captured_snapshot_id,
                expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                expiry_planned_at, maintenance_completed_at, stale_at, created_at
         FROM backup_maintenance_runs
         WHERE id = $1 AND owner_user_id = $2"
    }
}

async fn load_maintenance_run(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    run_id: BackupMaintenanceRunId,
) -> Result<Option<BackupMaintenanceRunRow>, BackupError> {
    sqlx::query_as::<_, BackupMaintenanceRunRow>(select_maintenance_run_columns(false))
        .bind(run_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

async fn load_maintenance_run_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    run_id: BackupMaintenanceRunId,
) -> Result<Option<BackupMaintenanceRunRow>, BackupError> {
    sqlx::query_as::<_, BackupMaintenanceRunRow>(select_maintenance_run_columns(true))
        .bind(run_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(Into::into)
}

async fn load_maintenance_run_by_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
    lock: bool,
) -> Result<Option<BackupMaintenanceRunRow>, BackupError> {
    let query = if lock {
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, capture_operation_id,
                expiry_plan_operation_id, state, captured_snapshot_id,
                expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                expiry_planned_at, maintenance_completed_at, stale_at, created_at
         FROM backup_maintenance_runs
         WHERE owner_user_id = $1 AND operation_id = $2
         FOR UPDATE"
    } else {
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, capture_operation_id,
                expiry_plan_operation_id, state, captured_snapshot_id,
                expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                expiry_planned_at, maintenance_completed_at, stale_at, created_at
         FROM backup_maintenance_runs
         WHERE owner_user_id = $1 AND operation_id = $2"
    };
    sqlx::query_as::<_, BackupMaintenanceRunRow>(query)
        .bind(owner_user_id.into_uuid())
        .bind(operation_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(Into::into)
}

async fn maintenance_has_foreign_active_expiry_plan(
    pool: &sqlx::PgPool,
    backup_set_id: BackupSetId,
    expiry_plan_operation_id: &str,
) -> Result<bool, BackupError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1
            FROM backup_snapshot_expiry_plans
            WHERE backup_set_id = $1
              AND state = 'PLANNED'
              AND operation_id <> $2
        )",
    )
    .bind(backup_set_id.into_uuid())
    .bind(expiry_plan_operation_id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn stale_maintenance_run(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    run_id: BackupMaintenanceRunId,
) -> Result<BackupMaintenanceRun, BackupError> {
    let mut transaction = pool.begin().await.map_err(MetadataError::from)?;
    let row = load_maintenance_run_for_update(&mut transaction, owner_user_id, run_id)
        .await?
        .ok_or(BackupError::NotFound)?;
    let run = row.try_into_domain()?;
    if run.state() == BackupMaintenanceRunState::Completed {
        transaction.commit().await?;
        return Err(BackupError::MaintenanceRunPreflight(
            BackupMaintenanceRunPreflightIssue::InvalidRunState,
        ));
    }
    if run.state() == BackupMaintenanceRunState::Stale {
        transaction.commit().await?;
        return Ok(run);
    }
    let row = sqlx::query_as::<_, BackupMaintenanceRunRow>(
        "UPDATE backup_maintenance_runs
         SET state = 'STALE', stale_at = clock_timestamp()
         WHERE id = $1
           AND owner_user_id = $2
           AND state IN ('CREATED', 'SNAPSHOT_CAPTURED', 'EXPIRY_PLANNED')
         RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                   policy_revision_number, operation_id, fingerprint_version,
                   request_fingerprint, capture_operation_id,
                   expiry_plan_operation_id, state, captured_snapshot_id,
                   expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                   expiry_planned_at, maintenance_completed_at, stale_at,
                   created_at",
    )
    .bind(run_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(BackupError::InvalidPersistedData)?;
    let stale = row.try_into_domain()?;
    transaction.commit().await?;
    Ok(stale)
}

async fn persist_maintenance_snapshot_progress(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    run_id: BackupMaintenanceRunId,
    snapshot_id: SnapshotId,
    captured_at: Timestamp,
) -> Result<BackupMaintenanceRun, BackupError> {
    let mut transaction = pool.begin().await.map_err(MetadataError::from)?;
    let run = load_maintenance_run_for_update(&mut transaction, owner_user_id, run_id)
        .await?
        .ok_or(BackupError::NotFound)?
        .try_into_domain()?;
    let result = match run.state() {
        BackupMaintenanceRunState::Created => {
            let row = sqlx::query_as::<_, BackupMaintenanceRunRow>(
                "UPDATE backup_maintenance_runs
                 SET state = 'SNAPSHOT_CAPTURED',
                     captured_snapshot_id = $3,
                     snapshot_captured_at = $4
                 WHERE id = $1 AND owner_user_id = $2 AND state = 'CREATED'
                 RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                           policy_revision_number, operation_id, fingerprint_version,
                           request_fingerprint, capture_operation_id,
                           expiry_plan_operation_id, state, captured_snapshot_id,
                           expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                           expiry_planned_at, maintenance_completed_at, stale_at,
                           created_at",
            )
            .bind(run_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(snapshot_id.into_uuid())
            .bind(captured_at.as_offset_datetime())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(BackupError::InvalidPersistedData)?;
            row.try_into_domain()?
        }
        BackupMaintenanceRunState::SnapshotCaptured
        | BackupMaintenanceRunState::ExpiryPlanned
        | BackupMaintenanceRunState::Completed => {
            if run.captured_snapshot_id() != Some(snapshot_id) {
                return Err(BackupError::MaintenanceRunPreflight(
                    BackupMaintenanceRunPreflightIssue::RunCorruption,
                ));
            }
            run
        }
        BackupMaintenanceRunState::Stale => {
            return Err(BackupError::MaintenanceRunPreflight(
                BackupMaintenanceRunPreflightIssue::InvalidRunState,
            ));
        }
    };
    transaction.commit().await?;
    Ok(result)
}

async fn persist_maintenance_expiry_plan_progress(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    run_id: BackupMaintenanceRunId,
    plan_id: BackupSnapshotExpiryPlanId,
    planned_at: Timestamp,
) -> Result<BackupMaintenanceRun, BackupError> {
    let mut transaction = pool.begin().await.map_err(MetadataError::from)?;
    let run = load_maintenance_run_for_update(&mut transaction, owner_user_id, run_id)
        .await?
        .ok_or(BackupError::NotFound)?
        .try_into_domain()?;
    let result = match run.state() {
        BackupMaintenanceRunState::SnapshotCaptured => {
            let row = sqlx::query_as::<_, BackupMaintenanceRunRow>(
                "UPDATE backup_maintenance_runs
                 SET state = 'EXPIRY_PLANNED',
                     expiry_plan_id = $3,
                     expiry_planned_at = $4
                 WHERE id = $1 AND owner_user_id = $2 AND state = 'SNAPSHOT_CAPTURED'
                 RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                           policy_revision_number, operation_id, fingerprint_version,
                           request_fingerprint, capture_operation_id,
                           expiry_plan_operation_id, state, captured_snapshot_id,
                           expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                           expiry_planned_at, maintenance_completed_at, stale_at,
                           created_at",
            )
            .bind(run_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(plan_id.into_uuid())
            .bind(planned_at.as_offset_datetime())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(BackupError::InvalidPersistedData)?;
            row.try_into_domain()?
        }
        BackupMaintenanceRunState::ExpiryPlanned | BackupMaintenanceRunState::Completed => {
            if run.expiry_plan_id() != Some(plan_id) {
                return Err(BackupError::MaintenanceRunPreflight(
                    BackupMaintenanceRunPreflightIssue::RunCorruption,
                ));
            }
            run
        }
        BackupMaintenanceRunState::Created | BackupMaintenanceRunState::Stale => {
            return Err(BackupError::MaintenanceRunPreflight(
                BackupMaintenanceRunPreflightIssue::InvalidRunState,
            ));
        }
    };
    transaction.commit().await?;
    Ok(result)
}

async fn persist_maintenance_completion(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    run_id: BackupMaintenanceRunId,
    execution_id: BackupSnapshotExpiryExecutionId,
    completed_at: Timestamp,
) -> Result<BackupMaintenanceRun, BackupError> {
    let mut transaction = pool.begin().await.map_err(MetadataError::from)?;
    let run = load_maintenance_run_for_update(&mut transaction, owner_user_id, run_id)
        .await?
        .ok_or(BackupError::NotFound)?
        .try_into_domain()?;
    let result = match run.state() {
        BackupMaintenanceRunState::ExpiryPlanned => {
            let row = sqlx::query_as::<_, BackupMaintenanceRunRow>(
                "UPDATE backup_maintenance_runs
                 SET state = 'COMPLETED',
                     expiry_execution_id = $3,
                     maintenance_completed_at = $4
                 WHERE id = $1 AND owner_user_id = $2 AND state = 'EXPIRY_PLANNED'
                 RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                           policy_revision_number, operation_id, fingerprint_version,
                           request_fingerprint, capture_operation_id,
                           expiry_plan_operation_id, state, captured_snapshot_id,
                           expiry_plan_id, expiry_execution_id, snapshot_captured_at,
                           expiry_planned_at, maintenance_completed_at, stale_at,
                           created_at",
            )
            .bind(run_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(execution_id.into_uuid())
            .bind(completed_at.as_offset_datetime())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(BackupError::InvalidPersistedData)?;
            row.try_into_domain()?
        }
        BackupMaintenanceRunState::Completed => {
            if run.expiry_execution_id() != Some(execution_id) {
                return Err(BackupError::MaintenanceRunPreflight(
                    BackupMaintenanceRunPreflightIssue::RunCorruption,
                ));
            }
            run
        }
        BackupMaintenanceRunState::Created
        | BackupMaintenanceRunState::SnapshotCaptured
        | BackupMaintenanceRunState::Stale => {
            return Err(BackupError::MaintenanceRunPreflight(
                BackupMaintenanceRunPreflightIssue::InvalidRunState,
            ));
        }
    };
    transaction.commit().await?;
    Ok(result)
}

async fn lock_owned_backup_set(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<(), BackupError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM backup_sets
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(BackupError::NotFound)?;
    Ok(())
}

async fn load_retention_policy_by_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
) -> Result<Option<BackupSnapshotRetentionPolicyRevisionRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotRetentionPolicyRevisionRow>(
        "SELECT id, owner_user_id, backup_set_id, revision_number, operation_id,
                fingerprint_version, request_fingerprint, keep_latest_completed,
                expire_after_seconds, created_at
         FROM backup_snapshot_retention_policy_revisions
         WHERE owner_user_id = $1 AND operation_id = $2
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(operation_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_current_retention_policy(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<Option<BackupSnapshotRetentionPolicyRevisionRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotRetentionPolicyRevisionRow>(
        "SELECT id, owner_user_id, backup_set_id, revision_number, operation_id,
                fingerprint_version, request_fingerprint, keep_latest_completed,
                expire_after_seconds, created_at
         FROM backup_snapshot_retention_policy_revisions
         WHERE owner_user_id = $1 AND backup_set_id = $2
         ORDER BY revision_number DESC
         LIMIT 1",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn load_current_retention_policy_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<Option<BackupSnapshotRetentionPolicyRevisionRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotRetentionPolicyRevisionRow>(
        "SELECT id, owner_user_id, backup_set_id, revision_number, operation_id,
                fingerprint_version, request_fingerprint, keep_latest_completed,
                expire_after_seconds, created_at
         FROM backup_snapshot_retention_policy_revisions
         WHERE owner_user_id = $1 AND backup_set_id = $2
         ORDER BY revision_number DESC
         LIMIT 1
         FOR SHARE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_expiry_plan_by_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
) -> Result<Option<BackupSnapshotExpiryPlanRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotExpiryPlanRow>(
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, evaluated_at, cutoff_at,
                snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                evaluated_completed_snapshot_count, expire_candidate_count,
                keep_latest_count, keep_recent_count, blocked_active_restore_count,
                state, created_at, stale_at
         FROM backup_snapshot_expiry_plans
         WHERE owner_user_id = $1 AND operation_id = $2
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(operation_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_expiry_plan(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    plan_id: BackupSnapshotExpiryPlanId,
) -> Result<Option<BackupSnapshotExpiryPlanRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotExpiryPlanRow>(
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, evaluated_at, cutoff_at,
                snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                evaluated_completed_snapshot_count, expire_candidate_count,
                keep_latest_count, keep_recent_count, blocked_active_restore_count,
                state, created_at, stale_at
         FROM backup_snapshot_expiry_plans
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn load_expiry_plan_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupSnapshotExpiryPlanId,
    lock: bool,
) -> Result<Option<BackupSnapshotExpiryPlanRow>, BackupError> {
    let query = if lock {
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, evaluated_at, cutoff_at,
                snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                evaluated_completed_snapshot_count, expire_candidate_count,
                keep_latest_count, keep_recent_count, blocked_active_restore_count,
                state, created_at, stale_at
         FROM backup_snapshot_expiry_plans
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE"
    } else {
        "SELECT id, owner_user_id, backup_set_id, policy_revision_id,
                policy_revision_number, operation_id, fingerprint_version,
                request_fingerprint, evaluated_at, cutoff_at,
                snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                evaluated_completed_snapshot_count, expire_candidate_count,
                keep_latest_count, keep_recent_count, blocked_active_restore_count,
                state, created_at, stale_at
         FROM backup_snapshot_expiry_plans
         WHERE id = $1 AND owner_user_id = $2"
    };
    sqlx::query_as::<_, BackupSnapshotExpiryPlanRow>(query)
        .bind(plan_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(Into::into)
}

async fn load_completed_expiry_cohort(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<Vec<ExpiryCohortRow>, BackupError> {
    sqlx::query_as::<_, ExpiryCohortRow>(
        "SELECT id AS snapshot_id, committed_at
         FROM backup_snapshots
         WHERE owner_user_id = $1
           AND backup_set_id = $2
           AND state = 'COMPLETED'
         ORDER BY committed_at DESC, id DESC
         FOR SHARE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_active_restore_blockers(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<BTreeSet<SnapshotId>, BackupError> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT restore_plan.id, restore_plan.snapshot_id
         FROM backup_restore_plans AS restore_plan
         WHERE restore_plan.owner_user_id = $1
           AND restore_plan.backup_set_id = $2
           AND restore_plan.state = 'PLANNED'
         ORDER BY restore_plan.id ASC
         FOR SHARE OF restore_plan",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(|(_, snapshot_id)| {
            decode_id(snapshot_id, "backup_restore_plans.snapshot_id").map_err(Into::into)
        })
        .collect()
}

async fn load_all_expiry_plan_entries(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: BackupSnapshotExpiryPlanId,
) -> Result<Vec<BackupSnapshotExpiryPlanEntry>, BackupError> {
    let rows = sqlx::query_as::<_, BackupSnapshotExpiryPlanEntryRow>(
        "SELECT plan_id, snapshot_id, committed_at, recency_rank, decision
         FROM backup_snapshot_expiry_plan_entries
         WHERE plan_id = $1
         ORDER BY recency_rank ASC",
    )
    .bind(plan_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(BackupSnapshotExpiryPlanEntryRow::try_into_domain)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

async fn verify_expiry_plan_entry_counts(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: BackupSnapshotExpiryPlanId,
    expected: ExpiryPlanCounts,
) -> Result<(), BackupError> {
    let persisted = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        "SELECT count(*)::BIGINT,
                count(*) FILTER (WHERE decision = 'EXPIRE')::BIGINT,
                count(*) FILTER (WHERE decision = 'KEEP_LATEST')::BIGINT,
                count(*) FILTER (WHERE decision = 'KEEP_RECENT')::BIGINT,
                count(*) FILTER (
                    WHERE decision = 'BLOCKED_ACTIVE_RESTORE_PLAN'
                )::BIGINT
         FROM backup_snapshot_expiry_plan_entries
         WHERE plan_id = $1",
    )
    .bind(plan_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    if persisted
        != (
            expected.evaluated_completed_snapshot_count,
            expected.expire_candidate_count,
            expected.keep_latest_count,
            expected.keep_recent_count,
            expected.blocked_active_restore_count,
        )
    {
        return Err(BackupError::InvalidPersistedData);
    }
    Ok(())
}

async fn stale_expiry_plan(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupSnapshotExpiryPlanId,
) -> Result<BackupSnapshotExpiryPlan, BackupError> {
    sqlx::query_as::<_, BackupSnapshotExpiryPlanRow>(
        "UPDATE backup_snapshot_expiry_plans
         SET state = 'STALE', stale_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'
         RETURNING id, owner_user_id, backup_set_id, policy_revision_id,
                   policy_revision_number, operation_id, fingerprint_version,
                   request_fingerprint, evaluated_at, cutoff_at,
                   snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
                   evaluated_completed_snapshot_count, expire_candidate_count,
                   keep_latest_count, keep_recent_count, blocked_active_restore_count,
                   state, created_at, stale_at",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(BackupError::InvalidPersistedData)?
    .try_into_domain()
    .map_err(Into::into)
}

async fn load_expiry_execution_for_plan(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupSnapshotExpiryPlanId,
) -> Result<Option<BackupSnapshotExpiryExecutionRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotExpiryExecutionRow>(
        "SELECT id, owner_user_id, expiry_plan_id, backup_set_id,
                policy_revision_id, evaluated_at, evaluated_snapshot_count,
                expired_snapshot_count, unchanged_snapshot_count,
                state, executed_at
         FROM backup_snapshot_expiry_executions
         WHERE expiry_plan_id = $1 AND owner_user_id = $2
           AND state = 'COMMITTED'",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

/// Verify that every planned EXPIRE snapshot still resolves to a COMPLETED
/// snapshot under the exact owner/backup-set scope, and lock those rows in
/// deterministic plan-entry order. Returns the canonical snapshot UUIDs to
/// transition in the same deterministic order. A missing or non-COMPLETED
/// snapshot means the cohort basis changed and the caller must fail closed.
async fn verify_snapshots_still_completed_for_expiry(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    expire_snapshot_ids: &[SnapshotId],
) -> Result<Vec<Uuid>, BackupError> {
    if expire_snapshot_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT snapshot.id, snapshot.state
         FROM backup_snapshots AS snapshot
         INNER JOIN backup_sets AS backup_set
            ON backup_set.id = snapshot.backup_set_id
           AND backup_set.owner_user_id = snapshot.owner_user_id
         WHERE snapshot.backup_set_id = $1
           AND snapshot.owner_user_id = $2
           AND snapshot.id = ANY($3)
           AND snapshot.state = 'COMPLETED'
         ORDER BY snapshot.id DESC
         FOR UPDATE OF snapshot",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(
        expire_snapshot_ids
            .iter()
            .map(|id| id.into_uuid())
            .collect::<Vec<_>>(),
    )
    .fetch_all(&mut **transaction)
    .await?;
    // Ordering by id DESC for locking; the caller transitions in the plan
    // order. A row count mismatch means a snapshot is missing or non-COMPLETED.
    if rows.len() != expire_snapshot_ids.len() {
        return Ok(Vec::new());
    }
    let mut result = Vec::with_capacity(rows.len());
    for (id, state) in rows {
        if state != "COMPLETED" {
            return Ok(Vec::new());
        }
        if SnapshotId::try_from_uuid(id).is_err() {
            return Ok(Vec::new());
        }
        result.push(id);
    }
    Ok(result)
}

#[derive(Clone, Debug, FromRow)]
struct RestoreTargetLibraryRow {
    journal_epoch: i64,
    sync_head: i64,
    status: String,
}

#[derive(Clone, Debug, FromRow)]
struct RestoreTargetParentRow {
    kind: String,
    state: String,
}

struct RestoreTargetObservation {
    journal_epoch: i64,
    sync_head: i64,
    parent_is_valid: bool,
    destination_exists: bool,
}

#[derive(Clone, Debug, FromRow)]
struct ExecutionTargetLibraryRow {
    id: Uuid,
    owner_user_id: Uuid,
    name: String,
    root_node_id: Uuid,
    dedup_domain_id: Uuid,
    status: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    revision: String,
    journal_epoch: i64,
    sync_head: i64,
}

impl ExecutionTargetLibraryRow {
    fn library_row(&self) -> LibraryRow {
        LibraryRow {
            id: self.id,
            owner_user_id: self.owner_user_id,
            name: self.name.clone(),
            root_node_id: self.root_node_id,
            dedup_domain_id: self.dedup_domain_id,
            status: self.status.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            revision: self.revision.clone(),
        }
    }
}

struct ExecutionTargetObservation {
    library: Library,
    parent: Node,
    journal_epoch: i64,
    sync_head: i64,
    parent_is_valid: bool,
    destination_exists: bool,
}

#[derive(Clone, Debug, FromRow)]
struct RetainedRestoreContentRow {
    manifest_node_id: Uuid,
    manifest_file_version_id: Uuid,
    pin_file_version_id: Option<Uuid>,
    pin_object_id: Option<Uuid>,
    pin_object_dedup_domain_id: Option<Uuid>,
    object_lifecycle_state: Option<String>,
    object_content_length: Option<String>,
    object_content_sha256: Option<Vec<u8>>,
    has_verified_replica: bool,
}

#[derive(Clone, Copy)]
struct RetainedRestoreContent {
    object: ObjectReference,
}

#[derive(Clone, Debug)]
struct PruneSourceContent {
    source_snapshot_node_id: NodeId,
    content: BackupManifestContent,
}

#[derive(Clone, Debug)]
struct PruneSourceObservation {
    snapshot_manifest_item_count: i64,
    snapshot_content_reference_count: i64,
    contents: Vec<PruneSourceContent>,
}

impl PruneSourceObservation {
    fn logical_entries(
        &self,
        plan_id: BackupPrunePlanId,
    ) -> Result<Vec<BackupPrunePlanEntry>, BackupError> {
        self.contents
            .iter()
            .enumerate()
            .map(|(ordinal, content)| {
                Ok(BackupPrunePlanEntry::new(
                    plan_id,
                    u64::try_from(ordinal).map_err(|_| BackupError::InvalidRequest)?,
                    content.source_snapshot_node_id,
                    content.content,
                ))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PruneObjectImpact {
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    target_snapshot_pin_count: i64,
    surviving_live_file_version_reference_count: i64,
    surviving_other_snapshot_pin_count: i64,
    predicted_post_release_reference_count: i64,
    impact: BackupPruneImpact,
}

#[derive(Clone, Copy, Debug)]
struct PrunePlanCounts {
    snapshot_manifest_item_count: i64,
    snapshot_content_reference_count: i64,
    planned_pin_release_count: i64,
    distinct_retained_content_count: i64,
    retained_after_release_count: i64,
    would_become_unreferenced_count: i64,
}

impl PrunePlanCounts {
    fn from_source_and_impacts(
        source: &PruneSourceObservation,
        impacts: &[PruneObjectImpact],
    ) -> Result<Self, BackupError> {
        let planned_pin_release_count =
            i64::try_from(source.contents.len()).map_err(|_| BackupError::InvalidPersistedData)?;
        let distinct_retained_content_count =
            i64::try_from(impacts.len()).map_err(|_| BackupError::InvalidPersistedData)?;
        let retained_after_release_count = i64::try_from(
            impacts
                .iter()
                .filter(|impact| impact.impact == BackupPruneImpact::RetainedByOtherReference)
                .count(),
        )
        .map_err(|_| BackupError::InvalidPersistedData)?;
        let would_become_unreferenced_count = i64::try_from(
            impacts
                .iter()
                .filter(|impact| impact.impact == BackupPruneImpact::WouldBecomeUnreferenced)
                .count(),
        )
        .map_err(|_| BackupError::InvalidPersistedData)?;
        if source.snapshot_content_reference_count != planned_pin_release_count
            || retained_after_release_count + would_become_unreferenced_count
                != distinct_retained_content_count
            || distinct_retained_content_count > planned_pin_release_count
        {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::RetentionCorrupt,
            ));
        }
        Ok(Self {
            snapshot_manifest_item_count: source.snapshot_manifest_item_count,
            snapshot_content_reference_count: source.snapshot_content_reference_count,
            planned_pin_release_count,
            distinct_retained_content_count,
            retained_after_release_count,
            would_become_unreferenced_count,
        })
    }

    fn matches_plan(&self, plan: &BackupPrunePlan) -> bool {
        matches!(
            (
                i64::try_from(plan.snapshot_manifest_item_count()),
                i64::try_from(plan.snapshot_content_reference_count()),
                i64::try_from(plan.planned_pin_release_count()),
                i64::try_from(plan.distinct_retained_content_count()),
                i64::try_from(plan.retained_after_release_count()),
                i64::try_from(plan.would_become_unreferenced_count()),
            ),
            (Ok(manifest), Ok(content), Ok(pins), Ok(distinct), Ok(retained), Ok(unreferenced))
                if manifest == self.snapshot_manifest_item_count
                    && content == self.snapshot_content_reference_count
                    && pins == self.planned_pin_release_count
                    && distinct == self.distinct_retained_content_count
                    && retained == self.retained_after_release_count
                    && unreferenced == self.would_become_unreferenced_count
        )
    }
}

#[derive(Clone, Debug, FromRow)]
struct PruneSourceContentRow {
    manifest_node_id: Uuid,
    manifest_file_version_id: Uuid,
    manifest_content_length: String,
    manifest_content_sha256: Vec<u8>,
    pin_manifest_node_id: Option<Uuid>,
    pin_file_version_id: Option<Uuid>,
    pin_object_id: Option<Uuid>,
    pin_object_dedup_domain_id: Option<Uuid>,
    object_id: Option<Uuid>,
    object_dedup_domain_id: Option<Uuid>,
    object_lifecycle_state: Option<String>,
    object_content_length: Option<String>,
    object_content_sha256: Option<Vec<u8>>,
}

#[derive(Clone, Debug, FromRow)]
struct PruneObjectImpactRow {
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    target_snapshot_pin_count: i64,
    surviving_live_file_version_reference_count: i64,
    surviving_other_snapshot_pin_count: i64,
    predicted_post_release_reference_count: Option<i64>,
    impact: Option<String>,
}

impl PruneObjectImpactRow {
    fn into_observed_impact(self) -> Result<PruneObjectImpact, BackupError> {
        decode_id::<synveil_core::ObjectId>(
            self.object_id,
            "backup_prune_plan_object_impacts.object_id",
        )
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt))?;
        decode_id::<synveil_core::DedupDomainId>(
            self.object_dedup_domain_id,
            "backup_prune_plan_object_impacts.object_dedup_domain_id",
        )
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt))?;
        let post_release =
            self.predicted_post_release_reference_count
                .ok_or(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::RetentionCorrupt,
                ))?;
        if self.target_snapshot_pin_count <= 0
            || self.surviving_live_file_version_reference_count < 0
            || self.surviving_other_snapshot_pin_count < 0
            || post_release < 0
            || self
                .surviving_live_file_version_reference_count
                .checked_add(self.surviving_other_snapshot_pin_count)
                != Some(post_release)
        {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::RetentionCorrupt,
            ));
        }
        let impact = match self.impact {
            Some(impact) => BackupPruneImpact::from_str(&impact).map_err(|_| {
                BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt)
            })?,
            None => {
                if post_release > 0 {
                    BackupPruneImpact::RetainedByOtherReference
                } else {
                    BackupPruneImpact::WouldBecomeUnreferenced
                }
            }
        };
        let expected = if post_release > 0 {
            BackupPruneImpact::RetainedByOtherReference
        } else {
            BackupPruneImpact::WouldBecomeUnreferenced
        };
        if impact != expected {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::RetentionCorrupt,
            ));
        }
        Ok(PruneObjectImpact {
            object_id: self.object_id,
            object_dedup_domain_id: self.object_dedup_domain_id,
            target_snapshot_pin_count: self.target_snapshot_pin_count,
            surviving_live_file_version_reference_count: self
                .surviving_live_file_version_reference_count,
            surviving_other_snapshot_pin_count: self.surviving_other_snapshot_pin_count,
            predicted_post_release_reference_count: post_release,
            impact,
        })
    }
}

async fn load_prune_plan(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    plan_id: BackupPrunePlanId,
) -> Result<Option<BackupPrunePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupPrunePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                fingerprint_version, request_fingerprint,
                snapshot_manifest_item_count, snapshot_content_reference_count,
                planned_pin_release_count, distinct_retained_content_count,
                retained_after_release_count, would_become_unreferenced_count,
                state, created_at, stale_at
         FROM backup_prune_plans
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn load_prune_plan_for_read(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupPrunePlanId,
) -> Result<Option<BackupPrunePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupPrunePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                fingerprint_version, request_fingerprint,
                snapshot_manifest_item_count, snapshot_content_reference_count,
                planned_pin_release_count, distinct_retained_content_count,
                retained_after_release_count, would_become_unreferenced_count,
                state, created_at, stale_at
         FROM backup_prune_plans
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_prune_plan_by_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
) -> Result<Option<BackupPrunePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupPrunePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                fingerprint_version, request_fingerprint,
                snapshot_manifest_item_count, snapshot_content_reference_count,
                planned_pin_release_count, distinct_retained_content_count,
                retained_after_release_count, would_become_unreferenced_count,
                state, created_at, stale_at
         FROM backup_prune_plans
         WHERE owner_user_id = $1 AND operation_id = $2
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(operation_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_prune_plan_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupPrunePlanId,
) -> Result<Option<BackupPrunePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupPrunePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                fingerprint_version, request_fingerprint,
                snapshot_manifest_item_count, snapshot_content_reference_count,
                planned_pin_release_count, distinct_retained_content_count,
                retained_after_release_count, would_become_unreferenced_count,
                state, created_at, stale_at
         FROM backup_prune_plans
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_active_prune_plan_for_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot_id: SnapshotId,
) -> Result<Option<BackupPrunePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupPrunePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                fingerprint_version, request_fingerprint,
                snapshot_manifest_item_count, snapshot_content_reference_count,
                planned_pin_release_count, distinct_retained_content_count,
                retained_after_release_count, would_become_unreferenced_count,
                state, created_at, stale_at
         FROM backup_prune_plans
         WHERE snapshot_id = $1 AND state = 'PLANNED'
         FOR SHARE",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_prune_snapshot_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
) -> Result<Option<BackupSnapshotRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotRow>(
        "SELECT snapshot.id, snapshot.backup_set_id, snapshot.owner_user_id,
                snapshot.source_library_id, snapshot.operation_id,
                snapshot.snapshot_epoch, snapshot.snapshot_resume_sequence,
                snapshot.manifest_item_count, snapshot.terminal_node_id,
                snapshot.content_reference_count, snapshot.state,
                snapshot.created_at, snapshot.committed_at, snapshot.expired_at
         FROM backup_snapshots AS snapshot
         INNER JOIN backup_sets AS backup_set
            ON backup_set.id = snapshot.backup_set_id
           AND backup_set.owner_user_id = snapshot.owner_user_id
         WHERE snapshot.id = $1
           AND snapshot.backup_set_id = $2
           AND snapshot.owner_user_id = $3
           AND backup_set.owner_user_id = $3
         FOR UPDATE OF snapshot",
    )
    .bind(snapshot_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn validate_prune_snapshot_set(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    snapshot: &BackupSnapshot,
) -> Result<(), BackupError> {
    let source_library_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT source_library_id
         FROM backup_sets
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(snapshot.backup_set_id().into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(BackupError::PrunePreflight(
        BackupPrunePreflightIssue::SnapshotCorrupt,
    ))?;
    if source_library_id != snapshot.source_library_id().into_uuid() {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotCorrupt,
        ));
    }
    Ok(())
}

async fn collect_prune_source_observation(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot: &BackupSnapshot,
) -> Result<PruneSourceObservation, BackupError> {
    if snapshot.state() != SnapshotState::Expired {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotNotExpired,
        ));
    }
    let manifest_rows = load_all_snapshot_nodes(transaction, snapshot.id()).await?;
    let manifest_nodes = manifest_rows
        .into_iter()
        .map(BackupSnapshotNodeRow::try_into_domain)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::SnapshotCorrupt))?;
    validate_expired_snapshot_tree(snapshot, &manifest_nodes)?;
    let contents = load_prune_source_contents(transaction, snapshot).await?;
    let manifest_count =
        i64::try_from(manifest_nodes.len()).map_err(|_| BackupError::InvalidPersistedData)?;
    let content_count =
        i64::try_from(contents.len()).map_err(|_| BackupError::InvalidPersistedData)?;
    let expected_manifest = i64::try_from(snapshot.manifest_item_count())
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::SnapshotCorrupt))?;
    let expected_content = i64::try_from(snapshot.content_reference_count())
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::SnapshotCorrupt))?;
    if manifest_count != expected_manifest || content_count != expected_content {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotCorrupt,
        ));
    }
    Ok(PruneSourceObservation {
        snapshot_manifest_item_count: manifest_count,
        snapshot_content_reference_count: content_count,
        contents,
    })
}

fn validate_expired_snapshot_tree(
    snapshot: &BackupSnapshot,
    nodes: &[BackupSnapshotNode],
) -> Result<(), BackupError> {
    let manifest_count =
        u64::try_from(nodes.len()).map_err(|_| BackupError::InvalidPersistedData)?;
    let content_count = u64::try_from(nodes.iter().filter(|node| node.content().is_some()).count())
        .map_err(|_| BackupError::InvalidPersistedData)?;
    if snapshot.state() != SnapshotState::Expired
        || snapshot.committed_at().is_none()
        || snapshot.expired_at().is_none()
        || manifest_count != snapshot.manifest_item_count()
        || content_count != snapshot.content_reference_count()
        || nodes.is_empty()
    {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotCorrupt,
        ));
    }

    let mut by_id = HashMap::with_capacity(nodes.len());
    for node in nodes {
        if by_id.insert(node.node_id(), node).is_some() {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::SnapshotCorrupt,
            ));
        }
        if (node.kind() == NodeKind::Directory && node.content().is_some())
            || (node.kind() == NodeKind::File && node.content().is_none())
        {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::SnapshotCorrupt,
            ));
        }
    }
    if snapshot
        .terminal_node_id()
        .is_none_or(|terminal_id| !by_id.contains_key(&terminal_id))
    {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotCorrupt,
        ));
    }
    let roots = nodes
        .iter()
        .filter(|node| node.parent_node_id().is_none())
        .collect::<Vec<_>>();
    if roots.len() != 1
        || roots[0].kind() != NodeKind::Directory
        || roots[0].state() != NodeState::Active
    {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotCorrupt,
        ));
    }
    for node in nodes {
        let mut current = node;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.node_id()) {
                return Err(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::SnapshotCorrupt,
                ));
            }
            let Some(parent_id) = current.parent_node_id() else {
                break;
            };
            let Some(parent) = by_id.get(&parent_id) else {
                return Err(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::SnapshotCorrupt,
                ));
            };
            if parent.kind() != NodeKind::Directory {
                return Err(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::SnapshotCorrupt,
                ));
            }
            current = parent;
        }
    }
    Ok(())
}

async fn load_prune_source_contents(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot: &BackupSnapshot,
) -> Result<Vec<PruneSourceContent>, BackupError> {
    let expected_content_count = i64::try_from(snapshot.content_reference_count())
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt))?;
    let pin_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    let unexpected_pin_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM backup_snapshot_content_pins AS pin
         LEFT JOIN backup_snapshot_nodes AS manifest
           ON manifest.snapshot_id = pin.snapshot_id
          AND manifest.node_id = pin.manifest_node_id
         WHERE pin.snapshot_id = $1
           AND (
               manifest.node_id IS NULL
               OR manifest.kind <> 'FILE'
               OR manifest.current_version_id IS NULL
               OR pin.file_version_id <> manifest.current_version_id
           )",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    if pin_count != expected_content_count || unexpected_pin_count != 0 {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::RetentionCorrupt,
        ));
    }

    let rows = sqlx::query_as::<_, PruneSourceContentRow>(
        "SELECT manifest.node_id AS manifest_node_id,
                manifest.current_version_id AS manifest_file_version_id,
                manifest.content_length::TEXT AS manifest_content_length,
                manifest.content_sha256 AS manifest_content_sha256,
                pin.manifest_node_id AS pin_manifest_node_id,
                pin.file_version_id AS pin_file_version_id,
                pin.object_id AS pin_object_id,
                pin.object_dedup_domain_id AS pin_object_dedup_domain_id,
                object.id AS object_id,
                object.dedup_domain_id AS object_dedup_domain_id,
                object.lifecycle_state AS object_lifecycle_state,
                object.plaintext_length::TEXT AS object_content_length,
                object.canonical_hash AS object_content_sha256
         FROM backup_snapshot_nodes AS manifest
         LEFT JOIN backup_snapshot_content_pins AS pin
           ON pin.snapshot_id = manifest.snapshot_id
          AND pin.manifest_node_id = manifest.node_id
         LEFT JOIN objects AS object
           ON object.id = pin.object_id
          AND object.dedup_domain_id = pin.object_dedup_domain_id
         WHERE manifest.snapshot_id = $1
           AND manifest.current_version_id IS NOT NULL
         ORDER BY manifest.node_id ASC",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    if i64::try_from(rows.len()).map_err(|_| BackupError::InvalidPersistedData)?
        != expected_content_count
    {
        return Err(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::RetentionCorrupt,
        ));
    }

    let mut source_nodes = BTreeSet::new();
    let mut contents = Vec::with_capacity(rows.len());
    for row in rows {
        let pin_manifest_node_id = row.pin_manifest_node_id.ok_or(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::RetentionCorrupt,
        ))?;
        let pin_file_version_id = row.pin_file_version_id.ok_or(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::RetentionCorrupt,
        ))?;
        let pin_object_id = row.pin_object_id.ok_or(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::RetentionCorrupt,
        ))?;
        let pin_object_dedup_domain_id =
            row.pin_object_dedup_domain_id
                .ok_or(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::RetentionCorrupt,
                ))?;
        let object_id = row.object_id.ok_or(BackupError::PrunePreflight(
            BackupPrunePreflightIssue::RetentionCorrupt,
        ))?;
        let object_dedup_domain_id =
            row.object_dedup_domain_id
                .ok_or(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::RetentionCorrupt,
                ))?;
        let object_content_length =
            row.object_content_length
                .ok_or(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::RetentionCorrupt,
                ))?;
        let object_content_sha256 =
            row.object_content_sha256
                .ok_or(BackupError::PrunePreflight(
                    BackupPrunePreflightIssue::RetentionCorrupt,
                ))?;
        if pin_manifest_node_id != row.manifest_node_id
            || pin_file_version_id != row.manifest_file_version_id
            || object_id != pin_object_id
            || object_dedup_domain_id != pin_object_dedup_domain_id
            || row.object_lifecycle_state.as_deref() != Some("AVAILABLE")
            || object_content_length != row.manifest_content_length
            || object_content_sha256 != row.manifest_content_sha256
        {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::RetentionCorrupt,
            ));
        }
        let source_snapshot_node_id =
            decode_id(row.manifest_node_id, "backup_snapshot_nodes.node_id").map_err(|_| {
                BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt)
            })?;
        let file_version_id = decode_id(
            row.manifest_file_version_id,
            "backup_snapshot_nodes.current_version_id",
        )
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt))?;
        decode_id::<synveil_core::ObjectId>(object_id, "backup_snapshot_content_pins.object_id")
            .map_err(|_| {
                BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt)
            })?;
        decode_id::<synveil_core::DedupDomainId>(
            object_dedup_domain_id,
            "backup_snapshot_content_pins.object_dedup_domain_id",
        )
        .map_err(|_| BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt))?;
        if !source_nodes.insert(source_snapshot_node_id) {
            return Err(BackupError::PrunePreflight(
                BackupPrunePreflightIssue::RetentionCorrupt,
            ));
        }
        let byte_length = u64::from_str(&row.manifest_content_length).map_err(|_| {
            BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt)
        })?;
        let sha256 =
            Sha256Digest::try_from(row.manifest_content_sha256.as_slice()).map_err(|_| {
                BackupError::PrunePreflight(BackupPrunePreflightIssue::RetentionCorrupt)
            })?;
        contents.push(PruneSourceContent {
            source_snapshot_node_id,
            content: BackupManifestContent::new(file_version_id, byte_length, sha256),
        });
    }
    Ok(contents)
}

/// The prune planner intentionally does not lock or mutate GC candidates. It
/// locks only canonical Object rows in the existing stable identity order,
/// after source snapshot/pin validation. Reference creators that may affect
/// counts lock these Object rows before their reference change commits.
async fn lock_prune_source_objects(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot_id: SnapshotId,
) -> Result<(), BackupError> {
    sqlx::query(
        "SELECT object.id, object.dedup_domain_id
         FROM objects AS object
         WHERE EXISTS (
             SELECT 1
             FROM backup_snapshot_content_pins AS pin
             WHERE pin.snapshot_id = $1
               AND pin.object_id = object.id
               AND pin.object_dedup_domain_id = object.dedup_domain_id
         )
         ORDER BY object.id ASC, object.dedup_domain_id ASC
         FOR UPDATE OF object",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Lock existing GC candidate rows for the affected Objects before locking the
/// canonical Object rows, preserving the repository's candidate-first order
/// used by purge, GC workers, and reference creation. The prune executor needs
/// candidate-row access for its GC handoff, so it must not invert this order
/// and risk a deadlock with a worker that holds a candidate and waits on the
/// same Object. For planning and validation (which never mutate candidates)
/// the Object-only order remains unchanged.
async fn lock_prune_candidate_rows(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot_id: SnapshotId,
) -> Result<(), BackupError> {
    sqlx::query(
        "SELECT candidate.object_id, candidate.object_dedup_domain_id
         FROM object_gc_candidates AS candidate
         WHERE EXISTS (
             SELECT 1
             FROM backup_snapshot_content_pins AS pin
             WHERE pin.snapshot_id = $1
               AND pin.object_id = candidate.object_id
               AND pin.object_dedup_domain_id = candidate.object_dedup_domain_id
         )
         ORDER BY candidate.object_id ASC, candidate.object_dedup_domain_id ASC
         FOR UPDATE OF candidate",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn observe_prune_object_impacts(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot_id: SnapshotId,
) -> Result<Vec<PruneObjectImpact>, BackupError> {
    let rows = sqlx::query_as::<_, PruneObjectImpactRow>(
        "WITH target_objects AS (
             SELECT object_id, object_dedup_domain_id,
                    count(*)::BIGINT AS target_snapshot_pin_count
             FROM backup_snapshot_content_pins
             WHERE snapshot_id = $1
             GROUP BY object_id, object_dedup_domain_id
         ), accounting AS (
             SELECT target.object_id, target.object_dedup_domain_id,
                    target.target_snapshot_pin_count,
                    (SELECT count(*)::BIGINT
                     FROM file_versions AS version
                     WHERE version.object_id = target.object_id
                       AND version.object_dedup_domain_id = target.object_dedup_domain_id)
                        AS surviving_live_file_version_reference_count,
                    (SELECT count(*)::BIGINT
                     FROM backup_snapshot_content_pins AS other_pin
                     WHERE other_pin.object_id = target.object_id
                       AND other_pin.object_dedup_domain_id = target.object_dedup_domain_id
                       AND other_pin.snapshot_id <> $1)
                        AS surviving_other_snapshot_pin_count
             FROM target_objects AS target
         )
         SELECT object_id, object_dedup_domain_id, target_snapshot_pin_count,
                surviving_live_file_version_reference_count,
                surviving_other_snapshot_pin_count,
                surviving_live_file_version_reference_count
                    + surviving_other_snapshot_pin_count
                    AS predicted_post_release_reference_count,
                CASE WHEN surviving_live_file_version_reference_count
                              + surviving_other_snapshot_pin_count > 0
                     THEN 'RETAINED_BY_OTHER_REFERENCE'
                     ELSE 'WOULD_BECOME_UNREFERENCED'
                END AS impact
         FROM accounting
         ORDER BY object_id ASC, object_dedup_domain_id ASC",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(PruneObjectImpactRow::into_observed_impact)
        .collect()
}

async fn verify_prune_plan_persisted_counts(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: BackupPrunePlanId,
    snapshot_id: SnapshotId,
    expected: &PrunePlanCounts,
) -> Result<(), BackupError> {
    let source_pin_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    let source_distinct_object_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM (
             SELECT DISTINCT object_id, object_dedup_domain_id
             FROM backup_snapshot_content_pins
             WHERE snapshot_id = $1
         ) AS source_objects",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    let persisted = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT count(*)::BIGINT,
                (SELECT count(*)::BIGINT
                 FROM backup_prune_plan_object_impacts
                 WHERE plan_id = $1),
                (SELECT count(*)::BIGINT
                 FROM backup_prune_plan_object_impacts
                 WHERE plan_id = $1
                   AND impact = 'RETAINED_BY_OTHER_REFERENCE'),
                (SELECT count(*)::BIGINT
                 FROM backup_prune_plan_object_impacts
                 WHERE plan_id = $1
                   AND impact = 'WOULD_BECOME_UNREFERENCED')
         FROM backup_prune_plan_entries
         WHERE plan_id = $1",
    )
    .bind(plan_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    if source_pin_count != expected.planned_pin_release_count
        || source_distinct_object_count != expected.distinct_retained_content_count
        || persisted
            != (
                expected.planned_pin_release_count,
                expected.distinct_retained_content_count,
                expected.retained_after_release_count,
                expected.would_become_unreferenced_count,
            )
    {
        return Err(BackupError::InvalidPersistedData);
    }
    Ok(())
}

async fn load_all_prune_plan_entries(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: BackupPrunePlanId,
) -> Result<Vec<BackupPrunePlanEntry>, BackupError> {
    let rows = sqlx::query_as::<_, BackupPrunePlanEntryRow>(
        "SELECT plan_id, ordinal, source_snapshot_node_id, file_version_id,
                content_length::TEXT AS content_length, content_sha256
         FROM backup_prune_plan_entries
         WHERE plan_id = $1
         ORDER BY ordinal ASC",
    )
    .bind(plan_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(BackupPrunePlanEntryRow::try_into_domain)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

async fn load_all_prune_plan_object_impacts(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: BackupPrunePlanId,
) -> Result<Vec<PruneObjectImpact>, BackupError> {
    let rows = sqlx::query_as::<_, PruneObjectImpactRow>(
        "SELECT object_id, object_dedup_domain_id, target_snapshot_pin_count,
                surviving_live_file_version_reference_count,
                surviving_other_snapshot_pin_count,
                predicted_post_release_reference_count, impact
         FROM backup_prune_plan_object_impacts
         WHERE plan_id = $1
         ORDER BY object_id ASC, object_dedup_domain_id ASC",
    )
    .bind(plan_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(PruneObjectImpactRow::into_observed_impact)
        .collect()
}

fn prune_plan_matches_source(plan: &BackupPrunePlan, source: &PruneSourceObservation) -> bool {
    i64::try_from(plan.snapshot_manifest_item_count())
        .ok()
        .is_some_and(|count| count == source.snapshot_manifest_item_count)
        && i64::try_from(plan.snapshot_content_reference_count())
            .ok()
            .is_some_and(|count| count == source.snapshot_content_reference_count)
        && plan.planned_pin_release_count() == plan.snapshot_content_reference_count()
}

fn prune_plan_evidence_matches(
    plan: &BackupPrunePlan,
    source: &PruneSourceObservation,
    entries: &[BackupPrunePlanEntry],
    current_impacts: &[PruneObjectImpact],
    stored_impacts: &[PruneObjectImpact],
) -> bool {
    let Ok(expected_entries) = source.logical_entries(plan.id()) else {
        return false;
    };
    let Ok(counts) = PrunePlanCounts::from_source_and_impacts(source, current_impacts) else {
        return false;
    };
    entries == expected_entries && stored_impacts == current_impacts && counts.matches_plan(plan)
}

fn prune_observation_error_makes_plan_stale(error: &BackupError) -> bool {
    matches!(
        error,
        BackupError::PrunePreflight(_) | BackupError::InvalidPersistedData
    )
}

async fn stale_prune_plan(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupPrunePlanId,
) -> Result<BackupPrunePlan, BackupError> {
    let row = sqlx::query_as::<_, BackupPrunePlanRow>(
        "UPDATE backup_prune_plans
         SET state = 'STALE', stale_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'
         RETURNING id, owner_user_id, backup_set_id, snapshot_id, operation_id,
                   fingerprint_version, request_fingerprint,
                   snapshot_manifest_item_count, snapshot_content_reference_count,
                   planned_pin_release_count, distinct_retained_content_count,
                   retained_after_release_count, would_become_unreferenced_count,
                   state, created_at, stale_at",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(BackupError::InvalidPersistedData)?;
    row.try_into_domain().map_err(Into::into)
}

async fn load_restore_plan(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    plan_id: BackupRestorePlanId,
) -> Result<Option<BackupRestorePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupRestorePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id,
                target_library_id, target_parent_node_id, operation_id,
                fingerprint_version, request_fingerprint, destination_name,
                base_journal_epoch, base_journal_head, item_count,
                content_item_count, state, created_at, stale_at
         FROM backup_restore_plans
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn load_restore_plan_by_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
    _lock: bool,
) -> Result<Option<BackupRestorePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupRestorePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id,
                target_library_id, target_parent_node_id, operation_id,
                fingerprint_version, request_fingerprint, destination_name,
                base_journal_epoch, base_journal_head, item_count,
                content_item_count, state, created_at, stale_at
         FROM backup_restore_plans
         WHERE owner_user_id = $1 AND operation_id = $2
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(operation_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_restore_plan_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupRestorePlanId,
) -> Result<Option<BackupRestorePlanRow>, BackupError> {
    sqlx::query_as::<_, BackupRestorePlanRow>(
        "SELECT id, owner_user_id, backup_set_id, snapshot_id,
                target_library_id, target_parent_node_id, operation_id,
                fingerprint_version, request_fingerprint, destination_name,
                base_journal_epoch, base_journal_head, item_count,
                content_item_count, state, created_at, stale_at
         FROM backup_restore_plans
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_restore_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
) -> Result<Option<BackupSnapshotRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotRow>(
        "SELECT snapshot.id, snapshot.backup_set_id, snapshot.owner_user_id,
                snapshot.source_library_id, snapshot.operation_id,
                snapshot.snapshot_epoch, snapshot.snapshot_resume_sequence,
                snapshot.manifest_item_count, snapshot.terminal_node_id,
                snapshot.content_reference_count, snapshot.state,
                snapshot.created_at, snapshot.committed_at, snapshot.expired_at
         FROM backup_snapshots AS snapshot
         INNER JOIN backup_sets AS backup_set
            ON backup_set.id = snapshot.backup_set_id
           AND backup_set.owner_user_id = snapshot.owner_user_id
         WHERE snapshot.id = $1
           AND snapshot.backup_set_id = $2
           AND snapshot.owner_user_id = $3
           AND backup_set.owner_user_id = $3
         FOR SHARE OF snapshot",
    )
    .bind(snapshot_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_all_restore_plan_entries(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: BackupRestorePlanId,
) -> Result<Vec<BackupRestorePlanEntryRow>, BackupError> {
    sqlx::query_as::<_, BackupRestorePlanEntryRow>(
        "SELECT plan_id, ordinal, planned_node_id, planned_parent_node_id,
                source_snapshot_node_id, source_parent_node_id, source_state,
                action, kind, name, source_revision::TEXT AS source_revision,
                file_version_id, content_length::TEXT AS content_length,
                content_sha256
         FROM backup_restore_plan_entries
         WHERE plan_id = $1
         ORDER BY ordinal ASC",
    )
    .bind(plan_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_restore_execution_for_plan(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupRestorePlanId,
) -> Result<Option<BackupRestoreExecutionRow>, BackupError> {
    sqlx::query_as::<_, BackupRestoreExecutionRow>(
        "SELECT id, owner_user_id, restore_plan_id, target_library_id,
                journal_first_sequence, journal_last_sequence,
                created_node_count, created_file_version_count, state,
                executed_at
         FROM backup_restore_executions
         WHERE restore_plan_id = $1
           AND owner_user_id = $2
           AND state = 'COMMITTED'
         FOR SHARE",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn load_prune_execution_for_plan(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupPrunePlanId,
) -> Result<Option<BackupPruneExecutionRow>, BackupError> {
    sqlx::query_as::<_, BackupPruneExecutionRow>(
        "SELECT id, owner_user_id, prune_plan_id, backup_set_id, snapshot_id,
                released_pin_count, distinct_object_count,
                retained_by_other_reference_count, gc_handoff_object_count,
                state, executed_at
         FROM backup_prune_executions
         WHERE prune_plan_id = $1
           AND owner_user_id = $2
           AND state = 'COMMITTED'
         FOR SHARE",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn mark_restore_plan_stale(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    plan_id: BackupRestorePlanId,
) -> Result<(), BackupError> {
    let updated = sqlx::query(
        "UPDATE backup_restore_plans
         SET state = 'STALE', stale_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND owner_user_id = $2 AND state = 'PLANNED'",
    )
    .bind(plan_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(BackupError::InvalidPersistedData);
    }
    Ok(())
}

fn restore_structure_corrupt() -> BackupError {
    BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt)
}

fn validate_restore_plan_structure(
    plan: &BackupRestorePlan,
    entries: &[BackupRestorePlanEntry],
    manifest_nodes: &[BackupSnapshotNode],
) -> Result<(), BackupError> {
    let expected_item_count =
        u64::try_from(entries.len()).map_err(|_| restore_structure_corrupt())?;
    let expected_content_count = u64::try_from(
        entries
            .iter()
            .filter(|entry| entry.content().is_some())
            .count(),
    )
    .map_err(|_| restore_structure_corrupt())?;
    if expected_item_count != plan.item_count()
        || expected_content_count != plan.content_item_count()
        || entries.is_empty()
        || entries.len() != manifest_nodes.len()
    {
        return Err(restore_structure_corrupt());
    }

    let mut manifest_by_id = HashMap::with_capacity(manifest_nodes.len());
    for node in manifest_nodes {
        if manifest_by_id.insert(node.node_id(), node).is_some() {
            return Err(restore_structure_corrupt());
        }
    }
    let root = manifest_nodes
        .iter()
        .find(|node| node.parent_node_id().is_none())
        .ok_or_else(restore_structure_corrupt)?;
    let wrapper = entries.first().ok_or_else(restore_structure_corrupt)?;
    if !wrapper.is_wrapper()
        || wrapper.ordinal() != 0
        || wrapper.plan_id() != plan.id()
        || wrapper.planned_parent_node_id() != plan.target_parent_node_id()
        || wrapper.name() != plan.destination_name()
        || wrapper.kind() != NodeKind::Directory
        || wrapper.action() != BackupRestoreAction::CreateDirectory
        || wrapper.content().is_some()
    {
        return Err(restore_structure_corrupt());
    }

    let mut planned_by_source = HashMap::with_capacity(entries.len());
    planned_by_source.insert(root.node_id(), wrapper.planned_node_id());
    let mut planned_ids = HashSet::with_capacity(entries.len());
    if wrapper.planned_node_id() == wrapper.planned_parent_node_id() {
        return Err(restore_structure_corrupt());
    }
    let mut seen_sources = HashSet::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let ordinal = u64::try_from(index).map_err(|_| restore_structure_corrupt())?;
        if entry.ordinal() != ordinal
            || entry.plan_id() != plan.id()
            || entry.planned_node_id() == entry.planned_parent_node_id()
            || !planned_ids.insert(entry.planned_node_id())
        {
            return Err(restore_structure_corrupt());
        }
        if index == 0 {
            continue;
        }
        let source_node_id = entry
            .source_snapshot_node_id()
            .ok_or_else(restore_structure_corrupt)?;
        if source_node_id == root.node_id()
            || !seen_sources.insert(source_node_id)
            || entry.is_wrapper()
        {
            return Err(restore_structure_corrupt());
        }
        let source = manifest_by_id
            .get(&source_node_id)
            .ok_or_else(restore_structure_corrupt)?;
        if entry.source_parent_node_id() != source.parent_node_id()
            || entry.source_state() != source.state()
            || entry.kind() != source.kind()
            || entry.name() != source.name()
            || entry.source_revision() != Some(source.revision())
            || entry.content() != source.content()
        {
            return Err(restore_structure_corrupt());
        }
        let expected_action = match source.kind() {
            NodeKind::Directory => BackupRestoreAction::CreateDirectory,
            NodeKind::File => BackupRestoreAction::CreateFile,
        };
        if entry.action() != expected_action {
            return Err(restore_structure_corrupt());
        }
        let source_parent_id = source
            .parent_node_id()
            .ok_or_else(restore_structure_corrupt)?;
        let expected_planned_parent = planned_by_source
            .get(&source_parent_id)
            .copied()
            .ok_or_else(restore_structure_corrupt)?;
        if entry.planned_parent_node_id() != expected_planned_parent {
            return Err(restore_structure_corrupt());
        }
        planned_by_source.insert(source_node_id, entry.planned_node_id());
    }

    if seen_sources.len() + 1 != manifest_nodes.len()
        || manifest_nodes
            .iter()
            .any(|node| node.node_id() != root.node_id() && !seen_sources.contains(&node.node_id()))
    {
        return Err(restore_structure_corrupt());
    }
    Ok(())
}

async fn validate_planned_destination_ids_free(
    transaction: &mut Transaction<'_, Postgres>,
    entries: &[BackupRestorePlanEntry],
) -> Result<(), BackupError> {
    for entry in entries {
        let occupied = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1 FROM nodes WHERE id = $1
             )",
        )
        .bind(entry.planned_node_id().into_uuid())
        .fetch_one(&mut **transaction)
        .await?;
        if occupied {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::TargetChanged,
            ));
        }
    }
    Ok(())
}

async fn validate_restore_object_compatibility(
    transaction: &mut Transaction<'_, Postgres>,
    target_library: &Library,
    entries: &[BackupRestorePlanEntry],
    retained: &HashMap<NodeId, RetainedRestoreContent>,
) -> Result<(), BackupError> {
    let mut objects = BTreeSet::new();
    for entry in entries {
        if entry.action() != BackupRestoreAction::CreateFile {
            continue;
        }
        let source_node_id = entry
            .source_snapshot_node_id()
            .ok_or_else(restore_structure_corrupt)?;
        let content = entry.content().ok_or_else(restore_structure_corrupt)?;
        let retained = retained
            .get(&source_node_id)
            .ok_or(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::ContentUnavailable,
            ))?;
        if retained.object.dedup_domain_id() != target_library.dedup_domain_id() {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::TargetStorageIncompatible,
            ));
        }
        if retained.object.plaintext_length() != content.byte_length()
            || retained.object.canonical_hash() != content.sha256()
        {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::ContentUnavailable,
            ));
        }
        objects.insert((
            retained.object.object_id().into_uuid(),
            retained.object.dedup_domain_id().into_uuid(),
        ));
    }

    for (object_id, object_dedup_domain_id) in objects {
        let object_id = decode_id(object_id, "objects.id")?;
        let object_dedup_domain_id = decode_id(object_dedup_domain_id, "objects.dedup_domain_id")?;
        DomainRepository::lock_gc_candidate_row_for_reference(
            transaction,
            object_id,
            object_dedup_domain_id,
        )
        .await?;
        DomainRepository::lock_object_for_gc(transaction, object_id, object_dedup_domain_id)
            .await?;
        let object = sqlx::query_as::<_, (String, String, Vec<u8>)>(
            "SELECT lifecycle_state, plaintext_length::TEXT, canonical_hash
             FROM objects
             WHERE id = $1 AND dedup_domain_id = $2",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(BackupError::InvalidPersistedData)?;
        if object.0 != "AVAILABLE" {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::ContentUnavailable,
            ));
        }
        let has_verified_replica = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM object_replicas
                 WHERE object_id = $1
                   AND object_dedup_domain_id = $2
                   AND state = 'VERIFIED'
                   AND stored_length = $3::NUMERIC
                   AND stored_sha256 = $4
             )",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .bind(&object.1)
        .bind(&object.2)
        .fetch_one(&mut **transaction)
        .await?;
        if !has_verified_replica {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::ContentUnavailable,
            ));
        }
    }
    Ok(())
}

struct ExecutedRestoreMutation {
    ordinal: u64,
    destination_node_id: NodeId,
    destination_file_version_id: Option<FileVersionId>,
    change_kind: ChangeKind,
}

async fn load_restore_target(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    target_library_id: LibraryId,
    target_parent_node_id: NodeId,
    destination_name: &LogicalName,
) -> Result<Option<RestoreTargetObservation>, BackupError> {
    let library = sqlx::query_as::<_, RestoreTargetLibraryRow>(
        "SELECT journal_epoch, sync_head, status
         FROM libraries
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(target_library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(library) = library else {
        return Ok(None);
    };

    let Some(parent) = sqlx::query_as::<_, RestoreTargetParentRow>(
        "SELECT node.kind, node.state
         FROM nodes AS node
         INNER JOIN libraries AS parent_library ON parent_library.id = node.library_id
         WHERE node.id = $1
           AND node.library_id = $2
           AND parent_library.owner_user_id = $3
         FOR SHARE OF node",
    )
    .bind(target_parent_node_id.into_uuid())
    .bind(target_library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    else {
        return Ok(None);
    };
    let parent_is_valid =
        library.status == "ACTIVE" && parent.kind == "DIRECTORY" && parent.state == "ACTIVE";
    if !parent_is_valid {
        return Ok(Some(RestoreTargetObservation {
            journal_epoch: library.journal_epoch,
            sync_head: library.sync_head,
            parent_is_valid: false,
            destination_exists: false,
        }));
    }

    let destination_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
             SELECT 1
             FROM nodes
             WHERE library_id = $1
               AND parent_node_id = $2
               AND name = $3
               AND state = 'ACTIVE'
         )",
    )
    .bind(target_library_id.into_uuid())
    .bind(target_parent_node_id.into_uuid())
    .bind(destination_name.as_str())
    .fetch_one(&mut **transaction)
    .await?;

    Ok(Some(RestoreTargetObservation {
        journal_epoch: library.journal_epoch,
        sync_head: library.sync_head,
        parent_is_valid: true,
        destination_exists,
    }))
}

/// Load the complete target domain objects for execution. The library clock
/// and destination parent are locked before any source or Object metadata is
/// examined, which closes the validation-to-insert race with normal metadata
/// mutations using the same namespace fence.
async fn load_execution_target(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    target_library_id: LibraryId,
    target_parent_node_id: NodeId,
    destination_name: &LogicalName,
) -> Result<Option<ExecutionTargetObservation>, BackupError> {
    let library_row = sqlx::query_as::<_, ExecutionTargetLibraryRow>(
        "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                created_at, updated_at, revision::TEXT AS revision,
                journal_epoch, sync_head
         FROM libraries
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(target_library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(library_row) = library_row else {
        return Ok(None);
    };
    if library_row.journal_epoch <= 0 || library_row.sync_head < 0 {
        return Err(BackupError::InvalidPersistedData);
    }

    let root_row = sqlx::query_as::<_, NodeRow>(
        "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                state, trashed_at, created_at, updated_at,
                revision::TEXT AS revision
         FROM nodes
         WHERE id = $1 AND library_id = $2
         FOR SHARE",
    )
    .bind(library_row.root_node_id)
    .bind(library_row.id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(BackupError::InvalidPersistedData)?;
    let root = root_row.try_into_domain()?;
    let library = library_row.library_row().try_into_domain(&root)?;

    let parent_row = sqlx::query_as::<_, NodeRow>(
        "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                state, trashed_at, created_at, updated_at,
                revision::TEXT AS revision
         FROM nodes
         WHERE id = $1 AND library_id = $2
         FOR UPDATE",
    )
    .bind(target_parent_node_id.into_uuid())
    .bind(target_library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(parent_row) = parent_row else {
        return Ok(None);
    };
    let parent = parent_row.try_into_domain()?;
    let parent_is_valid = library.status() == LibraryStatus::Active
        && parent.kind() == NodeKind::Directory
        && parent.state() == NodeState::Active;
    if !parent_is_valid {
        return Ok(Some(ExecutionTargetObservation {
            library,
            parent,
            journal_epoch: library_row.journal_epoch,
            sync_head: library_row.sync_head,
            parent_is_valid: false,
            destination_exists: false,
        }));
    }

    let destination_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
             SELECT 1
             FROM nodes
             WHERE library_id = $1
               AND parent_node_id = $2
               AND name = $3
               AND state = 'ACTIVE'
         )",
    )
    .bind(target_library_id.into_uuid())
    .bind(target_parent_node_id.into_uuid())
    .bind(destination_name.as_str())
    .fetch_one(&mut **transaction)
    .await?;

    Ok(Some(ExecutionTargetObservation {
        library,
        parent,
        journal_epoch: library_row.journal_epoch,
        sync_head: library_row.sync_head,
        parent_is_valid: true,
        destination_exists,
    }))
}

async fn load_all_snapshot_nodes(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot_id: SnapshotId,
) -> Result<Vec<BackupSnapshotNodeRow>, BackupError> {
    sqlx::query_as::<_, BackupSnapshotNodeRow>(
        "SELECT snapshot_id, node_id, parent_node_id, name, kind, state,
                revision::TEXT AS revision, current_version_id,
                content_length::TEXT AS content_length, content_sha256,
                node_created_at, node_updated_at
         FROM backup_snapshot_nodes
         WHERE snapshot_id = $1
         ORDER BY node_id ASC",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(Into::into)
}

fn validate_snapshot_tree(
    snapshot: &BackupSnapshot,
    nodes: &[BackupSnapshotNode],
) -> Result<(), BackupError> {
    let manifest_count =
        u64::try_from(nodes.len()).map_err(|_| BackupError::InvalidPersistedData)?;
    let content_count = u64::try_from(nodes.iter().filter(|node| node.content().is_some()).count())
        .map_err(|_| BackupError::InvalidPersistedData)?;
    if manifest_count != snapshot.manifest_item_count()
        || content_count != snapshot.content_reference_count()
        || nodes.is_empty()
    {
        return Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::SnapshotCorrupt,
        ));
    }

    let mut by_id = HashMap::with_capacity(nodes.len());
    for node in nodes {
        if by_id.insert(node.node_id(), node).is_some() {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        }
        match node.kind() {
            NodeKind::Directory if node.content().is_some() => {
                return Err(BackupError::RestorePreflight(
                    BackupRestorePreflightIssue::SnapshotCorrupt,
                ));
            }
            NodeKind::File if node.content().is_none() => {
                return Err(BackupError::RestorePreflight(
                    BackupRestorePreflightIssue::SnapshotCorrupt,
                ));
            }
            NodeKind::Directory | NodeKind::File => {}
        }
    }

    if snapshot
        .terminal_node_id()
        .is_none_or(|terminal_id| !by_id.contains_key(&terminal_id))
    {
        return Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::SnapshotCorrupt,
        ));
    }

    let roots = nodes
        .iter()
        .filter(|node| node.parent_node_id().is_none())
        .collect::<Vec<_>>();
    if roots.len() != 1
        || roots[0].kind() != NodeKind::Directory
        || roots[0].state() != NodeState::Active
        || snapshot.committed_at().is_none()
        || snapshot.expired_at().is_some()
    {
        return Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::SnapshotCorrupt,
        ));
    }

    for node in nodes {
        let Some(parent_id) = node.parent_node_id() else {
            continue;
        };
        let Some(parent) = by_id.get(&parent_id) else {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        };
        if parent.kind() != NodeKind::Directory {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        }
    }

    // Every node must reach the one root through an in-snapshot parent chain.
    // This rejects both cycles and disconnected orphan components.
    for node in nodes {
        let mut current = node;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.node_id()) {
                return Err(BackupError::RestorePreflight(
                    BackupRestorePreflightIssue::SnapshotCorrupt,
                ));
            }
            let Some(parent_id) = current.parent_node_id() else {
                break;
            };
            let Some(parent) = by_id.get(&parent_id) else {
                return Err(BackupError::RestorePreflight(
                    BackupRestorePreflightIssue::SnapshotCorrupt,
                ));
            };
            current = parent;
        }
    }
    Ok(())
}

fn build_restore_plan_entries(
    request: &BackupRestorePlanRequest,
    plan_id: BackupRestorePlanId,
    nodes: &[BackupSnapshotNode],
) -> Result<Vec<BackupRestorePlanEntry>, BackupError> {
    let root = nodes
        .iter()
        .find(|node| node.parent_node_id().is_none())
        .ok_or(BackupError::InvalidPersistedData)?;
    let wrapper_id = NodeId::new();
    let wrapper = BackupRestorePlanEntry::new(
        plan_id,
        0,
        wrapper_id,
        request.target_parent_node_id(),
        None,
        None,
        NodeState::Active,
        BackupRestoreAction::CreateDirectory,
        NodeKind::Directory,
        request.destination_name().clone(),
        None,
        None,
    )
    .map_err(|_| BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt))?;

    let mut children: BTreeMap<NodeId, Vec<&BackupSnapshotNode>> = BTreeMap::new();
    for node in nodes {
        if let Some(parent_id) = node.parent_node_id() {
            children.entry(parent_id).or_default().push(node);
        }
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|node| node.node_id());
    }

    let mut entries = vec![wrapper];
    let mut planned_ids = HashMap::with_capacity(nodes.len());
    planned_ids.insert(root.node_id(), wrapper_id);
    let mut stack = children.get(&root.node_id()).cloned().unwrap_or_default();
    while let Some(node) = stack.pop() {
        let source_parent_id = node
            .parent_node_id()
            .ok_or(BackupError::InvalidPersistedData)?;
        let planned_parent_id = *planned_ids
            .get(&source_parent_id)
            .ok_or(BackupError::InvalidPersistedData)?;
        let planned_id = NodeId::new();
        planned_ids.insert(node.node_id(), planned_id);
        let (action, content) = match node.kind() {
            NodeKind::Directory => (BackupRestoreAction::CreateDirectory, None),
            NodeKind::File => (BackupRestoreAction::CreateFile, node.content().copied()),
        };
        let ordinal = u64::try_from(entries.len()).map_err(|_| BackupError::InvalidRequest)?;
        let entry = BackupRestorePlanEntry::new(
            plan_id,
            ordinal,
            planned_id,
            planned_parent_id,
            Some(node.node_id()),
            Some(source_parent_id),
            node.state(),
            action,
            node.kind(),
            node.name().clone(),
            Some(node.revision()),
            content,
        )
        .map_err(|_| BackupError::RestorePreflight(BackupRestorePreflightIssue::SnapshotCorrupt))?;
        entries.push(entry);

        if let Some(siblings) = children.get(&node.node_id()) {
            for child in siblings.iter().rev() {
                stack.push(*child);
            }
        }
    }
    if planned_ids.len() != nodes.len() {
        return Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::SnapshotCorrupt,
        ));
    }
    Ok(entries)
}

async fn load_retained_restore_content(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot: &BackupSnapshot,
    manifest_nodes: &[BackupSnapshotNode],
) -> Result<HashMap<NodeId, RetainedRestoreContent>, BackupError> {
    let expected_content_count = i64::try_from(snapshot.content_reference_count())
        .map_err(|_| BackupError::InvalidPersistedData)?;
    let pin_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    if pin_count != expected_content_count {
        return Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::MissingRetentionReference,
        ));
    }

    let rows = sqlx::query_as::<_, RetainedRestoreContentRow>(
        "SELECT manifest.node_id AS manifest_node_id,
                manifest.current_version_id AS manifest_file_version_id,
                pin.file_version_id AS pin_file_version_id,
                pin.object_id AS pin_object_id,
                pin.object_dedup_domain_id AS pin_object_dedup_domain_id,
                object.lifecycle_state AS object_lifecycle_state,
                object.plaintext_length::TEXT AS object_content_length,
                object.canonical_hash AS object_content_sha256,
                EXISTS(
                    SELECT 1
                    FROM object_replicas AS replica
                    WHERE replica.object_id = pin.object_id
                      AND replica.object_dedup_domain_id = pin.object_dedup_domain_id
                      AND replica.state = 'VERIFIED'
                      AND replica.stored_length = object.plaintext_length
                      AND replica.stored_sha256 = object.canonical_hash
                ) AS has_verified_replica
         FROM backup_snapshot_nodes AS manifest
         LEFT JOIN backup_snapshot_content_pins AS pin
           ON pin.snapshot_id = manifest.snapshot_id
          AND pin.manifest_node_id = manifest.node_id
         LEFT JOIN objects AS object
           ON object.id = pin.object_id
          AND object.dedup_domain_id = pin.object_dedup_domain_id
         WHERE manifest.snapshot_id = $1
           AND manifest.current_version_id IS NOT NULL
         ORDER BY manifest.node_id ASC",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    if i64::try_from(rows.len()).map_err(|_| BackupError::InvalidPersistedData)?
        != expected_content_count
    {
        return Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::SnapshotCorrupt,
        ));
    }

    let manifest_content = manifest_nodes
        .iter()
        .filter_map(|node| node.content().map(|content| (node.node_id(), content)))
        .collect::<HashMap<_, _>>();
    let mut retained = HashMap::with_capacity(rows.len());
    for row in rows {
        let manifest_node_id = decode_id(row.manifest_node_id, "backup_snapshot_nodes.node_id")?;
        let Some(expected) = manifest_content.get(&manifest_node_id) else {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        };
        if row.pin_file_version_id != Some(expected.file_version_id().into_uuid())
            || row.manifest_file_version_id != expected.file_version_id().into_uuid()
            || row.pin_object_id.is_none()
            || row.pin_object_dedup_domain_id.is_none()
        {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::MissingRetentionReference,
            ));
        }
        if row.object_lifecycle_state.as_deref() != Some("AVAILABLE")
            || row.object_content_length.is_none()
            || row.object_content_sha256.is_none()
            || !row.has_verified_replica
        {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::ContentUnavailable,
            ));
        }
        let object_length = u64::from_str(
            row.object_content_length
                .as_deref()
                .ok_or(BackupError::InvalidPersistedData)?,
        )
        .map_err(|_| BackupError::InvalidPersistedData)?;
        let object_sha256 = Sha256Digest::try_from(
            row.object_content_sha256
                .as_deref()
                .ok_or(BackupError::InvalidPersistedData)?,
        )
        .map_err(|_| BackupError::InvalidPersistedData)?;
        if object_length != expected.byte_length() || object_sha256 != expected.sha256() {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::ContentUnavailable,
            ));
        }

        let object_id = decode_id(
            row.pin_object_id.ok_or(BackupError::InvalidPersistedData)?,
            "backup_snapshot_content_pins.object_id",
        )?;
        let object_dedup_domain_id = decode_id(
            row.pin_object_dedup_domain_id
                .ok_or(BackupError::InvalidPersistedData)?,
            "backup_snapshot_content_pins.object_dedup_domain_id",
        )?;
        let object = ObjectReference::verified(
            object_id,
            object_dedup_domain_id,
            object_sha256,
            object_length,
        );
        if retained
            .insert(manifest_node_id, RetainedRestoreContent { object })
            .is_some()
        {
            return Err(BackupError::RestorePreflight(
                BackupRestorePreflightIssue::SnapshotCorrupt,
            ));
        }
    }
    Ok(retained)
}

async fn validate_retained_content(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot: &BackupSnapshot,
    manifest_nodes: &[BackupSnapshotNode],
) -> Result<(), BackupError> {
    load_retained_restore_content(transaction, snapshot, manifest_nodes).await?;
    Ok(())
}

/// Lock all existing GC candidates for the captured Objects before locking any
/// Object. The `ORDER BY` exactly matches the GC/purge ordering; acquiring the
/// complete candidate set first avoids a capture holding Object A while it
/// waits on a worker that already holds candidate B.
async fn lock_backup_pin_candidate_rows(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    snapshot_id: SnapshotId,
) -> Result<(), BackupError> {
    sqlx::query(
        "SELECT candidate.object_id, candidate.object_dedup_domain_id
         FROM object_gc_candidates AS candidate
         WHERE EXISTS (
             SELECT 1
             FROM backup_snapshot_nodes AS manifest
             INNER JOIN file_versions AS version
               ON version.id = manifest.current_version_id
             WHERE manifest.snapshot_id = $1
               AND version.object_id = candidate.object_id
               AND version.object_dedup_domain_id = candidate.object_dedup_domain_id
         )
         ORDER BY candidate.object_id ASC, candidate.object_dedup_domain_id ASC
         FOR UPDATE OF candidate",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Lock the canonical Objects for all captured content after candidate locks.
/// The Object lifecycle is checked before a pin is written, while the database
/// trigger on the pin table independently enforces the same rule for every
/// future writer.
async fn lock_backup_pin_objects(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    snapshot_id: SnapshotId,
) -> Result<Vec<String>, BackupError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT object.lifecycle_state
         FROM objects AS object
         WHERE EXISTS (
             SELECT 1
             FROM backup_snapshot_nodes AS manifest
             INNER JOIN file_versions AS version
               ON version.id = manifest.current_version_id
             WHERE manifest.snapshot_id = $1
               AND version.object_id = object.id
               AND version.object_dedup_domain_id = object.dedup_domain_id
         )
         ORDER BY object.id ASC, object.dedup_domain_id ASC
         FOR UPDATE OF object",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    Ok(rows)
}

/// Remove any candidate that was stale before this capture established its
/// durable backup reference. The candidate rows and Objects remain locked
/// until the capture transaction commits, so a concurrent purge/GC recheck
/// observes either no pin before capture commits or the committed pin after.
async fn clear_backup_pin_candidates(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    snapshot_id: SnapshotId,
) -> Result<(), BackupError> {
    sqlx::query(
        "DELETE FROM object_gc_candidates AS candidate
         WHERE EXISTS (
             SELECT 1
             FROM backup_snapshot_nodes AS manifest
             INNER JOIN file_versions AS version
               ON version.id = manifest.current_version_id
             WHERE manifest.snapshot_id = $1
               AND version.object_id = candidate.object_id
               AND version.object_dedup_domain_id = candidate.object_dedup_domain_id
         )",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_operation_key(value: &str) -> Result<(), BackupError> {
    if (MIN_OPERATION_KEY_BYTES..=MAX_OPERATION_KEY_BYTES).contains(&value.len()) {
        Ok(())
    } else {
        Err(BackupError::InvalidRequest)
    }
}

fn validate_page_limit_set(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_SET_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn validate_page_limit_node(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_SNAPSHOT_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn validate_page_limit_maintenance(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn validate_page_limit_operation(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_OPERATION_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn validate_restore_plan_entry_limit(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_RESTORE_PLAN_ENTRY_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn validate_prune_plan_entry_limit(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn validate_expiry_plan_entry_limit(limit: u32) -> Result<u32, BackupError> {
    if (1..=MAX_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(BackupError::InvalidLimit)
    }
}

fn encode_timestamp(value: Timestamp, field: &'static str) -> Result<OffsetDateTime, MappingError> {
    use time::UtcOffset;
    let value = value.as_offset_datetime().to_offset(UtcOffset::UTC);
    if !value.nanosecond().is_multiple_of(1_000) {
        return Err(MappingError::TimestampPrecisionLoss { field });
    }
    Ok(value)
}

fn revision_from_str(value: &str, field: &'static str) -> Result<Revision, MappingError> {
    Revision::from_str(value).map_err(|_| MappingError::InvalidDecimal { field })
}

fn decode_id<T>(value: Uuid, field: &'static str) -> Result<T, MappingError>
where
    T: TryFrom<Uuid, Error = synveil_core::IdParseError>,
{
    T::try_from(value).map_err(|reason| MappingError::InvalidId { field, reason })
}

fn decode_sequence(value: i64, field: &'static str) -> Result<Sequence, MappingError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| MappingError::InvalidDecimal { field })
}

fn decode_nonnegative_count(value: i64, field: &'static str) -> Result<u64, MappingError> {
    u64::try_from(value).map_err(|_| MappingError::InvalidDecimal { field })
}

fn decode_node_kind(value: &str) -> Result<NodeKind, MappingError> {
    match value {
        "FILE" => Ok(NodeKind::File),
        "DIRECTORY" => Ok(NodeKind::Directory),
        _ => Err(MappingError::InvalidEnum {
            field: "backup_snapshot_nodes.kind",
        }),
    }
}

fn decode_public_node_state(value: &str) -> Result<NodeState, MappingError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        _ => Err(MappingError::InvalidEnum {
            field: "backup_snapshot_nodes.state",
        }),
    }
}

fn map_metadata_error(error: MetadataError) -> BackupError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => BackupError::DependencyUnavailable,
        MetadataError::Database(error) => BackupError::Database(error),
        MetadataError::Mapping(_) | MetadataError::CapacityUnavailable => {
            BackupError::InvalidPersistedData
        }
    }
}

impl From<MetadataError> for BackupError {
    fn from(error: MetadataError) -> Self {
        map_metadata_error(error)
    }
}

impl From<sqlx::Error> for BackupError {
    fn from(error: sqlx::Error) -> Self {
        map_metadata_error(MetadataError::from(error))
    }
}

impl From<MappingError> for BackupError {
    fn from(error: MappingError) -> Self {
        map_metadata_error(MetadataError::Mapping(error))
    }
}

impl From<synveil_core::DomainError> for BackupError {
    fn from(error: synveil_core::DomainError) -> Self {
        map_metadata_error(MetadataError::Mapping(MappingError::Domain(error)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synveil_core::{FileVersionId, Revision};

    fn name(value: &str) -> LogicalName {
        LogicalName::new(value).expect("test logical name is valid")
    }

    #[test]
    fn backup_set_row_round_trips_domain_owner_scope_and_retention() {
        let at = Timestamp::parse("2026-08-29T00:00:00Z").expect("fixed timestamp is valid");
        let set = BackupSet::new(
            BackupSetId::new(),
            UserId::new(),
            name("nightly"),
            LibraryId::new(),
            Some(14),
            at,
        )
        .expect("backup set is valid");

        let row = BackupSetRow::from_domain(&set).expect("row encodes");
        assert_eq!(row.source, "LIBRARY");
        assert_eq!(row.state, "CREATED");
        assert_eq!(row.retention_days, Some(14));
        let round = row.try_into_domain().expect("row decodes");
        assert_eq!(round, set);
    }

    #[test]
    fn backup_snapshot_node_row_round_trips_without_physical_identity() {
        let at = Timestamp::parse("2026-08-29T00:00:00Z").expect("fixed timestamp is valid");
        let version_id = FileVersionId::new();
        let digest = Sha256Digest::from_bytes([0x2cu8; 32]);
        let node = BackupSnapshotNode::new(
            NodeId::new(),
            Some(NodeId::new()),
            name("report.pdf"),
            NodeKind::File,
            NodeState::Active,
            Revision::new(3),
            Some(BackupManifestContent::new(version_id, 2048, digest)),
            at,
            at,
        )
        .expect("manifest node is valid");

        let row = BackupSnapshotNodeRow {
            snapshot_id: SnapshotId::new().into_uuid(),
            node_id: node.node_id().into_uuid(),
            parent_node_id: node.parent_node_id().map(NodeId::into_uuid),
            name: node.name().as_str().to_owned(),
            kind: node.kind().as_str().to_owned(),
            state: node.state().as_str().to_owned(),
            revision: node.revision().get().to_string(),
            current_version_id: Some(version_id.into_uuid()),
            content_length: Some(node.content().unwrap().byte_length().to_string()),
            content_sha256: Some(node.content().unwrap().sha256().as_bytes().to_vec()),
            node_created_at: at.as_offset_datetime(),
            node_updated_at: at.as_offset_datetime(),
        };
        let round = row.try_into_domain().expect("row decodes");
        assert_eq!(round, node);
        assert_eq!(round.content().unwrap().file_version_id(), version_id);
        assert_eq!(round.content().unwrap().byte_length(), 2048);
        assert_eq!(round.content().unwrap().sha256(), digest);
    }

    #[test]
    fn operation_key_bounds_are_enforced() {
        assert_eq!(
            validate_operation_key("short"),
            Err(BackupError::InvalidRequest)
        );
        assert_eq!(validate_operation_key("0123456789"), Ok(()));
        assert_eq!(
            validate_operation_key(&"x".repeat(257)),
            Err(BackupError::InvalidRequest)
        );
    }

    #[test]
    fn page_limits_distinguish_set_and_node_cursors() {
        assert_eq!(validate_page_limit_set(0), Err(BackupError::InvalidLimit));
        assert_eq!(
            validate_page_limit_set(MAX_BACKUP_SET_PAGE_LIMIT),
            Ok(MAX_BACKUP_SET_PAGE_LIMIT)
        );
        assert_eq!(validate_page_limit_node(0), Err(BackupError::InvalidLimit));
        assert_eq!(
            validate_page_limit_node(MAX_BACKUP_SNAPSHOT_PAGE_LIMIT),
            Ok(MAX_BACKUP_SNAPSHOT_PAGE_LIMIT)
        );
        assert_eq!(
            validate_prune_plan_entry_limit(0),
            Err(BackupError::InvalidLimit)
        );
        assert_eq!(
            validate_prune_plan_entry_limit(MAX_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT),
            Ok(MAX_BACKUP_PRUNE_PLAN_ENTRY_PAGE_LIMIT)
        );
        assert_eq!(
            validate_expiry_plan_entry_limit(0),
            Err(BackupError::InvalidLimit)
        );
        assert_eq!(
            validate_expiry_plan_entry_limit(MAX_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT),
            Ok(MAX_BACKUP_SNAPSHOT_EXPIRY_PLAN_ENTRY_PAGE_LIMIT)
        );
    }

    #[test]
    fn prune_plan_rows_and_logical_entries_round_trip_without_physical_identity() {
        let at = Timestamp::parse("2026-08-30T00:00:00Z").expect("fixed timestamp is valid");
        let request = BackupPrunePlanRequest::new(BackupSetId::new(), SnapshotId::new());
        let fingerprint = request.fingerprint();
        let row = BackupPrunePlanRow {
            id: BackupPrunePlanId::new().into_uuid(),
            owner_user_id: UserId::new().into_uuid(),
            backup_set_id: request.backup_set_id().into_uuid(),
            snapshot_id: request.snapshot_id().into_uuid(),
            operation_id: "prune-row-round-trip".to_owned(),
            fingerprint_version: i16::try_from(fingerprint.version()).expect("version fits i16"),
            request_fingerprint: fingerprint.sha256().to_vec(),
            snapshot_manifest_item_count: 3,
            snapshot_content_reference_count: 2,
            planned_pin_release_count: 2,
            distinct_retained_content_count: 1,
            retained_after_release_count: 1,
            would_become_unreferenced_count: 0,
            state: "PLANNED".to_owned(),
            created_at: at.as_offset_datetime(),
            stale_at: None,
        };
        let plan = row.try_into_domain().expect("logical-safe row decodes");
        assert_eq!(plan.snapshot_id(), request.snapshot_id());
        assert_eq!(plan.planned_pin_release_count(), 2);
        assert_eq!(plan.distinct_retained_content_count(), 1);

        let entry_row = BackupPrunePlanEntryRow {
            plan_id: plan.id().into_uuid(),
            ordinal: 0,
            source_snapshot_node_id: NodeId::new().into_uuid(),
            file_version_id: FileVersionId::new().into_uuid(),
            content_length: "4096".to_owned(),
            content_sha256: vec![0x6a; 32],
        };
        let entry = entry_row
            .try_into_domain()
            .expect("logical release entry decodes");
        assert_eq!(entry.plan_id(), plan.id());
        assert_eq!(entry.content().byte_length(), 4096);
    }

    #[test]
    fn expiry_policy_boundary_equality_is_age_eligible_after_keep_floor() {
        let plan_id = BackupSnapshotExpiryPlanId::new();
        let backup_set_id = BackupSetId::new();
        let policy_revision_id = BackupSnapshotRetentionPolicyRevisionId::new();
        let evaluated_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let cutoff_at = Timestamp::parse("2026-07-31T00:00:00Z").unwrap();
        let newest = SnapshotId::new();
        let boundary = SnapshotId::new();
        let cohort = vec![
            ExpiryCohortRow {
                snapshot_id: newest.into_uuid(),
                committed_at: Timestamp::parse("2026-08-29T00:00:00Z")
                    .unwrap()
                    .as_offset_datetime(),
            },
            ExpiryCohortRow {
                snapshot_id: boundary.into_uuid(),
                committed_at: cutoff_at.as_offset_datetime(),
            },
        ];
        let observation = build_expiry_observation(
            plan_id,
            backup_set_id,
            policy_revision_id,
            evaluated_at,
            cutoff_at,
            BackupSnapshotRetentionPolicyConfig::new(1, 2_592_000).unwrap(),
            &cohort,
            &BTreeSet::new(),
        )
        .expect("boundary observation is valid");
        assert_eq!(
            observation.entries[0].decision(),
            BackupSnapshotExpiryDecision::KeepLatest
        );
        assert_eq!(
            observation.entries[1].decision(),
            BackupSnapshotExpiryDecision::Expire,
            "committed_at == cutoff is old enough"
        );
    }

    #[test]
    fn retention_policy_and_expiry_rows_round_trip_without_physical_identity() {
        let owner_user_id = UserId::new();
        let backup_set_id = BackupSetId::new();
        let config = BackupSnapshotRetentionPolicyConfig::new(2, 86_400).unwrap();
        let policy_request = BackupSnapshotRetentionPolicyRequest::new(backup_set_id, config);
        let policy_row = BackupSnapshotRetentionPolicyRevisionRow {
            id: BackupSnapshotRetentionPolicyRevisionId::new().into_uuid(),
            owner_user_id: owner_user_id.into_uuid(),
            backup_set_id: backup_set_id.into_uuid(),
            revision_number: 1,
            operation_id: "policy-row-roundtrip".to_owned(),
            fingerprint_version: i16::try_from(policy_request.fingerprint().version()).unwrap(),
            request_fingerprint: policy_request.fingerprint().sha256().to_vec(),
            keep_latest_completed: 2,
            expire_after_seconds: 86_400,
            created_at: Timestamp::parse("2026-08-30T00:00:00Z")
                .unwrap()
                .as_offset_datetime(),
        };
        let policy = policy_row.try_into_domain().expect("policy row decodes");
        assert_eq!(policy.config(), config);

        let evaluated_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let cutoff_at = Timestamp::parse("2026-08-29T00:00:00Z").unwrap();
        let plan_request = BackupSnapshotExpiryPlanRequest::new(backup_set_id);
        let basis = BackupSnapshotExpiryBasisFingerprint::calculate(
            backup_set_id,
            policy.id(),
            evaluated_at,
            &[],
        );
        let plan_row = BackupSnapshotExpiryPlanRow {
            id: BackupSnapshotExpiryPlanId::new().into_uuid(),
            owner_user_id: owner_user_id.into_uuid(),
            backup_set_id: backup_set_id.into_uuid(),
            policy_revision_id: policy.id().into_uuid(),
            policy_revision_number: 1,
            operation_id: "expiry-row-roundtrip".to_owned(),
            fingerprint_version: i16::try_from(plan_request.fingerprint().version()).unwrap(),
            request_fingerprint: plan_request.fingerprint().sha256().to_vec(),
            evaluated_at: evaluated_at.as_offset_datetime(),
            cutoff_at: cutoff_at.as_offset_datetime(),
            snapshot_basis_fingerprint_version: i16::try_from(basis.version()).unwrap(),
            snapshot_basis_fingerprint: basis.sha256().to_vec(),
            evaluated_completed_snapshot_count: 0,
            expire_candidate_count: 0,
            keep_latest_count: 0,
            keep_recent_count: 0,
            blocked_active_restore_count: 0,
            state: "PLANNED".to_owned(),
            created_at: evaluated_at.as_offset_datetime(),
            stale_at: None,
        };
        let plan = plan_row.try_into_domain().expect("expiry plan row decodes");
        assert_eq!(plan.evaluated_completed_snapshot_count(), 0);
        assert_eq!(plan.policy_revision_id(), policy.id());
    }

    #[test]
    fn backup_operation_summary_is_durable_kind_specific_and_step_based() {
        let owner_user_id = UserId::new();
        let backup_set_id = BackupSetId::new();
        let policy_id = BackupSnapshotRetentionPolicyRevisionId::new();
        let policy_number = BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap();
        let created_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let captured_at = Timestamp::parse("2026-08-30T01:00:00Z").unwrap();
        let stale_at = Timestamp::parse("2026-08-30T02:00:00Z").unwrap();
        let maintenance_request = BackupMaintenanceRunRequest::new(backup_set_id);
        let stale_run = BackupMaintenanceRun::new(
            BackupMaintenanceRunId::new(),
            owner_user_id,
            "maintenance-summary-stale".to_owned(),
            maintenance_request.fingerprint(),
            backup_set_id,
            policy_id,
            policy_number,
            "maintenance-capture-summary".to_owned(),
            "maintenance-expiry-summary".to_owned(),
            BackupMaintenanceRunState::Stale,
            Some(SnapshotId::new()),
            None,
            None,
            Some(captured_at),
            None,
            None,
            Some(stale_at),
            created_at,
        )
        .unwrap();
        let summary = BackupOperationDetail::Maintenance(stale_run)
            .summary()
            .unwrap();
        assert_eq!(summary.operation_kind(), BackupOperationKind::Maintenance);
        assert_eq!(summary.completed_steps(), 1);
        assert_eq!(summary.total_steps(), 3);
        assert_eq!(summary.last_transition_at(), stale_at);
        assert_eq!(summary.completed_at(), None);

        let snapshot_id = SnapshotId::new();
        let target_library_id = LibraryId::new();
        let target_parent_node_id = NodeId::new();
        let restore_request = BackupRestorePlanRequest::new(
            backup_set_id,
            snapshot_id,
            target_library_id,
            target_parent_node_id,
            name("Recovered"),
        );
        let restore_plan_id = BackupRestorePlanId::new();
        let restore_plan = BackupRestorePlan::new(
            restore_plan_id,
            owner_user_id,
            "restore-summary-planned".to_owned(),
            restore_request.fingerprint(),
            restore_request.clone(),
            Sequence::new(1),
            Sequence::new(1),
            2,
            1,
            BackupRestorePlanState::Planned,
            created_at,
            None,
        )
        .unwrap();
        let summary = BackupOperationDetail::Restore {
            plan: restore_plan,
            execution: None,
        }
        .summary()
        .unwrap();
        assert_eq!(summary.operation_kind(), BackupOperationKind::Restore);
        assert_eq!(summary.completed_steps(), 1);
        assert_eq!(summary.total_steps(), 2);
        assert_eq!(summary.completed_at(), None);

        let stale_restore_plan = BackupRestorePlan::new(
            BackupRestorePlanId::new(),
            owner_user_id,
            "restore-summary-stale".to_owned(),
            restore_request.fingerprint(),
            restore_request.clone(),
            Sequence::new(1),
            Sequence::new(1),
            2,
            1,
            BackupRestorePlanState::Stale,
            created_at,
            Some(stale_at),
        )
        .unwrap();
        let summary = BackupOperationDetail::Restore {
            plan: stale_restore_plan,
            execution: None,
        }
        .summary()
        .unwrap();
        assert_eq!(summary.completed_steps(), 1);
        assert_eq!(summary.total_steps(), 2);
        assert_eq!(summary.last_transition_at(), stale_at);
        assert_eq!(summary.completed_at(), None);

        let corrupt_restore_plan = BackupRestorePlan::new(
            BackupRestorePlanId::new(),
            owner_user_id,
            "restore-summary-corrupt".to_owned(),
            restore_request.fingerprint(),
            restore_request.clone(),
            Sequence::new(1),
            Sequence::new(1),
            2,
            1,
            BackupRestorePlanState::Executed,
            created_at,
            None,
        )
        .unwrap();
        assert_eq!(
            BackupOperationDetail::Restore {
                plan: corrupt_restore_plan,
                execution: None,
            }
            .summary(),
            Err(BackupError::InvalidPersistedData)
        );

        let executed_at = Timestamp::parse("2026-08-30T03:00:00Z").unwrap();
        let executed_restore_plan = BackupRestorePlan::new(
            BackupRestorePlanId::new(),
            owner_user_id,
            "restore-summary-executed".to_owned(),
            restore_request.fingerprint(),
            restore_request,
            Sequence::new(1),
            Sequence::new(1),
            2,
            1,
            BackupRestorePlanState::Executed,
            created_at,
            None,
        )
        .unwrap();
        let execution = BackupRestoreExecution::new(
            BackupRestoreExecutionId::new(),
            owner_user_id,
            executed_restore_plan.id(),
            target_library_id,
            Sequence::new(2),
            Sequence::new(3),
            2,
            1,
            executed_at,
        )
        .unwrap();
        let summary = BackupOperationDetail::Restore {
            plan: executed_restore_plan,
            execution: Some(execution),
        }
        .summary()
        .unwrap();
        assert_eq!(summary.completed_steps(), 2);
        assert_eq!(summary.last_transition_at(), executed_at);
        assert_eq!(summary.completed_at(), Some(executed_at));

        let prune_request = BackupPrunePlanRequest::new(backup_set_id, snapshot_id);
        let prune_plan = BackupPrunePlan::new(
            BackupPrunePlanId::new(),
            owner_user_id,
            "prune-summary-planned".to_owned(),
            prune_request.fingerprint(),
            prune_request,
            2,
            1,
            1,
            1,
            1,
            0,
            BackupPrunePlanState::Planned,
            created_at,
            None,
        )
        .unwrap();
        let summary = BackupOperationDetail::Prune {
            plan: prune_plan,
            execution: None,
        }
        .summary()
        .unwrap();
        assert_eq!(summary.operation_kind(), BackupOperationKind::Prune);
        assert_eq!(summary.completed_steps(), 1);
        assert_eq!(summary.total_steps(), 2);
        assert_eq!(summary.completed_at(), None);

        let stale_prune_request = BackupPrunePlanRequest::new(backup_set_id, snapshot_id);
        let stale_prune_plan = BackupPrunePlan::new(
            BackupPrunePlanId::new(),
            owner_user_id,
            "prune-summary-stale".to_owned(),
            stale_prune_request.fingerprint(),
            stale_prune_request,
            2,
            1,
            1,
            1,
            1,
            0,
            BackupPrunePlanState::Stale,
            created_at,
            Some(stale_at),
        )
        .unwrap();
        let summary = BackupOperationDetail::Prune {
            plan: stale_prune_plan,
            execution: None,
        }
        .summary()
        .unwrap();
        assert_eq!(summary.completed_steps(), 1);
        assert_eq!(summary.total_steps(), 2);
        assert_eq!(summary.last_transition_at(), stale_at);
        assert_eq!(summary.completed_at(), None);

        let corrupt_prune_request = BackupPrunePlanRequest::new(backup_set_id, snapshot_id);
        let corrupt_prune_plan = BackupPrunePlan::new(
            BackupPrunePlanId::new(),
            owner_user_id,
            "prune-summary-corrupt".to_owned(),
            corrupt_prune_request.fingerprint(),
            corrupt_prune_request,
            2,
            1,
            1,
            1,
            1,
            0,
            BackupPrunePlanState::Executed,
            created_at,
            None,
        )
        .unwrap();
        assert_eq!(
            BackupOperationDetail::Prune {
                plan: corrupt_prune_plan,
                execution: None,
            }
            .summary(),
            Err(BackupError::InvalidPersistedData)
        );

        let executed_prune_plan = BackupPrunePlan::new(
            BackupPrunePlanId::new(),
            owner_user_id,
            "prune-summary-executed".to_owned(),
            BackupPrunePlanRequest::new(backup_set_id, snapshot_id).fingerprint(),
            BackupPrunePlanRequest::new(backup_set_id, snapshot_id),
            2,
            1,
            1,
            1,
            1,
            0,
            BackupPrunePlanState::Executed,
            created_at,
            None,
        )
        .unwrap();
        let execution = BackupPruneExecution::new(
            BackupPruneExecutionId::new(),
            owner_user_id,
            executed_prune_plan.id(),
            backup_set_id,
            snapshot_id,
            1,
            1,
            1,
            0,
            executed_at,
        )
        .unwrap();
        let summary = BackupOperationDetail::Prune {
            plan: executed_prune_plan,
            execution: Some(execution),
        }
        .summary()
        .unwrap();
        assert_eq!(summary.completed_steps(), 2);
        assert_eq!(summary.last_transition_at(), executed_at);
        assert_eq!(summary.completed_at(), Some(executed_at));
    }

    #[test]
    fn backup_operation_order_is_deterministic_for_equal_timestamps() {
        let backup_set_id = BackupSetId::new();
        let created_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let summary = |operation_kind, operation_id| {
            let (state, completed_steps, total_steps) = match operation_kind {
                BackupOperationKind::Maintenance => (
                    BackupOperationState::Maintenance(BackupMaintenanceRunState::Created),
                    0,
                    3,
                ),
                BackupOperationKind::Restore => (
                    BackupOperationState::Restore(BackupRestorePlanState::Planned),
                    1,
                    2,
                ),
                BackupOperationKind::Prune => (
                    BackupOperationState::Prune(BackupPrunePlanState::Planned),
                    1,
                    2,
                ),
            };
            BackupOperationSummary {
                operation_kind,
                operation_id,
                backup_set_id,
                snapshot_id: None,
                state,
                completed_steps,
                total_steps,
                created_at,
                last_transition_at: created_at,
                completed_at: None,
            }
        };

        let mut summaries = [
            summary(
                BackupOperationKind::Prune,
                BackupOperationId::Prune(BackupPrunePlanId::new()),
            ),
            summary(
                BackupOperationKind::Restore,
                BackupOperationId::Restore(BackupRestorePlanId::new()),
            ),
            summary(
                BackupOperationKind::Maintenance,
                BackupOperationId::Maintenance(BackupMaintenanceRunId::new()),
            ),
        ];
        summaries.sort_by(compare_backup_operation_summaries);
        assert_eq!(
            summaries
                .iter()
                .map(BackupOperationSummary::operation_kind)
                .collect::<Vec<_>>(),
            vec![
                BackupOperationKind::Maintenance,
                BackupOperationKind::Restore,
                BackupOperationKind::Prune,
            ]
        );

        let first = BackupMaintenanceRunId::new();
        let second = BackupMaintenanceRunId::new();
        let expected_first = std::cmp::max(first.into_uuid(), second.into_uuid());
        let mut tied = [
            summary(
                BackupOperationKind::Maintenance,
                BackupOperationId::Maintenance(first),
            ),
            summary(
                BackupOperationKind::Maintenance,
                BackupOperationId::Maintenance(second),
            ),
        ];
        tied.sort_by(compare_backup_operation_summaries);
        assert_eq!(tied[0].operation_id().into_uuid(), expected_first);
    }
}
