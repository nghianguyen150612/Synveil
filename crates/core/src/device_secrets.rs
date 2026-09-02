//! Portable, bounded machine-secret types shared by server and desktop.
//!
//! Generation belongs to the authentication adapter's existing OS CSPRNG.
//! Core only validates, zeroizes, and derives domain-separated verifiers.

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

pub const DEVICE_SECRET_ENTROPY_BYTES: usize = 32;
/// Five version/purpose bytes plus 64 canonical lowercase hex bytes.
pub const DEVICE_SECRET_ENCODED_BYTES: usize = 69;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceSecretError {
    InvalidLength,
    InvalidFormat,
}

impl fmt::Display for DeviceSecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength => "device secret has an invalid length",
            Self::InvalidFormat => "device secret has an invalid format",
        })
    }
}

impl std::error::Error for DeviceSecretError {}

macro_rules! machine_secret {
    ($name:ident, $prefix:literal, $domain:literal) => {
        /// Opaque 256-bit machine secret, with an explicit purpose/version.
        ///
        /// No `Display` or serialization implementation exists. Only the
        /// header/enrollment/secure-storage boundary may explicitly expose it.
        #[derive(Clone)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, DeviceSecretError> {
                // Check bytes before allocation and before slicing untrusted
                // UTF-8. A valid payload is ASCII only and exactly bounded.
                if value.len() != DEVICE_SECRET_ENCODED_BYTES {
                    return Err(DeviceSecretError::InvalidLength);
                }
                let Some(payload) = value.strip_prefix($prefix) else {
                    return Err(DeviceSecretError::InvalidFormat);
                };
                if !payload.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
                    return Err(DeviceSecretError::InvalidFormat);
                }
                Ok(Self(value.to_owned()))
            }

            /// Encode bytes supplied by a secure random generator (or an
            /// explicit test fixture). A UUID is never used as secret entropy.
            #[must_use]
            pub fn from_bytes(mut bytes: [u8; DEVICE_SECRET_ENTROPY_BYTES]) -> Self {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let mut value = String::with_capacity(DEVICE_SECRET_ENCODED_BYTES);
                value.push_str($prefix);
                for byte in &bytes {
                    value.push(char::from(HEX[usize::from(byte >> 4)]));
                    value.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
                bytes.zeroize();
                Self(value)
            }

            /// Secret presentation boundary. Never log, serialize into a
            /// profile/SQLite record, or append this value to a URL.
            #[must_use]
            pub fn expose_secret(&self) -> &str {
                &self.0
            }

            /// SHA-256 is appropriate for uniformly random 256-bit tokens:
            /// password stretching does not improve their brute-force
            /// resistance. The purpose/version domain prevents cross-protocol
            /// verifier reuse; PostgreSQL receives only these 32 digest bytes.
            #[must_use]
            pub fn digest(&self) -> [u8; 32] {
                let mut hasher = Sha256::new();
                hasher.update($domain);
                hasher.update(self.0.as_bytes());
                hasher.finalize().into()
            }

            #[must_use]
            pub fn digest_matches(&self, candidate: &[u8]) -> bool {
                candidate.len() == 32 && bool::from(self.digest().as_slice().ct_eq(candidate))
            }
        }

        impl FromStr for $name {
            type Err = DeviceSecretError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
            }
        }

        impl Eq for $name {}

        impl Drop for $name {
            fn drop(&mut self) {
                self.0.zeroize();
            }
        }
    };
}

machine_secret!(
    DeviceCredentialSecret,
    "svd1_",
    b"synveil.device-credential.v1\0"
);
machine_secret!(
    EnrollmentSecret,
    "sve1_",
    b"synveil.device-enrollment-grant.v1\0"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_and_enrollment_formats_are_bounded_distinct_and_redacted() {
        let credential = DeviceCredentialSecret::from_bytes([0xa5; 32]);
        let enrollment = EnrollmentSecret::from_bytes([0xa5; 32]);
        assert_eq!(
            credential.expose_secret().len(),
            DEVICE_SECRET_ENCODED_BYTES
        );
        assert_eq!(
            enrollment.expose_secret().len(),
            DEVICE_SECRET_ENCODED_BYTES
        );
        assert!(credential.expose_secret().starts_with("svd1_"));
        assert!(enrollment.expose_secret().starts_with("sve1_"));
        assert_eq!(
            DeviceCredentialSecret::parse(credential.expose_secret()).unwrap(),
            credential
        );
        assert_eq!(
            EnrollmentSecret::parse(enrollment.expose_secret()).unwrap(),
            enrollment
        );
        assert_eq!(
            format!("{credential:?}"),
            "DeviceCredentialSecret([REDACTED])"
        );
        assert_eq!(format!("{enrollment:?}"), "EnrollmentSecret([REDACTED])");
        for invalid in [
            "".to_owned(),
            "x".repeat(70),
            credential.expose_secret().to_ascii_uppercase(),
            credential.expose_secret().replace('a', "\n"),
            credential.expose_secret().replace('a', "g"),
        ] {
            assert!(DeviceCredentialSecret::parse(&invalid).is_err());
            assert!(EnrollmentSecret::parse(&invalid).is_err());
        }
        assert!(DeviceCredentialSecret::parse(enrollment.expose_secret()).is_err());
        assert!(EnrollmentSecret::parse(credential.expose_secret()).is_err());
    }

    #[test]
    fn secret_digests_are_stable_domain_separated_and_constant_time_checked() {
        let credential = DeviceCredentialSecret::from_bytes([7; 32]);
        let enrollment = EnrollmentSecret::from_bytes([7; 32]);
        // Independently computed with sha256sum over the purpose domain,
        // literal NUL separator, and complete versioned fixture token.
        assert_eq!(
            crate::Sha256Digest::from_bytes(credential.digest()).to_string(),
            "sha256:7fabfd4b3fa54642c31c7097be2f1fa0f312bda45686ccb6f600c6a83628cf1b"
        );
        assert_eq!(
            crate::Sha256Digest::from_bytes(enrollment.digest()).to_string(),
            "sha256:70eb786c7a40b93066919bf728522cc613cae3f06b4175d67162f3f0cf95e73c"
        );
        assert_eq!(credential.digest(), credential.clone().digest());
        assert_eq!(enrollment.digest(), enrollment.clone().digest());
        assert_ne!(credential.digest(), enrollment.digest());
        assert!(credential.digest_matches(&credential.digest()));
        assert!(enrollment.digest_matches(&enrollment.digest()));
        assert!(!credential.digest_matches(&enrollment.digest()));
        assert!(!enrollment.digest_matches(&credential.digest()));
        assert!(!credential.digest_matches(&[0; 31]));
        assert!(!credential.digest_matches(&[0; 33]));
        assert!(!credential.digest_matches(&DeviceCredentialSecret::from_bytes([8; 32]).digest()));
    }
}
