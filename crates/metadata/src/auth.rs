//! PostgreSQL persistence for password credentials and one-time bootstrap.

use sqlx::FromRow;
use synveil_core::{
    DedupDomainId, Library, LibraryId, LogicalName, LoginIdentifier, Node, NodeId, Timestamp,
    UserId,
};
use time::OffsetDateTime;

use crate::{
    DatabasePool, LibraryRow, MetadataError, NodeRow, SessionRow, UserCredentialRow, UserLoginRow,
    UserRow,
};

/// Durable bootstrap state. `Inconsistent` is intentionally not exposed as a
/// public setup status by the authentication service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapState {
    Open,
    Closed,
    Inconsistent,
}

/// Outcome of one first-administrator transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapAttempt {
    Created,
    AlreadyClosed,
    Inconsistent,
}

#[derive(Debug, FromRow)]
struct BootstrapStateRow {
    state: String,
    closed_at: Option<OffsetDateTime>,
}

/// Focused PostgreSQL adapter for the authentication foundation.
pub struct AuthRepository<'pool> {
    pool: &'pool DatabasePool,
}

impl<'pool> AuthRepository<'pool> {
    #[must_use]
    pub const fn new(pool: &'pool DatabasePool) -> Self {
        Self { pool }
    }

    pub async fn bootstrap_state(&self) -> Result<BootstrapState, MetadataError> {
        let row = sqlx::query_as::<_, BootstrapStateRow>(
            "SELECT state, closed_at
             FROM bootstrap_state
             WHERE id = 1",
        )
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        Ok(row.map_or(BootstrapState::Inconsistent, decode_bootstrap_state))
    }

    /// Serialize the first-admin race on the singleton bootstrap row. Every
    /// write that creates the user, credential, and closed marker shares this
    /// transaction and either commits together or rolls back together.
    pub async fn create_first_admin(
        &self,
        user: &UserRow,
        credential: &UserCredentialRow,
    ) -> Result<BootstrapAttempt, MetadataError> {
        if !user.is_instance_admin || user.id != credential.user_id {
            return Ok(BootstrapAttempt::Inconsistent);
        }

        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;

        let state = sqlx::query_as::<_, BootstrapStateRow>(
            "SELECT state, closed_at
             FROM bootstrap_state
             WHERE id = 1
             FOR UPDATE",
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;

        let Some(state) = state else {
            return Ok(BootstrapAttempt::Inconsistent);
        };

        match decode_bootstrap_state(state) {
            BootstrapState::Closed => return Ok(BootstrapAttempt::AlreadyClosed),
            BootstrapState::Inconsistent => return Ok(BootstrapAttempt::Inconsistent),
            BootstrapState::Open => {}
        }

        let users_exist: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
            .fetch_one(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
        if users_exist {
            return Ok(BootstrapAttempt::Inconsistent);
        }

        insert_user(&mut transaction, user).await?;
        insert_credential(&mut transaction, credential).await?;
        insert_initial_library(&mut transaction, user.id, user.updated_at).await?;

        let closed_at = user.updated_at;
        let closed = sqlx::query(
            "UPDATE bootstrap_state
             SET state = 'CLOSED', closed_at = $1
             WHERE id = 1 AND state = 'OPEN' AND closed_at IS NULL",
        )
        .bind(closed_at)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if closed.rows_affected() != 1 {
            return Ok(BootstrapAttempt::Inconsistent);
        }

        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(BootstrapAttempt::Created)
    }

    pub async fn load_credential(
        &self,
        user_id: synveil_core::UserId,
    ) -> Result<Option<UserCredentialRow>, MetadataError> {
        sqlx::query_as::<_, UserCredentialRow>(
            "SELECT user_id, password_hash, created_at, updated_at
             FROM user_credentials
             WHERE user_id = $1",
        )
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
    }

    /// Look up a login using only the adapter-supplied uniqueness key. The
    /// authentication service decides the public failure semantics; this
    /// adapter never normalizes or derives a key from the display value.
    pub async fn load_login(
        &self,
        login: &LoginIdentifier,
    ) -> Result<Option<UserLoginRow>, MetadataError> {
        self.load_login_by_key(login.uniqueness_key()).await
    }

    pub async fn load_login_by_key(
        &self,
        login_key: &str,
    ) -> Result<Option<UserLoginRow>, MetadataError> {
        sqlx::query_as::<_, UserLoginRow>(
            "SELECT u.id AS user_id, u.login_value, u.login_key, u.status,
                    u.is_instance_admin, u.created_at AS user_created_at,
                    u.updated_at AS user_updated_at,
                    u.revision::TEXT AS user_revision,
                    c.password_hash,
                    c.created_at AS credential_created_at,
                    c.updated_at AS credential_updated_at
             FROM users AS u
             LEFT JOIN user_credentials AS c ON c.user_id = u.id
             WHERE u.login_key = $1",
        )
        .bind(login_key)
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
    }

    pub async fn load_user(&self, user_id: UserId) -> Result<Option<UserRow>, MetadataError> {
        sqlx::query_as::<_, UserRow>(
            "SELECT id, login_value, login_key, status, is_instance_admin,
                    created_at, updated_at, revision::TEXT AS revision
             FROM users
             WHERE id = $1",
        )
        .bind(user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
    }

    pub async fn create_session(&self, row: &SessionRow) -> Result<(), MetadataError> {
        // Keep creation behind an explicit transaction boundary so this
        // adapter remains safe if session-side audit/epoch facts are added by
        // an accepted contract later. The FK and insert commit together.
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        sqlx::query(
            "INSERT INTO sessions
                (id, user_id, token_digest, created_at, expires_at, revoked_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(row.id)
        .bind(row.user_id)
        .bind(&row.token_digest)
        .bind(row.created_at)
        .bind(row.expires_at)
        .bind(row.revoked_at)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;

        transaction.commit().await.map_err(MetadataError::from)
    }

    pub async fn load_session_by_token_digest(
        &self,
        token_digest: &[u8],
    ) -> Result<Option<SessionRow>, MetadataError> {
        sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, token_digest, created_at, expires_at, revoked_at
             FROM sessions
             WHERE token_digest = $1",
        )
        .bind(token_digest)
        .fetch_optional(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
    }

    pub async fn count_sessions_for_user(&self, user_id: UserId) -> Result<i64, MetadataError> {
        sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
            .bind(user_id.into_uuid())
            .fetch_one(self.pool.sqlx_pool())
            .await
            .map_err(MetadataError::from)
    }

    pub async fn revoke_session(
        &self,
        session_id: uuid::Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "UPDATE sessions
             SET revoked_at = COALESCE(revoked_at, $2)
             WHERE id = $1",
        )
        .bind(session_id)
        .bind(revoked_at)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    pub async fn cleanup_sessions(
        &self,
        observed_at: OffsetDateTime,
    ) -> Result<u64, MetadataError> {
        sqlx::query(
            "DELETE FROM sessions
             WHERE expires_at <= $1 OR revoked_at IS NOT NULL",
        )
        .bind(observed_at)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|result| result.rows_affected())
    }

    pub async fn replace_password_hash(
        &self,
        user_id: synveil_core::UserId,
        password_hash: &str,
        updated_at: OffsetDateTime,
    ) -> Result<bool, MetadataError> {
        let result = sqlx::query(
            "UPDATE user_credentials
             SET password_hash = $2, updated_at = $3
             WHERE user_id = $1",
        )
        .bind(user_id.into_uuid())
        .bind(password_hash)
        .bind(updated_at)
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;

        Ok(result.rows_affected() == 1)
    }
}

fn decode_bootstrap_state(row: BootstrapStateRow) -> BootstrapState {
    match (row.state.as_str(), row.closed_at) {
        ("OPEN", None) => BootstrapState::Open,
        ("CLOSED", Some(_)) => BootstrapState::Closed,
        _ => BootstrapState::Inconsistent,
    }
}

async fn insert_user(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &UserRow,
) -> Result<(), MetadataError> {
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
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

async fn insert_credential(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &UserCredentialRow,
) -> Result<(), MetadataError> {
    sqlx::query(
        "INSERT INTO user_credentials (user_id, password_hash, created_at, updated_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(row.user_id)
    .bind(&row.password_hash)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

/// The one-time bootstrap creates exactly one owner library and root together
/// with the first administrator. This gives authenticated metadata operations
/// a deterministic initial namespace without introducing multi-library policy.
async fn insert_initial_library(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_user_id: uuid::Uuid,
    observed_at: OffsetDateTime,
) -> Result<(), MetadataError> {
    let owner_user_id = UserId::try_from_uuid(owner_user_id).map_err(|_| {
        MetadataError::Mapping(crate::MappingError::InvalidId {
            field: "users.id",
            reason: synveil_core::IdParseError::InvalidUuid,
        })
    })?;
    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        LogicalName::new("root")?,
        Timestamp::from_offset_datetime(observed_at),
    );
    let library = Library::new(
        library_id,
        owner_user_id,
        LogicalName::new("Primary")?,
        &root,
        DedupDomainId::new(),
        Timestamp::from_offset_datetime(observed_at),
    )?;
    let library = LibraryRow::from_domain(&library)?;
    let root = NodeRow::from_domain(&root)?;

    sqlx::query(
        "INSERT INTO libraries
            (id, owner_user_id, name, root_node_id, dedup_domain_id, status,
             created_at, updated_at, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::NUMERIC)",
    )
    .bind(library.id)
    .bind(library.owner_user_id)
    .bind(&library.name)
    .bind(library.root_node_id)
    .bind(library.dedup_domain_id)
    .bind(&library.status)
    .bind(library.created_at)
    .bind(library.updated_at)
    .bind(&library.revision)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;

    sqlx::query(
        "INSERT INTO nodes
            (id, library_id, parent_node_id, kind, name, current_version_id,
             state, created_at, updated_at, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::NUMERIC)",
    )
    .bind(root.id)
    .bind(root.library_id)
    .bind(root.parent_node_id)
    .bind(&root.kind)
    .bind(&root.name)
    .bind(root.current_version_id)
    .bind(&root.state)
    .bind(root.created_at)
    .bind(root.updated_at)
    .bind(&root.revision)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}
