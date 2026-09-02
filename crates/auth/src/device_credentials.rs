//! Device machine credentials within the existing authentication boundary.
//!
//! Browser authentication/CSRF authorizes grant creation and revocation in the
//! HTTP adapter. The enrollment token alone authorizes a once-only exchange.
//! No browser SessionId is fabricated for a device credential.

use std::fmt;

use synveil_core::{
    Device, DeviceCredentialId, DeviceCredentialSecret, DeviceEnrollmentGrantId, DeviceId,
    DeviceStatus, EnrollmentSecret, LogicalName, Timestamp, UserId,
};
use synveil_metadata::{
    DatabasePool, DeviceCredentialRepository, DeviceCredentialRepositoryError,
    NewDeviceEnrollmentGrant,
};

use crate::{SessionToken, sessions::truncate_to_microseconds};

pub const DEVICE_ENROLLMENT_TTL_SECONDS: i64 = 600;

/// Safe stable failure classes. No input token, URL, database error payload,
/// header, or one-time response is retained in any error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceAuthError {
    InvalidEnrollment,
    InvalidCredential,
    DeviceRevoked,
    DeviceNotFound,
    Unavailable,
    InvalidPersistedData,
}

impl fmt::Display for DeviceAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEnrollment => "invalid_device_enrollment",
            Self::InvalidCredential => "invalid_device_credential",
            Self::DeviceRevoked => "device_credential_revoked",
            Self::DeviceNotFound => "device_not_found",
            Self::Unavailable => "device_authentication_unavailable",
            Self::InvalidPersistedData => "device_authentication_persisted_data_invalid",
        })
    }
}

impl std::error::Error for DeviceAuthError {}

impl From<DeviceCredentialRepositoryError> for DeviceAuthError {
    fn from(error: DeviceCredentialRepositoryError) -> Self {
        match error {
            DeviceCredentialRepositoryError::InvalidEnrollment => Self::InvalidEnrollment,
            DeviceCredentialRepositoryError::DeviceNotFound => Self::DeviceNotFound,
            DeviceCredentialRepositoryError::InvalidPersistedData => Self::InvalidPersistedData,
            DeviceCredentialRepositoryError::Persistence(_) => Self::Unavailable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceEnrollmentTarget {
    Existing(DeviceId),
    New(LogicalName),
}

/// Raw enrollment token is handed to the authenticated browser once. Only
/// its verifier and non-secret metadata have been written to PostgreSQL.
#[derive(Debug)]
pub struct IssuedEnrollmentGrant {
    pub grant_id: DeviceEnrollmentGrantId,
    pub owner_user_id: UserId,
    pub device_id: DeviceId,
    pub token: EnrollmentSecret,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// Raw device bearer is handed to the claimant once and must immediately go
/// to its secure SecretStore. Replaying enrollment never retrieves it.
#[derive(Debug)]
pub struct IssuedDeviceCredential {
    pub owner_user_id: UserId,
    pub device_id: DeviceId,
    pub credential_id: DeviceCredentialId,
    pub secret: DeviceCredentialSecret,
    pub created_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceCredentialPrincipal {
    pub owner_user_id: UserId,
    pub device_id: DeviceId,
    pub credential_id: DeviceCredentialId,
}

/// Platform-neutral device auth service using the same core identities,
/// random-token helper and PostgreSQL boundary as browser authentication.
pub struct DeviceAuthenticationService<'pool> {
    repository: DeviceCredentialRepository<'pool>,
}

impl<'pool> DeviceAuthenticationService<'pool> {
    #[must_use]
    pub const fn new(pool: &'pool DatabasePool) -> Self {
        Self {
            repository: DeviceCredentialRepository::new(pool),
        }
    }

    pub async fn create_grant(
        &self,
        owner_user_id: UserId,
        target: DeviceEnrollmentTarget,
    ) -> Result<IssuedEnrollmentGrant, DeviceAuthError> {
        self.create_grant_at(owner_user_id, target, Timestamp::now())
            .await
    }

    pub async fn create_grant_at(
        &self,
        owner_user_id: UserId,
        target: DeviceEnrollmentTarget,
        observed_at: Timestamp,
    ) -> Result<IssuedEnrollmentGrant, DeviceAuthError> {
        let created_at = truncate_to_microseconds(observed_at);
        let expires_at = created_at
            .as_offset_datetime()
            .checked_add(time::Duration::seconds(DEVICE_ENROLLMENT_TTL_SECONDS))
            .map(Timestamp::from_offset_datetime)
            .ok_or(DeviceAuthError::InvalidEnrollment)?;
        let (device_id, new_device) = match target {
            DeviceEnrollmentTarget::New(display_name) => {
                let device = Device::new(DeviceId::new(), owner_user_id, display_name, created_at);
                (device.id(), Some(device))
            }
            DeviceEnrollmentTarget::Existing(device_id) => (device_id, None),
        };
        let token = EnrollmentSecret::from_bytes(*SessionToken::generate().as_bytes());
        let grant_id = DeviceEnrollmentGrantId::new();
        let record = NewDeviceEnrollmentGrant {
            grant_id,
            owner_user_id,
            device_id,
            secret_digest: token.digest(),
            created_at,
            expires_at,
        };
        self.repository
            .create_grant(&record, new_device.as_ref())
            .await?;
        Ok(IssuedEnrollmentGrant {
            grant_id,
            owner_user_id,
            device_id,
            token,
            created_at,
            expires_at,
        })
    }

    pub async fn exchange(
        &self,
        token: &EnrollmentSecret,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError> {
        self.exchange_with_clock(token, || truncate_to_microseconds(Timestamp::now()))
            .await
    }

    /// Once-only handoff: if commit succeeds and the HTTP response is lost,
    /// the owner must revoke-all for the Device and create a fresh grant.
    pub async fn exchange_at(
        &self,
        token: &EnrollmentSecret,
        observed_at: Timestamp,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError> {
        self.exchange_with_clock(token, || truncate_to_microseconds(observed_at))
            .await
    }

    async fn exchange_with_clock(
        &self,
        token: &EnrollmentSecret,
        clock: impl FnOnce() -> Timestamp + Send,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError> {
        let secret = DeviceCredentialSecret::from_bytes(*SessionToken::generate().as_bytes());
        let credential_id = DeviceCredentialId::new();
        let consumed = self
            .repository
            .consume_grant_with_clock(&token.digest(), credential_id, &secret.digest(), clock)
            .await?;
        Ok(IssuedDeviceCredential {
            owner_user_id: consumed.owner_user_id,
            device_id: consumed.device_id,
            credential_id: consumed.credential_id,
            secret,
            created_at: consumed.created_at,
        })
    }

    pub async fn authenticate(
        &self,
        secret: &DeviceCredentialSecret,
    ) -> Result<DeviceCredentialPrincipal, DeviceAuthError> {
        let row = self
            .repository
            .load_credential_by_digest(&secret.digest())
            .await?
            .ok_or(DeviceAuthError::InvalidCredential)?;
        if !secret.digest_matches(&row.secret_digest) || !row.owner_active {
            return Err(DeviceAuthError::InvalidCredential);
        }
        // The distinct revoked result is available only after the caller has
        // proved knowledge of this actual credential. Unknown/tampered tokens
        // never disclose Device or credential existence/state.
        if row.revoked_at.is_some() || row.device_status == DeviceStatus::Revoked {
            return Err(DeviceAuthError::DeviceRevoked);
        }
        if row.device_status != DeviceStatus::Active {
            return Err(DeviceAuthError::InvalidCredential);
        }
        Ok(DeviceCredentialPrincipal {
            owner_user_id: row.owner_user_id,
            device_id: row.device_id,
            credential_id: row.credential_id,
        })
    }

    pub async fn authenticate_raw(
        &self,
        secret: &str,
    ) -> Result<DeviceCredentialPrincipal, DeviceAuthError> {
        let secret = DeviceCredentialSecret::parse(secret)
            .map_err(|_| DeviceAuthError::InvalidCredential)?;
        self.authenticate(&secret).await
    }

    pub async fn revoke_credential(
        &self,
        owner: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
    ) -> Result<(), DeviceAuthError> {
        self.revoke_credential_at(owner, device_id, credential_id, Timestamp::now())
            .await
    }

    pub async fn revoke_credential_at(
        &self,
        owner: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        observed_at: Timestamp,
    ) -> Result<(), DeviceAuthError> {
        self.repository
            .revoke_credential(
                owner,
                device_id,
                credential_id,
                truncate_to_microseconds(observed_at),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn revoke_all_credentials(
        &self,
        owner: UserId,
        device_id: DeviceId,
    ) -> Result<(), DeviceAuthError> {
        self.revoke_all_credentials_at(owner, device_id, Timestamp::now())
            .await
    }

    pub async fn revoke_all_credentials_at(
        &self,
        owner: UserId,
        device_id: DeviceId,
        observed_at: Timestamp,
    ) -> Result<(), DeviceAuthError> {
        self.repository
            .revoke_all_credentials(owner, device_id, truncate_to_microseconds(observed_at))
            .await
            .map_err(Into::into)
    }

    pub async fn revoke_device(
        &self,
        owner: UserId,
        device_id: DeviceId,
    ) -> Result<(), DeviceAuthError> {
        self.revoke_device_at(owner, device_id, Timestamp::now())
            .await
    }

    pub async fn revoke_device_at(
        &self,
        owner: UserId,
        device_id: DeviceId,
        observed_at: Timestamp,
    ) -> Result<(), DeviceAuthError> {
        self.repository
            .revoke_device(owner, device_id, truncate_to_microseconds(observed_at))
            .await
            .map_err(Into::into)
    }
}
