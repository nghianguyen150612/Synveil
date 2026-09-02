//! Authenticated HTTP inspection and bounded manual mutation for the durable
//! backup domain.
//!
//! Mutations stop at canonical backup-set/retention/maintenance orchestration,
//! exact restore plans, and explicit two-phase prune plans. This boundary never
//! implements child algorithms, direct retention-pin SQL, GC execution,
//! physical deletion, or direct `ObjectStore` access.

use std::str::FromStr;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use synveil_core::{
    BackupMaintenanceRun, BackupMaintenanceRunId, BackupMaintenanceRunState, BackupOperationKind,
    BackupPruneExecution, BackupPruneExecutionId, BackupPruneExecutionPreflightIssue,
    BackupPruneImpact, BackupPrunePlan, BackupPrunePlanId, BackupPrunePlanState,
    BackupPrunePreflightIssue, BackupRestoreExecution, BackupRestoreExecutionId, BackupRestorePlan,
    BackupRestorePlanId, BackupRestorePlanState, BackupSet, BackupSetId, BackupSnapshot,
    BackupSnapshotExpiryPreflightIssue, BackupSnapshotNode, BackupSnapshotRetentionPolicyRevision,
    LibraryId, LogicalName, NodeId, SnapshotId, SnapshotState, Timestamp, UserId,
};
use synveil_metadata::{
    BackupError, BackupMaintenanceRunPagePosition, BackupMutationBackend, BackupOperationDetail,
    BackupOperationId, BackupOperationPagePosition, BackupOperationState, BackupOperationSummary,
    BackupReadBackend, BackupSnapshotPagePosition, DEFAULT_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT,
    DEFAULT_BACKUP_OPERATION_PAGE_LIMIT, DEFAULT_BACKUP_SET_PAGE_LIMIT,
    DEFAULT_BACKUP_SNAPSHOT_PAGE_LIMIT, DatabaseError, DatabaseErrorKind,
    MAX_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT, MAX_BACKUP_OPERATION_PAGE_LIMIT,
    MAX_BACKUP_SET_PAGE_LIMIT, MAX_BACKUP_SNAPSHOT_PAGE_LIMIT,
};
use uuid::Uuid;

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthenticatedPrincipal, ResponseMeta},
};

const CURSOR_VERSION: &str = "v1";
const SET_CURSOR_KIND: &str = "backup-set";
const SNAPSHOT_CURSOR_KIND: &str = "backup-snapshot";
const NODE_CURSOR_KIND: &str = "backup-node";
const MAINTENANCE_CURSOR_KIND: &str = "backup-maintenance";
const OPERATION_CURSOR_KIND: &str = "backup-operation";
const MAX_CURSOR_BYTES: usize = 512;
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const CREATE_SET_ID_DOMAIN: &[u8] = b"synveil/http/create-backup-set/v1\0";
const RETENTION_OPERATION_PREFIX: &str = "http:retention-policy:";
const MAINTENANCE_OPERATION_PREFIX: &str = "http:maintenance-run:";
const RESTORE_PLAN_OPERATION_PREFIX: &str = "http:restore-plan:";
const PRUNE_PLAN_OPERATION_PREFIX: &str = "http:prune-plan:";

/// Fail-closed backend used before the PostgreSQL composition root is
/// installed. It cannot accidentally turn an unconfigured API into a fake
/// successful backup read surface.
pub(crate) struct UnavailableBackupReadBackend;

#[async_trait]
impl BackupReadBackend for UnavailableBackupReadBackend {
    async fn list_backup_sets(
        &self,
        _owner_user_id: UserId,
        _after: Option<BackupSetId>,
        _limit: u32,
    ) -> Result<(Vec<BackupSet>, bool), BackupError> {
        Err(unavailable_error())
    }

    async fn get_backup_set(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
    ) -> Result<BackupSet, BackupError> {
        Err(unavailable_error())
    }

    async fn list_backup_snapshots(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
        _state: Option<SnapshotState>,
        _after: Option<BackupSnapshotPagePosition>,
        _limit: u32,
    ) -> Result<(Vec<BackupSnapshot>, bool), BackupError> {
        Err(unavailable_error())
    }

    async fn get_backup_snapshot(
        &self,
        _owner_user_id: UserId,
        _snapshot_id: SnapshotId,
    ) -> Result<BackupSnapshot, BackupError> {
        Err(unavailable_error())
    }

    async fn list_backup_snapshot_nodes(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
        _snapshot_id: SnapshotId,
        _parent_node_id: Option<NodeId>,
        _after: Option<NodeId>,
        _limit: u32,
    ) -> Result<(Vec<BackupSnapshotNode>, bool), BackupError> {
        Err(unavailable_error())
    }

    async fn get_current_snapshot_retention_policy(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        Err(unavailable_error())
    }

    async fn list_backup_maintenance_runs(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
        _after: Option<BackupMaintenanceRunPagePosition>,
        _limit: u32,
    ) -> Result<(Vec<BackupMaintenanceRun>, bool), BackupError> {
        Err(unavailable_error())
    }

    async fn get_backup_maintenance_run(
        &self,
        _owner_user_id: UserId,
        _run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        Err(unavailable_error())
    }

    async fn get_restore_plan(
        &self,
        _owner_user_id: UserId,
        _plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestorePlan, BackupError> {
        Err(unavailable_error())
    }

    async fn get_restore_execution(
        &self,
        _owner_user_id: UserId,
        _execution_id: BackupRestoreExecutionId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        Err(unavailable_error())
    }

    async fn get_prune_plan(
        &self,
        _owner_user_id: UserId,
        _plan_id: BackupPrunePlanId,
    ) -> Result<BackupPrunePlan, BackupError> {
        Err(unavailable_error())
    }

    async fn get_prune_execution(
        &self,
        _owner_user_id: UserId,
        _execution_id: BackupPruneExecutionId,
    ) -> Result<BackupPruneExecution, BackupError> {
        Err(unavailable_error())
    }

    async fn list_backup_operations(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
        _kind: Option<BackupOperationKind>,
        _after: Option<BackupOperationPagePosition>,
        _limit: u32,
    ) -> Result<(Vec<BackupOperationSummary>, bool), BackupError> {
        Err(unavailable_error())
    }

    async fn get_backup_operation(
        &self,
        _owner_user_id: UserId,
        _kind: BackupOperationKind,
        _operation_id: BackupOperationId,
    ) -> Result<BackupOperationDetail, BackupError> {
        Err(unavailable_error())
    }
}

/// Fail-closed mutation port used until the PostgreSQL composition root is
/// installed.
pub(crate) struct UnavailableBackupMutationBackend;

#[async_trait]
impl BackupMutationBackend for UnavailableBackupMutationBackend {
    async fn create_backup_set(
        &self,
        _owner_user_id: UserId,
        _backup_set_id: BackupSetId,
        _name: LogicalName,
        _source_library_id: LibraryId,
        _observed_at: Timestamp,
    ) -> Result<BackupSet, BackupError> {
        Err(unavailable_error())
    }

    async fn configure_snapshot_retention_policy(
        &self,
        _owner_user_id: UserId,
        _operation_id: String,
        _backup_set_id: BackupSetId,
        _keep_latest_completed: u64,
        _expire_after_seconds: u64,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        Err(unavailable_error())
    }

    async fn create_backup_maintenance_run(
        &self,
        _owner_user_id: UserId,
        _operation_id: String,
        _backup_set_id: BackupSetId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        Err(unavailable_error())
    }

    async fn advance_backup_maintenance_run(
        &self,
        _owner_user_id: UserId,
        _run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        Err(unavailable_error())
    }

    async fn create_restore_plan(
        &self,
        _owner_user_id: UserId,
        _operation_id: String,
        _backup_set_id: BackupSetId,
        _snapshot_id: SnapshotId,
        _target_library_id: LibraryId,
        _target_parent_node_id: NodeId,
        _destination_name: LogicalName,
    ) -> Result<BackupRestorePlan, BackupError> {
        Err(unavailable_error())
    }

    async fn execute_restore_plan(
        &self,
        _owner_user_id: UserId,
        _plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        Err(unavailable_error())
    }

    async fn create_prune_plan(
        &self,
        _owner_user_id: UserId,
        _operation_id: String,
        _backup_set_id: BackupSetId,
        _snapshot_id: SnapshotId,
    ) -> Result<BackupPrunePlan, BackupError> {
        Err(unavailable_error())
    }

    async fn execute_prune_plan(
        &self,
        _owner_user_id: UserId,
        _plan_id: BackupPrunePlanId,
    ) -> Result<BackupPruneExecution, BackupError> {
        Err(unavailable_error())
    }
}

fn unavailable_error() -> BackupError {
    BackupError::Database(DatabaseError::Failure(
        DatabaseErrorKind::ConnectionUnavailable,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupSetListQuery {
    cursor: Option<String>,
    limit: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupSnapshotListQuery {
    cursor: Option<String>,
    limit: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupSnapshotNodeListQuery {
    cursor: Option<String>,
    limit: Option<String>,
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupMaintenanceRunListQuery {
    cursor: Option<String>,
    limit: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupOperationListQuery {
    cursor: Option<String>,
    limit: Option<String>,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateBackupSetRequest {
    library_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigureBackupRetentionPolicyRequest {
    keep_latest_completed: u64,
    expire_after_seconds: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupSetListResponse {
    data: Vec<BackupSetData>,
    page: PageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupSetResponse {
    data: BackupSetData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupSnapshotListResponse {
    data: Vec<BackupSnapshotData>,
    page: PageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupSnapshotResponse {
    data: BackupSnapshotData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupSnapshotNodeListResponse {
    data: Vec<BackupSnapshotNodeData>,
    page: PageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupRetentionPolicyResponse {
    data: BackupRetentionPolicyData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupMaintenanceRunListResponse {
    data: Vec<BackupMaintenanceRunData>,
    page: PageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupMaintenanceRunResponse {
    data: BackupMaintenanceRunData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupRestorePlanResponse {
    data: BackupRestorePlanData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupRestoreExecutionResponse {
    data: BackupRestoreExecutionData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupPrunePlanResponse {
    data: BackupPrunePlanData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupPruneExecutionResponse {
    data: BackupPruneExecutionData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupOperationListResponse {
    data: Vec<BackupOperationSummaryData>,
    page: PageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupOperationResponse {
    data: BackupOperationDetailData,
    meta: ResponseMeta,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateBackupRestorePlanRequest {
    target_library_id: String,
    target_parent_node_id: String,
    destination_name: String,
}

/// Identity-bound confirmation for the retention-reducing phase. No physical
/// storage identity or deletion authority is accepted from the client.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecuteBackupPrunePlanRequest {
    confirm_snapshot_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupRestorePlanData {
    restore_plan_id: String,
    source_snapshot_id: String,
    source_backup_set_id: String,
    target_library_id: String,
    target_parent_node_id: String,
    destination_name: String,
    state: String,
    planned_entry_count: String,
    planned_directory_count: String,
    planned_file_count: String,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupRestoreExecutionData {
    restore_execution_id: String,
    restore_plan_id: String,
    source_snapshot_id: String,
    target_library_id: String,
    created_directory_count: String,
    created_file_count: String,
    journal_start_sequence: String,
    journal_end_sequence: String,
    executed_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupPrunePlanData {
    prune_plan_id: String,
    snapshot_id: String,
    backup_set_id: String,
    state: &'static str,
    entry_count: String,
    distinct_content_count: String,
    retained_by_other_reference_count: String,
    would_become_unreferenced_count: String,
    impact_summary: [BackupPruneImpactSummaryData; 2],
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackupPruneImpactSummaryData {
    impact: &'static str,
    content_count: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackupPruneExecutionData {
    prune_execution_id: String,
    prune_plan_id: String,
    snapshot_id: String,
    backup_set_id: String,
    released_content_reference_count: String,
    distinct_content_count: String,
    retained_elsewhere_count: String,
    gc_handoff_count: String,
    executed_at: String,
}

#[derive(Debug, Serialize)]
struct PageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
struct BackupOperationProgressData {
    completed_steps: u32,
    total_steps: u32,
}

#[derive(Debug, Serialize)]
struct BackupOperationSummaryData {
    operation_kind: &'static str,
    operation_id: String,
    backup_set_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_id: Option<String>,
    state: &'static str,
    phase: &'static str,
    progress: BackupOperationProgressData,
    terminal: bool,
    next_action: &'static str,
    created_at: String,
    last_transition_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "operation_kind")]
enum BackupOperationDetailData {
    #[serde(rename = "MAINTENANCE")]
    Maintenance(BackupMaintenanceOperationData),
    #[serde(rename = "RESTORE")]
    Restore(BackupRestoreOperationData),
    #[serde(rename = "PRUNE")]
    Prune(BackupPruneOperationData),
}

#[derive(Debug, Serialize)]
struct BackupMaintenanceOperationData {
    operation_id: String,
    maintenance_run_id: String,
    backup_set_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_id: Option<String>,
    state: &'static str,
    phase: &'static str,
    progress: BackupOperationProgressData,
    terminal: bool,
    next_action: &'static str,
    policy_revision_id: String,
    policy_revision_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_execution_id: Option<String>,
    created_at: String,
    last_transition_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_captured_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_planned_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackupRestoreOperationData {
    operation_id: String,
    restore_plan_id: String,
    backup_set_id: String,
    snapshot_id: String,
    source_snapshot_id: String,
    target_library_id: String,
    target_parent_node_id: String,
    destination_name: String,
    state: &'static str,
    phase: &'static str,
    progress: BackupOperationProgressData,
    terminal: bool,
    next_action: &'static str,
    planned_entry_count: String,
    planned_directory_count: String,
    planned_file_count: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    restore_execution_id: Option<String>,
    created_at: String,
    last_transition_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackupPruneOperationData {
    operation_id: String,
    prune_plan_id: String,
    backup_set_id: String,
    snapshot_id: String,
    state: &'static str,
    phase: &'static str,
    progress: BackupOperationProgressData,
    terminal: bool,
    next_action: &'static str,
    planned_pin_release_count: String,
    distinct_content_count: String,
    retained_by_other_reference_count: String,
    would_become_unreferenced_count: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prune_execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    released_content_reference_count: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gc_handoff_count: Option<String>,
    created_at: String,
    last_transition_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackupSetData {
    backup_set_id: String,
    name: String,
    library_id: String,
    state: &'static str,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct BackupSnapshotData {
    snapshot_id: String,
    backup_set_id: String,
    library_id: String,
    state: &'static str,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    committed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expired_at: Option<String>,
    logical_node_count: String,
    content_reference_count: String,
}

#[derive(Debug, Serialize)]
struct BackupSnapshotNodeData {
    snapshot_node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_snapshot_node_id: Option<String>,
    name: String,
    kind: &'static str,
    state: &'static str,
    revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_length: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct BackupRetentionPolicyData {
    policy_revision_id: String,
    backup_set_id: String,
    revision_number: String,
    keep_latest_completed: String,
    expire_after_seconds: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct BackupMaintenanceRunData {
    maintenance_run_id: String,
    backup_set_id: String,
    state: &'static str,
    policy_revision_id: String,
    policy_revision_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_execution_id: Option<String>,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_captured_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_planned_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_at: Option<String>,
}

pub(crate) async fn create_backup_set(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    headers: HeaderMap,
    Json(request): Json<CreateBackupSetRequest>,
) -> Result<Response, ApiError> {
    let idempotency_key = backup_idempotency_key(&headers)?;
    let library_id = parse_id::<LibraryId>(&request.library_id)?;
    let name = LogicalName::new(request.name).map_err(|_| ApiError::InvalidRequest)?;
    let backup_set_id = owner_scoped_backup_set_id(auth.owner_user_id(), idempotency_key);
    let set = state
        .backup_mutation_backend()
        .create_backup_set(
            auth.owner_user_id(),
            backup_set_id,
            name,
            library_id,
            database_timestamp_now(),
        )
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::CREATED,
            Json(BackupSetResponse {
                data: backup_set_data(&set),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn configure_backup_retention_policy(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ConfigureBackupRetentionPolicyRequest>,
) -> Result<Response, ApiError> {
    let idempotency_key = backup_idempotency_key(&headers)?;
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    validate_positive_i64(request.keep_latest_completed)?;
    validate_positive_i64(request.expire_after_seconds)?;
    let policy = state
        .backup_mutation_backend()
        .configure_snapshot_retention_policy(
            auth.owner_user_id(),
            format!("{RETENTION_OPERATION_PREFIX}{idempotency_key}"),
            backup_set_id,
            request.keep_latest_completed,
            request.expire_after_seconds,
        )
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::CREATED,
            Json(BackupRetentionPolicyResponse {
                data: retention_policy_data(&policy),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn create_backup_maintenance_run(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let idempotency_key = backup_idempotency_key(&headers)?;
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    let run = state
        .backup_mutation_backend()
        .create_backup_maintenance_run(
            auth.owner_user_id(),
            format!("{MAINTENANCE_OPERATION_PREFIX}{idempotency_key}"),
            backup_set_id,
        )
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::CREATED,
            Json(BackupMaintenanceRunResponse {
                data: maintenance_run_data(&run),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn advance_backup_maintenance_run(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let _idempotency_key = backup_idempotency_key(&headers)?;
    let run_id = parse_id::<BackupMaintenanceRunId>(&run_id)?;
    let run = state
        .backup_mutation_backend()
        .advance_backup_maintenance_run(auth.owner_user_id(), run_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupMaintenanceRunResponse {
                data: maintenance_run_data(&run),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_restore_plan(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CreateBackupRestorePlanRequest>,
) -> Result<Response, ApiError> {
    let idempotency_key = backup_idempotency_key(&headers)?;
    let snapshot_id = parse_id::<SnapshotId>(&snapshot_id)?;
    let target_library_id = parse_id::<LibraryId>(&request.target_library_id)?;
    let target_parent_node_id = parse_id::<NodeId>(&request.target_parent_node_id)?;
    let destination_name =
        LogicalName::new(request.destination_name).map_err(|_| ApiError::InvalidRequest)?;
    let snapshot = state
        .backup_read_backend()
        .get_backup_snapshot(auth.owner_user_id(), snapshot_id)
        .await
        .map_err(map_backup_error)?;
    let plan = state
        .backup_mutation_backend()
        .create_restore_plan(
            auth.owner_user_id(),
            format!("{RESTORE_PLAN_OPERATION_PREFIX}{idempotency_key}"),
            snapshot.backup_set_id(),
            snapshot_id,
            target_library_id,
            target_parent_node_id,
            destination_name,
        )
        .await
        .map_err(map_backup_error)?;
    let plan_data = plan_data(&plan);
    Ok(private_no_store(
        (
            StatusCode::CREATED,
            Json(BackupRestorePlanResponse {
                data: plan_data,
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_restore_plan(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(plan_id): Path<String>,
) -> Result<Response, ApiError> {
    let plan_id = parse_id::<BackupRestorePlanId>(&plan_id)?;
    let plan = state
        .backup_read_backend()
        .get_restore_plan(auth.owner_user_id(), plan_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupRestorePlanResponse {
                data: plan_data(&plan),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn execute_restore_plan(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(plan_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let _idempotency_key = backup_idempotency_key(&headers)?;
    let plan_id = parse_id::<BackupRestorePlanId>(&plan_id)?;
    let execution = state
        .backup_mutation_backend()
        .execute_restore_plan(auth.owner_user_id(), plan_id)
        .await
        .map_err(map_backup_error)?;
    let plan = state
        .backup_read_backend()
        .get_restore_plan(auth.owner_user_id(), plan_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupRestoreExecutionResponse {
                data: BackupRestoreExecutionData {
                    restore_execution_id: execution.id().to_string(),
                    restore_plan_id: execution.plan_id().to_string(),
                    source_snapshot_id: plan.snapshot_id().to_string(),
                    target_library_id: execution.target_library_id().to_string(),
                    created_directory_count: (execution.created_node_count()
                        - execution.created_file_version_count())
                    .to_string(),
                    created_file_count: execution.created_file_version_count().to_string(),
                    journal_start_sequence: execution.journal_first_sequence().to_string(),
                    journal_end_sequence: execution.journal_last_sequence().to_string(),
                    executed_at: execution.executed_at().to_string(),
                },
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_restore_execution(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(execution_id): Path<String>,
) -> Result<Response, ApiError> {
    let execution_id = parse_id::<BackupRestoreExecutionId>(&execution_id)?;
    let execution = state
        .backup_read_backend()
        .get_restore_execution(auth.owner_user_id(), execution_id)
        .await
        .map_err(map_backup_error)?;
    let plan = state
        .backup_read_backend()
        .get_restore_plan(auth.owner_user_id(), execution.plan_id())
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupRestoreExecutionResponse {
                data: BackupRestoreExecutionData {
                    restore_execution_id: execution.id().to_string(),
                    restore_plan_id: execution.plan_id().to_string(),
                    source_snapshot_id: plan.snapshot_id().to_string(),
                    target_library_id: execution.target_library_id().to_string(),
                    created_directory_count: (execution.created_node_count()
                        - execution.created_file_version_count())
                    .to_string(),
                    created_file_count: execution.created_file_version_count().to_string(),
                    journal_start_sequence: execution.journal_first_sequence().to_string(),
                    journal_end_sequence: execution.journal_last_sequence().to_string(),
                    executed_at: execution.executed_at().to_string(),
                },
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

/// Persist a non-destructive reference-accounting plan for exactly one owned
/// EXPIRED snapshot. The canonical service owns every retention calculation.
pub(crate) async fn create_prune_plan(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let idempotency_key = backup_idempotency_key(&headers)?;
    let snapshot_id = parse_id::<SnapshotId>(&snapshot_id)?;
    let snapshot = state
        .backup_read_backend()
        .get_backup_snapshot(auth.owner_user_id(), snapshot_id)
        .await
        .map_err(map_backup_error)?;
    let plan = state
        .backup_mutation_backend()
        .create_prune_plan(
            auth.owner_user_id(),
            format!("{PRUNE_PLAN_OPERATION_PREFIX}{idempotency_key}"),
            snapshot.backup_set_id(),
            snapshot_id,
        )
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::CREATED,
            Json(BackupPrunePlanResponse {
                data: prune_plan_data(&plan),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

/// Read persisted prune evidence exactly as planned. This deliberately does
/// not invoke validation, staleness detection, or execution.
pub(crate) async fn get_prune_plan(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(plan_id): Path<String>,
) -> Result<Response, ApiError> {
    let plan_id = parse_id::<BackupPrunePlanId>(&plan_id)?;
    let plan = state
        .backup_read_backend()
        .get_prune_plan(auth.owner_user_id(), plan_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupPrunePlanResponse {
                data: prune_plan_data(&plan),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

/// Execute only the persisted plan named in the path after an exact snapshot
/// identity echo. HTTP validates authority; canonical Prompt 46 code owns the
/// atomic release, drift fence, receipt replay, and GC-candidate handoff.
pub(crate) async fn execute_prune_plan(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(plan_id): Path<String>,
    headers: HeaderMap,
    request: Result<Json<ExecuteBackupPrunePlanRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _idempotency_key = backup_idempotency_key(&headers)?;
    let plan_id = parse_id::<BackupPrunePlanId>(&plan_id)?;
    let Json(request) = request.map_err(|_| ApiError::InvalidRequest)?;
    let confirmed_snapshot_id = parse_id::<SnapshotId>(&request.confirm_snapshot_id)?;
    let plan = state
        .backup_read_backend()
        .get_prune_plan(auth.owner_user_id(), plan_id)
        .await
        .map_err(map_backup_error)?;
    if confirmed_snapshot_id != plan.snapshot_id() {
        return Err(ApiError::InvalidRequest);
    }
    let execution = state
        .backup_mutation_backend()
        .execute_prune_plan(auth.owner_user_id(), plan_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupPruneExecutionResponse {
                data: prune_execution_data(&execution),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

/// Read the immutable logical receipt without retrying execution or touching
/// the GC pipeline.
pub(crate) async fn get_prune_execution(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(execution_id): Path<String>,
) -> Result<Response, ApiError> {
    let execution_id = parse_id::<BackupPruneExecutionId>(&execution_id)?;
    let execution = state
        .backup_read_backend()
        .get_prune_execution(auth.owner_user_id(), execution_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupPruneExecutionResponse {
                data: prune_execution_data(&execution),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

fn prune_plan_data(plan: &BackupPrunePlan) -> BackupPrunePlanData {
    BackupPrunePlanData {
        prune_plan_id: plan.id().to_string(),
        snapshot_id: plan.snapshot_id().to_string(),
        backup_set_id: plan.backup_set_id().to_string(),
        state: plan.state().as_str(),
        entry_count: plan.snapshot_content_reference_count().to_string(),
        distinct_content_count: plan.distinct_retained_content_count().to_string(),
        retained_by_other_reference_count: plan.retained_after_release_count().to_string(),
        would_become_unreferenced_count: plan.would_become_unreferenced_count().to_string(),
        impact_summary: [
            BackupPruneImpactSummaryData {
                impact: BackupPruneImpact::RetainedByOtherReference.as_str(),
                content_count: plan.retained_after_release_count().to_string(),
            },
            BackupPruneImpactSummaryData {
                impact: BackupPruneImpact::WouldBecomeUnreferenced.as_str(),
                content_count: plan.would_become_unreferenced_count().to_string(),
            },
        ],
        created_at: plan.created_at().to_string(),
        stale_at: plan.stale_at().map(|timestamp| timestamp.to_string()),
    }
}

fn prune_execution_data(execution: &BackupPruneExecution) -> BackupPruneExecutionData {
    BackupPruneExecutionData {
        prune_execution_id: execution.id().to_string(),
        prune_plan_id: execution.prune_plan_id().to_string(),
        snapshot_id: execution.snapshot_id().to_string(),
        backup_set_id: execution.backup_set_id().to_string(),
        released_content_reference_count: execution.released_pin_count().to_string(),
        distinct_content_count: execution.distinct_object_count().to_string(),
        retained_elsewhere_count: execution.retained_by_other_reference_count().to_string(),
        gc_handoff_count: execution.gc_handoff_object_count().to_string(),
        executed_at: execution.executed_at().to_string(),
    }
}

fn plan_data(plan: &BackupRestorePlan) -> BackupRestorePlanData {
    BackupRestorePlanData {
        restore_plan_id: plan.id().to_string(),
        source_snapshot_id: plan.snapshot_id().to_string(),
        source_backup_set_id: plan.backup_set_id().to_string(),
        target_library_id: plan.target_library_id().to_string(),
        target_parent_node_id: plan.target_parent_node_id().to_string(),
        destination_name: plan.destination_name().as_str().to_owned(),
        state: plan.state().as_str().to_owned(),
        planned_entry_count: plan.item_count().to_string(),
        planned_directory_count: (plan.item_count() - plan.content_item_count()).to_string(),
        planned_file_count: plan.content_item_count().to_string(),
        created_at: plan.created_at().to_string(),
        stale_at: plan.stale_at().map(|timestamp| timestamp.to_string()),
    }
}

pub(crate) async fn list_backup_sets(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<BackupSetListQuery>,
) -> Result<Response, ApiError> {
    let limit = parse_limit(
        query.limit.as_deref(),
        DEFAULT_BACKUP_SET_PAGE_LIMIT,
        MAX_BACKUP_SET_PAGE_LIMIT,
    )?;
    let after = decode_set_cursor(query.cursor.as_deref())?;
    let (sets, has_more) = state
        .backup_read_backend()
        .list_backup_sets(auth.owner_user_id(), after, limit)
        .await
        .map_err(map_backup_error)?;
    let next_cursor = has_more
        .then(|| sets.last().map(|set| encode_set_cursor(set.id())))
        .flatten();
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupSetListResponse {
                data: sets.iter().map(backup_set_data).collect(),
                page: page_response(next_cursor, has_more),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_backup_set(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
) -> Result<Response, ApiError> {
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    let set = state
        .backup_read_backend()
        .get_backup_set(auth.owner_user_id(), backup_set_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupSetResponse {
                data: backup_set_data(&set),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn list_backup_snapshots(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
    Query(query): Query<BackupSnapshotListQuery>,
) -> Result<Response, ApiError> {
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    let state_filter = query
        .state
        .as_deref()
        .map(SnapshotState::from_str)
        .transpose()
        .map_err(|_| ApiError::InvalidRequest)?;
    let limit = parse_limit(
        query.limit.as_deref(),
        DEFAULT_BACKUP_SNAPSHOT_PAGE_LIMIT,
        MAX_BACKUP_SNAPSHOT_PAGE_LIMIT,
    )?;
    let after = decode_snapshot_cursor(query.cursor.as_deref(), backup_set_id, state_filter)?;
    let (snapshots, has_more) = state
        .backup_read_backend()
        .list_backup_snapshots(
            auth.owner_user_id(),
            backup_set_id,
            state_filter,
            after,
            limit,
        )
        .await
        .map_err(map_backup_error)?;
    let next_cursor = has_more.then(|| {
        snapshots
            .last()
            .map(|snapshot| encode_snapshot_cursor(backup_set_id, state_filter, snapshot))
    });
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupSnapshotListResponse {
                data: snapshots.iter().map(backup_snapshot_data).collect(),
                page: page_response(next_cursor.flatten(), has_more),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_backup_snapshot(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
) -> Result<Response, ApiError> {
    let snapshot_id = parse_id::<SnapshotId>(&snapshot_id)?;
    let snapshot = state
        .backup_read_backend()
        .get_backup_snapshot(auth.owner_user_id(), snapshot_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupSnapshotResponse {
                data: backup_snapshot_data(&snapshot),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn list_backup_snapshot_nodes(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
    Query(query): Query<BackupSnapshotNodeListQuery>,
) -> Result<Response, ApiError> {
    let snapshot_id = parse_id::<SnapshotId>(&snapshot_id)?;
    let parent_node_id = query
        .parent_id
        .as_deref()
        .map(parse_id::<NodeId>)
        .transpose()?;
    let limit = parse_limit(
        query.limit.as_deref(),
        DEFAULT_BACKUP_SNAPSHOT_PAGE_LIMIT,
        MAX_BACKUP_SNAPSHOT_PAGE_LIMIT,
    )?;
    let snapshot = state
        .backup_read_backend()
        .get_backup_snapshot(auth.owner_user_id(), snapshot_id)
        .await
        .map_err(map_backup_error)?;
    let after = decode_node_cursor(query.cursor.as_deref(), snapshot_id, parent_node_id)?;
    let (nodes, has_more) = state
        .backup_read_backend()
        .list_backup_snapshot_nodes(
            auth.owner_user_id(),
            snapshot.backup_set_id(),
            snapshot_id,
            parent_node_id,
            after,
            limit,
        )
        .await
        .map_err(map_backup_error)?;
    let next_cursor = has_more.then(|| {
        nodes
            .last()
            .map(|node| encode_node_cursor(snapshot_id, parent_node_id, node.node_id()))
    });
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupSnapshotNodeListResponse {
                data: nodes.iter().map(backup_snapshot_node_data).collect(),
                page: page_response(next_cursor.flatten(), has_more),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_backup_retention_policy(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
) -> Result<Response, ApiError> {
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    let policy = state
        .backup_read_backend()
        .get_current_snapshot_retention_policy(auth.owner_user_id(), backup_set_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupRetentionPolicyResponse {
                data: retention_policy_data(&policy),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn list_backup_operations(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
    Query(query): Query<BackupOperationListQuery>,
) -> Result<Response, ApiError> {
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    let kind = query
        .kind
        .as_deref()
        .map(parse_operation_kind)
        .transpose()?;
    let limit = parse_limit(
        query.limit.as_deref(),
        DEFAULT_BACKUP_OPERATION_PAGE_LIMIT,
        MAX_BACKUP_OPERATION_PAGE_LIMIT,
    )?;
    let after = decode_operation_cursor(query.cursor.as_deref(), backup_set_id, kind)?;
    let (operations, has_more) = state
        .backup_read_backend()
        .list_backup_operations(auth.owner_user_id(), backup_set_id, kind, after, limit)
        .await
        .map_err(map_backup_error)?;
    let next_cursor = has_more.then(|| {
        operations
            .last()
            .map(|operation| encode_operation_cursor(backup_set_id, kind, operation))
    });
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupOperationListResponse {
                data: operations.iter().map(operation_summary_data).collect(),
                page: page_response(next_cursor.flatten(), has_more),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_backup_operation(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((operation_kind, operation_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let kind = parse_operation_kind(&operation_kind)?;
    let operation_id = parse_operation_id(kind, &operation_id)?;
    let operation = state
        .backup_read_backend()
        .get_backup_operation(auth.owner_user_id(), kind, operation_id)
        .await
        .map_err(map_backup_error)?;
    let data = operation_detail_data(&operation).map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupOperationResponse {
                data,
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn list_backup_maintenance_runs(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(backup_set_id): Path<String>,
    Query(query): Query<BackupMaintenanceRunListQuery>,
) -> Result<Response, ApiError> {
    let backup_set_id = parse_id::<BackupSetId>(&backup_set_id)?;
    let limit = parse_limit(
        query.limit.as_deref(),
        DEFAULT_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT,
        MAX_BACKUP_MAINTENANCE_RUN_PAGE_LIMIT,
    )?;
    let after = decode_maintenance_cursor(query.cursor.as_deref(), backup_set_id)?;
    let (runs, has_more) = state
        .backup_read_backend()
        .list_backup_maintenance_runs(auth.owner_user_id(), backup_set_id, after, limit)
        .await
        .map_err(map_backup_error)?;
    let next_cursor = has_more.then(|| {
        runs.last()
            .map(|run| encode_maintenance_cursor(backup_set_id, run.created_at(), run.id()))
    });
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupMaintenanceRunListResponse {
                data: runs.iter().map(maintenance_run_data).collect(),
                page: page_response(next_cursor.flatten(), has_more),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_backup_maintenance_run(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(run_id): Path<String>,
) -> Result<Response, ApiError> {
    let run_id = parse_id::<BackupMaintenanceRunId>(&run_id)?;
    let run = state
        .backup_read_backend()
        .get_backup_maintenance_run(auth.owner_user_id(), run_id)
        .await
        .map_err(map_backup_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BackupMaintenanceRunResponse {
                data: maintenance_run_data(&run),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

fn backup_set_data(set: &BackupSet) -> BackupSetData {
    BackupSetData {
        backup_set_id: set.id().to_string(),
        name: set.name().as_str().to_owned(),
        library_id: set.source_library_id().to_string(),
        state: set.state().as_str(),
        created_at: set.created_at().to_string(),
        updated_at: set.updated_at().to_string(),
    }
}

fn backup_snapshot_data(snapshot: &BackupSnapshot) -> BackupSnapshotData {
    BackupSnapshotData {
        snapshot_id: snapshot.id().to_string(),
        backup_set_id: snapshot.backup_set_id().to_string(),
        library_id: snapshot.source_library_id().to_string(),
        state: snapshot.state().as_str(),
        created_at: snapshot.created_at().to_string(),
        committed_at: snapshot.committed_at().map(|value| value.to_string()),
        expired_at: snapshot.expired_at().map(|value| value.to_string()),
        logical_node_count: snapshot.manifest_item_count().to_string(),
        content_reference_count: snapshot.content_reference_count().to_string(),
    }
}

fn backup_snapshot_node_data(node: &BackupSnapshotNode) -> BackupSnapshotNodeData {
    let (file_version_id, byte_length, sha256) =
        node.content().map_or((None, None, None), |content| {
            (
                Some(content.file_version_id().to_string()),
                Some(content.byte_length().to_string()),
                Some(content.sha256().to_string()),
            )
        });
    BackupSnapshotNodeData {
        snapshot_node_id: node.node_id().to_string(),
        parent_snapshot_node_id: node.parent_node_id().map(|value| value.to_string()),
        name: node.name().as_str().to_owned(),
        kind: node.kind().as_str(),
        state: node.state().as_str(),
        revision: node.revision().get().to_string(),
        file_version_id,
        byte_length,
        sha256,
        created_at: node.created_at().to_string(),
        updated_at: node.updated_at().to_string(),
    }
}

fn retention_policy_data(
    policy: &BackupSnapshotRetentionPolicyRevision,
) -> BackupRetentionPolicyData {
    let config = policy.config();
    BackupRetentionPolicyData {
        policy_revision_id: policy.id().to_string(),
        backup_set_id: policy.backup_set_id().to_string(),
        revision_number: policy.revision_number().get().to_string(),
        keep_latest_completed: config.keep_latest_completed().to_string(),
        expire_after_seconds: config.expire_after_seconds().to_string(),
        created_at: policy.created_at().to_string(),
    }
}

fn maintenance_run_data(run: &BackupMaintenanceRun) -> BackupMaintenanceRunData {
    BackupMaintenanceRunData {
        maintenance_run_id: run.id().to_string(),
        backup_set_id: run.backup_set_id().to_string(),
        state: run.state().as_str(),
        policy_revision_id: run.policy_revision_id().to_string(),
        policy_revision_number: run.policy_revision_number().get().to_string(),
        captured_snapshot_id: run.captured_snapshot_id().map(|value| value.to_string()),
        expiry_plan_id: run.expiry_plan_id().map(|value| value.to_string()),
        expiry_execution_id: run.expiry_execution_id().map(|value| value.to_string()),
        created_at: run.created_at().to_string(),
        snapshot_captured_at: run.snapshot_captured_at().map(|value| value.to_string()),
        expiry_planned_at: run.expiry_planned_at().map(|value| value.to_string()),
        completed_at: run
            .maintenance_completed_at()
            .map(|value| value.to_string()),
        stale_at: run.stale_at().map(|value| value.to_string()),
    }
}

struct OperationUiStatus {
    phase: &'static str,
    terminal: bool,
    next_action: &'static str,
}

fn operation_ui_status(state: BackupOperationState) -> OperationUiStatus {
    match state {
        BackupOperationState::Maintenance(state) => match state {
            BackupMaintenanceRunState::Created => OperationUiStatus {
                phase: "AWAITING_ADVANCE",
                terminal: false,
                next_action: "ADVANCE",
            },
            BackupMaintenanceRunState::SnapshotCaptured => OperationUiStatus {
                phase: "SNAPSHOT_CAPTURED",
                terminal: false,
                next_action: "ADVANCE",
            },
            BackupMaintenanceRunState::ExpiryPlanned => OperationUiStatus {
                phase: "EXPIRY_PLANNED",
                terminal: false,
                next_action: "ADVANCE",
            },
            BackupMaintenanceRunState::Completed => OperationUiStatus {
                phase: "COMPLETED",
                terminal: true,
                next_action: "NONE",
            },
            BackupMaintenanceRunState::Stale => OperationUiStatus {
                phase: "STALE",
                terminal: true,
                next_action: "CREATE_NEW_RUN",
            },
        },
        BackupOperationState::Restore(state) => match state {
            BackupRestorePlanState::Planned => OperationUiStatus {
                phase: "AWAITING_EXECUTION",
                terminal: false,
                next_action: "EXECUTE",
            },
            BackupRestorePlanState::Executed => OperationUiStatus {
                phase: "COMPLETED",
                terminal: true,
                next_action: "NONE",
            },
            BackupRestorePlanState::Stale => OperationUiStatus {
                phase: "STALE",
                terminal: true,
                next_action: "CREATE_NEW_PLAN",
            },
        },
        BackupOperationState::Prune(state) => match state {
            BackupPrunePlanState::Planned => OperationUiStatus {
                phase: "AWAITING_CONFIRMATION",
                terminal: false,
                next_action: "EXECUTE",
            },
            BackupPrunePlanState::Executed => OperationUiStatus {
                phase: "COMPLETED",
                terminal: true,
                next_action: "NONE",
            },
            BackupPrunePlanState::Stale => OperationUiStatus {
                phase: "STALE",
                terminal: true,
                next_action: "CREATE_NEW_PLAN",
            },
        },
    }
}

fn operation_id_string(operation_id: BackupOperationId) -> String {
    operation_id.into_uuid().to_string()
}

fn operation_summary_data(summary: &BackupOperationSummary) -> BackupOperationSummaryData {
    let status = operation_ui_status(summary.state());
    BackupOperationSummaryData {
        operation_kind: summary.operation_kind().as_str(),
        operation_id: operation_id_string(summary.operation_id()),
        backup_set_id: summary.backup_set_id().to_string(),
        snapshot_id: summary.snapshot_id().map(|value| value.to_string()),
        state: summary.state().as_str(),
        phase: status.phase,
        progress: BackupOperationProgressData {
            completed_steps: summary.completed_steps(),
            total_steps: summary.total_steps(),
        },
        terminal: status.terminal,
        next_action: status.next_action,
        created_at: summary.created_at().to_string(),
        last_transition_at: summary.last_transition_at().to_string(),
        completed_at: summary.completed_at().map(|value| value.to_string()),
    }
}

fn operation_detail_data(
    operation: &BackupOperationDetail,
) -> Result<BackupOperationDetailData, BackupError> {
    let summary = operation.summary()?;
    let status = operation_ui_status(summary.state());
    let common_progress = || BackupOperationProgressData {
        completed_steps: summary.completed_steps(),
        total_steps: summary.total_steps(),
    };
    match operation {
        BackupOperationDetail::Maintenance(run) => Ok(BackupOperationDetailData::Maintenance(
            BackupMaintenanceOperationData {
                operation_id: operation_id_string(summary.operation_id()),
                maintenance_run_id: run.id().to_string(),
                backup_set_id: run.backup_set_id().to_string(),
                snapshot_id: summary.snapshot_id().map(|value| value.to_string()),
                state: summary.state().as_str(),
                phase: status.phase,
                progress: common_progress(),
                terminal: status.terminal,
                next_action: status.next_action,
                policy_revision_id: run.policy_revision_id().to_string(),
                policy_revision_number: run.policy_revision_number().get().to_string(),
                captured_snapshot_id: run.captured_snapshot_id().map(|value| value.to_string()),
                expiry_plan_id: run.expiry_plan_id().map(|value| value.to_string()),
                expiry_execution_id: run.expiry_execution_id().map(|value| value.to_string()),
                created_at: summary.created_at().to_string(),
                last_transition_at: summary.last_transition_at().to_string(),
                snapshot_captured_at: run.snapshot_captured_at().map(|value| value.to_string()),
                expiry_planned_at: run.expiry_planned_at().map(|value| value.to_string()),
                completed_at: summary.completed_at().map(|value| value.to_string()),
                stale_at: run.stale_at().map(|value| value.to_string()),
            },
        )),
        BackupOperationDetail::Restore { plan, execution } => {
            let source_snapshot_id = plan.snapshot_id().to_string();
            Ok(BackupOperationDetailData::Restore(
                BackupRestoreOperationData {
                    operation_id: operation_id_string(summary.operation_id()),
                    restore_plan_id: plan.id().to_string(),
                    backup_set_id: plan.backup_set_id().to_string(),
                    snapshot_id: source_snapshot_id.clone(),
                    source_snapshot_id,
                    target_library_id: plan.target_library_id().to_string(),
                    target_parent_node_id: plan.target_parent_node_id().to_string(),
                    destination_name: plan.destination_name().as_str().to_owned(),
                    state: summary.state().as_str(),
                    phase: status.phase,
                    progress: common_progress(),
                    terminal: status.terminal,
                    next_action: status.next_action,
                    planned_entry_count: plan.item_count().to_string(),
                    planned_directory_count: (plan.item_count() - plan.content_item_count())
                        .to_string(),
                    planned_file_count: plan.content_item_count().to_string(),
                    restore_execution_id: execution.as_ref().map(|value| value.id().to_string()),
                    created_at: summary.created_at().to_string(),
                    last_transition_at: summary.last_transition_at().to_string(),
                    completed_at: summary.completed_at().map(|value| value.to_string()),
                    executed_at: execution
                        .as_ref()
                        .map(|value| value.executed_at().to_string()),
                    stale_at: plan.stale_at().map(|value| value.to_string()),
                },
            ))
        }
        BackupOperationDetail::Prune { plan, execution } => {
            Ok(BackupOperationDetailData::Prune(BackupPruneOperationData {
                operation_id: operation_id_string(summary.operation_id()),
                prune_plan_id: plan.id().to_string(),
                backup_set_id: plan.backup_set_id().to_string(),
                snapshot_id: plan.snapshot_id().to_string(),
                state: summary.state().as_str(),
                phase: status.phase,
                progress: common_progress(),
                terminal: status.terminal,
                next_action: status.next_action,
                planned_pin_release_count: plan.planned_pin_release_count().to_string(),
                distinct_content_count: plan.distinct_retained_content_count().to_string(),
                retained_by_other_reference_count: plan.retained_after_release_count().to_string(),
                would_become_unreferenced_count: plan.would_become_unreferenced_count().to_string(),
                prune_execution_id: execution.as_ref().map(|value| value.id().to_string()),
                released_content_reference_count: execution
                    .as_ref()
                    .map(|value| value.released_pin_count().to_string()),
                gc_handoff_count: execution
                    .as_ref()
                    .map(|value| value.gc_handoff_object_count().to_string()),
                created_at: summary.created_at().to_string(),
                last_transition_at: summary.last_transition_at().to_string(),
                completed_at: summary.completed_at().map(|value| value.to_string()),
                executed_at: execution
                    .as_ref()
                    .map(|value| value.executed_at().to_string()),
                stale_at: plan.stale_at().map(|value| value.to_string()),
            }))
        }
    }
}

fn page_response(next_cursor: Option<String>, has_more: bool) -> PageResponse {
    PageResponse {
        next_cursor,
        has_more,
    }
}

fn response_meta(context: &RequestContext) -> ResponseMeta {
    ResponseMeta {
        request_id: context.request_id().to_string(),
    }
}

fn private_no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn backup_idempotency_key(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_KEY_HEADER).iter();
    let value = values.next().ok_or(ApiError::InvalidRequest)?;
    if values.next().is_some() {
        return Err(ApiError::InvalidRequest);
    }
    let value = value.to_str().map_err(|_| ApiError::InvalidRequest)?;
    let uuid = Uuid::parse_str(value).map_err(|_| ApiError::InvalidRequest)?;
    if uuid.hyphenated().to_string() != value || BackupSetId::try_from_uuid(uuid).is_err() {
        return Err(ApiError::InvalidRequest);
    }
    Ok(uuid)
}

fn owner_scoped_backup_set_id(owner_user_id: UserId, idempotency_key: Uuid) -> BackupSetId {
    let mut digest = Sha256::new();
    digest.update(CREATE_SET_ID_DOMAIN);
    digest.update(owner_user_id.as_bytes());
    digest.update(idempotency_key.as_bytes());
    let digest = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes[..6].copy_from_slice(&idempotency_key.as_bytes()[..6]);
    bytes[6] = (digest[0] & 0x0f) | 0x70;
    bytes[7] = digest[1];
    bytes[8] = (digest[2] & 0x3f) | 0x80;
    bytes[9..].copy_from_slice(&digest[3..10]);
    BackupSetId::try_from_uuid(Uuid::from_bytes(bytes))
        .expect("the derived backup-set identity is a canonical UUIDv7")
}

fn validate_positive_i64(value: u64) -> Result<(), ApiError> {
    if value == 0 || i64::try_from(value).is_err() {
        Err(ApiError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn database_timestamp_now() -> Timestamp {
    let value = Timestamp::now().as_offset_datetime();
    let nanos = value.nanosecond();
    Timestamp::from_offset_datetime(
        value
            .replace_nanosecond(nanos / 1_000 * 1_000)
            .expect("truncating a valid timestamp preserves its range"),
    )
}

fn parse_limit(value: Option<&str>, default: u32, maximum: u32) -> Result<u32, ApiError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let limit = value
        .parse::<u32>()
        .map_err(|_| ApiError::BackupInvalidLimit)?;
    if limit.to_string() != value || !(1..=maximum).contains(&limit) {
        return Err(ApiError::BackupInvalidLimit);
    }
    Ok(limit)
}

fn parse_id<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    value.parse().map_err(|_| ApiError::InvalidRequest)
}

fn parse_cursor_id<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr + std::fmt::Display,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| ApiError::BackupInvalidCursor)?;
    if parsed.to_string() != value {
        return Err(ApiError::BackupInvalidCursor);
    }
    Ok(parsed)
}

fn decode_set_cursor(value: Option<&str>) -> Result<Option<BackupSetId>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(ApiError::BackupInvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 || parts[0] != CURSOR_VERSION || parts[1] != SET_CURSOR_KIND {
        return Err(ApiError::BackupInvalidCursor);
    }
    parse_cursor_id(parts[2]).map(Some)
}

fn encode_set_cursor(set_id: BackupSetId) -> String {
    format!("{CURSOR_VERSION}.{SET_CURSOR_KIND}.{set_id}")
}

fn decode_snapshot_cursor(
    value: Option<&str>,
    backup_set_id: BackupSetId,
    state: Option<SnapshotState>,
) -> Result<Option<BackupSnapshotPagePosition>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(ApiError::BackupInvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 6 || parts[0] != CURSOR_VERSION || parts[1] != SNAPSHOT_CURSOR_KIND {
        return Err(ApiError::BackupInvalidCursor);
    }
    let cursor_set_id = parse_cursor_id::<BackupSetId>(parts[2])?;
    let expected_filter = state.map_or("ALL", SnapshotState::as_str);
    if cursor_set_id != backup_set_id || parts[3] != expected_filter {
        return Err(ApiError::BackupInvalidCursor);
    }
    let sort_at = decode_timestamp(parts[4])?;
    let snapshot_id = parse_cursor_id::<SnapshotId>(parts[5])?;
    Ok(Some(BackupSnapshotPagePosition::new(sort_at, snapshot_id)))
}

fn encode_snapshot_cursor(
    backup_set_id: BackupSetId,
    state: Option<SnapshotState>,
    snapshot: &BackupSnapshot,
) -> String {
    let sort_at = snapshot.committed_at().unwrap_or(snapshot.created_at());
    format!(
        "{CURSOR_VERSION}.{SNAPSHOT_CURSOR_KIND}.{backup_set_id}.{}.{}.{}",
        state.map_or("ALL", SnapshotState::as_str),
        encode_timestamp(sort_at),
        snapshot.id()
    )
}

fn decode_node_cursor(
    value: Option<&str>,
    snapshot_id: SnapshotId,
    parent_node_id: Option<NodeId>,
) -> Result<Option<NodeId>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(ApiError::BackupInvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 5 || parts[0] != CURSOR_VERSION || parts[1] != NODE_CURSOR_KIND {
        return Err(ApiError::BackupInvalidCursor);
    }
    let cursor_snapshot_id = parse_cursor_id::<SnapshotId>(parts[2])?;
    let parent_matches =
        parent_node_id.map_or(parts[3] == "root", |value| parts[3] == value.to_string());
    if cursor_snapshot_id != snapshot_id || !parent_matches {
        return Err(ApiError::BackupInvalidCursor);
    }
    parse_cursor_id::<NodeId>(parts[4]).map(Some)
}

fn encode_node_cursor(
    snapshot_id: SnapshotId,
    parent_node_id: Option<NodeId>,
    node_id: NodeId,
) -> String {
    let parent = parent_node_id.map_or_else(|| "root".to_owned(), |value| value.to_string());
    format!("{CURSOR_VERSION}.{NODE_CURSOR_KIND}.{snapshot_id}.{parent}.{node_id}")
}

fn decode_maintenance_cursor(
    value: Option<&str>,
    backup_set_id: BackupSetId,
) -> Result<Option<BackupMaintenanceRunPagePosition>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(ApiError::BackupInvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 5 || parts[0] != CURSOR_VERSION || parts[1] != MAINTENANCE_CURSOR_KIND {
        return Err(ApiError::BackupInvalidCursor);
    }
    let cursor_set_id = parse_cursor_id::<BackupSetId>(parts[2])?;
    if cursor_set_id != backup_set_id {
        return Err(ApiError::BackupInvalidCursor);
    }
    let created_at = decode_timestamp(parts[3])?;
    let run_id = parse_cursor_id::<BackupMaintenanceRunId>(parts[4])?;
    Ok(Some(BackupMaintenanceRunPagePosition::new(
        created_at, run_id,
    )))
}

fn encode_maintenance_cursor(
    backup_set_id: BackupSetId,
    created_at: Timestamp,
    run_id: BackupMaintenanceRunId,
) -> String {
    format!(
        "{CURSOR_VERSION}.{MAINTENANCE_CURSOR_KIND}.{backup_set_id}.{}.{}",
        encode_timestamp(created_at),
        run_id
    )
}

fn decode_operation_cursor(
    value: Option<&str>,
    backup_set_id: BackupSetId,
    kind: Option<BackupOperationKind>,
) -> Result<Option<BackupOperationPagePosition>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(ApiError::BackupInvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 7 || parts[0] != CURSOR_VERSION || parts[1] != OPERATION_CURSOR_KIND {
        return Err(ApiError::BackupInvalidCursor);
    }
    let cursor_set_id = parse_cursor_id::<BackupSetId>(parts[2])?;
    if cursor_set_id != backup_set_id {
        return Err(ApiError::BackupInvalidCursor);
    }
    let expected_filter = kind.map_or("ALL", BackupOperationKind::as_str);
    if parts[3] != expected_filter {
        return Err(ApiError::BackupInvalidCursor);
    }
    let created_at = decode_timestamp(parts[4])?;
    let cursor_kind = operation_kind_from_rank(parts[5])?;
    if kind.is_some_and(|kind| kind != cursor_kind) {
        return Err(ApiError::BackupInvalidCursor);
    }
    let operation_id = match cursor_kind {
        BackupOperationKind::Maintenance => {
            BackupOperationId::Maintenance(parse_cursor_id(parts[6])?)
        }
        BackupOperationKind::Restore => BackupOperationId::Restore(parse_cursor_id(parts[6])?),
        BackupOperationKind::Prune => BackupOperationId::Prune(parse_cursor_id(parts[6])?),
    };
    Ok(Some(BackupOperationPagePosition::new(
        created_at,
        operation_id,
    )))
}

fn encode_operation_cursor(
    backup_set_id: BackupSetId,
    kind: Option<BackupOperationKind>,
    operation: &BackupOperationSummary,
) -> String {
    format!(
        "{CURSOR_VERSION}.{OPERATION_CURSOR_KIND}.{backup_set_id}.{}.{}.{}.{}",
        kind.map_or("ALL", BackupOperationKind::as_str),
        encode_timestamp(operation.created_at()),
        operation.operation_kind().rank(),
        operation_id_string(operation.operation_id()),
    )
}

fn operation_kind_from_rank(value: &str) -> Result<BackupOperationKind, ApiError> {
    let rank = value
        .parse::<u8>()
        .map_err(|_| ApiError::BackupInvalidCursor)?;
    if rank.to_string() != value {
        return Err(ApiError::BackupInvalidCursor);
    }
    match rank {
        1 => Ok(BackupOperationKind::Maintenance),
        2 => Ok(BackupOperationKind::Restore),
        3 => Ok(BackupOperationKind::Prune),
        _ => Err(ApiError::BackupInvalidCursor),
    }
}

fn parse_operation_kind(value: &str) -> Result<BackupOperationKind, ApiError> {
    BackupOperationKind::from_str(value).map_err(|_| ApiError::InvalidRequest)
}

fn parse_operation_id(
    kind: BackupOperationKind,
    value: &str,
) -> Result<BackupOperationId, ApiError> {
    match kind {
        BackupOperationKind::Maintenance => {
            parse_id::<BackupMaintenanceRunId>(value).map(BackupOperationId::Maintenance)
        }
        BackupOperationKind::Restore => {
            parse_id::<BackupRestorePlanId>(value).map(BackupOperationId::Restore)
        }
        BackupOperationKind::Prune => {
            parse_id::<BackupPrunePlanId>(value).map(BackupOperationId::Prune)
        }
    }
}

fn encode_timestamp(value: Timestamp) -> String {
    encode_hex(value.to_string().as_bytes())
}

fn decode_timestamp(value: &str) -> Result<Timestamp, ApiError> {
    let bytes = decode_hex(value).ok_or(ApiError::BackupInvalidCursor)?;
    let text = String::from_utf8(bytes).map_err(|_| ApiError::BackupInvalidCursor)?;
    let timestamp = Timestamp::from_str(&text).map_err(|_| ApiError::BackupInvalidCursor)?;
    if timestamp.to_string() != text {
        return Err(ApiError::BackupInvalidCursor);
    }
    Ok(timestamp)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(hex_digit(byte >> 4));
        encoded.push(hex_digit(byte & 0x0f));
    }
    encoded
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    for pair in pairs {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'a' + value - 10),
        _ => unreachable!("hex digit is four bits"),
    }
}

fn map_backup_error(error: BackupError) -> ApiError {
    match error {
        BackupError::NotFound
        | BackupError::OwnerScopeMismatch
        | BackupError::CrossOwnerBackupSet => ApiError::Core(synveil_core::ErrorCode::NotFound),
        BackupError::InvalidRequest => ApiError::InvalidRequest,
        BackupError::InvalidLimit => ApiError::BackupInvalidLimit,
        BackupError::ExpiryPreflight(
            BackupSnapshotExpiryPreflightIssue::RetentionPolicyNotConfigured,
        ) => ApiError::Core(synveil_core::ErrorCode::NotFound),
        BackupError::InvalidState
        | BackupError::BackupSetDisabled
        | BackupError::ExpiryPreflight(_)
        | BackupError::ExpiryExecutionPreflight(_)
        | BackupError::RestorePreflight(_) => ApiError::Core(synveil_core::ErrorCode::InvalidState),
        BackupError::BackupSetOperationConflict
        | BackupError::RetentionPolicyConflict
        | BackupError::PrunePlanConflict => ApiError::IdempotencyConflict,
        BackupError::BackupSetConflict => ApiError::BackupSetConflict,
        BackupError::SnapshotAlreadyPruned => ApiError::BackupSnapshotAlreadyPruned,
        BackupError::PruneAlreadyPlanned => ApiError::BackupPrunePlanAlreadyExists,
        BackupError::PrunePreflight(BackupPrunePreflightIssue::SnapshotNotExpired)
        | BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::SnapshotNotExpired,
        ) => ApiError::BackupSnapshotNotPrunable,
        BackupError::PrunePreflight(
            BackupPrunePreflightIssue::SnapshotCorrupt
            | BackupPrunePreflightIssue::RetentionCorrupt,
        )
        | BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::PlanCorruption,
        ) => ApiError::BackupRetentionCorruption,
        BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::PlanStale
            | BackupPruneExecutionPreflightIssue::PlanReferenceDrift,
        ) => ApiError::BackupPrunePlanStale,
        BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::SnapshotAlreadyPruned,
        ) => ApiError::BackupSnapshotAlreadyPruned,
        BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::PlanAlreadyExecuted,
        ) => ApiError::Core(synveil_core::ErrorCode::InvalidState),
        BackupError::MaintenanceRunPreflight(issue) => match issue {
            synveil_core::BackupMaintenanceRunPreflightIssue::RetentionPolicyNotConfigured => {
                ApiError::BackupRetentionPolicyNotConfigured
            }
            synveil_core::BackupMaintenanceRunPreflightIssue::MaintenanceAlreadyRunning => {
                ApiError::BackupMaintenanceAlreadyRunning
            }
            synveil_core::BackupMaintenanceRunPreflightIssue::OperationConflict => {
                ApiError::IdempotencyConflict
            }
            synveil_core::BackupMaintenanceRunPreflightIssue::PolicyChanged
            | synveil_core::BackupMaintenanceRunPreflightIssue::ExpiryPlanStale
            | synveil_core::BackupMaintenanceRunPreflightIssue::InvalidRunState => {
                ApiError::BackupMaintenanceRunStale
            }
            synveil_core::BackupMaintenanceRunPreflightIssue::RunCorruption => ApiError::Internal,
        },
        BackupError::SnapshotConflict
        | BackupError::SnapshotAlreadyBuilding
        | BackupError::RestorePlanConflict
        | BackupError::ExpiryPlanConflict => ApiError::Core(synveil_core::ErrorCode::InvalidState),
        BackupError::DependencyUnavailable
        | BackupError::Database(DatabaseError::Failure(DatabaseErrorKind::ConnectionUnavailable)) => {
            ApiError::ReadinessUnavailable
        }
        BackupError::Database(_)
        | BackupError::InvalidPersistedData
        | BackupError::InternalError => ApiError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        backup_idempotency_key, decode_hex, decode_timestamp, encode_hex, encode_timestamp,
        hex_digit, owner_scoped_backup_set_id, parse_limit,
    };
    use crate::ApiError;
    use axum::http::{HeaderMap, HeaderValue};
    use synveil_core::{Timestamp, UserId};
    use uuid::Uuid;

    #[test]
    fn timestamp_cursor_encoding_is_canonical_and_bounded() {
        let timestamp = Timestamp::parse("2026-08-31T00:00:00.123456Z").expect("valid timestamp");
        let encoded = encode_timestamp(timestamp);
        assert_eq!(decode_timestamp(&encoded), Ok(timestamp));
        assert!(decode_timestamp("00").is_err());
        assert_eq!(encode_hex(b"v1"), "7631");
        assert_eq!(decode_hex("7631"), Some(b"v1".to_vec()));
        assert_eq!(hex_digit(15), 'f');
    }

    #[test]
    fn limits_are_canonical_and_bounded() {
        assert_eq!(parse_limit(None, 100, 500), Ok(100));
        assert_eq!(parse_limit(Some("1"), 100, 500), Ok(1));
        assert_eq!(
            parse_limit(Some("01"), 100, 500),
            Err(ApiError::BackupInvalidLimit)
        );
        assert_eq!(
            parse_limit(Some("0"), 100, 500),
            Err(ApiError::BackupInvalidLimit)
        );
        assert_eq!(
            parse_limit(Some("501"), 100, 500),
            Err(ApiError::BackupInvalidLimit)
        );
    }

    #[test]
    fn backup_idempotency_keys_are_canonical_uuidv7_and_owner_scoped() {
        let key = Uuid::now_v7();
        let mut headers = HeaderMap::new();
        headers.insert(
            "idempotency-key",
            HeaderValue::from_str(&key.hyphenated().to_string()).expect("valid header"),
        );
        assert_eq!(backup_idempotency_key(&headers), Ok(key));

        let owner_a = UserId::new();
        let owner_b = UserId::new();
        let first = owner_scoped_backup_set_id(owner_a, key);
        assert_eq!(first, owner_scoped_backup_set_id(owner_a, key));
        assert_ne!(first, owner_scoped_backup_set_id(owner_b, key));

        headers.insert(
            "idempotency-key",
            HeaderValue::from_static("00000000-0000-4000-8000-000000000000"),
        );
        assert_eq!(
            backup_idempotency_key(&headers),
            Err(ApiError::InvalidRequest)
        );
    }
}
