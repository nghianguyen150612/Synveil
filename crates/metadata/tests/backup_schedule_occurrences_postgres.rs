//! PostgreSQL verification for Prompt 62's durable occurrence ledger.
//!
//! Run against a fresh disposable PostgreSQL 17 database with:
//! `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-metadata --test
//! backup_schedule_occurrences_postgres -- --ignored`.

use std::{borrow::Cow, path::Path};

use sqlx::{PgPool, migrate::Migrator};
use synveil_core::{
    BackupScheduleConfig, BackupScheduleId, BackupScheduleOccurrenceMaterializationResult,
    BackupScheduleOccurrenceNotEffectiveReason, BackupScheduleRecurrenceKind,
    BackupScheduleRevision, BackupScheduleTimezone, BackupScheduleWeekday, BackupSetId,
    DedupDomainId, Library, LibraryId, LogicalName, Node, NodeId, Timestamp, User, UserId,
    UserStatus,
};
use synveil_metadata::{
    BackupScheduleError, BackupScheduleService, BackupService, DatabaseConfig, DatabasePool,
    DomainRepository, MigrationRunner,
};
use time::{Date, Duration, OffsetDateTime, UtcOffset};

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

fn days_after(value: Timestamp, days: i64) -> Date {
    (value.as_offset_datetime() + Duration::days(days)).date()
}

fn observed_after(occurrence: synveil_core::PlannedScheduleOccurrence) -> Timestamp {
    occurrence
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(1))
        .expect("test occurrence supports observed-time addition")
}

fn occurrence_id(
    result: &BackupScheduleOccurrenceMaterializationResult,
) -> synveil_core::BackupScheduleOccurrenceId {
    result
        .occurrence()
        .expect("result must carry an occurrence")
        .id()
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

    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    let other_user_id = UserId::new();
    let repository = DomainRepository::new(&pool);
    for (user_id, login) in [
        (owner_user_id, "occurrence-owner"),
        (other_user_id, "occurrence-other"),
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

async fn count(pool: &PgPool, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .expect("table count must succeed")
}

async fn occurrence_count(pool: &PgPool, schedule_id: BackupScheduleId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_schedule_occurrences
         WHERE schedule_id = $1",
    )
    .bind(schedule_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("occurrence count must succeed")
}

async fn configure(
    fixture: &Fixture,
    service: &BackupScheduleService,
    operation: &str,
    config: BackupScheduleConfig,
) -> BackupScheduleRevision {
    service
        .configure_backup_schedule(
            fixture.owner_user_id,
            operation.to_owned(),
            fixture.backup_set_id,
            config,
        )
        .await
        .expect("schedule configuration must persist")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_occurrence_basic_due_replay_lookup_fences_and_immutability() {
    let fixture = fixture("occurrence-basic").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = configure(
        &fixture,
        &service,
        "occurrence-basic-config",
        daily("UTC", "09:00"),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("schedule must be readable");
    let target_date = days_after(schedule.effective_from(), 1);
    let planned = revision
        .occurrence_on_local_date(target_date)
        .expect("daily target must exist");

    let not_due = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            planned
                .scheduled_for_utc()
                .checked_sub_std(std::time::Duration::from_secs(1))
                .unwrap(),
        )
        .await
        .expect("future target must return a bounded outcome");
    assert_eq!(
        not_due,
        BackupScheduleOccurrenceMaterializationResult::NotDue
    );
    assert_eq!(
        occurrence_count(&fixture.inspection, schedule.id()).await,
        0
    );

    let created = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            observed_after(planned),
        )
        .await
        .expect("due target must materialize");
    assert!(matches!(
        created,
        BackupScheduleOccurrenceMaterializationResult::Created(_)
    ));
    let canonical_id = occurrence_id(&created);
    assert_eq!(
        occurrence_count(&fixture.inspection, schedule.id()).await,
        1
    );

    let replay = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            observed_after(planned),
        )
        .await
        .expect("lost-response retry must replay");
    assert!(matches!(
        replay,
        BackupScheduleOccurrenceMaterializationResult::Existing(_)
    ));
    assert_eq!(occurrence_id(&replay), canonical_id);
    assert_eq!(
        occurrence_count(&fixture.inspection, schedule.id()).await,
        1
    );

    let by_id = service
        .get_backup_schedule_occurrence(fixture.owner_user_id, canonical_id)
        .await
        .expect("occurrence identity lookup must succeed");
    let by_key = service
        .get_backup_schedule_occurrence_by_logical_key(
            fixture.owner_user_id,
            revision.id(),
            target_date,
        )
        .await
        .expect("logical-key lookup must succeed")
        .expect("logical-key occurrence must exist");
    assert_eq!(by_id, by_key);
    assert_eq!(by_id.timezone().as_str(), "UTC");
    assert_eq!(by_id.resolved_local_wall_time().to_string(), "09:00");
    assert_eq!(by_id.scheduled_for_utc(), planned.scheduled_for_utc());

    assert_eq!(
        service
            .get_backup_schedule_occurrence(fixture.other_user_id, canonical_id)
            .await,
        Err(BackupScheduleError::NotFound)
    );

    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule disable must persist");
    let replay_while_disabled = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            observed_after(planned),
        )
        .await
        .expect("historical replay must survive disable");
    assert_eq!(occurrence_id(&replay_while_disabled), canonical_id);
    assert!(matches!(
        replay_while_disabled,
        BackupScheduleOccurrenceMaterializationResult::Existing(_)
    ));

    assert!(
        sqlx::query(
            "UPDATE backup_schedule_occurrences
             SET materialized_at = materialized_at + INTERVAL '1 second'
             WHERE id = $1",
        )
        .bind(canonical_id.into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err(),
        "database must reject occurrence UPDATE"
    );
    assert!(
        sqlx::query("DELETE FROM backup_schedule_occurrences WHERE id = $1")
            .bind(canonical_id.into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err(),
        "database must reject occurrence DELETE"
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_occurrence_invalid_disabled_inactive_and_scope_targets_create_zero_rows() {
    let fixture = fixture("occurrence-negative").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = configure(
        &fixture,
        &service,
        "occurrence-negative-config",
        weekly("UTC", "10:00", vec![BackupScheduleWeekday::Wednesday]),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let mut wrong_date = days_after(schedule.effective_from(), 1);
    while wrong_date.weekday().number_days_from_monday()
        == BackupScheduleWeekday::Wednesday.ordinal() - 1
    {
        wrong_date = wrong_date.next_day().unwrap();
    }
    let invalid = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            wrong_date,
            timestamp("9999-12-30T00:00:00Z"),
        )
        .await
        .expect("wrong weekday must return a bounded outcome");
    assert_eq!(
        invalid,
        BackupScheduleOccurrenceMaterializationResult::InvalidTarget
    );

    let mut selected_date = wrong_date;
    while selected_date.weekday().number_days_from_monday()
        != BackupScheduleWeekday::Wednesday.ordinal() - 1
    {
        selected_date = selected_date.next_day().unwrap();
    }
    let planned = revision
        .occurrence_on_local_date(selected_date)
        .expect("selected weekday must plan");
    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .unwrap();
    assert_eq!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.owner_user_id,
                schedule.id(),
                revision.id(),
                selected_date,
                observed_after(planned),
            )
            .await
            .unwrap(),
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled
        )
    );
    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, true)
        .await
        .unwrap();
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
    .unwrap();
    assert_eq!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.owner_user_id,
                schedule.id(),
                revision.id(),
                selected_date,
                observed_after(planned),
            )
            .await
            .unwrap(),
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::BackupSetInactive
        )
    );
    assert_eq!(
        occurrence_count(&fixture.inspection, schedule.id()).await,
        0
    );
    assert_eq!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.other_user_id,
                schedule.id(),
                revision.id(),
                selected_date,
                observed_after(planned),
            )
            .await,
        Err(BackupScheduleError::NotFound)
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_occurrence_concurrent_exact_target_converges_to_one_identity() {
    let fixture = fixture("occurrence-concurrency").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = configure(
        &fixture,
        &service,
        "occurrence-concurrency-config",
        daily("America/New_York", "01:30"),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let target_date = days_after(schedule.effective_from(), 2);
    let planned = revision.occurrence_on_local_date(target_date).unwrap();
    let observed = observed_after(planned);
    let owner_user_id = fixture.owner_user_id;
    let schedule_id = schedule.id();
    let revision_id = revision.id();

    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let service = service.clone();
        tasks.spawn(async move {
            service
                .materialize_due_backup_schedule_occurrence(
                    owner_user_id,
                    schedule_id,
                    revision_id,
                    target_date,
                    observed,
                )
                .await
        });
    }

    let mut identities = Vec::new();
    let mut created = 0;
    while let Some(result) = tasks.join_next().await {
        let result = result
            .expect("materialization task must join")
            .expect("materialization task must return a bounded result");
        if matches!(
            result,
            BackupScheduleOccurrenceMaterializationResult::Created(_)
        ) {
            created += 1;
        }
        identities.push(occurrence_id(&result));
    }
    assert_eq!(created, 1);
    assert_eq!(identities.len(), 12);
    assert!(identities.iter().all(|id| *id == identities[0]));
    assert_eq!(occurrence_count(&fixture.inspection, schedule_id).await, 1);
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_effectivity_noop_edit_enable_gap_and_same_instant_fences() {
    let fixture = fixture("occurrence-effectivity").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = configure(
        &fixture,
        &service,
        "occurrence-effectivity-first",
        daily("UTC", "09:00"),
    )
    .await;
    let initial = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();

    let noop = configure(
        &fixture,
        &service,
        "occurrence-effectivity-noop",
        daily("UTC", "09:00"),
    )
    .await;
    let after_noop = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    assert_eq!(noop.id(), first.id());
    assert_eq!(after_noop.effective_from(), initial.effective_from());

    let old_date = days_after(initial.effective_from(), 1);
    let old_planned = first.occurrence_on_local_date(old_date).unwrap();
    let old_created = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            initial.id(),
            first.id(),
            old_date,
            observed_after(old_planned),
        )
        .await
        .unwrap();
    assert!(matches!(
        old_created,
        BackupScheduleOccurrenceMaterializationResult::Created(_)
    ));

    // 16:00 Asia/Ho_Chi_Minh is the same absolute instant as 09:00 UTC.
    let same_instant_revision = configure(
        &fixture,
        &service,
        "occurrence-effectivity-same-instant",
        daily("Asia/Ho_Chi_Minh", "16:00"),
    )
    .await;
    let same_instant = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            initial.id(),
            same_instant_revision.id(),
            old_date,
            observed_after(
                same_instant_revision
                    .occurrence_on_local_date(old_date)
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        same_instant,
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::ScheduleInstantAlreadyMaterialized
        )
    );

    // A new revision whose midnight target is on the effectivity date cannot
    // backfill that already-past/equal local instant.
    let past_revision = configure(
        &fixture,
        &service,
        "occurrence-effectivity-past",
        daily("UTC", "00:00"),
    )
    .await;
    let after_past_edit = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let past_date = after_past_edit.effective_from().as_offset_datetime().date();
    let past_planned = past_revision.occurrence_on_local_date(past_date).unwrap();
    assert_eq!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.owner_user_id,
                initial.id(),
                past_revision.id(),
                past_date,
                observed_after(past_planned),
            )
            .await
            .unwrap(),
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::BeforeEffectiveFrom
        )
    );

    let before_disable = after_past_edit.effective_from();
    let disabled = service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .unwrap();
    let reenabled = service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, true)
        .await
        .unwrap();
    assert!(disabled.effective_from() > before_disable);
    assert!(reenabled.effective_from() > disabled.effective_from());
    assert_eq!(reenabled.current_revision_id(), past_revision.id());

    let gap_date = reenabled.effective_from().as_offset_datetime().date();
    let gap_planned = past_revision.occurrence_on_local_date(gap_date).unwrap();
    assert_eq!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.owner_user_id,
                initial.id(),
                past_revision.id(),
                gap_date,
                observed_after(gap_planned),
            )
            .await
            .unwrap(),
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::BeforeEffectiveFrom
        )
    );
    let later_date = gap_date.next_day().unwrap();
    let later_planned = past_revision.occurrence_on_local_date(later_date).unwrap();
    assert!(matches!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.owner_user_id,
                initial.id(),
                past_revision.id(),
                later_date,
                observed_after(later_planned),
            )
            .await
            .unwrap(),
        BackupScheduleOccurrenceMaterializationResult::Created(_)
    ));
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_edit_to_later_same_day_and_lost_response_revision_rules() {
    let fixture = fixture("occurrence-edit-replay").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = configure(
        &fixture,
        &service,
        "occurrence-edit-replay-first",
        daily("UTC", "09:00"),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let target_date = days_after(schedule.effective_from(), 1);
    let first_planned = first.occurrence_on_local_date(target_date).unwrap();
    let first_created = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            first.id(),
            target_date,
            observed_after(first_planned),
        )
        .await
        .unwrap();
    let first_occurrence_id = occurrence_id(&first_created);

    let later = configure(
        &fixture,
        &service,
        "occurrence-edit-replay-later",
        daily("UTC", "18:00"),
    )
    .await;
    let later_planned = later.occurrence_on_local_date(target_date).unwrap();
    let later_created = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            later.id(),
            target_date,
            observed_after(later_planned),
        )
        .await
        .unwrap();
    assert!(matches!(
        later_created,
        BackupScheduleOccurrenceMaterializationResult::Created(_)
    ));
    assert_ne!(occurrence_id(&later_created), first_occurrence_id);
    assert_ne!(
        later_created.occurrence().unwrap().scheduled_for_utc(),
        first_created.occurrence().unwrap().scheduled_for_utc()
    );

    // The first response can be lost and replayed after the edit because the
    // exact old logical occurrence already committed.
    let replay_old = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            first.id(),
            target_date,
            observed_after(first_planned),
        )
        .await
        .unwrap();
    assert!(matches!(
        replay_old,
        BackupScheduleOccurrenceMaterializationResult::Existing(_)
    ));
    assert_eq!(occurrence_id(&replay_old), first_occurrence_id);

    // An old logical date that never committed cannot materialize after the
    // revision has become historical.
    let uncommitted_date = target_date.next_day().unwrap();
    let uncommitted_planned = first.occurrence_on_local_date(uncommitted_date).unwrap();
    assert_eq!(
        service
            .materialize_due_backup_schedule_occurrence(
                fixture.owner_user_id,
                schedule.id(),
                first.id(),
                uncommitted_date,
                observed_after(uncommitted_planned),
            )
            .await
            .unwrap(),
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::RevisionSuperseded
        )
    );
    assert_eq!(
        occurrence_count(&fixture.inspection, schedule.id()).await,
        2
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_configuration_and_disable_races_serialize_on_backup_set_fence() {
    let fixture = fixture("occurrence-races").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let first = configure(
        &fixture,
        &service,
        "occurrence-race-first",
        daily("UTC", "11:00"),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let materialize_date = days_after(schedule.effective_from(), 1);
    let planned = first.occurrence_on_local_date(materialize_date).unwrap();

    let materializer = service.clone();
    let configurator = service.clone();
    let (materialized, configured) = tokio::join!(
        materializer.materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            first.id(),
            materialize_date,
            observed_after(planned),
        ),
        configurator.configure_backup_schedule(
            fixture.owner_user_id,
            "occurrence-race-edit".to_owned(),
            fixture.backup_set_id,
            daily("UTC", "12:00"),
        )
    );
    let configured = configured.expect("concurrent edit must commit atomically");
    assert_eq!(configured.revision_number().get(), 2);
    let materialized = materialized.expect("materializer must return a bounded race outcome");
    match materialized {
        BackupScheduleOccurrenceMaterializationResult::Created(_) => {
            assert_eq!(
                occurrence_count(&fixture.inspection, schedule.id()).await,
                1
            );
        }
        BackupScheduleOccurrenceMaterializationResult::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::RevisionSuperseded,
        ) => {
            assert_eq!(
                occurrence_count(&fixture.inspection, schedule.id()).await,
                0
            );
        }
        other => panic!("configuration race returned illegal outcome: {other:?}"),
    }

    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let current_revision = current.current_revision().clone();
    let disable_date = days_after(current.effective_from(), 2);
    let disable_planned = current_revision
        .occurrence_on_local_date(disable_date)
        .unwrap();
    let materializer = service.clone();
    let disabler = service.clone();
    let (materialized, disabled) = tokio::join!(
        materializer.materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            current.id(),
            current_revision.id(),
            disable_date,
            observed_after(disable_planned),
        ),
        disabler.set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false,)
    );
    let disabled = disabled.expect("concurrent disable must commit atomically");
    assert!(!disabled.enabled());
    let materialized = materialized.expect("materializer must return a bounded race outcome");
    assert!(matches!(
        materialized,
        BackupScheduleOccurrenceMaterializationResult::Created(_)
            | BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled
            )
    ));
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_occurrence_composite_fks_and_no_side_effect_boundaries_hold() {
    let fixture = fixture("occurrence-side-effects").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = configure(
        &fixture,
        &service,
        "occurrence-side-effects-config",
        daily("UTC", "14:00"),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let target_date = days_after(schedule.effective_from(), 1);
    let planned = revision.occurrence_on_local_date(target_date).unwrap();
    let before = [
        count(&fixture.inspection, "backup_snapshots").await,
        count(&fixture.inspection, "backup_maintenance_runs").await,
        count(&fixture.inspection, "change_journal").await,
        count(&fixture.inspection, "device_sync_checkpoints").await,
        count(&fixture.inspection, "backup_snapshot_content_pins").await,
        count(&fixture.inspection, "object_gc_candidates").await,
        count(&fixture.inspection, "backup_restore_executions").await,
        count(&fixture.inspection, "backup_prune_executions").await,
    ];
    let current_revision_before = schedule.current_revision_id();
    let effective_before = schedule.effective_from();

    let created = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            target_date,
            observed_after(planned),
        )
        .await
        .unwrap();
    assert!(created.occurrence().is_some());

    let after = [
        count(&fixture.inspection, "backup_snapshots").await,
        count(&fixture.inspection, "backup_maintenance_runs").await,
        count(&fixture.inspection, "change_journal").await,
        count(&fixture.inspection, "device_sync_checkpoints").await,
        count(&fixture.inspection, "backup_snapshot_content_pins").await,
        count(&fixture.inspection, "object_gc_candidates").await,
        count(&fixture.inspection, "backup_restore_executions").await,
        count(&fixture.inspection, "backup_prune_executions").await,
    ];
    assert_eq!(after, before);
    let current = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    assert_eq!(current.current_revision_id(), current_revision_before);
    assert_eq!(current.effective_from(), effective_before);

    let second_set = fixture.create_active_backup_set("forged-scope").await;
    let second_revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "occurrence-side-effects-second".to_owned(),
            second_set,
            daily("UTC", "15:00"),
        )
        .await
        .unwrap();
    let forged_id = synveil_core::BackupScheduleOccurrenceId::new();
    assert!(
        sqlx::query(
            "INSERT INTO backup_schedule_occurrences
                (id, owner_user_id, backup_set_id, schedule_id,
                 schedule_revision_id, local_calendar_date,
                 resolved_local_time_minute, scheduled_for_utc, materialized_at)
             VALUES ($1, $2, $3, $4, $5, $6, 840,
                     $7 + INTERVAL '1 hour', clock_timestamp())",
        )
        .bind(forged_id.into_uuid())
        .bind(fixture.owner_user_id.into_uuid())
        .bind(fixture.backup_set_id.into_uuid())
        .bind(second_revision.schedule_id().into_uuid())
        .bind(revision.id().into_uuid())
        .bind(target_date)
        .bind(planned.scheduled_for_utc().as_offset_datetime())
        .execute(&fixture.inspection)
        .await
        .is_err(),
        "revision from schedule A plus schedule B must fail at the DB boundary"
    );
    assert!(
        sqlx::query(
            "INSERT INTO backup_schedule_occurrences
                (id, owner_user_id, backup_set_id, schedule_id,
                 schedule_revision_id, local_calendar_date,
                 resolved_local_time_minute, scheduled_for_utc, materialized_at)
             VALUES ($1, $2, $3, $4, $5, $6, 840,
                     $7 + INTERVAL '2 hours', clock_timestamp())",
        )
        .bind(synveil_core::BackupScheduleOccurrenceId::new().into_uuid())
        .bind(fixture.other_user_id.into_uuid())
        .bind(fixture.backup_set_id.into_uuid())
        .bind(schedule.id().into_uuid())
        .bind(revision.id().into_uuid())
        .bind(target_date)
        .bind(planned.scheduled_for_utc().as_offset_datetime())
        .execute(&fixture.inspection)
        .await
        .is_err(),
        "cross-owner occurrence scope must fail at the DB boundary"
    );
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_edit_to_future_same_day_becomes_materializable_when_due() {
    let fixture = fixture("occurrence-edit-future").await;
    let service = BackupScheduleService::new(fixture.pool.clone());
    configure(
        &fixture,
        &service,
        "occurrence-edit-future-base",
        daily("UTC", "00:00"),
    )
    .await;

    let database_now: OffsetDateTime = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&fixture.inspection)
        .await
        .unwrap();
    let candidates = [
        ("UTC", UtcOffset::UTC),
        (
            "Etc/GMT+12",
            UtcOffset::from_hms(-12, 0, 0).expect("fixed offset must be valid"),
        ),
        (
            "Pacific/Kiritimati",
            UtcOffset::from_hms(14, 0, 0).expect("fixed offset must be valid"),
        ),
    ];
    let (zone, local_now) = candidates
        .into_iter()
        .map(|(zone, offset)| (zone, database_now.to_offset(offset)))
        .find(|(_, local)| local.hour() < 23)
        .expect("one representative timezone must have same-day room");
    let local_target = local_now + Duration::minutes(10);
    assert_eq!(local_target.date(), local_now.date());
    let target_time = format!("{:02}:{:02}", local_target.hour(), local_target.minute());

    let revision = configure(
        &fixture,
        &service,
        "occurrence-edit-future-later",
        daily(zone, &target_time),
    )
    .await;
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let planned = revision
        .occurrence_on_local_date(local_target.date())
        .expect("later same-day target must plan");
    assert!(planned.scheduled_for_utc() > schedule.effective_from());
    let created = service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            revision.id(),
            local_target.date(),
            observed_after(planned),
        )
        .await
        .unwrap();
    assert!(matches!(
        created,
        BackupScheduleOccurrenceMaterializationResult::Created(_)
    ));
    fixture.pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_forward_migration_preserves_prompt61_schedule_and_prompt49_run() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let base = PgPool::connect(&url)
        .await
        .expect("base test pool must connect");
    let schema = format!("prompt62_forward_{}", uuid::Uuid::now_v7().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&base)
        .await
        .expect("isolated forward-migration schema must be created");
    let separator = if url.contains('?') { '&' } else { '?' };
    let schema_url = format!("{url}{separator}options=-csearch_path%3D{schema}");
    let inspection = PgPool::connect(&schema_url)
        .await
        .expect("schema-scoped pool must connect");

    let all = Migrator::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations"))
        .await
        .expect("migration source must load");
    let prompt61 = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 20260902000000)
                .cloned()
                .collect(),
        ),
        ignore_missing: false,
        locking: false,
        no_tx: false,
    };
    let through_prompt63 = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 20260902000002)
                .cloned()
                .collect(),
        ),
        ignore_missing: false,
        locking: false,
        no_tx: false,
    };
    prompt61
        .run(&inspection)
        .await
        .expect("migrations 1-30 must apply in the isolated schema");

    let config = DatabaseConfig::from_url(&schema_url).expect("schema test URL must be valid");
    let database_pool = DatabasePool::connect(&config)
        .await
        .expect("schema-scoped metadata pool must connect");
    let repository = DomainRepository::new(&database_pool);
    let observed_at = timestamp("2026-09-01T08:00:00Z");
    let owner_user_id = UserId::new();
    let user = User::new(
        owner_user_id,
        synveil_core::LoginIdentifier::new(
            format!("forward-owner-{owner_user_id}"),
            format!("forward-owner-key-{owner_user_id}"),
        )
        .unwrap(),
        UserStatus::Active,
        observed_at,
    );
    repository.insert_user(&user).await.unwrap();
    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        owner_user_id,
        name(format!("forward-library-{library_id}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .unwrap();
    repository
        .insert_library_with_root(&library, &root)
        .await
        .unwrap();
    let backup_set_id = BackupSetId::new();
    BackupService::new(database_pool.clone())
        .create_backup_set(
            owner_user_id,
            backup_set_id,
            name(format!("forward-backup-{backup_set_id}")),
            library_id,
            None,
            observed_at,
        )
        .await
        .unwrap();

    let schedule_id = BackupScheduleId::new();
    let revision_id = synveil_core::BackupScheduleRevisionId::new();
    let schedule_config = daily("UTC", "09:00");
    let request = synveil_core::BackupScheduleRequest::new(backup_set_id, schedule_config.clone());
    let fingerprint = request
        .fingerprint_for_version(1)
        .expect("Prompt 61 fingerprint projection must remain available");
    let created_at = timestamp("2026-09-01T09:00:00Z").as_offset_datetime();
    let last_transition = timestamp("2026-09-02T13:45:00Z").as_offset_datetime();
    let mut transaction = inspection.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO backup_schedules
            (id, owner_user_id, backup_set_id, current_revision_id,
             enabled, created_at, updated_at)
         VALUES ($1, $2, $3, $4, TRUE, $5, $6)",
    )
    .bind(schedule_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(revision_id.into_uuid())
    .bind(created_at)
    .bind(last_transition)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO backup_schedule_revisions
            (id, schedule_id, owner_user_id, backup_set_id, revision_number,
             operation_id, fingerprint_version, request_fingerprint,
             recurrence_kind, timezone, local_time_minute, weekly_days, created_at)
         VALUES ($1, $2, $3, $4, 1, 'forward-config-operation', 1, $5,
                 'DAILY', 'UTC', 540, '{}'::SMALLINT[], $6)",
    )
    .bind(revision_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(fingerprint.as_bytes().as_slice())
    .bind(created_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO backup_schedule_operations
            (owner_user_id, operation_id, backup_set_id, schedule_id,
             result_revision_id, fingerprint_version, request_fingerprint, created_at)
         VALUES ($1, 'forward-config-operation', $2, $3, $4, 1, $5, $6)",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(revision_id.into_uuid())
    .bind(fingerprint.as_bytes().as_slice())
    .bind(created_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let backup = BackupService::new(database_pool.clone());
    let policy = backup
        .configure_snapshot_retention_policy(
            owner_user_id,
            "forward-retention-policy".to_owned(),
            backup_set_id,
            1,
            86_400,
        )
        .await
        .unwrap();
    let manual_run = backup
        .create_backup_maintenance_run(
            owner_user_id,
            "forward-manual-maintenance".to_owned(),
            backup_set_id,
        )
        .await
        .unwrap();
    assert_eq!(manual_run.policy_revision_id(), policy.id());

    through_prompt63
        .run(&inspection)
        .await
        .expect("Prompt 62 and Prompt 63 migrations must apply over Prompt 61 data");

    let first_occurrence_id = synveil_core::BackupScheduleOccurrenceId::new();
    sqlx::query(
        "INSERT INTO backup_schedule_occurrences
            (id, owner_user_id, backup_set_id, schedule_id,
             schedule_revision_id, local_calendar_date,
             resolved_local_time_minute, scheduled_for_utc, materialized_at)
         VALUES ($1, $2, $3, $4, $5, DATE '2026-09-03', 540,
                 TIMESTAMPTZ '2026-09-03 09:00:00+00',
                 TIMESTAMPTZ '2026-09-03 09:00:01+00')",
    )
    .bind(first_occurrence_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(revision_id.into_uuid())
    .execute(&inspection)
    .await
    .expect("Prompt 62 occurrence must exist before Prompt 65 migration");
    sqlx::query(
        "INSERT INTO backup_schedule_occurrence_handoffs
            (occurrence_id, owner_user_id, backup_set_id, schedule_id,
             maintenance_run_id, created_at)
         VALUES ($1, $2, $3, $4, $5, TIMESTAMPTZ '2026-09-03 09:00:02+00')",
    )
    .bind(first_occurrence_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(manual_run.id().into_uuid())
    .execute(&inspection)
    .await
    .expect("Prompt 63 handoff must exist before Prompt 65 migration");

    let second_revision_id = synveil_core::BackupScheduleRevisionId::new();
    let second_config = daily("UTC", "10:00");
    let second_fingerprint =
        synveil_core::BackupScheduleRequest::new(backup_set_id, second_config.clone())
            .fingerprint_for_version(1)
            .unwrap();
    sqlx::query(
        "INSERT INTO backup_schedule_revisions
            (id, schedule_id, owner_user_id, backup_set_id, revision_number,
             operation_id, fingerprint_version, request_fingerprint,
             recurrence_kind, timezone, local_time_minute, weekly_days, created_at)
         VALUES ($1, $2, $3, $4, 2, 'forward-config-operation-two', 1, $5,
                 'DAILY', 'UTC', 600, '{}'::SMALLINT[],
                 TIMESTAMPTZ '2026-09-04 00:00:00+00')",
    )
    .bind(second_revision_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(second_fingerprint.as_bytes().as_slice())
    .execute(&inspection)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE backup_schedules
         SET current_revision_id = $1,
             effective_from = TIMESTAMPTZ '2026-09-04 00:00:00+00',
             updated_at = TIMESTAMPTZ '2026-09-04 00:00:00+00'
         WHERE id = $2",
    )
    .bind(second_revision_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .execute(&inspection)
    .await
    .unwrap();
    let second_occurrence_id = synveil_core::BackupScheduleOccurrenceId::new();
    sqlx::query(
        "INSERT INTO backup_schedule_occurrences
            (id, owner_user_id, backup_set_id, schedule_id,
             schedule_revision_id, local_calendar_date,
             resolved_local_time_minute, scheduled_for_utc, materialized_at)
         VALUES ($1, $2, $3, $4, $5, DATE '2026-09-05', 600,
                 TIMESTAMPTZ '2026-09-05 10:00:00+00',
                 TIMESTAMPTZ '2026-09-05 10:00:01+00')",
    )
    .bind(second_occurrence_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(second_revision_id.into_uuid())
    .execute(&inspection)
    .await
    .unwrap();
    for (enabled, effective) in [
        (false, "2026-09-06 00:00:00+00"),
        (true, "2026-09-07 00:00:00+00"),
    ] {
        sqlx::query(
            "UPDATE backup_schedules
             SET enabled = $1, effective_from = $2::timestamptz,
                 updated_at = $2::timestamptz
             WHERE id = $3",
        )
        .bind(enabled)
        .bind(effective)
        .bind(schedule_id.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    }

    all.run(&inspection)
        .await
        .expect("Prompt 65 migration must preserve populated Prompt 64 data");
    let migrated_schedule = BackupScheduleService::new(database_pool.clone())
        .get_backup_schedule(owner_user_id, backup_set_id)
        .await
        .expect("Prompt 61 schedule must remain readable after Prompt 65");
    assert_eq!(
        migrated_schedule.current_revision().misfire_mode(),
        synveil_core::BackupScheduleMisfireMode::LatestOnly
    );
    assert_eq!(
        migrated_schedule.current_revision().max_lateness_seconds(),
        synveil_core::DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS
    );
    assert_eq!(
        migrated_schedule
            .current_revision()
            .request_fingerprint()
            .version(),
        1
    );
    let schedule_service = BackupScheduleService::new(database_pool.clone());
    let migrated_effective_from = migrated_schedule.effective_from();
    let post_upgrade_noop = schedule_service
        .configure_backup_schedule(
            owner_user_id,
            "forward-v2-noop-operation".to_owned(),
            backup_set_id,
            second_config.clone(),
        )
        .await
        .expect("new v2 no-op must preserve the migrated v1 current revision");
    assert_eq!(post_upgrade_noop.id(), second_revision_id);
    assert_eq!(post_upgrade_noop.request_fingerprint().version(), 1);
    let post_upgrade_noop_replay = schedule_service
        .configure_backup_schedule(
            owner_user_id,
            "forward-v2-noop-operation".to_owned(),
            backup_set_id,
            second_config,
        )
        .await
        .expect("new v2 no-op evidence must replay its migrated v1 result");
    assert_eq!(post_upgrade_noop_replay.id(), second_revision_id);
    assert_eq!(
        schedule_service
            .get_backup_schedule(owner_user_id, backup_set_id)
            .await
            .unwrap()
            .effective_from(),
        migrated_effective_from
    );
    assert_eq!(
        sqlx::query_scalar::<_, i16>(
            "SELECT fingerprint_version FROM backup_schedule_operations
             WHERE owner_user_id = $1 AND operation_id = 'forward-v2-noop-operation'",
        )
        .bind(owner_user_id.into_uuid())
        .fetch_one(&inspection)
        .await
        .unwrap(),
        2
    );
    let historical_replay = schedule_service
        .configure_backup_schedule(
            owner_user_id,
            "forward-config-operation".to_owned(),
            backup_set_id,
            schedule_config,
        )
        .await
        .expect("historical v1 operation must replay canonically after Prompt 65");
    assert_eq!(historical_replay.id(), revision_id);
    assert_eq!(historical_replay.request_fingerprint().version(), 1);
    let (effective_from, current_revision_id): (OffsetDateTime, uuid::Uuid) = sqlx::query_as(
        "SELECT effective_from, current_revision_id
             FROM backup_schedules
             WHERE id = $1",
    )
    .bind(schedule_id.into_uuid())
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!(
        effective_from,
        timestamp("2026-09-07T00:00:00Z").as_offset_datetime()
    );
    assert_eq!(current_revision_id, second_revision_id.into_uuid());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_schedule_revisions WHERE schedule_id = $1",
        )
        .bind(schedule_id.into_uuid())
        .fetch_one(&inspection)
        .await
        .unwrap(),
        2
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT to_regclass('backup_schedule_occurrences') IS NOT NULL",
        )
        .fetch_one(&inspection)
        .await
        .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_schedule_occurrences")
            .fetch_one(&inspection)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_schedule_occurrence_handoffs")
            .fetch_one(&inspection)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_schedule_misfire_skips")
            .fetch_one(&inspection)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_schedule_revisions
             WHERE misfire_mode = 'LATEST_ONLY' AND max_lateness_seconds = 604800",
        )
        .fetch_one(&inspection)
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_maintenance_runs WHERE id = $1",)
            .bind(manual_run.id().into_uuid())
            .fetch_one(&inspection)
            .await
            .unwrap(),
        1
    );
    database_pool.close().await;
    inspection.close().await;
    base.close().await;
}
