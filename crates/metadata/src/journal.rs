//! Durable logical change-journal writer and transport-neutral reader.
//!
//! PostgreSQL is the authority for both the per-library clock and the journal
//! rows. Mutation repositories call [`append_changes`] with their active
//! transaction after the domain state is ready; the helper never opens a
//! second transaction. The reader uses a bounded, repeatable-read keyset page
//! so a caller can safely resume with `sequence > cursor`.

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    ChangeEvent, ChangeEventId, ChangeKind, ChangeResourceKind, ChangeResourceKindParseError,
    FileVersionId, LibraryId, Node, NodeId, NodeKind, NodeState, Revision, Sequence, Timestamp,
    UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{DatabaseError, DatabaseErrorKind, DatabasePool, MappingError, MetadataError};

pub const DEFAULT_JOURNAL_PAGE_LIMIT: u32 = 100;
pub const MAX_JOURNAL_PAGE_LIMIT: u32 = 500;
pub const MAX_JOURNAL_CURSOR_BYTES: usize = 256;

const CURSOR_VERSION: &str = "v1";
const CURSOR_KIND: &str = "journal";
const CURSOR_PARTS: usize = 6;
const CURSOR_ID_HEX_BYTES: usize = 32;
const CURSOR_NUMBER_HEX_BYTES: usize = 16;
const CURSOR_DIGEST_HEX_BYTES: usize = 64;
const JOURNAL_SCHEMA_VERSION: u16 = 1;
const MAX_EVENTS_PER_APPEND: usize = 64;
const NAMESPACE_GUARD_DOMAIN: &str = "synveil:library-namespace:v1";

/// A compact resulting Node projection supplied by a mutation repository.
///
/// It is intentionally separate from the public event so repositories cannot
/// accidentally bind storage-internal values to the journal contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JournalChange {
    library_id: LibraryId,
    change_kind: ChangeKind,
    resource_id: NodeId,
    resource_revision: Revision,
    parent_node_id: Option<NodeId>,
    node_kind: Option<NodeKind>,
    node_state: Option<NodeState>,
    current_version_id: Option<FileVersionId>,
}

impl JournalChange {
    pub(crate) fn from_node(change_kind: ChangeKind, node: &Node) -> Self {
        Self {
            library_id: node.library_id(),
            change_kind,
            resource_id: node.id(),
            resource_revision: node.revision(),
            parent_node_id: node.parent_node_id(),
            node_kind: Some(node.kind()),
            node_state: Some(node.state()),
            current_version_id: node.current_version_id(),
        }
    }

    /// A purge is the one event whose subject no longer has a live Node row.
    /// Keep only the identity, kind, and last logical revision required for a
    /// consumer tombstone; do not retain its name, path, or content identity.
    pub(crate) fn purged(node: &Node) -> Self {
        Self {
            library_id: node.library_id(),
            change_kind: ChangeKind::NodePurged,
            resource_id: node.id(),
            resource_revision: node.revision(),
            parent_node_id: None,
            node_kind: Some(node.kind()),
            node_state: None,
            current_version_id: None,
        }
    }
}

/// Serialize all cooperative logical namespace mutations for one library.
///
/// This transaction-scoped advisory lock is intentionally separate from the
/// `libraries` row used as the journal clock. Callers acquire it before any
/// `Node` row lock; [`append_changes`] then takes the library-row clock lock
/// only at the end of the domain mutation. The guard is never held by byte
/// transport because upload callers invoke it only during finalization.
pub(crate) async fn acquire_namespace_guard(
    transaction: &mut Transaction<'_, Postgres>,
    library_id: LibraryId,
) -> Result<(), MetadataError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended($1::TEXT || ':' || $2::UUID::TEXT, 0)
         )",
    )
    .bind(NAMESPACE_GUARD_DOMAIN)
    .bind(library_id.into_uuid())
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

#[derive(Debug, FromRow)]
struct JournalHeadRow {
    journal_epoch: i64,
    sync_head: i64,
    minimum_retained_sequence: i64,
}

/// Append one or more logical facts to the caller's active PostgreSQL
/// transaction. The per-library `libraries` row is locked and advanced here,
/// immediately before the inserts. No event is visible unless the caller's
/// domain mutation and transaction commit also succeed.
pub(crate) async fn append_changes(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
    changes: &[JournalChange],
) -> Result<Vec<ChangeEvent>, MetadataError> {
    if changes.is_empty() {
        return Ok(Vec::new());
    }
    if changes.len() > MAX_EVENTS_PER_APPEND {
        return Err(MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "change_journal.event_count",
        }));
    }

    if changes.iter().any(|change| change.library_id != library_id) {
        return Err(MetadataError::Mapping(MappingError::RelationMismatch {
            relation: "change_journal.resource_library",
        }));
    }

    let head = sqlx::query_as::<_, JournalHeadRow>(
        "SELECT journal_epoch, sync_head, minimum_retained_sequence
         FROM libraries
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?
    .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
        relation: "change_journal.library_scope",
    }))?;

    if head.journal_epoch <= 0
        || head.sync_head < 0
        || head.minimum_retained_sequence < 0
        || head.minimum_retained_sequence > head.sync_head
    {
        return Err(MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "libraries.journal_clock",
        }));
    }

    let event_count = i64::try_from(changes.len()).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "change_journal.event_count",
        })
    })?;
    let last_sequence = head
        .sync_head
        .checked_add(event_count)
        .ok_or(MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "libraries.sync_head",
        }))?;

    // Keep BIGINT allocation within the signed PostgreSQL range. The domain
    // `Sequence` is wider for other manifests, but this journal's durable
    // clock is explicitly PostgreSQL BIGINT as required by ADR-006.
    if last_sequence <= 0 {
        return Err(MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "libraries.sync_head",
        }));
    }

    let updated = sqlx::query(
        "UPDATE libraries
         SET sync_head = $3
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(last_sequence)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;
    if updated.rows_affected() != 1 {
        return Err(MetadataError::Mapping(MappingError::RelationMismatch {
            relation: "libraries.sync_head_update",
        }));
    }

    // `CURRENT_TIMESTAMP` is PostgreSQL's transaction-consistent canonical
    // time. It is descriptive only; sequence remains the ordering authority.
    let occurred_at = sqlx::query_scalar::<_, OffsetDateTime>("SELECT CURRENT_TIMESTAMP")
        .fetch_one(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;

    let journal_epoch = Sequence::new(u64::try_from(head.journal_epoch).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "change_journal.journal_epoch",
        })
    })?);
    let occurred_at = Timestamp::from_offset_datetime(occurred_at);
    let mut events = Vec::with_capacity(changes.len());

    for (offset, change) in changes.iter().enumerate() {
        let offset = i64::try_from(offset).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "change_journal.sequence",
            })
        })?;
        let sequence = head
            .sync_head
            .checked_add(offset + 1)
            .ok_or(MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "change_journal.sequence",
            }))?;
        let sequence = Sequence::new(u64::try_from(sequence).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "change_journal.sequence",
            })
        })?);
        let entry_id = ChangeEventId::new();

        sqlx::query(
            "INSERT INTO change_journal
                (entry_id, owner_user_id, library_id, journal_epoch, sequence,
                 schema_version, resource_kind, resource_id, change_kind,
                 occurred_at, resource_revision, parent_node_id, node_kind,
                 node_state, current_version_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::NUMERIC,
                     $12, $13, $14, $15)",
        )
        .bind(entry_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(head.journal_epoch)
        .bind(i64::try_from(sequence.get()).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "change_journal.sequence",
            })
        })?)
        .bind(i16::try_from(JOURNAL_SCHEMA_VERSION).expect("journal schema version fits SMALLINT"))
        .bind(ChangeResourceKind::Node.as_str())
        .bind(change.resource_id.into_uuid())
        .bind(change.change_kind.as_str())
        .bind(occurred_at.as_offset_datetime())
        .bind(change.resource_revision.get().to_string())
        .bind(change.parent_node_id.map(NodeId::into_uuid))
        .bind(change.node_kind.map(NodeKind::as_str))
        .bind(change.node_state.map(NodeState::as_str))
        .bind(change.current_version_id.map(FileVersionId::into_uuid))
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;

        events.push(ChangeEvent::new(
            entry_id,
            owner_user_id,
            library_id,
            journal_epoch,
            sequence,
            JOURNAL_SCHEMA_VERSION,
            ChangeResourceKind::Node,
            change.resource_id,
            change.change_kind,
            occurred_at,
            change.resource_revision,
            change.parent_node_id,
            change.node_kind,
            change.node_state,
            change.current_version_id,
        ));
    }

    Ok(events)
}

/// An opaque journal position bound to one library and epoch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JournalCursor {
    library_id: LibraryId,
    journal_epoch: Sequence,
    sequence: Sequence,
}

impl JournalCursor {
    #[must_use]
    pub const fn new(library_id: LibraryId, journal_epoch: Sequence, sequence: Sequence) -> Self {
        Self {
            library_id,
            journal_epoch,
            sequence,
        }
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn sequence(self) -> Sequence {
        self.sequence
    }

    /// Encode claims without exposing their canonical textual IDs. The digest
    /// catches tampering/corruption, while the service still validates owner,
    /// library, epoch, and committed-head scope against PostgreSQL.
    #[must_use]
    pub fn encode(self) -> String {
        let library = self.library_id.as_bytes();
        let epoch = self.journal_epoch.get().to_be_bytes();
        let sequence = self.sequence.get().to_be_bytes();
        let digest = cursor_digest(library, epoch, sequence);
        format!(
            "{CURSOR_VERSION}.{CURSOR_KIND}.{}.{}.{}.{}",
            encode_hex(library),
            encode_hex(&epoch),
            encode_hex(&sequence),
            encode_hex(&digest)
        )
    }

    pub fn decode(value: &str) -> Result<Self, JournalCursorError> {
        if value.len() > MAX_JOURNAL_CURSOR_BYTES {
            return Err(JournalCursorError::TooLong);
        }
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() != CURSOR_PARTS || parts[0] != CURSOR_VERSION || parts[1] != CURSOR_KIND {
            return Err(JournalCursorError::Malformed);
        }

        let library_bytes = decode_hex(parts[2], CURSOR_ID_HEX_BYTES)?;
        let library_bytes: [u8; 16] = library_bytes
            .try_into()
            .map_err(|_| JournalCursorError::Malformed)?;
        let library_id = LibraryId::try_from_uuid(Uuid::from_bytes(library_bytes))
            .map_err(|_| JournalCursorError::Malformed)?;

        let epoch_bytes = decode_hex(parts[3], CURSOR_NUMBER_HEX_BYTES)?;
        let epoch_bytes: [u8; 8] = epoch_bytes
            .try_into()
            .map_err(|_| JournalCursorError::Malformed)?;
        let sequence_bytes = decode_hex(parts[4], CURSOR_NUMBER_HEX_BYTES)?;
        let sequence_bytes: [u8; 8] = sequence_bytes
            .try_into()
            .map_err(|_| JournalCursorError::Malformed)?;
        let digest = decode_hex(parts[5], CURSOR_DIGEST_HEX_BYTES)?;
        let expected_digest = cursor_digest(&library_bytes, epoch_bytes, sequence_bytes);
        if digest != expected_digest {
            return Err(JournalCursorError::Integrity);
        }

        let journal_epoch = u64::from_be_bytes(epoch_bytes);
        let sequence = u64::from_be_bytes(sequence_bytes);
        if journal_epoch == 0 || journal_epoch > i64::MAX as u64 || sequence > i64::MAX as u64 {
            return Err(JournalCursorError::Overflow);
        }

        Ok(Self::new(
            library_id,
            Sequence::new(journal_epoch),
            Sequence::new(sequence),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalCursorError {
    TooLong,
    Malformed,
    Integrity,
    Overflow,
}

impl fmt::Display for JournalCursorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TooLong => "journal cursor is too long",
            Self::Malformed => "journal cursor is malformed",
            Self::Integrity => "journal cursor integrity check failed",
            Self::Overflow => "journal cursor number is out of range",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for JournalCursorError {}

/// Safe failures for the transport-neutral journal read application service.
/// Missing or inaccessible libraries intentionally share `NotFound`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalError {
    NotFound,
    InvalidCursor,
    CursorEpochMismatch,
    CursorExpired,
    InvalidLimit,
    DependencyUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
    InternalError,
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "change journal resource was not found",
            Self::InvalidCursor => "change journal cursor is invalid",
            Self::CursorEpochMismatch => "change journal cursor epoch is invalid",
            Self::CursorExpired => "change journal cursor has expired",
            Self::InvalidLimit => "change journal limit is invalid",
            Self::DependencyUnavailable => "change journal dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "change journal persisted data is invalid",
            Self::InternalError => "change journal internal error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for JournalError {}

/// Durable high-water mark captured from one library's journal clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalHighWatermark {
    library_id: LibraryId,
    journal_epoch: Sequence,
    sequence: Sequence,
}

impl JournalHighWatermark {
    #[must_use]
    pub const fn new(library_id: LibraryId, journal_epoch: Sequence, sequence: Sequence) -> Self {
        Self {
            library_id,
            journal_epoch,
            sequence,
        }
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn sequence(self) -> Sequence {
        self.sequence
    }

    #[must_use]
    pub const fn cursor(self) -> JournalCursor {
        JournalCursor::new(self.library_id, self.journal_epoch, self.sequence)
    }
}

/// A bounded ordered page of immutable journal facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeJournalPage {
    events: Vec<ChangeEvent>,
    next_cursor: String,
    has_more: bool,
    high_watermark: JournalHighWatermark,
}

impl ChangeJournalPage {
    #[must_use]
    pub fn events(&self) -> &[ChangeEvent] {
        &self.events
    }

    #[must_use]
    pub fn into_events(self) -> Vec<ChangeEvent> {
        self.events
    }

    #[must_use]
    pub fn next_cursor(&self) -> &str {
        &self.next_cursor
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub const fn high_watermark(&self) -> JournalHighWatermark {
        self.high_watermark
    }
}

#[derive(Clone, Debug, FromRow)]
struct JournalRow {
    entry_id: Uuid,
    owner_user_id: Uuid,
    library_id: Uuid,
    journal_epoch: i64,
    sequence: i64,
    schema_version: i16,
    resource_kind: String,
    resource_id: Uuid,
    change_kind: String,
    occurred_at: OffsetDateTime,
    resource_revision: String,
    parent_node_id: Option<Uuid>,
    node_kind: Option<String>,
    node_state: Option<String>,
    current_version_id: Option<Uuid>,
}

impl JournalRow {
    fn try_into_event(self) -> Result<ChangeEvent, MetadataError> {
        let entry_id = decode_id(self.entry_id, "change_journal.entry_id")?;
        let owner_user_id = decode_id(self.owner_user_id, "change_journal.owner_user_id")?;
        let library_id = decode_id(self.library_id, "change_journal.library_id")?;
        let resource_id = decode_id(self.resource_id, "change_journal.resource_id")?;
        let parent_node_id = self
            .parent_node_id
            .map(|value| decode_id(value, "change_journal.parent_node_id"))
            .transpose()?;
        let current_version_id = self
            .current_version_id
            .map(|value| decode_id(value, "change_journal.current_version_id"))
            .transpose()?;
        let journal_epoch = decode_sequence(self.journal_epoch, "change_journal.journal_epoch")?;
        let sequence = decode_sequence(self.sequence, "change_journal.sequence")?;
        if journal_epoch.get() == 0 || sequence.get() == 0 {
            return Err(MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "change_journal.clock",
            }));
        }
        if self.schema_version != i16::try_from(JOURNAL_SCHEMA_VERSION).expect("schema fits") {
            return Err(MetadataError::Mapping(MappingError::InvalidEnum {
                field: "change_journal.schema_version",
            }));
        }
        let resource_kind = ChangeResourceKind::from_str(&self.resource_kind).map_err(
            |ChangeResourceKindParseError| {
                MetadataError::Mapping(MappingError::InvalidEnum {
                    field: "change_journal.resource_kind",
                })
            },
        )?;
        let change_kind = ChangeKind::from_str(&self.change_kind).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidEnum {
                field: "change_journal.change_kind",
            })
        })?;
        let occurred_at = Timestamp::from_offset_datetime(self.occurred_at);
        let resource_revision = Revision::from_str(&self.resource_revision).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "change_journal.resource_revision",
            })
        })?;
        let node_kind = self.node_kind.as_deref().map(parse_node_kind).transpose()?;
        let node_state = self
            .node_state
            .as_deref()
            .map(parse_node_state)
            .transpose()?;

        Ok(ChangeEvent::new(
            entry_id,
            owner_user_id,
            library_id,
            journal_epoch,
            sequence,
            JOURNAL_SCHEMA_VERSION,
            resource_kind,
            resource_id,
            change_kind,
            occurred_at,
            resource_revision,
            parent_node_id,
            node_kind,
            node_state,
            current_version_id,
        ))
    }
}

/// PostgreSQL-backed, transport-neutral journal reader.
#[derive(Clone)]
pub struct ChangeJournalService {
    pool: DatabasePool,
}

impl ChangeJournalService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// List committed changes after an optional cursor using bounded keyset
    /// pagination. The authenticated owner is always supplied separately;
    /// possessing a library ID or cursor cannot authorize a read.
    pub async fn list_changes(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        after_cursor: Option<String>,
        limit: u32,
    ) -> Result<ChangeJournalPage, JournalError> {
        let limit = validate_limit(limit)?;
        let after = after_cursor
            .as_deref()
            .map(JournalCursor::decode)
            .transpose()
            .map_err(|_| JournalError::InvalidCursor)?;

        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;

        let page = Self::list_changes_in_transaction(
            &mut transaction,
            owner_user_id,
            library_id,
            after,
            limit,
        )
        .await?;

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;

        Ok(page)
    }

    /// Read a coherent bounded journal page inside a caller-owned transaction.
    /// The caller is responsible for its isolation/locking contract and for
    /// committing or rolling back. Keeping this helper here lets device feed
    /// reads share the exact Prompt 31 SQL and high-watermark semantics without
    /// opening a competing journal reader.
    pub(crate) async fn list_changes_in_transaction(
        transaction: &mut Transaction<'_, Postgres>,
        owner_user_id: UserId,
        library_id: LibraryId,
        after: Option<JournalCursor>,
        limit: u32,
    ) -> Result<ChangeJournalPage, JournalError> {
        let limit = validate_limit(limit)?;

        let head = sqlx::query_as::<_, JournalHeadRow>(
            "SELECT journal_epoch, sync_head, minimum_retained_sequence
             FROM libraries
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(library_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(JournalError::NotFound)?;
        validate_head(&head)?;

        let journal_epoch = Sequence::new(
            u64::try_from(head.journal_epoch).map_err(|_| JournalError::InvalidPersistedData)?,
        );
        let head_sequence = Sequence::new(
            u64::try_from(head.sync_head).map_err(|_| JournalError::InvalidPersistedData)?,
        );
        let position = after.map_or(Sequence::new(0), JournalCursor::sequence);
        if let Some(cursor) = after {
            if cursor.library_id() != library_id {
                return Err(JournalError::InvalidCursor);
            }
            if cursor.journal_epoch() != journal_epoch {
                return Err(JournalError::CursorEpochMismatch);
            }
            if cursor.sequence() > head_sequence {
                return Err(JournalError::InvalidCursor);
            }
        }
        let minimum_retained = Sequence::new(
            u64::try_from(head.minimum_retained_sequence)
                .map_err(|_| JournalError::InvalidPersistedData)?,
        );
        // `minimum_retained_sequence` is the durable compacted-through
        // boundary. A cursor exactly at the boundary can request the first
        // retained event (`sequence > cursor`); every lower cursor is stale.
        // This also makes an absent cursor stale after any non-zero cleanup.
        if position < minimum_retained {
            return Err(JournalError::CursorExpired);
        }

        let rows = sqlx::query_as::<_, JournalRow>(
            "SELECT entry_id, owner_user_id, library_id, journal_epoch, sequence,
                    schema_version, resource_kind, resource_id, change_kind,
                    occurred_at, resource_revision::TEXT AS resource_revision,
                    parent_node_id, node_kind, node_state, current_version_id
             FROM change_journal
             WHERE owner_user_id = $1
               AND library_id = $2
               AND journal_epoch = $3
               AND sequence > $4
               AND sequence <= $5
             ORDER BY sequence ASC
             LIMIT $6",
        )
        .bind(owner_user_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(head.journal_epoch)
        .bind(i64::try_from(position.get()).map_err(|_| JournalError::InvalidCursor)?)
        .bind(head.sync_head)
        .bind(i64::from(limit) + 1)
        .fetch_all(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        let has_more = rows.len() > limit as usize;
        let events = rows
            .into_iter()
            .take(limit as usize)
            .map(JournalRow::try_into_event)
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_metadata_error)?;
        let last_sequence = events
            .last()
            .map_or(head_sequence, |event| event.sequence());
        let high_watermark = JournalHighWatermark::new(library_id, journal_epoch, head_sequence);
        let next_cursor = JournalCursor::new(library_id, journal_epoch, last_sequence).encode();

        Ok(ChangeJournalPage {
            events,
            next_cursor,
            has_more,
            high_watermark,
        })
    }

    /// Read the durable per-library head. This is a watermark, not a claim
    /// that every future sequence is already present in a later page.
    pub async fn get_current_high_watermark(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
    ) -> Result<JournalHighWatermark, JournalError> {
        let head = sqlx::query_as::<_, JournalHeadRow>(
            "SELECT journal_epoch, sync_head, minimum_retained_sequence
             FROM libraries
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(library_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(JournalError::NotFound)?;
        validate_head(&head)?;
        Ok(JournalHighWatermark {
            library_id,
            journal_epoch: Sequence::new(
                u64::try_from(head.journal_epoch)
                    .map_err(|_| JournalError::InvalidPersistedData)?,
            ),
            sequence: Sequence::new(
                u64::try_from(head.sync_head).map_err(|_| JournalError::InvalidPersistedData)?,
            ),
        })
    }
}

/// Compatibility name for callers that prefer to emphasize the read-service
/// boundary rather than the storage-backed implementation.
pub type JournalReadService = ChangeJournalService;

fn validate_limit(limit: u32) -> Result<u32, JournalError> {
    if (1..=MAX_JOURNAL_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(JournalError::InvalidLimit)
    }
}

fn validate_head(head: &JournalHeadRow) -> Result<(), JournalError> {
    if head.journal_epoch <= 0
        || head.sync_head < 0
        || head.minimum_retained_sequence < 0
        || head.minimum_retained_sequence > head.sync_head
    {
        Err(JournalError::InvalidPersistedData)
    } else {
        Ok(())
    }
}

fn map_metadata_error(error: MetadataError) -> JournalError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => JournalError::DependencyUnavailable,
        MetadataError::Database(error) => JournalError::Database(error),
        MetadataError::Mapping(_) | MetadataError::CapacityUnavailable => {
            JournalError::InvalidPersistedData
        }
    }
}

fn decode_id<T>(value: Uuid, field: &'static str) -> Result<T, MetadataError>
where
    T: TryFrom<Uuid, Error = synveil_core::IdParseError>,
{
    T::try_from(value)
        .map_err(|reason| MetadataError::Mapping(MappingError::InvalidId { field, reason }))
}

fn decode_sequence(value: i64, field: &'static str) -> Result<Sequence, MetadataError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| MetadataError::Mapping(MappingError::InvalidDecimal { field }))
}

fn parse_node_kind(value: &str) -> Result<NodeKind, MetadataError> {
    match value {
        "FILE" => Ok(NodeKind::File),
        "DIRECTORY" => Ok(NodeKind::Directory),
        _ => Err(MetadataError::Mapping(MappingError::InvalidEnum {
            field: "change_journal.node_kind",
        })),
    }
}

fn parse_node_state(value: &str) -> Result<NodeState, MetadataError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        "PURGING" => Ok(NodeState::Purging),
        _ => Err(MetadataError::Mapping(MappingError::InvalidEnum {
            field: "change_journal.node_state",
        })),
    }
}

fn cursor_digest(library: &[u8; 16], epoch: [u8; 8], sequence: [u8; 8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"synveil/change-journal-cursor/v1\0");
    hasher.update(library);
    hasher.update(epoch);
    hasher.update(sequence);
    let digest = hasher.finalize();
    let mut result = [0_u8; 32];
    result.copy_from_slice(&digest);
    result
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(hex_digit(byte >> 4));
        result.push(hex_digit(byte & 0x0f));
    }
    result
}

fn decode_hex(value: &str, expected_length: usize) -> Result<Vec<u8>, JournalCursorError> {
    if value.len() != expected_length || !value.len().is_multiple_of(2) {
        return Err(JournalCursorError::Malformed);
    }
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    pairs
        .iter()
        .map(|[high, low]| {
            let high = hex_nibble(*high).ok_or(JournalCursorError::Malformed)?;
            let low = hex_nibble(*low).ok_or(JournalCursorError::Malformed)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CURSOR_KIND, CURSOR_VERSION, JournalCursor, JournalCursorError, JournalError,
        MAX_JOURNAL_CURSOR_BYTES, MAX_JOURNAL_PAGE_LIMIT, decode_hex, validate_limit,
    };
    use synveil_core::{LibraryId, Sequence};

    #[test]
    fn journal_cursor_round_trips_as_an_opaque_scope_bound_token() {
        let cursor = JournalCursor::new(LibraryId::new(), Sequence::new(3), Sequence::new(41));
        let encoded = cursor.encode();

        assert!(encoded.starts_with(&format!("{CURSOR_VERSION}.{CURSOR_KIND}.")));
        assert!(!encoded.contains(&cursor.library_id().to_string()));
        assert_eq!(JournalCursor::decode(&encoded), Ok(cursor));
    }

    #[test]
    fn journal_cursor_rejects_scope_tampering_malformed_values_and_length_overflow() {
        let cursor = JournalCursor::new(LibraryId::new(), Sequence::new(1), Sequence::new(9));
        let encoded = cursor.encode();
        let other = JournalCursor::new(LibraryId::new(), Sequence::new(1), Sequence::new(9));
        assert_ne!(encoded, other.encode());
        assert_eq!(
            JournalCursor::decode("v0.journal.bad"),
            Err(JournalCursorError::Malformed)
        );
        assert_eq!(
            JournalCursor::decode(&"x".repeat(MAX_JOURNAL_CURSOR_BYTES + 1)),
            Err(JournalCursorError::TooLong)
        );
        let mut tampered = encoded.clone();
        let replacement = if tampered.ends_with('0') { "1" } else { "0" };
        tampered.replace_range(tampered.len() - 1.., replacement);
        assert_eq!(
            JournalCursor::decode(&tampered),
            Err(JournalCursorError::Integrity)
        );
    }

    #[test]
    fn journal_cursor_rejects_numbers_outside_postgresql_bigint() {
        let cursor = JournalCursor::new(
            LibraryId::new(),
            Sequence::new(1),
            Sequence::new(i64::MAX as u64 + 1),
        );
        assert_eq!(
            JournalCursor::decode(&cursor.encode()),
            Err(JournalCursorError::Overflow)
        );
    }

    #[test]
    fn journal_limits_are_bounded_and_distinct_from_other_cursors() {
        assert_eq!(validate_limit(1), Ok(1));
        assert_eq!(
            validate_limit(MAX_JOURNAL_PAGE_LIMIT),
            Ok(MAX_JOURNAL_PAGE_LIMIT)
        );
        assert_eq!(validate_limit(0), Err(JournalError::InvalidLimit));
        assert_eq!(
            validate_limit(MAX_JOURNAL_PAGE_LIMIT + 1),
            Err(JournalError::InvalidLimit)
        );
        assert_eq!(CURSOR_KIND, "journal");
    }

    #[test]
    fn cursor_hex_decoder_rejects_noncanonical_input() {
        assert_eq!(decode_hex("AA", 2), Err(JournalCursorError::Malformed));
        assert_eq!(decode_hex("0", 2), Err(JournalCursorError::Malformed));
    }
}
