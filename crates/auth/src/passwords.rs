//! Argon2id password boundaries and parameter policy.

use std::fmt;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

/// Maximum UTF-8 byte length accepted for an application password.
pub const MAX_PASSWORD_BYTES: usize = 1_024;

/// Explicit interactive-server baseline. Production calibration remains a
/// deployment decision, while the stored PHC string preserves the parameters
/// used for each credential.
pub const DEFAULT_PASSWORD_MEMORY_KIB: u32 = 64 * 1_024;
pub const DEFAULT_PASSWORD_TIME_COST: u32 = 3;
pub const DEFAULT_PASSWORD_PARALLELISM: u32 = 1;
pub const DEFAULT_PASSWORD_OUTPUT_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordError {
    Empty,
    TooLong,
    InvalidParameters,
    HashingFailed,
    InvalidStoredHash,
    UnsupportedAlgorithm,
}

impl fmt::Display for PasswordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "password is empty",
            Self::TooLong => "password exceeds the accepted length bound",
            Self::InvalidParameters => "password hashing parameters are invalid",
            Self::HashingFailed => "password hashing failed",
            Self::InvalidStoredHash => "stored password hash is invalid",
            Self::UnsupportedAlgorithm => "stored password hash uses an unsupported algorithm",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PasswordError {}

/// Plaintext exists only inside the authentication boundary and is cleared on
/// drop on a best-effort basis. This wrapper deliberately has no revealing
/// debug or display representation.
pub struct PlaintextPassword(Zeroizing<String>);

impl PlaintextPassword {
    pub fn new(value: impl Into<String>) -> Result<Self, PasswordError> {
        let value = Zeroizing::new(value.into());
        if value.is_empty() {
            return Err(PasswordError::Empty);
        }
        if value.len() > MAX_PASSWORD_BYTES {
            return Err(PasswordError::TooLong);
        }
        Ok(Self(value))
    }

    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for PlaintextPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PlaintextPassword([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PasswordParameters {
    memory_kib: u32,
    time_cost: u32,
    parallelism: u32,
    output_bytes: usize,
}

impl PasswordParameters {
    pub fn new(
        memory_kib: u32,
        time_cost: u32,
        parallelism: u32,
        output_bytes: usize,
    ) -> Result<Self, PasswordError> {
        Params::new(memory_kib, time_cost, parallelism, Some(output_bytes))
            .map_err(|_| PasswordError::InvalidParameters)?;
        Ok(Self {
            memory_kib,
            time_cost,
            parallelism,
            output_bytes,
        })
    }

    #[must_use]
    pub const fn memory_kib(self) -> u32 {
        self.memory_kib
    }

    #[must_use]
    pub const fn time_cost(self) -> u32 {
        self.time_cost
    }

    #[must_use]
    pub const fn parallelism(self) -> u32 {
        self.parallelism
    }

    #[must_use]
    pub const fn output_bytes(self) -> usize {
        self.output_bytes
    }

    fn to_argon2_params(self) -> Result<Params, PasswordError> {
        Params::new(
            self.memory_kib,
            self.time_cost,
            self.parallelism,
            Some(self.output_bytes),
        )
        .map_err(|_| PasswordError::InvalidParameters)
    }
}

impl Default for PasswordParameters {
    fn default() -> Self {
        Self::new(
            DEFAULT_PASSWORD_MEMORY_KIB,
            DEFAULT_PASSWORD_TIME_COST,
            DEFAULT_PASSWORD_PARALLELISM,
            DEFAULT_PASSWORD_OUTPUT_BYTES,
        )
        .expect("documented default Argon2id parameters must be valid")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PasswordHasherConfig {
    parameters: PasswordParameters,
}

impl PasswordHasherConfig {
    pub fn new(parameters: PasswordParameters) -> Result<Self, PasswordError> {
        parameters.to_argon2_params()?;
        Ok(Self { parameters })
    }

    #[must_use]
    pub const fn parameters(self) -> PasswordParameters {
        self.parameters
    }

    fn argon2(self) -> Result<Argon2<'static>, PasswordError> {
        Ok(Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            self.parameters.to_argon2_params()?,
        ))
    }

    fn needs_rehash(self, stored: &StoredPasswordHash) -> Result<bool, PasswordError> {
        let parsed = stored.parse()?;
        let stored_params =
            Params::try_from(&parsed).map_err(|_| PasswordError::InvalidStoredHash)?;
        Ok(parsed.version != Some(u32::from(Version::V0x13))
            || stored_params != self.parameters.to_argon2_params()?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordVerification {
    Verified { needs_rehash: bool },
    Rejected,
}

/// Owned PHC password verifier. The string is never formatted through `Debug`.
#[derive(Clone, Eq, PartialEq)]
pub struct StoredPasswordHash(String);

impl StoredPasswordHash {
    pub fn try_from_phc(value: impl Into<String>) -> Result<Self, PasswordError> {
        let value = value.into();
        if value.is_empty() || value.len() > 4_096 {
            return Err(PasswordError::InvalidStoredHash);
        }
        let parsed = PasswordHash::new(&value).map_err(|_| PasswordError::InvalidStoredHash)?;
        if parsed.algorithm.as_str() != "argon2id" {
            return Err(PasswordError::UnsupportedAlgorithm);
        }
        if parsed.salt.is_none() || parsed.hash.is_none() {
            return Err(PasswordError::InvalidStoredHash);
        }
        Ok(Self(value))
    }

    pub fn hash(
        config: PasswordHasherConfig,
        password: &PlaintextPassword,
    ) -> Result<Self, PasswordError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = config.argon2()?;
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| PasswordError::HashingFailed)?
            .to_string();
        Self::try_from_phc(hash)
    }

    pub fn verify(
        &self,
        config: PasswordHasherConfig,
        password: &PlaintextPassword,
    ) -> Result<PasswordVerification, PasswordError> {
        let parsed = self.parse()?;
        let argon2 = config.argon2()?;
        if argon2
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Ok(PasswordVerification::Rejected);
        }
        Ok(PasswordVerification::Verified {
            needs_rehash: config.needs_rehash(self)?,
        })
    }

    #[must_use]
    pub fn as_phc_str(&self) -> &str {
        &self.0
    }

    fn parse(&self) -> Result<PasswordHash<'_>, PasswordError> {
        PasswordHash::new(&self.0).map_err(|_| PasswordError::InvalidStoredHash)
    }
}

impl fmt::Debug for StoredPasswordHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("StoredPasswordHash([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password(value: &str) -> PlaintextPassword {
        PlaintextPassword::new(value).expect("test password is within the bound")
    }

    #[test]
    fn parameters_are_centralized_and_explicit() {
        let config = PasswordHasherConfig::default();
        assert_eq!(
            config.parameters().memory_kib(),
            DEFAULT_PASSWORD_MEMORY_KIB
        );
        assert_eq!(config.parameters().time_cost(), DEFAULT_PASSWORD_TIME_COST);
        assert_eq!(
            config.parameters().parallelism(),
            DEFAULT_PASSWORD_PARALLELISM
        );
        assert_eq!(
            config.parameters().output_bytes(),
            DEFAULT_PASSWORD_OUTPUT_BYTES
        );
    }

    #[test]
    fn argon2id_hash_verifies_and_wrong_password_is_rejected() {
        let config = PasswordHasherConfig::default();
        let hash = StoredPasswordHash::hash(config, &password("correct horse battery staple"))
            .expect("hashing succeeds");

        assert!(hash.as_phc_str().starts_with("$argon2id$"));
        assert_eq!(
            hash.verify(config, &password("correct horse battery staple")),
            Ok(PasswordVerification::Verified {
                needs_rehash: false
            })
        );
        assert_eq!(
            hash.verify(config, &password("wrong password")),
            Ok(PasswordVerification::Rejected)
        );
    }

    #[test]
    fn same_password_gets_random_salts() {
        let config = PasswordHasherConfig::default();
        let first = StoredPasswordHash::hash(config, &password("same password"))
            .expect("first hash succeeds");
        let second = StoredPasswordHash::hash(config, &password("same password"))
            .expect("second hash succeeds");
        assert_ne!(first, second);
    }

    #[test]
    fn password_boundaries_and_debug_are_safe() {
        assert!(matches!(
            PlaintextPassword::new(""),
            Err(PasswordError::Empty)
        ));
        assert!(matches!(
            PlaintextPassword::new("x".repeat(MAX_PASSWORD_BYTES + 1)),
            Err(PasswordError::TooLong)
        ));
        let password = password("do not reveal me");
        assert!(!format!("{password:?}").contains("do not reveal me"));
    }

    #[test]
    fn invalid_or_non_argon2id_phc_is_rejected() {
        assert_eq!(
            StoredPasswordHash::try_from_phc("not-a-phc"),
            Err(PasswordError::InvalidStoredHash)
        );
        let argon2i = Argon2::new(
            Algorithm::Argon2i,
            Version::V0x13,
            Params::new(19 * 1_024, 2, 1, Some(DEFAULT_PASSWORD_OUTPUT_BYTES))
                .expect("test Argon2i parameters are valid"),
        );
        let salt = SaltString::generate(&mut OsRng);
        let argon2i_hash = argon2i
            .hash_password(b"test password", &salt)
            .expect("test Argon2i hash succeeds")
            .to_string();
        assert_eq!(
            StoredPasswordHash::try_from_phc(argon2i_hash),
            Err(PasswordError::UnsupportedAlgorithm)
        );
    }

    #[test]
    fn weaker_parameters_need_rehash() {
        let weaker = PasswordHasherConfig::new(
            PasswordParameters::new(32 * 1_024, 2, 1, DEFAULT_PASSWORD_OUTPUT_BYTES)
                .expect("weaker test parameters are valid"),
        )
        .expect("weaker config is valid");
        let current = PasswordHasherConfig::default();
        let hash = StoredPasswordHash::hash(weaker, &password("rehash me"))
            .expect("hashing with old parameters succeeds");
        assert_eq!(
            hash.verify(current, &password("rehash me")),
            Ok(PasswordVerification::Verified { needs_rehash: true })
        );
    }
}
