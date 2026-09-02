//! Internal Trash-retention policy application boundary.
//!
//! This module lists bounded metadata-only purge candidates, performs the
//! owner-authorized, revision-checked transition into `PURGING`, and exposes
//! the separate trusted metadata execution boundary. The execution phase
//! deletes only the purged node's metadata and FileVersion references; it never
//! deletes Object/ObjectReplica rows or calls an `ObjectStore`.

use std::{fmt, str::FromStr};

use synveil_core::{
    LibraryId, NodeId, NodeKind, Revision, Timestamp, TrashRetentionPolicy, UserId,
};

use crate::{
    DatabaseError, DatabasePool, DomainRepository, MappingError, MetadataError,
    repository::{PurgeCandidateRecord, PurgeExecutionMutation, PurgeMutation},
};

pub const DEFAULT_PURGE_CANDIDATE_LIMIT: u32 = 100;
pub const MAX_PURGE_CANDIDATE_LIMIT: u32 = 500;

const CURSOR_VERSION: &str = "v1";
const CURSOR_KIND: &str = "purge";
const CURSOR_PARTS: usize = 5;
const MAX_CURSOR_BYTES: usize = 512;

/// Safe failures for internal retention-worker/application calls. A normal
/// user-facing transport may map these to its existing generic error envelope
/// without exposing SQL or storage details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurgeError {
    NotFound,
    InvalidRequest,
    InvalidCursor,
    PurgeNotEligible,
    VersionConflict { current_revision: Revision },
    InvalidState,
    Database(DatabaseError),
    InvalidPersistedData,
    InvalidPolicy,
}

impl fmt::Display for PurgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "purge resource was not found",
            Self::InvalidRequest => "purge request is invalid",
            Self::InvalidCursor => "purge candidate cursor is invalid",
            Self::PurgeNotEligible => "node is not eligible for purge",
            Self::VersionConflict { .. } => "purge node revision conflicts",
            Self::InvalidState => "purge resource is in an invalid state",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "purge metadata persisted data is invalid",
            Self::InvalidPolicy => "trash retention policy cannot be evaluated",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PurgeError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeCandidate {
    node_id: NodeId,
    library_id: LibraryId,
    owner_user_id: UserId,
    kind: NodeKind,
    trashed_at: Timestamp,
    revision: Revision,
}

impl PurgeCandidate {
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    #[must_use]
    pub const fn trashed_at(&self) -> Timestamp {
        self.trashed_at
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeCandidatePage {
    candidates: Vec<PurgeCandidate>,
    next_cursor: Option<String>,
    has_more: bool,
}

/// Deterministic outcome of the trusted metadata-purge execution boundary.
/// `AlreadyCompleted` is a successful replay after the original transaction
/// committed and its Node row was removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurgeExecutionResult {
    Completed,
    AlreadyCompleted,
}

impl PurgeCandidatePage {
    #[must_use]
    pub fn candidates(&self) -> &[PurgeCandidate] {
        &self.candidates
    }

    #[must_use]
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

/// PostgreSQL-backed retention service. The candidate query is deliberately
/// not part of the ordinary owner-scoped HTTP metadata trait; it is an
/// internal bounded worker/application boundary.
#[derive(Clone)]
pub struct TrashRetentionService {
    pool: DatabasePool,
    policy: TrashRetentionPolicy,
}

impl TrashRetentionService {
    #[must_use]
    pub fn new(pool: DatabasePool, policy: TrashRetentionPolicy) -> Self {
        Self { pool, policy }
    }

    #[must_use]
    pub const fn policy(&self) -> TrashRetentionPolicy {
        self.policy
    }

    pub async fn list_purge_candidates(
        &self,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<PurgeCandidatePage, PurgeError> {
        let limit = validate_limit(limit)?;
        let now = now();
        let current_cutoff = self
            .policy
            .eligibility_cutoff(now)
            .ok_or(PurgeError::InvalidPolicy)?;
        let after = decode_cursor(cursor.as_deref())?;
        let cutoff = after.map_or(current_cutoff, |cursor| {
            // A cursor is a snapshot boundary. Keeping its cutoff stable
            // prevents time advancing between pages from reordering the set.
            cursor.cutoff
        });
        if cutoff > current_cutoff {
            return Err(PurgeError::InvalidCursor);
        }

        let rows = DomainRepository::new(&self.pool)
            .list_purge_candidates(cutoff, after, limit)
            .await
            .map_err(map_metadata_error)?;
        let candidates = rows
            .candidates
            .into_iter()
            .map(PurgeCandidate::from_record)
            .collect::<Vec<_>>();
        let next_cursor = rows.has_more.then(|| {
            candidates
                .last()
                .map(|candidate| encode_cursor(cutoff, candidate.trashed_at(), candidate.node_id()))
        });
        Ok(PurgeCandidatePage {
            candidates,
            next_cursor: next_cursor.flatten(),
            has_more: rows.has_more,
        })
    }

    /// Begin the metadata-only purge transition using the server clock. The
    /// owner is checked inside the same transaction as the row lock and state
    /// change; possessing a `NodeId` is never authorization.
    pub async fn begin_node_purge(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<synveil_core::Node, PurgeError> {
        let observed_at = now();
        let cutoff = self
            .policy
            .eligibility_cutoff(observed_at)
            .ok_or(PurgeError::InvalidPolicy)?;
        match DomainRepository::new(&self.pool)
            .begin_node_purge_owned(
                owner_user_id,
                node_id,
                expected_revision,
                cutoff,
                observed_at,
            )
            .await
            .map_err(map_metadata_error)?
        {
            PurgeMutation::Applied(node) => Ok(node),
            PurgeMutation::NotFound => Err(PurgeError::NotFound),
            PurgeMutation::VersionConflict(node) => Err(PurgeError::VersionConflict {
                current_revision: node.revision(),
            }),
            PurgeMutation::NotEligible => Err(PurgeError::PurgeNotEligible),
        }
    }

    /// Execute the permanent metadata phase for one node already in the
    /// canonical `PURGING` state. This is an internal trusted-worker boundary,
    /// not a public destructive HTTP route. The owner and expected revision
    /// remain transactionally rechecked even for this internal operation.
    pub async fn execute_metadata_purge(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<PurgeExecutionResult, PurgeError> {
        match DomainRepository::new(&self.pool)
            .execute_node_purge_owned(owner_user_id, node_id, expected_revision, now())
            .await
            .map_err(map_metadata_error)?
        {
            PurgeExecutionMutation::Completed => Ok(PurgeExecutionResult::Completed),
            PurgeExecutionMutation::AlreadyCompleted => Ok(PurgeExecutionResult::AlreadyCompleted),
            PurgeExecutionMutation::NotFound => Err(PurgeError::NotFound),
            PurgeExecutionMutation::VersionConflict { current_revision } => {
                Err(PurgeError::VersionConflict { current_revision })
            }
            PurgeExecutionMutation::InvalidState => Err(PurgeError::InvalidState),
            PurgeExecutionMutation::NotEligible => Err(PurgeError::PurgeNotEligible),
        }
    }
}

impl PurgeCandidate {
    fn from_record(record: PurgeCandidateRecord) -> Self {
        Self {
            node_id: record.node_id,
            library_id: record.library_id,
            owner_user_id: record.owner_user_id,
            kind: record.kind,
            trashed_at: record.trashed_at,
            revision: record.revision,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PurgeCursor {
    pub(crate) cutoff: Timestamp,
    pub(crate) trashed_at: Timestamp,
    pub(crate) node_id: NodeId,
}

fn validate_limit(limit: u32) -> Result<u32, PurgeError> {
    if (1..=MAX_PURGE_CANDIDATE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(PurgeError::InvalidRequest)
    }
}

fn map_metadata_error(error: MetadataError) -> PurgeError {
    match error {
        MetadataError::Database(error) => PurgeError::Database(error),
        MetadataError::Mapping(MappingError::Domain(_)) => PurgeError::InvalidState,
        MetadataError::Mapping(_) => PurgeError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => PurgeError::InvalidState,
    }
}

fn now() -> Timestamp {
    let value = Timestamp::now().as_offset_datetime();
    let nanos = value.nanosecond();
    Timestamp::from_offset_datetime(
        value
            .replace_nanosecond(nanos / 1_000 * 1_000)
            .expect("microsecond truncation remains a valid timestamp"),
    )
}

fn encode_cursor(cutoff: Timestamp, trashed_at: Timestamp, node_id: NodeId) -> String {
    format!(
        "{CURSOR_VERSION}.{CURSOR_KIND}.{}.{}.{}",
        encode_hex(cutoff.to_string().as_bytes()),
        encode_hex(trashed_at.to_string().as_bytes()),
        encode_hex(node_id.as_bytes())
    )
}

fn decode_cursor(value: Option<&str>) -> Result<Option<PurgeCursor>, PurgeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(PurgeError::InvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != CURSOR_PARTS || parts[0] != CURSOR_VERSION || parts[1] != CURSOR_KIND {
        return Err(PurgeError::InvalidCursor);
    }
    let cutoff = decode_timestamp_hex(parts[2])?;
    let trashed_at = decode_timestamp_hex(parts[3])?;
    let node_id = decode_node_id_hex(parts[4])?;
    Ok(Some(PurgeCursor {
        cutoff,
        trashed_at,
        node_id,
    }))
}

fn decode_timestamp_hex(value: &str) -> Result<Timestamp, PurgeError> {
    let bytes = decode_hex(value)?;
    let value = String::from_utf8(bytes).map_err(|_| PurgeError::InvalidCursor)?;
    let timestamp = Timestamp::from_str(&value).map_err(|_| PurgeError::InvalidCursor)?;
    if timestamp.to_string() != value {
        return Err(PurgeError::InvalidCursor);
    }
    Ok(timestamp)
}

fn decode_node_id_hex(value: &str) -> Result<NodeId, PurgeError> {
    let bytes = decode_hex(value)?;
    let bytes: [u8; 16] = bytes.try_into().map_err(|_| PurgeError::InvalidCursor)?;
    NodeId::try_from(uuid::Uuid::from_bytes(bytes)).map_err(|_| PurgeError::InvalidCursor)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn decode_hex(value: &str) -> Result<Vec<u8>, PurgeError> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(PurgeError::InvalidCursor);
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or(PurgeError::InvalidCursor)?;
            let low = hex_nibble(pair[1]).ok_or(PurgeError::InvalidCursor)?;
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
        CURSOR_KIND, CURSOR_VERSION, PurgeCursor, PurgeError, decode_cursor, encode_cursor,
        validate_limit,
    };
    use synveil_core::{NodeId, Timestamp};

    #[test]
    fn purge_cursors_are_snapshot_scoped_and_opaque() {
        let cutoff = Timestamp::parse("2026-07-25T00:00:00Z").unwrap();
        let trashed_at = Timestamp::parse("2026-07-24T00:00:00Z").unwrap();
        let node_id = NodeId::new();
        let cursor = encode_cursor(cutoff, trashed_at, node_id);

        assert!(cursor.starts_with(&format!("{CURSOR_VERSION}.{CURSOR_KIND}.")));
        assert!(!cursor.contains(&node_id.to_string()));
        assert_eq!(
            decode_cursor(Some(&cursor)),
            Ok(Some(PurgeCursor {
                cutoff,
                trashed_at,
                node_id,
            }))
        );
    }

    #[test]
    fn purge_cursors_reject_malformed_values_and_limits_are_bounded() {
        assert_eq!(
            decode_cursor(Some("v0.purge.bad")),
            Err(PurgeError::InvalidCursor)
        );
        assert_eq!(
            decode_cursor(Some("v1.purge.zz.zz.zz")),
            Err(PurgeError::InvalidCursor)
        );
        assert!(validate_limit(1).is_ok());
        assert!(validate_limit(super::MAX_PURGE_CANDIDATE_LIMIT).is_ok());
        assert_eq!(validate_limit(0), Err(PurgeError::InvalidRequest));
        assert_eq!(
            validate_limit(super::MAX_PURGE_CANDIDATE_LIMIT + 1),
            Err(PurgeError::InvalidRequest)
        );
    }
}
