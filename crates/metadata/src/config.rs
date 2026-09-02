use std::{env, fmt, time::Duration};

use crate::DatabaseConfigError;

/// Conventional development/configuration environment variable. Production
/// deployments may provide the same value through a typed configuration source
/// or secret reference before constructing [`DatabaseConfig`].
pub const DATABASE_URL_ENV: &str = "DATABASE_URL";

/// Pool settings independent of the PostgreSQL connection URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PoolConfig {
    max_connections: u32,
    min_connections: u32,
    acquire_timeout: Duration,
    idle_timeout: Option<Duration>,
    max_lifetime: Option<Duration>,
}

impl PoolConfig {
    pub fn new(
        max_connections: u32,
        min_connections: u32,
        acquire_timeout: Duration,
    ) -> Result<Self, DatabaseConfigError> {
        let config = Self {
            max_connections,
            min_connections,
            acquire_timeout,
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(1_800)),
        };
        config.validate()?;
        Ok(config)
    }

    #[must_use]
    pub const fn max_connections(self) -> u32 {
        self.max_connections
    }

    #[must_use]
    pub const fn min_connections(self) -> u32 {
        self.min_connections
    }

    #[must_use]
    pub const fn acquire_timeout(self) -> Duration {
        self.acquire_timeout
    }

    #[must_use]
    pub const fn idle_timeout(self) -> Option<Duration> {
        self.idle_timeout
    }

    #[must_use]
    pub const fn max_lifetime(self) -> Option<Duration> {
        self.max_lifetime
    }

    pub fn with_idle_timeout(
        mut self,
        idle_timeout: Option<Duration>,
    ) -> Result<Self, DatabaseConfigError> {
        self.idle_timeout = idle_timeout;
        self.validate()?;
        Ok(self)
    }

    pub fn with_max_lifetime(
        mut self,
        max_lifetime: Option<Duration>,
    ) -> Result<Self, DatabaseConfigError> {
        self.max_lifetime = max_lifetime;
        self.validate()?;
        Ok(self)
    }

    fn validate(self) -> Result<(), DatabaseConfigError> {
        if self.max_connections == 0
            || self.min_connections > self.max_connections
            || self.acquire_timeout.is_zero()
            || self.idle_timeout.is_some_and(|value| value.is_zero())
            || self.max_lifetime.is_some_and(|value| value.is_zero())
        {
            return Err(DatabaseConfigError::InvalidPoolConfiguration);
        }
        Ok(())
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(5),
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(1_800)),
        }
    }
}

/// PostgreSQL connection configuration. The URL is intentionally private and
/// omitted from `Debug` output because it may contain credentials.
#[derive(Clone)]
pub struct DatabaseConfig {
    database_url: String,
    pool: PoolConfig,
}

impl DatabaseConfig {
    pub fn from_url(url: impl Into<String>) -> Result<Self, DatabaseConfigError> {
        let database_url = url.into();
        let database_url = database_url.trim();
        if database_url.is_empty() {
            return Err(DatabaseConfigError::EmptyUrl);
        }
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err(DatabaseConfigError::UnsupportedScheme);
        }

        Ok(Self {
            database_url: database_url.to_owned(),
            pool: PoolConfig::default(),
        })
    }

    pub fn from_env() -> Result<Self, DatabaseConfigError> {
        Self::from_env_var(DATABASE_URL_ENV)
    }

    pub fn from_env_var(variable: &'static str) -> Result<Self, DatabaseConfigError> {
        let url = env::var(variable)
            .map_err(|_| DatabaseConfigError::MissingEnvironmentVariable { variable })?;
        Self::from_url(url)
    }

    pub fn with_pool_config(mut self, pool: PoolConfig) -> Result<Self, DatabaseConfigError> {
        pool.validate()?;
        self.pool = pool;
        Ok(self)
    }

    #[must_use]
    pub const fn pool_config(&self) -> PoolConfig {
        self.pool
    }

    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseConfig")
            .field("database_url", &"<redacted>")
            .field("pool", &self.pool)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DatabaseConfig, PoolConfig};
    use crate::DatabaseConfigError;

    #[test]
    fn config_accepts_only_postgresql_urls() {
        assert!(DatabaseConfig::from_url("postgresql://user:secret@db/synveil").is_ok());
        assert!(matches!(
            DatabaseConfig::from_url("sqlite://synveil.db"),
            Err(DatabaseConfigError::UnsupportedScheme)
        ));
        assert!(matches!(
            DatabaseConfig::from_url(""),
            Err(DatabaseConfigError::EmptyUrl)
        ));
    }

    #[test]
    fn config_requires_an_explicit_environment_value() {
        assert!(matches!(
            DatabaseConfig::from_env_var("SYNVEIL_METADATA_TEST_DATABASE_URL_MUST_BE_PROVIDED"),
            Err(DatabaseConfigError::MissingEnvironmentVariable { variable })
                if variable == "SYNVEIL_METADATA_TEST_DATABASE_URL_MUST_BE_PROVIDED"
        ));
    }

    #[test]
    fn config_debug_redacts_the_connection_url() {
        let config = DatabaseConfig::from_url("postgresql://user:super-secret@db/synveil")
            .expect("valid PostgreSQL URL");
        let debug = format!("{config:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("postgresql://"));
    }

    #[test]
    fn pool_config_rejects_invalid_bounds_and_zero_timeouts() {
        assert_eq!(
            PoolConfig::new(0, 0, Duration::from_secs(1)),
            Err(DatabaseConfigError::InvalidPoolConfiguration)
        );
        assert_eq!(
            PoolConfig::new(1, 2, Duration::from_secs(1)),
            Err(DatabaseConfigError::InvalidPoolConfiguration)
        );
        assert_eq!(
            PoolConfig::new(1, 0, Duration::ZERO),
            Err(DatabaseConfigError::InvalidPoolConfiguration)
        );
    }
}
