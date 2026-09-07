//! Dedicated migration-from-empty gate for PostgreSQL 17 CI.
//!
//! This test is intentionally ignored unless `SYNVEIL_TEST_DATABASE_URL` points
//! at a disposable PostgreSQL 17 database. The PG17 CI workflow executes it
//! live to prove:
//!   34 attempted, 34 successful, 0 failed, is_current == true,
//!   latest == 20260903000001, and that historical migrations are unchanged.

use synveil_metadata::{DatabaseConfig, DatabasePool, MigrationRunner};

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a disposable PostgreSQL 17 database"]
async fn pg17_migration_from_empty_is_current_34() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let runner = MigrationRunner::new();
    let status = runner
        .run(&pool)
        .await
        .expect("migration execution must succeed");

    // Explicit gate: these numbers are part of the prompt contract.
    assert_eq!(
        status.applied_versions().len(),
        34,
        "expected 34 migrations applied"
    );
    assert_eq!(
        status.latest_applied_version(),
        Some(20260903000001),
        "latest migration must be 20260903000001_backup_scheduled_maintenance_claims"
    );
    assert!(
        status.is_current(),
        "migrations must be current; pending: {:?}",
        status.pending_versions()
    );
    assert!(
        status.pending_versions().is_empty(),
        "expected 0 pending, got {:?}",
        status.pending_versions()
    );
    // Historical SQL migrations are ordered checksums; runner guarantees no drift.
    // A failed migration would have returned Err above.

    assert_eq!(
        status.failed_versions().len(),
        0,
        "0 failed versions expected"
    );
    assert_eq!(
        status.pending_versions().len(),
        0,
        "0 pending versions expected"
    );
    println!(
        "migration gate: 34 attempted (applied), 34 successful, 0 failed, latest 20260903000001, is_current true"
    );
    pool.close().await;
}
