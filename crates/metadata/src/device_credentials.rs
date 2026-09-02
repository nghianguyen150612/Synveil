//! Digest-only device credentials and one-time enrollment persistence.
//!
//! All writes lock owner, Device, then grant/credential in that order. This
//! serializes exchange with revocation and prevents one grant from issuing two
//! credentials. A failed transaction leaves activation and consumption undone.

use std::fmt;

use sqlx::{FromRow, Postgres, Transaction};
use subtle::ConstantTimeEq;
use synveil_core::{
    Device, DeviceCredentialId, DeviceEnrollmentGrantId, DeviceId, DeviceStatus, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{DatabasePool, DeviceRow, MetadataError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceCredentialRepositoryError {
    InvalidEnrollment,
    DeviceNotFound,
    InvalidPersistedData,
    Persistence(MetadataError),
}

impl From<MetadataError> for DeviceCredentialRepositoryError {
    fn from(error: MetadataError) -> Self {
        Self::Persistence(error)
    }
}

impl From<sqlx::Error> for DeviceCredentialRepositoryError {
    fn from(error: sqlx::Error) -> Self {
        Self::Persistence(MetadataError::from(error))
    }
}

impl fmt::Display for DeviceCredentialRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEnrollment => "invalid_device_enrollment",
            Self::DeviceNotFound => "device_not_found",
            Self::InvalidPersistedData => "device_credential_persisted_data_invalid",
            Self::Persistence(_) => "device_credential_persistence_unavailable",
        })
    }
}

impl std::error::Error for DeviceCredentialRepositoryError {}

/// All fields are public metadata except the verifier, which is still
/// redacted. There is deliberately no field capable of holding a raw token.
#[derive(Clone)]
pub struct NewDeviceEnrollmentGrant {
    pub grant_id: DeviceEnrollmentGrantId,
    pub owner_user_id: UserId,
    pub device_id: DeviceId,
    pub secret_digest: [u8; 32],
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

impl fmt::Debug for NewDeviceEnrollmentGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewDeviceEnrollmentGrant")
            .field("grant_id", &self.grant_id)
            .field("owner_user_id", &self.owner_user_id)
            .field("device_id", &self.device_id)
            .field("secret_digest", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone)]
pub struct StoredDeviceCredential {
    pub credential_id: DeviceCredentialId,
    pub owner_user_id: UserId,
    pub device_id: DeviceId,
    pub secret_digest: [u8; 32],
    pub created_at: Timestamp,
    pub revoked_at: Option<Timestamp>,
    pub owner_active: bool,
    pub device_status: DeviceStatus,
}

impl fmt::Debug for StoredDeviceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredDeviceCredential")
            .field("credential_id", &self.credential_id)
            .field("owner_user_id", &self.owner_user_id)
            .field("device_id", &self.device_id)
            .field("secret_digest", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("revoked_at", &self.revoked_at)
            .field("owner_active", &self.owner_active)
            .field("device_status", &self.device_status)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsumedDeviceEnrollment {
    pub owner_user_id: UserId,
    pub device_id: DeviceId,
    pub credential_id: DeviceCredentialId,
    pub created_at: Timestamp,
}

#[derive(FromRow)]
struct EnrollmentRow {
    grant_id: Uuid,
    owner_user_id: Uuid,
    device_id: Uuid,
    secret_digest: Vec<u8>,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
    consumed_at: Option<OffsetDateTime>,
    revoked_at: Option<OffsetDateTime>,
}

#[derive(FromRow)]
struct CredentialRow {
    credential_id: Uuid,
    owner_user_id: Uuid,
    device_id: Uuid,
    secret_digest: Vec<u8>,
    created_at: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
    owner_active: bool,
    device_status: String,
}

/// Focused adapter used by the existing authentication crate. Plaintext
/// credentials and tokens never cross this persistence interface.
pub struct DeviceCredentialRepository<'pool> {
    pool: &'pool DatabasePool,
}

impl<'pool> DeviceCredentialRepository<'pool> {
    #[must_use]
    pub const fn new(pool: &'pool DatabasePool) -> Self {
        Self { pool }
    }

    /// A new canonical PENDING Device and its grant commit together. Existing
    /// devices must belong to the authenticated owner and be PENDING/ACTIVE.
    pub async fn create_grant(
        &self,
        grant: &NewDeviceEnrollmentGrant,
        new_device: Option<&Device>,
    ) -> Result<(), DeviceCredentialRepositoryError> {
        let ttl = grant.expires_at.as_offset_datetime() - grant.created_at.as_offset_datetime();
        if ttl <= time::Duration::ZERO || ttl > time::Duration::minutes(15) {
            return Err(DeviceCredentialRepositoryError::InvalidEnrollment);
        }
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        lock_active_owner(&mut transaction, grant.owner_user_id).await?;
        if let Some(device) = new_device {
            if device.id() != grant.device_id
                || device.owner_user_id() != grant.owner_user_id
                || device.status() != DeviceStatus::Pending
                || device.created_at() != grant.created_at
            {
                return Err(DeviceCredentialRepositoryError::InvalidEnrollment);
            }
            insert_device(&mut transaction, device).await?;
        }
        let device = lock_device(&mut transaction, grant.owner_user_id, grant.device_id).await?;
        if !matches!(
            device.status(),
            DeviceStatus::Pending | DeviceStatus::Active
        ) || grant.created_at < device.updated_at()
        {
            return Err(DeviceCredentialRepositoryError::DeviceNotFound);
        }
        sqlx::query(
            "INSERT INTO device_enrollment_grants
                (grant_id, owner_user_id, device_id, secret_digest, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(grant.grant_id.into_uuid())
        .bind(grant.owner_user_id.into_uuid())
        .bind(grant.device_id.into_uuid())
        .bind(grant.secret_digest.as_slice())
        .bind(grant.created_at.as_offset_datetime())
        .bind(grant.expires_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Consume once and insert one digest-only credential in one transaction.
    /// Replaying a consumed token never returns or recreates a bearer secret.
    pub async fn consume_grant(
        &self,
        grant_digest: &[u8; 32],
        credential_id: DeviceCredentialId,
        credential_digest: &[u8; 32],
        observed_at: Timestamp,
    ) -> Result<ConsumedDeviceEnrollment, DeviceCredentialRepositoryError> {
        self.consume_grant_with_clock(grant_digest, credential_id, credential_digest, || {
            observed_at
        })
        .await
    }

    /// The production caller supplies its server clock, evaluated only after
    /// owner/Device/grant locks are held. Time spent waiting cannot extend a
    /// grant's lifetime. The explicit timestamp wrapper above remains useful
    /// for deterministic persistence fixtures.
    pub async fn consume_grant_with_clock(
        &self,
        grant_digest: &[u8; 32],
        credential_id: DeviceCredentialId,
        credential_digest: &[u8; 32],
        clock: impl FnOnce() -> Timestamp + Send,
    ) -> Result<ConsumedDeviceEnrollment, DeviceCredentialRepositoryError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        // This first read locates the lock scope only. Authority is rechecked
        // below after owner/Device locks; it is not an unlocked consume.
        let initial = load_grant(&mut transaction, grant_digest, false)
            .await?
            .ok_or(DeviceCredentialRepositoryError::InvalidEnrollment)?;
        let owner = UserId::try_from_uuid(initial.owner_user_id)
            .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
        let device_id = DeviceId::try_from_uuid(initial.device_id)
            .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
        lock_active_owner(&mut transaction, owner)
            .await
            .map_err(enrollment_scope_error)?;
        let mut device = lock_device(&mut transaction, owner, device_id)
            .await
            .map_err(enrollment_scope_error)?;
        let grant = load_grant(&mut transaction, grant_digest, true)
            .await?
            .ok_or(DeviceCredentialRepositoryError::InvalidEnrollment)?;
        let observed_at = clock();
        if grant.grant_id != initial.grant_id
            || grant.owner_user_id != owner.into_uuid()
            || grant.device_id != device_id.into_uuid()
            || !bool::from(grant.secret_digest.as_slice().ct_eq(grant_digest))
            || grant.consumed_at.is_some()
            || grant.revoked_at.is_some()
            || observed_at.as_offset_datetime() < grant.created_at
            || observed_at.as_offset_datetime() >= grant.expires_at
            || observed_at < device.updated_at()
            || !matches!(
                device.status(),
                DeviceStatus::Pending | DeviceStatus::Active
            )
        {
            return Err(DeviceCredentialRepositoryError::InvalidEnrollment);
        }
        if device.status() == DeviceStatus::Pending {
            device
                .transition_status(DeviceStatus::Active, observed_at)
                .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
            update_device(&mut transaction, &device).await?;
        }
        sqlx::query(
            "INSERT INTO device_credentials
                (credential_id, owner_user_id, device_id, secret_digest, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(credential_id.into_uuid())
        .bind(owner.into_uuid())
        .bind(device_id.into_uuid())
        .bind(credential_digest.as_slice())
        .bind(observed_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?;
        let consumed = sqlx::query(
            "UPDATE device_enrollment_grants
             SET consumed_at = $1, consumed_credential_id = $2
             WHERE grant_id = $3 AND consumed_at IS NULL AND revoked_at IS NULL",
        )
        .bind(observed_at.as_offset_datetime())
        .bind(credential_id.into_uuid())
        .bind(grant.grant_id)
        .execute(&mut *transaction)
        .await?;
        if consumed.rows_affected() != 1 {
            return Err(DeviceCredentialRepositoryError::InvalidEnrollment);
        }
        transaction.commit().await?;
        Ok(ConsumedDeviceEnrollment {
            owner_user_id: owner,
            device_id,
            credential_id,
            created_at: observed_at,
        })
    }

    /// No positive cache and no last-used write on the authentication path.
    /// Device, owner and credential state are read afresh for every request.
    pub async fn load_credential_by_digest(
        &self,
        digest: &[u8; 32],
    ) -> Result<Option<StoredDeviceCredential>, DeviceCredentialRepositoryError> {
        let row = sqlx::query_as::<_, CredentialRow>(
            "SELECT c.credential_id, c.owner_user_id, c.device_id, c.secret_digest,
                    c.created_at, c.revoked_at, u.status = 'ACTIVE' AS owner_active,
                    d.status AS device_status
             FROM device_credentials AS c
             JOIN devices AS d ON d.id = c.device_id AND d.owner_user_id = c.owner_user_id
             JOIN users AS u ON u.id = c.owner_user_id
             WHERE c.secret_digest = $1 AND c.format_version = 1",
        )
        .bind(digest.as_slice())
        .fetch_optional(self.pool.sqlx_pool())
        .await?;
        row.map(decode_credential).transpose()
    }

    pub async fn revoke_credential(
        &self,
        owner: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        observed_at: Timestamp,
    ) -> Result<(), DeviceCredentialRepositoryError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        lock_active_owner(&mut transaction, owner).await?;
        lock_device(&mut transaction, owner, device_id).await?;
        let result = sqlx::query(
            "UPDATE device_credentials
             SET revoked_at = COALESCE(revoked_at, GREATEST($1, created_at))
             WHERE credential_id = $2 AND device_id = $3 AND owner_user_id = $4",
        )
        .bind(observed_at.as_offset_datetime())
        .bind(credential_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(owner.into_uuid())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(DeviceCredentialRepositoryError::DeviceNotFound);
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Recovery when the once-only exchange response was lost. Invalidate all
    /// credentials and outstanding grants while leaving the Device enrollable.
    pub async fn revoke_all_credentials(
        &self,
        owner: UserId,
        device_id: DeviceId,
        observed_at: Timestamp,
    ) -> Result<(), DeviceCredentialRepositoryError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        lock_active_owner(&mut transaction, owner).await?;
        lock_device(&mut transaction, owner, device_id).await?;
        revoke_all(&mut transaction, owner, device_id, observed_at).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn revoke_device(
        &self,
        owner: UserId,
        device_id: DeviceId,
        observed_at: Timestamp,
    ) -> Result<(), DeviceCredentialRepositoryError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        lock_active_owner(&mut transaction, owner).await?;
        let mut device = lock_device(&mut transaction, owner, device_id).await?;
        let transition_at = observed_at.max(device.updated_at());
        device
            .transition_status(DeviceStatus::Revoked, transition_at)
            .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
        update_device(&mut transaction, &device).await?;
        revoke_all(&mut transaction, owner, device_id, observed_at).await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn enrollment_scope_error(
    error: DeviceCredentialRepositoryError,
) -> DeviceCredentialRepositoryError {
    match error {
        DeviceCredentialRepositoryError::DeviceNotFound => {
            DeviceCredentialRepositoryError::InvalidEnrollment
        }
        error => error,
    }
}

async fn lock_active_owner(
    transaction: &mut Transaction<'_, Postgres>,
    owner: UserId,
) -> Result<(), DeviceCredentialRepositoryError> {
    let active: Option<bool> =
        sqlx::query_scalar("SELECT status = 'ACTIVE' FROM users WHERE id = $1 FOR SHARE")
            .bind(owner.into_uuid())
            .fetch_optional(&mut **transaction)
            .await?;
    if active != Some(true) {
        return Err(DeviceCredentialRepositoryError::DeviceNotFound);
    }
    Ok(())
}

async fn lock_device(
    transaction: &mut Transaction<'_, Postgres>,
    owner: UserId,
    device_id: DeviceId,
) -> Result<Device, DeviceCredentialRepositoryError> {
    sqlx::query_as::<_, DeviceRow>(
        "SELECT id, owner_user_id, display_name, status, created_at, updated_at,
                revision::TEXT AS revision
         FROM devices WHERE id = $1 AND owner_user_id = $2 FOR UPDATE",
    )
    .bind(device_id.into_uuid())
    .bind(owner.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(DeviceCredentialRepositoryError::DeviceNotFound)?
    .try_into_domain()
    .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)
}

async fn insert_device(
    transaction: &mut Transaction<'_, Postgres>,
    device: &Device,
) -> Result<(), DeviceCredentialRepositoryError> {
    let row = DeviceRow::from_domain(device)
        .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
    sqlx::query(
        "INSERT INTO devices
            (id, owner_user_id, display_name, status, created_at, updated_at, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7::NUMERIC)",
    )
    .bind(row.id)
    .bind(row.owner_user_id)
    .bind(row.display_name)
    .bind(row.status)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(row.revision)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn update_device(
    transaction: &mut Transaction<'_, Postgres>,
    device: &Device,
) -> Result<(), DeviceCredentialRepositoryError> {
    let row = DeviceRow::from_domain(device)
        .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
    let result = sqlx::query(
        "UPDATE devices SET status = $1, updated_at = $2, revision = $3::NUMERIC
         WHERE id = $4 AND owner_user_id = $5",
    )
    .bind(row.status)
    .bind(row.updated_at)
    .bind(row.revision)
    .bind(row.id)
    .bind(row.owner_user_id)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(DeviceCredentialRepositoryError::InvalidPersistedData);
    }
    Ok(())
}

async fn load_grant(
    transaction: &mut Transaction<'_, Postgres>,
    digest: &[u8; 32],
    lock: bool,
) -> Result<Option<EnrollmentRow>, DeviceCredentialRepositoryError> {
    let statement = if lock {
        "SELECT grant_id, owner_user_id, device_id, secret_digest, created_at,
                expires_at, consumed_at, revoked_at
         FROM device_enrollment_grants WHERE secret_digest = $1 AND format_version = 1 FOR UPDATE"
    } else {
        "SELECT grant_id, owner_user_id, device_id, secret_digest, created_at,
                expires_at, consumed_at, revoked_at
         FROM device_enrollment_grants WHERE secret_digest = $1 AND format_version = 1"
    };
    sqlx::query_as::<_, EnrollmentRow>(statement)
        .bind(digest.as_slice())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(Into::into)
}

async fn revoke_all(
    transaction: &mut Transaction<'_, Postgres>,
    owner: UserId,
    device_id: DeviceId,
    observed_at: Timestamp,
) -> Result<(), DeviceCredentialRepositoryError> {
    sqlx::query(
        "UPDATE device_credentials
         SET revoked_at = GREATEST($1, created_at)
         WHERE owner_user_id = $2 AND device_id = $3 AND revoked_at IS NULL",
    )
    .bind(observed_at.as_offset_datetime())
    .bind(owner.into_uuid())
    .bind(device_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE device_enrollment_grants
         SET revoked_at = GREATEST($1, created_at)
         WHERE owner_user_id = $2 AND device_id = $3 AND revoked_at IS NULL AND consumed_at IS NULL",
    )
    .bind(observed_at.as_offset_datetime())
    .bind(owner.into_uuid())
    .bind(device_id.into_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn decode_credential(
    row: CredentialRow,
) -> Result<StoredDeviceCredential, DeviceCredentialRepositoryError> {
    let invalid = |_| DeviceCredentialRepositoryError::InvalidPersistedData;
    let credential_id = DeviceCredentialId::try_from_uuid(row.credential_id).map_err(invalid)?;
    let owner_user_id = UserId::try_from_uuid(row.owner_user_id).map_err(invalid)?;
    let device_id = DeviceId::try_from_uuid(row.device_id).map_err(invalid)?;
    let secret_digest = row
        .secret_digest
        .try_into()
        .map_err(|_| DeviceCredentialRepositoryError::InvalidPersistedData)?;
    let device_status = match row.device_status.as_str() {
        "PENDING" => DeviceStatus::Pending,
        "ACTIVE" => DeviceStatus::Active,
        "PAUSED" => DeviceStatus::Paused,
        "REVOKED" => DeviceStatus::Revoked,
        _ => return Err(DeviceCredentialRepositoryError::InvalidPersistedData),
    };
    Ok(StoredDeviceCredential {
        credential_id,
        owner_user_id,
        device_id,
        secret_digest,
        created_at: Timestamp::from_offset_datetime(row.created_at),
        revoked_at: row.revoked_at.map(Timestamp::from_offset_datetime),
        owner_active: row.owner_active,
        device_status,
    })
}
