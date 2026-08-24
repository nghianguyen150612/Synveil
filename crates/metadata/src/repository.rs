//! Focused PostgreSQL repositories for the initial canonical domain.

use synveil_core::{
    Device, DomainError, FileVersion, Library, LibraryId, LibraryStatus, LogicalName, Node, NodeId,
    NodeKind, NodeState, ObjectReference, Revision, Timestamp, User, UserId,
};
use uuid::Uuid;

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
                    n.current_version_id, n.state, n.created_at, n.updated_at,
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

    pub(crate) async fn find_owned_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<Node>, MetadataError> {
        let row = sqlx::query_as::<_, NodeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.created_at, n.updated_at,
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

    pub async fn insert_node(&self, value: &Node) -> Result<(), MetadataError> {
        self.insert_node_row(&NodeRow::from_domain(value)?).await
    }

    pub async fn insert_node_row(&self, row: &NodeRow) -> Result<(), MetadataError> {
        sqlx::query(
            "INSERT INTO nodes
                (id, library_id, parent_node_id, kind, name, current_version_id,
                 state, created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::NUMERIC)",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
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
                 current_version_id = $6, state = $7, created_at = $8,
                 updated_at = $9, revision = $10::NUMERIC
             WHERE id = $1",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
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
        let row = FileVersionRow::from_domain(value)?;
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
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
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
                    state, created_at, updated_at, revision::TEXT AS revision
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
                    n.current_version_id, n.state, n.created_at, n.updated_at,
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
                 state, created_at, updated_at, revision)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::NUMERIC)",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
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
                 current_version_id = $6, state = $7, created_at = $8,
                 updated_at = $9, revision = $10::NUMERIC
             WHERE id = $1",
        )
        .bind(row.id)
        .bind(row.library_id)
        .bind(row.parent_node_id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(row.current_version_id)
        .bind(&row.state)
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(&row.revision)
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
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
                    state, created_at, updated_at, revision::TEXT AS revision
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
                    state, created_at, updated_at, revision::TEXT AS revision
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

    async fn load_owned_node(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Option<Node>, MetadataError> {
        let row = sqlx::query_as::<_, NodeRow>(
            "SELECT n.id, n.library_id, n.parent_node_id, n.kind, n.name,
                    n.current_version_id, n.state, n.created_at, n.updated_at,
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

fn ensure_library_writable(library: &Library) -> Result<(), MetadataError> {
    if library.status() != LibraryStatus::Active {
        return Err(MetadataError::Mapping(MappingError::Domain(
            DomainError::LibraryNotWritable,
        )));
    }
    Ok(())
}
