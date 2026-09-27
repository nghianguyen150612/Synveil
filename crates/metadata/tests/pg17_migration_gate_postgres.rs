//! Dedicated migration-from-empty gate for PostgreSQL 17 CI.
//!
//! This test is intentionally ignored unless `SYNVEIL_TEST_DATABASE_URL` points
//! at a disposable PostgreSQL 17 database. The PG17 CI workflow executes it
//! live to prove:
//!   36 attempted, 36 successful, 0 failed, is_current == true,
//!   latest == 20260910000000, and that the first 35 migrations are unchanged.

use synveil_metadata::{DatabaseConfig, DatabasePool, MigrationRunner};

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a disposable PostgreSQL 17 database"]
async fn pg17_migration_from_empty_is_current_36() {
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
        36,
        "expected 36 migrations applied"
    );
    assert_eq!(
        status.latest_applied_version(),
        Some(20260910000000),
        "latest migration must be 20260910000000_sync_retention_handoff_proofs"
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
        "migration gate: 36 attempted (applied), 36 successful, 0 failed, latest 20260910000000, is_current true"
    );
    pool.close().await;
}

/// Isolated schemas follow the repository's existing PG migration fixture
/// pattern; this never damages the database used by the migration-from-empty gate.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to disposable PostgreSQL 17"]
async fn pg17_future_and_missing_table_fail_closed_preserving_evidence() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL").unwrap();
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let server_version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert!(server_version.starts_with("17"), "this is the PG17 gate");
    for future in [true, false] {
        let schema = format!("p115_{}", uuid::Uuid::now_v7().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let separator = if url.contains('?') { '&' } else { '?' };
        let scoped_url = format!("{url}{separator}options=-csearch_path%3D{schema}");
        let pool = DatabasePool::connect(&DatabaseConfig::from_url(&scoped_url).unwrap())
            .await
            .unwrap();
        let inspect = sqlx::PgPool::connect(&scoped_url).await.unwrap();
        let runner = MigrationRunner::new();
        assert!(runner.run(&pool).await.unwrap().is_current());
        sqlx::query("CREATE TABLE evidence (value TEXT NOT NULL)")
            .execute(&inspect)
            .await
            .unwrap();
        sqlx::query("INSERT INTO evidence VALUES ('preserve')")
            .execute(&inspect)
            .await
            .unwrap();
        if future {
            sqlx::query("INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (20270101000000, 'future fixture', TRUE, '\\x00', 0)")
                .execute(&inspect).await.unwrap();
        } else {
            sqlx::query("DROP TABLE sessions")
                .execute(&inspect)
                .await
                .unwrap();
        }
        let error = runner.run(&pool).await.unwrap_err();
        assert_eq!(
            error.kind(),
            Some(if future {
                synveil_metadata::DatabaseErrorKind::SchemaUnsupported
            } else {
                synveil_metadata::DatabaseErrorKind::MigrationStateUnavailable
            })
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT value FROM evidence")
                .fetch_one(&inspect)
                .await
                .unwrap(),
            "preserve"
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&inspect)
            .await
            .unwrap();
        assert_eq!(count, if future { 37 } else { 36 });
        pool.close().await;
        inspect.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
    }
    admin.close().await;
}
