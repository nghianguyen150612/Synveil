use std::fmt;

use synveil_metadata::DatabaseConfig;
use zeroize::Zeroizing;

/// Fixed, bounded names understood by the runtime and provisioning boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CredentialId {
    DatabaseUrl,
    DatabasePassword,
    RebaselineTokenKey,
    NetworkCaKey,
    NetworkTlsKey,
}

impl CredentialId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DatabaseUrl => "database-url",
            Self::DatabasePassword => "database-password",
            Self::RebaselineTokenKey => "rebaseline-token-key",
            Self::NetworkCaKey => "network-ca-key",
            Self::NetworkTlsKey => "network-tls-key",
        }
    }
}

impl serde::Serialize for CredentialId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for CredentialId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "database-url" => Ok(Self::DatabaseUrl),
            "database-password" => Ok(Self::DatabasePassword),
            "rebaseline-token-key" => Ok(Self::RebaselineTokenKey),
            "network-ca-key" => Ok(Self::NetworkCaKey),
            "network-tls-key" => Ok(Self::NetworkTlsKey),
            _ => Err(serde::de::Error::custom("unsupported credential reference")),
        }
    }
}

/// A short-lived secret value. Formatting never reveals its contents.
pub struct SecretMaterial(Zeroizing<String>);

impl SecretMaterial {
    pub(crate) fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    #[must_use]
    pub(crate) fn expose_for_storage(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Run a narrowly scoped operation over the secret without returning or
    /// formatting an owned plaintext copy. The wrapper remains redacted.
    pub fn with_secret<T>(&self, use_secret: impl FnOnce(&str) -> T) -> T {
        use_secret(&self.0)
    }
}

impl fmt::Debug for SecretMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretMaterial([REDACTED])")
    }
}

impl fmt::Display for SecretMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// Validated administrator-supplied external PostgreSQL URL.
pub struct ExternalDatabaseCredential(SecretMaterial);

impl ExternalDatabaseCredential {
    pub fn new(value: String) -> Result<Self, ExternalDatabaseCredentialError> {
        let mut value = Zeroizing::new(value);
        if value.is_empty()
            || value.len() > MAX_EXTERNAL_DATABASE_URL_BYTES
            || value.as_bytes().contains(&0)
            || value.contains(['\n', '\r'])
            || value.trim() != value.as_str()
        {
            return Err(ExternalDatabaseCredentialError);
        }
        DatabaseConfig::from_url(value.to_string()).map_err(|_| ExternalDatabaseCredentialError)?;
        Ok(Self(SecretMaterial::new(std::mem::take(&mut *value))))
    }

    pub(crate) fn expose_for_storage(&self) -> &[u8] {
        self.0.expose_for_storage()
    }

    pub(crate) fn non_secret_endpoint(&self) -> Option<crate::DatabaseEndpoint> {
        let value = std::str::from_utf8(self.0.expose_for_storage()).ok()?;
        let parsed = url::Url::parse(value).ok()?;
        if parsed.scheme() != "postgres" && parsed.scheme() != "postgresql" {
            return None;
        }
        Some(crate::DatabaseEndpoint {
            host: parsed.host_str()?.to_owned(),
            port: parsed.port().unwrap_or(5432),
        })
    }
}

impl fmt::Debug for ExternalDatabaseCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExternalDatabaseCredential([REDACTED])")
    }
}

impl fmt::Display for ExternalDatabaseCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalDatabaseCredentialError;

impl fmt::Display for ExternalDatabaseCredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("external database credential is invalid")
    }
}

impl std::error::Error for ExternalDatabaseCredentialError {}

pub const MAX_EXTERNAL_DATABASE_URL_BYTES: usize = 8 * 1024;

/// Safe non-secret identifier recorded in SERVER_CONFIG.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecretReference(CredentialId);

impl SecretReference {
    #[must_use]
    pub const fn new(id: CredentialId) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn id(self) -> CredentialId {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialId, ExternalDatabaseCredential, SecretMaterial};

    #[test]
    fn credential_ids_are_closed_and_round_trip() {
        assert_eq!(
            serde_json::to_string(&CredentialId::DatabaseUrl).unwrap(),
            "\"database-url\""
        );
        assert!(serde_json::from_str::<CredentialId>("\"../secret\"").is_err());
        assert!(serde_json::from_str::<CredentialId>("\"database-url\"").is_ok());
    }

    #[test]
    fn secret_wrappers_redact_canaries() {
        let canary = "CANARY-external-db-password";
        let material = SecretMaterial::new(canary.to_owned());
        let external = ExternalDatabaseCredential::new(
            "postgresql://user:CANARY-external-db-password@example.invalid/db".to_owned(),
        )
        .unwrap();
        for formatted in [
            format!("{material:?}"),
            format!("{material}"),
            format!("{external:?}"),
            format!("{external}"),
        ] {
            assert!(!formatted.contains(canary));
        }
    }

    #[test]
    fn external_database_credential_requires_postgresql_without_echoing_input() {
        let canary = "CANARY-invalid-credential";
        let error = ExternalDatabaseCredential::new(canary.to_owned()).unwrap_err();
        assert_eq!(error.to_string(), "external database credential is invalid");
        assert!(!format!("{error:?}").contains(canary));
    }
}
