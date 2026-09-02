//! Owner-authorized immutable file-version metadata application boundary.
//!
//! This module exposes the logical history of a file without opening an
//! object store or returning any physical storage identity. The PostgreSQL
//! repository owns the joins and integrity checks; this service owns bounded
//! pagination, cursor scope, and the safe application-facing result.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use synveil_core::{FileVersionId, NodeId, Revision, Sha256Digest, Timestamp, UserId};

use crate::{DatabaseError, DatabasePool, DomainRepository, MAX_PAGE_LIMIT, MetadataError};

const CURSOR_VERSION: &str = "v1";
const CURSOR_KIND: &str = "version";
const CURSOR_PARTS: usize = 5;
const MAX_CURSOR_BYTES: usize = 512;
pub const MIN_RESTORE_IDEMPOTENCY_KEY_BYTES: usize = 8;
pub const MAX_RESTORE_IDEMPOTENCY_KEY_BYTES: usize = 256;

/// Safe immutable metadata for one owner-visible file version.
///
/// The record deliberately contains no object, replica, backend, staging, or
/// filesystem identity. `committed_at` is the canonical server-observed instant
/// and is named `created_at` by the HTTP DTO to match the public metadata
/// vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileVersionMetadata {
    id: FileVersionId,
    node_id: NodeId,
    committed_at: Timestamp,
    byte_length: u64,
    sha256: Sha256Digest,
    is_current: bool,
}

impl FileVersionMetadata {
    #[must_use]
    pub const fn new(
        id: FileVersionId,
        node_id: NodeId,
        committed_at: Timestamp,
        byte_length: u64,
        sha256: Sha256Digest,
        is_current: bool,
    ) -> Self {
        Self {
            id,
            node_id,
            committed_at,
            byte_length,
            sha256,
            is_current,
        }
    }

    #[must_use]
    pub const fn id(self) -> FileVersionId {
        self.id
    }

    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn committed_at(self) -> Timestamp {
        self.committed_at
    }

    #[must_use]
    pub const fn byte_length(self) -> u64 {
        self.byte_length
    }

    #[must_use]
    pub const fn sha256(self) -> Sha256Digest {
        self.sha256
    }

    #[must_use]
    pub const fn is_current(self) -> bool {
        self.is_current
    }
}

/// A bounded, deterministic page of immutable file-version metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileVersionPage {
    versions: Vec<FileVersionMetadata>,
    next_cursor: Option<String>,
    has_more: bool,
}

/// Safe result of a historical-version restore. The node revision is returned
/// separately because it is the concurrency token for the mutable node; no
/// storage or object-replica identity crosses this application boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoredFileVersion {
    version: FileVersionMetadata,
    node_revision: Revision,
}

impl RestoredFileVersion {
    #[must_use]
    pub const fn new(version: FileVersionMetadata, node_revision: Revision) -> Self {
        Self {
            version,
            node_revision,
        }
    }

    #[must_use]
    pub const fn version(&self) -> FileVersionMetadata {
        self.version
    }

    #[must_use]
    pub const fn node_revision(&self) -> Revision {
        self.node_revision
    }
}

impl FileVersionPage {
    #[must_use]
    pub fn new(
        versions: Vec<FileVersionMetadata>,
        next_cursor: Option<String>,
        has_more: bool,
    ) -> Self {
        Self {
            versions,
            next_cursor,
            has_more,
        }
    }

    #[must_use]
    pub fn versions(&self) -> &[FileVersionMetadata] {
        &self.versions
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

/// Safe application failures for version-history reads.
///
/// `NotFound` is also the concealed outcome for an unknown, cross-owner, or
/// trashed/purging node/version. This prevents typed IDs from becoming an
/// existence oracle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionHistoryError {
    NotFound,
    InvalidRequest,
    InvalidCursor,
    InvalidState,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for VersionHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "file version metadata was not found",
            Self::InvalidRequest => "file version metadata request is invalid",
            Self::InvalidCursor => "file version metadata cursor is invalid",
            Self::InvalidState => "file version metadata resource is in an invalid state",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "file version metadata persisted data is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for VersionHistoryError {}

/// Safe application failures for an authenticated historical-version restore.
/// Cross-owner and cross-node source failures use `NotFound` so identifiers do
/// not become an ownership or relationship oracle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionRestoreError {
    NotFound,
    InvalidRequest,
    InvalidState,
    VersionConflict { current_revision: Revision },
    ContentUnavailable,
    IdempotencyConflict,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for VersionRestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "file version restore source was not found",
            Self::InvalidRequest => "file version restore request is invalid",
            Self::InvalidState => "file version restore resource is in an invalid state",
            Self::VersionConflict { .. } => "file version restore revision conflicts",
            Self::ContentUnavailable => "file version restore content is unavailable",
            Self::IdempotencyConflict => "file version restore idempotency key conflicts",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "file version restore persisted data is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for VersionRestoreError {}

/// Transport-neutral metadata port for authenticated immutable version reads.
#[async_trait]
pub trait VersionHistoryBackend: Send + Sync {
    async fn list_file_versions(
        &self,
        user_id: UserId,
        node_id: NodeId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<FileVersionPage, VersionHistoryError>;

    async fn get_file_version_metadata(
        &self,
        user_id: UserId,
        version_id: FileVersionId,
    ) -> Result<FileVersionMetadata, VersionHistoryError>;
}

/// Transport-neutral owner-authorized restore port. The idempotency key is
/// part of this boundary so every caller receives the same retry-safe
/// transaction semantics, not only the HTTP adapter.
#[async_trait]
pub trait VersionRestoreBackend: Send + Sync {
    async fn restore_file_version(
        &self,
        user_id: UserId,
        node_id: NodeId,
        source_version_id: FileVersionId,
        expected_revision: Revision,
        idempotency_key: String,
    ) -> Result<RestoredFileVersion, VersionRestoreError>;
}

/// PostgreSQL-backed immutable version-history application service.
#[derive(Clone)]
pub struct VersionHistoryService {
    pool: DatabasePool,
}

impl VersionHistoryService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    pub async fn list_file_versions(
        &self,
        user_id: UserId,
        node_id: NodeId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<FileVersionPage, VersionHistoryError> {
        let limit = validate_limit(limit)?;
        let after = decode_cursor(cursor.as_deref(), node_id)?;
        let page = DomainRepository::new(&self.pool)
            .list_owned_file_versions(user_id, node_id, after, limit)
            .await
            .map_err(map_metadata_error)?
            .ok_or(VersionHistoryError::NotFound)?;

        ensure_visible_file(&page.node)?;
        let next_cursor = page
            .has_more
            .then(|| {
                page.versions
                    .last()
                    .map(|record| encode_cursor(node_id, record.committed_at, record.id))
            })
            .flatten();
        let versions = page
            .versions
            .into_iter()
            .map(|record| FileVersionMetadata::from_record(record, page.current_version_id))
            .collect::<Vec<_>>();
        Ok(FileVersionPage::new(versions, next_cursor, page.has_more))
    }

    pub async fn get_file_version_metadata(
        &self,
        user_id: UserId,
        version_id: FileVersionId,
    ) -> Result<FileVersionMetadata, VersionHistoryError> {
        let owned = DomainRepository::new(&self.pool)
            .find_owned_file_version(user_id, version_id)
            .await
            .map_err(map_metadata_error)?
            .ok_or(VersionHistoryError::NotFound)?;
        ensure_visible_file(&owned.node)?;
        Ok(FileVersionMetadata::from_record(
            owned.record,
            owned.current_version_id,
        ))
    }
}

/// PostgreSQL-backed safe historical-version restore application service.
#[derive(Clone)]
pub struct VersionRestoreService {
    pool: DatabasePool,
}

impl VersionRestoreService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    pub async fn restore_file_version(
        &self,
        user_id: UserId,
        node_id: NodeId,
        source_version_id: FileVersionId,
        expected_revision: Revision,
        idempotency_key: String,
    ) -> Result<RestoredFileVersion, VersionRestoreError> {
        validate_restore_idempotency_key(&idempotency_key)?;
        let mutation = DomainRepository::new(&self.pool)
            .restore_file_version_owned(
                user_id,
                node_id,
                source_version_id,
                expected_revision,
                &idempotency_key,
                now(),
            )
            .await
            .map_err(map_restore_metadata_error)?;
        match mutation {
            crate::repository::VersionRestoreMutation::Applied {
                record,
                node_revision,
            } => Ok(RestoredFileVersion::new(
                FileVersionMetadata::from_record(record, Some(record.id)),
                node_revision,
            )),
            crate::repository::VersionRestoreMutation::Replayed {
                record,
                node_revision,
                current_version_id,
            } => Ok(RestoredFileVersion::new(
                FileVersionMetadata::from_record(record, current_version_id),
                node_revision,
            )),
            crate::repository::VersionRestoreMutation::NotFound => {
                Err(VersionRestoreError::NotFound)
            }
            crate::repository::VersionRestoreMutation::InvalidRequest => {
                Err(VersionRestoreError::InvalidRequest)
            }
            crate::repository::VersionRestoreMutation::InvalidState => {
                Err(VersionRestoreError::InvalidState)
            }
            crate::repository::VersionRestoreMutation::VersionConflict { current_revision } => {
                Err(VersionRestoreError::VersionConflict { current_revision })
            }
            crate::repository::VersionRestoreMutation::ContentUnavailable => {
                Err(VersionRestoreError::ContentUnavailable)
            }
            crate::repository::VersionRestoreMutation::IdempotencyConflict => {
                Err(VersionRestoreError::IdempotencyConflict)
            }
        }
    }
}

#[async_trait]
impl VersionRestoreBackend for VersionRestoreService {
    async fn restore_file_version(
        &self,
        user_id: UserId,
        node_id: NodeId,
        source_version_id: FileVersionId,
        expected_revision: Revision,
        idempotency_key: String,
    ) -> Result<RestoredFileVersion, VersionRestoreError> {
        self.restore_file_version(
            user_id,
            node_id,
            source_version_id,
            expected_revision,
            idempotency_key,
        )
        .await
    }
}

#[async_trait]
impl VersionHistoryBackend for VersionHistoryService {
    async fn list_file_versions(
        &self,
        user_id: UserId,
        node_id: NodeId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<FileVersionPage, VersionHistoryError> {
        self.list_file_versions(user_id, node_id, cursor, limit)
            .await
    }

    async fn get_file_version_metadata(
        &self,
        user_id: UserId,
        version_id: FileVersionId,
    ) -> Result<FileVersionMetadata, VersionHistoryError> {
        self.get_file_version_metadata(user_id, version_id).await
    }
}

impl FileVersionMetadata {
    fn from_record(record: VersionRecord, current_version_id: Option<FileVersionId>) -> Self {
        Self::new(
            record.id,
            record.node_id,
            record.committed_at,
            record.byte_length,
            record.sha256,
            current_version_id == Some(record.id),
        )
    }
}

/// The keyset boundary is internal to the metadata adapter. Its serialized
/// cursor contains a node scope and both immutable ordering keys, but those
/// values are hex-encoded so callers must treat the token as opaque.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VersionCursor {
    pub(crate) node_id: NodeId,
    pub(crate) committed_at: Timestamp,
    pub(crate) version_id: FileVersionId,
}

/// A repository result before conversion to the public application DTO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VersionRecord {
    pub(crate) id: FileVersionId,
    pub(crate) node_id: NodeId,
    pub(crate) committed_at: Timestamp,
    pub(crate) byte_length: u64,
    pub(crate) sha256: Sha256Digest,
}

fn validate_limit(limit: u32) -> Result<u32, VersionHistoryError> {
    if (1..=MAX_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(VersionHistoryError::InvalidRequest)
    }
}

fn ensure_visible_file(node: &synveil_core::Node) -> Result<(), VersionHistoryError> {
    if node.kind() != synveil_core::NodeKind::File {
        return Err(VersionHistoryError::InvalidState);
    }
    if node.state() != synveil_core::NodeState::Active {
        return Err(VersionHistoryError::NotFound);
    }
    Ok(())
}

fn map_metadata_error(error: MetadataError) -> VersionHistoryError {
    match error {
        MetadataError::Database(error) => VersionHistoryError::Database(error),
        MetadataError::Mapping(_) | MetadataError::CapacityUnavailable => {
            VersionHistoryError::InvalidPersistedData
        }
    }
}

fn map_restore_metadata_error(error: MetadataError) -> VersionRestoreError {
    match error {
        MetadataError::Database(error) => VersionRestoreError::Database(error),
        MetadataError::Mapping(_) | MetadataError::CapacityUnavailable => {
            VersionRestoreError::InvalidPersistedData
        }
    }
}

fn validate_restore_idempotency_key(value: &str) -> Result<(), VersionRestoreError> {
    if !(MIN_RESTORE_IDEMPOTENCY_KEY_BYTES..=MAX_RESTORE_IDEMPOTENCY_KEY_BYTES)
        .contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-'))
    {
        return Err(VersionRestoreError::InvalidRequest);
    }
    Ok(())
}

pub(crate) fn restore_request_fingerprint(
    node_id: NodeId,
    source_version_id: FileVersionId,
    expected_revision: Revision,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"synveil/version-restore/v1\0");
    hasher.update(node_id.as_bytes());
    hasher.update(source_version_id.as_bytes());
    hasher.update(expected_revision.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut result = [0_u8; 32];
    result.copy_from_slice(&digest);
    result
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

fn encode_cursor(node_id: NodeId, committed_at: Timestamp, version_id: FileVersionId) -> String {
    format!(
        "{CURSOR_VERSION}.{CURSOR_KIND}.{}.{}.{}",
        encode_hex(node_id.as_bytes()),
        encode_hex(committed_at.to_string().as_bytes()),
        encode_hex(version_id.as_bytes())
    )
}

fn decode_cursor(
    value: Option<&str>,
    node_id: NodeId,
) -> Result<Option<VersionCursor>, VersionHistoryError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_CURSOR_BYTES {
        return Err(VersionHistoryError::InvalidCursor);
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != CURSOR_PARTS || parts[0] != CURSOR_VERSION || parts[1] != CURSOR_KIND {
        return Err(VersionHistoryError::InvalidCursor);
    }

    let cursor_node = decode_id_hex::<NodeId>(parts[2])?;
    if cursor_node != node_id {
        return Err(VersionHistoryError::InvalidCursor);
    }
    let timestamp_text =
        String::from_utf8(decode_hex(parts[3])?).map_err(|_| VersionHistoryError::InvalidCursor)?;
    let committed_at =
        Timestamp::from_str(&timestamp_text).map_err(|_| VersionHistoryError::InvalidCursor)?;
    if committed_at.to_string() != timestamp_text {
        return Err(VersionHistoryError::InvalidCursor);
    }
    let version_id = decode_id_hex::<FileVersionId>(parts[4])?;
    Ok(Some(VersionCursor {
        node_id: cursor_node,
        committed_at,
        version_id,
    }))
}

fn decode_id_hex<T>(value: &str) -> Result<T, VersionHistoryError>
where
    T: TryFrom<uuid::Uuid, Error = synveil_core::IdParseError>,
{
    let bytes = decode_hex(value)?;
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| VersionHistoryError::InvalidCursor)?;
    T::try_from(uuid::Uuid::from_bytes(bytes)).map_err(|_| VersionHistoryError::InvalidCursor)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn decode_hex(value: &str) -> Result<Vec<u8>, VersionHistoryError> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(VersionHistoryError::InvalidCursor);
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or(VersionHistoryError::InvalidCursor)?;
            let low = hex_nibble(pair[1]).ok_or(VersionHistoryError::InvalidCursor)?;
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
        CURSOR_KIND, CURSOR_VERSION, MAX_RESTORE_IDEMPOTENCY_KEY_BYTES,
        MIN_RESTORE_IDEMPOTENCY_KEY_BYTES, VersionCursor, VersionHistoryError, VersionRestoreError,
        decode_cursor, encode_cursor, restore_request_fingerprint,
        validate_restore_idempotency_key,
    };
    use synveil_core::{FileVersionId, NodeId, Revision, Timestamp};

    #[test]
    fn cursors_are_opaque_stable_and_node_scoped() {
        let node = NodeId::new();
        let version = FileVersionId::new();
        let timestamp = Timestamp::parse("2026-08-24T00:00:00.123456Z").unwrap();
        let cursor = encode_cursor(node, timestamp, version);

        assert!(cursor.starts_with(&format!("{CURSOR_VERSION}.{CURSOR_KIND}.")));
        assert!(!cursor.contains(&node.to_string()));
        assert_eq!(
            decode_cursor(Some(&cursor), node),
            Ok(Some(VersionCursor {
                node_id: node,
                committed_at: timestamp,
                version_id: version,
            }))
        );
        assert_eq!(
            decode_cursor(Some(&cursor), NodeId::new()),
            Err(VersionHistoryError::InvalidCursor)
        );
    }

    #[test]
    fn cursors_reject_tampering_and_malformed_values() {
        let node = NodeId::new();
        let version = FileVersionId::new();
        let timestamp = Timestamp::parse("2026-08-24T00:00:00Z").unwrap();
        let cursor = encode_cursor(node, timestamp, version);
        assert_eq!(
            decode_cursor(Some("v0.version.bad"), node),
            Err(VersionHistoryError::InvalidCursor)
        );
        let mut parts: Vec<_> = cursor.split('.').map(str::to_owned).collect();
        parts[3].replace_range(0..2, "zz");
        assert_eq!(
            decode_cursor(Some(&parts.join(".")), node),
            Err(VersionHistoryError::InvalidCursor)
        );
        let oversized = format!("v1.version.{}", "a".repeat(600));
        assert_eq!(
            decode_cursor(Some(&oversized), node),
            Err(VersionHistoryError::InvalidCursor)
        );
    }

    #[test]
    fn restore_idempotency_keys_are_bounded_and_ascii_scoped() {
        assert!(validate_restore_idempotency_key("restore-key-1").is_ok());
        assert!(
            validate_restore_idempotency_key(&"a".repeat(MIN_RESTORE_IDEMPOTENCY_KEY_BYTES))
                .is_ok()
        );
        assert!(
            validate_restore_idempotency_key(&"a".repeat(MAX_RESTORE_IDEMPOTENCY_KEY_BYTES))
                .is_ok()
        );
        for key in [
            "short",
            "contains space",
            "contains/slash",
            "contains,comma",
            "contains\nnewline",
        ] {
            assert_eq!(
                validate_restore_idempotency_key(key),
                Err(VersionRestoreError::InvalidRequest)
            );
        }
        assert_eq!(
            validate_restore_idempotency_key(&"a".repeat(MAX_RESTORE_IDEMPOTENCY_KEY_BYTES + 1)),
            Err(VersionRestoreError::InvalidRequest)
        );
    }

    #[test]
    fn restore_request_fingerprint_binds_the_mutation_identity() {
        let node = NodeId::new();
        let source = FileVersionId::new();
        let fingerprint = restore_request_fingerprint(node, source, Revision::new(4));
        assert_eq!(
            fingerprint,
            restore_request_fingerprint(node, source, Revision::new(4))
        );
        assert_ne!(
            fingerprint,
            restore_request_fingerprint(node, source, Revision::new(5))
        );
        assert_ne!(
            fingerprint,
            restore_request_fingerprint(NodeId::new(), source, Revision::new(4))
        );
        assert_ne!(
            fingerprint,
            restore_request_fingerprint(node, FileVersionId::new(), Revision::new(4))
        );
    }
}
