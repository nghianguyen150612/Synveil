use std::{
    env, fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{NodeState, Timestamp};

/// The single supported environment override for the logical Trash retention
/// policy. The value is an unsigned number of seconds so configuration is
/// explicit and does not depend on a locale or an ambiguous calendar unit.
pub const TRASH_RETENTION_SECONDS_ENV: &str = "SYNVEIL_TRASH_RETENTION_SECONDS";

/// Conservative personal-cloud default. This is a configurable deployment
/// default, not an immutable protocol guarantee.
pub const DEFAULT_TRASH_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrashRetentionPolicyError {
    ZeroDuration,
    DurationOutOfRange,
    InvalidEnvironmentValue,
}

impl fmt::Display for TrashRetentionPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroDuration => "Trash retention duration must be greater than zero",
            Self::DurationOutOfRange => "Trash retention duration is out of range",
            Self::InvalidEnvironmentValue => {
                "Trash retention configuration must be an unsigned number of seconds"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for TrashRetentionPolicyError {}

/// Authoritative logical Trash retention policy shared by metadata and API
/// composition roots. It contains no storage, HTTP, or operating-system
/// assumptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrashRetentionPolicy {
    retention_duration: Duration,
}

impl TrashRetentionPolicy {
    pub fn new(retention_duration: Duration) -> Result<Self, TrashRetentionPolicyError> {
        if retention_duration.is_zero() {
            return Err(TrashRetentionPolicyError::ZeroDuration);
        }
        if retention_duration.as_secs() > i64::MAX as u64
            || (retention_duration.as_secs() == i64::MAX as u64
                && retention_duration.subsec_nanos() != 0)
        {
            return Err(TrashRetentionPolicyError::DurationOutOfRange);
        }
        Ok(Self { retention_duration })
    }

    pub fn from_env() -> Result<Self, TrashRetentionPolicyError> {
        let Ok(value) = env::var(TRASH_RETENTION_SECONDS_ENV) else {
            return Ok(Self::default());
        };
        let seconds = value
            .parse::<u64>()
            .map_err(|_| TrashRetentionPolicyError::InvalidEnvironmentValue)?;
        Self::new(Duration::from_secs(seconds))
    }

    #[must_use]
    pub const fn retention_duration(self) -> Duration {
        self.retention_duration
    }

    /// Calculate the user-visible restore deadline without persisting a
    /// redundant derived timestamp.
    #[must_use]
    pub fn restore_deadline(self, trashed_at: Timestamp) -> Option<Timestamp> {
        trashed_at.checked_add_std(self.retention_duration)
    }

    /// Calculate the inclusive timestamp cutoff used by bounded candidate
    /// scans. The comparison is equivalent to `now >= trashed_at + retention`.
    #[must_use]
    pub fn eligibility_cutoff(self, now: Timestamp) -> Option<Timestamp> {
        now.checked_sub_std(self.retention_duration)
    }

    /// Evaluate only canonical state and server-observed timestamps. Client
    /// timestamps are not inputs to this rule.
    #[must_use]
    pub fn is_purge_eligible(
        self,
        state: NodeState,
        is_root: bool,
        trashed_at: Option<Timestamp>,
        now: Timestamp,
    ) -> bool {
        if is_root || state != NodeState::Trashed {
            return false;
        }
        let Some(trashed_at) = trashed_at else {
            return false;
        };
        let Some(deadline) = self.restore_deadline(trashed_at) else {
            return false;
        };
        now >= deadline
    }
}

impl Default for TrashRetentionPolicy {
    fn default() -> Self {
        Self {
            retention_duration: DEFAULT_TRASH_RETENTION,
        }
    }
}

macro_rules! logical_path {
    ($name:ident) => {
        /// A logical path supplied by configuration; platform resolution is external to core.
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(PathBuf);

        impl $name {
            #[must_use]
            pub fn new(path: impl Into<PathBuf>) -> Self {
                Self(path.into())
            }

            #[must_use]
            pub fn as_path(&self) -> &Path {
                &self.0
            }

            #[must_use]
            pub fn into_path_buf(self) -> PathBuf {
                self.0
            }
        }

        impl From<PathBuf> for $name {
            fn from(path: PathBuf) -> Self {
                Self::new(path)
            }
        }

        impl AsRef<Path> for $name {
            fn as_ref(&self) -> &Path {
                self.as_path()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.display().fmt(formatter)
            }
        }
    };
}

logical_path!(DataDir);
logical_path!(ConfigDir);
logical_path!(CacheDir);
logical_path!(RuntimeDir);

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crate::{NodeState, Timestamp};

    use super::{
        CacheDir, ConfigDir, DEFAULT_TRASH_RETENTION, DataDir, RuntimeDir, TrashRetentionPolicy,
        TrashRetentionPolicyError,
    };

    #[test]
    fn logical_paths_preserve_portable_path_values() {
        let path = PathBuf::from("synveil").join("data");

        assert_eq!(DataDir::new(path.clone()).as_path(), path.as_path());
        assert_eq!(ConfigDir::from(path.clone()).into_path_buf(), path);
        assert_eq!(CacheDir::new("cache").to_string(), "cache");
        assert_eq!(
            RuntimeDir::new("runtime").as_ref(),
            std::path::Path::new("runtime")
        );
    }

    #[test]
    fn trash_retention_defaults_to_thirty_days_and_rejects_unsafe_durations() {
        assert_eq!(
            TrashRetentionPolicy::default().retention_duration(),
            DEFAULT_TRASH_RETENTION
        );
        assert_eq!(
            TrashRetentionPolicy::new(Duration::ZERO),
            Err(TrashRetentionPolicyError::ZeroDuration)
        );
        assert_eq!(
            TrashRetentionPolicy::new(Duration::new(i64::MAX as u64, 1)),
            Err(TrashRetentionPolicyError::DurationOutOfRange)
        );
    }

    #[test]
    fn trash_purge_eligibility_is_inclusive_and_state_scoped() {
        let policy = TrashRetentionPolicy::new(Duration::from_secs(10)).unwrap();
        let trashed_at = Timestamp::parse("2026-08-24T00:00:00Z").unwrap();
        let deadline = Timestamp::parse("2026-08-24T00:00:10Z").unwrap();
        let after_deadline = Timestamp::parse("2026-08-24T00:00:11Z").unwrap();

        assert!(!policy.is_purge_eligible(
            NodeState::Trashed,
            false,
            Some(trashed_at),
            Timestamp::parse("2026-08-24T00:00:09Z").unwrap()
        ));
        assert!(policy.is_purge_eligible(NodeState::Trashed, false, Some(trashed_at), deadline));
        assert!(policy.is_purge_eligible(
            NodeState::Trashed,
            false,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(
            NodeState::Active,
            false,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(
            NodeState::Purging,
            false,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(
            NodeState::Trashed,
            true,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(NodeState::Trashed, false, None, after_deadline));
        assert_eq!(policy.restore_deadline(trashed_at), Some(deadline));
    }
}
