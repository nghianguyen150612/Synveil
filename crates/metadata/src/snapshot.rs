//! PostgreSQL-backed transport-neutral logical rebaseline snapshots.
//!
//! Prompt 81's in-memory builder captures a library-scoped logical state and
//! its journal high-watermark from one repeatable-read database view. Prompt 82
//! additionally persists that validated cut as an immutable artifact, so
//! independently bounded page reads survive a connection or process restart
//! without reading the live namespace again.

use std::{fmt, time::Duration};

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    FileVersionId, LibraryId, LogicalName, LogicalSnapshot, LogicalSnapshotError,
    LogicalSnapshotNode, NodeId, NodeKind, NodeState, RebaselineSnapshotId,
    RebaselineSnapshotPageCursor, Revision, Sequence, Sha256Digest, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    DatabaseError, DatabaseErrorKind, DatabasePool, MappingError, MetadataError,
    journal::{JournalHighWatermark, acquire_namespace_guard},
    retention::DEFAULT_HANDOFF_PROOF_RETENTION_SECONDS,
};

/// Bounded internal page size for immutable rebaseline transfer artifacts.
pub const DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE: u32 = 256;
/// A caller may not turn one internal artifact page into an unbounded read.
pub const MAX_REBASELINE_SNAPSHOT_PAGE_SIZE: u32 = 1_000;
/// Gen-1 logical validity window. Physical cleanup is intentionally deferred.
pub const DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS: u64 = 86_400;
/// Maximum number of active durable artifacts admitted for one owner/library
/// namespace. Expired rows remain durable history and do not count.
pub const MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY: u32 = 8;

/// Stable failures for logical and durable rebaseline snapshot operations.
///
/// Owner and library scope failures intentionally collapse to `NotFound`, so a
/// request cannot become an ownership, library-existence, or snapshot-ID
/// oracle. Lifecycle status is evaluated only after that owner-scoped lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    NotFound,
    InvalidPageSize,
    InvalidPageCursor,
    Expired,
    ActiveArtifactLimitReached,
    InvalidObservedTime,
    DependencyUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
    InvalidSnapshot(LogicalSnapshotError),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "logical snapshot resource was not found",
            Self::InvalidPageSize => "logical snapshot page size is invalid",
            Self::InvalidPageCursor => "logical snapshot page cursor is invalid",
            Self::Expired => "logical snapshot artifact has expired",
            Self::ActiveArtifactLimitReached => {
                "logical snapshot active artifact limit has been reached"
            }
            Self::InvalidObservedTime => "logical snapshot observed time cannot represent expiry",
            Self::DependencyUnavailable => "logical snapshot dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "logical snapshot persisted data is invalid",
            Self::InvalidSnapshot(error) => return error.fmt(formatter),
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SnapshotError {}

/// One authoritative logical namespace and the exact journal handoff point
/// from which a client may request the next incremental feed page.
///
/// This remains the bounded in-memory Prompt 81 value. Durable transfer uses
/// [`RebaselineSnapshotDescriptor`] and [`RebaselineSnapshotPage`] below.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebaselineSnapshot {
    state: LogicalSnapshot,
    boundary: JournalHighWatermark,
}

impl RebaselineSnapshot {
    #[must_use]
    pub fn new(state: LogicalSnapshot, boundary: JournalHighWatermark) -> Self {
        Self { state, boundary }
    }

    #[must_use]
    pub const fn state(&self) -> &LogicalSnapshot {
        &self.state
    }

    #[must_use]
    pub const fn logical_state(&self) -> &LogicalSnapshot {
        &self.state
    }

    #[must_use]
    pub fn into_state(self) -> LogicalSnapshot {
        self.state
    }

    #[must_use]
    pub const fn boundary(&self) -> JournalHighWatermark {
        self.boundary
    }

    #[must_use]
    pub const fn cursor(&self) -> crate::JournalCursor {
        self.boundary.cursor()
    }
}

/// Metadata describing one durable immutable logical snapshot artifact.
///
/// The owner is deliberately an authorization input rather than descriptor
/// content. The descriptor is library-scoped, carries the full typed journal
/// continuation boundary, and contains no device/checkpoint state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaselineSnapshotDescriptor {
    snapshot_id: RebaselineSnapshotId,
    library_id: LibraryId,
    boundary: JournalHighWatermark,
    entry_count: u64,
    created_at: Timestamp,
    expires_at: Timestamp,
}

impl RebaselineSnapshotDescriptor {
    #[must_use]
    pub const fn new(
        snapshot_id: RebaselineSnapshotId,
        library_id: LibraryId,
        boundary: JournalHighWatermark,
        entry_count: u64,
        created_at: Timestamp,
        expires_at: Timestamp,
    ) -> Self {
        Self {
            snapshot_id,
            library_id,
            boundary,
            entry_count,
            created_at,
            expires_at,
        }
    }

    #[must_use]
    pub const fn snapshot_id(self) -> RebaselineSnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn boundary(self) -> JournalHighWatermark {
        self.boundary
    }

    #[must_use]
    pub const fn entry_count(self) -> u64 {
        self.entry_count
    }

    #[must_use]
    pub const fn created_at(self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn expires_at(self) -> Timestamp {
        self.expires_at
    }

    /// Valid strictly before expiry; equality is expired by contract.
    #[must_use]
    pub fn is_expired_at(self, observed_at: Timestamp) -> bool {
        observed_at >= self.expires_at
    }
}

/// One bounded, stable keyset page from an immutable rebaseline artifact.
///
/// Every page repeats its descriptor, including artifact identity and journal
/// boundary, so a caller cannot accidentally combine entries from two cuts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebaselineSnapshotPage {
    descriptor: RebaselineSnapshotDescriptor,
    entries: Vec<LogicalSnapshotNode>,
    next_cursor: Option<RebaselineSnapshotPageCursor>,
}

impl RebaselineSnapshotPage {
    #[must_use]
    pub fn new(
        descriptor: RebaselineSnapshotDescriptor,
        entries: Vec<LogicalSnapshotNode>,
        next_cursor: Option<RebaselineSnapshotPageCursor>,
    ) -> Self {
        Self {
            descriptor,
            entries,
            next_cursor,
        }
    }

    #[must_use]
    pub const fn descriptor(&self) -> RebaselineSnapshotDescriptor {
        self.descriptor
    }

    #[must_use]
    pub fn entries(&self) -> &[LogicalSnapshotNode] {
        &self.entries
    }

    #[must_use]
    pub fn into_entries(self) -> Vec<LogicalSnapshotNode> {
        self.entries
    }

    #[must_use]
    pub const fn next_cursor(&self) -> Option<RebaselineSnapshotPageCursor> {
        self.next_cursor
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }
}

#[derive(Clone, Debug, FromRow)]
struct SnapshotHeadRow {
    journal_epoch: i64,
    sync_head: i64,
}

#[derive(Clone, Debug, FromRow)]
struct SnapshotNodeRow {
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
struct DurableSnapshotRow {
    id: Uuid,
    owner_user_id: Uuid,
    library_id: Uuid,
    journal_epoch: i64,
    snapshot_resume_sequence: i64,
    entry_count: i64,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
}

/// PostgreSQL implementation of the Prompt 81 logical snapshot foundation and
/// Prompt 82's durable paging seam.
#[derive(Clone)]
pub struct LogicalSnapshotService {
    pool: DatabasePool,
}

impl LogicalSnapshotService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Build one complete current logical snapshot for an owned library.
    ///
    /// `REPEATABLE READ` establishes one MVCC view for both the library head
    /// and all node/content metadata. The namespace guard is acquired before
    /// that view is first read, matching the guard order used by every
    /// cooperative namespace mutation. Thus a concurrent mutation is either
    /// visible in both the state and boundary or visible in neither; its
    /// committed journal event is then available strictly after the returned
    /// boundary. This retains Prompt 81's bounded in-memory foundation for
    /// callers that need the complete aggregate directly.
    pub async fn build(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
    ) -> Result<RebaselineSnapshot, SnapshotError> {
        let mut transaction = self
            .begin_snapshot_transaction(SnapshotTransactionMode::RepeatableRead)
            .await?;
        acquire_namespace_guard(&mut transaction, library_id)
            .await
            .map_err(map_metadata_error)?;

        let head = load_snapshot_head(&mut transaction, owner_user_id, library_id).await?;
        let boundary = JournalHighWatermark::new(
            library_id,
            positive_sequence(head.journal_epoch)?,
            nonnegative_sequence(head.sync_head)?,
        );
        let state = load_logical_snapshot(&mut transaction, owner_user_id, library_id).await?;

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;

        Ok(RebaselineSnapshot::new(state, boundary))
    }

    /// Descriptive alias for callers that want to make the Prompt 81 read
    /// operation explicit at the application/metadata seam.
    pub async fn build_snapshot(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
    ) -> Result<RebaselineSnapshot, SnapshotError> {
        self.build(owner_user_id, library_id).await
    }

    /// Materialize one validated logical cut into an immutable durable artifact.
    ///
    /// The transaction holds the existing library namespace guard only while it
    /// reads/validates the Prompt 81 aggregate, persists the header and copies
    /// the canonical projection with one `INSERT ... SELECT`. A transaction
    /// failure drops the header, boundary proof, and entries together. The
    /// later page-read methods intentionally take no namespace guard and never
    /// consult `nodes`.
    pub async fn create_rebaseline_snapshot(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        observed_at: Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
        let expires_at = observed_at
            .checked_add_std(Duration::from_secs(
                DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS,
            ))
            .ok_or(SnapshotError::InvalidObservedTime)?;
        let proof_expires_at = expires_at
            .checked_add_std(Duration::from_secs(DEFAULT_HANDOFF_PROOF_RETENTION_SECONDS))
            .ok_or(SnapshotError::InvalidObservedTime)?;
        let snapshot_id = RebaselineSnapshotId::new();
        // Keep creation at READ COMMITTED. The advisory-lock statement can
        // establish a REPEATABLE READ snapshot before it waits for the
        // existing namespace guard, which would let queued creators count a
        // stale set of committed artifacts. READ COMMITTED takes the head and
        // admission snapshots after the guard; the guard then prevents every
        // cooperative namespace mutation and creator from interleaving with
        // the materialization.
        let mut transaction = self
            .begin_snapshot_transaction(SnapshotTransactionMode::ReadCommitted)
            .await?;
        acquire_namespace_guard(&mut transaction, library_id)
            .await
            .map_err(map_metadata_error)?;

        let head = load_snapshot_head(&mut transaction, owner_user_id, library_id).await?;
        ensure_active_snapshot_capacity(&mut transaction, owner_user_id, library_id, observed_at)
            .await?;
        let boundary = JournalHighWatermark::new(
            library_id,
            positive_sequence(head.journal_epoch)?,
            nonnegative_sequence(head.sync_head)?,
        );

        // Prompt 81 aggregate validation remains authoritative. This is the
        // only O(total-node-count) Rust allocation in creation; copied rows are
        // still inserted set-wise, and every later page is O(page_size).
        let state = load_logical_snapshot(&mut transaction, owner_user_id, library_id).await?;
        let entry_count =
            u64::try_from(state.len()).map_err(|_| SnapshotError::InvalidPersistedData)?;
        let persisted_entry_count =
            i64::try_from(entry_count).map_err(|_| SnapshotError::InvalidPersistedData)?;

        sqlx::query(
            "INSERT INTO rebaseline_snapshots
                (id, owner_user_id, library_id, journal_epoch,
                 snapshot_resume_sequence, entry_count, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(snapshot_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(head.journal_epoch)
        .bind(head.sync_head)
        .bind(persisted_entry_count)
        .bind(observed_at.as_offset_datetime())
        .bind(expires_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        // The small proof is the single handoff authority before and after
        // payload cleanup. It is committed in this same creation transaction,
        // so neither payload nor proof can become durable alone.
        sqlx::query(
            "INSERT INTO rebaseline_snapshot_handoff_proofs
                (snapshot_id, owner_user_id, library_id, journal_epoch,
                 snapshot_resume_sequence, snapshot_created_at,
                 snapshot_expires_at, proof_expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(snapshot_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(head.journal_epoch)
        .bind(head.sync_head)
        .bind(observed_at.as_offset_datetime())
        .bind(expires_at.as_offset_datetime())
        .bind(proof_expires_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        // This deliberately reuses the exact Prompt 81 projection but leaves
        // row copying inside PostgreSQL rather than issuing one insert per
        // Node. The namespace guard prevents any cooperative projection or
        // journal mutation between the validated head/state reads and copy.
        let inserted = sqlx::query(
            "INSERT INTO rebaseline_snapshot_entries
                (snapshot_id, node_id, parent_node_id, name, kind, state,
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
        .bind(snapshot_id.into_uuid())
        .bind(library_id.into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        if inserted.rows_affected() != entry_count {
            return Err(SnapshotError::InvalidPersistedData);
        }

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;

        Ok(RebaselineSnapshotDescriptor::new(
            snapshot_id,
            library_id,
            boundary,
            entry_count,
            observed_at,
            expires_at,
        ))
    }

    /// Read an owner-scoped durable artifact descriptor without touching the
    /// live namespace or a device checkpoint.
    pub async fn get_rebaseline_snapshot(
        &self,
        owner_user_id: UserId,
        snapshot_id: RebaselineSnapshotId,
        observed_at: Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
        let mut transaction = self
            .begin_snapshot_transaction(SnapshotTransactionMode::ReadCommitted)
            .await?;
        let descriptor =
            load_rebaseline_snapshot_descriptor(&mut transaction, owner_user_id, snapshot_id)
                .await?;
        let descriptor = match descriptor {
            Some(descriptor) => descriptor,
            None => {
                return Err(
                    missing_payload_error(&mut transaction, owner_user_id, snapshot_id).await?,
                );
            }
        };
        ensure_snapshot_is_live(descriptor, observed_at)?;
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(descriptor)
    }

    /// Read one immutable Node-ID keyset page from a durable artifact.
    ///
    /// The owner-scoped header and indexed entry reads share one short
    /// repeatable-read snapshot. Concurrent payload cleanup therefore yields a
    /// complete immutable page or canonical expired/not-found semantics, never
    /// a successful torn page. No transaction survives this metadata call.
    pub async fn read_rebaseline_snapshot_page(
        &self,
        owner_user_id: UserId,
        snapshot_id: RebaselineSnapshotId,
        page_cursor: Option<RebaselineSnapshotPageCursor>,
        page_size: u32,
        observed_at: Timestamp,
    ) -> Result<RebaselineSnapshotPage, SnapshotError> {
        let page_size = validate_rebaseline_snapshot_page_size(page_size)?;
        let mut transaction = self
            .begin_snapshot_transaction(SnapshotTransactionMode::RepeatableRead)
            .await?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        let descriptor =
            load_rebaseline_snapshot_descriptor(&mut transaction, owner_user_id, snapshot_id)
                .await?;
        let descriptor = match descriptor {
            Some(descriptor) => descriptor,
            None => {
                return Err(
                    missing_payload_error(&mut transaction, owner_user_id, snapshot_id).await?,
                );
            }
        };
        ensure_snapshot_is_live(descriptor, observed_at)?;

        let after_node_id = match page_cursor {
            Some(cursor) => {
                if cursor.snapshot_id() != snapshot_id {
                    return Err(SnapshotError::InvalidPageCursor);
                }
                Some(cursor.after_node_id())
            }
            None => None,
        };
        let rows = sqlx::query_as::<_, SnapshotNodeRow>(
            "SELECT node_id, parent_node_id, name, kind, state,
                    revision::TEXT AS revision, current_version_id,
                    content_length::TEXT AS content_length, content_sha256
             FROM rebaseline_snapshot_entries
             WHERE snapshot_id = $1
               AND ($2::UUID IS NULL OR node_id > $2)
             ORDER BY node_id ASC
             LIMIT $3",
        )
        .bind(snapshot_id.into_uuid())
        .bind(after_node_id.map(NodeId::into_uuid))
        .bind(i64::from(page_size) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        let has_more = rows.len() > page_size as usize;
        let entries = rows
            .into_iter()
            .take(page_size as usize)
            .map(map_snapshot_node)
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if has_more {
            let after_node_id = entries
                .last()
                .map(LogicalSnapshotNode::node_id)
                .ok_or(SnapshotError::InvalidPersistedData)?;
            Some(RebaselineSnapshotPageCursor::new(
                snapshot_id,
                after_node_id,
            ))
        } else {
            None
        };

        let page = RebaselineSnapshotPage::new(descriptor, entries, next_cursor);
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(page)
    }

    async fn begin_snapshot_transaction(
        &self,
        mode: SnapshotTransactionMode,
    ) -> Result<Transaction<'_, Postgres>, SnapshotError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        if matches!(mode, SnapshotTransactionMode::RepeatableRead) {
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .execute(&mut *transaction)
                .await
                .map_err(MetadataError::from)
                .map_err(map_metadata_error)?;
        }
        Ok(transaction)
    }
}

async fn load_rebaseline_snapshot_descriptor(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    snapshot_id: RebaselineSnapshotId,
) -> Result<Option<RebaselineSnapshotDescriptor>, SnapshotError> {
    let row = sqlx::query_as::<_, DurableSnapshotRow>(
        "SELECT id, owner_user_id, library_id, journal_epoch,
                snapshot_resume_sequence, entry_count, created_at, expires_at
         FROM rebaseline_snapshots
         WHERE id = $1
           AND owner_user_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?;
    row.map(|row| map_durable_snapshot_descriptor(row, owner_user_id, snapshot_id))
        .transpose()
}

async fn missing_payload_error(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    snapshot_id: RebaselineSnapshotId,
) -> Result<SnapshotError, SnapshotError> {
    let proof_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1
            FROM rebaseline_snapshot_handoff_proofs
            WHERE snapshot_id = $1
              AND owner_user_id = $2
         )",
    )
    .bind(snapshot_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?;
    Ok(if proof_exists {
        SnapshotError::Expired
    } else {
        SnapshotError::NotFound
    })
}

/// Compatibility name for code that describes this boundary as a rebaseline
/// service rather than a logical snapshot service. Its durable methods are
/// intentionally transport-neutral; Prompt 83 owns public route serialization.
pub type RebaselineSnapshotService = LogicalSnapshotService;

#[derive(Clone, Copy)]
enum SnapshotTransactionMode {
    RepeatableRead,
    ReadCommitted,
}

async fn load_snapshot_head(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
) -> Result<SnapshotHeadRow, SnapshotError> {
    sqlx::query_as::<_, SnapshotHeadRow>(
        "SELECT journal_epoch, sync_head
         FROM libraries
         WHERE id = $1
           AND owner_user_id = $2
           AND status <> 'DELETING'
         FOR SHARE",
    )
    .bind(library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?
    .ok_or(SnapshotError::NotFound)
}

/// Enforce the durable creation bound while the existing per-library
/// namespace guard is held. The observed time is the same clock value used to
/// calculate the new artifact expiry, so equality is expired and immediately
/// available for re-admission. Keeping this query in the materialization
/// transaction makes concurrent callers race on the canonical library guard
/// rather than on an in-process or global lock.
async fn ensure_active_snapshot_capacity(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
    observed_at: Timestamp,
) -> Result<(), SnapshotError> {
    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM rebaseline_snapshots
         WHERE owner_user_id = $1
           AND library_id = $2
           AND expires_at > $3",
    )
    .bind(owner_user_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(observed_at.as_offset_datetime())
    .fetch_one(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?;

    if active_count >= i64::from(MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY) {
        return Err(SnapshotError::ActiveArtifactLimitReached);
    }
    Ok(())
}

async fn load_logical_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
) -> Result<LogicalSnapshot, SnapshotError> {
    let rows = sqlx::query_as::<_, SnapshotNodeRow>(
        "SELECT n.id AS node_id,
                n.parent_node_id,
                n.name,
                n.kind,
                n.state,
                n.revision::TEXT AS revision,
                n.current_version_id,
                o.plaintext_length::TEXT AS content_length,
                o.canonical_hash AS content_sha256
         FROM nodes AS n
         INNER JOIN libraries AS l
           ON l.id = n.library_id
          AND l.owner_user_id = $2
         LEFT JOIN file_versions AS v
           ON v.id = n.current_version_id
          AND v.library_id = n.library_id
         LEFT JOIN objects AS o
           ON o.id = v.object_id
          AND o.dedup_domain_id = v.object_dedup_domain_id
         WHERE n.library_id = $1
           AND n.state IN ('ACTIVE', 'TRASHED')
         ORDER BY n.id ASC",
    )
    .bind(library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?;

    let entries = rows
        .into_iter()
        .map(map_snapshot_node)
        .collect::<Result<Vec<_>, _>>()?;
    LogicalSnapshot::new(library_id, entries).map_err(SnapshotError::InvalidSnapshot)
}

fn map_snapshot_node(row: SnapshotNodeRow) -> Result<LogicalSnapshotNode, SnapshotError> {
    let node_id =
        NodeId::try_from_uuid(row.node_id).map_err(|_| SnapshotError::InvalidPersistedData)?;
    let parent_node_id = row
        .parent_node_id
        .map(NodeId::try_from_uuid)
        .transpose()
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    let name = LogicalName::new(row.name).map_err(|_| SnapshotError::InvalidPersistedData)?;
    let kind = match row.kind.as_str() {
        "FILE" => NodeKind::File,
        "DIRECTORY" => NodeKind::Directory,
        _ => return Err(SnapshotError::InvalidPersistedData),
    };
    let state = match row.state.as_str() {
        "ACTIVE" => NodeState::Active,
        "TRASHED" => NodeState::Trashed,
        _ => return Err(SnapshotError::InvalidPersistedData),
    };
    let revision = row
        .revision
        .parse::<u64>()
        .map(Revision::new)
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    let current_version_id = row
        .current_version_id
        .map(FileVersionId::try_from_uuid)
        .transpose()
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    let content_length = row
        .content_length
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    let content_sha256 = row
        .content_sha256
        .as_deref()
        .map(Sha256Digest::try_from)
        .transpose()
        .map_err(|_| SnapshotError::InvalidPersistedData)?;

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
    .map_err(|_| SnapshotError::InvalidPersistedData)
}

fn map_durable_snapshot_descriptor(
    row: DurableSnapshotRow,
    expected_owner_user_id: UserId,
    expected_snapshot_id: RebaselineSnapshotId,
) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
    let snapshot_id = RebaselineSnapshotId::try_from_uuid(row.id)
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    let owner_user_id = UserId::try_from_uuid(row.owner_user_id)
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    let library_id = LibraryId::try_from_uuid(row.library_id)
        .map_err(|_| SnapshotError::InvalidPersistedData)?;
    if snapshot_id != expected_snapshot_id || owner_user_id != expected_owner_user_id {
        return Err(SnapshotError::InvalidPersistedData);
    }
    let entry_count =
        u64::try_from(row.entry_count).map_err(|_| SnapshotError::InvalidPersistedData)?;
    let created_at = Timestamp::from_offset_datetime(row.created_at);
    let expires_at = Timestamp::from_offset_datetime(row.expires_at);
    if expires_at <= created_at {
        return Err(SnapshotError::InvalidPersistedData);
    }
    let boundary = JournalHighWatermark::new(
        library_id,
        positive_sequence(row.journal_epoch)?,
        nonnegative_sequence(row.snapshot_resume_sequence)?,
    );
    Ok(RebaselineSnapshotDescriptor::new(
        snapshot_id,
        library_id,
        boundary,
        entry_count,
        created_at,
        expires_at,
    ))
}

fn ensure_snapshot_is_live(
    descriptor: RebaselineSnapshotDescriptor,
    observed_at: Timestamp,
) -> Result<(), SnapshotError> {
    if descriptor.is_expired_at(observed_at) {
        Err(SnapshotError::Expired)
    } else {
        Ok(())
    }
}

fn validate_rebaseline_snapshot_page_size(page_size: u32) -> Result<u32, SnapshotError> {
    if (1..=MAX_REBASELINE_SNAPSHOT_PAGE_SIZE).contains(&page_size) {
        Ok(page_size)
    } else {
        Err(SnapshotError::InvalidPageSize)
    }
}

fn positive_sequence(value: i64) -> Result<Sequence, SnapshotError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Sequence::new)
        .ok_or(SnapshotError::InvalidPersistedData)
}

fn nonnegative_sequence(value: i64) -> Result<Sequence, SnapshotError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| SnapshotError::InvalidPersistedData)
}

fn map_metadata_error(error: MetadataError) -> SnapshotError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => SnapshotError::DependencyUnavailable,
        MetadataError::Database(error) => SnapshotError::Database(error),
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
        | MetadataError::Mapping(MappingError::Domain(_)) => SnapshotError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => SnapshotError::DependencyUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::{
        DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE, MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY,
        MAX_REBASELINE_SNAPSHOT_PAGE_SIZE, RebaselineSnapshotDescriptor, SnapshotError,
        nonnegative_sequence, positive_sequence, validate_rebaseline_snapshot_page_size,
    };
    use crate::journal::{JournalCursor, JournalHighWatermark};
    use synveil_core::{
        LibraryId, NodeId, RebaselineSnapshotId, RebaselineSnapshotPageCursor, Sequence, Timestamp,
    };

    #[test]
    fn sequence_mapping_rejects_invalid_database_values() {
        assert_eq!(
            positive_sequence(0),
            Err(SnapshotError::InvalidPersistedData)
        );
        assert_eq!(
            positive_sequence(-1),
            Err(SnapshotError::InvalidPersistedData)
        );
        assert_eq!(positive_sequence(3), Ok(Sequence::new(3)));
        assert_eq!(
            nonnegative_sequence(-1),
            Err(SnapshotError::InvalidPersistedData)
        );
        assert_eq!(nonnegative_sequence(0), Ok(Sequence::new(0)));
    }

    #[test]
    fn durable_descriptor_preserves_identity_boundary_and_expiry_edge() {
        let library_id = LibraryId::new();
        let snapshot_id = RebaselineSnapshotId::new();
        let created_at = Timestamp::parse("2026-09-08T00:00:00Z").unwrap();
        let expires_at = Timestamp::parse("2026-09-09T00:00:00Z").unwrap();
        let boundary = JournalHighWatermark::new(library_id, Sequence::new(2), Sequence::new(7));
        let descriptor = RebaselineSnapshotDescriptor::new(
            snapshot_id,
            library_id,
            boundary,
            3,
            created_at,
            expires_at,
        );

        assert_eq!(descriptor.snapshot_id(), snapshot_id);
        assert_eq!(descriptor.library_id(), library_id);
        assert_eq!(descriptor.boundary(), boundary);
        assert_eq!(descriptor.entry_count(), 3);
        assert!(!descriptor.is_expired_at(created_at));
        assert!(descriptor.is_expired_at(expires_at));
    }

    #[test]
    fn durable_page_cursor_is_not_a_journal_cursor_and_is_snapshot_scoped() {
        let snapshot_id = RebaselineSnapshotId::new();
        let node_id = NodeId::new();
        let page_cursor = RebaselineSnapshotPageCursor::new(snapshot_id, node_id);

        assert_ne!(
            TypeId::of::<RebaselineSnapshotPageCursor>(),
            TypeId::of::<JournalCursor>()
        );
        assert_eq!(page_cursor.snapshot_id(), snapshot_id);
        assert_eq!(page_cursor.after_node_id(), node_id);
    }

    #[test]
    fn durable_page_size_is_explicitly_bounded() {
        assert_eq!(
            validate_rebaseline_snapshot_page_size(0),
            Err(SnapshotError::InvalidPageSize)
        );
        assert_eq!(
            validate_rebaseline_snapshot_page_size(DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE),
            Ok(DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE)
        );
        assert_eq!(
            validate_rebaseline_snapshot_page_size(MAX_REBASELINE_SNAPSHOT_PAGE_SIZE),
            Ok(MAX_REBASELINE_SNAPSHOT_PAGE_SIZE)
        );
        assert_eq!(
            validate_rebaseline_snapshot_page_size(MAX_REBASELINE_SNAPSHOT_PAGE_SIZE + 1),
            Err(SnapshotError::InvalidPageSize)
        );
    }

    #[test]
    fn active_artifact_bound_is_explicit_and_typed() {
        assert_eq!(MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY, 8);
        assert_eq!(
            SnapshotError::ActiveArtifactLimitReached.to_string(),
            "logical snapshot active artifact limit has been reached"
        );
    }
}
