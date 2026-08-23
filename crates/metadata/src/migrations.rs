use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use sqlx::{Row, migrate::Migrator};

use crate::{DatabaseError, DatabasePool};

/// Ordered migration status without exposing SQLx row types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationStatus {
    applied_versions: Vec<i64>,
    pending_versions: Vec<i64>,
    unknown_versions: Vec<i64>,
    failed_versions: Vec<i64>,
}

impl MigrationStatus {
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.pending_versions.is_empty()
            && self.unknown_versions.is_empty()
            && self.failed_versions.is_empty()
    }

    #[must_use]
    pub fn applied_versions(&self) -> &[i64] {
        &self.applied_versions
    }

    #[must_use]
    pub fn pending_versions(&self) -> &[i64] {
        &self.pending_versions
    }

    #[must_use]
    pub fn unknown_versions(&self) -> &[i64] {
        &self.unknown_versions
    }

    #[must_use]
    pub fn failed_versions(&self) -> &[i64] {
        &self.failed_versions
    }

    #[must_use]
    pub fn latest_applied_version(&self) -> Option<i64> {
        self.applied_versions.iter().copied().max()
    }
}

/// One-shot forward migration runner. SQLx supplies the PostgreSQL migration
/// lock and checksum validation; this wrapper keeps those details out of the
/// application/domain boundary.
#[derive(Clone, Debug)]
pub struct MigrationRunner {
    directory: PathBuf,
}

impl Default for MigrationRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::from_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations"))
    }

    /// Construct a runner from an explicitly supplied migration directory.
    ///
    /// The path is an adapter/runtime concern so deployment can package or
    /// mount the reviewed migration set without putting filesystem assumptions
    /// into core or application code.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            directory: path.into(),
        }
    }

    async fn migrator(&self) -> Result<Migrator, DatabaseError> {
        Migrator::new(self.directory.clone())
            .await
            .map_err(DatabaseError::migration)
    }

    pub async fn run(&self, pool: &DatabasePool) -> Result<MigrationStatus, DatabaseError> {
        let migrator = self.migrator().await?;
        migrator
            .run(pool.sqlx_pool())
            .await
            .map_err(DatabaseError::migration)?;
        self.status(pool).await
    }

    pub async fn status(&self, pool: &DatabasePool) -> Result<MigrationStatus, DatabaseError> {
        let migrator = self.migrator().await?;
        let rows =
            sqlx::query("SELECT version, success FROM _sqlx_migrations ORDER BY version ASC")
                .fetch_all(pool.sqlx_pool())
                .await
                .map_err(DatabaseError::migration_state)?;

        let expected: BTreeSet<i64> = migrator.iter().map(|migration| migration.version).collect();
        let mut applied_versions = Vec::new();
        let mut failed_versions = Vec::new();

        for row in rows {
            let version = row
                .try_get::<i64, _>("version")
                .map_err(|_| DatabaseError::migration_state_decode())?;
            let success = row
                .try_get::<bool, _>("success")
                .map_err(|_| DatabaseError::migration_state_decode())?;

            if success {
                applied_versions.push(version);
            } else {
                failed_versions.push(version);
            }
        }

        let applied: BTreeSet<i64> = applied_versions.iter().copied().collect();
        let failed: BTreeSet<i64> = failed_versions.iter().copied().collect();
        let recorded: BTreeSet<i64> = applied.union(&failed).copied().collect();
        let pending_versions = expected.difference(&applied).copied().collect();
        let unknown_versions = recorded.difference(&expected).copied().collect();

        Ok(MigrationStatus {
            applied_versions,
            pending_versions,
            unknown_versions,
            failed_versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MigrationRunner, MigrationStatus};

    #[test]
    fn default_runner_targets_the_workspace_migration_directory() {
        let runner = MigrationRunner::new();
        assert!(runner.directory.ends_with("migrations"));
    }

    #[test]
    fn status_is_current_only_when_no_unapplied_or_unknown_versions_exist() {
        let status = MigrationStatus {
            applied_versions: vec![1],
            pending_versions: Vec::new(),
            unknown_versions: Vec::new(),
            failed_versions: Vec::new(),
        };
        assert!(status.is_current());

        let status = MigrationStatus {
            pending_versions: vec![2],
            ..status
        };
        assert!(!status.is_current());
    }
}
