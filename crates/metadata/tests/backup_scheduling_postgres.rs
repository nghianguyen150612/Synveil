//! PostgreSQL verification for Prompt 61's durable scheduling domain.
//!
//! Run against a fresh disposable PostgreSQL 17 database with:
//! `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-metadata --test
//! backup_scheduling_postgres -- --ignored`.

use sqlx::PgPool;
use synveil_core::{
    BACKUP_SCHEDULE_FINGERPRINT_VERSION, BackupScheduleConfig, BackupScheduleMisfireMode,
    BackupScheduleRecurrenceKind, BackupScheduleTimezone, BackupScheduleWeekday, BackupSetId,
    DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS, DedupDomainId, Library, LibraryId, LogicalName,
    Node, NodeId, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupScheduleError, BackupScheduleService, BackupService, DatabaseConfig, DatabasePool,
    DomainRepository, MigrationRunner,
};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp must be valid")
}

fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("test logical name must be valid")
}

fn timezone(value: &str) -> BackupScheduleTimezone {
    value.parse().expect("test timezone must be valid")
}

fn local_time(value: &str) -> synveil_core::BackupScheduleLocalTime {
    value.parse().expect("test local time must be valid")
}

fn daily(zone: &str, time: &str) -> BackupScheduleConfig {
    BackupScheduleConfig::daily(timezone(zone), local_time(time)).expect("daily config is valid")
}

fn weekly(zone: &str, time: &str, days: Vec<BackupScheduleWeekday>) -> BackupScheduleConfig {
    BackupScheduleConfig::new(
        BackupScheduleRecurrenceKind::Weekly,
        timezone(zone),
        local_time(time),
        days,
    )
    .expect("weekly config is valid")
}

struct Fixture {
    pool: DatabasePool,
    inspection: PgPool,
    owner_user_id: UserId,
    other_user_id: UserId,
    backup_set_id: BackupSetId,
}

async fn fixture(label: &str) -> Fixture {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("all forward migrations must apply");
    assert!(status.is_current(), "all migrations must be current");

    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    let other_user_id = UserId::new();
    let repository = DomainRepository::new(&pool);
    for (user_id, login) in [
        (owner_user_id, "schedule-owner"),
        (other_user_id, "other-owner"),
    ] {
        let user = User::new(
            user_id,
            synveil_core::LoginIdentifier::new(
                format!("{login}-{label}"),
                format!("{login}-{label}-{}", user_id),
            )
            .expect("fixture login must be valid"),
            UserStatus::Active,
            observed_at,
        );
        repository
            .insert_user(&user)
            .await
            .expect("fixture user must persist");
    }

    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        owner_user_id,
        name(format!("library-{label}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library must be valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");

    let backup_set_id = BackupSetId::new();
    BackupService::new(pool.clone())
        .create_backup_set(
            owner_user_id,
            backup_set_id,
            name(format!("backup-{label}")),
            library_id,
            None,
            observed_at,
        )
        .await
        .expect("fixture backup set must persist");
    sqlx::query(
        "UPDATE backup_sets
         SET state = 'ACTIVE', revision = revision + 1, updated_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .execute(&inspection)
    .await
    .expect("fixture backup set must become active");

    Fixture {
        pool,
        inspection,
        owner_user_id,
        other_user_id,
        backup_set_id,
    }
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .expect("table count must succeed")
}

async fn revision_count(pool: &PgPool, schedule_id: synveil_core::BackupScheduleId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_schedule_revisions
         WHERE schedule_id = $1",
    )
    .bind(schedule_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("schedule revision count must succeed")
}

async fn operation_count(pool: &PgPool, schedule_id: synveil_core::BackupScheduleId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_schedule_operations
         WHERE schedule_id = $1",
    )
    .bind(schedule_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("schedule operation count must succeed")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_backup_scheduling_schema_is_current_and_has_thirty_three_successful_migrations() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("all migrations must apply from the current database");
    assert!(status.is_current());
    let successful: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success = TRUE")
            .fetch_one(&inspection)
            .await
            .expect("migration history count must succeed");
    assert_eq!(successful, 34);
    assert_eq!(status.applied_versions().len(), 34);
    assert_eq!(status.latest_applied_version(), Some(20260903000001));
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&inspection)
        .await
        .expect("server version query must succeed");
    assert!(
        version.starts_with("17."),
        "Prompt 61 PostgreSQL evidence requires PostgreSQL 17, got {version}"
    );
    inspection.close().await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_creation_revision_history_and_noop_are_durable() {
    let fixture = fixture("creation").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-create-1".to_owned(),
            fixture.backup_set_id,
            daily("Asia/Ho_Chi_Minh", "02:30"),
        )
        .await
        .expect("first schedule revision must persist");
    assert_eq!(first.revision_number().get(), 1);

    let replay = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-create-1".to_owned(),
            fixture.backup_set_id,
            daily("Asia/Ho_Chi_Minh", "02:30"),
        )
        .await
        .expect("same operation must replay");
    assert_eq!(replay, first);

    let noop = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-noop-1".to_owned(),
            fixture.backup_set_id,
            daily("Asia/Ho_Chi_Minh", "02:30"),
        )
        .await
        .expect("semantically identical configuration must be a no-op");
    assert_eq!(noop, first);

    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("current schedule must be readable");
    assert_eq!(current.current_revision_id(), first.id());
    assert!(current.enabled());
    assert_eq!(revision_count(&fixture.inspection, current.id()).await, 1);
    assert_eq!(operation_count(&fixture.inspection, current.id()).await, 2);

    let changed = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-edit-1".to_owned(),
            fixture.backup_set_id,
            daily("Europe/Berlin", "04:00"),
        )
        .await
        .expect("changed configuration must append a revision");
    assert_eq!(changed.revision_number().get(), 2);
    assert_eq!(revision_count(&fixture.inspection, current.id()).await, 2);

    let history = service
        .get_backup_schedule_revision(fixture.owner_user_id, first.id())
        .await
        .expect("historical revision must remain readable");
    assert_eq!(history.config(), first.config());
    assert_eq!(history.local_time().to_string(), "02:30");
    assert_eq!(
        history
            .next_occurrence_after(timestamp("2026-09-02T18:00:00Z"))
            .expect("historical revision must remain plannable")
            .scheduled_for_utc(),
        timestamp("2026-09-02T19:30:00Z")
    );

    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("edited schedule must be readable");
    assert_eq!(current.current_revision_id(), changed.id());
    assert_eq!(current.current_revision().local_time().to_string(), "04:00");
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_enable_disable_and_effective_backup_set_state_are_fail_closed() {
    let fixture = fixture("effective").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-effective-1".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "02:00"),
        )
        .await
        .expect("schedule must persist");

    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("configured schedule must be readable");
    let reference = current.effective_from();
    let planned = service
        .get_effective_next_backup_occurrence(
            fixture.owner_user_id,
            fixture.backup_set_id,
            reference,
        )
        .await
        .expect("active enabled schedule must plan");
    assert_eq!(
        planned,
        current
            .current_revision()
            .next_occurrence_after(current.effective_from())
    );

    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule disable must persist");
    assert!(
        service
            .get_effective_next_backup_occurrence(
                fixture.owner_user_id,
                fixture.backup_set_id,
                reference,
            )
            .await
            .expect("disabled schedule read must succeed")
            .is_none()
    );
    let disabled = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("disabled schedule history must remain readable");
    assert_eq!(disabled.current_revision_id(), revision.id());

    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, true)
        .await
        .expect("schedule re-enable must persist");
    sqlx::query(
        "UPDATE backup_sets
         SET state = 'DISABLED', revision = revision + 1, updated_at = clock_timestamp()
         WHERE id = $1",
    )
    .bind(fixture.backup_set_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("fixture backup set disable must persist");
    assert!(
        service
            .get_effective_next_backup_occurrence(
                fixture.owner_user_id,
                fixture.backup_set_id,
                reference,
            )
            .await
            .expect("disabled BackupSet read must succeed")
            .is_none()
    );
    assert_eq!(revision_count(&fixture.inspection, disabled.id()).await, 1);
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_ownership_and_idempotency_conflicts_are_concealed() {
    let fixture = fixture("ownership").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-owner-1".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "03:00"),
        )
        .await
        .expect("owner schedule must persist");

    assert_eq!(
        service
            .get_backup_schedule(fixture.other_user_id, fixture.backup_set_id)
            .await,
        Err(BackupScheduleError::NotFound)
    );
    assert_eq!(
        service
            .get_backup_schedule_revision(fixture.other_user_id, revision.id())
            .await,
        Err(BackupScheduleError::NotFound)
    );
    assert_eq!(
        service
            .configure_backup_schedule(
                fixture.other_user_id,
                "schedule-foreign-1".to_owned(),
                fixture.backup_set_id,
                daily("UTC", "03:00"),
            )
            .await,
        Err(BackupScheduleError::NotFound)
    );

    assert_eq!(
        service
            .configure_backup_schedule(
                fixture.owner_user_id,
                "schedule-owner-1".to_owned(),
                fixture.backup_set_id,
                daily("UTC", "04:00"),
            )
            .await,
        Err(BackupScheduleError::SemanticIdempotencyConflict)
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_concurrent_first_configuration_converges_to_one_schedule() {
    let fixture = fixture("first-race").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let service_a = service.clone();
    let service_b = service.clone();
    let owner = fixture.owner_user_id;
    let backup_set_id = fixture.backup_set_id;
    let config_a = daily("UTC", "05:00");
    let config_b = daily("UTC", "05:00");

    let (a, b) = tokio::join!(
        service_a.configure_backup_schedule(
            owner,
            "schedule-race-a".to_owned(),
            backup_set_id,
            config_a,
        ),
        service_b.configure_backup_schedule(
            owner,
            "schedule-race-b".to_owned(),
            backup_set_id,
            config_b,
        )
    );
    let a = a.expect("first concurrent configuration A must succeed");
    let b = b.expect("first concurrent configuration B must succeed");
    assert_eq!(a, b, "same semantics must return one canonical revision");

    let schedules: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_schedules WHERE backup_set_id = $1")
            .bind(backup_set_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("schedule count must succeed");
    assert_eq!(schedules, 1);
    assert_eq!(
        revision_count(&fixture.inspection, a.schedule_id()).await,
        1
    );
    assert_eq!(
        operation_count(&fixture.inspection, a.schedule_id()).await,
        2
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_concurrent_distinct_updates_allocate_monotonic_revisions() {
    let fixture = fixture("update-race").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-update-base".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "05:00"),
        )
        .await
        .expect("base schedule must persist");
    let service_a = service.clone();
    let service_b = service.clone();
    let (a, b) = tokio::join!(
        service_a.configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-update-a".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "06:00"),
        ),
        service_b.configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-update-b".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "07:00"),
        )
    );
    let a = a.expect("distinct concurrent update A must succeed");
    let b = b.expect("distinct concurrent update B must succeed");
    assert_eq!(a.revision_number().get() + b.revision_number().get(), 5);
    assert_ne!(a.revision_number(), b.revision_number());
    assert_eq!(
        revision_count(&fixture.inspection, first.schedule_id()).await,
        3
    );

    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("current schedule must remain valid");
    assert_eq!(current.current_revision().revision_number().get(), 3);
    assert!(
        current.current_revision_id() == a.id() || current.current_revision_id() == b.id(),
        "current pointer must reference one of the two committed updates"
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_failed_revision_update_rolls_back_without_orphans() {
    let fixture = fixture("failed-update").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-failure-base".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "05:00"),
        )
        .await
        .expect("base schedule must persist");

    sqlx::query(
        "CREATE FUNCTION synveil_test_fail_schedule_revision()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.operation_id = 'schedule-fail-1' THEN
                 RAISE EXCEPTION 'injected schedule revision failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .expect("failure function must install");
    sqlx::query(
        "CREATE TRIGGER synveil_test_fail_schedule_revision_trigger
         BEFORE INSERT ON backup_schedule_revisions
         FOR EACH ROW EXECUTE FUNCTION synveil_test_fail_schedule_revision()",
    )
    .execute(&fixture.inspection)
    .await
    .expect("failure trigger must install");

    let result = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-fail-1".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "06:00"),
        )
        .await;
    assert!(result.is_err(), "injected revision failure must surface");
    sqlx::query(
        "DROP TRIGGER synveil_test_fail_schedule_revision_trigger
         ON backup_schedule_revisions",
    )
    .execute(&fixture.inspection)
    .await
    .expect("failure trigger must remove");
    sqlx::query("DROP FUNCTION synveil_test_fail_schedule_revision()")
        .execute(&fixture.inspection)
        .await
        .expect("failure function must remove");

    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("current schedule must remain readable");
    assert_eq!(current.current_revision_id(), first.id());
    assert_eq!(
        revision_count(&fixture.inspection, first.schedule_id()).await,
        1
    );
    assert_eq!(
        operation_count(&fixture.inspection, first.schedule_id()).await,
        1
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_revision_database_fence_rejects_mutation_and_cross_pointer() {
    let fixture = fixture("db-fence").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-fence-1".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "05:00"),
        )
        .await
        .expect("first schedule must persist");
    let second_set = BackupSetId::new();
    let library_id: synveil_core::LibraryId = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT source_library_id FROM backup_sets WHERE id = $1",
    )
    .bind(fixture.backup_set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("fixture library identity must load")
    .try_into()
    .expect("library identity must be UUIDv7");
    BackupService::new(fixture.pool.clone())
        .create_backup_set(
            fixture.owner_user_id,
            second_set,
            name("backup-db-fence-second"),
            library_id,
            None,
            timestamp("2026-09-02T00:00:01Z"),
        )
        .await
        .expect("second backup set must persist");
    sqlx::query(
        "UPDATE backup_sets
         SET state = 'ACTIVE', revision = revision + 1, updated_at = clock_timestamp()
         WHERE id = $1",
    )
    .bind(second_set.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("second set must become active");
    let second = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-fence-2".to_owned(),
            second_set,
            daily("UTC", "06:00"),
        )
        .await
        .expect("second schedule must persist");

    assert!(
        sqlx::query(
            "UPDATE backup_schedule_revisions
             SET local_time_minute = 360
             WHERE id = $1",
        )
        .bind(first.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM backup_schedule_revisions WHERE id = $1")
            .bind(first.id().into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE backup_schedules
             SET current_revision_id = $1
             WHERE id = $2",
        )
        .bind(second.id().into_uuid())
        .bind(first.schedule_id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_configuration_has_no_backup_or_sync_side_effects() {
    let fixture = fixture("side-effects").await;
    let before = [
        count(&fixture.inspection, "backup_snapshots").await,
        count(&fixture.inspection, "backup_maintenance_runs").await,
        count(&fixture.inspection, "change_journal").await,
        count(&fixture.inspection, "device_sync_checkpoints").await,
        count(&fixture.inspection, "backup_snapshot_content_pins").await,
        count(&fixture.inspection, "object_gc_candidates").await,
    ];

    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-side-effect-1".to_owned(),
            fixture.backup_set_id,
            weekly(
                "UTC",
                "21:30",
                vec![BackupScheduleWeekday::Friday, BackupScheduleWeekday::Monday],
            ),
        )
        .await
        .expect("schedule configuration must persist");
    service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-side-effect-2".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "22:00"),
        )
        .await
        .expect("schedule edit must persist");
    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule disable must persist");
    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, true)
        .await
        .expect("schedule enable must persist");

    let after = [
        count(&fixture.inspection, "backup_snapshots").await,
        count(&fixture.inspection, "backup_maintenance_runs").await,
        count(&fixture.inspection, "change_journal").await,
        count(&fixture.inspection, "device_sync_checkpoints").await,
        count(&fixture.inspection, "backup_snapshot_content_pins").await,
        count(&fixture.inspection, "object_gc_candidates").await,
    ];
    assert_eq!(after, before);
    assert_eq!(
        revision_count(&fixture.inspection, revision.schedule_id()).await,
        2
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_validation_rejects_invalid_wire_values() {
    let fixture = fixture("validation").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    assert_eq!(
        service
            .configure_backup_schedule_from_values(
                fixture.owner_user_id,
                "schedule-invalid-zone".to_owned(),
                fixture.backup_set_id,
                BackupScheduleRecurrenceKind::Daily,
                "Not/A_Timezone",
                "02:00",
                Vec::new(),
            )
            .await,
        Err(BackupScheduleError::InvalidTimezone)
    );
    assert_eq!(
        service
            .configure_backup_schedule_from_values(
                fixture.owner_user_id,
                "schedule-invalid-time".to_owned(),
                fixture.backup_set_id,
                BackupScheduleRecurrenceKind::Daily,
                "UTC",
                "25:00",
                Vec::new(),
            )
            .await,
        Err(BackupScheduleError::InvalidLocalTime)
    );
    assert_eq!(
        service
            .configure_backup_schedule_from_values(
                fixture.owner_user_id,
                "schedule-invalid-days".to_owned(),
                fixture.backup_set_id,
                BackupScheduleRecurrenceKind::Weekly,
                "UTC",
                "02:00",
                Vec::new(),
            )
            .await,
        Err(BackupScheduleError::InvalidWeeklyDays)
    );
    assert_eq!(
        service
            .configure_backup_schedule(
                fixture.owner_user_id,
                "short".to_owned(),
                fixture.backup_set_id,
                daily("UTC", "02:00"),
            )
            .await,
        Err(BackupScheduleError::InvalidOperationId)
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_policy_fields_are_revisioned_idempotent_and_v2_fingerprinted() {
    let fixture = fixture("misfire-policy-revisions").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let default_config = daily("UTC", "02:00");
    let first = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "misfire-policy-first".to_owned(),
            fixture.backup_set_id,
            default_config.clone(),
        )
        .await
        .unwrap();
    assert_eq!(first.misfire_mode(), BackupScheduleMisfireMode::LatestOnly);
    assert_eq!(
        first.max_lateness_seconds(),
        DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS
    );
    assert_eq!(
        first.request_fingerprint().version(),
        BACKUP_SCHEDULE_FINGERPRINT_VERSION
    );
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let effective_from = schedule.effective_from();

    let noop = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "misfire-policy-noop".to_owned(),
            fixture.backup_set_id,
            default_config,
        )
        .await
        .unwrap();
    assert_eq!(noop.id(), first.id());
    assert_eq!(
        service
            .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
            .await
            .unwrap()
            .effective_from(),
        effective_from
    );

    let replay_config = BackupScheduleConfig::new_with_misfire_policy(
        BackupScheduleRecurrenceKind::Daily,
        timezone("UTC"),
        local_time("02:00"),
        Vec::new(),
        BackupScheduleMisfireMode::ReplayOneByOne,
        172_800,
    )
    .unwrap();
    let replay = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "misfire-policy-replay".to_owned(),
            fixture.backup_set_id,
            replay_config.clone(),
        )
        .await
        .unwrap();
    assert_eq!(replay.revision_number().get(), 2);
    assert_eq!(
        replay.misfire_mode(),
        BackupScheduleMisfireMode::ReplayOneByOne
    );
    assert_eq!(replay.max_lateness_seconds(), 172_800);
    let replay_effective = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap()
        .effective_from();
    assert!(replay_effective > effective_from);

    let changed_lateness = BackupScheduleConfig::new_with_misfire_policy(
        BackupScheduleRecurrenceKind::Daily,
        timezone("UTC"),
        local_time("02:00"),
        Vec::new(),
        BackupScheduleMisfireMode::ReplayOneByOne,
        86_400,
    )
    .unwrap();
    let third = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "misfire-policy-lateness".to_owned(),
            fixture.backup_set_id,
            changed_lateness,
        )
        .await
        .unwrap();
    assert_eq!(third.revision_number().get(), 3);
    assert_eq!(third.max_lateness_seconds(), 86_400);

    assert_eq!(
        service
            .configure_backup_schedule(
                fixture.owner_user_id,
                "misfire-policy-replay".to_owned(),
                fixture.backup_set_id,
                BackupScheduleConfig::daily(timezone("UTC"), local_time("02:00")).unwrap(),
            )
            .await,
        Err(BackupScheduleError::SemanticIdempotencyConflict)
    );
    assert_eq!(
        service
            .configure_backup_schedule_with_misfire_policy_from_values(
                fixture.owner_user_id,
                "misfire-invalid-mode".to_owned(),
                fixture.backup_set_id,
                BackupScheduleRecurrenceKind::Daily,
                "UTC",
                "02:00",
                Vec::new(),
                "UNKNOWN",
                60,
            )
            .await,
        Err(BackupScheduleError::InvalidMisfireMode)
    );
    assert_eq!(
        service
            .configure_backup_schedule_with_misfire_policy_from_values(
                fixture.owner_user_id,
                "misfire-invalid-lateness".to_owned(),
                fixture.backup_set_id,
                BackupScheduleRecurrenceKind::Daily,
                "UTC",
                "02:00",
                Vec::new(),
                "LATEST_ONLY",
                59,
            )
            .await,
        Err(BackupScheduleError::InvalidMaxLateness)
    );
    assert_eq!(revision_count(&fixture.inspection, schedule.id()).await, 3);
    fixture.pool.close().await;
}
