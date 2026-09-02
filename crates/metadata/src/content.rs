//! Owner-authorized immutable content-resolution metadata boundary.
//!
//! This module resolves logical file/version metadata to one verified replica
//! without exposing SQL rows or treating object identity as authorization.
//! It deliberately does not read bytes, open paths, or mutate read state.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use sqlx::FromRow;
use synveil_core::{
    DedupDomainId, FileVersionId, NodeId, ObjectId, Revision, Sha256Digest, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{DatabasePool, MappingError, MetadataError};

/// Trusted metadata needed by the storage application service to open one
/// immutable object. The storage key is intentionally redacted from `Debug`
/// and must never cross a transport boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct AuthorizedContent {
    node_id: NodeId,
    file_version_id: FileVersionId,
    object_id: ObjectId,
    length: u64,
    sha256: Sha256Digest,
    file_version_revision: Revision,
    committed_at: Timestamp,
    storage_key: String,
}

impl AuthorizedContent {
    /// Construct a trusted record for a metadata implementation or a focused
    /// application test. The storage service validates the opaque key again
    /// before giving it to an `ObjectStore`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: NodeId,
        file_version_id: FileVersionId,
        object_id: ObjectId,
        length: u64,
        sha256: Sha256Digest,
        file_version_revision: Revision,
        committed_at: Timestamp,
        storage_key: impl Into<String>,
    ) -> Self {
        Self {
            node_id,
            file_version_id,
            object_id,
            length,
            sha256,
            file_version_revision,
            committed_at,
            storage_key: storage_key.into(),
        }
    }

    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn file_version_id(&self) -> FileVersionId {
        self.file_version_id
    }

    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(&self) -> Sha256Digest {
        self.sha256
    }

    #[must_use]
    pub const fn file_version_revision(&self) -> Revision {
        self.file_version_revision
    }

    #[must_use]
    pub const fn committed_at(&self) -> Timestamp {
        self.committed_at
    }

    /// Internal storage-composition evidence. Never serialize this value for
    /// an API, browser, log, or diagnostic response.
    #[doc(hidden)]
    #[must_use]
    pub fn storage_key(&self) -> &str {
        &self.storage_key
    }
}

impl fmt::Debug for AuthorizedContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedContent")
            .field("node_id", &self.node_id)
            .field("file_version_id", &self.file_version_id)
            .field("object_id", &self.object_id)
            .field("length", &self.length)
            .field("sha256", &"<redacted>")
            .field("file_version_revision", &self.file_version_revision)
            .field("committed_at", &self.committed_at)
            .field("storage_key", &"<redacted>")
            .finish()
    }
}

/// Owner-concealing outcomes for a content-resolution query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentReadResolution {
    /// The logical resource is absent, inaccessible, or not active for the
    /// normal content path.
    NotFound,
    /// The owner can see the node but it is a directory rather than a file.
    NotAFile,
    /// Canonical metadata exists but no readable verified replica is selected
    /// for the configured storage backend.
    ContentUnavailable,
    /// One immutable, owner-authorized content identity and replica.
    Found(AuthorizedContent),
}

/// Metadata port used by the transport-neutral content-read application
/// service. The backend kind comes from trusted runtime composition, never a
/// caller-supplied object-store selector.
#[async_trait]
pub trait ContentReadMetadataBackend: Send + Sync {
    async fn resolve_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError>;

    async fn resolve_file_version_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError>;
}

#[derive(FromRow)]
struct OwnedNodeRow {
    id: Uuid,
    library_id: Uuid,
    kind: String,
    state: String,
    current_version_id: Option<Uuid>,
}

#[derive(FromRow)]
struct VersionObjectRow {
    file_version_id: Uuid,
    node_id: Uuid,
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    canonical_hash: Vec<u8>,
    plaintext_length: String,
    committed_at: OffsetDateTime,
    file_version_revision: String,
}

#[derive(FromRow)]
struct HistoricalVersionRow {
    node_kind: String,
    node_state: String,
    file_version_id: Uuid,
    node_id: Uuid,
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    canonical_hash: Vec<u8>,
    plaintext_length: String,
    committed_at: OffsetDateTime,
    file_version_revision: String,
}

#[derive(FromRow)]
struct ReplicaRow {
    storage_key: String,
    stored_length: String,
    stored_sha256: Vec<u8>,
}

/// PostgreSQL implementation of the immutable content-resolution port.
#[derive(Clone)]
pub struct PostgresContentReadRepository {
    pool: DatabasePool,
}

impl PostgresContentReadRepository {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }
}

#[async_trait]
impl ContentReadMetadataBackend for PostgresContentReadRepository {
    async fn resolve_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError> {
        let node = sqlx::query_as::<_, OwnedNodeRow>(
            "SELECT n.id, n.library_id, n.kind, n.state, n.current_version_id
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE n.id = $1 AND l.owner_user_id = $2",
        )
        .bind(node_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        let Some(node) = node else {
            return Ok(ContentReadResolution::NotFound);
        };

        if !is_active_node_state(&node.state)? {
            return Ok(ContentReadResolution::NotFound);
        }
        if is_directory(&node.kind)? {
            return Ok(ContentReadResolution::NotAFile);
        }

        let Some(current_version_id) = node.current_version_id else {
            return Ok(ContentReadResolution::NotFound);
        };
        let current_version_id = decode_id(
            current_version_id,
            "nodes.current_version_id",
            FileVersionId::try_from_uuid,
        )?;
        let version =
            load_version_for_node(&self.pool, current_version_id, node.id, node.library_id)
                .await?
                .ok_or_else(|| relation_mismatch("nodes.current_version_id"))?;

        resolve_replica(&self.pool, version, backend_kind).await
    }

    async fn resolve_file_version_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError> {
        let row = sqlx::query_as::<_, HistoricalVersionRow>(
            "SELECT n.kind AS node_kind, n.state AS node_state,
                    fv.id AS file_version_id, fv.node_id, fv.object_id,
                    fv.object_dedup_domain_id, o.canonical_hash,
                    o.plaintext_length::TEXT AS plaintext_length, fv.committed_at,
                    fv.revision::TEXT AS file_version_revision
             FROM file_versions AS fv
             INNER JOIN nodes AS n
                ON n.id = fv.node_id AND n.library_id = fv.library_id
             INNER JOIN libraries AS l ON l.id = fv.library_id
             INNER JOIN objects AS o
                ON o.id = fv.object_id
               AND o.dedup_domain_id = fv.object_dedup_domain_id
             WHERE fv.id = $1 AND l.owner_user_id = $2",
        )
        .bind(file_version_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        let Some(row) = row else {
            return Ok(ContentReadResolution::NotFound);
        };

        if !is_active_node_state(&row.node_state)? {
            return Ok(ContentReadResolution::NotFound);
        }
        if is_directory(&row.node_kind)? {
            return Err(relation_mismatch("file_versions.node_id"));
        }

        let version = VersionObjectRow {
            file_version_id: row.file_version_id,
            node_id: row.node_id,
            object_id: row.object_id,
            object_dedup_domain_id: row.object_dedup_domain_id,
            canonical_hash: row.canonical_hash,
            plaintext_length: row.plaintext_length,
            committed_at: row.committed_at,
            file_version_revision: row.file_version_revision,
        };
        resolve_replica(&self.pool, version, backend_kind).await
    }
}

async fn load_version_for_node(
    pool: &DatabasePool,
    file_version_id: FileVersionId,
    node_id: Uuid,
    library_id: Uuid,
) -> Result<Option<VersionObjectRow>, MetadataError> {
    sqlx::query_as::<_, VersionObjectRow>(
        "SELECT fv.id AS file_version_id, fv.node_id, fv.object_id,
                fv.object_dedup_domain_id, o.canonical_hash,
                o.plaintext_length::TEXT AS plaintext_length, fv.committed_at,
                fv.revision::TEXT AS file_version_revision
         FROM file_versions AS fv
         INNER JOIN objects AS o
            ON o.id = fv.object_id
           AND o.dedup_domain_id = fv.object_dedup_domain_id
         WHERE fv.id = $1 AND fv.node_id = $2 AND fv.library_id = $3",
    )
    .bind(file_version_id.into_uuid())
    .bind(node_id)
    .bind(library_id)
    .fetch_optional(pool.sqlx_pool())
    .await
    .map_err(MetadataError::from)
}

async fn resolve_replica(
    pool: &DatabasePool,
    version: VersionObjectRow,
    backend_kind: &str,
) -> Result<ContentReadResolution, MetadataError> {
    let replica = sqlx::query_as::<_, ReplicaRow>(
        "SELECT storage_key, stored_length::TEXT AS stored_length, stored_sha256
         FROM object_replicas
         WHERE object_id = $1
           AND object_dedup_domain_id = $2
           AND backend_kind = $3
           AND state = 'VERIFIED'
         ORDER BY verified_at ASC, id ASC
         LIMIT 1",
    )
    .bind(version.object_id)
    .bind(version.object_dedup_domain_id)
    .bind(backend_kind)
    .fetch_optional(pool.sqlx_pool())
    .await
    .map_err(MetadataError::from)?;
    let Some(replica) = replica else {
        return Ok(ContentReadResolution::ContentUnavailable);
    };

    let content = authorized_content_from_rows(version, replica)?;
    Ok(ContentReadResolution::Found(content))
}

fn authorized_content_from_rows(
    version: VersionObjectRow,
    replica: ReplicaRow,
) -> Result<AuthorizedContent, MetadataError> {
    let node_id = decode_id(
        version.node_id,
        "file_versions.node_id",
        NodeId::try_from_uuid,
    )?;
    let file_version_id = decode_id(
        version.file_version_id,
        "file_versions.id",
        FileVersionId::try_from_uuid,
    )?;
    let object_id = decode_id(version.object_id, "objects.id", ObjectId::try_from_uuid)?;
    let _dedup_domain_id = decode_id(
        version.object_dedup_domain_id,
        "objects.dedup_domain_id",
        DedupDomainId::try_from_uuid,
    )?;
    let length = decode_u64(&version.plaintext_length, "objects.plaintext_length")?;
    let sha256 = Sha256Digest::try_from(version.canonical_hash.as_slice()).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDigest {
            field: "objects.canonical_hash",
        })
    })?;
    let stored_length = decode_u64(&replica.stored_length, "object_replicas.stored_length")?;
    let stored_sha256 = Sha256Digest::try_from(replica.stored_sha256.as_slice()).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDigest {
            field: "object_replicas.stored_sha256",
        })
    })?;
    if stored_length != length || stored_sha256 != sha256 {
        return Err(relation_mismatch("object_replicas.integrity"));
    }
    let file_version_revision =
        Revision::from_str(&version.file_version_revision).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidDecimal {
                field: "file_versions.revision",
            })
        })?;

    Ok(AuthorizedContent::new(
        node_id,
        file_version_id,
        object_id,
        length,
        sha256,
        file_version_revision,
        Timestamp::from_offset_datetime(version.committed_at),
        replica.storage_key,
    ))
}

fn decode_id<T>(
    value: Uuid,
    field: &'static str,
    decode: impl FnOnce(Uuid) -> Result<T, synveil_core::IdParseError>,
) -> Result<T, MetadataError> {
    decode(value)
        .map_err(|reason| MetadataError::Mapping(MappingError::InvalidId { field, reason }))
}

fn decode_u64(value: &str, field: &'static str) -> Result<u64, MetadataError> {
    value
        .parse()
        .map_err(|_| MetadataError::Mapping(MappingError::InvalidDecimal { field }))
}

fn is_active_node_state(value: &str) -> Result<bool, MetadataError> {
    match value {
        "ACTIVE" => Ok(true),
        "TRASHED" | "PURGING" => Ok(false),
        _ => Err(MetadataError::Mapping(MappingError::InvalidEnum {
            field: "nodes.state",
        })),
    }
}

fn is_directory(value: &str) -> Result<bool, MetadataError> {
    match value {
        "FILE" => Ok(false),
        "DIRECTORY" => Ok(true),
        _ => Err(MetadataError::Mapping(MappingError::InvalidEnum {
            field: "nodes.kind",
        })),
    }
}

fn relation_mismatch(relation: &'static str) -> MetadataError {
    MetadataError::Mapping(MappingError::RelationMismatch { relation })
}
