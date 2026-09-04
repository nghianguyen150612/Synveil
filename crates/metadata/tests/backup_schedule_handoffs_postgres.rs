//! PostgreSQL verification for Prompt 63's exactly-once scheduled-maintenance
//! handoff. The suite is intentionally ignored unless the caller supplies a
//! disposable PostgreSQL 17 URL through `SYNVEIL_TEST_DATABASE_URL`.

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunPreflightIssue, BackupMaintenanceRunState, BackupOperationKind,
    BackupSchedule, BackupScheduleConfig, BackupScheduleLocalTime,
    BackupScheduleOccurrenceHandoffResult, BackupScheduleOccurrenceMaterializationResult,
    BackupScheduleOccurrenceNotEffectiveReason, BackupScheduleRevision, BackupScheduleTimezone,
    BackupSetId, DedupDomainId, Library, LibraryId, LogicalName, Node, NodeId, Timestamp, User,
    UserId, UserStatus,
};
use synveil_metadata::{
    BackupError, BackupScheduleHandoffError, BackupScheduleService, BackupService, DatabaseConfig,
    DatabasePool, DomainRepository, MigrationRunner,
};
use time::Duration;
use uuid::Uuid;

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp must be valid")
}

fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("test logical name must be valid")
}

fn timezone(value: &str) -> BackupScheduleTimezone {
    value.parse().expect("test timezone must be valid")
}

fn local_time(value: &str) -> BackupScheduleLocalTime {
    value.parse().expect("test local time must be valid")
}

fn daily(timezone_name: &str, time: &str) -> BackupScheduleConfig {
    BackupScheduleConfig::daily(timezone(timezone_name), local_time(time))
        .expect("daily schedule configuration must be valid")
}

fn observed_after(planned: synveil_core::PlannedScheduleOccurrence) -> Timestamp {
    planned
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(1))
        .expect("test observed time must be representable")
}

struct Fixture {
    pool: DatabasePool,
    inspection: PgPool,
    owner_user_id: UserId,
    other_user_id: UserId,
    library_id: LibraryId,
    backup_set_id: BackupSetId,
}

impl Fixture {
    async fn close(self) {
        self.pool.close().await;
        self.inspection.close().await;
    }

    async fn create_active_backup_set(&self, label: &str) -> BackupSetId {
        let backup_set_id = BackupSetId::new();
        BackupService::new(self.pool.clone())
            .create_backup_set(
                self.owner_user_id,
                backup_set_id,
                name(format!("backup-{label}-{backup_set_id}")),
                self.library_id,
                None,
                timestamp("2026-09-02T00:00:01Z"),
            )
            .await
            .expect("additional backup set must persist");
        sqlx::query(
            "UPDATE backup_sets
             SET state = 'ACTIVE', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(backup_set_id.into_uuid())
        .bind(self.owner_user_id.into_uuid())
        .execute(&self.inspection)
        .await
        .expect("additional backup set must become active");
        backup_set_id
    }
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
    assert_eq!(status.applied_versions().len(), 34);
    assert_eq!(status.latest_applied_version(), Some(20260903000001));

    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    let other_user_id = UserId::new();
    let repository = DomainRepository::new(&pool);
    for (user_id, login) in [
        (owner_user_id, "handoff-owner"),
        (other_user_id, "handoff-other"),
    ] {
        let user = User::new(
            user_id,
            synveil_core::LoginIdentifier::new(
                format!("{login}-{label}-{user_id}"),
                format!("{login}-{label}-key-{user_id}"),
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
        name(format!("library-{label}-{library_id}")),
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
            name(format!("backup-{label}-{backup_set_id}")),
            library_id,
            None,
            observed_at,
        )
        .await
        .expect("fixture backup set must persist");
    sqlx::query(
        "UPDATE backup_sets
         SET state = 'ACTIVE', revision = revision + 1,
             updated_at = clock_timestamp()
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
        library_id,
        backup_set_id,
    }
}

async fn configure_policy(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    label: &str,
) -> synveil_core::BackupSnapshotRetentionPolicyRevision {
    BackupService::new(fixture.pool.clone())
        .configure_snapshot_retention_policy(
            fixture.owner_user_id,
            format!("policy-{label}-{backup_set_id}"),
            backup_set_id,
            1,
            86_400,
        )
        .await
        .expect("retention policy must persist")
}

async fn configure_schedule(
    fixture: &Fixture,
    service: &BackupScheduleService,
    backup_set_id: BackupSetId,
    label: &str,
    schedule_config: BackupScheduleConfig,
) -> BackupScheduleRevision {
    service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-{label}-{backup_set_id}"),
            backup_set_id,
            schedule_config,
        )
        .await
        .expect("schedule revision must persist")
}

struct PreparedOccurrence {
    service: BackupScheduleService,
    schedule: BackupSchedule,
    revision: BackupScheduleRevision,
    occurrence: synveil_core::BackupScheduleOccurrence,
    observed_at: Timestamp,
}

async fn prepare_occurrence(
    fixture: &Fixture,
    label: &str,
    with_policy: bool,
) -> PreparedOccurrence {
    let backup_set_id = fixture.backup_set_id;
    if with_policy {
        configure_policy(fixture, backup_set_id, label).await;
    }
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = configure_schedule(
        fixture,
        &service,
        backup_set_id,
        label,
        daily("UTC", "09:00"),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, backup_set_id)
        .await
        .expect("configured schedule must be readable");
    let target_date = (schedule.effective_from().as_offset_datetime() + Duration::days(1)).date();
    let planned = revision
        .occurrence_on_local_date(target_date)
        .expect("daily target must be planned");
    let observed_at = observed_after(planned);
    let materialized = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            observed_at,
        )
        .await
        .expect("due occurrence must materialize");
    let occurrence = match materialized {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("fresh occurrence must materialize, got {other:?}"),
    };
    PreparedOccurrence {
        service,
        schedule,
        revision,
        occurrence,
        observed_at,
    }
}

async fn count_maintenance_runs(pool: &PgPool, backup_set_id: BackupSetId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM backup_maintenance_runs WHERE backup_set_id = $1")
        .bind(backup_set_id.into_uuid())
        .fetch_one(pool)
        .await
        .expect("maintenance count must succeed")
}

async fn count_handoffs(pool: &PgPool, backup_set_id: BackupSetId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM backup_schedule_occurrence_handoffs WHERE backup_set_id = $1",
    )
    .bind(backup_set_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("handoff count must succeed")
}

async fn side_effect_counts(fixture: &Fixture) -> [i64; 9] {
    let backup_set_id = fixture.backup_set_id.into_uuid();
    let library_id = fixture.library_id.into_uuid();
    [
        sqlx::query_scalar("SELECT count(*) FROM backup_snapshots WHERE backup_set_id = $1")
            .bind(backup_set_id)
            .fetch_one(&fixture.inspection)
            .await
            .expect("snapshot count must succeed"),
        sqlx::query_scalar(
            "SELECT count(*)
               FROM backup_snapshot_content_pins AS pin
               JOIN backup_snapshots AS snapshot ON snapshot.id = pin.snapshot_id
              WHERE snapshot.backup_set_id = $1",
        )
        .bind(backup_set_id)
        .fetch_one(&fixture.inspection)
        .await
        .expect("snapshot content pin count must succeed"),
        sqlx::query_scalar(
            "SELECT count(*) FROM backup_snapshot_expiry_plans WHERE backup_set_id = $1",
        )
        .bind(backup_set_id)
        .fetch_one(&fixture.inspection)
        .await
        .expect("expiry plan count must succeed"),
        sqlx::query_scalar(
            "SELECT count(*) FROM backup_snapshot_expiry_executions WHERE backup_set_id = $1",
        )
        .bind(backup_set_id)
        .fetch_one(&fixture.inspection)
        .await
        .expect("expiry execution count must succeed"),
        sqlx::query_scalar("SELECT count(*) FROM backup_prune_plans WHERE backup_set_id = $1")
            .bind(backup_set_id)
            .fetch_one(&fixture.inspection)
            .await
            .expect("prune plan count must succeed"),
        sqlx::query_scalar("SELECT count(*) FROM backup_prune_executions WHERE backup_set_id = $1")
            .bind(backup_set_id)
            .fetch_one(&fixture.inspection)
            .await
            .expect("prune execution count must succeed"),
        sqlx::query_scalar(
            "SELECT count(*)
               FROM object_gc_candidates
              WHERE object_dedup_domain_id = (
                    SELECT dedup_domain_id FROM libraries WHERE id = $1
              )",
        )
        .bind(library_id)
        .fetch_one(&fixture.inspection)
        .await
        .expect("object GC candidate count must succeed"),
        sqlx::query_scalar("SELECT count(*) FROM change_journal WHERE library_id = $1")
            .bind(library_id)
            .fetch_one(&fixture.inspection)
            .await
            .expect("change journal count must succeed"),
        sqlx::query_scalar("SELECT count(*) FROM device_sync_checkpoints WHERE library_id = $1")
            .bind(library_id)
            .fetch_one(&fixture.inspection)
            .await
            .expect("device checkpoint count must succeed"),
    ]
}

fn created_result(
    result: &BackupScheduleOccurrenceHandoffResult,
) -> (
    &synveil_core::BackupMaintenanceRun,
    &synveil_core::BackupScheduleOccurrenceHandoff,
) {
    assert!(result.is_created());
    (result.maintenance_run(), result.handoff())
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_schema_is_current_with_thirty_three_migrations() {
    let fixture = fixture("schema").await;
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&fixture.inspection)
        .await
        .expect("server version query must succeed");
    assert!(
        version.starts_with("17."),
        "expected PostgreSQL 17, got {version}"
    );
    let successful: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success = TRUE")
            .fetch_one(&fixture.inspection)
            .await
            .expect("migration history count must succeed");
    assert_eq!(successful, 34);
    let handoff_table: Option<String> = sqlx::query_scalar(
        "SELECT to_regclass('public.backup_schedule_occurrence_handoffs')::text",
    )
    .fetch_one(&fixture.inspection)
    .await
    .expect("handoff table lookup must succeed");
    assert_eq!(
        handoff_table.as_deref(),
        Some("backup_schedule_occurrence_handoffs")
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_creates_canonical_created_run_without_side_effects() {
    let fixture = fixture("basic").await;
    let prepared = prepare_occurrence(&fixture, "basic", true).await;
    let backup = BackupService::new(fixture.pool.clone());
    let before = side_effect_counts(&fixture).await;
    let run_count_before = count_maintenance_runs(&fixture.inspection, fixture.backup_set_id).await;

    let result = backup
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("active scheduled occurrence must hand off");
    let (run, handoff) = created_result(&result);
    assert_eq!(run.state(), BackupMaintenanceRunState::Created);
    assert_eq!(run.backup_set_id(), fixture.backup_set_id);
    assert_eq!(run.policy_revision_number().get(), 1);
    assert_eq!(
        run.capture_operation_id(),
        format!("maint:{}:capture", run.id())
    );
    assert_eq!(
        run.expiry_plan_operation_id(),
        format!("maint:{}:expiry-plan", run.id())
    );
    assert_eq!(handoff.occurrence_id(), prepared.occurrence.id());
    assert_eq!(handoff.maintenance_run_id(), run.id());
    assert_eq!(handoff.owner_user_id(), fixture.owner_user_id);
    assert_eq!(handoff.backup_set_id(), fixture.backup_set_id);
    assert_eq!(handoff.schedule_id(), prepared.schedule.id());
    assert_eq!(side_effect_counts(&fixture).await, before);
    assert_eq!(
        count_maintenance_runs(&fixture.inspection, fixture.backup_set_id).await,
        run_count_before + 1
    );
    assert_eq!(
        count_handoffs(&fixture.inspection, fixture.backup_set_id).await,
        1
    );

    let read = backup
        .get_backup_maintenance_run(fixture.owner_user_id, run.id())
        .await
        .expect("scheduled run must use the maintenance read boundary");
    assert_eq!(read, *run);
    let reverse = backup
        .get_scheduled_occurrence_for_maintenance_run(fixture.owner_user_id, run.id())
        .await
        .expect("reverse scheduled lookup must succeed")
        .expect("scheduled run must have occurrence provenance");
    assert_eq!(reverse, prepared.occurrence);

    let (feed, has_more) = backup
        .list_backup_operations(fixture.owner_user_id, fixture.backup_set_id, None, None, 20)
        .await
        .expect("maintenance operation feed must read the canonical run");
    assert!(!has_more);
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].operation_kind(), BackupOperationKind::Maintenance);
    assert_eq!(feed[0].completed_steps(), 0);
    assert_eq!(feed[0].total_steps(), 3);

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_replay_survives_lost_response_revision_edit_and_disable() {
    let fixture = fixture("replay").await;
    let prepared = prepare_occurrence(&fixture, "replay", true).await;
    let backup = BackupService::new(fixture.pool.clone());
    let first = backup
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("first handoff must commit");
    let first_run_id = first.maintenance_run().id();
    let first_capture_id = first.maintenance_run().capture_operation_id().to_owned();
    let first_expiry_id = first
        .maintenance_run()
        .expiry_plan_operation_id()
        .to_owned();

    let edited = configure_schedule(
        &fixture,
        &prepared.service,
        fixture.backup_set_id,
        "replay-edit",
        daily("Europe/Berlin", "04:00"),
    )
    .await;
    assert_eq!(edited.revision_number().get(), 2);
    let replay_after_edit = backup
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("lost-response retry must replay after revision edit");
    assert!(replay_after_edit.is_existing());
    assert_eq!(replay_after_edit.maintenance_run().id(), first_run_id);
    assert_eq!(
        replay_after_edit.maintenance_run().capture_operation_id(),
        first_capture_id
    );
    assert_eq!(
        replay_after_edit
            .maintenance_run()
            .expiry_plan_operation_id(),
        first_expiry_id
    );

    prepared
        .service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule disable must commit");
    let replay_disabled = backup
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("committed handoff must remain replayable after disable");
    assert!(replay_disabled.is_existing());
    assert_eq!(replay_disabled.maintenance_run().id(), first_run_id);

    sqlx::query(
        "UPDATE backup_sets
         SET state = 'DISABLED', revision = revision + 1,
             updated_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(fixture.backup_set_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("fixture set must be disabled for replay check");
    let replay_set_disabled = backup
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("committed handoff must remain replayable after set disable");
    assert!(replay_set_disabled.is_existing());
    assert_eq!(replay_set_disabled.maintenance_run().id(), first_run_id);
    assert_eq!(
        count_maintenance_runs(&fixture.inspection, fixture.backup_set_id).await,
        1
    );
    assert_eq!(
        count_handoffs(&fixture.inspection, fixture.backup_set_id).await,
        1
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_allows_materialized_historical_revision() {
    let fixture = fixture("historical").await;
    let prepared = prepare_occurrence(&fixture, "historical", true).await;
    let old_revision_id = prepared.occurrence.schedule_revision_id();
    let edited = configure_schedule(
        &fixture,
        &prepared.service,
        fixture.backup_set_id,
        "historical-edit",
        daily("America/New_York", "01:30"),
    )
    .await;
    assert_ne!(edited.id(), old_revision_id);
    let current = prepared
        .service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("edited schedule must be readable");
    assert_eq!(current.current_revision_id(), edited.id());
    let result = prepared
        .service
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("materialized historical revision must remain handoff-authorized");
    assert!(result.is_created());
    assert_eq!(result.occurrence().schedule_revision_id(), old_revision_id);
    assert_eq!(
        result.maintenance_run().state(),
        BackupMaintenanceRunState::Created
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_new_fences_concealment_and_no_policy_match_manual_semantics() {
    let base_fixture = fixture("fences").await;
    let prepared = prepare_occurrence(&base_fixture, "fences", false).await;
    let backup = BackupService::new(base_fixture.pool.clone());

    let manual_error = backup
        .create_backup_maintenance_run(
            base_fixture.owner_user_id,
            "manual-no-policy".to_owned(),
            base_fixture.backup_set_id,
        )
        .await;
    assert_eq!(
        manual_error,
        Err(BackupError::MaintenanceRunPreflight(
            BackupMaintenanceRunPreflightIssue::RetentionPolicyNotConfigured
        ))
    );
    let handoff_error = backup
        .handoff_backup_schedule_occurrence(
            base_fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await;
    assert_eq!(
        handoff_error,
        Err(BackupScheduleHandoffError::MaintenanceRunPreflight(
            BackupMaintenanceRunPreflightIssue::RetentionPolicyNotConfigured
        ))
    );
    assert_eq!(
        count_maintenance_runs(&base_fixture.inspection, base_fixture.backup_set_id).await,
        0
    );
    assert_eq!(
        count_handoffs(&base_fixture.inspection, base_fixture.backup_set_id).await,
        0
    );

    configure_policy(
        &base_fixture,
        base_fixture.backup_set_id,
        "fences-after-policy",
    )
    .await;
    prepared
        .service
        .set_backup_schedule_enabled(
            base_fixture.owner_user_id,
            base_fixture.backup_set_id,
            false,
        )
        .await
        .expect("schedule disable must commit");
    let disabled = backup
        .handoff_backup_schedule_occurrence(
            base_fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await;
    assert_eq!(
        disabled,
        Err(BackupScheduleHandoffError::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled
        ))
    );
    assert_eq!(
        count_maintenance_runs(&base_fixture.inspection, base_fixture.backup_set_id).await,
        0
    );

    let inactive_fixture = fixture("fences-inactive").await;
    let inactive = prepare_occurrence(&inactive_fixture, "inactive", true).await;
    sqlx::query(
        "UPDATE backup_sets
         SET state = 'DISABLED', revision = revision + 1,
             updated_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(inactive_fixture.backup_set_id.into_uuid())
    .bind(inactive_fixture.owner_user_id.into_uuid())
    .execute(&inactive_fixture.inspection)
    .await
    .expect("set must be disabled before new handoff");
    let inactive_error = BackupService::new(inactive_fixture.pool.clone())
        .handoff_backup_schedule_occurrence(
            inactive_fixture.owner_user_id,
            inactive.occurrence.id(),
            inactive.observed_at,
        )
        .await;
    assert_eq!(
        inactive_error,
        Err(BackupScheduleHandoffError::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::BackupSetInactive
        ))
    );
    assert_eq!(
        count_maintenance_runs(&inactive_fixture.inspection, inactive_fixture.backup_set_id).await,
        0
    );
    assert_eq!(
        BackupService::new(inactive_fixture.pool.clone())
            .get_backup_schedule_occurrence_handoff(
                inactive_fixture.other_user_id,
                inactive.occurrence.id(),
            )
            .await,
        Ok(None)
    );
    inactive_fixture.close().await;
    base_fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_twelve_concurrent_calls_converge_to_one_run() {
    let fixture = fixture("concurrency").await;
    let prepared = prepare_occurrence(&fixture, "concurrency", true).await;
    let backup = BackupService::new(fixture.pool.clone());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let backup = backup.clone();
        let owner_user_id = fixture.owner_user_id;
        let occurrence_id = prepared.occurrence.id();
        let observed_at = prepared.observed_at;
        tasks.spawn(async move {
            backup
                .handoff_backup_schedule_occurrence(owner_user_id, occurrence_id, observed_at)
                .await
        });
    }
    let mut results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results.push(
            result
                .expect("handoff task must join")
                .expect("handoff must be bounded"),
        );
    }
    assert_eq!(results.len(), 12);
    assert!(
        results
            .iter()
            .any(BackupScheduleOccurrenceHandoffResult::is_created)
    );
    assert!(
        results
            .iter()
            .any(BackupScheduleOccurrenceHandoffResult::is_existing)
    );
    let run_id = results[0].maintenance_run().id();
    let handoff_id = results[0].handoff().occurrence_id();
    assert!(
        results
            .iter()
            .all(|result| result.maintenance_run().id() == run_id)
    );
    assert!(
        results
            .iter()
            .all(|result| result.handoff().occurrence_id() == handoff_id)
    );
    assert_eq!(
        count_handoffs(&fixture.inspection, fixture.backup_set_id).await,
        1
    );
    assert_eq!(
        count_maintenance_runs(&fixture.inspection, fixture.backup_set_id).await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_races_are_serializable_for_disable_and_edit() {
    let base_fixture = fixture("races").await;
    let prepared = prepare_occurrence(&base_fixture, "races", true).await;
    let backup = BackupService::new(base_fixture.pool.clone());
    let handoff_future = backup.handoff_backup_schedule_occurrence(
        base_fixture.owner_user_id,
        prepared.occurrence.id(),
        prepared.observed_at,
    );
    let disable_future = prepared.service.set_backup_schedule_enabled(
        base_fixture.owner_user_id,
        base_fixture.backup_set_id,
        false,
    );
    let (handoff_result, disable_result) = tokio::join!(handoff_future, disable_future);
    disable_result.expect("schedule disable race must not deadlock");
    match handoff_result {
        Ok(result) => {
            assert!(result.is_created());
            let replay = backup
                .handoff_backup_schedule_occurrence(
                    base_fixture.owner_user_id,
                    prepared.occurrence.id(),
                    prepared.observed_at,
                )
                .await
                .expect("winner handoff must replay after disable");
            assert!(replay.is_existing());
        }
        Err(BackupScheduleHandoffError::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled,
        )) => {}
        other => panic!("schedule-disable race returned an illegal outcome: {other:?}"),
    }
    assert!(count_handoffs(&base_fixture.inspection, base_fixture.backup_set_id).await <= 1);

    let edit_fixture = fixture("races-edit").await;
    let edit_prepared = prepare_occurrence(&edit_fixture, "races-edit", true).await;
    let edit_backup = BackupService::new(edit_fixture.pool.clone());
    let edit_handoff = edit_backup.handoff_backup_schedule_occurrence(
        edit_fixture.owner_user_id,
        edit_prepared.occurrence.id(),
        edit_prepared.observed_at,
    );
    let edit_schedule = configure_schedule(
        &edit_fixture,
        &edit_prepared.service,
        edit_fixture.backup_set_id,
        "races-edit-new-revision",
        daily("Asia/Tokyo", "23:30"),
    );
    let (handoff_result, edit_result) = tokio::join!(edit_handoff, edit_schedule);
    handoff_result.expect("revision-edit race handoff must succeed");
    let revision = edit_result;
    assert_eq!(revision.revision_number().get(), 2);
    assert_eq!(
        count_handoffs(&edit_fixture.inspection, edit_fixture.backup_set_id).await,
        1
    );
    assert_eq!(
        count_maintenance_runs(&edit_fixture.inspection, edit_fixture.backup_set_id).await,
        1
    );
    edit_fixture.close().await;
    base_fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_races_with_backup_set_disable_using_set_fence() {
    let fixture = fixture("set-race").await;
    let prepared = prepare_occurrence(&fixture, "set-race", true).await;
    let backup = BackupService::new(fixture.pool.clone());
    let handoff = backup.handoff_backup_schedule_occurrence(
        fixture.owner_user_id,
        prepared.occurrence.id(),
        prepared.observed_at,
    );
    let inspection = fixture.inspection.clone();
    let owner = fixture.owner_user_id;
    let set_id = fixture.backup_set_id;
    let disable = async move {
        sqlx::query(
            "UPDATE backup_sets
             SET state = 'DISABLED', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(set_id.into_uuid())
        .bind(owner.into_uuid())
        .execute(&inspection)
        .await
    };
    let (handoff_result, disable_result) = tokio::join!(handoff, disable);
    disable_result.expect("BackupSet disable race must not deadlock");
    match handoff_result {
        Ok(result) => {
            assert!(result.is_created());
            assert!(
                backup
                    .handoff_backup_schedule_occurrence(
                        fixture.owner_user_id,
                        prepared.occurrence.id(),
                        prepared.observed_at,
                    )
                    .await
                    .expect("set-disable winner handoff must replay")
                    .is_existing()
            );
        }
        Err(BackupScheduleHandoffError::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::BackupSetInactive,
        )) => {}
        other => panic!("BackupSet-disable race returned an illegal outcome: {other:?}"),
    }
    assert!(count_handoffs(&fixture.inspection, fixture.backup_set_id).await <= 1);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_manual_run_and_explicit_advance_regressions() {
    let fixture = fixture("manual").await;
    let backup = BackupService::new(fixture.pool.clone());
    let manual_set = fixture.backup_set_id;
    configure_policy(&fixture, manual_set, "manual").await;
    let manual = backup
        .create_backup_maintenance_run(
            fixture.owner_user_id,
            "manual-maintenance-regression".to_owned(),
            manual_set,
        )
        .await
        .expect("manual Prompt 49 creation must remain valid");
    assert_eq!(manual.state(), BackupMaintenanceRunState::Created);
    assert_eq!(count_handoffs(&fixture.inspection, manual_set).await, 0);
    let (manual_feed, _) = backup
        .list_backup_operations(fixture.owner_user_id, manual_set, None, None, 20)
        .await
        .expect("manual run must remain in one maintenance feed item");
    assert_eq!(manual_feed.len(), 1);
    assert_eq!(
        manual_feed[0].operation_kind(),
        BackupOperationKind::Maintenance
    );

    let scheduled_set = fixture.create_active_backup_set("scheduled-advance").await;
    configure_policy(&fixture, scheduled_set, "scheduled-advance").await;
    let schedule_service = BackupScheduleService::new(fixture.pool.clone());
    let revision = configure_schedule(
        &fixture,
        &schedule_service,
        scheduled_set,
        "scheduled-advance",
        daily("UTC", "09:00"),
    )
    .await;
    let schedule = schedule_service
        .get_backup_schedule(fixture.owner_user_id, scheduled_set)
        .await
        .expect("scheduled set schedule must be readable");
    let target_date = (schedule.effective_from().as_offset_datetime() + Duration::days(1)).date();
    let planned = revision.occurrence_on_local_date(target_date).unwrap();
    let observed_at = observed_after(planned);
    let occurrence = match schedule_service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            observed_at,
        )
        .await
        .unwrap()
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence) => occurrence,
        other => panic!("scheduled advance test needs a fresh occurrence, got {other:?}"),
    };
    let handoff = backup
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence.id(), observed_at)
        .await
        .expect("scheduled run must be created");
    let completed = backup
        .advance_backup_maintenance_run(fixture.owner_user_id, handoff.maintenance_run().id())
        .await
        .expect("explicit Prompt 49 advance must still work");
    assert_eq!(completed.state(), BackupMaintenanceRunState::Completed);
    assert!(completed.captured_snapshot_id().is_some());
    assert!(completed.expiry_plan_id().is_some());
    assert!(completed.expiry_execution_id().is_some());
    let replay = backup
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence.id(), observed_at)
        .await
        .expect("completed scheduled run must still replay through handoff");
    assert!(replay.is_existing());
    assert_eq!(replay.maintenance_run(), &completed);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_database_fences_enforce_scope_reuse_and_immutability() {
    let fixture = fixture("fences-sql").await;
    let prepared = prepare_occurrence(&fixture, "fences-sql", true).await;
    let backup = BackupService::new(fixture.pool.clone());
    let first = backup
        .handoff_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.occurrence.id(),
            prepared.observed_at,
        )
        .await
        .expect("first handoff must persist");
    let run_id = first.maintenance_run().id();
    let created_at = first.handoff().created_at().as_offset_datetime();

    let second_date =
        (prepared.schedule.effective_from().as_offset_datetime() + Duration::days(2)).date();
    let second_planned = prepared
        .revision
        .occurrence_on_local_date(second_date)
        .unwrap();
    let second_observed = observed_after(second_planned);
    let second_occurrence = match prepared
        .service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            prepared.schedule.id(),
            prepared.revision.id(),
            second_date,
            second_observed,
        )
        .await
        .unwrap()
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence) => occurrence,
        other => panic!("second occurrence must be fresh, got {other:?}"),
    };

    let reused_run = sqlx::query(
        "INSERT INTO backup_schedule_occurrence_handoffs
            (occurrence_id, owner_user_id, backup_set_id, schedule_id,
             maintenance_run_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(second_occurrence.id().into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(prepared.schedule.id().into_uuid())
    .bind(run_id.into_uuid())
    .bind(created_at)
    .execute(&fixture.inspection)
    .await;
    assert!(reused_run.is_err(), "one maintenance run cannot bind twice");

    let foreign_owner = sqlx::query(
        "INSERT INTO backup_schedule_occurrence_handoffs
            (occurrence_id, owner_user_id, backup_set_id, schedule_id,
             maintenance_run_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(prepared.occurrence.id().into_uuid())
    .bind(fixture.other_user_id.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(prepared.schedule.id().into_uuid())
    .bind(run_id.into_uuid())
    .bind(created_at)
    .execute(&fixture.inspection)
    .await;
    assert!(
        foreign_owner.is_err(),
        "owner scope must be database-enforced"
    );

    let other_set = fixture.create_active_backup_set("cross-set").await;
    let other_policy = configure_policy(&fixture, other_set, "cross-set").await;
    let other_run = backup
        .create_backup_maintenance_run(
            fixture.owner_user_id,
            "cross-set-manual-run".to_owned(),
            other_set,
        )
        .await
        .expect("second set manual run must persist");
    assert_eq!(other_run.policy_revision_id(), other_policy.id());
    let cross_set = sqlx::query(
        "INSERT INTO backup_schedule_occurrence_handoffs
            (occurrence_id, owner_user_id, backup_set_id, schedule_id,
             maintenance_run_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(prepared.occurrence.id().into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(other_set.into_uuid())
    .bind(prepared.schedule.id().into_uuid())
    .bind(other_run.id().into_uuid())
    .bind(created_at)
    .execute(&fixture.inspection)
    .await;
    assert!(
        cross_set.is_err(),
        "BackupSet scope must be database-enforced"
    );

    let update = sqlx::query(
        "UPDATE backup_schedule_occurrence_handoffs
         SET created_at = created_at
         WHERE occurrence_id = $1",
    )
    .bind(prepared.occurrence.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(update.is_err(), "handoff UPDATE must be rejected");
    let delete =
        sqlx::query("DELETE FROM backup_schedule_occurrence_handoffs WHERE occurrence_id = $1")
            .bind(prepared.occurrence.id().into_uuid())
            .execute(&fixture.inspection)
            .await;
    assert!(delete.is_err(), "handoff DELETE must be rejected");

    assert_eq!(
        backup
            .get_backup_schedule_occurrence_handoff(fixture.other_user_id, prepared.occurrence.id())
            .await
            .expect("foreign handoff lookup must conceal the occurrence"),
        None
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_schedule_handoff_transaction_rollback_leaves_no_run_or_handoff() {
    let fixture = fixture("rollback").await;
    let prepared = prepare_occurrence(&fixture, "rollback", true).await;
    let policy = configure_policy(&fixture, fixture.backup_set_id, "rollback-extra").await;
    let run_id = synveil_core::BackupMaintenanceRunId::new();
    let operation_id = format!("rollback-run-{run_id}");
    let capture_operation_id = format!("maint:{run_id}:capture");
    let expiry_operation_id = format!("maint:{run_id}:expiry-plan");
    let fingerprint =
        synveil_core::BackupMaintenanceRunRequest::new(fixture.backup_set_id).fingerprint();
    let mut transaction = fixture
        .inspection
        .begin()
        .await
        .expect("rollback transaction must begin");
    sqlx::query(
        "INSERT INTO backup_maintenance_runs
            (id, owner_user_id, backup_set_id, policy_revision_id,
             policy_revision_number, operation_id, fingerprint_version,
             request_fingerprint, capture_operation_id,
             expiry_plan_operation_id, state, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'CREATED', $11)",
    )
    .bind(run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(policy.id().into_uuid())
    .bind(i64::try_from(policy.revision_number().get()).unwrap())
    .bind(&operation_id)
    .bind(i16::try_from(fingerprint.version()).unwrap())
    .bind(fingerprint.as_bytes().as_slice())
    .bind(&capture_operation_id)
    .bind(&expiry_operation_id)
    .bind(timestamp("2026-09-02T01:00:00Z").as_offset_datetime())
    .execute(&mut *transaction)
    .await
    .expect("canonical-shaped run insert must succeed inside transaction");
    let invalid_handoff = sqlx::query(
        "INSERT INTO backup_schedule_occurrence_handoffs
            (occurrence_id, owner_user_id, backup_set_id, schedule_id,
             maintenance_run_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(prepared.schedule.id().into_uuid())
    .bind(run_id.into_uuid())
    .bind(timestamp("2026-09-02T01:00:00Z").as_offset_datetime())
    .execute(&mut *transaction)
    .await;
    assert!(invalid_handoff.is_err());
    transaction
        .rollback()
        .await
        .expect("failed handoff transaction must roll back");
    let persisted_run: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_maintenance_runs WHERE id = $1")
            .bind(run_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("rolled-back run count must succeed");
    assert_eq!(persisted_run, 0);
    assert_eq!(
        count_handoffs(&fixture.inspection, fixture.backup_set_id).await,
        0
    );
    fixture.close().await;
}
