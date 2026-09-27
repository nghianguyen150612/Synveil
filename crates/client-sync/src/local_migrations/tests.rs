use std::{borrow::Cow, fs};

use sqlx::{
    SqlitePool,
    migrate::{Migration, MigrationType, Migrator},
    sqlite::SqliteConnectOptions,
};

use crate::{
    ClientSyncError, LocalStateConfig, LocalStateStore, test_support::remove_dir_all_bounded,
};

async fn fixture(version: i64) -> (std::path::PathBuf, LocalStateConfig, SqlitePool) {
    let directory = std::env::temp_dir().join(format!("synveil-upgrade-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&directory).unwrap();
    let config = LocalStateConfig::new(directory.join("state.sqlite3"));
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(config.database_path())
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    prefix(version).run(&pool).await.unwrap();
    (directory, config, pool)
}

fn prefix(version: i64) -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.migrations = Cow::Owned(
        migrator
            .iter()
            .filter(|m| m.version <= version)
            .cloned()
            .collect(),
    );
    migrator
}

async fn schema(pool: &SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as("SELECT type, name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")
        .fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn every_frozen_prefix_upgrades_reopens_and_matches_fresh_schema() {
    let (fresh_dir, fresh_config, fresh_pool) = fixture(7).await;
    let expected = schema(&fresh_pool).await;
    let migrator = sqlx::migrate!("./migrations");
    assert_eq!(migrator.iter().count(), 7);
    assert!(migrator.iter().all(|migration| !migration.no_tx));
    assert!(!migrator.ignore_missing);
    let mut required = super::REQUIRED_OBJECTS
        .iter()
        .map(|&(_, kind, name)| (kind.to_owned(), name.to_owned()))
        .collect::<std::collections::BTreeSet<_>>();
    required.insert((
        "trigger".to_owned(),
        "server_profile_id_is_immutable".to_owned(),
    ));
    required.insert(("table".to_owned(), "_sqlx_migrations".to_owned()));
    assert_eq!(
        required,
        expected
            .iter()
            .map(|(kind, name, _)| (kind.clone(), name.clone()))
            .collect()
    );
    fresh_pool.close().await;
    for version in 1..=7 {
        let (directory, config, pool) = fixture(version).await;
        pool.close().await;
        let store = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 7);
        assert_eq!(schema(&store.pool).await, expected);
        store.close_pool().await;
        drop(store);
        let store = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 7);
        store.close_pool().await;
        drop(store);
        remove_dir_all_bounded(&directory).unwrap();
    }
    assert!(fresh_config.database_path().exists());
    remove_dir_all_bounded(&fresh_dir).unwrap();
}

#[tokio::test]
async fn failed_transaction_rolls_back_ddl_data_and_version_then_restart_resumes() {
    let (directory, config, pool) = fixture(6).await;
    seed_evidence_profile(&pool).await;
    let mut broken = prefix(6);
    let mut migrations = broken.migrations.into_owned();
    migrations.push(Migration::new(7, Cow::Borrowed("injected failure"), MigrationType::Simple,
        Cow::Borrowed("DELETE FROM server_profiles; CREATE TABLE half_migrated (value TEXT); SELECT * FROM absent_failure_target;"), false));
    broken.migrations = Cow::Owned(migrations);
    assert!(broken.run(&pool).await.is_err());
    pool.close().await;
    // Reconnect to the file, so assertions cannot rely on a rolled-back
    // transaction's process-local view.
    let pool =
        SqlitePool::connect_with(SqliteConnectOptions::new().filename(config.database_path()))
            .await
            .unwrap();
    let value: String = sqlx::query_scalar("SELECT display_label FROM server_profiles")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(value, "preserve");
    let half: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name='half_migrated'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(half, 0);
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 6);
    pool.close().await;
    let store = LocalStateStore::open(&config).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), 7);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT display_label FROM server_profiles")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        "preserve"
    );
    store.close_pool().await;
    drop(store);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn damaged_or_unknown_schema_fails_closed_without_repairing_evidence() {
    for (mutation, unsupported) in [
        (
            "UPDATE _sqlx_migrations SET version=8 WHERE version=7",
            true,
        ),
        (
            "UPDATE _sqlx_migrations SET version=-1 WHERE version=7",
            false,
        ),
        ("DELETE FROM _sqlx_migrations WHERE version=3", false),
        (
            "UPDATE _sqlx_migrations SET success=0 WHERE version=7",
            false,
        ),
        (
            "UPDATE _sqlx_migrations SET success=2 WHERE version=7",
            false,
        ),
        (
            "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=7",
            false,
        ),
        ("CREATE TABLE future_state (value TEXT)", false),
        (
            "CREATE TRIGGER future_trigger AFTER INSERT ON observation_state BEGIN SELECT 1; END",
            false,
        ),
        ("DROP TABLE sync_conflicts", false),
        (
            "ALTER TABLE server_profiles RENAME COLUMN canonical_base_url TO broken",
            false,
        ),
        ("DROP TRIGGER server_profile_id_is_immutable", false),
        ("DROP INDEX active_outbound_intent_dedupe", false),
        ("DROP TABLE _sqlx_migrations", false),
        (
            "ALTER TABLE _sqlx_migrations RENAME COLUMN version TO broken",
            false,
        ),
    ] {
        let (directory, config, pool) = fixture(7).await;
        seed_evidence_profile(&pool).await;
        sqlx::query(mutation).execute(&pool).await.unwrap();
        let before = schema(&pool).await;
        pool.close().await;
        for _ in 0..2 {
            let error = LocalStateStore::open(&config).await.unwrap_err();
            if unsupported {
                assert!(matches!(error, ClientSyncError::LocalSchemaUnsupported));
                assert_eq!(error.code(), "LOCAL_SCHEMA_UNSUPPORTED");
            } else {
                assert!(matches!(error, ClientSyncError::LocalSchemaInvalid));
            }
        }
        let pool =
            SqlitePool::connect_with(SqliteConnectOptions::new().filename(config.database_path()))
                .await
                .unwrap();
        assert_eq!(schema(&pool).await, before, "{mutation}");
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT display_label FROM server_profiles")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "preserve"
        );
        pool.close().await;
        remove_dir_all_bounded(&directory).unwrap();
    }
}

#[tokio::test]
async fn non_database_file_is_preserved_on_open_failure() {
    let directory =
        std::env::temp_dir().join(format!("synveil-invalid-db-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("state.sqlite3");
    fs::write(&path, b"invalid SQLite evidence").unwrap();
    assert!(
        LocalStateStore::open(&LocalStateConfig::new(&path))
            .await
            .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"invalid SQLite evidence");
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn v6_upgrade_preserves_profile_scope_pending_ambiguous_completed_and_conflict_rows() {
    let (directory, config, pool) = fixture(6).await;
    let profile = uuid::Uuid::now_v7().to_string();
    let library = uuid::Uuid::now_v7().to_string();
    let owner = uuid::Uuid::now_v7().to_string();
    let device = uuid::Uuid::now_v7().to_string();
    let binding = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO server_profiles VALUES (?, 'https://upgrade.example', 'HTTPS', 'Upgrade fixture', 0, NULL)")
        .bind(&profile).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO profile_device_enrollments VALUES (?, ?, ?, ?, 0, NULL)")
        .bind(&profile)
        .bind(&owner)
        .bind(&device)
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO replicas (library_id, owner_user_id, device_id, root_binding_id, root_node_id, journal_epoch, applied_sequence, acknowledged_sequence, created_at_ms, updated_at_ms, server_profile_id) VALUES (?, ?, ?, ?, ?, 1, 4, 3, 0, 0, ?)")
        .bind(&library).bind(&owner).bind(&device).bind(&binding).bind(uuid::Uuid::now_v7().to_string()).bind(&profile).execute(&pool).await.unwrap();
    let mut conflict_intent = String::new();
    for (index, state) in ["PENDING", "SUBMITTING", "SERVER_APPLIED", "CONFLICT"]
        .into_iter()
        .enumerate()
    {
        let intent = uuid::Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO outbound_intents (intent_id, library_id, intent_kind, state, observed_relative_path, observed_kind, base_epoch, base_applied_sequence, dedupe_version, dedupe_sha256, created_at_ms, updated_at_ms) VALUES (?, ?, 'CREATE_DIRECTORY', ?, 'pending', 'DIRECTORY', 1, 4, 1, ?, 0, 0)")
            .bind(&intent).bind(&library).bind(state).bind(vec![u8::try_from(index).unwrap();32]).execute(&pool).await.unwrap();
        if state == "CONFLICT" {
            conflict_intent = intent;
        }
    }
    sqlx::query("INSERT INTO sync_conflicts (conflict_id, library_id, intent_id, kind, detected_at_ms, status) VALUES (?, ?, ?, 'NAME_COLLISION', 0, 'UNRESOLVED')")
        .bind(uuid::Uuid::now_v7().to_string()).bind(&library).bind(&conflict_intent).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pending_acknowledgements VALUES (?, 1, 3, 4, 4, x'01', 0)")
        .bind(&library)
        .execute(&pool)
        .await
        .unwrap();
    let tables = [
        "server_profiles",
        "profile_device_enrollments",
        "replicas",
        "observation_state",
        "outbound_intents",
        "sync_conflicts",
        "pending_acknowledgements",
    ];
    let before = snapshot(&pool, &tables).await;
    pool.close().await;
    for _ in 0..2 {
        let store = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(snapshot(&store.pool, &tables).await, before);
        assert!(
            sqlx::query("UPDATE server_profiles SET profile_id=? WHERE profile_id=?")
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(&profile)
                .execute(&store.pool)
                .await
                .is_err()
        );
        store.close_pool().await;
        drop(store);
    }
    remove_dir_all_bounded(&directory).unwrap();
}

async fn snapshot(pool: &SqlitePool, tables: &[&str]) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    for table in tables {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(table)
                .fetch_all(pool)
                .await
                .unwrap();
        let expression = columns
            .iter()
            .map(|name| format!("quote({name})"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let rows = sqlx::query_scalar(&format!(
            "SELECT {expression} AS value FROM {table} ORDER BY value"
        ))
        .fetch_all(pool)
        .await
        .unwrap();
        result.push(rows);
    }
    result
}

#[tokio::test]
async fn unrecognized_historical_columns_cannot_be_erased_by_table_rebuild() {
    let (directory, config, pool) = fixture(3).await;
    let library = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO replicas (library_id, owner_user_id, device_id, root_binding_id, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, 0, 0)")
        .bind(&library).bind(uuid::Uuid::now_v7().to_string()).bind(uuid::Uuid::now_v7().to_string())
        .bind(uuid::Uuid::now_v7().to_string()).execute(&pool).await.unwrap();
    sqlx::query("ALTER TABLE outbound_intents ADD COLUMN future_evidence TEXT NOT NULL DEFAULT 'preserve unknown data'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO outbound_intents (intent_id, library_id, intent_kind, state, observed_relative_path, observed_kind, base_epoch, base_applied_sequence, dedupe_version, dedupe_sha256, created_at_ms, updated_at_ms) VALUES (?, ?, 'CREATE_DIRECTORY', 'PENDING', 'pending', 'DIRECTORY', 1, 0, 1, ?, 0, 0)")
        .bind(uuid::Uuid::now_v7().to_string()).bind(&library).bind(vec![0_u8;32]).execute(&pool).await.unwrap();
    pool.close().await;
    let result = LocalStateStore::open(&config).await;
    assert!(
        matches!(result, Err(ClientSyncError::LocalSchemaInvalid)),
        "unknown columns must fail before migration 0004 rebuilds the table"
    );
    let pool =
        SqlitePool::connect_with(SqliteConnectOptions::new().filename(config.database_path()))
            .await
            .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT future_evidence FROM outbound_intents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "preserve unknown data"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        3
    );
    pool.close().await;
    remove_dir_all_bounded(&directory).unwrap();
}

async fn seed_evidence_profile(pool: &SqlitePool) {
    sqlx::query("INSERT INTO server_profiles VALUES (?, 'https://evidence.example', 'HTTPS', 'preserve', 0, NULL)")
        .bind(uuid::Uuid::now_v7().to_string()).execute(pool).await.unwrap();
}
