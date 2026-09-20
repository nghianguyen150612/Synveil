//! Focused PostgreSQL repositories for the initial canonical domain.

use std::str::FromStr;

use sqlx::FromRow;
use synveil_core::{
    ChangeEvent, ChangeKind, ClientMutation, ClientMutationKind, ClientMutationRequest,
    DedupDomainId, Device, DomainError, FileVersion, FileVersionId, Library, LibraryId,
    LibraryStatus, LogicalName, Node, NodeId, NodeKind, NodeState, ObjectGcPolicy, ObjectId,
    ObjectReference, Revision, Sequence, Sha256Digest, SyncConflictId, Timestamp, User, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::conflicts::persist_sync_conflict;
use crate::gc::{
    GcLeaseId, GcPlanMutation, GcReleaseMutation, GcRenewalMutation, ObjectGcCandidate,
    ObjectGcCandidateRow, ObjectGcCandidateState, ObjectGcLease, map_object_gc_candidate_row,
};
use crate::journal::{JournalChange, acquire_namespace_guard, append_changes};
use crate::mutations::{
    ClientMutationError, ClientMutationResult, MutationConflict, MutationConflictReason,
};
use crate::purge::PurgeCursor;
use crate::versions::{VersionCursor, VersionRecord, restore_request_fingerprint};
use crate::{
    DatabaseError, DatabaseErrorKind, DatabasePool, DeviceRow, FileVersionRow, LibraryRow,
    MappingError, MetadataError, NodeRow, ObjectRow, UserRow,
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

#[derive(Debug, FromRow)]
struct ClientMutationScopeRow {
    device_owner_user_id: Uuid,
    device_status: String,
    library_owner_user_id: Uuid,
    library_status: String,
    journal_epoch: i64,
    sync_head: i64,
    minimum_retained_sequence: i64,
}

#[derive(Debug, FromRow)]
struct ClientMutationOperationRow {
    owner_user_id: Uuid,
    device_id: Uuid,
    library_id: Uuid,
    client_mutation_id: Uuid,
    fingerprint_version: i16,
    fingerprint: Vec<u8>,
    kind: String,
    base_epoch: i64,
    base_sequence: i64,
    outcome: String,
    resource_id: Option<Uuid>,
    result_parent_node_id: Option<Uuid>,
    result_kind: Option<String>,
    result_name: Option<String>,
    result_state: Option<String>,
    result_current_version_id: Option<Uuid>,
    result_revision: Option<String>,
    result_trashed_at: Option<OffsetDateTime>,
    result_created_at: Option<OffsetDateTime>,
    result_updated_at: Option<OffsetDateTime>,
    journal_event_id: Option<Uuid>,
    journal_sequence: Option<i64>,
    conflict_reason: Option<String>,
    conflict_expected_revision: Option<String>,
    conflict_current_revision: Option<String>,
    conflict_current_state: Option<String>,
    conflict_current_parent_id: Option<Uuid>,
    conflict_current_name: Option<String>,
    conflict_id: Option<Uuid>,
    server_epoch: i64,
    server_sequence: i64,
    completed_at: Option<OffsetDateTime>,
}

#[derive(Clone, Copy, Debug)]
struct ClientMutationScope {
    journal_epoch: Sequence,
    sync_head: Sequence,
    minimum_retained_sequence: Sequence,
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

    /// Create an owner-scoped library and root using a caller-supplied UUID.
    /// `ON CONFLICT DO NOTHING` makes a retried request return the existing
    /// durable row; the application layer then verifies owner and name before
    /// exposing it to the caller.
    pub(crate) async fn create_library_owned(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        name: LogicalName,
        observed_at: Timestamp,
    ) -> Result<Library, MetadataError> {
        let root = Node::new_root(
            NodeId::new(),
            library_id,
            LogicalName::new("root")?,
            observed_at,
        );
        let library = Library::new(
            library_id,
            owner_user_id,
            name,
            &root,
            DedupDomainId::new(),
            observed_at,
        )?;
        library.validate_root(&root)?;
        let library_row = LibraryRow::from_domain(&library)?;
        let root_row = NodeRow::from_domain(&root)?;
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        let result = sqlx::query(
            "INSERT INTO libraries
                (id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                 created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::NUMERIC)
             ON CONFLICT (id) DO NOTHING",
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

        if result.rows_affected() == 0 {
            transaction.commit().await.map_err(MetadataError::from)?;
            return self
                .find_library(library_id)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "libraries.id",
                }));
        }

        Self::insert_node_row_in_transaction(&mut transaction, &root_row).await?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(library)
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

        // The namespace guard is acquired before the idempotency row so a
        // concurrent metadata purge cannot hold the guard while waiting for a
        // restore-operation row that this transaction already owns.
        acquire_namespace_guard(&mut transaction, library_id).await?;

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
            let library =
                Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id)
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

        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
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

        // Restore reference creation participates in the same canonical
        // order as GC and purge: target library/node -> candidate row (if
        // present) -> Object advisory/Object row -> replica row. The source
        // lookup deliberately locks only the immutable FileVersion; taking
        // the Object lock there would invert this order.
        Self::lock_gc_candidate_row_for_reference(
            &mut transaction,
            object.object_id(),
            object.dedup_domain_id(),
        )
        .await?;
        Self::lock_object_for_gc(
            &mut transaction,
            object.object_id(),
            object.dedup_domain_id(),
        )
        .await?;
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
        append_changes(
            &mut transaction,
            user_id,
            library_id,
            &[JournalChange::from_node(
                ChangeKind::FileVersionRestored,
                &next_node,
            )],
        )
        .await?;
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

    /// Submit exactly one typed client mutation. The operation identity row,
    /// canonical node update, and one journal append share this transaction.
    /// The namespace guard is acquired before any Node row lock, matching the
    /// existing metadata mutation and rebaseline paths.
    pub(crate) async fn submit_client_mutation(
        &self,
        owner_user_id: UserId,
        device_id: synveil_core::DeviceId,
        library_id: LibraryId,
        request: ClientMutationRequest,
    ) -> Result<ClientMutationResult, ClientMutationError> {
        let fingerprint = request.fingerprint();
        if request.base_epoch().get() == 0 {
            return Err(ClientMutationError::InvalidMutation);
        }
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(map_client_sqlx_error)?;

        // Establish the namespace lock before the operation insert. The
        // operation's owner-pair foreign keys take PostgreSQL key-share locks
        // on the Device/Library rows; inserting first and acquiring the
        // namespace guard second would deadlock against another mutation
        // that already owns the guard and is waiting for those row locks.
        acquire_namespace_guard(&mut transaction, library_id)
            .await
            .map_err(map_client_metadata_error)?;
        let scope = load_client_mutation_scope(
            &mut transaction,
            owner_user_id,
            device_id,
            library_id,
            true,
        )
        .await?;
        let base_epoch = i64_from_client_sequence(request.base_epoch())?;
        let base_sequence = i64_from_client_sequence(request.base_sequence())?;
        let fingerprint_version = i16::try_from(fingerprint.version())
            .map_err(|_| ClientMutationError::InvalidMutation)?;

        sqlx::query(
            "INSERT INTO device_mutation_operations
                (owner_user_id, device_id, library_id, client_mutation_id,
                 fingerprint_version, fingerprint, kind, base_epoch, base_sequence,
                 outcome, server_epoch, server_sequence, created_at, completed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'IN_PROGRESS',
                     $10, $11, CURRENT_TIMESTAMP, NULL)
             ON CONFLICT (owner_user_id, device_id, library_id, client_mutation_id)
             DO NOTHING",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(request.mutation_id().into_uuid())
        .bind(fingerprint_version)
        .bind(fingerprint.sha256().to_vec())
        .bind(request.kind().as_str())
        .bind(base_epoch)
        .bind(base_sequence)
        .bind(i64_from_client_sequence(scope.journal_epoch)?)
        .bind(i64_from_client_sequence(scope.sync_head)?)
        .execute(&mut *transaction)
        .await
        .map_err(map_client_sqlx_error)?;

        let operation = load_client_mutation_operation(
            &mut transaction,
            owner_user_id,
            device_id,
            library_id,
            request.mutation_id(),
        )
        .await?
        .ok_or(ClientMutationError::InvalidPersistedData)?;
        if operation.fingerprint_version != fingerprint_version
            || operation.fingerprint.as_slice() != fingerprint.sha256().as_slice()
        {
            return Err(ClientMutationError::MutationIdConflict);
        }

        if operation.outcome != "IN_PROGRESS" {
            let result = map_client_mutation_operation(
                operation,
                &request,
                owner_user_id,
                device_id,
                library_id,
                true,
            )?;
            transaction.commit().await.map_err(map_client_sqlx_error)?;
            return Ok(result);
        }

        // The namespace and exact owner/device/library rows were locked before
        // the operation identity insert. This scope therefore remains the one
        // current PostgreSQL journal clock for the mutation decision.
        validate_client_mutation_base(&request, scope)?;

        let observed_at = Self::database_now(&mut transaction)
            .await
            .map_err(map_client_metadata_error)?;
        let decision = prepare_canonical_client_mutation(
            &mut transaction,
            owner_user_id,
            library_id,
            scope.journal_epoch,
            scope.sync_head,
            observed_at,
            request.mutation(),
        )
        .await?;

        match decision {
            ClientMutationDecision::Conflict(conflict) => {
                let conflict_id = persist_sync_conflict(
                    &mut transaction,
                    owner_user_id,
                    device_id,
                    library_id,
                    &request,
                    &conflict,
                )
                .await?;
                persist_client_mutation_conflict(
                    &mut transaction,
                    owner_user_id,
                    device_id,
                    library_id,
                    request.mutation_id(),
                    conflict_id,
                    &conflict,
                )
                .await?;
                let result = ClientMutationResult::Conflict {
                    mutation_id: request.mutation_id(),
                    kind: request.kind(),
                    conflict_id,
                    conflict,
                    replayed: false,
                };
                transaction.commit().await.map_err(map_client_sqlx_error)?;
                Ok(result)
            }
            ClientMutationDecision::Applied { node, change_kind } => {
                let events = append_changes(
                    &mut transaction,
                    owner_user_id,
                    library_id,
                    &[JournalChange::from_node(change_kind, &node)],
                )
                .await
                .map_err(map_client_metadata_error)?;
                let event = events
                    .into_iter()
                    .next()
                    .ok_or(ClientMutationError::InvalidPersistedData)?;
                persist_client_mutation_applied(
                    &mut transaction,
                    owner_user_id,
                    device_id,
                    library_id,
                    request.mutation_id(),
                    &node,
                    event,
                )
                .await?;
                let result = ClientMutationResult::Applied {
                    mutation_id: request.mutation_id(),
                    kind: request.kind(),
                    node,
                    journal_event_id: event.id(),
                    journal_sequence: event.sequence(),
                    replayed: false,
                };
                transaction.commit().await.map_err(map_client_sqlx_error)?;
                Ok(result)
            }
        }
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
        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(None);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;
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
        append_changes(
            &mut transaction,
            user_id,
            library_id,
            &[JournalChange::from_node(ChangeKind::NodeCreated, &node)],
        )
        .await?;
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
        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;
        let Some(mut node) =
            Self::lock_node_in_library(&mut transaction, library_id, node_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        if node.revision() != expected_revision {
            return Ok(NodeMutation::VersionConflict(node));
        }
        let previous_revision = node.revision();
        node.rename(name, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        if node.revision() != previous_revision {
            append_changes(
                &mut transaction,
                user_id,
                library_id,
                &[JournalChange::from_node(ChangeKind::NodeRenamed, &node)],
            )
            .await?;
        }
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
        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;
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
        let previous_revision = node.revision();
        node.validate_move_parent(&destination, &ancestors)?;
        node.move_to(&destination, observed_at)?;
        Self::update_node_in_transaction(&mut transaction, &node).await?;
        if node.revision() != previous_revision {
            append_changes(
                &mut transaction,
                user_id,
                library_id,
                &[JournalChange::from_node(ChangeKind::NodeMoved, &node)],
            )
            .await?;
        }
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
        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;
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
        append_changes(
            &mut transaction,
            user_id,
            library_id,
            &[JournalChange::from_node(ChangeKind::NodeTrashed, &node)],
        )
        .await?;
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
        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(NodeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;
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
        append_changes(
            &mut transaction,
            user_id,
            library_id,
            &[JournalChange::from_node(ChangeKind::NodeRestored, &node)],
        )
        .await?;
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
        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(PurgeMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;
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

        let Some(library) =
            Self::load_owned_library_in_transaction(&mut transaction, user_id, library_id).await?
        else {
            return Ok(PurgeExecutionMutation::NotFound);
        };
        ensure_library_writable(&library)?;
        acquire_namespace_guard(&mut transaction, library_id).await?;

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

        // Candidate rows must precede canonical Object locks in the shared
        // lock order. This is the Prompt 26 side of the GC worker/purge race:
        // library/node -> candidate -> object advisory/Object row.
        Self::lock_gc_candidate_rows_for_purge(&mut transaction, node.id(), library_id).await?;

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

        append_changes(
            &mut transaction,
            user_id,
            library_id,
            &[JournalChange::purged(&node)],
        )
        .await?;

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

    /// Claim a bounded batch of metadata-only object-GC candidates. Candidate
    /// rows are locked in stable order first; each canonical Object is then
    /// locked and every committed content-retention reference (live
    /// FileVersion or durable backup pin) is rechecked before the lease is
    /// written. This method never touches replicas or object bytes.
    pub(crate) async fn claim_object_gc_candidates(
        &self,
        policy: ObjectGcPolicy,
        limit: u32,
    ) -> Result<Vec<ObjectGcLease>, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let now = Self::database_now(&mut transaction).await?;
        let (grace_cutoff, _) = gc_time_window(policy, now)?;

        let rows = sqlx::query_as::<_, ObjectGcCandidateRow>(
            "SELECT object_id, object_dedup_domain_id, unreferenced_at, source,
                    state, lease_id, lease_generation::TEXT AS lease_generation,
                    lease_acquired_at, lease_expires_at, validated_at
             FROM object_gc_candidates
             WHERE source = 'METADATA_PURGE'
               AND unreferenced_at <= $1
               -- A durable physical operation is its own recovery queue. Do
               -- not let generic candidate planning reclaim it as new work;
               -- the GC coordinator must rebind it through the explicit
               -- resume-before-new-work boundary instead.
               AND NOT EXISTS (
                    SELECT 1
                    FROM object_gc_operations AS operation
                    WHERE operation.object_id = object_gc_candidates.object_id
                      AND operation.object_dedup_domain_id =
                          object_gc_candidates.object_dedup_domain_id
                      AND operation.state <> 'COMPLETED'
               )
               AND (
                    state = 'ELIGIBLE'
                    OR (
                        state IN ('LEASED', 'READY')
                        AND lease_expires_at <= $2
                    )
               )
             ORDER BY unreferenced_at ASC, object_id ASC,
                      object_dedup_domain_id ASC
             LIMIT $3
             FOR UPDATE SKIP LOCKED",
        )
        .bind(grace_cutoff.as_offset_datetime())
        .bind(now.as_offset_datetime())
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;

        let mut leases = Vec::with_capacity(rows.len());
        for row in rows {
            let candidate = map_object_gc_candidate_row(row)?;
            Self::lock_object_for_gc(
                &mut transaction,
                candidate.object_id(),
                candidate.dedup_domain_id(),
            )
            .await?;

            // The Object lock can wait behind another metadata/storage
            // transaction. Start the lease at a fresh server-observed instant
            // after that wait, rather than shortening or misdating the lease
            // from the initial candidate-query timestamp.
            let claimed_at = Self::database_now(&mut transaction).await?;
            let (_, lease_expires_at) = gc_time_window(policy, claimed_at)?;
            if Self::has_object_retention_reference(
                &mut transaction,
                candidate.object_id(),
                candidate.dedup_domain_id(),
            )
            .await?
            {
                Self::delete_object_gc_candidate(
                    &mut transaction,
                    candidate.object_id(),
                    candidate.dedup_domain_id(),
                )
                .await?;
                continue;
            }

            let lease_generation =
                candidate
                    .lease_generation()
                    .checked_add(1)
                    .ok_or(MetadataError::Mapping(MappingError::InvalidDecimal {
                        field: "object_gc_candidates.lease_generation",
                    }))?;
            let lease_id = GcLeaseId::new();
            let updated = sqlx::query(
                "UPDATE object_gc_candidates
                 SET state = 'LEASED', lease_id = $4,
                     lease_generation = $5::NUMERIC,
                     lease_acquired_at = $6, lease_expires_at = $7,
                     validated_at = $6
                 WHERE object_id = $1
                   AND object_dedup_domain_id = $2
                   AND source = 'METADATA_PURGE'
                   AND state = $3
                   AND lease_generation = $8::NUMERIC",
            )
            .bind(candidate.object_id().into_uuid())
            .bind(candidate.dedup_domain_id().into_uuid())
            .bind(candidate.state().as_str())
            .bind(lease_id.into_uuid())
            .bind(lease_generation.to_string())
            .bind(claimed_at.as_offset_datetime())
            .bind(lease_expires_at.as_offset_datetime())
            .bind(candidate.lease_generation().to_string())
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
            if updated.rows_affected() != 1 {
                return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "object_gc_candidates.claim",
                }));
            }

            leases.push(ObjectGcLease::from_parts(
                candidate.object_id(),
                candidate.dedup_domain_id(),
                lease_id,
                lease_generation,
                claimed_at,
                lease_expires_at,
                ObjectGcCandidateState::Leased,
            ));
        }

        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(leases)
    }

    /// Renew a matching lease after revalidating its committed content
    /// retention relation under the canonical Object lock.
    pub(crate) async fn renew_object_gc_lease(
        &self,
        policy: ObjectGcPolicy,
        lease: ObjectGcLease,
    ) -> Result<GcRenewalMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(row) = Self::lock_object_gc_candidate(&mut transaction, lease).await? else {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::CandidateGone);
        };
        let now = Self::database_now(&mut transaction).await?;
        let (grace_cutoff, _lease_expires_at) = gc_time_window(policy, now)?;
        let candidate = map_object_gc_candidate_row(row)?;
        let Some(stored_lease) = candidate.lease() else {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::StaleLease);
        };
        if !same_gc_lease(stored_lease, lease) {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::StaleLease);
        }
        if stored_lease.lease_expires_at() <= now {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::LeaseExpired);
        }
        if candidate.unreferenced_at() > grace_cutoff {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::GraceNotMature);
        }

        Self::lock_object_for_gc(
            &mut transaction,
            candidate.object_id(),
            candidate.dedup_domain_id(),
        )
        .await?;
        let validation_now = Self::database_now(&mut transaction).await?;
        let (validation_grace_cutoff, lease_expires_at) = gc_time_window(policy, validation_now)?;
        if stored_lease.lease_expires_at() <= validation_now {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::LeaseExpired);
        }
        if candidate.unreferenced_at() > validation_grace_cutoff {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::GraceNotMature);
        }
        if Self::has_object_retention_reference(
            &mut transaction,
            candidate.object_id(),
            candidate.dedup_domain_id(),
        )
        .await?
        {
            Self::delete_object_gc_candidate(
                &mut transaction,
                candidate.object_id(),
                candidate.dedup_domain_id(),
            )
            .await?;
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcRenewalMutation::Invalidated);
        }

        let updated = sqlx::query(
            "UPDATE object_gc_candidates
             SET lease_expires_at = $4, validated_at = $4
             WHERE object_id = $1
               AND object_dedup_domain_id = $2
               AND source = 'METADATA_PURGE'
               AND lease_id = $3
               AND lease_generation = $5::NUMERIC
               AND state IN ('LEASED', 'READY')",
        )
        .bind(candidate.object_id().into_uuid())
        .bind(candidate.dedup_domain_id().into_uuid())
        .bind(lease.lease_id().into_uuid())
        .bind(lease_expires_at.as_offset_datetime())
        .bind(lease.lease_generation().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if updated.rows_affected() != 1 {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "object_gc_candidates.renew",
            }));
        }

        let renewed = ObjectGcLease::from_parts(
            candidate.object_id(),
            candidate.dedup_domain_id(),
            stored_lease.lease_id(),
            stored_lease.lease_generation(),
            stored_lease.lease_acquired_at(),
            lease_expires_at,
            candidate.state(),
        );
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(GcRenewalMutation::Renewed(renewed))
    }

    /// Safely release a matching lease back to `ELIGIBLE`. The transition is
    /// metadata-only and remains fenced by the generation supplied by the
    /// worker.
    pub(crate) async fn release_object_gc_lease(
        &self,
        lease: ObjectGcLease,
    ) -> Result<GcReleaseMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(row) = Self::lock_object_gc_candidate(&mut transaction, lease).await? else {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcReleaseMutation::CandidateGone);
        };
        let candidate = map_object_gc_candidate_row(row)?;
        if candidate.lease_generation() != lease.lease_generation() {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcReleaseMutation::StaleLease);
        }
        if candidate.state() == ObjectGcCandidateState::Eligible {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcReleaseMutation::AlreadyReleased);
        }
        let Some(stored_lease) = candidate.lease() else {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcReleaseMutation::StaleLease);
        };
        if !same_gc_lease(stored_lease, lease) {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcReleaseMutation::StaleLease);
        }

        let updated = sqlx::query(
            "UPDATE object_gc_candidates
             SET state = 'ELIGIBLE', lease_id = NULL,
                 lease_acquired_at = NULL, lease_expires_at = NULL,
                 validated_at = NULL
             WHERE object_id = $1
               AND object_dedup_domain_id = $2
               AND source = 'METADATA_PURGE'
               AND lease_id = $3
               AND lease_generation = $4::NUMERIC
               AND state IN ('LEASED', 'READY')",
        )
        .bind(candidate.object_id().into_uuid())
        .bind(candidate.dedup_domain_id().into_uuid())
        .bind(lease.lease_id().into_uuid())
        .bind(lease.lease_generation().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if updated.rows_affected() != 1 {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "object_gc_candidates.release",
            }));
        }
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(GcReleaseMutation::Released)
    }

    /// Revalidate a leased candidate, optionally moving it to the revocable
    /// `READY` planning state. The Object row is only locked for reference
    /// truth; no Object or replica row is deleted or updated.
    pub(crate) async fn revalidate_object_gc_candidate(
        &self,
        policy: ObjectGcPolicy,
        lease: ObjectGcLease,
        mark_ready: bool,
    ) -> Result<GcPlanMutation, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(row) = Self::lock_object_gc_candidate(&mut transaction, lease).await? else {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::CandidateGone);
        };
        let now = Self::database_now(&mut transaction).await?;
        let (grace_cutoff, _lease_expires_at) = gc_time_window(policy, now)?;
        let candidate = map_object_gc_candidate_row(row)?;
        let Some(stored_lease) = candidate.lease() else {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::StaleLease);
        };
        if !same_gc_lease(stored_lease, lease) {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::StaleLease);
        }
        if stored_lease.lease_expires_at() <= now {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::LeaseExpired);
        }
        if candidate.unreferenced_at() > grace_cutoff {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::GraceNotMature);
        }

        Self::lock_object_for_gc(
            &mut transaction,
            candidate.object_id(),
            candidate.dedup_domain_id(),
        )
        .await?;
        let validation_now = Self::database_now(&mut transaction).await?;
        let validation_grace_cutoff = validation_now
            .checked_sub_std(policy.grace_period())
            .ok_or(MetadataError::Mapping(MappingError::InvalidTimestamp {
                field: "object_gc_policy.grace_cutoff",
            }))?;
        if stored_lease.lease_expires_at() <= validation_now {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::LeaseExpired);
        }
        if candidate.unreferenced_at() > validation_grace_cutoff {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::GraceNotMature);
        }
        if Self::has_object_retention_reference(
            &mut transaction,
            candidate.object_id(),
            candidate.dedup_domain_id(),
        )
        .await?
        {
            Self::delete_object_gc_candidate(
                &mut transaction,
                candidate.object_id(),
                candidate.dedup_domain_id(),
            )
            .await?;
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(GcPlanMutation::Invalidated);
        }

        let state = if mark_ready {
            ObjectGcCandidateState::Ready
        } else {
            candidate.state()
        };
        let updated = sqlx::query(
            "UPDATE object_gc_candidates
             SET state = $4, validated_at = $5
             WHERE object_id = $1
               AND object_dedup_domain_id = $2
               AND source = 'METADATA_PURGE'
               AND lease_id = $3
               AND lease_generation = $6::NUMERIC
               AND state IN ('LEASED', 'READY')",
        )
        .bind(candidate.object_id().into_uuid())
        .bind(candidate.dedup_domain_id().into_uuid())
        .bind(lease.lease_id().into_uuid())
        .bind(state.as_str())
        .bind(validation_now.as_offset_datetime())
        .bind(lease.lease_generation().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if updated.rows_affected() != 1 {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "object_gc_candidates.revalidate",
            }));
        }

        let planned_lease = ObjectGcLease::from_parts(
            candidate.object_id(),
            candidate.dedup_domain_id(),
            stored_lease.lease_id(),
            stored_lease.lease_generation(),
            stored_lease.lease_acquired_at(),
            stored_lease.lease_expires_at(),
            state,
        );
        let planned = ObjectGcCandidate::from_parts(
            candidate.object_id(),
            candidate.dedup_domain_id(),
            candidate.unreferenced_at(),
            state,
            candidate.lease_generation(),
            Some(validation_now),
            Some(planned_lease),
        );
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(GcPlanMutation::Valid(planned))
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

    pub(crate) async fn insert_node_row_in_transaction(
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

    pub(crate) async fn update_node_in_transaction(
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

    async fn database_now(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Timestamp, MetadataError> {
        let now = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
            .fetch_one(&mut **transaction)
            .await
            .map_err(MetadataError::from)?;
        Ok(Timestamp::from_offset_datetime(now))
    }

    async fn lock_object_gc_candidate(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        lease: ObjectGcLease,
    ) -> Result<Option<ObjectGcCandidateRow>, MetadataError> {
        sqlx::query_as::<_, ObjectGcCandidateRow>(
            "SELECT object_id, object_dedup_domain_id, unreferenced_at, source,
                    state, lease_id, lease_generation::TEXT AS lease_generation,
                    lease_acquired_at, lease_expires_at, validated_at
             FROM object_gc_candidates
             WHERE object_id = $1 AND object_dedup_domain_id = $2
             FOR UPDATE",
        )
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)
    }

    pub(crate) async fn lock_gc_candidate_row_for_reference(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        object_id: ObjectId,
        object_dedup_domain_id: DedupDomainId,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "SELECT 1
             FROM object_gc_candidates
             WHERE object_id = $1 AND object_dedup_domain_id = $2
             FOR UPDATE",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub(crate) async fn lock_object_for_gc(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        object_id: ObjectId,
        object_dedup_domain_id: DedupDomainId,
    ) -> Result<(), MetadataError> {
        // Canonical object lock order: candidate row first, then this
        // advisory lock, then the Object row. The advisory key is derived
        // only from typed canonical identity; collisions reduce concurrency
        // but cannot authorize a different object.
        sqlx::query(
            "SELECT pg_advisory_xact_lock(
                hashtextextended($1::UUID::TEXT || ':' || $2::UUID::TEXT, 0)
             )",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;

        let object_exists = sqlx::query(
            "SELECT 1
             FROM objects
             WHERE id = $1 AND dedup_domain_id = $2
             FOR UPDATE",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?
        .is_some();
        if !object_exists {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "object_gc_candidates.object",
            }));
        }
        Ok(())
    }

    /// The authoritative logical-content relation. A durable backup pin is
    /// intentionally independent from mutable Node/FileVersion rows, so it
    /// remains a live reference after metadata purge and after a snapshot has
    /// moved from `COMPLETED` to lifecycle-only `EXPIRED`.
    async fn has_object_retention_reference(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        object_id: ObjectId,
        object_dedup_domain_id: DedupDomainId,
    ) -> Result<bool, MetadataError> {
        sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM file_versions
                WHERE object_id = $1 AND object_dedup_domain_id = $2
                UNION ALL
                SELECT 1 FROM backup_snapshot_content_pins
                WHERE object_id = $1 AND object_dedup_domain_id = $2
            )",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .fetch_one(&mut **transaction)
        .await
        .map_err(MetadataError::from)
    }

    async fn delete_object_gc_candidate(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        object_id: ObjectId,
        object_dedup_domain_id: DedupDomainId,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "DELETE FROM object_gc_candidates
             WHERE object_id = $1
               AND object_dedup_domain_id = $2
               AND source = 'METADATA_PURGE'",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    /// Prompt 26 purge execution participates in the same candidate-first,
    /// canonical-object-second order as GC workers and committed reference
    /// creation. This closes the candidate/object deadlock cycle without
    /// weakening the survivor `NOT EXISTS` authority.
    async fn lock_gc_candidate_rows_for_purge(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        node_id: NodeId,
        library_id: LibraryId,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "SELECT candidate.object_id, candidate.object_dedup_domain_id
             FROM object_gc_candidates AS candidate
             WHERE EXISTS (
                 SELECT 1
                 FROM file_versions AS version
                 WHERE version.node_id = $1
                   AND version.library_id = $2
                   AND version.object_id = candidate.object_id
                   AND version.object_dedup_domain_id = candidate.object_dedup_domain_id
             )
             ORDER BY candidate.object_id ASC, candidate.object_dedup_domain_id ASC
             FOR UPDATE OF candidate",
        )
        .bind(node_id.into_uuid())
        .bind(library_id.into_uuid())
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
        // Reference creation and GC/purge all acquire the candidate row before
        // the canonical object lock. The row may be absent, so the delete after
        // the advisory lock also closes the absent-at-first-read window for a
        // candidate committed by another metadata transaction.
        sqlx::query(
            "SELECT 1
             FROM object_gc_candidates
             WHERE object_id = $1 AND object_dedup_domain_id = $2
             FOR UPDATE",
        )
        .bind(object_id.into_uuid())
        .bind(object_dedup_domain_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(MetadataError::from)?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(
                hashtextextended($1::UUID::TEXT || ':' || $2::UUID::TEXT, 0)
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
    /// canonical Objects whose last committed content-retention reference
    /// disappeared.
    ///
    /// The `NOT EXISTS` relation is the reference-accounting authority. The
    /// data-modifying CTE sees the statement snapshot, so the survivor query
    /// intentionally excludes the node being deleted and still sees every
    /// committed reference from other nodes/libraries and every durable backup
    /// pin. No Object or replica row is mutated here.
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
               AND NOT EXISTS (
                   SELECT 1
                   FROM backup_snapshot_content_pins AS pin
                   WHERE pin.object_id = deleted.object_id
                     AND pin.object_dedup_domain_id = deleted.object_dedup_domain_id
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

    pub(crate) async fn insert_file_version_in_transaction(
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

    async fn load_owned_library_in_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: UserId,
        library_id: LibraryId,
    ) -> Result<Option<Library>, MetadataError> {
        let row = sqlx::query_as::<_, LibraryRow>(
            "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                    created_at, updated_at, revision::TEXT AS revision
             FROM libraries
             WHERE id = $1 AND owner_user_id = $2",
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
             WHERE id = $1 AND library_id = $2",
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

#[derive(Clone, Debug)]
pub(crate) enum ClientMutationDecision {
    Applied { node: Node, change_kind: ChangeKind },
    Conflict(MutationConflict),
}

#[derive(Clone, Copy, Debug)]
struct ClientMutationPrepareContext {
    owner_user_id: UserId,
    library_id: LibraryId,
    scope: ClientMutationScope,
    observed_at: Timestamp,
}

/// Shared transaction-local executor for Prompt 34 submission and Prompt 35
/// explicit manual apply. Callers acquire the namespace guard and validate
/// owner/device/library scope first. A conflict decision writes neither
/// canonical state nor a journal entry.
pub(crate) async fn prepare_canonical_client_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
    journal_epoch: Sequence,
    server_sequence: Sequence,
    observed_at: Timestamp,
    mutation: &ClientMutation,
) -> Result<ClientMutationDecision, ClientMutationError> {
    let context = ClientMutationPrepareContext {
        owner_user_id,
        library_id,
        scope: ClientMutationScope {
            journal_epoch,
            sync_head: server_sequence,
            minimum_retained_sequence: Sequence::new(0),
        },
        observed_at,
    };
    match mutation {
        ClientMutation::CreateDirectory {
            parent_node_id,
            expected_parent_revision,
            name,
        } => {
            prepare_create_directory_mutation(
                transaction,
                context,
                *parent_node_id,
                *expected_parent_revision,
                name.clone(),
            )
            .await
        }
        ClientMutation::RenameNode {
            node_id,
            expected_revision,
            new_name,
        } => {
            prepare_rename_mutation(
                transaction,
                context,
                *node_id,
                *expected_revision,
                new_name.clone(),
            )
            .await
        }
        ClientMutation::MoveNode {
            node_id,
            expected_revision,
            new_parent_node_id,
            expected_new_parent_revision,
        } => {
            prepare_move_mutation(
                transaction,
                context,
                *node_id,
                *expected_revision,
                *new_parent_node_id,
                *expected_new_parent_revision,
            )
            .await
        }
        ClientMutation::TrashNode {
            node_id,
            expected_revision,
        } => prepare_trash_mutation(transaction, context, *node_id, *expected_revision).await,
        ClientMutation::RestoreNode {
            node_id,
            expected_revision,
            expected_parent_node_id,
            expected_parent_revision,
        } => {
            prepare_restore_mutation(
                transaction,
                context,
                *node_id,
                *expected_revision,
                *expected_parent_node_id,
                *expected_parent_revision,
            )
            .await
        }
    }
}

async fn load_client_mutation_scope(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: synveil_core::DeviceId,
    library_id: LibraryId,
    lock_rows: bool,
) -> Result<ClientMutationScope, ClientMutationError> {
    let row = if lock_rows {
        sqlx::query_as::<_, ClientMutationScopeRow>(
            "SELECT d.owner_user_id AS device_owner_user_id,
                    d.status AS device_status,
                    l.owner_user_id AS library_owner_user_id,
                    l.status AS library_status,
                    l.journal_epoch, l.sync_head, l.minimum_retained_sequence
             FROM devices AS d
             CROSS JOIN libraries AS l
             WHERE d.id = $1 AND l.id = $2
             FOR UPDATE OF d, l",
        )
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(map_client_sqlx_error)?
    } else {
        sqlx::query_as::<_, ClientMutationScopeRow>(
            "SELECT d.owner_user_id AS device_owner_user_id,
                    d.status AS device_status,
                    l.owner_user_id AS library_owner_user_id,
                    l.status AS library_status,
                    l.journal_epoch, l.sync_head, l.minimum_retained_sequence
             FROM devices AS d
             CROSS JOIN libraries AS l
             WHERE d.id = $1 AND l.id = $2",
        )
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(map_client_sqlx_error)?
    }
    .ok_or(ClientMutationError::NotFound)?;

    validate_client_mutation_scope(row, owner_user_id)
}

fn validate_client_mutation_scope(
    row: ClientMutationScopeRow,
    owner_user_id: UserId,
) -> Result<ClientMutationScope, ClientMutationError> {
    if row.device_owner_user_id != owner_user_id.into_uuid()
        || row.library_owner_user_id != owner_user_id.into_uuid()
    {
        return Err(ClientMutationError::NotFound);
    }
    if row.device_status != "ACTIVE" {
        return Err(ClientMutationError::NotFound);
    }
    if row.library_status != "ACTIVE" {
        return Err(ClientMutationError::InvalidMutation);
    }

    let journal_epoch = positive_client_sequence(row.journal_epoch)?;
    let sync_head = nonnegative_client_sequence(row.sync_head)?;
    let minimum_retained_sequence = nonnegative_client_sequence(row.minimum_retained_sequence)?;
    if minimum_retained_sequence.get() > sync_head.get() {
        return Err(ClientMutationError::InvalidPersistedData);
    }
    Ok(ClientMutationScope {
        journal_epoch,
        sync_head,
        minimum_retained_sequence,
    })
}

fn positive_client_sequence(value: i64) -> Result<Sequence, ClientMutationError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Sequence::new)
        .ok_or(ClientMutationError::InvalidPersistedData)
}

fn nonnegative_client_sequence(value: i64) -> Result<Sequence, ClientMutationError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| ClientMutationError::InvalidPersistedData)
}

fn i64_from_client_sequence(value: Sequence) -> Result<i64, ClientMutationError> {
    i64::try_from(value.get()).map_err(|_| ClientMutationError::InvalidMutation)
}

fn validate_client_mutation_base(
    request: &ClientMutationRequest,
    scope: ClientMutationScope,
) -> Result<(), ClientMutationError> {
    if request.base_epoch().get() == 0 {
        return Err(ClientMutationError::InvalidMutation);
    }
    if request.base_epoch() != scope.journal_epoch {
        return Err(ClientMutationError::RebaselineRequired {
            reason: crate::RebaselineReason::EpochMismatch,
            current_epoch: scope.journal_epoch,
            minimum_retained_sequence: scope.minimum_retained_sequence,
        });
    }
    if request.base_sequence().get() < scope.minimum_retained_sequence.get() {
        return Err(ClientMutationError::RebaselineRequired {
            reason: crate::RebaselineReason::HistoryUnavailable,
            current_epoch: scope.journal_epoch,
            minimum_retained_sequence: scope.minimum_retained_sequence,
        });
    }
    if request.base_sequence().get() > scope.sync_head.get() {
        return Err(ClientMutationError::InvalidMutation);
    }
    Ok(())
}

async fn load_client_mutation_operation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: synveil_core::DeviceId,
    library_id: LibraryId,
    mutation_id: synveil_core::ClientMutationId,
) -> Result<Option<ClientMutationOperationRow>, ClientMutationError> {
    sqlx::query_as::<_, ClientMutationOperationRow>(
        "SELECT owner_user_id, device_id, library_id, client_mutation_id,
                fingerprint_version, fingerprint, kind, base_epoch, base_sequence,
                outcome, resource_id, result_parent_node_id, result_kind,
                result_name, result_state, result_current_version_id,
                result_revision::TEXT AS result_revision, result_trashed_at,
                result_created_at, result_updated_at, journal_event_id,
                journal_sequence, conflict_reason,
                conflict_expected_revision::TEXT AS conflict_expected_revision,
                conflict_current_revision::TEXT AS conflict_current_revision,
                conflict_current_state, conflict_current_parent_id,
                conflict_current_name, conflict_id, server_epoch, server_sequence,
                completed_at
         FROM device_mutation_operations
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
           AND client_mutation_id = $4
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(mutation_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_client_sqlx_error)
}

async fn persist_client_mutation_conflict(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: synveil_core::DeviceId,
    library_id: LibraryId,
    mutation_id: synveil_core::ClientMutationId,
    conflict_id: SyncConflictId,
    conflict: &MutationConflict,
) -> Result<(), ClientMutationError> {
    let updated = sqlx::query(
        "UPDATE device_mutation_operations
         SET outcome = 'CONFLICT',
             resource_id = $5,
             result_parent_node_id = NULL,
             result_kind = NULL,
             result_name = NULL,
             result_state = NULL,
             result_current_version_id = NULL,
             result_revision = NULL,
             result_trashed_at = NULL,
             result_created_at = NULL,
             result_updated_at = NULL,
             journal_event_id = NULL,
             journal_sequence = NULL,
             conflict_reason = $6,
             conflict_expected_revision = $7::NUMERIC,
             conflict_current_revision = $8::NUMERIC,
             conflict_current_state = $9,
             conflict_current_parent_id = $10,
             conflict_current_name = $11,
             conflict_id = $12,
             server_epoch = $13,
             server_sequence = $14,
             completed_at = CURRENT_TIMESTAMP
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
           AND client_mutation_id = $4
           AND outcome = 'IN_PROGRESS'",
    )
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(mutation_id.into_uuid())
    .bind(conflict.resource_id().into_uuid())
    .bind(conflict.reason().as_str())
    .bind(
        conflict
            .expected_revision()
            .map(|revision| revision.get().to_string()),
    )
    .bind(
        conflict
            .current_revision()
            .map(|revision| revision.get().to_string()),
    )
    .bind(conflict.current_state().map(NodeState::as_str))
    .bind(conflict.current_parent_id().map(NodeId::into_uuid))
    .bind(conflict.current_name().map(LogicalName::as_str))
    .bind(conflict_id.into_uuid())
    .bind(i64_from_client_sequence(conflict.server_epoch())?)
    .bind(i64_from_client_sequence(conflict.server_sequence())?)
    .execute(&mut **transaction)
    .await
    .map_err(map_client_sqlx_error)?;
    if updated.rows_affected() != 1 {
        return Err(ClientMutationError::InvalidPersistedData);
    }
    Ok(())
}

async fn persist_client_mutation_applied(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    device_id: synveil_core::DeviceId,
    library_id: LibraryId,
    mutation_id: synveil_core::ClientMutationId,
    node: &Node,
    event: ChangeEvent,
) -> Result<(), ClientMutationError> {
    let updated = sqlx::query(
        "UPDATE device_mutation_operations
         SET outcome = 'APPLIED',
             resource_id = $5,
             result_parent_node_id = $6,
             result_kind = $7,
             result_name = $8,
             result_state = $9,
             result_current_version_id = $10,
             result_revision = $11::NUMERIC,
             result_trashed_at = $12,
             result_created_at = $13,
             result_updated_at = $14,
             journal_event_id = $15,
             journal_sequence = $16,
             conflict_reason = NULL,
             conflict_expected_revision = NULL,
             conflict_current_revision = NULL,
             conflict_current_state = NULL,
             conflict_current_parent_id = NULL,
             conflict_current_name = NULL,
             conflict_id = NULL,
             server_epoch = $17,
             server_sequence = $18,
             completed_at = CURRENT_TIMESTAMP
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
           AND client_mutation_id = $4
           AND outcome = 'IN_PROGRESS'",
    )
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(mutation_id.into_uuid())
    .bind(node.id().into_uuid())
    .bind(node.parent_node_id().map(NodeId::into_uuid))
    .bind(node.kind().as_str())
    .bind(node.name().as_str())
    .bind(node.state().as_str())
    .bind(
        node.current_version_id()
            .map(synveil_core::FileVersionId::into_uuid),
    )
    .bind(node.revision().get().to_string())
    .bind(
        node.trashed_at()
            .map(|timestamp| timestamp.as_offset_datetime()),
    )
    .bind(node.created_at().as_offset_datetime())
    .bind(node.updated_at().as_offset_datetime())
    .bind(event.id().into_uuid())
    .bind(i64_from_client_sequence(event.sequence())?)
    .bind(i64_from_client_sequence(event.journal_epoch())?)
    .bind(i64_from_client_sequence(event.sequence())?)
    .execute(&mut **transaction)
    .await
    .map_err(map_client_sqlx_error)?;
    if updated.rows_affected() != 1 {
        return Err(ClientMutationError::InvalidPersistedData);
    }
    Ok(())
}

fn map_client_mutation_operation(
    row: ClientMutationOperationRow,
    request: &ClientMutationRequest,
    owner_user_id: UserId,
    device_id: synveil_core::DeviceId,
    library_id: LibraryId,
    replayed: bool,
) -> Result<ClientMutationResult, ClientMutationError> {
    let expected_owner = owner_user_id.into_uuid();
    if row.owner_user_id != expected_owner
        || row.device_id != device_id.into_uuid()
        || row.library_id != library_id.into_uuid()
        || row.client_mutation_id != request.mutation_id().into_uuid()
        || row.fingerprint_version
            != i16::try_from(synveil_core::CLIENT_MUTATION_FINGERPRINT_VERSION)
                .map_err(|_| ClientMutationError::InvalidPersistedData)?
        || row.base_epoch != i64::try_from(request.base_epoch().get()).unwrap_or(i64::MIN)
        || row.base_sequence != i64::try_from(request.base_sequence().get()).unwrap_or(i64::MIN)
    {
        return Err(ClientMutationError::InvalidPersistedData);
    }
    let kind = ClientMutationKind::from_str(&row.kind)
        .map_err(|_| ClientMutationError::InvalidPersistedData)?;
    if kind != request.kind() || row.completed_at.is_none() {
        return Err(ClientMutationError::InvalidPersistedData);
    }
    let server_epoch = positive_client_sequence(row.server_epoch)?;
    let server_sequence = nonnegative_client_sequence(row.server_sequence)?;

    match row.outcome.as_str() {
        "APPLIED" => {
            let node_id = decode_client_node_id(
                row.resource_id
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )?;
            let parent_node_id = row
                .result_parent_node_id
                .map(decode_client_node_id)
                .transpose()?;
            let node_kind = match row
                .result_kind
                .as_deref()
                .ok_or(ClientMutationError::InvalidPersistedData)?
            {
                "FILE" => NodeKind::File,
                "DIRECTORY" => NodeKind::Directory,
                _ => return Err(ClientMutationError::InvalidPersistedData),
            };
            let state = match row
                .result_state
                .as_deref()
                .ok_or(ClientMutationError::InvalidPersistedData)?
            {
                "ACTIVE" => NodeState::Active,
                "TRASHED" => NodeState::Trashed,
                _ => return Err(ClientMutationError::InvalidPersistedData),
            };
            let name = LogicalName::new(
                row.result_name
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )
            .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let current_version_id = row
                .result_current_version_id
                .map(|value| {
                    synveil_core::FileVersionId::try_from_uuid(value)
                        .map_err(|_| ClientMutationError::InvalidPersistedData)
                })
                .transpose()?;
            let revision = Revision::from_str(
                &row.result_revision
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )
            .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let created_at = row
                .result_created_at
                .ok_or(ClientMutationError::InvalidPersistedData)?;
            let updated_at = row
                .result_updated_at
                .ok_or(ClientMutationError::InvalidPersistedData)?;
            let node = Node::rehydrate_with_trash(
                node_id,
                library_id,
                parent_node_id,
                node_kind,
                name,
                current_version_id,
                state,
                row.result_trashed_at.map(Timestamp::from_offset_datetime),
                Timestamp::from_offset_datetime(created_at),
                Timestamp::from_offset_datetime(updated_at),
                revision,
            )
            .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let journal_event_id = synveil_core::ChangeEventId::try_from_uuid(
                row.journal_event_id
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )
            .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let journal_sequence = positive_client_sequence(
                row.journal_sequence
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )?;
            if server_epoch != request.base_epoch() || journal_sequence != server_sequence {
                return Err(ClientMutationError::InvalidPersistedData);
            }
            Ok(ClientMutationResult::Applied {
                mutation_id: request.mutation_id(),
                kind,
                node,
                journal_event_id,
                journal_sequence,
                replayed,
            })
        }
        "CONFLICT" => {
            let conflict_id = SyncConflictId::try_from_uuid(
                row.conflict_id
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )
            .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let resource_id = decode_client_node_id(
                row.resource_id
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )?;
            let reason = MutationConflictReason::from_str(
                row.conflict_reason
                    .as_deref()
                    .ok_or(ClientMutationError::InvalidPersistedData)?,
            )
            .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let expected_revision = row
                .conflict_expected_revision
                .as_deref()
                .map(Revision::from_str)
                .transpose()
                .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let current_revision = row
                .conflict_current_revision
                .as_deref()
                .map(Revision::from_str)
                .transpose()
                .map_err(|_| ClientMutationError::InvalidPersistedData)?;
            let current_state = row
                .conflict_current_state
                .as_deref()
                .map(parse_client_node_state)
                .transpose()?;
            let current_parent_id = row
                .conflict_current_parent_id
                .map(decode_client_node_id)
                .transpose()?;
            let current_name = row
                .conflict_current_name
                .map(|value| {
                    LogicalName::new(value).map_err(|_| ClientMutationError::InvalidPersistedData)
                })
                .transpose()?;
            Ok(ClientMutationResult::Conflict {
                mutation_id: request.mutation_id(),
                kind,
                conflict_id,
                conflict: MutationConflict::new(
                    reason,
                    resource_id,
                    expected_revision,
                    current_revision,
                    current_state,
                    current_parent_id,
                    current_name,
                    server_epoch,
                    server_sequence,
                ),
                replayed,
            })
        }
        _ => Err(ClientMutationError::InvalidPersistedData),
    }
}

fn decode_client_node_id(value: Uuid) -> Result<NodeId, ClientMutationError> {
    NodeId::try_from_uuid(value).map_err(|_| ClientMutationError::InvalidPersistedData)
}

fn parse_client_node_state(value: &str) -> Result<NodeState, ClientMutationError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        "PURGING" => Ok(NodeState::Purging),
        _ => Err(ClientMutationError::InvalidPersistedData),
    }
}

fn node_conflict(
    reason: MutationConflictReason,
    node: &Node,
    expected_revision: Option<Revision>,
    scope: ClientMutationScope,
) -> ClientMutationDecision {
    ClientMutationDecision::Conflict(MutationConflict::new(
        reason,
        node.id(),
        expected_revision,
        Some(node.revision()),
        Some(node.state()),
        node.parent_node_id(),
        Some(node.name().clone()),
        scope.journal_epoch,
        scope.sync_head,
    ))
}

fn resource_conflict(
    reason: MutationConflictReason,
    resource_id: NodeId,
    expected_revision: Option<Revision>,
    current_revision: Option<Revision>,
    scope: ClientMutationScope,
) -> ClientMutationDecision {
    ClientMutationDecision::Conflict(MutationConflict::new(
        reason,
        resource_id,
        expected_revision,
        current_revision,
        None,
        None,
        None,
        scope.journal_epoch,
        scope.sync_head,
    ))
}

async fn latest_purged_revision(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
    node_id: NodeId,
) -> Result<Option<Revision>, ClientMutationError> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT resource_revision::TEXT
         FROM change_journal
         WHERE owner_user_id = $1
           AND library_id = $2
           AND resource_kind = 'NODE'
           AND resource_id = $3
           AND change_kind = 'NODE_PURGED'
         ORDER BY sequence DESC
         LIMIT 1",
    )
    .bind(owner_user_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(node_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_client_sqlx_error)?;
    value
        .map(|value| {
            Revision::from_str(&value).map_err(|_| ClientMutationError::InvalidPersistedData)
        })
        .transpose()
}

async fn missing_client_resource(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
    resource_id: NodeId,
    expected_revision: Option<Revision>,
    scope: ClientMutationScope,
) -> Result<ClientMutationDecision, ClientMutationError> {
    if let Some(current_revision) =
        latest_purged_revision(transaction, owner_user_id, library_id, resource_id).await?
    {
        return Ok(resource_conflict(
            MutationConflictReason::ResourcePurged,
            resource_id,
            expected_revision,
            Some(current_revision),
            scope,
        ));
    }
    Err(ClientMutationError::NotFound)
}

async fn active_child_with_name(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_id: LibraryId,
    parent_node_id: NodeId,
    name: &LogicalName,
    exclude_node_id: Option<NodeId>,
) -> Result<Option<Node>, ClientMutationError> {
    let row = sqlx::query_as::<_, NodeRow>(
        "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                state, trashed_at, created_at, updated_at, revision::TEXT AS revision
         FROM nodes
         WHERE library_id = $1
           AND parent_node_id = $2
           AND name = $3
           AND state = 'ACTIVE'
           AND ($4::UUID IS NULL OR id <> $4)
         ORDER BY id ASC
         LIMIT 1
         FOR UPDATE",
    )
    .bind(library_id.into_uuid())
    .bind(parent_node_id.into_uuid())
    .bind(name.as_str())
    .bind(exclude_node_id.map(NodeId::into_uuid))
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_client_sqlx_error)?;
    row.map(NodeRow::try_into_domain)
        .transpose()
        .map_err(|_| ClientMutationError::InvalidPersistedData)
}

async fn prepare_create_directory_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: ClientMutationPrepareContext,
    parent_node_id: NodeId,
    expected_parent_revision: Revision,
    name: LogicalName,
) -> Result<ClientMutationDecision, ClientMutationError> {
    let ClientMutationPrepareContext {
        owner_user_id,
        library_id,
        scope,
        observed_at,
    } = context;
    let Some(parent) =
        DomainRepository::lock_node_in_library(transaction, library_id, parent_node_id)
            .await
            .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            parent_node_id,
            Some(expected_parent_revision),
            scope,
        )
        .await;
    };
    if parent.revision() != expected_parent_revision {
        return Ok(node_conflict(
            MutationConflictReason::ParentChanged,
            &parent,
            Some(expected_parent_revision),
            scope,
        ));
    }
    if parent.kind() != NodeKind::Directory || parent.state() != NodeState::Active {
        return Ok(node_conflict(
            MutationConflictReason::NodeStateChanged,
            &parent,
            Some(expected_parent_revision),
            scope,
        ));
    }
    if let Some(existing) =
        active_child_with_name(transaction, library_id, parent_node_id, &name, None).await?
    {
        return Ok(node_conflict(
            MutationConflictReason::NameOccupied,
            &existing,
            None,
            scope,
        ));
    }
    let node = Node::new_child(
        NodeId::new(),
        library_id,
        &parent,
        NodeKind::Directory,
        name,
        observed_at,
    )
    .map_err(map_client_domain_error)?;
    DomainRepository::insert_node_row_in_transaction(
        transaction,
        &NodeRow::from_domain(&node).map_err(|_| ClientMutationError::InvalidPersistedData)?,
    )
    .await
    .map_err(map_client_metadata_error)?;
    Ok(ClientMutationDecision::Applied {
        node,
        change_kind: ChangeKind::NodeCreated,
    })
}

async fn prepare_rename_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: ClientMutationPrepareContext,
    node_id: NodeId,
    expected_revision: Revision,
    new_name: LogicalName,
) -> Result<ClientMutationDecision, ClientMutationError> {
    let ClientMutationPrepareContext {
        owner_user_id,
        library_id,
        scope,
        observed_at,
    } = context;
    let Some(mut node) = DomainRepository::lock_node_in_library(transaction, library_id, node_id)
        .await
        .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            node_id,
            Some(expected_revision),
            scope,
        )
        .await;
    };
    if node.revision() != expected_revision {
        return Ok(node_conflict(
            MutationConflictReason::RevisionMismatch,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.state() != NodeState::Active {
        return Ok(node_conflict(
            MutationConflictReason::NodeStateChanged,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.name() == &new_name {
        return Err(ClientMutationError::InvalidMutation);
    }
    if let Some(parent_node_id) = node.parent_node_id()
        && let Some(existing) = active_child_with_name(
            transaction,
            library_id,
            parent_node_id,
            &new_name,
            Some(node_id),
        )
        .await?
    {
        return Ok(node_conflict(
            MutationConflictReason::NameOccupied,
            &existing,
            None,
            scope,
        ));
    }
    node.rename(new_name, observed_at)
        .map_err(map_client_domain_error)?;
    DomainRepository::update_node_in_transaction(transaction, &node)
        .await
        .map_err(map_client_metadata_error)?;
    Ok(ClientMutationDecision::Applied {
        node,
        change_kind: ChangeKind::NodeRenamed,
    })
}

async fn prepare_move_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: ClientMutationPrepareContext,
    node_id: NodeId,
    expected_revision: Revision,
    new_parent_node_id: NodeId,
    expected_new_parent_revision: Revision,
) -> Result<ClientMutationDecision, ClientMutationError> {
    let ClientMutationPrepareContext {
        owner_user_id,
        library_id,
        scope,
        observed_at,
    } = context;
    let Some(mut node) = DomainRepository::lock_node_in_library(transaction, library_id, node_id)
        .await
        .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            node_id,
            Some(expected_revision),
            scope,
        )
        .await;
    };
    if node.revision() != expected_revision {
        return Ok(node_conflict(
            MutationConflictReason::RevisionMismatch,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.state() != NodeState::Active {
        return Ok(node_conflict(
            MutationConflictReason::NodeStateChanged,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    let Some(destination) =
        DomainRepository::lock_node_in_library(transaction, library_id, new_parent_node_id)
            .await
            .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            new_parent_node_id,
            Some(expected_new_parent_revision),
            scope,
        )
        .await;
    };
    if destination.revision() != expected_new_parent_revision {
        return Ok(node_conflict(
            MutationConflictReason::DestinationChanged,
            &destination,
            Some(expected_new_parent_revision),
            scope,
        ));
    }
    if destination.kind() != NodeKind::Directory || destination.state() != NodeState::Active {
        return Ok(node_conflict(
            MutationConflictReason::DestinationChanged,
            &destination,
            Some(expected_new_parent_revision),
            scope,
        ));
    }
    if node.parent_node_id() == Some(new_parent_node_id) {
        return Err(ClientMutationError::InvalidMutation);
    }
    if let Some(existing) = active_child_with_name(
        transaction,
        library_id,
        new_parent_node_id,
        node.name(),
        Some(node_id),
    )
    .await?
    {
        return Ok(node_conflict(
            MutationConflictReason::NameOccupied,
            &existing,
            None,
            scope,
        ));
    }
    let ancestors = load_ancestors(transaction, &destination)
        .await
        .map_err(map_client_metadata_error)?;
    node.validate_move_parent(&destination, &ancestors)
        .map_err(map_client_domain_error)?;
    node.move_to(&destination, observed_at)
        .map_err(map_client_domain_error)?;
    DomainRepository::update_node_in_transaction(transaction, &node)
        .await
        .map_err(map_client_metadata_error)?;
    Ok(ClientMutationDecision::Applied {
        node,
        change_kind: ChangeKind::NodeMoved,
    })
}

async fn prepare_trash_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: ClientMutationPrepareContext,
    node_id: NodeId,
    expected_revision: Revision,
) -> Result<ClientMutationDecision, ClientMutationError> {
    let ClientMutationPrepareContext {
        owner_user_id,
        library_id,
        scope,
        observed_at,
    } = context;
    let Some(mut node) = DomainRepository::lock_node_in_library(transaction, library_id, node_id)
        .await
        .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            node_id,
            Some(expected_revision),
            scope,
        )
        .await;
    };
    if node.revision() != expected_revision {
        return Ok(node_conflict(
            MutationConflictReason::RevisionMismatch,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.state() != NodeState::Active {
        return Ok(node_conflict(
            MutationConflictReason::NodeStateChanged,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.kind() == NodeKind::Directory {
        let has_children = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM nodes WHERE library_id = $1 AND parent_node_id = $2
            )",
        )
        .bind(library_id.into_uuid())
        .bind(node.id().into_uuid())
        .fetch_one(&mut **transaction)
        .await
        .map_err(map_client_sqlx_error)?;
        if has_children {
            return Err(ClientMutationError::InvalidMutation);
        }
    }
    node.transition_state(NodeState::Trashed, observed_at)
        .map_err(map_client_domain_error)?;
    DomainRepository::update_node_in_transaction(transaction, &node)
        .await
        .map_err(map_client_metadata_error)?;
    Ok(ClientMutationDecision::Applied {
        node,
        change_kind: ChangeKind::NodeTrashed,
    })
}

async fn prepare_restore_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: ClientMutationPrepareContext,
    node_id: NodeId,
    expected_revision: Revision,
    expected_parent_node_id: NodeId,
    expected_parent_revision: Revision,
) -> Result<ClientMutationDecision, ClientMutationError> {
    let ClientMutationPrepareContext {
        owner_user_id,
        library_id,
        scope,
        observed_at,
    } = context;
    let Some(mut node) = DomainRepository::lock_node_in_library(transaction, library_id, node_id)
        .await
        .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            node_id,
            Some(expected_revision),
            scope,
        )
        .await;
    };
    if node.revision() != expected_revision {
        return Ok(node_conflict(
            MutationConflictReason::RevisionMismatch,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.state() != NodeState::Trashed {
        return Ok(node_conflict(
            MutationConflictReason::NodeStateChanged,
            &node,
            Some(expected_revision),
            scope,
        ));
    }
    if node.parent_node_id() != Some(expected_parent_node_id) {
        return Ok(node_conflict(
            MutationConflictReason::ParentChanged,
            &node,
            Some(expected_parent_revision),
            scope,
        ));
    }
    let Some(parent) =
        DomainRepository::lock_node_in_library(transaction, library_id, expected_parent_node_id)
            .await
            .map_err(map_client_metadata_error)?
    else {
        return missing_client_resource(
            transaction,
            owner_user_id,
            library_id,
            expected_parent_node_id,
            Some(expected_parent_revision),
            scope,
        )
        .await;
    };
    if parent.revision() != expected_parent_revision
        || parent.kind() != NodeKind::Directory
        || parent.state() != NodeState::Active
    {
        return Ok(node_conflict(
            MutationConflictReason::ParentChanged,
            &parent,
            Some(expected_parent_revision),
            scope,
        ));
    }
    node.validate_parent_relationship(&parent)
        .map_err(map_client_domain_error)?;
    if let Some(existing) = active_child_with_name(
        transaction,
        library_id,
        expected_parent_node_id,
        node.name(),
        Some(node_id),
    )
    .await?
    {
        return Ok(node_conflict(
            MutationConflictReason::NameOccupied,
            &existing,
            None,
            scope,
        ));
    }
    node.transition_state(NodeState::Active, observed_at)
        .map_err(map_client_domain_error)?;
    DomainRepository::update_node_in_transaction(transaction, &node)
        .await
        .map_err(map_client_metadata_error)?;
    Ok(ClientMutationDecision::Applied {
        node,
        change_kind: ChangeKind::NodeRestored,
    })
}

fn map_client_domain_error(error: DomainError) -> ClientMutationError {
    match error {
        DomainError::RevisionOverflow | DomainError::InvalidTrashTimestamp => {
            ClientMutationError::InvalidPersistedData
        }
        _ => ClientMutationError::InvalidMutation,
    }
}

fn map_client_metadata_error(error: MetadataError) -> ClientMutationError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => ClientMutationError::DependencyUnavailable,
        MetadataError::Database(error) => ClientMutationError::Database(error),
        MetadataError::Mapping(_) => ClientMutationError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => ClientMutationError::DependencyUnavailable,
    }
}

fn map_client_sqlx_error(error: sqlx::Error) -> ClientMutationError {
    map_client_metadata_error(MetadataError::from(error))
}

fn gc_time_window(
    policy: ObjectGcPolicy,
    now: Timestamp,
) -> Result<(Timestamp, Timestamp), MetadataError> {
    let grace_cutoff = now
        .checked_sub_std(policy.grace_period())
        .ok_or(MetadataError::Mapping(MappingError::InvalidTimestamp {
            field: "object_gc_policy.grace_cutoff",
        }))?;
    let lease_expires_at =
        now.checked_add_std(policy.lease_duration())
            .ok_or(MetadataError::Mapping(MappingError::InvalidTimestamp {
                field: "object_gc_policy.lease_expiry",
            }))?;
    Ok((grace_cutoff, lease_expires_at))
}

fn same_gc_lease(left: ObjectGcLease, right: ObjectGcLease) -> bool {
    left.object_id() == right.object_id()
        && left.dedup_domain_id() == right.dedup_domain_id()
        && left.lease_id() == right.lease_id()
        && left.lease_generation() == right.lease_generation()
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
         FOR UPDATE OF fv",
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
