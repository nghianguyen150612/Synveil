//! PostgreSQL-backed logical snapshot/rebaseline bootstrap.
//!
//! Start uses the Prompt 31 per-library namespace guard, locks the journal
//! head, and copies the current logical Node projection into an immutable
//! manifest in one transaction. HTTP page reads therefore need no long-lived
//! PostgreSQL transaction and cannot mix later renames, moves, Trash/purge, or
//! content replacements into the captured cut.

use std::{fmt, str::FromStr};

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    DeviceId, DeviceSyncCheckpoint, FileVersionId, LibraryId, LogicalName, LogicalSnapshotNode,
    NodeId, NodeKind, NodeState, Revision, Sequence, Sha256Digest, SyncBootstrap, SyncBootstrapId,
    SyncBootstrapState, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    DatabaseError, DatabaseErrorKind, DatabasePool, MappingError, MetadataError, RebaselineReason,
    journal::acquire_namespace_guard,
};

pub const DEFAULT_SYNC_BOOTSTRAP_PAGE_LIMIT: u32 = 200;
pub const MAX_SYNC_BOOTSTRAP_PAGE_LIMIT: u32 = 1_000;
pub const DEFAULT_SYNC_BOOTSTRAP_TTL_SECONDS: i32 = 3_600;
pub const MAX_SYNC_BOOTSTRAP_CLEANUP_LIMIT: u32 = 1_000;

const COMPLETED_REPLAY_RETENTION_SECONDS: i32 = 86_400;

/// Stable failures for materialized logical bootstrap operations.
/// Inaccessible owner/device/library/session scopes intentionally collapse to
/// `NotFound` so IDs and cursor possession cannot enumerate resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineError {
    NotFound,
    InvalidLimit,
    InvalidCursor,
    InvalidBootstrapToken,
    BootstrapExpired,
    BootstrapConflict,
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

impl fmt::Display for RebaselineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "sync bootstrap resource was not found",
            Self::InvalidLimit => "sync bootstrap page limit is invalid",
            Self::InvalidCursor => "sync bootstrap cursor is invalid",
            Self::InvalidBootstrapToken => "sync bootstrap completion token is invalid",
            Self::BootstrapExpired => "sync bootstrap session has expired",
            Self::BootstrapConflict => "sync bootstrap session conflicts with newer progress",
            Self::RebaselineRequired { .. } => "a new sync rebaseline is required",
            Self::DependencyUnavailable => "sync bootstrap dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "sync bootstrap persisted data is invalid",
            Self::InternalError => "sync bootstrap internal error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RebaselineError {}

/// Server-verified cursor claims for one immutable manifest page boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapPagePosition {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    bootstrap_id: SyncBootstrapId,
    generation: Sequence,
    snapshot_epoch: Sequence,
    snapshot_resume_sequence: Sequence,
    after_node_id: NodeId,
}

impl BootstrapPagePosition {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        generation: Sequence,
        snapshot_epoch: Sequence,
        snapshot_resume_sequence: Sequence,
        after_node_id: NodeId,
    ) -> Self {
        Self {
            owner_user_id,
            device_id,
            library_id,
            bootstrap_id,
            generation,
            snapshot_epoch,
            snapshot_resume_sequence,
            after_node_id,
        }
    }

    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn bootstrap_id(self) -> SyncBootstrapId {
        self.bootstrap_id
    }

    #[must_use]
    pub const fn generation(self) -> Sequence {
        self.generation
    }

    #[must_use]
    pub const fn snapshot_epoch(self) -> Sequence {
        self.snapshot_epoch
    }

    #[must_use]
    pub const fn snapshot_resume_sequence(self) -> Sequence {
        self.snapshot_resume_sequence
    }

    #[must_use]
    pub const fn after_node_id(self) -> NodeId {
        self.after_node_id
    }
}

/// Claims issued only after the immutable manifest's terminal page is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapTerminalEvidence {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    bootstrap_id: SyncBootstrapId,
    generation: Sequence,
    snapshot_epoch: Sequence,
    snapshot_resume_sequence: Sequence,
    manifest_item_count: u64,
    terminal_node_id: Option<NodeId>,
}

impl BootstrapTerminalEvidence {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        generation: Sequence,
        snapshot_epoch: Sequence,
        snapshot_resume_sequence: Sequence,
        manifest_item_count: u64,
        terminal_node_id: Option<NodeId>,
    ) -> Self {
        Self {
            owner_user_id,
            device_id,
            library_id,
            bootstrap_id,
            generation,
            snapshot_epoch,
            snapshot_resume_sequence,
            manifest_item_count,
            terminal_node_id,
        }
    }

    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn bootstrap_id(self) -> SyncBootstrapId {
        self.bootstrap_id
    }

    #[must_use]
    pub const fn generation(self) -> Sequence {
        self.generation
    }

    #[must_use]
    pub const fn snapshot_epoch(self) -> Sequence {
        self.snapshot_epoch
    }

    #[must_use]
    pub const fn snapshot_resume_sequence(self) -> Sequence {
        self.snapshot_resume_sequence
    }

    #[must_use]
    pub const fn manifest_item_count(self) -> u64 {
        self.manifest_item_count
    }

    #[must_use]
    pub const fn terminal_node_id(self) -> Option<NodeId> {
        self.terminal_node_id
    }
}

/// Completion proof is a distinct type even though its claims match the
/// terminal-page evidence. Only the API's HMAC verifier constructs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapCompletionEvidence(BootstrapTerminalEvidence);

impl BootstrapCompletionEvidence {
    #[must_use]
    pub const fn new(evidence: BootstrapTerminalEvidence) -> Self {
        Self(evidence)
    }

    #[must_use]
    pub const fn terminal(self) -> BootstrapTerminalEvidence {
        self.0
    }
}

/// One bounded immutable page. The service returns typed keyset state; only
/// the HTTP boundary serializes it into a signed opaque cursor/token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotNodePage {
    bootstrap: SyncBootstrap,
    nodes: Vec<LogicalSnapshotNode>,
    has_more: bool,
    next_position: Option<BootstrapPagePosition>,
    terminal_evidence: Option<BootstrapTerminalEvidence>,
}

impl SnapshotNodePage {
    #[must_use]
    pub fn new(
        bootstrap: SyncBootstrap,
        nodes: Vec<LogicalSnapshotNode>,
        has_more: bool,
        next_position: Option<BootstrapPagePosition>,
        terminal_evidence: Option<BootstrapTerminalEvidence>,
    ) -> Self {
        Self {
            bootstrap,
            nodes,
            has_more,
            next_position,
            terminal_evidence,
        }
    }

    #[must_use]
    pub const fn bootstrap(&self) -> SyncBootstrap {
        self.bootstrap
    }

    #[must_use]
    pub fn nodes(&self) -> &[LogicalSnapshotNode] {
        &self.nodes
    }

    #[must_use]
    pub fn into_nodes(self) -> Vec<LogicalSnapshotNode> {
        self.nodes
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub const fn next_position(&self) -> Option<BootstrapPagePosition> {
        self.next_position
    }

    #[must_use]
    pub const fn terminal_evidence(&self) -> Option<BootstrapTerminalEvidence> {
        self.terminal_evidence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapCompletion {
    bootstrap: SyncBootstrap,
    checkpoint: DeviceSyncCheckpoint,
    replayed: bool,
}

impl BootstrapCompletion {
    #[must_use]
    pub const fn new(
        bootstrap: SyncBootstrap,
        checkpoint: DeviceSyncCheckpoint,
        replayed: bool,
    ) -> Self {
        Self {
            bootstrap,
            checkpoint,
            replayed,
        }
    }

    #[must_use]
    pub const fn bootstrap(self) -> SyncBootstrap {
        self.bootstrap
    }

    #[must_use]
    pub const fn checkpoint(self) -> DeviceSyncCheckpoint {
        self.checkpoint
    }

    #[must_use]
    pub const fn replayed(self) -> bool {
        self.replayed
    }
}

#[derive(Clone, Debug, FromRow)]
struct ScopeRow {
    device_owner_user_id: Uuid,
    device_status: String,
    library_owner_user_id: Uuid,
    library_status: String,
    journal_epoch: i64,
    sync_head: i64,
    minimum_retained_sequence: i64,
}

#[derive(Clone, Copy, Debug)]
struct Scope {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    journal_epoch: Sequence,
    sync_head: Sequence,
    minimum_retained_sequence: Sequence,
}

#[derive(Clone, Debug, FromRow)]
struct BootstrapRow {
    id: Uuid,
    owner_user_id: Uuid,
    device_id: Uuid,
    library_id: Uuid,
    generation: i64,
    snapshot_epoch: i64,
    snapshot_resume_sequence: i64,
    manifest_item_count: i64,
    terminal_node_id: Option<Uuid>,
    state: String,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
    completed_at: Option<OffsetDateTime>,
    is_expired: bool,
}

#[derive(Clone, Debug, FromRow)]
struct ManifestNodeRow {
    node_id: Uuid,
    parent_node_id: Option<Uuid>,
    name: String,
    kind: String,
    state: String,
    revision: String,
    current_version_id: Option<Uuid>,
    content_length: Option<String>,
    content_sha256: Option<Vec<u8>>,
}

#[derive(Clone, Debug, FromRow)]
struct CheckpointRow {
    owner_user_id: Uuid,
    device_id: Uuid,
    library_id: Uuid,
    journal_epoch: i64,
    acknowledged_sequence: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    last_seen_high_watermark: Option<i64>,
    rebaseline_generation: i64,
}

#[derive(Clone)]
pub struct SyncBootstrapService {
    pool: DatabasePool,
}

impl SyncBootstrapService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Start or retry the one active bootstrap for this device/library.
    ///
    /// The namespace guard is acquired before the journal-head row, matching
    /// every Prompt 31 logical mutation. While both are held, one INSERT ..
    /// SELECT copies the logical projection without buffering it in Rust.
    pub async fn start(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> Result<SyncBootstrap, RebaselineError> {
        let mut transaction = self.begin_transaction().await?;
        acquire_namespace_guard(&mut transaction, library_id)
            .await
            .map_err(map_metadata_error)?;
        let scope =
            load_scope(&mut transaction, owner_user_id, device_id, library_id, true).await?;

        sqlx::query(
            "UPDATE sync_bootstraps
             SET state = 'EXPIRED'
             WHERE owner_user_id = $1
               AND device_id = $2
               AND library_id = $3
               AND state = 'OPEN'
               AND expires_at <= CURRENT_TIMESTAMP",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        if let Some(row) = load_open_bootstrap(&mut transaction, scope).await? {
            let bootstrap = map_bootstrap(row)?;
            transaction
                .commit()
                .await
                .map_err(MetadataError::from)
                .map_err(map_metadata_error)?;
            return Ok(bootstrap);
        }

        let checkpoint = ensure_checkpoint_for_update(&mut transaction, scope).await?;
        validate_checkpoint_row(&checkpoint, scope)?;
        let generation = checkpoint
            .rebaseline_generation
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or(RebaselineError::InvalidPersistedData)?;
        let updated = sqlx::query(
            "UPDATE device_sync_checkpoints
             SET rebaseline_generation = $4,
                 updated_at = CURRENT_TIMESTAMP
             WHERE owner_user_id = $1
               AND device_id = $2
               AND library_id = $3
               AND rebaseline_generation = $5",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(generation)
        .bind(checkpoint.rebaseline_generation)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        if updated.rows_affected() != 1 {
            return Err(RebaselineError::BootstrapConflict);
        }

        let bootstrap_id = SyncBootstrapId::new();
        sqlx::query(
            "INSERT INTO sync_bootstraps
                (id, owner_user_id, device_id, library_id, generation,
                 snapshot_epoch, snapshot_resume_sequence,
                 manifest_item_count, terminal_node_id, state, created_at,
                 expires_at, completed_at)
             VALUES
                ($1, $2, $3, $4, $5, $6, $7, 0, NULL, 'OPEN',
                 CURRENT_TIMESTAMP,
                 CURRENT_TIMESTAMP + ($8 * INTERVAL '1 second'), NULL)",
        )
        .bind(bootstrap_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(generation)
        .bind(i64_from_sequence(scope.journal_epoch)?)
        .bind(i64_from_sequence(scope.sync_head)?)
        .bind(DEFAULT_SYNC_BOOTSTRAP_TTL_SECONDS)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        let inserted = sqlx::query(
            "INSERT INTO sync_bootstrap_nodes
                (bootstrap_id, node_id, parent_node_id, name, kind, state,
                 revision, current_version_id, content_length, content_sha256)
             SELECT $1, n.id, n.parent_node_id, n.name, n.kind, n.state,
                    n.revision, n.current_version_id,
                    o.plaintext_length, o.canonical_hash
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
        .bind(bootstrap_id.into_uuid())
        .bind(library_id.into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        let manifest_item_count = i64::try_from(inserted.rows_affected())
            .map_err(|_| RebaselineError::InvalidPersistedData)?;
        let terminal_node_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT node_id
             FROM sync_bootstrap_nodes
             WHERE bootstrap_id = $1
             ORDER BY node_id DESC
             LIMIT 1",
        )
        .bind(bootstrap_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        let row = sqlx::query_as::<_, BootstrapRow>(
            "UPDATE sync_bootstraps
             SET manifest_item_count = $2,
                 terminal_node_id = $3
             WHERE id = $1
             RETURNING id, owner_user_id, device_id, library_id, generation,
                       snapshot_epoch, snapshot_resume_sequence,
                       manifest_item_count, terminal_node_id, state, created_at,
                       expires_at, completed_at,
                       CURRENT_TIMESTAMP >= expires_at AS is_expired",
        )
        .bind(bootstrap_id.into_uuid())
        .bind(manifest_item_count)
        .bind(terminal_node_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(RebaselineError::InvalidPersistedData)?;
        let bootstrap = map_bootstrap(row)?;
        validate_bootstrap_scope(bootstrap, scope)?;

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(bootstrap)
    }

    /// Read one immutable Node-ID keyset page. This never updates the durable
    /// device checkpoint (or any other row).
    pub async fn page_nodes(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        position: Option<BootstrapPagePosition>,
        limit: u32,
    ) -> Result<SnapshotNodePage, RebaselineError> {
        validate_page_limit(limit)?;
        let mut transaction = self.begin_transaction().await?;
        let scope = load_scope(
            &mut transaction,
            owner_user_id,
            device_id,
            library_id,
            false,
        )
        .await?;
        let row = load_bootstrap(
            &mut transaction,
            owner_user_id,
            device_id,
            library_id,
            bootstrap_id,
            false,
        )
        .await?
        .ok_or(RebaselineError::NotFound)?;
        if row.is_expired && row.state == "OPEN" {
            return Err(RebaselineError::BootstrapExpired);
        }
        let bootstrap = map_bootstrap(row)?;
        validate_bootstrap_scope(bootstrap, scope)?;
        match bootstrap.state() {
            SyncBootstrapState::Open | SyncBootstrapState::Completed => {}
            SyncBootstrapState::Expired => return Err(RebaselineError::BootstrapExpired),
            SyncBootstrapState::Aborted => return Err(RebaselineError::BootstrapConflict),
        }

        let after_node_id = match position {
            Some(position) => {
                validate_position(position, bootstrap, owner_user_id, device_id, library_id)?;
                Some(position.after_node_id())
            }
            None => None,
        };
        let rows = sqlx::query_as::<_, ManifestNodeRow>(
            "SELECT node_id, parent_node_id, name, kind, state,
                    revision::TEXT AS revision, current_version_id,
                    content_length::TEXT AS content_length, content_sha256
             FROM sync_bootstrap_nodes
             WHERE bootstrap_id = $1
               AND ($2::UUID IS NULL OR node_id > $2)
             ORDER BY node_id ASC
             LIMIT $3",
        )
        .bind(bootstrap_id.into_uuid())
        .bind(after_node_id.map(NodeId::into_uuid))
        .bind(i64::from(limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        let has_more = rows.len() > limit as usize;
        let nodes = rows
            .into_iter()
            .take(limit as usize)
            .map(map_manifest_node)
            .collect::<Result<Vec<_>, _>>()?;
        let served_through = nodes.last().map(LogicalSnapshotNode::node_id);

        let next_position = if has_more {
            let after = served_through.ok_or(RebaselineError::InvalidPersistedData)?;
            Some(BootstrapPagePosition::new(
                owner_user_id,
                device_id,
                library_id,
                bootstrap_id,
                bootstrap.generation(),
                bootstrap.snapshot_epoch(),
                bootstrap.snapshot_resume_sequence(),
                after,
            ))
        } else {
            None
        };
        let terminal_evidence = if has_more {
            None
        } else {
            validate_terminal_page(bootstrap, after_node_id, served_through)?;
            Some(BootstrapTerminalEvidence::new(
                owner_user_id,
                device_id,
                library_id,
                bootstrap_id,
                bootstrap.generation(),
                bootstrap.snapshot_epoch(),
                bootstrap.snapshot_resume_sequence(),
                bootstrap.manifest_item_count(),
                bootstrap.terminal_node_id(),
            ))
        };

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(SnapshotNodePage {
            bootstrap,
            nodes,
            has_more,
            next_position,
            terminal_evidence,
        })
    }

    /// Atomically replace the device checkpoint with the captured journal cut
    /// after terminal-page evidence has been verified by the API boundary.
    pub async fn complete(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        evidence: BootstrapCompletionEvidence,
    ) -> Result<BootstrapCompletion, RebaselineError> {
        let terminal = evidence.terminal();
        if terminal.owner_user_id() != owner_user_id
            || terminal.device_id() != device_id
            || terminal.library_id() != library_id
            || terminal.bootstrap_id() != bootstrap_id
        {
            return Err(RebaselineError::InvalidBootstrapToken);
        }

        let mut transaction = self.begin_transaction().await?;
        let scope =
            load_scope(&mut transaction, owner_user_id, device_id, library_id, true).await?;
        let row = load_bootstrap(
            &mut transaction,
            owner_user_id,
            device_id,
            library_id,
            bootstrap_id,
            true,
        )
        .await?
        .ok_or(RebaselineError::NotFound)?;
        let is_expired = row.is_expired;
        let bootstrap = map_bootstrap(row)?;
        validate_bootstrap_scope(bootstrap, scope)?;
        validate_completion_evidence(terminal, bootstrap)?;

        let checkpoint = load_checkpoint_for_update(&mut transaction, scope)
            .await?
            .ok_or(RebaselineError::BootstrapConflict)?;
        validate_checkpoint_row(&checkpoint, scope)?;

        if bootstrap.state() == SyncBootstrapState::Completed {
            let checkpoint = map_checkpoint(checkpoint)?;
            transaction
                .commit()
                .await
                .map_err(MetadataError::from)
                .map_err(map_metadata_error)?;
            return Ok(BootstrapCompletion {
                bootstrap,
                checkpoint,
                replayed: true,
            });
        }
        if is_expired || bootstrap.state() == SyncBootstrapState::Expired {
            return Err(RebaselineError::BootstrapExpired);
        }
        if bootstrap.state() != SyncBootstrapState::Open {
            return Err(RebaselineError::BootstrapConflict);
        }

        validate_cut_still_usable(scope, bootstrap)?;
        let expected_generation = i64_from_sequence(bootstrap.generation())?;
        if checkpoint.rebaseline_generation != expected_generation {
            return Err(RebaselineError::BootstrapConflict);
        }
        if checkpoint.journal_epoch == i64_from_sequence(bootstrap.snapshot_epoch())?
            && checkpoint.acknowledged_sequence
                > i64_from_sequence(bootstrap.snapshot_resume_sequence())?
        {
            return Err(RebaselineError::BootstrapConflict);
        }

        let checkpoint = sqlx::query_as::<_, CheckpointRow>(
            "UPDATE device_sync_checkpoints
             SET journal_epoch = $4,
                 acknowledged_sequence = $5,
                 last_seen_high_watermark = $5,
                 updated_at = CURRENT_TIMESTAMP
             WHERE owner_user_id = $1
               AND device_id = $2
               AND library_id = $3
               AND rebaseline_generation = $6
               AND (
                    journal_epoch <> $4
                    OR acknowledged_sequence <= $5
               )
             RETURNING owner_user_id, device_id, library_id, journal_epoch,
                       acknowledged_sequence, created_at, updated_at,
                       last_seen_high_watermark, rebaseline_generation",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(i64_from_sequence(bootstrap.snapshot_epoch())?)
        .bind(i64_from_sequence(bootstrap.snapshot_resume_sequence())?)
        .bind(expected_generation)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(RebaselineError::BootstrapConflict)?;

        let completed_row = sqlx::query_as::<_, BootstrapRow>(
            "UPDATE sync_bootstraps
             SET state = 'COMPLETED',
                 completed_at = CURRENT_TIMESTAMP
             WHERE id = $1
               AND owner_user_id = $2
               AND device_id = $3
               AND library_id = $4
               AND generation = $5
               AND state = 'OPEN'
               AND expires_at > CURRENT_TIMESTAMP
             RETURNING id, owner_user_id, device_id, library_id, generation,
                       snapshot_epoch, snapshot_resume_sequence,
                       manifest_item_count, terminal_node_id, state, created_at,
                       expires_at, completed_at,
                       CURRENT_TIMESTAMP >= expires_at AS is_expired",
        )
        .bind(bootstrap_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(expected_generation)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(RebaselineError::BootstrapConflict)?;
        let completed = map_bootstrap(completed_row)?;
        let checkpoint = map_checkpoint(checkpoint)?;

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(BootstrapCompletion {
            bootstrap: completed,
            checkpoint,
            replayed: false,
        })
    }

    /// Bounded cleanup for retired manifests. Canonical Node, FileVersion,
    /// Object, journal, and checkpoint rows are never selected by this query.
    pub async fn cleanup_retired(&self, limit: u32) -> Result<u64, RebaselineError> {
        if !(1..=MAX_SYNC_BOOTSTRAP_CLEANUP_LIMIT).contains(&limit) {
            return Err(RebaselineError::InvalidLimit);
        }
        let mut transaction = self.begin_transaction().await?;
        sqlx::query(
            "WITH expired AS (
                 SELECT id
                 FROM sync_bootstraps
                 WHERE state = 'OPEN'
                   AND expires_at <= CURRENT_TIMESTAMP
                 ORDER BY expires_at, id
                 LIMIT $1
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE sync_bootstraps AS b
             SET state = 'EXPIRED'
             FROM expired
             WHERE b.id = expired.id",
        )
        .bind(i64::from(limit))
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        let deleted = sqlx::query(
            "WITH retired AS (
                 SELECT id
                 FROM sync_bootstraps
                 WHERE (
                     state IN ('EXPIRED', 'ABORTED')
                     AND expires_at <= CURRENT_TIMESTAMP
                 ) OR (
                     state = 'COMPLETED'
                     AND completed_at <= CURRENT_TIMESTAMP
                         - ($2 * INTERVAL '1 second')
                 )
                 ORDER BY COALESCE(completed_at, expires_at), id
                 LIMIT $1
                 FOR UPDATE SKIP LOCKED
             )
             DELETE FROM sync_bootstraps AS b
             USING retired
             WHERE b.id = retired.id",
        )
        .bind(i64::from(limit))
        .bind(COMPLETED_REPLAY_RETENTION_SECONDS)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(deleted.rows_affected())
    }

    async fn begin_transaction(&self) -> Result<Transaction<'_, Postgres>, RebaselineError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(transaction)
    }
}

async fn load_scope(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    lock_library_exclusive: bool,
) -> Result<Scope, RebaselineError> {
    let statement = if lock_library_exclusive {
        "SELECT d.owner_user_id AS device_owner_user_id,
                d.status AS device_status,
                l.owner_user_id AS library_owner_user_id,
                l.status AS library_status,
                l.journal_epoch, l.sync_head, l.minimum_retained_sequence
         FROM devices AS d
         CROSS JOIN libraries AS l
         WHERE d.id = $1 AND l.id = $2
         FOR UPDATE OF l FOR SHARE OF d"
    } else {
        "SELECT d.owner_user_id AS device_owner_user_id,
                d.status AS device_status,
                l.owner_user_id AS library_owner_user_id,
                l.status AS library_status,
                l.journal_epoch, l.sync_head, l.minimum_retained_sequence
         FROM devices AS d
         CROSS JOIN libraries AS l
         WHERE d.id = $1 AND l.id = $2
         FOR SHARE OF d, l"
    };
    let row = sqlx::query_as::<_, ScopeRow>(statement)
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(RebaselineError::NotFound)?;
    if row.device_owner_user_id != owner_user_id.into_uuid()
        || row.library_owner_user_id != owner_user_id.into_uuid()
        || row.device_status != "ACTIVE"
        || row.library_status == "DELETING"
    {
        return Err(RebaselineError::NotFound);
    }
    let journal_epoch = positive_sequence(row.journal_epoch)?;
    let sync_head = nonnegative_sequence(row.sync_head)?;
    let minimum_retained_sequence = nonnegative_sequence(row.minimum_retained_sequence)?;
    if minimum_retained_sequence > sync_head {
        return Err(RebaselineError::InvalidPersistedData);
    }
    Ok(Scope {
        owner_user_id,
        device_id,
        library_id,
        journal_epoch,
        sync_head,
        minimum_retained_sequence,
    })
}

async fn load_open_bootstrap(
    transaction: &mut Transaction<'_, Postgres>,
    scope: Scope,
) -> Result<Option<BootstrapRow>, RebaselineError> {
    sqlx::query_as::<_, BootstrapRow>(
        "SELECT id, owner_user_id, device_id, library_id, generation,
                snapshot_epoch, snapshot_resume_sequence,
                manifest_item_count, terminal_node_id, state, created_at,
                expires_at, completed_at,
                CURRENT_TIMESTAMP >= expires_at AS is_expired
         FROM sync_bootstraps
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
           AND state = 'OPEN'
         FOR UPDATE",
    )
    .bind(scope.owner_user_id.into_uuid())
    .bind(scope.device_id.into_uuid())
    .bind(scope.library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)
}

async fn load_bootstrap(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    bootstrap_id: SyncBootstrapId,
    for_update: bool,
) -> Result<Option<BootstrapRow>, RebaselineError> {
    let statement = if for_update {
        "SELECT id, owner_user_id, device_id, library_id, generation,
                snapshot_epoch, snapshot_resume_sequence,
                manifest_item_count, terminal_node_id, state, created_at,
                expires_at, completed_at,
                CURRENT_TIMESTAMP >= expires_at AS is_expired
         FROM sync_bootstraps
         WHERE id = $1 AND owner_user_id = $2 AND device_id = $3 AND library_id = $4
         FOR UPDATE"
    } else {
        "SELECT id, owner_user_id, device_id, library_id, generation,
                snapshot_epoch, snapshot_resume_sequence,
                manifest_item_count, terminal_node_id, state, created_at,
                expires_at, completed_at,
                CURRENT_TIMESTAMP >= expires_at AS is_expired
         FROM sync_bootstraps
         WHERE id = $1 AND owner_user_id = $2 AND device_id = $3 AND library_id = $4"
    };
    sqlx::query_as::<_, BootstrapRow>(statement)
        .bind(bootstrap_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)
}

async fn ensure_checkpoint_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    scope: Scope,
) -> Result<CheckpointRow, RebaselineError> {
    sqlx::query(
        "INSERT INTO device_sync_checkpoints
            (owner_user_id, device_id, library_id, journal_epoch,
             acknowledged_sequence, created_at, updated_at,
             last_seen_high_watermark, rebaseline_generation)
         VALUES ($1, $2, $3, $4, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL, 0)
         ON CONFLICT (device_id, library_id) DO NOTHING",
    )
    .bind(scope.owner_user_id.into_uuid())
    .bind(scope.device_id.into_uuid())
    .bind(scope.library_id.into_uuid())
    .bind(i64_from_sequence(scope.journal_epoch)?)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?;
    load_checkpoint_for_update(transaction, scope)
        .await?
        .ok_or(RebaselineError::InvalidPersistedData)
}

async fn load_checkpoint_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    scope: Scope,
) -> Result<Option<CheckpointRow>, RebaselineError> {
    sqlx::query_as::<_, CheckpointRow>(
        "SELECT owner_user_id, device_id, library_id, journal_epoch,
                acknowledged_sequence, created_at, updated_at,
                last_seen_high_watermark, rebaseline_generation
         FROM device_sync_checkpoints
         WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3
         FOR UPDATE",
    )
    .bind(scope.owner_user_id.into_uuid())
    .bind(scope.device_id.into_uuid())
    .bind(scope.library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)
}

fn map_bootstrap(row: BootstrapRow) -> Result<SyncBootstrap, RebaselineError> {
    let id = SyncBootstrapId::try_from_uuid(row.id)
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let owner_user_id = UserId::try_from_uuid(row.owner_user_id)
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let device_id = DeviceId::try_from_uuid(row.device_id)
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let library_id = LibraryId::try_from_uuid(row.library_id)
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let generation = positive_sequence(row.generation)?;
    let snapshot_epoch = positive_sequence(row.snapshot_epoch)?;
    let snapshot_resume_sequence = nonnegative_sequence(row.snapshot_resume_sequence)?;
    let manifest_item_count = u64::try_from(row.manifest_item_count)
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let terminal_node_id = row
        .terminal_node_id
        .map(NodeId::try_from_uuid)
        .transpose()
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    if (manifest_item_count == 0) != terminal_node_id.is_none() {
        return Err(RebaselineError::InvalidPersistedData);
    }
    let state = SyncBootstrapState::from_str(&row.state)
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let created_at = Timestamp::from_offset_datetime(row.created_at);
    let expires_at = Timestamp::from_offset_datetime(row.expires_at);
    let completed_at = row.completed_at.map(Timestamp::from_offset_datetime);
    if expires_at <= created_at
        || (state == SyncBootstrapState::Completed) != completed_at.is_some()
    {
        return Err(RebaselineError::InvalidPersistedData);
    }
    Ok(SyncBootstrap::new(
        id,
        owner_user_id,
        device_id,
        library_id,
        generation,
        snapshot_epoch,
        snapshot_resume_sequence,
        manifest_item_count,
        terminal_node_id,
        state,
        created_at,
        expires_at,
        completed_at,
    ))
}

fn map_manifest_node(row: ManifestNodeRow) -> Result<LogicalSnapshotNode, RebaselineError> {
    let node_id =
        NodeId::try_from_uuid(row.node_id).map_err(|_| RebaselineError::InvalidPersistedData)?;
    let parent_node_id = row
        .parent_node_id
        .map(NodeId::try_from_uuid)
        .transpose()
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let name = LogicalName::new(row.name).map_err(|_| RebaselineError::InvalidPersistedData)?;
    let kind = match row.kind.as_str() {
        "FILE" => NodeKind::File,
        "DIRECTORY" => NodeKind::Directory,
        _ => return Err(RebaselineError::InvalidPersistedData),
    };
    let state = match row.state.as_str() {
        "ACTIVE" => NodeState::Active,
        "TRASHED" => NodeState::Trashed,
        _ => return Err(RebaselineError::InvalidPersistedData),
    };
    let revision =
        Revision::from_str(&row.revision).map_err(|_| RebaselineError::InvalidPersistedData)?;
    let current_version_id = row
        .current_version_id
        .map(FileVersionId::try_from_uuid)
        .transpose()
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let content_length = row
        .content_length
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    let content_sha256 = row
        .content_sha256
        .as_deref()
        .map(Sha256Digest::try_from)
        .transpose()
        .map_err(|_| RebaselineError::InvalidPersistedData)?;
    LogicalSnapshotNode::new(
        node_id,
        parent_node_id,
        name,
        kind,
        state,
        revision,
        current_version_id,
        content_length,
        content_sha256,
    )
    .map_err(|_| RebaselineError::InvalidPersistedData)
}

fn map_checkpoint(row: CheckpointRow) -> Result<DeviceSyncCheckpoint, RebaselineError> {
    Ok(DeviceSyncCheckpoint::new(
        UserId::try_from_uuid(row.owner_user_id)
            .map_err(|_| RebaselineError::InvalidPersistedData)?,
        DeviceId::try_from_uuid(row.device_id)
            .map_err(|_| RebaselineError::InvalidPersistedData)?,
        LibraryId::try_from_uuid(row.library_id)
            .map_err(|_| RebaselineError::InvalidPersistedData)?,
        positive_sequence(row.journal_epoch)?,
        nonnegative_sequence(row.acknowledged_sequence)?,
        Timestamp::from_offset_datetime(row.created_at),
        Timestamp::from_offset_datetime(row.updated_at),
        row.last_seen_high_watermark
            .map(nonnegative_sequence)
            .transpose()?,
    ))
}

fn validate_checkpoint_row(row: &CheckpointRow, scope: Scope) -> Result<(), RebaselineError> {
    if row.owner_user_id != scope.owner_user_id.into_uuid()
        || row.device_id != scope.device_id.into_uuid()
        || row.library_id != scope.library_id.into_uuid()
        || row.journal_epoch <= 0
        || row.acknowledged_sequence < 0
        || row.rebaseline_generation < 0
    {
        return Err(RebaselineError::InvalidPersistedData);
    }
    if row.journal_epoch == i64_from_sequence(scope.journal_epoch)?
        && row.acknowledged_sequence > i64_from_sequence(scope.sync_head)?
    {
        return Err(RebaselineError::InvalidPersistedData);
    }
    Ok(())
}

fn validate_bootstrap_scope(bootstrap: SyncBootstrap, scope: Scope) -> Result<(), RebaselineError> {
    if bootstrap.owner_user_id() != scope.owner_user_id
        || bootstrap.device_id() != scope.device_id
        || bootstrap.library_id() != scope.library_id
    {
        return Err(RebaselineError::InvalidPersistedData);
    }
    Ok(())
}

fn validate_position(
    position: BootstrapPagePosition,
    bootstrap: SyncBootstrap,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
) -> Result<(), RebaselineError> {
    if position.owner_user_id() != owner_user_id
        || position.device_id() != device_id
        || position.library_id() != library_id
        || position.bootstrap_id() != bootstrap.id()
        || position.generation() != bootstrap.generation()
        || position.snapshot_epoch() != bootstrap.snapshot_epoch()
        || position.snapshot_resume_sequence() != bootstrap.snapshot_resume_sequence()
        || bootstrap
            .terminal_node_id()
            .is_none_or(|terminal| position.after_node_id() >= terminal)
    {
        return Err(RebaselineError::InvalidCursor);
    }
    Ok(())
}

fn validate_terminal_page(
    bootstrap: SyncBootstrap,
    after_node_id: Option<NodeId>,
    served_through: Option<NodeId>,
) -> Result<(), RebaselineError> {
    match (
        bootstrap.manifest_item_count(),
        bootstrap.terminal_node_id(),
        served_through,
    ) {
        (0, None, None) if after_node_id.is_none() => Ok(()),
        (_, Some(terminal), Some(served)) if terminal == served => Ok(()),
        _ => Err(RebaselineError::InvalidCursor),
    }
}

fn validate_completion_evidence(
    evidence: BootstrapTerminalEvidence,
    bootstrap: SyncBootstrap,
) -> Result<(), RebaselineError> {
    if evidence.owner_user_id() != bootstrap.owner_user_id()
        || evidence.device_id() != bootstrap.device_id()
        || evidence.library_id() != bootstrap.library_id()
        || evidence.bootstrap_id() != bootstrap.id()
        || evidence.generation() != bootstrap.generation()
        || evidence.snapshot_epoch() != bootstrap.snapshot_epoch()
        || evidence.snapshot_resume_sequence() != bootstrap.snapshot_resume_sequence()
        || evidence.manifest_item_count() != bootstrap.manifest_item_count()
        || evidence.terminal_node_id() != bootstrap.terminal_node_id()
    {
        return Err(RebaselineError::InvalidBootstrapToken);
    }
    Ok(())
}

fn validate_cut_still_usable(
    scope: Scope,
    bootstrap: SyncBootstrap,
) -> Result<(), RebaselineError> {
    if scope.journal_epoch != bootstrap.snapshot_epoch() {
        return Err(rebaseline_required(scope, RebaselineReason::EpochMismatch));
    }
    if bootstrap.snapshot_resume_sequence() > scope.sync_head {
        return Err(RebaselineError::InvalidPersistedData);
    }
    // Prompt 32 defines minimum_retained_sequence as the minimum checkpoint
    // position accepted by its feed service. Match that conservative boundary
    // exactly so successful completion is immediately feed-readable.
    if bootstrap.snapshot_resume_sequence() < scope.minimum_retained_sequence {
        return Err(rebaseline_required(
            scope,
            RebaselineReason::HistoryUnavailable,
        ));
    }
    Ok(())
}

fn validate_page_limit(limit: u32) -> Result<(), RebaselineError> {
    if (1..=MAX_SYNC_BOOTSTRAP_PAGE_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(RebaselineError::InvalidLimit)
    }
}

fn positive_sequence(value: i64) -> Result<Sequence, RebaselineError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Sequence::new)
        .ok_or(RebaselineError::InvalidPersistedData)
}

fn nonnegative_sequence(value: i64) -> Result<Sequence, RebaselineError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| RebaselineError::InvalidPersistedData)
}

fn i64_from_sequence(value: Sequence) -> Result<i64, RebaselineError> {
    i64::try_from(value.get()).map_err(|_| RebaselineError::InvalidPersistedData)
}

fn rebaseline_required(scope: Scope, reason: RebaselineReason) -> RebaselineError {
    RebaselineError::RebaselineRequired {
        reason,
        current_epoch: scope.journal_epoch,
        minimum_retained_sequence: scope.minimum_retained_sequence,
    }
}

fn map_metadata_error(error: MetadataError) -> RebaselineError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => RebaselineError::DependencyUnavailable,
        MetadataError::Database(error) => RebaselineError::Database(error),
        MetadataError::Mapping(MappingError::InvalidId { .. })
        | MetadataError::Mapping(MappingError::InvalidTimestamp { .. })
        | MetadataError::Mapping(MappingError::TimestampPrecisionLoss { .. })
        | MetadataError::Mapping(MappingError::InvalidDecimal { .. })
        | MetadataError::Mapping(MappingError::InvalidDigest { .. })
        | MetadataError::Mapping(MappingError::InvalidEnum { .. })
        | MetadataError::Mapping(MappingError::InvalidName { .. })
        | MetadataError::Mapping(MappingError::InvalidLogin { .. })
        | MetadataError::Mapping(MappingError::RelationMismatch { .. })
        | MetadataError::Mapping(MappingError::RevisionConflict { .. })
        | MetadataError::Mapping(MappingError::Domain(_)) => RebaselineError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => RebaselineError::DependencyUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SYNC_BOOTSTRAP_PAGE_LIMIT, RebaselineError, validate_page_limit};

    #[test]
    fn bootstrap_page_limits_are_bounded() {
        assert_eq!(validate_page_limit(0), Err(RebaselineError::InvalidLimit));
        assert_eq!(validate_page_limit(1), Ok(()));
        assert_eq!(validate_page_limit(MAX_SYNC_BOOTSTRAP_PAGE_LIMIT), Ok(()));
        assert_eq!(
            validate_page_limit(MAX_SYNC_BOOTSTRAP_PAGE_LIMIT + 1),
            Err(RebaselineError::InvalidLimit)
        );
    }
}
