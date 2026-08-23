//! Opaque browser-session primitives.
//!
//! A session ID is only a durable record identity. The bearer credential is a
//! separate 256-bit random value and is never used as, or derived from, the
//! session ID. Only the credential's SHA-256 digest crosses the persistence
//! boundary.

use std::{fmt, str::FromStr, time::Duration};

use argon2::password_hash::rand_core::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use synveil_core::{Timestamp, UserId};
use uuid::Uuid;
use zeroize::Zeroize;

pub const SESSION_TOKEN_BYTES: usize = 32;
const SESSION_TOKEN_HEX_BYTES: usize = SESSION_TOKEN_BYTES * 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIdError {
    InvalidVariant,
    NonCanonical,
    NotUuidV7,
}

impl fmt::Display for SessionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidVariant => "session id has an unsupported UUID variant",
            Self::NonCanonical => "session id is not in canonical lowercase UUID form",
            Self::NotUuidV7 => "session id is not UUIDv7",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SessionIdError {}

/// Public database identity for one browser session. It is not a bearer
/// credential and does not grant access by itself.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId(Uuid);

impl SessionId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn try_from_uuid(value: Uuid) -> Result<Self, SessionIdError> {
        let bytes = value.as_bytes();
        if bytes[6] >> 4 != 7 {
            return Err(SessionIdError::NotUuidV7);
        }
        if bytes[8] & 0xc0 != 0x80 {
            return Err(SessionIdError::InvalidVariant);
        }
        Ok(Self(value))
    }

    pub fn parse_str(value: &str) -> Result<Self, SessionIdError> {
        let uuid = Uuid::parse_str(value).map_err(|_| SessionIdError::NotUuidV7)?;
        if value != uuid.hyphenated().to_string() {
            return Err(SessionIdError::NonCanonical);
        }
        Self::try_from_uuid(uuid)
    }

    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    #[must_use]
    pub const fn into_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<Uuid> for SessionId {
    fn as_ref(&self) -> &Uuid {
        self.as_uuid()
    }
}

impl From<SessionId> for Uuid {
    fn from(value: SessionId) -> Self {
        value.into_uuid()
    }
}

impl FromStr for SessionId {
    type Err = SessionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_str(value)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.hyphenated())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionTokenError {
    InvalidLength,
    InvalidCharacter,
}

impl fmt::Display for SessionTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidLength => "session token has an invalid length",
            Self::InvalidCharacter => "session token has an invalid encoding",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SessionTokenError {}

/// A high-entropy opaque browser bearer credential.
///
/// The raw bytes are intentionally not printable. `Debug` is redacted and no
/// `Display` implementation exists; a transport adapter must explicitly call
/// [`Self::to_hex`] when it needs the one-time presentation form.
#[derive(Clone, Eq, PartialEq)]
pub struct SessionToken([u8; SESSION_TOKEN_BYTES]);

impl SessionToken {
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0_u8; SESSION_TOKEN_BYTES];
        argon2::password_hash::rand_core::OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; SESSION_TOKEN_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn try_from_hex(value: &str) -> Result<Self, SessionTokenError> {
        if value.len() != SESSION_TOKEN_HEX_BYTES {
            return Err(SessionTokenError::InvalidLength);
        }

        let mut bytes = [0_u8; SESSION_TOKEN_BYTES];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let high = decode_hex_nibble(value.as_bytes()[index * 2])
                .ok_or(SessionTokenError::InvalidCharacter)?;
            let low = decode_hex_nibble(value.as_bytes()[index * 2 + 1])
                .ok_or(SessionTokenError::InvalidCharacter)?;
            *byte = (high << 4) | low;
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; SESSION_TOKEN_BYTES] {
        &self.0
    }

    /// Return the raw bearer presentation form. Callers must treat the
    /// resulting string as secret material and must not log or persist it.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut value = String::with_capacity(SESSION_TOKEN_HEX_BYTES);
        for byte in self.0 {
            value.push(hex_digit(byte >> 4));
            value.push(hex_digit(byte & 0x0f));
        }
        value
    }

    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let digest = Sha256::digest(self.0);
        let mut value = [0_u8; 32];
        value.copy_from_slice(&digest);
        value
    }

    #[must_use]
    pub fn digest_matches(&self, candidate: &[u8]) -> bool {
        if candidate.len() != 32 {
            return false;
        }
        self.digest().as_slice().ct_eq(candidate).into()
    }
}

impl TryFrom<&str> for SessionToken {
    type Error = SessionTokenError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from_hex(value)
    }
}

impl TryFrom<String> for SessionToken {
    type Error = SessionTokenError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from_hex(&value)
    }
}

impl FromStr for SessionToken {
    type Err = SessionTokenError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from_hex(value)
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken([REDACTED])")
    }
}

impl Drop for SessionToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionExpiryError {
    InvalidDuration,
    Overflow,
}

impl fmt::Display for SessionExpiryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidDuration => "session expiry duration must be positive",
            Self::Overflow => "session expiry is outside the supported time range",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SessionExpiryError {}

/// Persisted absolute expiry for a browser session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionExpiry(Timestamp);

impl SessionExpiry {
    #[must_use]
    pub const fn new(expires_at: Timestamp) -> Self {
        Self(expires_at)
    }

    pub fn from_now(observed_at: Timestamp, ttl: Duration) -> Result<Self, SessionExpiryError> {
        if ttl.is_zero() {
            return Err(SessionExpiryError::InvalidDuration);
        }
        let seconds = i64::try_from(ttl.as_secs()).map_err(|_| SessionExpiryError::Overflow)?;
        let nanos = i32::try_from(ttl.subsec_nanos()).map_err(|_| SessionExpiryError::Overflow)?;
        let duration = time::Duration::new(seconds, nanos);
        let expires_at = observed_at
            .as_offset_datetime()
            .checked_add(duration)
            .ok_or(SessionExpiryError::Overflow)?;
        Ok(Self(Timestamp::from_offset_datetime(expires_at)))
    }

    #[must_use]
    pub const fn expires_at(self) -> Timestamp {
        self.0
    }

    #[must_use]
    pub fn is_expired_at(self, observed_at: Timestamp) -> bool {
        observed_at.as_offset_datetime() >= self.0.as_offset_datetime()
    }

    #[must_use]
    pub fn is_active_at(self, observed_at: Timestamp) -> bool {
        !self.is_expired_at(observed_at)
    }
}

/// Minimal identity passed from authentication to later authorization.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionPrincipal {
    user_id: UserId,
    is_instance_admin: bool,
    session_id: SessionId,
}

impl SessionPrincipal {
    #[must_use]
    pub const fn new(user_id: UserId, is_instance_admin: bool, session_id: SessionId) -> Self {
        Self {
            user_id,
            is_instance_admin,
            session_id,
        }
    }

    #[must_use]
    pub const fn user_id(self) -> UserId {
        self.user_id
    }

    #[must_use]
    pub const fn is_instance_admin(self) -> bool {
        self.is_instance_admin
    }

    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
}

/// The one-time result of successful password authentication. The raw token
/// is retained only by the caller and is never represented by a metadata row.
#[derive(Clone, Eq, PartialEq)]
pub struct SessionCredential {
    session_id: SessionId,
    token: SessionToken,
    expires_at: SessionExpiry,
}

impl SessionCredential {
    pub(crate) const fn new(
        session_id: SessionId,
        token: SessionToken,
        expires_at: SessionExpiry,
    ) -> Self {
        Self {
            session_id,
            token,
            expires_at,
        }
    }

    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    #[must_use]
    pub const fn token(&self) -> &SessionToken {
        &self.token
    }

    #[must_use]
    pub const fn expires_at(&self) -> SessionExpiry {
        self.expires_at
    }

    #[must_use]
    pub fn into_token(self) -> SessionToken {
        self.token
    }
}

impl fmt::Debug for SessionCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionCredential")
            .field("session_id", &self.session_id)
            .field("token", &self.token)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Successful login result containing both the new credential and the
/// principal that was authenticated by it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSession {
    credential: SessionCredential,
    principal: SessionPrincipal,
}

impl AuthenticatedSession {
    pub(crate) const fn new(credential: SessionCredential, principal: SessionPrincipal) -> Self {
        Self {
            credential,
            principal,
        }
    }

    #[must_use]
    pub const fn credential(&self) -> &SessionCredential {
        &self.credential
    }

    #[must_use]
    pub const fn principal(&self) -> SessionPrincipal {
        self.principal
    }

    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.credential.session_id()
    }

    #[must_use]
    pub const fn token(&self) -> &SessionToken {
        self.credential.token()
    }

    #[must_use]
    pub const fn expires_at(&self) -> SessionExpiry {
        self.credential.expires_at()
    }
}

/// Naming used by future transport adapters without making them part of this
/// transport-neutral crate's implementation.
pub type BrowserSession = AuthenticatedSession;

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

pub(crate) fn truncate_to_microseconds(value: Timestamp) -> Timestamp {
    let datetime = value.as_offset_datetime();
    let nanos = datetime.nanosecond();
    Timestamp::from_offset_datetime(
        datetime
            .replace_nanosecond(nanos / 1_000 * 1_000)
            .expect("truncated nanoseconds remain valid"),
    )
}

#[cfg(test)]
mod tests {
    use synveil_core::{Timestamp, UserId};

    use super::{
        SESSION_TOKEN_BYTES, SessionExpiry, SessionId, SessionIdError, SessionPrincipal,
        SessionToken,
    };

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("test timestamp is valid")
    }

    #[test]
    fn session_token_generation_is_random_and_debug_is_redacted() {
        let first = SessionToken::generate();
        let second = SessionToken::generate();

        assert_ne!(first, second);
        assert_eq!(first.as_bytes().len(), SESSION_TOKEN_BYTES);
        let debug = format!("{first:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains(&first.to_hex()));
    }

    #[test]
    fn session_token_digest_is_deterministic_and_token_specific() {
        let first = SessionToken::from_bytes([0x11; SESSION_TOKEN_BYTES]);
        let same = SessionToken::from_bytes([0x11; SESSION_TOKEN_BYTES]);
        let other = SessionToken::from_bytes([0x12; SESSION_TOKEN_BYTES]);

        assert_eq!(first.digest(), same.digest());
        assert_ne!(first.digest(), other.digest());
        assert!(first.digest_matches(&same.digest()));
        assert!(!first.digest_matches(&other.digest()));
        assert!(!first.digest_matches(&[0_u8; 31]));
    }

    #[test]
    fn session_token_transport_encoding_round_trips_without_display() {
        let token = SessionToken::from_bytes([0xab; SESSION_TOKEN_BYTES]);
        let encoded = token.to_hex();

        assert_eq!(encoded.len(), SESSION_TOKEN_BYTES * 2);
        assert_eq!(SessionToken::try_from_hex(&encoded), Ok(token.clone()));
        assert_eq!(encoded.parse::<SessionToken>(), Ok(token));
        assert!(SessionToken::try_from_hex("not-a-token").is_err());
    }

    #[test]
    fn session_expiry_distinguishes_active_and_expired_instants() {
        let now = timestamp("2026-08-22T00:00:00Z");
        let expiry = SessionExpiry::from_now(now, std::time::Duration::from_secs(60))
            .expect("test expiry is valid");

        assert!(expiry.is_active_at(timestamp("2026-08-22T00:00:59Z")));
        assert!(expiry.is_expired_at(timestamp("2026-08-22T00:01:00Z")));
        assert!(!expiry.is_active_at(timestamp("2026-08-22T00:01:00Z")));
    }

    #[test]
    fn session_principal_contains_only_intended_identity_fields() {
        let user_id = UserId::new();
        let session_id = SessionId::new();
        let principal = SessionPrincipal::new(user_id, true, session_id);

        assert_eq!(principal.user_id(), user_id);
        assert!(principal.is_instance_admin());
        assert_eq!(principal.session_id(), session_id);
    }

    #[test]
    fn session_ids_are_uuidv7_and_reject_other_versions() {
        let id = SessionId::new();
        assert_eq!(SessionId::try_from_uuid(*id.as_uuid()), Ok(id));
        assert_eq!(
            id.to_string().to_ascii_uppercase().parse::<SessionId>(),
            Err(SessionIdError::NonCanonical)
        );
        assert_eq!(
            SessionId::try_from_uuid(uuid::Uuid::nil()),
            Err(SessionIdError::NotUuidV7)
        );
    }
}
