//! Focused PostgreSQL repositories for the initial canonical domain.

use synveil_core::{Device, FileVersion, Library, Node, ObjectReference, User};
use uuid::Uuid;

use crate::{
    DatabasePool, DeviceRow, FileVersionRow, LibraryRow, MappingError, MetadataError, NodeRow,
    ObjectRow, UserRow,
};

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
}
