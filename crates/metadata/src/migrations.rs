use std::{collections::BTreeSet, path::PathBuf};

use sqlx::{Row, migrate::Migrator};

use crate::{DatabaseError, DatabasePool};

static EMBEDDED_MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

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
    source: MigrationSource,
}

#[derive(Clone, Debug)]
enum MigrationSource {
    Embedded,
    Directory(PathBuf),
}

impl Default for MigrationRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRunner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            source: MigrationSource::Embedded,
        }
    }

    /// Construct a runner from an explicitly supplied migration directory.
    ///
    /// The path is an adapter/runtime concern so deployment can package or
    /// mount the reviewed migration set without putting filesystem assumptions
    /// into core or application code.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            source: MigrationSource::Directory(path.into()),
        }
    }

    async fn migrator(&self) -> Result<Migrator, DatabaseError> {
        match &self.source {
            MigrationSource::Embedded => Ok(Migrator {
                migrations: EMBEDDED_MIGRATOR.migrations.clone(),
                ignore_missing: EMBEDDED_MIGRATOR.ignore_missing,
                locking: EMBEDDED_MIGRATOR.locking,
                no_tx: EMBEDDED_MIGRATOR.no_tx,
            }),
            MigrationSource::Directory(directory) => Migrator::new(directory.clone())
                .await
                .map_err(DatabaseError::migration),
        }
    }

    pub async fn run(&self, pool: &DatabasePool) -> Result<MigrationStatus, DatabaseError> {
        let migrator = self.migrator().await?;
        let mut connection = pool
            .sqlx_pool()
            .acquire()
            .await
            .map_err(DatabaseError::connection)?;
        if let Err(error) = migrator.run(&mut *connection).await {
            // SQLx returns early on migration errors before its advisory-lock
            // release. Do not return that session to the pool still locked.
            let _ = connection.close().await;
            return Err(DatabaseError::migration(error));
        }
        drop(connection);
        if matches!(self.source, MigrationSource::Embedded) {
            // SQLx verifies the ledger, but a recorded successful migration
            // does not prove its tables still exist after external damage.
            // Metadata-only probes reject missing current tables at startup.
            let missing: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM unnest($1::text[]) AS required(name)
                 WHERE NOT EXISTS (
                     SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                     WHERE n.nspname = current_schema() AND c.relname = required.name
                       AND c.relkind IN ('r', 'p')
                 )",
            )
            .bind(REQUIRED_TABLES)
            .fetch_one(pool.sqlx_pool())
            .await
            .map_err(DatabaseError::migration_state)?;
            if missing != 0 {
                return Err(DatabaseError::migration_state_decode());
            }
        }
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

const REQUIRED_TABLES: &[&str] = &[
    "users",
    "devices",
    "libraries",
    "objects",
    "nodes",
    "file_versions",
    "user_credentials",
    "bootstrap_state",
    "sessions",
    "object_replicas",
    "upload_sessions",
    "file_version_restore_operations",
    "metadata_purge_operations",
    "object_gc_candidates",
    "object_gc_holds",
    "object_gc_operations",
    "object_gc_replica_actions",
    "change_journal",
    "device_sync_checkpoints",
    "sync_bootstraps",
    "sync_bootstrap_nodes",
    "device_mutation_operations",
    "sync_conflicts",
    "sync_conflict_resolutions",
    "device_credentials",
    "device_enrollment_grants",
    "backup_sets",
    "backup_snapshots",
    "backup_snapshot_nodes",
    "backup_snapshot_content_pins",
    "backup_restore_plans",
    "backup_restore_plan_entries",
    "backup_restore_executions",
    "backup_restore_execution_entries",
    "backup_prune_plans",
    "backup_prune_plan_entries",
    "backup_prune_plan_object_impacts",
    "backup_prune_executions",
    "backup_prune_execution_object_results",
    "backup_snapshot_retention_policy_revisions",
    "backup_snapshot_expiry_plans",
    "backup_snapshot_expiry_plan_entries",
    "backup_snapshot_expiry_executions",
    "backup_snapshot_expiry_execution_entries",
    "backup_maintenance_runs",
    "backup_schedules",
    "backup_schedule_revisions",
    "backup_schedule_operations",
    "backup_schedule_occurrences",
    "backup_schedule_occurrence_handoffs",
    "backup_schedule_misfire_skips",
    "backup_scheduled_maintenance_claims",
    "rebaseline_snapshots",
    "rebaseline_snapshot_entries",
    "rebaseline_snapshot_handoff_proofs",
];

#[cfg(test)]
mod tests {
    use super::{MigrationRunner, MigrationStatus};

    #[test]
    fn default_runner_targets_the_workspace_migration_directory() {
        let runner = MigrationRunner::new();
        assert!(matches!(runner.source, super::MigrationSource::Embedded));
        assert_eq!(super::EMBEDDED_MIGRATOR.iter().count(), 36);
        let migrations = super::EMBEDDED_MIGRATOR.iter().collect::<Vec<_>>();
        assert!(
            migrations
                .windows(2)
                .all(|pair| pair[0].version < pair[1].version)
        );
        assert!(migrations.iter().all(|migration| !migration.no_tx));
        assert!(!super::EMBEDDED_MIGRATOR.ignore_missing);
        assert!(super::EMBEDDED_MIGRATOR.locking);
        assert_eq!(migrations.last().unwrap().version, 20260910000000);
    }

    #[test]
    fn required_table_catalog_covers_every_frozen_table() {
        let tables = super::EMBEDDED_MIGRATOR
            .iter()
            .flat_map(|migration| {
                migration.sql.lines().filter_map(|line| {
                    line.strip_prefix("CREATE TABLE ")
                        .and_then(|rest| rest.split_whitespace().next())
                })
            })
            .collect::<std::collections::BTreeSet<_>>();
        let required = super::REQUIRED_TABLES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(tables, required);
    }

    #[test]
    fn unknown_schema_error_is_typed_and_redacted() {
        let error = crate::DatabaseError::migration(sqlx::migrate::MigrateError::VersionMissing(
            20270101000000,
        ));
        assert_eq!(
            error.kind(),
            Some(crate::DatabaseErrorKind::SchemaUnsupported)
        );
        assert_eq!(error.to_string(), "database_schema_unsupported");
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
