//! Durable Prompt 35 conflict evidence, inspection, and explicit resolution.
//!
//! Conflict rows are control-plane evidence. Opening, inspecting, dismissing,
//! or stale-retrying a conflict never appends to the canonical resource
//! journal. A successful `APPLY_CLIENT_INTENT` delegates to the shared Prompt
//! 34 transaction-local executor and appends exactly its normal Node event.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use sqlx::FromRow;
use synveil_core::{
    ChangeEventId, ClientMutation, ClientMutationId, ClientMutationKind, ClientMutationRequest,
    ConflictLifecycle, ConflictResolutionAction, ConflictResolutionId, ConflictResolutionRequest,
    DeviceId, LibraryId, LogicalName, NodeId, NodeState, Revision, Sequence, SyncConflictId,
    Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::journal::{JournalChange, acquire_namespace_guard, append_changes};
use crate::mutations::{ClientMutationError, MutationConflict, MutationConflictReason};
use crate::repository::{ClientMutationDecision, prepare_canonical_client_mutation};
use crate::{DatabaseError, DatabaseErrorKind, DatabasePool, MetadataError, RebaselineReason};

pub const DEFAULT_CONFLICT_PAGE_LIMIT: u32 = 50;
pub const MAX_CONFLICT_PAGE_LIMIT: u32 = 100;

const _: () = {
    assert!(DEFAULT_CONFLICT_PAGE_LIMIT > 0);
    assert!(DEFAULT_CONFLICT_PAGE_LIMIT <= MAX_CONFLICT_PAGE_LIMIT);
    assert!(MAX_CONFLICT_PAGE_LIMIT <= 100);
};

/// Immutable keyset boundary recovered from a server-issued cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictPagePosition {
    created_at: Timestamp,
    conflict_id: SyncConflictId,
}

impl ConflictPagePosition {
    #[must_use]
    pub const fn new(created_at: Timestamp, conflict_id: SyncConflictId) -> Self {
        Self {
            created_at,
            conflict_id,
        }
    }

    #[must_use]
    pub const fn created_at(self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn conflict_id(self) -> SyncConflictId {
        self.conflict_id
    }
}

/// Terminal resolution metadata linked to immutable conflict evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictTerminalResolution {
    resolution_id: ConflictResolutionId,
    action: ConflictResolutionAction,
    journal_event_id: Option<ChangeEventId>,
    journal_sequence: Option<Sequence>,
    completed_at: Timestamp,
}

impl ConflictTerminalResolution {
    #[must_use]
    pub const fn new(
        resolution_id: ConflictResolutionId,
        action: ConflictResolutionAction,
        journal_event_id: Option<ChangeEventId>,
        journal_sequence: Option<Sequence>,
        completed_at: Timestamp,
    ) -> Self {
        Self {
            resolution_id,
            action,
            journal_event_id,
            journal_sequence,
            completed_at,
        }
    }

    #[must_use]
    pub const fn resolution_id(self) -> ConflictResolutionId {
        self.resolution_id
    }

    #[must_use]
    pub const fn action(self) -> ConflictResolutionAction {
        self.action
    }

    #[must_use]
    pub const fn journal_event_id(self) -> Option<ChangeEventId> {
        self.journal_event_id
    }

    #[must_use]
    pub const fn journal_sequence(self) -> Option<Sequence> {
        self.journal_sequence
    }

    #[must_use]
    pub const fn completed_at(self) -> Timestamp {
        self.completed_at
    }
}

/// One durable, fully typed conflict record. `historical_observation` is the
/// server state seen at rejection time; it is never presented as current.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncConflictRecord {
    conflict_id: SyncConflictId,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    original_client_mutation_id: ClientMutationId,
    resource_id: NodeId,
    original_intent: ClientMutation,
    historical_observation: MutationConflict,
    created_at: Timestamp,
    lifecycle: ConflictLifecycle,
    terminal_resolution: Option<ConflictTerminalResolution>,
}

impl SyncConflictRecord {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conflict_id: SyncConflictId,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        original_client_mutation_id: ClientMutationId,
        resource_id: NodeId,
        original_intent: ClientMutation,
        historical_observation: MutationConflict,
        created_at: Timestamp,
        lifecycle: ConflictLifecycle,
        terminal_resolution: Option<ConflictTerminalResolution>,
    ) -> Self {
        Self {
            conflict_id,
            owner_user_id,
            device_id,
            library_id,
            original_client_mutation_id,
            resource_id,
            original_intent,
            historical_observation,
            created_at,
            lifecycle,
            terminal_resolution,
        }
    }

    #[must_use]
    pub const fn conflict_id(&self) -> SyncConflictId {
        self.conflict_id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn original_client_mutation_id(&self) -> ClientMutationId {
        self.original_client_mutation_id
    }

    #[must_use]
    pub const fn resource_id(&self) -> NodeId {
        self.resource_id
    }

    #[must_use]
    pub const fn mutation_kind(&self) -> ClientMutationKind {
        self.original_intent.kind()
    }

    #[must_use]
    pub const fn original_intent(&self) -> &ClientMutation {
        &self.original_intent
    }

    #[must_use]
    pub const fn historical_observation(&self) -> &MutationConflict {
        &self.historical_observation
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn lifecycle(&self) -> ConflictLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn terminal_resolution(&self) -> Option<ConflictTerminalResolution> {
        self.terminal_resolution
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictPage {
    conflicts: Vec<SyncConflictRecord>,
    has_more: bool,
    next_position: Option<ConflictPagePosition>,
}

impl ConflictPage {
    #[must_use]
    pub fn new(
        conflicts: Vec<SyncConflictRecord>,
        has_more: bool,
        next_position: Option<ConflictPagePosition>,
    ) -> Self {
        Self {
            conflicts,
            has_more,
            next_position,
        }
    }

    #[must_use]
    pub fn conflicts(&self) -> &[SyncConflictRecord] {
        &self.conflicts
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub const fn next_position(&self) -> Option<ConflictPagePosition> {
        self.next_position
    }
}

/// Successful terminal decision. Stale manual applies are explicit errors and
/// carry their durably replayable server observation in the error enum below.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictResolutionResult {
    AcceptedServer {
        conflict_id: SyncConflictId,
        resolution_id: ConflictResolutionId,
        completed_at: Timestamp,
        replayed: bool,
    },
    AppliedClientIntent {
        conflict_id: SyncConflictId,
        resolution_id: ConflictResolutionId,
        journal_event_id: ChangeEventId,
        journal_sequence: Sequence,
        completed_at: Timestamp,
        replayed: bool,
    },
}

impl ConflictResolutionResult {
    #[must_use]
    pub const fn conflict_id(self) -> SyncConflictId {
        match self {
            Self::AcceptedServer { conflict_id, .. }
            | Self::AppliedClientIntent { conflict_id, .. } => conflict_id,
        }
    }

    #[must_use]
    pub const fn resolution_id(self) -> ConflictResolutionId {
        match self {
            Self::AcceptedServer { resolution_id, .. }
            | Self::AppliedClientIntent { resolution_id, .. } => resolution_id,
        }
    }

    #[must_use]
    pub const fn replayed(self) -> bool {
        match self {
            Self::AcceptedServer { replayed, .. } | Self::AppliedClientIntent { replayed, .. } => {
                replayed
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConflictManagementError {
    NotFound,
    InvalidListRequest,
    InvalidConflictResolution,
    ResolutionIdConflict,
    ConflictNotOpen {
        lifecycle: ConflictLifecycle,
    },
    ResolutionConflict {
        resolution_id: ConflictResolutionId,
        conflict: Box<MutationConflict>,
        completed_at: Timestamp,
        replayed: bool,
    },
    RebaselineRequired {
        reason: RebaselineReason,
        current_epoch: Sequence,
        minimum_retained_sequence: Sequence,
    },
    DependencyUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
    InternalError,
}

impl fmt::Display for ConflictManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "sync conflict scope was not found",
            Self::InvalidListRequest => "sync conflict list request is invalid",
            Self::InvalidConflictResolution => "sync conflict resolution is invalid",
            Self::ResolutionIdConflict => "sync conflict resolution identity conflicts",
            Self::ConflictNotOpen { .. } => "sync conflict is not open",
            Self::ResolutionConflict { .. } => "sync conflict resolution became stale",
            Self::RebaselineRequired { .. } => {
                "sync conflict resolution requires synchronization rebaseline"
            }
            Self::DependencyUnavailable => "sync conflict dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "sync conflict persisted data is invalid",
            Self::InternalError => "sync conflict internal error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ConflictManagementError {}

#[async_trait]
pub trait ConflictManagementBackend: Send + Sync {
    async fn list_open(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        position: Option<ConflictPagePosition>,
        limit: u32,
    ) -> Result<ConflictPage, ConflictManagementError>;

    async fn detail(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
    ) -> Result<SyncConflictRecord, ConflictManagementError>;

    async fn resolve(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
        request: ConflictResolutionRequest,
    ) -> Result<ConflictResolutionResult, ConflictManagementError>;
}

#[derive(Clone)]
pub struct ConflictManagementService {
    pool: DatabasePool,
}

impl ConflictManagementService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    pub async fn list_open(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        position: Option<ConflictPagePosition>,
        limit: u32,
    ) -> Result<ConflictPage, ConflictManagementError> {
        if !(1..=MAX_CONFLICT_PAGE_LIMIT).contains(&limit) {
            return Err(ConflictManagementError::InvalidListRequest);
        }
        validate_read_scope(&self.pool, owner_user_id, device_id, library_id).await?;
        let fetch_limit = i64::from(limit) + 1;
        let rows = sqlx::query_as::<_, SyncConflictRow>(
            "SELECT conflict_id, owner_user_id, device_id, library_id,
                    original_client_mutation_id, mutation_kind, resource_id,
                    conflict_reason, intent_node_id, intent_parent_node_id,
                    intent_requested_parent_id,
                    intent_expected_revision::TEXT AS intent_expected_revision,
                    intent_expected_parent_revision::TEXT AS intent_expected_parent_revision,
                    intent_requested_name, historical_resource_id,
                    historical_expected_revision::TEXT AS historical_expected_revision,
                    historical_server_revision::TEXT AS historical_server_revision,
                    historical_server_state, historical_server_parent_id,
                    historical_server_name, historical_server_epoch,
                    historical_server_sequence, created_at, lifecycle,
                    terminal_resolution_id, terminal_action,
                    terminal_journal_event_id, terminal_journal_sequence, terminal_at
             FROM sync_conflicts
             WHERE owner_user_id = $1
               AND device_id = $2
               AND library_id = $3
               AND lifecycle = 'OPEN'
               AND (
                    $4::TIMESTAMPTZ IS NULL
                    OR (created_at, conflict_id) < ($4, $5)
               )
             ORDER BY created_at DESC, conflict_id DESC
             LIMIT $6",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(position.map(|position| position.created_at().as_offset_datetime()))
        .bind(position.map(|position| position.conflict_id().into_uuid()))
        .bind(fetch_limit)
        .fetch_all(self.pool.sqlx_pool())
        .await
        .map_err(map_sqlx_error)?;

        let has_more = rows.len() > limit as usize;
        let mut conflicts = rows
            .into_iter()
            .take(limit as usize)
            .map(map_sync_conflict_row)
            .collect::<Result<Vec<_>, _>>()?;
        let next_position = has_more.then(|| {
            let last = conflicts
                .last()
                .expect("a page with a hidden extra row has a visible terminal row");
            ConflictPagePosition::new(last.created_at(), last.conflict_id())
        });
        conflicts.shrink_to_fit();
        Ok(ConflictPage {
            conflicts,
            has_more,
            next_position,
        })
    }

    pub async fn detail(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
    ) -> Result<SyncConflictRecord, ConflictManagementError> {
        validate_read_scope(&self.pool, owner_user_id, device_id, library_id).await?;
        load_conflict(
            self.pool.sqlx_pool(),
            owner_user_id,
            device_id,
            library_id,
            conflict_id,
        )
        .await?
        .ok_or(ConflictManagementError::NotFound)
    }

    pub async fn resolve(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
        request: ConflictResolutionRequest,
    ) -> Result<ConflictResolutionResult, ConflictManagementError> {
        let fingerprint = request.fingerprint(conflict_id);
        let fingerprint_version = i16::try_from(fingerprint.version())
            .map_err(|_| ConflictManagementError::InvalidConflictResolution)?;
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(map_sqlx_error)?;

        // Match every canonical metadata writer's lock order: namespace guard,
        // Device/Library scope, then conflict and Node rows.
        acquire_namespace_guard(&mut transaction, library_id)
            .await
            .map_err(map_metadata_error)?;
        let scope =
            load_resolution_scope(&mut transaction, owner_user_id, device_id, library_id).await?;
        let conflict = load_conflict_for_update(
            &mut transaction,
            owner_user_id,
            device_id,
            library_id,
            conflict_id,
        )
        .await?
        .ok_or(ConflictManagementError::NotFound)?;

        if let Some(operation) =
            load_resolution_operation(&mut transaction, request.resolution_id()).await?
        {
            let result = replay_resolution_operation(
                operation,
                owner_user_id,
                device_id,
                library_id,
                conflict_id,
                request,
                fingerprint.as_bytes(),
            )?;
            transaction.commit().await.map_err(map_sqlx_error)?;
            return result;
        }

        let prepared_mutation = validate_resolution_shape(&conflict, request)?;
        if conflict.lifecycle() != ConflictLifecycle::Open {
            return Err(ConflictManagementError::ConflictNotOpen {
                lifecycle: conflict.lifecycle(),
            });
        }

        let inserted = sqlx::query(
            "INSERT INTO sync_conflict_resolutions
                (resolution_id, owner_user_id, device_id, library_id, conflict_id,
                 fingerprint_version, fingerprint, action,
                 expected_current_revision, expected_current_parent_revision,
                 outcome, created_at, completed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::NUMERIC,
                     $10::NUMERIC, 'IN_PROGRESS', CURRENT_TIMESTAMP, NULL)
             ON CONFLICT (resolution_id) DO NOTHING",
        )
        .bind(request.resolution_id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(conflict_id.into_uuid())
        .bind(fingerprint_version)
        .bind(fingerprint.sha256().to_vec())
        .bind(request.action().as_str())
        .bind(
            request
                .expected_current_revision()
                .map(|revision| revision.get().to_string()),
        )
        .bind(
            request
                .expected_current_parent_revision()
                .map(|revision| revision.get().to_string()),
        )
        .execute(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;
        if inserted.rows_affected() != 1 {
            let operation = load_resolution_operation(&mut transaction, request.resolution_id())
                .await?
                .ok_or(ConflictManagementError::ResolutionIdConflict)?;
            let result = replay_resolution_operation(
                operation,
                owner_user_id,
                device_id,
                library_id,
                conflict_id,
                request,
                fingerprint.as_bytes(),
            )?;
            transaction.commit().await.map_err(map_sqlx_error)?;
            return result;
        }

        let completed_at = database_now(&mut transaction).await?;
        match request.action() {
            ConflictResolutionAction::AcceptServer => {
                terminalize_accept_server(
                    &mut transaction,
                    owner_user_id,
                    device_id,
                    library_id,
                    conflict_id,
                    request.resolution_id(),
                    completed_at,
                )
                .await?;
                let result = ConflictResolutionResult::AcceptedServer {
                    conflict_id,
                    resolution_id: request.resolution_id(),
                    completed_at,
                    replayed: false,
                };
                transaction.commit().await.map_err(map_sqlx_error)?;
                Ok(result)
            }
            ConflictResolutionAction::ApplyClientIntent => {
                let mutation =
                    prepared_mutation.ok_or(ConflictManagementError::InvalidConflictResolution)?;
                let decision = prepare_canonical_client_mutation(
                    &mut transaction,
                    owner_user_id,
                    library_id,
                    scope.journal_epoch,
                    scope.sync_head,
                    completed_at,
                    &mutation,
                )
                .await
                .map_err(map_client_mutation_error)?;
                match decision {
                    ClientMutationDecision::Conflict(stale) => {
                        terminalize_stale_resolution(
                            &mut transaction,
                            request.resolution_id(),
                            &stale,
                            completed_at,
                        )
                        .await?;
                        let error = ConflictManagementError::ResolutionConflict {
                            resolution_id: request.resolution_id(),
                            conflict: Box::new(stale),
                            completed_at,
                            replayed: false,
                        };
                        transaction.commit().await.map_err(map_sqlx_error)?;
                        Err(error)
                    }
                    ClientMutationDecision::Applied { node, change_kind } => {
                        let event = append_changes(
                            &mut transaction,
                            owner_user_id,
                            library_id,
                            &[JournalChange::from_node(change_kind, &node)],
                        )
                        .await
                        .map_err(map_metadata_error)?
                        .into_iter()
                        .next()
                        .ok_or(ConflictManagementError::InvalidPersistedData)?;
                        terminalize_applied_resolution(
                            &mut transaction,
                            owner_user_id,
                            device_id,
                            library_id,
                            conflict_id,
                            request.resolution_id(),
                            event.id(),
                            event.sequence(),
                            completed_at,
                        )
                        .await?;
                        let result = ConflictResolutionResult::AppliedClientIntent {
                            conflict_id,
                            resolution_id: request.resolution_id(),
                            journal_event_id: event.id(),
                            journal_sequence: event.sequence(),
                            completed_at,
                            replayed: false,
                        };
                        transaction.commit().await.map_err(map_sqlx_error)?;
                        Ok(result)
                    }
                }
            }
        }
    }
}

#[async_trait]
impl ConflictManagementBackend for ConflictManagementService {
    async fn list_open(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        position: Option<ConflictPagePosition>,
        limit: u32,
    ) -> Result<ConflictPage, ConflictManagementError> {
        self.list_open(owner_user_id, device_id, library_id, position, limit)
            .await
    }

    async fn detail(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
    ) -> Result<SyncConflictRecord, ConflictManagementError> {
        self.detail(owner_user_id, device_id, library_id, conflict_id)
            .await
    }

    async fn resolve(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
        request: ConflictResolutionRequest,
    ) -> Result<ConflictResolutionResult, ConflictManagementError> {
        self.resolve(owner_user_id, device_id, library_id, conflict_id, request)
            .await
    }
}

/// Insert exactly one canonical conflict row while the Prompt 34 operation is
/// still `IN_PROGRESS`. The operation's terminal update occurs in the same
/// caller transaction and a deferred bidirectional FK proves the equivalence
/// at commit.
pub(crate) async fn persist_sync_conflict(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    request: &ClientMutationRequest,
    conflict: &MutationConflict,
) -> Result<SyncConflictId, ClientMutationError> {
    let conflict_id = SyncConflictId::new();
    let projection = IntentProjection::from_mutation(request.mutation());
    sqlx::query(
        "INSERT INTO sync_conflicts
            (conflict_id, owner_user_id, device_id, library_id,
             original_client_mutation_id, mutation_kind, resource_id,
             conflict_reason, intent_node_id, intent_parent_node_id,
             intent_requested_parent_id, intent_expected_revision,
             intent_expected_parent_revision, intent_requested_name,
             historical_resource_id, historical_expected_revision,
             historical_server_revision, historical_server_state,
             historical_server_parent_id, historical_server_name,
             historical_server_epoch, historical_server_sequence,
             created_at, lifecycle)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                 $12::NUMERIC, $13::NUMERIC, $14, $15, $16::NUMERIC,
                 $17::NUMERIC, $18, $19, $20, $21, $22,
                 CURRENT_TIMESTAMP, 'OPEN')",
    )
    .bind(conflict_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(request.mutation_id().into_uuid())
    .bind(request.kind().as_str())
    .bind(projection.resource_id.into_uuid())
    .bind(conflict.reason().as_str())
    .bind(projection.node_id.map(NodeId::into_uuid))
    .bind(projection.parent_node_id.map(NodeId::into_uuid))
    .bind(projection.requested_parent_id.map(NodeId::into_uuid))
    .bind(revision_text(projection.expected_revision))
    .bind(revision_text(projection.expected_parent_revision))
    .bind(projection.requested_name.as_ref().map(LogicalName::as_str))
    .bind(conflict.resource_id().into_uuid())
    .bind(revision_text(conflict.expected_revision()))
    .bind(revision_text(conflict.current_revision()))
    .bind(conflict.current_state().map(NodeState::as_str))
    .bind(conflict.current_parent_id().map(NodeId::into_uuid))
    .bind(conflict.current_name().map(LogicalName::as_str))
    .bind(i64_from_sequence_for_client(conflict.server_epoch())?)
    .bind(i64_from_sequence_for_client(conflict.server_sequence())?)
    .execute(&mut **transaction)
    .await
    .map_err(map_sqlx_client_error)?;
    Ok(conflict_id)
}

#[derive(Debug, FromRow)]
struct SyncConflictRow {
    conflict_id: Uuid,
    owner_user_id: Uuid,
    device_id: Uuid,
    library_id: Uuid,
    original_client_mutation_id: Uuid,
    mutation_kind: String,
    resource_id: Uuid,
    conflict_reason: String,
    intent_node_id: Option<Uuid>,
    intent_parent_node_id: Option<Uuid>,
    intent_requested_parent_id: Option<Uuid>,
    intent_expected_revision: Option<String>,
    intent_expected_parent_revision: Option<String>,
    intent_requested_name: Option<String>,
    historical_resource_id: Uuid,
    historical_expected_revision: Option<String>,
    historical_server_revision: Option<String>,
    historical_server_state: Option<String>,
    historical_server_parent_id: Option<Uuid>,
    historical_server_name: Option<String>,
    historical_server_epoch: i64,
    historical_server_sequence: i64,
    created_at: OffsetDateTime,
    lifecycle: String,
    terminal_resolution_id: Option<Uuid>,
    terminal_action: Option<String>,
    terminal_journal_event_id: Option<Uuid>,
    terminal_journal_sequence: Option<i64>,
    terminal_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct ScopeRow {
    device_owner_user_id: Uuid,
    device_status: String,
    library_owner_user_id: Uuid,
    library_status: String,
    journal_epoch: i64,
    sync_head: i64,
}

#[derive(Clone, Copy, Debug)]
struct ResolutionScope {
    journal_epoch: Sequence,
    sync_head: Sequence,
}

#[derive(Debug, FromRow)]
struct ResolutionOperationRow {
    resolution_id: Uuid,
    owner_user_id: Uuid,
    device_id: Uuid,
    library_id: Uuid,
    conflict_id: Uuid,
    fingerprint_version: i16,
    fingerprint: Vec<u8>,
    action: String,
    expected_current_revision: Option<String>,
    expected_current_parent_revision: Option<String>,
    outcome: String,
    stale_reason: Option<String>,
    stale_resource_id: Option<Uuid>,
    stale_expected_revision: Option<String>,
    stale_server_revision: Option<String>,
    stale_server_state: Option<String>,
    stale_server_parent_id: Option<Uuid>,
    stale_server_name: Option<String>,
    stale_server_epoch: Option<i64>,
    stale_server_sequence: Option<i64>,
    journal_event_id: Option<Uuid>,
    journal_sequence: Option<i64>,
    completed_at: Option<OffsetDateTime>,
}

#[derive(Debug)]
struct IntentProjection {
    resource_id: NodeId,
    node_id: Option<NodeId>,
    parent_node_id: Option<NodeId>,
    requested_parent_id: Option<NodeId>,
    expected_revision: Option<Revision>,
    expected_parent_revision: Option<Revision>,
    requested_name: Option<LogicalName>,
}

impl IntentProjection {
    fn from_mutation(mutation: &ClientMutation) -> Self {
        match mutation {
            ClientMutation::CreateDirectory {
                parent_node_id,
                expected_parent_revision,
                name,
            } => Self {
                resource_id: *parent_node_id,
                node_id: None,
                parent_node_id: Some(*parent_node_id),
                requested_parent_id: None,
                expected_revision: None,
                expected_parent_revision: Some(*expected_parent_revision),
                requested_name: Some(name.clone()),
            },
            ClientMutation::RenameNode {
                node_id,
                expected_revision,
                new_name,
            } => Self {
                resource_id: *node_id,
                node_id: Some(*node_id),
                parent_node_id: None,
                requested_parent_id: None,
                expected_revision: Some(*expected_revision),
                expected_parent_revision: None,
                requested_name: Some(new_name.clone()),
            },
            ClientMutation::MoveNode {
                node_id,
                expected_revision,
                new_parent_node_id,
                expected_new_parent_revision,
            } => Self {
                resource_id: *node_id,
                node_id: Some(*node_id),
                parent_node_id: None,
                requested_parent_id: Some(*new_parent_node_id),
                expected_revision: Some(*expected_revision),
                expected_parent_revision: Some(*expected_new_parent_revision),
                requested_name: None,
            },
            ClientMutation::TrashNode {
                node_id,
                expected_revision,
            } => Self {
                resource_id: *node_id,
                node_id: Some(*node_id),
                parent_node_id: None,
                requested_parent_id: None,
                expected_revision: Some(*expected_revision),
                expected_parent_revision: None,
                requested_name: None,
            },
            ClientMutation::RestoreNode {
                node_id,
                expected_revision,
                expected_parent_node_id,
                expected_parent_revision,
            } => Self {
                resource_id: *node_id,
                node_id: Some(*node_id),
                parent_node_id: Some(*expected_parent_node_id),
                requested_parent_id: None,
                expected_revision: Some(*expected_revision),
                expected_parent_revision: Some(*expected_parent_revision),
                requested_name: None,
            },
        }
    }
}

async fn validate_read_scope(
    pool: &DatabasePool,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
) -> Result<(), ConflictManagementError> {
    let row = sqlx::query_as::<_, ScopeRow>(
        "SELECT d.owner_user_id AS device_owner_user_id,
                d.status AS device_status,
                l.owner_user_id AS library_owner_user_id,
                l.status AS library_status,
                l.journal_epoch, l.sync_head
         FROM devices AS d
         CROSS JOIN libraries AS l
         WHERE d.id = $1 AND l.id = $2",
    )
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_optional(pool.sqlx_pool())
    .await
    .map_err(map_sqlx_error)?
    .ok_or(ConflictManagementError::NotFound)?;
    validate_scope_row(row, owner_user_id).map(|_| ())
}

async fn load_resolution_scope(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
) -> Result<ResolutionScope, ConflictManagementError> {
    let row = sqlx::query_as::<_, ScopeRow>(
        "SELECT d.owner_user_id AS device_owner_user_id,
                d.status AS device_status,
                l.owner_user_id AS library_owner_user_id,
                l.status AS library_status,
                l.journal_epoch, l.sync_head
         FROM devices AS d
         CROSS JOIN libraries AS l
         WHERE d.id = $1 AND l.id = $2
         FOR UPDATE OF d, l",
    )
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_sqlx_error)?
    .ok_or(ConflictManagementError::NotFound)?;
    validate_scope_row(row, owner_user_id)
}

fn validate_scope_row(
    row: ScopeRow,
    owner_user_id: UserId,
) -> Result<ResolutionScope, ConflictManagementError> {
    if row.device_owner_user_id != owner_user_id.into_uuid()
        || row.library_owner_user_id != owner_user_id.into_uuid()
        || row.device_status != "ACTIVE"
    {
        return Err(ConflictManagementError::NotFound);
    }
    if row.library_status != "ACTIVE" {
        return Err(ConflictManagementError::InvalidConflictResolution);
    }
    Ok(ResolutionScope {
        journal_epoch: positive_sequence(row.journal_epoch)?,
        sync_head: nonnegative_sequence(row.sync_head)?,
    })
}

async fn load_conflict(
    pool: &sqlx::PgPool,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    conflict_id: SyncConflictId,
) -> Result<Option<SyncConflictRecord>, ConflictManagementError> {
    let row = sqlx::query_as::<_, SyncConflictRow>(
        "SELECT conflict_id, owner_user_id, device_id, library_id,
                original_client_mutation_id, mutation_kind, resource_id,
                conflict_reason, intent_node_id, intent_parent_node_id,
                intent_requested_parent_id,
                intent_expected_revision::TEXT AS intent_expected_revision,
                intent_expected_parent_revision::TEXT AS intent_expected_parent_revision,
                intent_requested_name, historical_resource_id,
                historical_expected_revision::TEXT AS historical_expected_revision,
                historical_server_revision::TEXT AS historical_server_revision,
                historical_server_state, historical_server_parent_id,
                historical_server_name, historical_server_epoch,
                historical_server_sequence, created_at, lifecycle,
                terminal_resolution_id, terminal_action,
                terminal_journal_event_id, terminal_journal_sequence, terminal_at
         FROM sync_conflicts
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
           AND conflict_id = $4",
    )
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(conflict_id.into_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;
    row.map(map_sync_conflict_row).transpose()
}

async fn load_conflict_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    conflict_id: SyncConflictId,
) -> Result<Option<SyncConflictRecord>, ConflictManagementError> {
    let row = sqlx::query_as::<_, SyncConflictRow>(
        "SELECT conflict_id, owner_user_id, device_id, library_id,
                original_client_mutation_id, mutation_kind, resource_id,
                conflict_reason, intent_node_id, intent_parent_node_id,
                intent_requested_parent_id,
                intent_expected_revision::TEXT AS intent_expected_revision,
                intent_expected_parent_revision::TEXT AS intent_expected_parent_revision,
                intent_requested_name, historical_resource_id,
                historical_expected_revision::TEXT AS historical_expected_revision,
                historical_server_revision::TEXT AS historical_server_revision,
                historical_server_state, historical_server_parent_id,
                historical_server_name, historical_server_epoch,
                historical_server_sequence, created_at, lifecycle,
                terminal_resolution_id, terminal_action,
                terminal_journal_event_id, terminal_journal_sequence, terminal_at
         FROM sync_conflicts
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
           AND conflict_id = $4
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(conflict_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_sqlx_error)?;
    row.map(map_sync_conflict_row).transpose()
}

fn map_sync_conflict_row(
    row: SyncConflictRow,
) -> Result<SyncConflictRecord, ConflictManagementError> {
    let conflict_id = decode_id::<SyncConflictId>(row.conflict_id)?;
    let owner_user_id = decode_id::<UserId>(row.owner_user_id)?;
    let device_id = decode_id::<DeviceId>(row.device_id)?;
    let library_id = decode_id::<LibraryId>(row.library_id)?;
    let original_client_mutation_id =
        decode_id::<ClientMutationId>(row.original_client_mutation_id)?;
    let resource_id = decode_id::<NodeId>(row.resource_id)?;
    let kind = ClientMutationKind::from_str(&row.mutation_kind)
        .map_err(|_| ConflictManagementError::InvalidPersistedData)?;
    let intent_node_id = row.intent_node_id.map(decode_id::<NodeId>).transpose()?;
    let intent_parent_node_id = row
        .intent_parent_node_id
        .map(decode_id::<NodeId>)
        .transpose()?;
    let intent_requested_parent_id = row
        .intent_requested_parent_id
        .map(decode_id::<NodeId>)
        .transpose()?;
    let intent_expected_revision = parse_revision(row.intent_expected_revision)?;
    let intent_expected_parent_revision = parse_revision(row.intent_expected_parent_revision)?;
    let intent_requested_name = row
        .intent_requested_name
        .map(LogicalName::new)
        .transpose()
        .map_err(|_| ConflictManagementError::InvalidPersistedData)?;
    let original_intent = match kind {
        ClientMutationKind::CreateDirectory => ClientMutation::create_directory(
            intent_parent_node_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_parent_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_requested_name.ok_or(ConflictManagementError::InvalidPersistedData)?,
        ),
        ClientMutationKind::RenameNode => ClientMutation::rename_node(
            intent_node_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_requested_name.ok_or(ConflictManagementError::InvalidPersistedData)?,
        ),
        ClientMutationKind::MoveNode => ClientMutation::move_node(
            intent_node_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_requested_parent_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_parent_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
        ),
        ClientMutationKind::TrashNode => ClientMutation::trash_node(
            intent_node_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
        ),
        ClientMutationKind::RestoreNode => ClientMutation::restore_node(
            intent_node_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_parent_node_id.ok_or(ConflictManagementError::InvalidPersistedData)?,
            intent_expected_parent_revision.ok_or(ConflictManagementError::InvalidPersistedData)?,
        ),
    };
    if IntentProjection::from_mutation(&original_intent).resource_id != resource_id {
        return Err(ConflictManagementError::InvalidPersistedData);
    }

    let reason = MutationConflictReason::from_str(&row.conflict_reason)
        .map_err(|_| ConflictManagementError::InvalidPersistedData)?;
    let historical_observation = MutationConflict::new(
        reason,
        decode_id(row.historical_resource_id)?,
        parse_revision(row.historical_expected_revision)?,
        parse_revision(row.historical_server_revision)?,
        row.historical_server_state
            .as_deref()
            .map(parse_node_state)
            .transpose()?,
        row.historical_server_parent_id
            .map(decode_id::<NodeId>)
            .transpose()?,
        row.historical_server_name
            .map(LogicalName::new)
            .transpose()
            .map_err(|_| ConflictManagementError::InvalidPersistedData)?,
        positive_sequence(row.historical_server_epoch)?,
        nonnegative_sequence(row.historical_server_sequence)?,
    );
    let lifecycle = ConflictLifecycle::from_str(&row.lifecycle)
        .map_err(|_| ConflictManagementError::InvalidPersistedData)?;
    let terminal_resolution = match lifecycle {
        ConflictLifecycle::Open => {
            if row.terminal_resolution_id.is_some()
                || row.terminal_action.is_some()
                || row.terminal_journal_event_id.is_some()
                || row.terminal_journal_sequence.is_some()
                || row.terminal_at.is_some()
            {
                return Err(ConflictManagementError::InvalidPersistedData);
            }
            None
        }
        ConflictLifecycle::Resolved | ConflictLifecycle::Dismissed => {
            let action = ConflictResolutionAction::from_str(
                row.terminal_action
                    .as_deref()
                    .ok_or(ConflictManagementError::InvalidPersistedData)?,
            )
            .map_err(|_| ConflictManagementError::InvalidPersistedData)?;
            let journal_event_id = row
                .terminal_journal_event_id
                .map(decode_id::<ChangeEventId>)
                .transpose()?;
            let journal_sequence = row
                .terminal_journal_sequence
                .map(positive_sequence)
                .transpose()?;
            if (lifecycle == ConflictLifecycle::Resolved
                && (action != ConflictResolutionAction::ApplyClientIntent
                    || journal_event_id.is_none()
                    || journal_sequence.is_none()))
                || (lifecycle == ConflictLifecycle::Dismissed
                    && (action != ConflictResolutionAction::AcceptServer
                        || journal_event_id.is_some()
                        || journal_sequence.is_some()))
            {
                return Err(ConflictManagementError::InvalidPersistedData);
            }
            Some(ConflictTerminalResolution {
                resolution_id: decode_id(
                    row.terminal_resolution_id
                        .ok_or(ConflictManagementError::InvalidPersistedData)?,
                )?,
                action,
                journal_event_id,
                journal_sequence,
                completed_at: Timestamp::from_offset_datetime(
                    row.terminal_at
                        .ok_or(ConflictManagementError::InvalidPersistedData)?,
                ),
            })
        }
    };
    Ok(SyncConflictRecord {
        conflict_id,
        owner_user_id,
        device_id,
        library_id,
        original_client_mutation_id,
        resource_id,
        original_intent,
        historical_observation,
        created_at: Timestamp::from_offset_datetime(row.created_at),
        lifecycle,
        terminal_resolution,
    })
}

fn validate_resolution_shape(
    conflict: &SyncConflictRecord,
    request: ConflictResolutionRequest,
) -> Result<Option<ClientMutation>, ConflictManagementError> {
    let revision = request.expected_current_revision();
    let parent_revision = request.expected_current_parent_revision();
    match request.action() {
        ConflictResolutionAction::AcceptServer => {
            if revision.is_some() || parent_revision.is_some() {
                return Err(ConflictManagementError::InvalidConflictResolution);
            }
            Ok(None)
        }
        ConflictResolutionAction::ApplyClientIntent => {
            let mutation = match conflict.original_intent() {
                ClientMutation::CreateDirectory {
                    parent_node_id,
                    name,
                    ..
                } => {
                    if parent_revision.is_some() {
                        return Err(ConflictManagementError::InvalidConflictResolution);
                    }
                    ClientMutation::create_directory(
                        *parent_node_id,
                        revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                        name.clone(),
                    )
                }
                ClientMutation::RenameNode {
                    node_id, new_name, ..
                } => {
                    if parent_revision.is_some() {
                        return Err(ConflictManagementError::InvalidConflictResolution);
                    }
                    ClientMutation::rename_node(
                        *node_id,
                        revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                        new_name.clone(),
                    )
                }
                ClientMutation::MoveNode {
                    node_id,
                    new_parent_node_id,
                    ..
                } => ClientMutation::move_node(
                    *node_id,
                    revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                    *new_parent_node_id,
                    parent_revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                ),
                ClientMutation::TrashNode { node_id, .. } => {
                    if parent_revision.is_some() {
                        return Err(ConflictManagementError::InvalidConflictResolution);
                    }
                    ClientMutation::trash_node(
                        *node_id,
                        revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                    )
                }
                ClientMutation::RestoreNode {
                    node_id,
                    expected_parent_node_id,
                    ..
                } => ClientMutation::restore_node(
                    *node_id,
                    revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                    *expected_parent_node_id,
                    parent_revision.ok_or(ConflictManagementError::InvalidConflictResolution)?,
                ),
            };
            Ok(Some(mutation))
        }
    }
}

async fn load_resolution_operation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    resolution_id: ConflictResolutionId,
) -> Result<Option<ResolutionOperationRow>, ConflictManagementError> {
    sqlx::query_as::<_, ResolutionOperationRow>(
        "SELECT resolution_id, owner_user_id, device_id, library_id,
                conflict_id, fingerprint_version, fingerprint, action,
                expected_current_revision::TEXT AS expected_current_revision,
                expected_current_parent_revision::TEXT AS expected_current_parent_revision,
                outcome, stale_reason, stale_resource_id,
                stale_expected_revision::TEXT AS stale_expected_revision,
                stale_server_revision::TEXT AS stale_server_revision,
                stale_server_state, stale_server_parent_id, stale_server_name,
                stale_server_epoch, stale_server_sequence, journal_event_id,
                journal_sequence, completed_at
         FROM sync_conflict_resolutions
         WHERE resolution_id = $1
         FOR UPDATE",
    )
    .bind(resolution_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_sqlx_error)
}

#[allow(clippy::too_many_arguments)]
fn replay_resolution_operation(
    row: ResolutionOperationRow,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    conflict_id: SyncConflictId,
    request: ConflictResolutionRequest,
    fingerprint: &[u8; 32],
) -> Result<Result<ConflictResolutionResult, ConflictManagementError>, ConflictManagementError> {
    if row.owner_user_id != owner_user_id.into_uuid()
        || row.device_id != device_id.into_uuid()
        || row.library_id != library_id.into_uuid()
        || row.conflict_id != conflict_id.into_uuid()
        || row.resolution_id != request.resolution_id().into_uuid()
        || row.fingerprint_version
            != i16::try_from(synveil_core::CONFLICT_RESOLUTION_FINGERPRINT_VERSION)
                .map_err(|_| ConflictManagementError::InvalidPersistedData)?
        || row.fingerprint.as_slice() != fingerprint.as_slice()
        || row.action != request.action().as_str()
        || parse_revision(row.expected_current_revision.clone())?
            != request.expected_current_revision()
        || parse_revision(row.expected_current_parent_revision.clone())?
            != request.expected_current_parent_revision()
    {
        return Err(ConflictManagementError::ResolutionIdConflict);
    }
    let completed_at = Timestamp::from_offset_datetime(
        row.completed_at
            .ok_or(ConflictManagementError::InvalidPersistedData)?,
    );
    let result = match row.outcome.as_str() {
        "ACCEPTED_SERVER" => Ok(ConflictResolutionResult::AcceptedServer {
            conflict_id,
            resolution_id: request.resolution_id(),
            completed_at,
            replayed: true,
        }),
        "APPLIED_CLIENT_INTENT" => Ok(ConflictResolutionResult::AppliedClientIntent {
            conflict_id,
            resolution_id: request.resolution_id(),
            journal_event_id: decode_id(
                row.journal_event_id
                    .ok_or(ConflictManagementError::InvalidPersistedData)?,
            )?,
            journal_sequence: positive_sequence(
                row.journal_sequence
                    .ok_or(ConflictManagementError::InvalidPersistedData)?,
            )?,
            completed_at,
            replayed: true,
        }),
        "STALE" => Err(ConflictManagementError::ResolutionConflict {
            resolution_id: request.resolution_id(),
            conflict: Box::new(map_stale_resolution(&row)?),
            completed_at,
            replayed: true,
        }),
        _ => return Err(ConflictManagementError::InvalidPersistedData),
    };
    Ok(result)
}

fn map_stale_resolution(
    row: &ResolutionOperationRow,
) -> Result<MutationConflict, ConflictManagementError> {
    Ok(MutationConflict::new(
        MutationConflictReason::from_str(
            row.stale_reason
                .as_deref()
                .ok_or(ConflictManagementError::InvalidPersistedData)?,
        )
        .map_err(|_| ConflictManagementError::InvalidPersistedData)?,
        decode_id(
            row.stale_resource_id
                .ok_or(ConflictManagementError::InvalidPersistedData)?,
        )?,
        parse_revision(row.stale_expected_revision.clone())?,
        parse_revision(row.stale_server_revision.clone())?,
        row.stale_server_state
            .as_deref()
            .map(parse_node_state)
            .transpose()?,
        row.stale_server_parent_id
            .map(decode_id::<NodeId>)
            .transpose()?,
        row.stale_server_name
            .as_ref()
            .map(|name| LogicalName::new(name.clone()))
            .transpose()
            .map_err(|_| ConflictManagementError::InvalidPersistedData)?,
        positive_sequence(
            row.stale_server_epoch
                .ok_or(ConflictManagementError::InvalidPersistedData)?,
        )?,
        nonnegative_sequence(
            row.stale_server_sequence
                .ok_or(ConflictManagementError::InvalidPersistedData)?,
        )?,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn terminalize_accept_server(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    conflict_id: SyncConflictId,
    resolution_id: ConflictResolutionId,
    completed_at: Timestamp,
) -> Result<(), ConflictManagementError> {
    update_resolution_rows(
        sqlx::query(
            "UPDATE sync_conflict_resolutions
             SET outcome = 'ACCEPTED_SERVER', completed_at = $2
             WHERE resolution_id = $1 AND outcome = 'IN_PROGRESS'",
        )
        .bind(resolution_id.into_uuid())
        .bind(completed_at.as_offset_datetime())
        .execute(&mut **transaction)
        .await
        .map_err(map_sqlx_error)?,
    )?;
    update_conflict_rows(
        sqlx::query(
            "UPDATE sync_conflicts
             SET lifecycle = 'DISMISSED', terminal_resolution_id = $5,
                 terminal_action = 'ACCEPT_SERVER', terminal_at = $6
             WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3
               AND conflict_id = $4 AND lifecycle = 'OPEN'",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(conflict_id.into_uuid())
        .bind(resolution_id.into_uuid())
        .bind(completed_at.as_offset_datetime())
        .execute(&mut **transaction)
        .await
        .map_err(map_sqlx_error)?,
    )
}

async fn terminalize_stale_resolution(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    resolution_id: ConflictResolutionId,
    stale: &MutationConflict,
    completed_at: Timestamp,
) -> Result<(), ConflictManagementError> {
    update_resolution_rows(
        sqlx::query(
            "UPDATE sync_conflict_resolutions
             SET outcome = 'STALE', stale_reason = $2, stale_resource_id = $3,
                 stale_expected_revision = $4::NUMERIC,
                 stale_server_revision = $5::NUMERIC,
                 stale_server_state = $6, stale_server_parent_id = $7,
                 stale_server_name = $8, stale_server_epoch = $9,
                 stale_server_sequence = $10, completed_at = $11
             WHERE resolution_id = $1 AND outcome = 'IN_PROGRESS'",
        )
        .bind(resolution_id.into_uuid())
        .bind(stale.reason().as_str())
        .bind(stale.resource_id().into_uuid())
        .bind(revision_text(stale.expected_revision()))
        .bind(revision_text(stale.current_revision()))
        .bind(stale.current_state().map(NodeState::as_str))
        .bind(stale.current_parent_id().map(NodeId::into_uuid))
        .bind(stale.current_name().map(LogicalName::as_str))
        .bind(i64_from_sequence(stale.server_epoch())?)
        .bind(i64_from_sequence(stale.server_sequence())?)
        .bind(completed_at.as_offset_datetime())
        .execute(&mut **transaction)
        .await
        .map_err(map_sqlx_error)?,
    )
}

#[allow(clippy::too_many_arguments)]
async fn terminalize_applied_resolution(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    conflict_id: SyncConflictId,
    resolution_id: ConflictResolutionId,
    journal_event_id: ChangeEventId,
    journal_sequence: Sequence,
    completed_at: Timestamp,
) -> Result<(), ConflictManagementError> {
    update_resolution_rows(
        sqlx::query(
            "UPDATE sync_conflict_resolutions
             SET outcome = 'APPLIED_CLIENT_INTENT', journal_event_id = $2,
                 journal_sequence = $3, completed_at = $4
             WHERE resolution_id = $1 AND outcome = 'IN_PROGRESS'",
        )
        .bind(resolution_id.into_uuid())
        .bind(journal_event_id.into_uuid())
        .bind(i64_from_sequence(journal_sequence)?)
        .bind(completed_at.as_offset_datetime())
        .execute(&mut **transaction)
        .await
        .map_err(map_sqlx_error)?,
    )?;
    update_conflict_rows(
        sqlx::query(
            "UPDATE sync_conflicts
             SET lifecycle = 'RESOLVED', terminal_resolution_id = $5,
                 terminal_action = 'APPLY_CLIENT_INTENT',
                 terminal_journal_event_id = $6, terminal_journal_sequence = $7,
                 terminal_at = $8
             WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3
               AND conflict_id = $4 AND lifecycle = 'OPEN'",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(conflict_id.into_uuid())
        .bind(resolution_id.into_uuid())
        .bind(journal_event_id.into_uuid())
        .bind(i64_from_sequence(journal_sequence)?)
        .bind(completed_at.as_offset_datetime())
        .execute(&mut **transaction)
        .await
        .map_err(map_sqlx_error)?,
    )
}

fn update_resolution_rows(
    result: sqlx::postgres::PgQueryResult,
) -> Result<(), ConflictManagementError> {
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(ConflictManagementError::InvalidPersistedData)
    }
}

fn update_conflict_rows(
    result: sqlx::postgres::PgQueryResult,
) -> Result<(), ConflictManagementError> {
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(ConflictManagementError::InvalidPersistedData)
    }
}

async fn database_now(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Timestamp, ConflictManagementError> {
    sqlx::query_scalar::<_, OffsetDateTime>("SELECT CURRENT_TIMESTAMP")
        .fetch_one(&mut **transaction)
        .await
        .map(Timestamp::from_offset_datetime)
        .map_err(map_sqlx_error)
}

fn decode_id<T>(value: Uuid) -> Result<T, ConflictManagementError>
where
    T: TryFrom<Uuid, Error = synveil_core::IdParseError>,
{
    T::try_from(value).map_err(|_| ConflictManagementError::InvalidPersistedData)
}

fn parse_revision(value: Option<String>) -> Result<Option<Revision>, ConflictManagementError> {
    value
        .as_deref()
        .map(Revision::from_str)
        .transpose()
        .map_err(|_| ConflictManagementError::InvalidPersistedData)
}

fn parse_node_state(value: &str) -> Result<NodeState, ConflictManagementError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        "PURGING" => Ok(NodeState::Purging),
        _ => Err(ConflictManagementError::InvalidPersistedData),
    }
}

fn positive_sequence(value: i64) -> Result<Sequence, ConflictManagementError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Sequence::new)
        .ok_or(ConflictManagementError::InvalidPersistedData)
}

fn nonnegative_sequence(value: i64) -> Result<Sequence, ConflictManagementError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| ConflictManagementError::InvalidPersistedData)
}

fn i64_from_sequence(value: Sequence) -> Result<i64, ConflictManagementError> {
    i64::try_from(value.get()).map_err(|_| ConflictManagementError::InvalidPersistedData)
}

fn i64_from_sequence_for_client(value: Sequence) -> Result<i64, ClientMutationError> {
    i64::try_from(value.get()).map_err(|_| ClientMutationError::InvalidPersistedData)
}

fn revision_text(value: Option<Revision>) -> Option<String> {
    value.map(|revision| revision.get().to_string())
}

fn map_client_mutation_error(error: ClientMutationError) -> ConflictManagementError {
    match error {
        ClientMutationError::NotFound => ConflictManagementError::NotFound,
        ClientMutationError::InvalidMutation => ConflictManagementError::InvalidConflictResolution,
        ClientMutationError::MutationIdConflict => ConflictManagementError::InternalError,
        ClientMutationError::RebaselineRequired {
            reason,
            current_epoch,
            minimum_retained_sequence,
        } => ConflictManagementError::RebaselineRequired {
            reason,
            current_epoch,
            minimum_retained_sequence,
        },
        ClientMutationError::DependencyUnavailable => {
            ConflictManagementError::DependencyUnavailable
        }
        ClientMutationError::Database(error) => ConflictManagementError::Database(error),
        ClientMutationError::InvalidPersistedData => ConflictManagementError::InvalidPersistedData,
        ClientMutationError::InternalError => ConflictManagementError::InternalError,
    }
}

fn map_metadata_error(error: MetadataError) -> ConflictManagementError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => ConflictManagementError::DependencyUnavailable,
        MetadataError::Database(error) => ConflictManagementError::Database(error),
        MetadataError::Mapping(_) => ConflictManagementError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => ConflictManagementError::DependencyUnavailable,
    }
}

fn map_sqlx_error(error: sqlx::Error) -> ConflictManagementError {
    map_metadata_error(MetadataError::from(error))
}

fn map_sqlx_client_error(error: sqlx::Error) -> ClientMutationError {
    match MetadataError::from(error) {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => ClientMutationError::DependencyUnavailable,
        MetadataError::Database(error) => ClientMutationError::Database(error),
        MetadataError::Mapping(_) | MetadataError::CapacityUnavailable => {
            ClientMutationError::InvalidPersistedData
        }
    }
}
