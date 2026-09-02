use std::fmt;
use zeroize::Zeroize;

/// A non-secret identifier for a value held by a secure credential backend.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecretName(String);

impl SecretName {
    pub fn new(value: impl Into<String>) -> Result<Self, SecretNameError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SecretNameError::Empty);
        }
        if value.chars().count() > 256 {
            return Err(SecretNameError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(SecretNameError::ControlCharacter);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for SecretName {
    type Error = SecretNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for SecretName {
    type Error = SecretNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl AsRef<str> for SecretName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validation failures for a secret identifier. Secret material is never
/// included in an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretNameError {
    Empty,
    TooLong,
    ControlCharacter,
}

impl fmt::Display for SecretNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "secret name is empty",
            Self::TooLong => "secret name is too long",
            Self::ControlCharacter => "secret name contains a control character",
        })
    }
}

impl std::error::Error for SecretNameError {}

/// Owned secret material. It has no ordinary `Debug` representation and is
/// cleared before its allocation is released.
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    #[must_use]
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl PartialEq for SecretValue {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for SecretValue {}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue(<redacted>)")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Availability of a secure secret backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretStoreState {
    Available,
    Unsupported,
    Unavailable,
}

/// Errors returned by the secret boundary. None can contain secret material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretStoreError {
    Unsupported,
    Unavailable,
    InvalidName,
    EmptySecret,
    SecretTooLarge,
}

impl fmt::Display for SecretStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unsupported => "secure secret storage is unsupported",
            Self::Unavailable => "secure secret storage is unavailable",
            Self::InvalidName => "secret name is invalid",
            Self::EmptySecret => "secret value is empty",
            Self::SecretTooLarge => "secret value exceeds secure storage limit",
        })
    }
}

impl std::error::Error for SecretStoreError {}

/// Secure storage port. Implementations must use an OS-backed or otherwise
/// reviewed secure backend; this trait intentionally has no disk fallback.
pub trait SecretStore: Send + Sync {
    fn state(&self) -> SecretStoreState;

    fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError>;

    fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError>;

    fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError>;
}

/// A safe placeholder until a reviewed Keychain, Credential Manager, Secret
/// Service, or equivalent backend is implemented.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnsupportedSecureSecretStore;

impl UnsupportedSecureSecretStore {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SecretStore for UnsupportedSecureSecretStore {
    fn state(&self) -> SecretStoreState {
        SecretStoreState::Unsupported
    }

    fn put_secret(&self, _name: &SecretName, _value: &[u8]) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported)
    }

    fn get_secret(&self, _name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
        Err(SecretStoreError::Unsupported)
    }

    fn delete_secret(&self, _name: &SecretName) -> Result<bool, SecretStoreError> {
        Err(SecretStoreError::Unsupported)
    }
}
