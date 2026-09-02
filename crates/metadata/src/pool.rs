use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{DatabaseConfig, DatabaseError, DatabaseReadiness, MigrationRunner};

/// PostgreSQL connection pool owned by the metadata adapter boundary.
#[derive(Clone)]
pub struct DatabasePool {
    pool: PgPool,
}

impl DatabasePool {
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let pool_config = config.pool_config();
        let pool = PgPoolOptions::new()
            .max_connections(pool_config.max_connections())
            .min_connections(pool_config.min_connections())
            .acquire_timeout(pool_config.acquire_timeout())
            .idle_timeout(pool_config.idle_timeout())
            .max_lifetime(pool_config.max_lifetime())
            .connect(config.database_url())
            .await
            .map_err(DatabaseError::connection)?;

        Ok(Self { pool })
    }

    /// Run a bounded `SELECT 1` probe without applying migrations or changing
    /// application state.
    pub async fn ping(&self) -> Result<(), DatabaseError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(DatabaseError::readiness)
    }

    /// Check one known relation name without exposing SQLx rows or provider
    /// errors. This is intentionally a bounded metadata probe for migration
    /// and readiness tests, not an arbitrary query surface.
    pub async fn table_exists(&self, relation: &str) -> Result<bool, DatabaseError> {
        sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(relation)
            .fetch_one(&self.pool)
            .await
            .map_err(DatabaseError::query)
    }

    pub async fn readiness(
        &self,
        migration_runner: &MigrationRunner,
    ) -> Result<DatabaseReadiness, DatabaseError> {
        self.ping().await?;
        let migration_status = migration_runner.status(self).await?;
        Ok(DatabaseReadiness::new(true, migration_status.is_current()))
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.pool.is_closed()
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    pub(crate) const fn sqlx_pool(&self) -> &PgPool {
        &self.pool
    }
}
