//! Focused PostgreSQL repositories for the initial canonical domain.

use std::str::FromStr;

use sqlx::FromRow;
use synveil_core::{
    DedupDomainId, Device, DomainError, FileVersion, FileVersionId, Library, LibraryId,
    LibraryStatus, LogicalName, Node, NodeId, NodeKind, NodeState, ObjectId, ObjectReference,
    Revision, Sha256Digest, Timestamp, User, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::purge::PurgeCursor;
use crate::versions::{VersionCursor, VersionRecord, restore_request_fingerprint};
use crate::{
    DatabasePool, DeviceRow, FileVersionRow, LibraryRow, MappingError, MetadataError, NodeRow,
    ObjectRow, UserRow,
};
use crate::{files::MAX_ANCESTOR_DEPTH, files::NodeMutation};

pub(crate) struct PagedNodes {
    pub(crate) nodes: Vec<Node>,
    pub(crate) has_more: bool,
}

pub(crate) struct PagedLibraries {
    pub(crate) libraries: Vec<Library>,
    pub(crate) has_more: bool,
}

pub(crate) struct VersionHistoryPage {
    pub(crate) node: Node,
    pub(crate) current_version_id: Option<FileVersionId>,
    pub(crate) versions: Vec<VersionRecord>,
    pub(crate) has_more: bool,
}

pub(crate) struct OwnedVersionRecord {
    pub(crate) node: Node,
    pub(crate) current_version_id: Option<FileVersionId>,
    pub(crate) record: VersionRecord,
}

pub(crate) struct PagedPurgeCandidates {
    pub(crate) candidates: Vec<PurgeCandidateRecord>,
    pub(crate) has_more: bool,
}

pub(crate) struct PurgeCandidateRecord {
    pub(crate) node_id: NodeId,
    pub(crate) library_id: LibraryId,
    pub(crate) owner_user_id: UserId,
    pub(crate) kind: NodeKind,
    pub(crate) trashed_at: Timestamp,
    pub(crate) revision: Revision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PurgeMutation {
    Applied(Node),
    NotFound,
    VersionConflict(Node),
    NotEligible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PurgeExecutionMutation {
    Completed,
    AlreadyCompleted,
    NotFound,
    VersionConflict { current_revision: Revision },
    InvalidState,
    NotEligible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VersionRestoreMutation {
    Applied {
        record: VersionRecord,
        node_revision: Revision,
    },
    Replayed {
        record: VersionRecord,
        node_revision: Revision,
        current_version_id: Option<FileVersionId>,
    },
    NotFound,
    InvalidRequest,
    InvalidState,
    VersionConflict {
        current_revision: Revision,
    },
    ContentUnavailable,
    IdempotencyConflict,
}

#[derive(Debug, FromRow)]
struct VersionScopeRow {
    id: Uuid,
    library_id: Uuid,
    parent_node_id: Option<Uuid>,
    kind: String,
    name: String,
    current_version_id: Option<Uuid>,
    state: String,
    trashed_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    revision: String,
    dedup_domain_id: Uuid,
    current_version_row_id: Option<Uuid>,
    current_version_node_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct PurgeCandidateRow {
    node_id: Uuid,
    library_id: Uuid,
    owner_user_id: Uuid,
    kind: String,
    trashed_at: OffsetDateTime,
    revision: String,
}

#[derive(Clone, Debug, FromRow)]
struct VersionMetadataRow {
    id: Uuid,
    library_id: Uuid,
    node_id: Uuid,
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    object_actual_dedup_domain_id: Uuid,
    parent_version_id: Option<Uuid>,
    committed_at: OffsetDateTime,
    revision: String,
    canonical_hash: Vec<u8>,
    plaintext_length: String,
    library_dedup_domain_id: Uuid,
}

#[derive(Debug, FromRow)]
struct RestoreOperationRow {
    library_id: Uuid,
    node_id: Uuid,
    source_version_id: Uuid,
    request_fingerprint: Vec<u8>,
    result_version_id: Option<Uuid>,
    result_node_revision: Option<String>,
}

struct OwnedVersionScope {
    node: Node,
    dedup_domain_id: DedupDomainId,
    current_version_id: Option<FileVersionId>,
}

/// Small persistence boundary for the validated core domain.
pub struct DomainRepository<'pool> {
    pool: &'pool DatabasePool,
}

impl<'pool> DomainRepository<'pool> {
    #[must_use]
    pub const fn new(pool: &'pool DatabasePool) -> Self {
        Self { pool }
    }

    pub async fn insert_user(&self, value: &User) -> Result<(), MetadataError> {
        self.insert_user_row(&UserRow::from_domain(value)?).await
    }

    pub async fn insert_user_row(&self, row: &UserRow) -> Result<(), MetadataError> {
        sqlx::query(
            "INSERT INTO users
                (id, login_value, login_key, status, is_instance_admin, created_at,
                 updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::NUMERIC)",
        )
        .bind(row.id)
        .bind(&row.login_value)
        .bind(&row.login_key)
        .bind(&row.status)
        .bind(row.is_instance_admin)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub async fn find_user(&self, id: synveil_core::UserId) -> Result<Option<User>, MetadataError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, login_value, login_key, status, is_instance_admin,
                    created_at, updated_at, revision::TEXT AS revision
             FROM users WHERE id = $1",
        )
        .bind(id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        row.map(UserRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn insert_device(&self, value: &Device) -> Result<(), MetadataError> {
        self.insert_device_row(&DeviceRow::from_domain(value)?)
            .await
    }

    pub async fn insert_device_row(&self, row: &DeviceRow) -> Result<(), MetadataError> {
        sqlx::query(
            "INSERT INTO devices
                (id, owner_user_id, display_name, status, created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7::NUMERIC)",
        )
        .bind(row.id)
        .bind(row.owner_user_id)
        .bind(&row.display_name)
        .bind(&row.status)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub async fn find_device(
        &self,
        id: synveil_core::DeviceId,
    ) -> Result<Option<Device>, MetadataError> {
        let row = sqlx::query_as::<_, DeviceRow>(
            "SELECT id, owner_user_id, display_name, status, created_at, updated_at,
                    revision::TEXT AS revision
             FROM devices WHERE id = $1",
        )
        .bind(id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        row.map(DeviceRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    /// Insert a library and its root in one short transaction. The root FK is
    /// deferred in the migration solely to make this domain-level pair
    /// insertable without weakening its final constraint.
    pub async fn insert_library_with_root(
        &self,
        library: &Library,
        root: &Node,
    ) -> Result<(), MetadataError> {
        library.validate_root(root)?;
        let library_row = LibraryRow::from_domain(library)?;
        let root_row = NodeRow::from_domain(root)?;
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        sqlx::query(
            "INSERT INTO libraries
                (id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                 created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::NUMERIC)",
        )
        .bind(library_row.id)
        .bind(library_row.owner_user_id)
        .bind(&library_row.name)
        .bind(library_row.root_node_id)
        .bind(library_row.dedup_domain_id)
        .bind(&library_row.status)
        .bind(library_row.created_at)
        .bind(library_row.updated_at)
        .bind(&library_row.revision)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;

        Self::insert_node_row_in_transaction(&mut transaction, &root_row).await?;
        transaction.commit().await.map_err(MetadataError::from)
    }

    pub async fn find_library(
        &self,
        id: synveil_core::LibraryId,
    ) -> Result<Option<Library>, MetadataError> {
        self.find_library_by_uuid(id.into_uuid()).await
    }

    pub(crate) async fn list_owned_libraries(
        &self,
        user_id: UserId,
        after: Option<LibraryId>,
        limit: u32,
    ) -> Result<PagedLibraries, MetadataError> {
        let rows = sqlx::query_as::<_, LibraryRow>(
            "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                    created_at, updated_at, revision::TEXT AS revision
             FROM libraries
             WHERE owner_user_id = $1
               AND status <> 'DELETING'
               AND ($2 IS NULL OR id > $2)
             ORDER BY id ASC
             LIMIT $3",
        )
        .bind(user_id.into_uuid())
        .bind(after.map(LibraryId::into_uuid))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let has_more = rows.len() > limit as usize;
        let rows = rows.into_iter().take(limit as usize);
        let mut libraries = Vec::new();
        for row in rows {
            let root = self
                .find_node_row(row.root_node_id)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "libraries.root_node_id",
                }))?
                .try_into_domain()?;
            libraries.push(row.try_into_domain(&root)?);
        }

        Ok(PagedLibraries {
            libraries,
            has_more,
        })
    }

    pub(crate) async fn resolve_owned_parent(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
    ) -> Result<Option<NodeId>, MetadataError> {
        let Some(library) = self.find_owned_library(user_id, library_id).await? else {
            return Ok(None);
        };
        if library.status() == LibraryStatus::Deleting {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::LibraryNotWritable,
            )));
        }
        let parent_node_id = parent_node_id.unwrap_or(library.root_node_id());
        let Some(parent) = self
            .find_owned_node_in_library(user_id, library_id, parent_node_id)
            .await?
        else {
            return Ok(None);
        };
        if parent.kind() != NodeKind::Directory || parent.state() != NodeState::Active {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::ParentMustBeActive,
            )));
        }
        Ok(Some(parent_node_id))
    }

    pub(crate) async fn list_owned_children(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: NodeId,
        after: Option<NodeId>,
        limit: u32,
    ) -> Result<PagedNodes, MetadataError> {
        let rows = sqlx::query_as::<_, NodeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.trashed_at, n.created_at, n.updated_at,
                    n.revision::TEXT AS revision
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE l.owner_user_id = $1
               AND n.library_id = $2
               AND n.parent_node_id = $3
               AND n.state = 'ACTIVE'
               AND ($4 IS NULL OR n.id > $4)
             ORDER BY n.id ASC
             LIMIT $5",
        )
        .bind(user_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(parent_node_id.into_uuid())
        .bind(after.map(NodeId::into_uuid))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let has_more = rows.len() > limit as usize;
        let nodes = rows
            .into_iter()
            .take(limit as usize)
            .map(NodeRow::try_into_domain)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PagedNodes { nodes, has_more })
    }

    pub(crate) async fn list_purge_candidates(
        &self,
        cutoff: Timestamp,
        after: Option<PurgeCursor>,
        limit: u32,
    ) -> Result<PagedPurgeCandidates, MetadataError> {
        let rows = sqlx::query_as::<_, PurgeCandidateRow>(
            "SELECT n.id AS node_id, n.library_id, l.owner_user_id, n.kind,
                    n.trashed_at, n.revision::TEXT AS revision
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE l.status = 'ACTIVE'
               AND n.state = 'TRASHED'
               AND n.id <> l.root_node_id
               AND n.parent_node_id IS NOT NULL
               AND n.trashed_at IS NOT NULL
               AND n.trashed_at <= $1
               AND EXISTS (
                    SELECT 1
                    FROM nodes AS parent
                    WHERE parent.library_id = n.library_id
                      AND parent.id = n.parent_node_id
                      AND parent.state = 'ACTIVE'
               )
               AND (
                    $2::TIMESTAMPTZ IS NULL
                    OR n.trashed_at > $2
                    OR (n.trashed_at = $2 AND n.id > $3)
               )
               AND NOT EXISTS (
                    SELECT 1
                    FROM nodes AS child
                    WHERE child.library_id = n.library_id
                      AND child.parent_node_id = n.id
               )
               AND NOT EXISTS (
                    SELECT 1
                    FROM upload_sessions AS upload
                    WHERE upload.library_id = n.library_id
                      AND upload.target_parent_node_id = n.id
               )
             ORDER BY n.trashed_at ASC, n.id ASC
             LIMIT $4",
        )
        .bind(cutoff.as_offset_datetime())
        .bind(after.map(|cursor| cursor.trashed_at.as_offset_datetime()))
        .bind(after.map(|cursor| cursor.node_id.into_uuid()))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let has_more = rows.len() > limit as usize;
        let candidates = rows
            .into_iter()
            .take(limit as usize)
            .map(map_purge_candidate_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PagedPurgeCandidates {
            candidates,
            has_more,
        })
    }

    pub(crate) async fn list_owned_file_versions(
        &self,
        user_id: UserId,
        node_id: NodeId,
        after: Option<VersionCursor>,
        limit: u32,
    ) -> Result<Option<VersionHistoryPage>, MetadataError> {
        let Some(scope) = self.load_owned_version_scope(user_id, node_id).await? else {
            return Ok(None);
        };
        if after.is_some_and(|cursor| cursor.node_id != node_id) {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "version_history.cursor.node_id",
            }));
        }

        let rows = sqlx::query_as::<_, VersionMetadataRow>(
            "SELECT fv.id, fv.library_id, fv.node_id, fv.object_id,
                    fv.object_dedup_domain_id, o.dedup_domain_id AS object_actual_dedup_domain_id,
                    fv.parent_version_id, fv.committed_at,
                    fv.revision::TEXT AS revision, o.canonical_hash,
                    o.plaintext_length::TEXT AS plaintext_length,
                    l.dedup_domain_id AS library_dedup_domain_id
             FROM file_versions AS fv
             INNER JOIN nodes AS n
                ON n.id = fv.node_id AND n.library_id = fv.library_id
             INNER JOIN libraries AS l ON l.id = fv.library_id
             INNER JOIN objects AS o ON o.id = fv.object_id
             WHERE fv.node_id = $1
               AND fv.library_id = $2
               AND l.owner_user_id = $3
               AND (
                    $4::TIMESTAMPTZ IS NULL
                    OR fv.committed_at < $4
                    OR (fv.committed_at = $4 AND fv.id < $5)
               )
             ORDER BY fv.committed_at DESC, fv.id DESC
             LIMIT $6",
        )
        .bind(node_id.into_uuid())
        .bind(scope.node.library_id().into_uuid())
        .bind(user_id.into_uuid())
        .bind(after.map(|cursor| cursor.committed_at.as_offset_datetime()))
        .bind(after.map(|cursor| cursor.version_id.into_uuid()))
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let has_more = rows.len() > limit as usize;
        let versions = rows
            .into_iter()
            .take(limit as usize)
            .map(|row| map_version_row(row, &scope))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(VersionHistoryPage {
            node: scope.node,
            current_version_id: scope.current_version_id,
            versions,
            has_more,
        }))
    }

    pub(crate) async fn find_owned_file_version(
        &self,
        user_id: UserId,
        version_id: FileVersionId,
    ) -> Result<Option<OwnedVersionRecord>, MetadataError> {
        let node_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT fv.node_id
             FROM file_versions AS fv
             INNER JOIN libraries AS l ON l.id = fv.library_id
             WHERE fv.id = $1 AND l.owner_user_id = $2",
        )
        .bind(version_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        let Some(node_id) = node_id else {
            return Ok(None);
        };
        let node_id = NodeId::try_from_uuid(node_id).map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "file_versions.node_id",
                reason,
            })
        })?;
        let Some(scope) = self.load_owned_version_scope(user_id, node_id).await? else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_versions.node_id",
            }));
        };

        let row = sqlx::query_as::<_, VersionMetadataRow>(
            "SELECT fv.id, fv.library_id, fv.node_id, fv.object_id,
                    fv.object_dedup_domain_id, o.dedup_domain_id AS object_actual_dedup_domain_id,
                    fv.parent_version_id, fv.committed_at,
                    fv.revision::TEXT AS revision, o.canonical_hash,
                    o.plaintext_length::TEXT AS plaintext_length,
                    l.dedup_domain_id AS library_dedup_domain_id
             FROM file_versions AS fv
             INNER JOIN libraries AS l ON l.id = fv.library_id
             INNER JOIN objects AS o ON o.id = fv.object_id
             WHERE fv.id = $1
               AND fv.node_id = $2
               AND fv.library_id = $3
               AND l.owner_user_id = $4",
        )
        .bind(version_id.into_uuid())
        .bind(node_id.into_uuid())
        .bind(scope.node.library_id().into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let record = map_version_row(row, &scope)?;
        Ok(Some(OwnedVersionRecord {
            node: scope.node,
            current_version_id: scope.current_version_id,
            record,
        }))
    }

    /// Append one immutable version that reuses an already verified
    /// historical Object. The idempotency row is created before the mutable
    /// node precondition is checked so a successful operation can be replayed
    /// even though the original If-Match revision is no longer current.
    pub(crate) async fn restore_file_version_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        source_version_id: FileVersionId,
        expected_revision: Revision,
        idempotency_key: &str,
        observed_at: synveil_core::Timestamp,
    ) -> Result<VersionRestoreMutation, MetadataError> {
        let fingerprint =
            restore_request_fingerprint(node_id, source_version_id, expected_revision);
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            return Ok(VersionRestoreMutation::NotFound);
        };

        let inserted = sqlx::query(
            "INSERT INTO file_version_restore_operations
                (owner_user_id, idempotency_key, request_fingerprint, library_id,
                 node_id, source_version_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (owner_user_id, idempotency_key) DO NOTHING",
        )
        .bind(user_id.into_uuid())
        .bind(idempotency_key)
        .bind(fingerprint.as_slice())
        .bind(library_id.into_uuid())
        .bind(node_id.into_uuid())
        .bind(source_version_id.into_uuid())
        .bind(observed_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?
        .rows_affected()
            == 1;

        if !inserted {
            let operation = sqlx::query_as::<_, RestoreOperationRow>(
                "SELECT library_id, node_id, source_version_id, request_fingerprint,
                        result_version_id,
                        result_node_revision::TEXT AS result_node_revision
                 FROM file_version_restore_operations
                 WHERE owner_user_id = $1 AND idempotency_key = $2
                 FOR UPDATE",
            )
            .bind(user_id.into_uuid())
            .bind(idempotency_key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(MetadataError::from)?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_version_restore_operations.idempotency_key",
            }))?;

            if operation.library_id != library_id.into_uuid()
                || operation.node_id != node_id.into_uuid()
                || operation.source_version_id != source_version_id.into_uuid()
                || operation.request_fingerprint.as_slice() != fingerprint.as_slice()
            {
                transaction.commit().await.map_err(MetadataError::from)?;
                return Ok(VersionRestoreMutation::IdempotencyConflict);
            }

            let result_version_id = operation.result_version_id.ok_or(MetadataError::Mapping(
                MappingError::RelationMismatch {
                    relation: "file_version_restore_operations.result",
                },
            ))?;
            let result_node_revision =
                operation
                    .result_node_revision
                    .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                        relation: "file_version_restore_operations.result_revision",
                    }))?;
            let result_version_id =
                FileVersionId::try_from_uuid(result_version_id).map_err(|reason| {
                    MetadataError::Mapping(MappingError::InvalidId {
                        field: "file_version_restore_operations.result_version_id",
                        reason,
                    })
                })?;
            let node_revision = Revision::from_str(&result_node_revision).map_err(|_| {
                MetadataError::Mapping(MappingError::InvalidDecimal {
                    field: "file_version_restore_operations.result_node_revision",
                })
            })?;
            let library = Self::lock_owned_library(&mut transaction, user_id, library_id)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "file_version_restore_operations.library",
                }))?;
            let node = Self::lock_node_in_library(&mut transaction, library_id, node_id)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "file_version_restore_operations.node",
                }))?;
            let scope = OwnedVersionScope {
                current_version_id: node.current_version_id(),
                node,
                dedup_domain_id: library.dedup_domain_id(),
            };
            let record = load_version_record_for_scope(&mut transaction, result_version_id, &scope)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "file_version_restore_operations.result_version",
                }))?;
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(VersionRestoreMutation::Replayed {
                record,
                node_revision,
                current_version_id: scope.current_version_id,
            });
        }

        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(VersionRestoreMutation::NotFound);
        };
        if library.status() != LibraryStatus::Active {
            return Ok(VersionRestoreMutation::InvalidState);
        }
        let Some(node) = Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(VersionRestoreMutation::NotFound);
        };
        if node.kind() != NodeKind::File {
            return Ok(VersionRestoreMutation::InvalidState);
        }
        if node.state() != NodeState::Active {
            return Ok(VersionRestoreMutation::NotFound);
        }
        if node.revision() != expected_revision {
            return Ok(VersionRestoreMutation::VersionConflict {
                current_revision: node.revision(),
            });
        }
        if node.current_version_id() == Some(source_version_id) {
            return Ok(VersionRestoreMutation::InvalidRequest);
        }

        let Some(source_row) = load_restore_source_for_update(
            &mut transaction,
            library_id,
            node_id,
            source_version_id,
        )
        .await?
        else {
            return Ok(VersionRestoreMutation::NotFound);
        };
        let scope = OwnedVersionScope {
            node: node.clone(),
            dedup_domain_id: library.dedup_domain_id(),
            current_version_id: node.current_version_id(),
        };
        let _source_record = map_version_row(source_row.clone(), &scope)?;
        let object = object_reference_from_version_row(&source_row)?;
        if !has_usable_verified_replica(&mut transaction, object).await? {
            return Ok(VersionRestoreMutation::ContentUnavailable);
        }

        let version = FileVersion::new(
            FileVersionId::new(),
            &library,
            &node,
            object,
            node.current_version_id(),
            observed_at,
        )?;
        let next_node = node.with_current_version(&version, observed_at)?;
        Self::insert_file_version_in_transaction(&mut transaction, version).await?;
        Self::update_node_in_transaction(&mut transaction, &next_node).await?;
        let updated = sqlx::query(
            "UPDATE file_version_restore_operations
             SET result_version_id = $3, result_node_revision = $4::NUMERIC
             WHERE owner_user_id = $1 AND idempotency_key = $2
               AND result_version_id IS NULL",
        )
        .bind(user_id.into_uuid())
        .bind(idempotency_key)
        .bind(version.id().into_uuid())
        .bind(next_node.revision().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?
        .rows_affected();
        if updated != 1 {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_version_restore_operations.result_update",
            }));
        }
        transaction.commit().await.map_err(MetadataError::from)?;

        Ok(VersionRestoreMutation::Applied {
            record: VersionRecord {
                id: version.id(),
                node_id: version.node_id(),
                committed_at: version.committed_at(),
                byte_length: version.plaintext_length(),
                sha256: version.canonical_hash(),
            },
            node_revision: next_node.revision(),
        })
    }

    pub(crate) async fn find_owned_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<Node>, MetadataError> {
        let row = sqlx::query_as::<_, NodeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.trashed_at, n.created_at, n.updated_at,
                    n.revision::TEXT AS revision
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE n.id = $1 AND l.owner_user_id = $2",
        )
        .bind(node_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        row.map(NodeRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    pub(crate) async fn create_directory_owned(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
        observed_at: Timestamp,
    ) -> Result<Option<Node>, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(None);
        };
        ensure_library_writable(&library)?;
        let parent_node_id = parent_node_id.unwrap_or(library.root_node_id());
        let Some(parent) =
            Self::lock_node_in_library(&mut transaction, library_id, parent_node_id).await?
        else {
            return Ok(None);
        };
        let node = Node::new_child(
            NodeId::new(),
            library_id,
            &parent,
            NodeKind::Directory,
            name,
            observed_at,
        )?;
        let row = NodeRow::from_domain(&node)?;
        Self::insert_node_row_in_transaction(&mut transaction, &row).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(Some(node))
    }

    pub(crate) async fn rename_node_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        name: LogicalName,
        expected_revision: Revision,
        observed_at: Timestamp,
    ) -> Result<NodeMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        let Some(mut node) =
            Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        if node.revision() != expected_revision {
            return Ok(NodeMutation::VersionConflict(node));
        }
        node.rename(name, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(NodeMutation::Applied(node))
    }

    pub(crate) async fn move_node_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        destination_parent_id: NodeId,
        expected_revision: Revision,
        observed_at: Timestamp,
    ) -> Result<NodeMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        let Some(mut node) =
            Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        if node.revision() != expected_revision {
            return Ok(NodeMutation::VersionConflict(node));
        }
        if node.is_root() {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::RootCannotHaveParent,
            )));
        }

        let Some(destination) =
            Self::load_owned_node(&mut transaction, user_id, destination_parent_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        if destination.library_id() != library_id {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::ParentLibraryMismatch,
            )));
        }
        let Some(destination) =
            Self::lock_node_in_library(&mut transaction, library_id, destination_parent_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        let ancestors = load_ancestors(&mut transaction, &destination).await?;
        node.validate_move_parent(&destination, &ancestors)?;
        node.move_to(&destination, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(NodeMutation::Applied(node))
    }

    pub(crate) async fn delete_node_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
        observed_at: Timestamp,
    ) -> Result<NodeMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        let Some(mut node) =
            Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        if node.revision() != expected_revision {
            return Ok(NodeMutation::VersionConflict(node));
        }
        if node.kind() == NodeKind::Directory {
            let has_children: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM nodes WHERE library_id = $1 AND parent_node_id = $2
                )",
            )
            .bind(library_id.into_uuid())
            .bind(node.id().into_uuid())
            .fetch_one(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
            if has_children {
                return Err(MetadataError::Mapping(MappingError::Domain(
                    DomainError::DirectoryNotEmpty,
                )));
            }
        }
        node.transition_state(NodeState::Trashed, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(NodeMutation::Applied(node))
    }

    pub(crate) async fn restore_node_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
        observed_at: Timestamp,
    ) -> Result<NodeMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        let Some(mut node) =
            Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        if node.revision() != expected_revision {
            return Ok(NodeMutation::VersionConflict(node));
        }
        let Some(parent_id) = node.parent_node_id() else {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::RootCannotBeDeleted,
            )));
        };
        let Some(parent) =
            Self::lock_node_in_library(&mut transaction, library_id, parent_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::ParentChainMismatch,
            )));
        };
        node.validate_parent_relationship(&parent)?;
        node.transition_state(NodeState::Active, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(NodeMutation::Applied(node))
    }

    pub(crate) async fn begin_node_purge_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
        cutoff: Timestamp,
        observed_at: Timestamp,
    ) -> Result<PurgeMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            return Ok(PurgeMutation::NotFound);
        };
        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(PurgeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        let Some(mut node) =
            Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(PurgeMutation::NotFound);
        };
        if node.revision() != expected_revision {
            return Ok(PurgeMutation::VersionConflict(node));
        }
        if node.state() == NodeState::Purging {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(PurgeMutation::Applied(node));
        }
        if node.state() != NodeState::Trashed
            || node.is_root()
            || node.id() == library.root_node_id()
            || !node
                .trashed_at()
                .is_some_and(|trashed_at| trashed_at <= cutoff)
        {
            return Ok(PurgeMutation::NotEligible);
        }
        let Some(parent_id) = node.parent_node_id() else {
            return Ok(PurgeMutation::NotEligible);
        };
        let Some(parent) =
            Self::lock_node_in_library(&mut transaction, library_id, parent_id).await?
        else {
            return Ok(PurgeMutation::NotEligible);
        };
        if parent.state() != NodeState::Active {
            return Ok(PurgeMutation::NotEligible);
        }
        let has_children: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM nodes WHERE library_id = $1 AND parent_node_id = $2
            )",
        )
        .bind(library_id.into_uuid())
        .bind(node.id().into_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if has_children {
            return Ok(PurgeMutation::NotEligible);
        }
        if Self::has_upload_parent_references(&mut transaction, library_id, node.id()).await? {
            return Ok(PurgeMutation::NotEligible);
        }
        node.transition_state(NodeState::Purging, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(PurgeMutation::Applied(node))
    }

    /// Permanently removes one already-`PURGING` node's metadata in one
    /// PostgreSQL transaction. The operation deliberately has no ObjectStore
    /// dependency: object and replica rows survive, while only the logical
    /// FileVersion references are released and zero-reference candidates are
    /// recorded for a later GC phase.
    pub(crate) async fn execute_node_purge_owned(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
        observed_at: Timestamp,
    ) -> Result<PurgeExecutionMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        let Some(library_id) =
            Self::owned_node_library_id(&mut transaction, user_id, node_id).await?
        else {
            let completed_revision =
                Self::completed_purge_revision(&mut transaction, user_id, node_id).await?;
            let mutation = match completed_revision {
                None => PurgeExecutionMutation::NotFound,
                Some(current_revision) if current_revision == expected_revision => {
                    PurgeExecutionMutation::AlreadyCompleted
                }
                Some(current_revision) => {
                    PurgeExecutionMutation::VersionConflict { current_revision }
                }
            };
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(mutation);
        };

        let Some(library) = Self::lock_owned_library(&mut transaction, user_id, library_id).await?
        else {
            return Ok(PurgeExecutionMutation::NotFound);
        };
        ensure_library_writable(&library)?;

        let Some(node) = Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            let completed_revision =
                Self::completed_purge_revision(&mut transaction, user_id, node_id).await?;
            let mutation = match completed_revision {
                None => PurgeExecutionMutation::NotFound,
                Some(current_revision) if current_revision == expected_revision => {
                    PurgeExecutionMutation::AlreadyCompleted
                }
                Some(current_revision) => {
                    PurgeExecutionMutation::VersionConflict { current_revision }
                }
            };
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(mutation);
        };

        if node.revision() != expected_revision {
            return Ok(PurgeExecutionMutation::VersionConflict {
                current_revision: node.revision(),
            });
        }
        if node.state() != NodeState::Purging {
            return Ok(PurgeExecutionMutation::InvalidState);
        }
        if node.is_root() || node.id() == library.root_node_id() {
            return Ok(PurgeExecutionMutation::NotEligible);
        }

        let Some(parent_id) = node.parent_node_id() else {
            return Ok(PurgeExecutionMutation::NotEligible);
        };
        let Some(parent) =
            Self::lock_node_in_library(&mut transaction, library_id, parent_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "nodes.parent_node_id",
            }));
        };
        if parent.state() != NodeState::Active {
            return Ok(PurgeExecutionMutation::NotEligible);
        }
        node.validate_parent_relationship(&parent)?;

        let has_children: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM nodes WHERE library_id = $1 AND parent_node_id = $2
            )",
        )
        .bind(library_id.into_uuid())
        .bind(node.id().into_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if has_children {
            return Ok(PurgeExecutionMutation::NotEligible);
        }
        if Self::has_upload_parent_references(&mut transaction, library_id, node.id()).await? {
            return Ok(PurgeExecutionMutation::NotEligible);
        }

        let has_external_version_children: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM file_versions AS child
                WHERE child.node_id <> $1
                  AND child.parent_version_id IN (
                      SELECT parent.id
                      FROM file_versions AS parent
                      WHERE parent.node_id = $1 AND parent.library_id = $2
                  )
            )",
        )
        .bind(node.id().into_uuid())
        .bind(library_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if has_external_version_children {
            return Ok(PurgeExecutionMutation::NotEligible);
        }

        if let Some(current_version_id) = node.current_version_id() {
            let current_version_matches: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM file_versions
                    WHERE id = $1 AND library_id = $2 AND node_id = $3
                )",
            )
            .bind(current_version_id.into_uuid())
            .bind(library_id.into_uuid())
            .bind(node.id().into_uuid())
            .fetch_one(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
            if !current_version_matches {
                return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "nodes.current_version_id",
                }));
            }
        }

        // Serialize reference creation against this release set, including
        // references from another library in the same deduplication domain.
        // The advisory key is derived only from canonical object identity; a
        // collision can reduce concurrency but cannot make accounting unsafe.
        Self::lock_object_references_for_purge(&mut transaction, node.id(), library_id).await?;

        // The node head FK must be cleared before its FileVersion rows are
        // removed. This is deliberately not exposed as a mutable domain
        // transition: the node is about to be deleted in this same transaction.
        let cleared_head = sqlx::query(
            "UPDATE nodes
             SET current_version_id = NULL
             WHERE id = $1 AND library_id = $2 AND state = 'PURGING'
               AND revision = $3::NUMERIC",
        )
        .bind(node.id().into_uuid())
        .bind(library_id.into_uuid())
        .bind(expected_revision.get().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if cleared_head.rows_affected() != 1 {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "nodes.purge_head_update",
            }));
        }

        // Restore idempotency rows refer to both the node and its result
        // version. They are operation metadata, not retained history, and
        // must be removed before either FK target is deleted.
        sqlx::query(
            "DELETE FROM file_version_restore_operations
             WHERE node_id = $1 AND library_id = $2",
        )
        .bind(node.id().into_uuid())
        .bind(library_id.into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;

        // `file_versions_parent_fk` is deferred by the Prompt 26 migration so
        // a large immutable history can be removed set-wise without loading
        // every version into application memory. The reusable set-based
        // release operation uses the pre-statement snapshot and excludes the
        // node being deleted from the survivor relation.
        Self::release_purged_node_references(&mut transaction, node.id(), library_id, observed_at)
            .await?;

        let deleted_node = sqlx::query(
            "DELETE FROM nodes
             WHERE id = $1 AND library_id = $2 AND state = 'PURGING'
               AND revision = $3::NUMERIC",
        )
        .bind(node.id().into_uuid())
        .bind(library_id.into_uuid())
        .bind(expected_revision.get().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if deleted_node.rows_affected() != 1 {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "nodes.purge_delete",
            }));
        }

        // This row intentionally has no FK to `nodes`: it is the minimal
        // replay identity that survives the permanent metadata deletion.
        sqlx::query(
            "INSERT INTO metadata_purge_operations
                (node_id, owner_user_id, library_id, purge_revision, completed_at)
             VALUES ($1, $2, $3, $4::NUMERIC, $5)",
        )
        .bind(node.id().into_uuid())
        .bind(user_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(expected_revision.get().to_string())
        .bind(observed_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;

        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(PurgeExecutionMutation::Completed)
    }

    pub async fn insert_node(&self, value: &Node) -> Result<(), MetadataError> {
        self.insert_node_row(&NodeRow::from_domain(value)?).await
    }

    pub async fn insert_node_row(&self, row: &NodeRow) -> Result<(), MetadataError> {
        sqlx::query(
            "INSERT INTO nodes
                (id, library_id, parent_node_id, kind, name, current_version_id,
                 state, trashed_at, created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::NUMERIC)",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
        .bind(row.trashed_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub async fn update_node(&self, value: &Node) -> Result<(), MetadataError> {
        let row = NodeRow::from_domain(value)?;
        sqlx::query(
            "UPDATE nodes
             SET library_id = $2, parent_node_id = $3, kind = $4, name = $5,
                 current_version_id = $6, state = $7, trashed_at = $8,
                 created_at = $9, updated_at = $10, revision = $11::NUMERIC
             WHERE id = $1",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
        .bind(row.trashed_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub async fn find_node(&self, id: synveil_core::NodeId) -> Result<Option<Node>, MetadataError> {
        let row = self.find_node_row(id.into_uuid()).await?;
        row.map(NodeRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn insert_object(
        &self,
        value: ObjectReference,
        created_at: synveil_core::Timestamp,
    ) -> Result<(), MetadataError> {
        self.insert_object_row(&ObjectRow::from_reference(value, created_at)?)
            .await
    }

    pub async fn insert_object_row(&self, row: &ObjectRow) -> Result<(), MetadataError> {
        sqlx::query(
            "INSERT INTO objects
                (id, dedup_domain_id, canonical_hash, plaintext_length, created_at)
             VALUES ($1, $2, $3, $4::NUMERIC, $5)",
        )
        .bind(row.id)
        .bind(row.dedup_domain_id)
        .bind(&row.canonical_hash)
        .bind(&row.plaintext_length)
        .bind(row.created_at)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub async fn find_object(
        &self,
        id: synveil_core::ObjectId,
    ) -> Result<Option<ObjectReference>, MetadataError> {
        let row = self.find_object_row(id.into_uuid()).await?;
        row.map(|row| row.try_into_reference())
            .transpose()
            .map_err(Into::into)
    }

    pub async fn insert_file_version(&self, value: FileVersion) -> Result<(), MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        // Purge locks the target Node before the canonical Object. Acquire
        // the same row lock here before `insert_file_version_in_transaction`
        // takes the Object advisory lock, so low-level repository writers do
        // not form a node/object lock-order cycle with purge execution.
        if Self::lock_node_in_library(&mut transaction, value.library_id(), value.node_id())
            .await?
            .is_none()
        {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_versions.node_id",
            }));
        }
        Self::insert_file_version_in_transaction(&mut transaction, value).await?;
        transaction.commit().await.map_err(MetadataError::from)
    }

    pub async fn find_file_version(
        &self,
        id: synveil_core::FileVersionId,
    ) -> Result<Option<FileVersion>, MetadataError> {
        let row = sqlx::query_as::<_, FileVersionRow>(
            "SELECT id, library_id, node_id, object_id, object_dedup_domain_id,
                    parent_version_id, committed_at, revision::TEXT AS revision
             FROM file_versions WHERE id = $1",
        )
        .bind(id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let Some(row) = row else {
            return Ok(None);
        };
        let library =
            self.find_library_by_uuid(row.library_id)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "file_versions.library_id",
                }))?;
        let node = self
            .find_node_row(row.node_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_versions.node_id",
            }))?
            .try_into_domain()?;
        let object = self
            .find_object_row(row.object_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_versions.object_id",
            }))?;

        row.try_into_domain(&library, &node, &object)
            .map(Some)
            .map_err(Into::into)
    }

    async fn find_node_row(&self, id: Uuid) -> Result<Option<NodeRow>, MetadataError> {
        sqlx::query_as::<_, NodeRow>(
            "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                    state, trashed_at, created_at, updated_at, revision::TEXT AS revision
             FROM nodes WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
    }

    async fn find_owned_library(
        &self,
        user_id: UserId,
        id: LibraryId,
    ) -> Result<Option<Library>, MetadataError> {
        let row = sqlx::query_as::<_, LibraryRow>(
            "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                    created_at, updated_at, revision::TEXT AS revision
             FROM libraries
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let Some(row) = row else {
            return Ok(None);
        };
        let root = self
            .find_node_row(row.root_node_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "libraries.root_node_id",
            }))?
            .try_into_domain()?;
        row.try_into_domain(&root).map(Some).map_err(Into::into)
    }

    async fn find_owned_node_in_library(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<Option<Node>, MetadataError> {
        let row = sqlx::query_as::<_, NodeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.trashed_at, n.created_at, n.updated_at,
                    n.revision::TEXT AS revision
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE n.id = $1 AND n.library_id = $2 AND l.owner_user_id = $3",
        )
        .bind(node_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        row.map(NodeRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    async fn find_object_row(&self, id: Uuid) -> Result<Option<ObjectRow>, MetadataError> {
        sqlx::query_as::<_, ObjectRow>(
            "SELECT id, dedup_domain_id, canonical_hash,
                    plaintext_length::TEXT AS plaintext_length, created_at
             FROM objects WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
    }

    async fn find_library_by_uuid(&self, id: Uuid) -> Result<Option<Library>, MetadataError> {
        let row = sqlx::query_as::<_, LibraryRow>(
            "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                    created_at, updated_at, revision::TEXT AS revision
             FROM libraries WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        let Some(row) = row else {
            return Ok(None);
        };
        let root = self
            .find_node_row(row.root_node_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "libraries.root_node_id",
            }))?
            .try_into_domain()?;
        row.try_into_domain(&root).map(Some).map_err(Into::into)
    }

    async fn insert_node_row_in_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        row: &NodeRow,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "INSERT INTO nodes
                (id, library_id, parent_node_id, kind, name, current_version_id,
                 state, trashed_at, created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::NUMERIC)",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
        .bind(row.trashed_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    async fn update_node_in_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        value: &Node,
    ) -> Result<(), MetadataError> {
        let row = NodeRow::from_domain(value)?;
        sqlx::query(
            "UPDATE nodes
             SET library_id = $2, parent_node_id = $3, kind = $4, name = $5,
                 current_version_id = $6, state = $7, trashed_at = $8,
                 created_at = $9, updated_at = $10, revision = $11::NUMERIC
             WHERE id = $1",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
        .bind(row.trashed_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub(crate) async fn clear_object_gc_candidate_in_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        object_id: ObjectId,
        object_dedup_domain_id: DedupDomainId,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "SELECT pg_advisory_xact_lock(
                hashtextextended($1::TEXT || ':' || $2::TEXT, 0)
             )",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        sqlx::query(
            "DELETE FROM object_gc_candidates
             WHERE object_id = $1 AND object_dedup_domain_id = $2",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    async fn lock_object_references_for_purge(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        node_id: NodeId,
        library_id: LibraryId,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "SELECT pg_advisory_xact_lock(
                hashtextextended(o.id::TEXT || ':' || o.dedup_domain_id::TEXT, 0)
             )
             FROM objects AS o
             WHERE EXISTS (
                 SELECT 1
                 FROM file_versions AS version
                 WHERE version.node_id = $1
                   AND version.library_id = $2
                   AND version.object_id = o.id
                   AND version.object_dedup_domain_id = o.dedup_domain_id
             )
             ORDER BY o.id, o.dedup_domain_id
             FOR UPDATE OF o",
        )
        .bind(node_id.into_uuid())
        .bind(library_id.into_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    /// Release one node's logical FileVersion references and record only the
    /// canonical Objects whose last committed reference disappeared.
    ///
    /// The `NOT EXISTS` relation is the reference-accounting authority. The
    /// data-modifying CTE sees the statement snapshot, so the survivor query
    /// intentionally excludes the node being deleted and still sees every
    /// committed reference from other nodes/libraries. No Object or replica
    /// row is mutated here.
    async fn release_purged_node_references(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        node_id: NodeId,
        library_id: LibraryId,
        unreferenced_at: Timestamp,
    ) -> Result<(), MetadataError> {
        // A candidate is only valid while no committed FileVersion references
        // the object. Clear stale candidate metadata before the set-based
        // release below re-adds only last-reference Objects.
        sqlx::query(
            "DELETE FROM object_gc_candidates AS candidate
             USING file_versions AS version
             WHERE version.node_id = $1
               AND version.library_id = $2
               AND candidate.object_id = version.object_id
               AND candidate.object_dedup_domain_id = version.object_dedup_domain_id",
        )
        .bind(node_id.into_uuid())
        .bind(library_id.into_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;

        sqlx::query(
            "WITH deleted_versions AS (
                 DELETE FROM file_versions
                 WHERE node_id = $1 AND library_id = $2
                 RETURNING object_id, object_dedup_domain_id
             )
             INSERT INTO object_gc_candidates
                 (object_id, object_dedup_domain_id, unreferenced_at, source)
             SELECT DISTINCT deleted.object_id, deleted.object_dedup_domain_id,
                    $3, 'METADATA_PURGE'
             FROM deleted_versions AS deleted
             WHERE NOT EXISTS (
                 SELECT 1
                 FROM file_versions AS survivor
                 WHERE survivor.object_id = deleted.object_id
                   AND survivor.object_dedup_domain_id = deleted.object_dedup_domain_id
                   AND survivor.node_id <> $1
             )
             ON CONFLICT (object_id, object_dedup_domain_id) DO NOTHING",
        )
        .bind(node_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(unreferenced_at.as_offset_datetime())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    async fn insert_file_version_in_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        value: FileVersion,
    ) -> Result<(), MetadataError> {
        let object = value.object_reference();
        let row = FileVersionRow::from_domain(value)?;
        Self::clear_object_gc_candidate_in_transaction(
            transaction,
            object.object_id(),
            object.dedup_domain_id(),
        )
        .await?;
        sqlx::query(
            "INSERT INTO file_versions
                (id, library_id, node_id, object_id, object_dedup_domain_id,
                 parent_version_id, committed_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::NUMERIC)",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.node_id)
        .bind(row.object_id)
        .bind(row.object_dedup_domain_id)
        .bind(row.parent_version_id)
        .bind(row.committed_at)
        .bind(&row.revision)
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    async fn completed_purge_revision(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<Revision>, MetadataError> {
        let revision = sqlx::query_scalar::<_, String>(
            "SELECT purge_revision::TEXT
             FROM metadata_purge_operations
             WHERE node_id = $1 AND owner_user_id = $2",
        )
        .bind(node_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        revision
            .map(|revision| {
                Revision::from_str(&revision).map_err(|_| {
                    MetadataError::Mapping(MappingError::InvalidDecimal {
                        field: "metadata_purge_operations.purge_revision",
                    })
                })
            })
            .transpose()
    }

    async fn owned_node_library_id(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<LibraryId>, MetadataError> {
        let id = sqlx::query_scalar::<_, Uuid>(
            "SELECT n.library_id
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE n.id = $1 AND l.owner_user_id = $2",
        )
        .bind(node_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        id.map(LibraryId::try_from_uuid)
            .transpose()
            .map_err(|reason| MappingError::InvalidId {
                field: "nodes.library_id",
                reason,
            })
            .map_err(Into::into)
    }

    async fn lock_owned_library(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: UserId,
        library_id: LibraryId,
    ) -> Result<Option<Library>, MetadataError> {
        let row = sqlx::query_as::<_, LibraryRow>(
            "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                    created_at, updated_at, revision::TEXT AS revision
             FROM libraries
             WHERE id = $1 AND owner_user_id = $2
             FOR UPDATE",
        )
        .bind(library_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let root = sqlx::query_as::<_, NodeRow>(
            "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                    state, trashed_at, created_at, updated_at, revision::TEXT AS revision
             FROM nodes
             WHERE id = $1 AND library_id = $2
             FOR UPDATE",
        )
        .bind(row.root_node_id)
        .bind(row.id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?
        .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
            relation: "libraries.root_node_id",
        }))?
        .try_into_domain()?;
        row.try_into_domain(&root).map(Some).map_err(Into::into)
    }

    async fn lock_node_in_library(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<Option<Node>, MetadataError> {
        let row = sqlx::query_as::<_, NodeRow>(
            "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                    state, trashed_at, created_at, updated_at, revision::TEXT AS revision
             FROM nodes
             WHERE id = $1 AND library_id = $2
             FOR UPDATE",
        )
        .bind(node_id.into_uuid())
        .bind(library_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        row.map(NodeRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    async fn has_upload_parent_references(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<bool, MetadataError> {
        sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM upload_sessions
                WHERE library_id = $1 AND target_parent_node_id = $2
            )",
        )
        .bind(library_id.into_uuid())
        .bind(node_id.into_uuid())
        .fetch_one(&mut **transaction)
        .await
        .map_err(MetadataError::from)
    }

    async fn load_owned_node(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<Node>, MetadataError> {
        let row = sqlx::query_as::<_, NodeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.trashed_at, n.created_at, n.updated_at,
                    n.revision::TEXT AS revision
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             WHERE n.id = $1 AND l.owner_user_id = $2",
        )
        .bind(node_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        row.map(NodeRow::try_into_domain)
            .transpose()
            .map_err(Into::into)
    }

    async fn load_owned_version_scope(
        &self,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<OwnedVersionScope>, MetadataError> {
        let row = sqlx::query_as::<_, VersionScopeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.trashed_at, n.created_at, n.updated_at,
                    n.revision::TEXT AS revision, l.dedup_domain_id,
                    current_version.id AS current_version_row_id,
                    current_version.node_id AS current_version_node_id
             FROM nodes AS n
             INNER JOIN libraries AS l ON l.id = n.library_id
             LEFT JOIN file_versions AS current_version
                ON current_version.id = n.current_version_id
               AND current_version.library_id = n.library_id
             WHERE n.id = $1 AND l.owner_user_id = $2",
        )
        .bind(node_id.into_uuid())
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        let Some(row) = row else {
            return Ok(None);
        };

        let node = NodeRow {
            id: row.id,
            library_id: row.library_id,
            parent_node_id: row.parent_node_id,
            kind: row.kind,
            name: row.name,
            current_version_id: row.current_version_id,
            state: row.state,
            trashed_at: row.trashed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            revision: row.revision,
        }
        .try_into_domain()?;
        if node.id() != node_id {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "nodes.id",
            }));
        }
        let library_id = LibraryId::try_from_uuid(row.library_id).map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "nodes.library_id",
                reason,
            })
        })?;
        if node.library_id() != library_id {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "nodes.library_id",
            }));
        }
        let dedup_domain_id =
            DedupDomainId::try_from_uuid(row.dedup_domain_id).map_err(|reason| {
                MetadataError::Mapping(MappingError::InvalidId {
                    field: "libraries.dedup_domain_id",
                    reason,
                })
            })?;
        let current_version_id = node.current_version_id();
        match current_version_id {
            Some(current_version_id)
                if row.current_version_row_id != Some(current_version_id.into_uuid())
                    || row.current_version_node_id != Some(node.id().into_uuid()) =>
            {
                return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "nodes.current_version_id",
                }));
            }
            None if row.current_version_row_id.is_some()
                || row.current_version_node_id.is_some() =>
            {
                return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "nodes.current_version_id",
                }));
            }
            _ => {}
        }

        Ok(Some(OwnedVersionScope {
            node,
            dedup_domain_id,
            current_version_id,
        }))
    }
}

async fn load_ancestors(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    destination: &Node,
) -> Result<Vec<Node>, MetadataError> {
    let mut ancestors = Vec::new();
    let mut next = destination.parent_node_id();
    let mut seen = std::collections::HashSet::new();
    while let Some(node_id) = next {
        if ancestors.len() >= MAX_ANCESTOR_DEPTH || !seen.insert(node_id) {
            return Err(MetadataError::Mapping(MappingError::Domain(
                DomainError::ParentCycle,
            )));
        }
        let Some(node) =
            DomainRepository::lock_node_in_library(transaction, destination.library_id(), node_id)
                .await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "nodes.parent_node_id",
            }));
        };
        next = node.parent_node_id();
        ancestors.push(node);
    }
    Ok(ancestors)
}

fn map_purge_candidate_row(row: PurgeCandidateRow) -> Result<PurgeCandidateRecord, MetadataError> {
    let node_id = NodeId::try_from_uuid(row.node_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "nodes.id",
            reason,
        })
    })?;
    let library_id = LibraryId::try_from_uuid(row.library_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "nodes.library_id",
            reason,
        })
    })?;
    let owner_user_id = UserId::try_from_uuid(row.owner_user_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "libraries.owner_user_id",
            reason,
        })
    })?;
    let kind = match row.kind.as_str() {
        "FILE" => NodeKind::File,
        "DIRECTORY" => NodeKind::Directory,
        _ => {
            return Err(MetadataError::Mapping(MappingError::InvalidEnum {
                field: "nodes.kind",
            }));
        }
    };
    let revision = Revision::from_str(&row.revision).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "nodes.revision",
        })
    })?;
    Ok(PurgeCandidateRecord {
        node_id,
        library_id,
        owner_user_id,
        kind,
        trashed_at: Timestamp::from_offset_datetime(row.trashed_at),
        revision,
    })
}

fn ensure_library_writable(library: &Library) -> Result<(), MetadataError> {
    if library.status() != LibraryStatus::Active {
        return Err(MetadataError::Mapping(MappingError::Domain(
            DomainError::LibraryNotWritable,
        )));
    }
    Ok(())
}

async fn load_restore_source_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    node_id: NodeId,
    source_version_id: FileVersionId,
) -> Result<Option<VersionMetadataRow>, MetadataError> {
    sqlx::query_as::<_, VersionMetadataRow>(
        "SELECT fv.id, fv.library_id, fv.node_id, fv.object_id,
                fv.object_dedup_domain_id, o.dedup_domain_id AS object_actual_dedup_domain_id,
                fv.parent_version_id, fv.committed_at,
                fv.revision::TEXT AS revision, o.canonical_hash,
                o.plaintext_length::TEXT AS plaintext_length,
                l.dedup_domain_id AS library_dedup_domain_id
         FROM file_versions AS fv
         INNER JOIN nodes AS n
            ON n.id = fv.node_id AND n.library_id = fv.library_id
         INNER JOIN libraries AS l ON l.id = fv.library_id
         INNER JOIN objects AS o
            ON o.id = fv.object_id
           AND o.dedup_domain_id = fv.object_dedup_domain_id
         WHERE fv.id = $1 AND fv.node_id = $2 AND fv.library_id = $3
         FOR UPDATE OF fv, o",
    )
    .bind(source_version_id.into_uuid())
    .bind(node_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
}

async fn load_version_record_for_scope(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version_id: FileVersionId,
    scope: &OwnedVersionScope,
) -> Result<Option<VersionRecord>, MetadataError> {
    let row = sqlx::query_as::<_, VersionMetadataRow>(
        "SELECT fv.id, fv.library_id, fv.node_id, fv.object_id,
                fv.object_dedup_domain_id, o.dedup_domain_id AS object_actual_dedup_domain_id,
                fv.parent_version_id, fv.committed_at,
                fv.revision::TEXT AS revision, o.canonical_hash,
                o.plaintext_length::TEXT AS plaintext_length,
                l.dedup_domain_id AS library_dedup_domain_id
         FROM file_versions AS fv
         INNER JOIN libraries AS l ON l.id = fv.library_id
         INNER JOIN objects AS o
            ON o.id = fv.object_id
           AND o.dedup_domain_id = fv.object_dedup_domain_id
         WHERE fv.id = $1 AND fv.node_id = $2 AND fv.library_id = $3",
    )
    .bind(version_id.into_uuid())
    .bind(scope.node.id().into_uuid())
    .bind(scope.node.library_id().into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;
    row.map(|row| map_version_row(row, scope)).transpose()
}

fn object_reference_from_version_row(
    row: &VersionMetadataRow,
) -> Result<ObjectReference, MetadataError> {
    let object_id = ObjectId::try_from_uuid(row.object_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "file_versions.object_id",
            reason,
        })
    })?;
    let dedup_domain_id =
        DedupDomainId::try_from_uuid(row.object_dedup_domain_id).map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "file_versions.object_dedup_domain_id",
                reason,
            })
        })?;
    let canonical_hash = Sha256Digest::try_from(row.canonical_hash.as_slice()).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDigest {
            field: "objects.canonical_hash",
        })
    })?;
    let plaintext_length = row.plaintext_length.parse::<u64>().map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "objects.plaintext_length",
        })
    })?;
    Ok(ObjectReference::verified(
        object_id,
        dedup_domain_id,
        canonical_hash,
        plaintext_length,
    ))
}

async fn has_usable_verified_replica(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    object: ObjectReference,
) -> Result<bool, MetadataError> {
    let replica_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM object_replicas
         WHERE object_id = $1
           AND object_dedup_domain_id = $2
           AND state = 'VERIFIED'
           AND stored_length = $3::NUMERIC
           AND stored_sha256 = $4
         ORDER BY verified_at ASC, id ASC
         LIMIT 1
         FOR UPDATE",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;
    Ok(replica_id.is_some())
}

fn map_version_row(
    row: VersionMetadataRow,
    scope: &OwnedVersionScope,
) -> Result<VersionRecord, MetadataError> {
    let id = FileVersionId::try_from_uuid(row.id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "file_versions.id",
            reason,
        })
    })?;
    let library_id = LibraryId::try_from_uuid(row.library_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "file_versions.library_id",
            reason,
        })
    })?;
    let node_id = NodeId::try_from_uuid(row.node_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "file_versions.node_id",
            reason,
        })
    })?;
    let object_id = synveil_core::ObjectId::try_from_uuid(row.object_id).map_err(|reason| {
        MetadataError::Mapping(MappingError::InvalidId {
            field: "file_versions.object_id",
            reason,
        })
    })?;
    let object_dedup_domain_id =
        DedupDomainId::try_from_uuid(row.object_dedup_domain_id).map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "file_versions.object_dedup_domain_id",
                reason,
            })
        })?;
    let object_actual_dedup_domain_id =
        DedupDomainId::try_from_uuid(row.object_actual_dedup_domain_id).map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "objects.dedup_domain_id",
                reason,
            })
        })?;
    let library_dedup_domain_id = DedupDomainId::try_from_uuid(row.library_dedup_domain_id)
        .map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "libraries.dedup_domain_id",
                reason,
            })
        })?;

    if library_id != scope.node.library_id()
        || node_id != scope.node.id()
        || object_dedup_domain_id != object_actual_dedup_domain_id
        || object_dedup_domain_id != library_dedup_domain_id
        || object_dedup_domain_id != scope.dedup_domain_id
    {
        return Err(MetadataError::Mapping(MappingError::RelationMismatch {
            relation: "file_versions.objects",
        }));
    }
    if row.parent_version_id == Some(row.id) {
        return Err(MetadataError::Mapping(MappingError::RelationMismatch {
            relation: "file_versions.parent_version_id",
        }));
    }
    if let Some(parent_version_id) = row.parent_version_id {
        FileVersionId::try_from_uuid(parent_version_id).map_err(|reason| {
            MetadataError::Mapping(MappingError::InvalidId {
                field: "file_versions.parent_version_id",
                reason,
            })
        })?;
    }
    Revision::from_str(&row.revision).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "file_versions.revision",
        })
    })?;
    let sha256 = Sha256Digest::try_from(row.canonical_hash.as_slice()).map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDigest {
            field: "objects.canonical_hash",
        })
    })?;
    let byte_length = row.plaintext_length.parse::<u64>().map_err(|_| {
        MetadataError::Mapping(MappingError::InvalidDecimal {
            field: "objects.plaintext_length",
        })
    })?;

    // Keep the object ID as an explicitly validated relation even though it is
    // intentionally absent from the application-facing record.
    let _ = object_id;
    Ok(VersionRecord {
        id,
        node_id,
        committed_at: Timestamp::from_offset_datetime(row.committed_at),
        byte_length,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use synveil_core::ObjectId;

    #[test]
    fn version_object_relation_mismatch_fails_closed() {
        let library_id = LibraryId::new();
        let scope_domain = DedupDomainId::new();
        let observed_at = Timestamp::parse("2026-08-24T00:00:00Z").expect("valid timestamp");
        let root = Node::new_root(
            NodeId::new(),
            library_id,
            LogicalName::new("root").expect("valid root name"),
            observed_at,
        );
        let file = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::File,
            LogicalName::new("file.bin").expect("valid file name"),
            observed_at,
        )
        .expect("valid file node");
        let row = VersionMetadataRow {
            id: FileVersionId::new().into_uuid(),
            library_id: library_id.into_uuid(),
            node_id: file.id().into_uuid(),
            object_id: ObjectId::new().into_uuid(),
            object_dedup_domain_id: scope_domain.into_uuid(),
            object_actual_dedup_domain_id: DedupDomainId::new().into_uuid(),
            parent_version_id: None,
            committed_at: observed_at.as_offset_datetime(),
            revision: "0".to_owned(),
            canonical_hash: vec![0x42; 32],
            plaintext_length: "42".to_owned(),
            library_dedup_domain_id: scope_domain.into_uuid(),
        };
        let scope = OwnedVersionScope {
            node: file,
            dedup_domain_id: scope_domain,
            current_version_id: None,
        };

        assert_eq!(
            map_version_row(row, &scope),
            Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "file_versions.objects",
            }))
        );
    }
}
