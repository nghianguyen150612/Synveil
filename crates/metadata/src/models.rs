//! PostgreSQL row representations owned by the metadata adapter.
//!
//! These structs deliberately do not appear on the domain entities. Numeric
//! unsigned values are selected as canonical decimal text because PostgreSQL
//! `NUMERIC` is wider than the Rust primitive and SQLx must not silently narrow
//! it before the explicit mapping boundary validates the value.

use std::fmt;

use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

/// A row from `users`, with the unsigned revision selected as decimal text.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct UserRow {
    pub id: Uuid,
    pub login_value: String,
    pub login_key: String,
    pub status: String,
    pub is_instance_admin: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revision: String,
}

/// A password credential row. Its debug representation is deliberately
/// redacted because the PHC string is still authentication secret material.
#[derive(Clone, FromRow, PartialEq, Eq)]
pub struct UserCredentialRow {
    pub user_id: Uuid,
    pub password_hash: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl UserCredentialRow {
    #[must_use]
    pub const fn new(
        user_id: Uuid,
        password_hash: String,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Self {
        Self {
            user_id,
            password_hash,
            created_at,
            updated_at,
        }
    }
}

impl fmt::Debug for UserCredentialRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserCredentialRow")
            .field("user_id", &self.user_id)
            .field("password_hash", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// Joined login lookup row. The optional credential fields represent a user
/// without a password credential; callers must not turn that distinction into
/// a public authentication error.
#[derive(Clone, FromRow, PartialEq, Eq)]
pub struct UserLoginRow {
    pub user_id: Uuid,
    pub login_value: String,
    pub login_key: String,
    pub status: String,
    pub is_instance_admin: bool,
    pub user_created_at: OffsetDateTime,
    pub user_updated_at: OffsetDateTime,
    pub user_revision: String,
    pub password_hash: Option<String>,
    pub credential_created_at: Option<OffsetDateTime>,
    pub credential_updated_at: Option<OffsetDateTime>,
}

impl UserLoginRow {
    #[must_use]
    pub fn user_row(&self) -> UserRow {
        UserRow {
            id: self.user_id,
            login_value: self.login_value.clone(),
            login_key: self.login_key.clone(),
            status: self.status.clone(),
            is_instance_admin: self.is_instance_admin,
            created_at: self.user_created_at,
            updated_at: self.user_updated_at,
            revision: self.user_revision.clone(),
        }
    }

    #[must_use]
    pub fn credential_row(&self) -> Option<UserCredentialRow> {
        match (
            self.password_hash.as_ref(),
            self.credential_created_at,
            self.credential_updated_at,
        ) {
            (Some(password_hash), Some(created_at), Some(updated_at)) => Some(
                UserCredentialRow::new(self.user_id, password_hash.clone(), created_at, updated_at),
            ),
            _ => None,
        }
    }
}

impl fmt::Debug for UserLoginRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserLoginRow")
            .field("user_id", &self.user_id)
            .field("login_value", &self.login_value)
            .field("login_key", &"[REDACTED]")
            .field("status", &self.status)
            .field("is_instance_admin", &self.is_instance_admin)
            .field("user_created_at", &self.user_created_at)
            .field("user_updated_at", &self.user_updated_at)
            .field("user_revision", &self.user_revision)
            .field("password_hash", &"[REDACTED]")
            .field("credential_created_at", &self.credential_created_at)
            .field("credential_updated_at", &self.credential_updated_at)
            .finish()
    }
}

/// Persisted browser-session metadata. `token_digest` is a one-way verifier,
/// never the raw bearer token; its debug representation is redacted as well.
#[derive(Clone, FromRow, PartialEq, Eq)]
pub struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_digest: Vec<u8>,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl SessionRow {
    #[must_use]
    pub const fn new(
        id: Uuid,
        user_id: Uuid,
        token_digest: Vec<u8>,
        created_at: OffsetDateTime,
        expires_at: OffsetDateTime,
        revoked_at: Option<OffsetDateTime>,
    ) -> Self {
        Self {
            id,
            user_id,
            token_digest,
            created_at,
            expires_at,
            revoked_at,
        }
    }
}

/// A durable per-device/per-library synchronization checkpoint. Journal
/// payloads are intentionally not stored here; this row records only consumer
/// progress and server-observed timestamps.
#[derive(Clone, Debug, FromRow, PartialEq, Eq)]
pub struct DeviceSyncCheckpointRow {
    pub owner_user_id: Uuid,
    pub device_id: Uuid,
    pub library_id: Uuid,
    pub journal_epoch: i64,
    pub acknowledged_sequence: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub last_seen_high_watermark: Option<i64>,
}

impl fmt::Debug for SessionRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionRow")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("token_digest", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("revoked_at", &self.revoked_at)
            .finish()
    }
}

/// A row from `devices`, with the unsigned revision selected as decimal text.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct DeviceRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub display_name: String,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revision: String,
}

/// A row from `libraries`, with the unsigned revision selected as decimal
/// text. The root node is loaded separately so its domain invariants can be
/// rechecked during rehydration.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct LibraryRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub name: String,
    pub root_node_id: Uuid,
    pub dedup_domain_id: Uuid,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revision: String,
}

/// A row from `nodes`, with the unsigned revision selected as decimal text.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct NodeRow {
    pub id: Uuid,
    pub library_id: Uuid,
    pub parent_node_id: Option<Uuid>,
    pub kind: String,
    pub name: String,
    pub current_version_id: Option<Uuid>,
    pub state: String,
    pub trashed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revision: String,
}

/// A row from `objects`. The digest is stored as exactly 32 raw bytes rather
/// than as an ad-hoc textual hash column.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct ObjectRow {
    pub id: Uuid,
    pub dedup_domain_id: Uuid,
    pub canonical_hash: Vec<u8>,
    pub plaintext_length: String,
    pub created_at: OffsetDateTime,
}

/// A row from `file_versions`. Object integrity metadata is joined from the
/// separate `ObjectRow` so object identity remains independent of filenames.
#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct FileVersionRow {
    pub id: Uuid,
    pub library_id: Uuid,
    pub node_id: Uuid,
    pub object_id: Uuid,
    pub object_dedup_domain_id: Uuid,
    pub parent_version_id: Option<Uuid>,
    pub committed_at: OffsetDateTime,
    pub revision: String,
}

/// The initial verified physical representation for one canonical object.
/// `storage_key` and `backend_version` are opaque adapter evidence; they are
/// never treated as user paths or authorization inputs.
#[derive(Clone, FromRow, PartialEq)]
pub struct ObjectReplicaRow {
    pub id: Uuid,
    pub object_id: Uuid,
    pub object_dedup_domain_id: Uuid,
    pub backend_kind: String,
    pub storage_key: String,
    pub stored_length: String,
    pub stored_sha256: Vec<u8>,
    pub backend_version: Option<String>,
    pub state: String,
    pub created_at: OffsetDateTime,
    pub verified_at: OffsetDateTime,
}

impl std::fmt::Debug for ObjectReplicaRow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ObjectReplicaRow")
            .field("id", &self.id)
            .field("object_id", &self.object_id)
            .field("object_dedup_domain_id", &self.object_dedup_domain_id)
            .field("backend_kind", &self.backend_kind)
            .field("storage_key", &"<redacted>")
            .field("stored_length", &self.stored_length)
            .field("stored_sha256", &"<redacted>")
            .field(
                "backend_version",
                &self.backend_version.as_ref().map(|_| "<redacted>"),
            )
            .field("state", &self.state)
            .field("created_at", &self.created_at)
            .field("verified_at", &self.verified_at)
            .finish()
    }
}

/// A persisted resumable upload session. The raw physical path is never part
/// of this row; storage identity fields are opaque handles/keys generated by
/// the application and redacted by the application-facing view.
#[derive(Clone, FromRow, PartialEq)]
pub struct UploadSessionRow {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub library_id: Uuid,
    pub operation: String,
    pub target_node_id: Uuid,
    pub target_parent_node_id: Option<Uuid>,
    pub target_name: Option<String>,
    pub expected_node_revision: Option<String>,
    pub expected_length: String,
    pub expected_sha256: Option<Vec<u8>>,
    pub object_id: Uuid,
    pub object_replica_id: Uuid,
    pub object_key: String,
    pub staging_handle: String,
    pub bytes_received: String,
    pub state: String,
    pub lease_generation: String,
    pub lease_expires_at: Option<OffsetDateTime>,
    pub cancel_requested_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub last_error_code: Option<String>,
    pub terminal_failure_code: Option<String>,
    pub durability_backend_kind: Option<String>,
    pub durability_backend_version: Option<String>,
    pub durability_length: Option<String>,
    pub durability_sha256: Option<Vec<u8>>,
    pub durable_at: Option<OffsetDateTime>,
    pub completed_node_id: Option<Uuid>,
    pub completed_file_version_id: Option<Uuid>,
    pub completed_object_id: Option<Uuid>,
    pub completed_object_replica_id: Option<Uuid>,
    pub completed_node_revision: Option<String>,
    pub completed_at: Option<OffsetDateTime>,
}

impl std::fmt::Debug for UploadSessionRow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UploadSessionRow")
            .field("id", &self.id)
            .field("owner_user_id", &self.owner_user_id)
            .field("library_id", &self.library_id)
            .field("operation", &self.operation)
            .field("target_node_id", &self.target_node_id)
            .field("target_parent_node_id", &self.target_parent_node_id)
            .field("target_name", &self.target_name)
            .field("expected_node_revision", &self.expected_node_revision)
            .field("expected_length", &self.expected_length)
            .field(
                "expected_sha256",
                &self.expected_sha256.as_ref().map(|_| "<redacted>"),
            )
            .field("object_id", &self.object_id)
            .field("object_replica_id", &self.object_replica_id)
            .field("object_key", &"<redacted>")
            .field("staging_handle", &"<redacted>")
            .field("bytes_received", &self.bytes_received)
            .field("state", &self.state)
            .field("lease_generation", &self.lease_generation)
            .field("lease_expires_at", &self.lease_expires_at)
            .field("cancel_requested_at", &self.cancel_requested_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("expires_at", &self.expires_at)
            .field("last_error_code", &self.last_error_code)
            .field("terminal_failure_code", &self.terminal_failure_code)
            .field("durability_backend_kind", &self.durability_backend_kind)
            .field(
                "durability_backend_version",
                &self
                    .durability_backend_version
                    .as_ref()
                    .map(|_| "<redacted>"),
            )
            .field("durability_length", &self.durability_length)
            .field(
                "durability_sha256",
                &self.durability_sha256.as_ref().map(|_| "<redacted>"),
            )
            .field("durable_at", &self.durable_at)
            .field("completed_node_id", &self.completed_node_id)
            .field("completed_file_version_id", &self.completed_file_version_id)
            .field("completed_object_id", &self.completed_object_id)
            .field(
                "completed_object_replica_id",
                &self.completed_object_replica_id,
            )
            .field("completed_node_revision", &self.completed_node_revision)
            .field("completed_at", &self.completed_at)
            .finish()
    }
}
