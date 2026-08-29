//! Non-secret desktop connection profiles and the secure credential lifecycle.
//!
//! A profile's origin and enrolled owner/device are immutable. Replacement is
//! explicit, and forgetting is local: it never pretends to revoke a server-side
//! credential while offline. SQLite holds only identities, timestamps and
//! cleanup intents; every bearer byte crosses the existing SecretStore port.

use std::{cell::Cell, fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use sqlx::Row;
use synveil_core::{DeviceCredentialId, DeviceCredentialSecret, DeviceId, UserId};
use synveil_platform::{SecretName, SecretStore, SecretStoreState};
use url::{Host, Url};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{ClientSyncError, EnrollmentCredentials, LocalStateStore, state::now_ms};

const MAX_BASE_URL_BYTES: usize = 2_048;
const MAX_PROFILE_LABEL_BYTES: usize = 256;
const MAX_PROFILES: usize = 1_024;
const MAX_PENDING_SECRET_CLEANUP: usize = 128;
const MAX_SECRET_ENVELOPE_BYTES: usize = 4_096;
const SECRET_ENVELOPE_VERSION: u8 = 1;

/// Opaque desktop-only identity, independent of URL aliases and server IDs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ServerProfileId(Uuid);

impl ServerProfileId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn parse_str(value: &str) -> Result<Self, ClientSyncError> {
        let value_id = Uuid::parse_str(value).map_err(|_| ClientSyncError::InvalidServerProfile)?;
        if value_id.get_version_num() != 7
            || value_id.as_bytes()[8] & 0xc0 != 0x80
            || value_id.to_string() != value
        {
            return Err(ClientSyncError::InvalidServerProfile);
        }
        Ok(Self(value_id))
    }
}

impl Default for ServerProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ServerProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for ServerProfileId {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_str(value)
    }
}

/// Strict origin-root URL. Production construction accepts verified HTTPS only.
/// The URL parser owns host, IDNA, IPv4/IPv6 and port parsing/normalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalBaseUrl {
    url: Url,
    loopback_test_http: bool,
}

impl CanonicalBaseUrl {
    pub fn parse(value: &str) -> Result<Self, ClientSyncError> {
        Self::parse_with_policy(value, false)
    }

    /// Explicit non-production exception, restricted to numeric loopback IPs.
    /// DNS names (including localhost) do not gain an HTTP exception.
    pub fn parse_for_loopback_test(value: &str) -> Result<Self, ClientSyncError> {
        Self::parse_with_policy(value, true)
    }

    fn parse_with_policy(value: &str, allow_loopback_http: bool) -> Result<Self, ClientSyncError> {
        // An origin has only the two scheme slashes and an optional root
        // slash. This lexical restriction prevents the parser's dot-segment
        // normalization from accepting a supplied application subpath.
        let slash_count = value.bytes().filter(|byte| *byte == b'/').count();
        if value.is_empty()
            || value.len() > MAX_BASE_URL_BYTES
            || value.chars().any(|c| c.is_control() || c.is_whitespace())
            || value.contains(['@', '\\'])
            || !(slash_count == 2 || (slash_count == 3 && value.ends_with('/')))
        {
            return Err(ClientSyncError::InvalidServerUrl);
        }
        // The WHATWG parser normally repairs some malformed input. Profiles
        // fail on every reported repair instead of silently accepting it.
        let violation = Cell::new(false);
        let on_violation = |_| violation.set(true);
        let url = Url::options()
            .syntax_violation_callback(Some(&on_violation))
            .parse(value)
            .map_err(|_| ClientSyncError::InvalidServerUrl)?;
        if violation.get()
            || !url.has_host()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || url.path() != "/"
            || url.port() == Some(0)
            // An empty explicit port is accepted by WHATWG, but not by this
            // origin configuration contract. This is a lexical restriction,
            // not a replacement for the parser's host/port interpretation.
            || value.ends_with(':')
            || value.ends_with(":/")
        {
            return Err(ClientSyncError::InvalidServerUrl);
        }
        if let Some(domain) = url.domain() {
            // url owns IDNA/host parsing; reject DNS host labels that its
            // permissive browser-URL grammar can otherwise preserve.
            let domain = domain.strip_suffix('.').unwrap_or(domain);
            if domain.len() > 253
                || domain.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
            {
                return Err(ClientSyncError::InvalidServerUrl);
            }
        }
        let loopback_test_http = url.scheme() == "http"
            && allow_loopback_http
            && match url.host() {
                Some(Host::Ipv4(address)) => address.is_loopback(),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                _ => false,
            };
        if url.scheme() != "https" && !loopback_test_http {
            return Err(ClientSyncError::InvalidServerUrl);
        }
        Ok(Self {
            url,
            loopback_test_http,
        })
    }

    #[must_use]
    pub const fn as_url(&self) -> &Url {
        &self.url
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    #[must_use]
    pub const fn is_loopback_test_http(&self) -> bool {
        self.loopback_test_http
    }
}

/// Exportable non-secret configuration. No server installation identity exists
/// in the current server protocol: verified origin/TLS is the present binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerProfile {
    profile_id: ServerProfileId,
    base_url: CanonicalBaseUrl,
    display_label: String,
    created_at_ms: i64,
    last_connected_at_ms: Option<i64>,
}

impl ServerProfile {
    pub fn new(
        base_url: CanonicalBaseUrl,
        display_label: impl Into<String>,
    ) -> Result<Self, ClientSyncError> {
        let display_label = display_label.into();
        validate_label(&display_label)?;
        Ok(Self {
            profile_id: ServerProfileId::new(),
            base_url,
            display_label,
            created_at_ms: now_ms()?,
            last_connected_at_ms: None,
        })
    }

    #[must_use]
    pub const fn profile_id(&self) -> ServerProfileId {
        self.profile_id
    }

    #[must_use]
    pub const fn base_url(&self) -> &CanonicalBaseUrl {
        &self.base_url
    }

    #[must_use]
    pub fn display_label(&self) -> &str {
        &self.display_label
    }

    #[must_use]
    pub const fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }

    #[must_use]
    pub const fn last_connected_at_ms(&self) -> Option<i64> {
        self.last_connected_at_ms
    }
}

/// Durable enrollment metadata; even after forget the owner/device binding is
/// retained, so re-enrollment cannot silently switch device identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceEnrollmentRecord {
    profile_id: ServerProfileId,
    owner_user_id: UserId,
    device_id: DeviceId,
    credential_id: DeviceCredentialId,
    completed_at_ms: i64,
    forgotten_at_ms: Option<i64>,
}

impl DeviceEnrollmentRecord {
    #[must_use]
    pub const fn profile_id(self) -> ServerProfileId {
        self.profile_id
    }
    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }
    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }
    #[must_use]
    pub const fn credential_id(self) -> DeviceCredentialId {
        self.credential_id
    }
    #[must_use]
    pub const fn completed_at_ms(self) -> i64 {
        self.completed_at_ms
    }
    #[must_use]
    pub const fn forgotten_at_ms(self) -> Option<i64> {
        self.forgotten_at_ms
    }
}

/// Loaded only through the profile-bound SecretStore lifecycle. Public callers
/// cannot relabel a bearer from one profile as a credential for another.
#[derive(Debug)]
pub struct LoadedDeviceCredential {
    metadata: DeviceEnrollmentRecord,
    verified_base_url: CanonicalBaseUrl,
    secret: DeviceCredentialSecret,
}

impl LoadedDeviceCredential {
    #[must_use]
    pub const fn profile_id(&self) -> ServerProfileId {
        self.metadata.profile_id
    }
    /// Origin read from and validated against the secure-store envelope, not
    /// authority inferred solely from editable local SQLite configuration.
    #[must_use]
    pub const fn base_url(&self) -> &CanonicalBaseUrl {
        &self.verified_base_url
    }
    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.metadata.owner_user_id
    }
    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.metadata.device_id
    }
    #[must_use]
    pub const fn credential_id(&self) -> DeviceCredentialId {
        self.metadata.credential_id
    }
    #[must_use]
    pub const fn secret(&self) -> &DeviceCredentialSecret {
        &self.secret
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        profile: &ServerProfile,
        owner_user_id: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        secret: DeviceCredentialSecret,
    ) -> Self {
        Self {
            metadata: DeviceEnrollmentRecord {
                profile_id: profile.profile_id(),
                owner_user_id,
                device_id,
                credential_id,
                completed_at_ms: 0,
                forgotten_at_ms: None,
            },
            verified_base_url: profile.base_url().clone(),
            secret,
        }
    }
}

/// This representation exists only inside SecretStore. Its borrowed strings
/// avoid extra secret allocations during parsing; owned serialized buffers
/// are always zeroized. It intentionally has no Debug or public constructor.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialEnvelope<'a> {
    version: u8,
    profile_id: &'a str,
    canonical_base_url: &'a str,
    transport_policy: &'a str,
    owner_user_id: &'a str,
    device_id: &'a str,
    credential_id: &'a str,
    secret: &'a str,
}

impl<'a> CredentialEnvelope<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, ClientSyncError> {
        if bytes.is_empty() || bytes.len() > MAX_SECRET_ENVELOPE_BYTES {
            return Err(ClientSyncError::SecureStoreUnavailable);
        }
        let envelope: Self =
            serde_json::from_slice(bytes).map_err(|_| ClientSyncError::SecureStoreUnavailable)?;
        if envelope.version != SECRET_ENVELOPE_VERSION {
            return Err(ClientSyncError::SecureStoreUnavailable);
        }
        Ok(envelope)
    }

    fn validate_origin_and_key(
        &self,
        profile: &ServerProfile,
        credential_id: DeviceCredentialId,
    ) -> Result<CanonicalBaseUrl, ClientSyncError> {
        if self.profile_id != profile.profile_id().to_string()
            || self.credential_id != credential_id.to_string()
            || self.canonical_base_url != profile.base_url().as_str()
            || self.transport_policy != profile_transport_policy(profile)
        {
            return Err(ClientSyncError::WrongServerProfile);
        }
        let origin = match self.transport_policy {
            "HTTPS" => CanonicalBaseUrl::parse(self.canonical_base_url),
            "LOOPBACK_TEST_HTTP" => {
                CanonicalBaseUrl::parse_for_loopback_test(self.canonical_base_url)
            }
            _ => return Err(ClientSyncError::SecureStoreUnavailable),
        }?;
        if origin.as_str() != self.canonical_base_url || &origin != profile.base_url() {
            return Err(ClientSyncError::WrongServerProfile);
        }
        Ok(origin)
    }

    fn validate_owner_device(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
    ) -> Result<(), ClientSyncError> {
        if self.owner_user_id != owner_user_id.to_string()
            || self.device_id != device_id.to_string()
        {
            return Err(ClientSyncError::WrongScope);
        }
        Ok(())
    }
}

fn profile_transport_policy(profile: &ServerProfile) -> &'static str {
    if profile.base_url().is_loopback_test_http() {
        "LOOPBACK_TEST_HTTP"
    } else {
        "HTTPS"
    }
}

fn encode_credential_envelope(
    profile: &ServerProfile,
    metadata: DeviceEnrollmentRecord,
    secret: &DeviceCredentialSecret,
) -> Result<Zeroizing<Vec<u8>>, ClientSyncError> {
    let profile_id = metadata.profile_id().to_string();
    let owner_user_id = metadata.owner_user_id().to_string();
    let device_id = metadata.device_id().to_string();
    let credential_id = metadata.credential_id().to_string();
    let envelope = CredentialEnvelope {
        version: SECRET_ENVELOPE_VERSION,
        profile_id: &profile_id,
        canonical_base_url: profile.base_url().as_str(),
        transport_policy: profile_transport_policy(profile),
        owner_user_id: &owner_user_id,
        device_id: &device_id,
        credential_id: &credential_id,
        secret: secret.expose_secret(),
    };
    let bytes = Zeroizing::new(
        serde_json::to_vec(&envelope).map_err(|_| ClientSyncError::SecureStoreUnavailable)?,
    );
    if bytes.len() > MAX_SECRET_ENVELOPE_BYTES {
        return Err(ClientSyncError::SecureStoreUnavailable);
    }
    Ok(bytes)
}

fn decode_credential_envelope(
    profile: &ServerProfile,
    metadata: DeviceEnrollmentRecord,
    bytes: &[u8],
) -> Result<LoadedDeviceCredential, ClientSyncError> {
    let envelope = CredentialEnvelope::parse(bytes)?;
    let verified_base_url = envelope.validate_origin_and_key(profile, metadata.credential_id())?;
    envelope.validate_owner_device(metadata.owner_user_id(), metadata.device_id())?;
    let secret = DeviceCredentialSecret::parse(envelope.secret)
        .map_err(|_| ClientSyncError::SecureStoreUnavailable)?;
    Ok(LoadedDeviceCredential {
        metadata,
        verified_base_url,
        secret,
    })
}

impl LocalStateStore {
    /// Inserts immutable non-secret connection configuration. Re-saving the
    /// exact same profile is idempotent; changing an existing ID is forbidden.
    pub async fn save_server_profile(
        &self,
        profile: &ServerProfile,
    ) -> Result<(), ClientSyncError> {
        sqlx::query(
            "INSERT INTO server_profiles (profile_id, canonical_base_url, transport_policy,
             display_label, created_at_ms, last_connected_at_ms) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(profile_id) DO NOTHING",
        )
        .bind(profile.profile_id.to_string())
        .bind(profile.base_url.as_str())
        .bind(if profile.base_url.loopback_test_http {
            "LOOPBACK_TEST_HTTP"
        } else {
            "HTTPS"
        })
        .bind(&profile.display_label)
        .bind(profile.created_at_ms)
        .bind(profile.last_connected_at_ms)
        .execute(&self.pool)
        .await?;
        let stored = self
            .server_profile(profile.profile_id)
            .await?
            .ok_or(ClientSyncError::InvalidServerProfile)?;
        if stored.base_url != profile.base_url
            || stored.display_label != profile.display_label
            || stored.created_at_ms != profile.created_at_ms
        {
            return Err(ClientSyncError::WrongServerProfile);
        }
        Ok(())
    }

    pub async fn server_profile(
        &self,
        profile_id: ServerProfileId,
    ) -> Result<Option<ServerProfile>, ClientSyncError> {
        sqlx::query("SELECT * FROM server_profiles WHERE profile_id = ?")
            .bind(profile_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(decode_profile)
            .transpose()
    }

    pub async fn server_profiles(&self) -> Result<Vec<ServerProfile>, ClientSyncError> {
        let rows =
            sqlx::query("SELECT * FROM server_profiles ORDER BY created_at_ms, profile_id LIMIT ?")
                .bind((MAX_PROFILES + 1) as i64)
                .fetch_all(&self.pool)
                .await?;
        if rows.len() > MAX_PROFILES {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_profile).collect()
    }

    pub async fn mark_profile_connected(
        &self,
        profile_id: ServerProfileId,
    ) -> Result<(), ClientSyncError> {
        let changed = sqlx::query("UPDATE server_profiles SET last_connected_at_ms = MAX(created_at_ms, ?) WHERE profile_id = ?")
            .bind(now_ms()?).bind(profile_id.to_string()).execute(&self.pool).await?.rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidServerProfile);
        }
        Ok(())
    }

    pub async fn profile_enrollment(
        &self,
        profile_id: ServerProfileId,
    ) -> Result<Option<DeviceEnrollmentRecord>, ClientSyncError> {
        sqlx::query("SELECT * FROM profile_device_enrollments WHERE profile_id = ?")
            .bind(profile_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(decode_enrollment)
            .transpose()
    }

    /// Persist the result of this profile's verified enrollment exchange.
    /// The receipt is origin-bound and cannot be assembled or relabeled by
    /// public callers; there is no arbitrary bearer-import API.
    pub async fn store_enrollment(
        &self,
        enrollment: &EnrollmentCredentials,
        secret_store: &dyn SecretStore,
    ) -> Result<DeviceEnrollmentRecord, ClientSyncError> {
        self.validate_enrollment_origin(enrollment).await?;
        self.store_device_enrollment(
            enrollment.profile_id(),
            enrollment.owner_user_id(),
            enrollment.device_id(),
            enrollment.credential_id(),
            enrollment.secret(),
            secret_store,
        )
        .await
    }

    /// Explicitly replace credentials using a new verified exchange for the
    /// same immutable profile, owner and Device. No automatic rebind occurs.
    pub async fn replace_enrollment(
        &self,
        enrollment: &EnrollmentCredentials,
        secret_store: &dyn SecretStore,
    ) -> Result<DeviceEnrollmentRecord, ClientSyncError> {
        self.validate_enrollment_origin(enrollment).await?;
        self.replace_device_enrollment(
            enrollment.profile_id(),
            enrollment.owner_user_id(),
            enrollment.device_id(),
            enrollment.credential_id(),
            enrollment.secret(),
            secret_store,
        )
        .await
    }

    async fn validate_enrollment_origin(
        &self,
        enrollment: &EnrollmentCredentials,
    ) -> Result<(), ClientSyncError> {
        let profile = self
            .server_profile(enrollment.profile_id())
            .await?
            .ok_or(ClientSyncError::InvalidServerProfile)?;
        if profile.base_url() != enrollment.base_url() {
            return Err(ClientSyncError::WrongServerProfile);
        }
        Ok(())
    }

    /// Internal lifecycle implementation. Public entry requires a verified
    /// origin-bound enrollment receipt; raw import is unavailable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn store_device_enrollment(
        &self,
        profile_id: ServerProfileId,
        owner_user_id: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        secret: &DeviceCredentialSecret,
        secret_store: &dyn SecretStore,
    ) -> Result<DeviceEnrollmentRecord, ClientSyncError> {
        self.persist_device_enrollment(
            profile_id,
            owner_user_id,
            device_id,
            credential_id,
            secret,
            secret_store,
            false,
        )
        .await
    }

    /// Explicit replacement, restricted to the already bound owner and Device.
    /// A forgotten profile still cannot be silently rebound to another Device.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn replace_device_enrollment(
        &self,
        profile_id: ServerProfileId,
        owner_user_id: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        secret: &DeviceCredentialSecret,
        secret_store: &dyn SecretStore,
    ) -> Result<DeviceEnrollmentRecord, ClientSyncError> {
        self.persist_device_enrollment(
            profile_id,
            owner_user_id,
            device_id,
            credential_id,
            secret,
            secret_store,
            true,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn persist_device_enrollment(
        &self,
        profile_id: ServerProfileId,
        owner_user_id: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        secret: &DeviceCredentialSecret,
        secret_store: &dyn SecretStore,
        replace: bool,
    ) -> Result<DeviceEnrollmentRecord, ClientSyncError> {
        let _guard = self.credential_lifecycle_lock.lock().await;
        let profile = self
            .server_profile(profile_id)
            .await?
            .ok_or(ClientSyncError::InvalidServerProfile)?;
        let previous = self.profile_enrollment(profile_id).await?;
        match previous {
            Some(previous)
                if previous.owner_user_id != owner_user_id || previous.device_id != device_id =>
            {
                return Err(ClientSyncError::WrongScope);
            }
            Some(previous) if !replace || previous.credential_id == credential_id => {
                return Err(ClientSyncError::CredentialReplacementRequired);
            }
            None if replace => return Err(ClientSyncError::AuthenticationRequired),
            _ => {}
        }
        require_secure_store(secret_store)?;
        self.cleanup_profile_secrets_inner(profile_id, secret_store)
            .await?;
        let completed_at_ms = now_ms()?;
        let metadata = DeviceEnrollmentRecord {
            profile_id,
            owner_user_id,
            device_id,
            credential_id,
            completed_at_ms,
            forgotten_at_ms: None,
        };
        let encoded = encode_credential_envelope(&profile, metadata, secret)?;
        let name = credential_secret_name(profile_id, credential_id)?;
        // A copied/reconstructed SQLite profile is configuration, not authority
        // to overwrite another origin's secure-store entry under the same IDs.
        if let Some(existing) = secret_store
            .get_secret(&name)
            .map_err(|_| ClientSyncError::SecureStoreUnavailable)?
        {
            let existing = decode_credential_envelope(&profile, metadata, existing.as_bytes())?;
            if !secret.digest_matches(&existing.secret().digest()) {
                return Err(ClientSyncError::CredentialReplacementRequired);
            }
        }
        // Durable intent precedes the keychain mutation. A process crash can
        // leave only a cleanup-able orphan, never an untracked bearer file.
        sqlx::query("INSERT INTO profile_secret_cleanup(profile_id, credential_id, requested_at_ms) VALUES (?, ?, ?)")
            .bind(profile_id.to_string()).bind(credential_id.to_string()).bind(completed_at_ms)
            .execute(&self.pool).await?;
        if secret_store.put_secret(&name, &encoded).is_err() {
            return Err(ClientSyncError::SecureStoreUnavailable);
        }
        let read_back = secret_store
            .get_secret(&name)
            .map_err(|_| ClientSyncError::SecureStoreUnavailable)?
            .ok_or(ClientSyncError::SecureStoreUnavailable)?;
        let read_back = decode_credential_envelope(&profile, metadata, read_back.as_bytes())?;
        if !secret.digest_matches(&read_back.secret().digest()) {
            return Err(ClientSyncError::SecureStoreUnavailable);
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO profile_device_enrollments(profile_id, owner_user_id, device_id,
             credential_id, completed_at_ms, forgotten_at_ms) VALUES (?, ?, ?, ?, ?, NULL)
             ON CONFLICT(profile_id) DO UPDATE SET credential_id = excluded.credential_id,
             completed_at_ms = excluded.completed_at_ms, forgotten_at_ms = NULL",
        )
        .bind(profile_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(device_id.to_string())
        .bind(credential_id.to_string())
        .bind(completed_at_ms)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM profile_secret_cleanup WHERE profile_id = ? AND credential_id = ?",
        )
        .bind(profile_id.to_string())
        .bind(credential_id.to_string())
        .execute(&mut *transaction)
        .await?;
        if let Some(previous) = previous {
            sqlx::query("INSERT OR IGNORE INTO profile_secret_cleanup(profile_id, credential_id, requested_at_ms) VALUES (?, ?, ?)")
                .bind(profile_id.to_string()).bind(previous.credential_id.to_string()).bind(completed_at_ms)
                .execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        self.cleanup_profile_secrets_inner(profile_id, secret_store)
            .await?;
        self.profile_enrollment(profile_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)
    }

    pub async fn load_device_credential(
        &self,
        profile_id: ServerProfileId,
        secret_store: &dyn SecretStore,
    ) -> Result<Option<LoadedDeviceCredential>, ClientSyncError> {
        let _guard = self.credential_lifecycle_lock.lock().await;
        let profile = self
            .server_profile(profile_id)
            .await?
            .ok_or(ClientSyncError::InvalidServerProfile)?;
        self.cleanup_profile_secrets_inner(profile_id, secret_store)
            .await?;
        let Some(metadata) = self.profile_enrollment(profile_id).await? else {
            return Ok(None);
        };
        if metadata.forgotten_at_ms.is_some() {
            return Ok(None);
        }
        require_secure_store(secret_store)?;
        let stored = secret_store
            .get_secret(&credential_secret_name(profile_id, metadata.credential_id)?)
            .map_err(|_| ClientSyncError::SecureStoreUnavailable)?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        decode_credential_envelope(&profile, metadata, stored.as_bytes()).map(Some)
    }

    /// Disconnect locally without deleting a replica or changing its progress.
    /// A durable tombstone is written first, so restart cannot reload a secret
    /// whose deletion was interrupted. Failure is surfaced until cleanup works.
    pub async fn forget_device_credential(
        &self,
        profile_id: ServerProfileId,
        secret_store: &dyn SecretStore,
    ) -> Result<(), ClientSyncError> {
        let _guard = self.credential_lifecycle_lock.lock().await;
        if self.server_profile(profile_id).await?.is_none() {
            return Err(ClientSyncError::InvalidServerProfile);
        }
        if let Some(previous) = self.profile_enrollment(profile_id).await? {
            let now = now_ms()?.max(previous.completed_at_ms);
            let mut transaction = self.pool.begin().await?;
            sqlx::query("UPDATE profile_device_enrollments SET forgotten_at_ms = COALESCE(forgotten_at_ms, ?) WHERE profile_id = ?")
                .bind(now).bind(profile_id.to_string()).execute(&mut *transaction).await?;
            sqlx::query("INSERT OR IGNORE INTO profile_secret_cleanup(profile_id, credential_id, requested_at_ms) VALUES (?, ?, ?)")
                .bind(profile_id.to_string()).bind(previous.credential_id.to_string()).bind(now)
                .execute(&mut *transaction).await?;
            transaction.commit().await?;
        }
        self.cleanup_profile_secrets_inner(profile_id, secret_store)
            .await
    }

    /// Resume interrupted replacement/forget without issuing any network call.
    pub async fn cleanup_profile_secrets(
        &self,
        profile_id: ServerProfileId,
        secret_store: &dyn SecretStore,
    ) -> Result<(), ClientSyncError> {
        let _guard = self.credential_lifecycle_lock.lock().await;
        self.cleanup_profile_secrets_inner(profile_id, secret_store)
            .await
    }

    async fn cleanup_profile_secrets_inner(
        &self,
        profile_id: ServerProfileId,
        secret_store: &dyn SecretStore,
    ) -> Result<(), ClientSyncError> {
        let credential_ids: Vec<String> = sqlx::query_scalar(
            "SELECT credential_id FROM profile_secret_cleanup WHERE profile_id = ? ORDER BY requested_at_ms, credential_id LIMIT ?",
        )
        .bind(profile_id.to_string()).bind((MAX_PENDING_SECRET_CLEANUP + 1) as i64)
        .fetch_all(&self.pool).await?;
        if credential_ids.len() > MAX_PENDING_SECRET_CLEANUP {
            return Err(ClientSyncError::ResourceLimit);
        }
        if !credential_ids.is_empty() {
            require_secure_store(secret_store)?;
        }
        for credential_id in credential_ids {
            let credential_id = credential_id
                .parse()
                .map_err(|_| ClientSyncError::InvalidState)?;
            // Defense against damaged/manual metadata: never delete a currently
            // active credential as cleanup work.
            if self
                .profile_enrollment(profile_id)
                .await?
                .is_some_and(|record| {
                    record.credential_id == credential_id && record.forgotten_at_ms.is_none()
                })
            {
                return Err(ClientSyncError::InvalidState);
            }
            let name = credential_secret_name(profile_id, credential_id)?;
            if let Some(stored) = secret_store
                .get_secret(&name)
                .map_err(|_| ClientSyncError::SecureStoreUnavailable)?
            {
                let profile = self
                    .server_profile(profile_id)
                    .await?
                    .ok_or(ClientSyncError::InvalidServerProfile)?;
                let envelope = CredentialEnvelope::parse(stored.as_bytes())?;
                envelope.validate_origin_and_key(&profile, credential_id)?;
                if let Some(metadata) = self.profile_enrollment(profile_id).await? {
                    envelope
                        .validate_owner_device(metadata.owner_user_id(), metadata.device_id())?;
                }
                secret_store
                    .delete_secret(&name)
                    .map_err(|_| ClientSyncError::SecureStoreUnavailable)?;
            }
            sqlx::query(
                "DELETE FROM profile_secret_cleanup WHERE profile_id = ? AND credential_id = ?",
            )
            .bind(profile_id.to_string())
            .bind(credential_id.to_string())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
}

fn require_secure_store(store: &dyn SecretStore) -> Result<(), ClientSyncError> {
    if store.state() == SecretStoreState::Available {
        Ok(())
    } else {
        Err(ClientSyncError::SecureStoreUnavailable)
    }
}

fn credential_secret_name(
    profile_id: ServerProfileId,
    credential_id: DeviceCredentialId,
) -> Result<SecretName, ClientSyncError> {
    SecretName::new(format!("desktop-device-v1/{profile_id}/{credential_id}"))
        .map_err(|_| ClientSyncError::InvalidServerProfile)
}

fn validate_label(label: &str) -> Result<(), ClientSyncError> {
    if label.trim().is_empty()
        || label.len() > MAX_PROFILE_LABEL_BYTES
        || label.chars().any(char::is_control)
    {
        Err(ClientSyncError::InvalidServerProfile)
    } else {
        Ok(())
    }
}

fn decode_profile(row: sqlx::sqlite::SqliteRow) -> Result<ServerProfile, ClientSyncError> {
    let raw: String = row.try_get("canonical_base_url")?;
    let policy: String = row.try_get("transport_policy")?;
    let base_url = match policy.as_str() {
        "HTTPS" => CanonicalBaseUrl::parse(&raw)?,
        "LOOPBACK_TEST_HTTP" => CanonicalBaseUrl::parse_for_loopback_test(&raw)?,
        _ => return Err(ClientSyncError::InvalidServerProfile),
    };
    if base_url.as_str() != raw {
        return Err(ClientSyncError::InvalidServerProfile);
    }
    let display_label = row.try_get::<String, _>("display_label")?;
    validate_label(&display_label)?;
    Ok(ServerProfile {
        profile_id: row.try_get::<String, _>("profile_id")?.parse()?,
        base_url,
        display_label,
        created_at_ms: row.try_get("created_at_ms")?,
        last_connected_at_ms: row.try_get("last_connected_at_ms")?,
    })
}

fn decode_enrollment(
    row: sqlx::sqlite::SqliteRow,
) -> Result<DeviceEnrollmentRecord, ClientSyncError> {
    Ok(DeviceEnrollmentRecord {
        profile_id: row.try_get::<String, _>("profile_id")?.parse()?,
        owner_user_id: row
            .try_get::<String, _>("owner_user_id")?
            .parse()
            .map_err(|_| ClientSyncError::InvalidState)?,
        device_id: row
            .try_get::<String, _>("device_id")?
            .parse()
            .map_err(|_| ClientSyncError::InvalidState)?,
        credential_id: row
            .try_get::<String, _>("credential_id")?
            .parse()
            .map_err(|_| ClientSyncError::InvalidState)?,
        completed_at_ms: row.try_get("completed_at_ms")?,
        forgotten_at_ms: row.try_get("forgotten_at_ms")?,
    })
}

#[cfg(test)]
mod tests;
