//! Authentication configuration that is safe by default and transport-neutral.

use std::{fmt, time::Duration};

/// Conservative browser-session lifetime baseline. This is an implementation
/// default, not a public HTTP or cookie protocol guarantee.
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// Upper bound for a configured browser-session lifetime. A deployment that
/// needs a longer policy requires an explicit security review rather than an
/// accidental long-lived bearer credential.
pub const MAX_SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionConfigError {
    TtlMustBePositive,
    TtlExceedsMaximum,
}

impl fmt::Display for SessionConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TtlMustBePositive => "session_ttl_must_be_positive",
            Self::TtlExceedsMaximum => "session_ttl_exceeds_maximum",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SessionConfigError {}

/// Session policy used by the authentication service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionConfig {
    ttl: Duration,
}

impl SessionConfig {
    pub fn new(ttl: Duration) -> Result<Self, SessionConfigError> {
        if ttl.is_zero() {
            return Err(SessionConfigError::TtlMustBePositive);
        }
        if ttl > MAX_SESSION_TTL {
            return Err(SessionConfigError::TtlExceedsMaximum);
        }
        Ok(Self { ttl })
    }

    #[must_use]
    pub const fn ttl(self) -> Duration {
        self.ttl
    }

    pub(crate) fn expiry_at(
        self,
        observed_at: synveil_core::Timestamp,
    ) -> Result<super::SessionExpiry, super::SessionExpiryError> {
        super::SessionExpiry::from_now(observed_at, self.ttl)
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self::new(DEFAULT_SESSION_TTL).expect("documented session TTL must be valid")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DEFAULT_SESSION_TTL, MAX_SESSION_TTL, SessionConfig, SessionConfigError};

    #[test]
    fn default_session_ttl_is_centralized() {
        assert_eq!(SessionConfig::default().ttl(), DEFAULT_SESSION_TTL);
    }

    #[test]
    fn session_ttl_validation_rejects_zero_and_excessive_values() {
        assert_eq!(
            SessionConfig::new(Duration::ZERO),
            Err(SessionConfigError::TtlMustBePositive)
        );
        assert_eq!(
            SessionConfig::new(MAX_SESSION_TTL + Duration::from_secs(1)),
            Err(SessionConfigError::TtlExceedsMaximum)
        );
        assert!(SessionConfig::new(Duration::from_secs(1)).is_ok());
    }
}
