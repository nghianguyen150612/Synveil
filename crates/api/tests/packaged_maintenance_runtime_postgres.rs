//! Prompt 79 — package-level hardened execution against PostgreSQL 17.
//!
//! Proves the NATIVE PACKAGED binary (extracted from the built `.deb`,
//! byte-identical to the release binary) performs a real scheduled-maintenance
//! activation through the Prompt 77 credential-file boundary:
//! `SYNVEIL_DATABASE_CREDENTIAL_FILE` -> packaged one-shot -> PostgreSQL 17.
//!
//! - idle: empty database -> exit 0, `tick=Idle worker=Idle`.
//! - due: seeded due daily schedule -> exactly one bounded cycle
//!   (`MaterializedAndHandedOff` + `Stepped`, `CREATED -> SNAPSHOT_CAPTURED`).
//! - existing: seeded handed-off work, no new due tick -> `tick=Idle` with the
//!   worker advancing at most one existing maintenance step.
//!
//! Each test uses a fresh isolated disposable database via
//! `SYNVEIL_TEST_DATABASE_URL` (server maintenance database) and never touches
//! the host package database. Run with:
//! `SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5433/postgres \
//!   cargo test -p synveil-api --test packaged_maintenance_runtime_postgres \
//!   -- --ignored --test-threads=1 --nocapture`
//!
//! Requires `deploy/packages/build.sh` artifacts under `target/packages/`.

use std::{fs, path::PathBuf, process::Command};

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunState, BackupScheduleConfig, BackupSetId, DedupDomainId, Library,
    LibraryId, LogicalName, Node, NodeId, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupScheduleService, BackupService, DatabaseConfig, DatabasePool, DomainRepository,
    MigrationRunner,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn unique_tag(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("p79_{label}_{nanos}_{}", std::process::id())
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Extract the packaged binary from the built DEB (proves the packaged
/// artifact itself executes; its SHA parity with the release binary is locked
/// by `linux_native_packaging_units`).
fn packaged_binary() -> PathBuf {
    let dir = repo_root().join("target/packages");
    let mut debs: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!("target/packages unreadable (run deploy/packages/build.sh first): {e}")
        })
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "deb"))
        .collect();
    debs.sort();
    let deb = debs
        .pop()
        .expect("no .deb found (run deploy/packages/build.sh first)");
    let dest = std::env::temp_dir().join(unique_tag("pkgbin"));
    fs::create_dir_all(&dest).expect("tempdir");
    let out = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "set -euo pipefail; ar p {} data.tar.gz | tar -xzf - -C {}",
            shell_quote(&deb.to_string_lossy()),
            shell_quote(&dest.to_string_lossy())
        ))
        .output()
        .expect("extract deb");
    assert!(
        out.status.success(),
        "deb extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("packaged binary source package: {}", deb.display());
    let bin = dest.join("usr/bin/synveil-scheduled-maintenance-once");
    assert!(bin.is_file(), "packaged binary missing in DEB payload");
    bin
}

struct Seed {
    db_url: String,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    pool: DatabasePool,
    inspection: PgPool,
}

async fn new_db(label: &str) -> (String, DatabasePool, PgPool) {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL").expect(
        "SYNVEIL_TEST_DATABASE_URL must identify the disposable server maintenance database",
    );
    let db_name = unique_tag(label);
    let maintenance = PgPool::connect(&base)
        .await
        .expect("maintenance connection must succeed");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("isolated test database must be created");
    maintenance.close().await;
    let url = format!(
        "{}/{}",
        base.rsplit_once('/')
            .expect("base URL must contain database path")
            .0,
        db_name
    );
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("migrations must apply");
    assert!(status.is_current());
    assert_eq!(status.applied_versions().len(), 36);
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    (url, pool, inspection)
}

fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("test logical name must be valid")
}

fn fixed_seed_timestamp() -> Timestamp {
    Timestamp::parse("2026-09-02T00:00:00.123456Z").expect("fixed seed timestamp must parse")
}

fn fixed_effective_from() -> Timestamp {
    Timestamp::parse("2026-09-02T12:00:00Z").expect("fixed effective_from must parse")
}

fn fixed_daily_config() -> BackupScheduleConfig {
    BackupScheduleConfig::daily("UTC".parse().unwrap(), "09:00".parse().unwrap())
        .expect("fixed daily config must be valid")
}

/// Pin the schedule's activation epoch to the deterministic fixed instant using
/// the canonical trigger-bypass pattern from
/// `scheduled_maintenance_lifecycle_postgres` (session_replication_role).
/// This places `effective_from` strictly before every daily occurrence the
/// packaged binary (real-clock `Timestamp::now_utc()`) will observe, so the
/// real-clock tick finds due backlog via `LATEST_ONLY` instead of correctly
/// rejecting today's occurrence as `BeforeEffectiveFrom`.
async fn pin_fixed_effective_from(
    inspection: &PgPool,
    schedule_id: synveil_core::BackupScheduleId,
    owner: UserId,
) {
    let fixed = fixed_effective_from();
    let mut tx = inspection
        .begin()
        .await
        .expect("pin transaction must begin");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .expect("replica role must be set");
    sqlx::query(
        "UPDATE backup_schedules SET created_at = $1, effective_from = $1, updated_at = $1 WHERE id = $2 AND owner_user_id = $3",
    )
    .bind(fixed.as_offset_datetime())
    .bind(schedule_id.into_uuid())
    .bind(owner.into_uuid())
    .execute(&mut *tx)
    .await
    .expect("pin update must succeed");
    tx.commit().await.expect("pin transaction must commit");
}

/// Resolve the latest canonical daily occurrence at or before `through_utc`
/// using only the pure planner (`next_occurrence_after`). Starting from
/// `effective_from`, walk forward until the next occurrence exceeds `through`.
/// This yields the `LATEST_ONLY` candidate the real-clock tick will select,
/// without fabricating an arbitrary "-2 hours" occurrence.
fn latest_due_on_or_before(
    schedule: &synveil_core::BackupSchedule,
    through_utc: Timestamp,
) -> synveil_core::PlannedScheduleOccurrence {
    let revision = schedule.current_revision();
    let mut cursor = schedule.effective_from();
    let mut latest: Option<synveil_core::PlannedScheduleOccurrence> = None;
    for _ in 0..366 {
        let Some(next) = revision.next_occurrence_after(cursor) else {
            break;
        };
        if next.scheduled_for_utc() > through_utc {
            break;
        }
        latest = Some(next);
        cursor = next.scheduled_for_utc();
    }
    latest.expect("at least one canonical occurrence must be due on or before through")
}

/// Real-clock timestamp truncated to microsecond precision: durable columns
/// are `TIMESTAMPTZ(6)`, and nanosecond values fail mapping with
/// `TimestampPrecisionLoss`.
fn observed_now_micros() -> Timestamp {
    let raw = Timestamp::now_utc().to_string();
    let truncated = if let Some(dot) = raw.find('.') {
        let (head, tail) = raw.split_at(dot + 1);
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        let suffix: String = tail.chars().skip_while(|c| c.is_ascii_digit()).collect();
        let micros = if digits.len() > 6 {
            digits[..6].to_string()
        } else {
            digits
        };
        format!("{head}{micros}{suffix}")
    } else {
        raw
    };
    Timestamp::parse(&truncated).expect("truncated timestamp must parse")
}

async fn seed_backup_set(label: &str) -> Seed {
    let (db_url, pool, inspection) = new_db(label).await;
    // Deterministic fixed seed instant (mirrors lifecycle/oneshot fixtures).
    // The packaged binary itself runs at real-clock now; the schedule's
    // activation epoch is pinned separately to the fixed effective_from so the
    // real-clock tick observes due backlog via LATEST_ONLY.
    let observed_at = fixed_seed_timestamp();
    let owner_user_id = UserId::new();
    let repo = DomainRepository::new(&pool);
    repo.insert_user(&User::new(
        owner_user_id,
        synveil_core::LoginIdentifier::new(
            format!("p79-{label}-{owner_user_id}"),
            format!("p79-key-{owner_user_id}"),
        )
        .unwrap(),
        UserStatus::Active,
        observed_at,
    ))
    .await
    .unwrap();
    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        owner_user_id,
        name(format!("lib-{label}-{library_id}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .unwrap();
    repo.insert_library_with_root(&library, &root)
        .await
        .unwrap();
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
        .unwrap();
    sqlx::query("UPDATE backup_sets SET state='ACTIVE', revision=revision+1, updated_at=clock_timestamp() WHERE id=$1 AND owner_user_id=$2")
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    BackupService::new(pool.clone())
        .configure_snapshot_retention_policy(
            owner_user_id,
            format!("policy-{label}-{backup_set_id}"),
            backup_set_id,
            1,
            86_400,
        )
        .await
        .unwrap();
    Seed {
        db_url,
        owner_user_id,
        backup_set_id,
        pool,
        inspection,
    }
}

/// Run the PACKAGED binary with credential-file delivery only (DATABASE_URL
/// absent: proves the Prompt 77 boundary, fails closed on ambiguity).
fn run_packaged(seed_db_url: &str) -> std::process::Output {
    let bin = packaged_binary();
    let cred_dir = std::env::temp_dir().join(unique_tag("cred"));
    fs::create_dir_all(&cred_dir).expect("credential dir");
    let cred_file = cred_dir.join("database-url");
    fs::write(&cred_file, seed_db_url).expect("credential file");
    // Restrictive mode mirrors the administrator contract (0600).
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(&cred_file, fs::Permissions::from_mode(0o600)).expect("chmod credential");
    let out = Command::new(&bin)
        .env_remove("DATABASE_URL")
        .env_remove("CREDENTIALS_DIRECTORY")
        .env("SYNVEIL_DATABASE_CREDENTIAL_FILE", &cred_file)
        .output()
        .expect("packaged binary must execute");
    let _ = fs::remove_dir_all(&cred_dir);
    out
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn packaged_idle_is_success_without_seed() {
    let (db_url, pool, inspection) = new_db("idle").await;
    let out = run_packaged(&db_url);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("packaged idle stdout: {stdout}");
    assert!(
        out.status.success(),
        "packaged idle run must exit 0 (stderr: {stderr})"
    );
    assert!(
        stdout.contains("tick=Idle"),
        "idle run must report tick=Idle (got: {stdout})"
    );
    assert!(
        stdout.contains("worker=Idle"),
        "idle run must report worker=Idle (got: {stdout})"
    );
    pool.close().await;
    inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn packaged_due_work_advances_exactly_one_bounded_cycle() {
    let seed = seed_backup_set("due").await;
    let svc = BackupScheduleService::new(seed.pool.clone());
    svc.configure_backup_schedule(
        seed.owner_user_id,
        format!("sched-due-{}", seed.backup_set_id),
        seed.backup_set_id,
        fixed_daily_config(),
    )
    .await
    .unwrap();
    // Pin the activation epoch to the deterministic fixed instant. The packaged
    // binary runs at real-clock now, so it observes due backlog via LATEST_ONLY.
    // Without this pin, today's 09:00 occurrence would precede the real-clock
    // effective_from and be correctly rejected as BeforeEffectiveFrom.
    let sched_before = svc
        .get_backup_schedule(seed.owner_user_id, seed.backup_set_id)
        .await
        .unwrap();
    pin_fixed_effective_from(&seed.inspection, sched_before.id(), seed.owner_user_id).await;
    let sched = svc
        .get_backup_schedule(seed.owner_user_id, seed.backup_set_id)
        .await
        .unwrap();
    assert_eq!(sched.effective_from(), fixed_effective_from());
    // Canonical proof that due work exists: the first planned occurrence after
    // the pinned epoch must be strictly before real-clock now.
    let first = sched
        .current_revision()
        .next_occurrence_after(sched.effective_from())
        .expect("first planned occurrence must exist");
    eprintln!(
        "packaged due fixture: effective_from={} first_planned={} ({})",
        sched.effective_from(),
        first.scheduled_for_utc(),
        first.local_calendar_date()
    );
    assert!(
        first.scheduled_for_utc() < observed_now_micros(),
        "pinned fixture must be due at real-clock now"
    );
    let out = run_packaged(&seed.db_url);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("packaged due stdout: {stdout}");
    assert!(
        out.status.success(),
        "packaged due run must exit 0 (stderr: {stderr})"
    );
    assert!(
        stdout.contains("MaterializedAndHandedOff"),
        "due run must hand off exactly once (got: {stdout})"
    );
    assert!(
        stdout.contains("Stepped"),
        "due run must step the worker exactly once (got: {stdout})"
    );
    // Durable proof: exactly one run, CREATED -> SNAPSHOT_CAPTURED, no expiry plan yet.
    let (state, expiry): (String, Option<String>) = sqlx::query_as(
        "SELECT state, expiry_plan_id::text FROM backup_maintenance_runs WHERE backup_set_id=$1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(seed.backup_set_id.into_uuid())
    .fetch_one(&seed.inspection)
    .await
    .expect("due run must create exactly one maintenance run");
    assert_eq!(
        state, "SNAPSHOT_CAPTURED",
        "due cycle must advance CREATED -> SNAPSHOT_CAPTURED"
    );
    assert!(expiry.is_none(), "first cycle must not plan expiry yet");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_maintenance_runs WHERE backup_set_id=$1")
            .bind(seed.backup_set_id.into_uuid())
            .fetch_one(&seed.inspection)
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "exactly one bounded cycle per packaged invocation"
    );
    seed.pool.close().await;
    seed.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn packaged_existing_work_advances_without_new_due() {
    let seed = seed_backup_set("existing").await;
    let svc = BackupScheduleService::new(seed.pool.clone());
    svc.configure_backup_schedule(
        seed.owner_user_id,
        format!("sched-existing-{}", seed.backup_set_id),
        seed.backup_set_id,
        fixed_daily_config(),
    )
    .await
    .unwrap();
    // Pin to the deterministic fixed epoch so canonical
    // effective_from/occurrence validation holds at real-clock now.
    let sched_before = svc
        .get_backup_schedule(seed.owner_user_id, seed.backup_set_id)
        .await
        .unwrap();
    pin_fixed_effective_from(&seed.inspection, sched_before.id(), seed.owner_user_id).await;
    // Materialize + hand off the LATEST canonical due occurrence at the
    // real-clock boundary (pure planner walk, no "-2 hours" fabrication):
    // the packaged tick must then go Idle while the worker advances one step.
    let sched = svc
        .get_backup_schedule(seed.owner_user_id, seed.backup_set_id)
        .await
        .unwrap();
    let observed = observed_now_micros();
    let planned = latest_due_on_or_before(&sched, observed);
    eprintln!(
        "packaged existing fixture: effective_from={} latest_planned={} ({}) observed={}",
        sched.effective_from(),
        planned.scheduled_for_utc(),
        planned.local_calendar_date(),
        observed
    );
    assert!(
        planned.scheduled_for_utc() > sched.effective_from(),
        "latest planned must be after effective_from (else BeforeEffectiveFrom)"
    );
    assert!(
        planned.scheduled_for_utc() <= observed,
        "latest planned must be due at observed"
    );
    let occ = match svc
        .materialize_due_backup_schedule_occurrence(
            seed.owner_user_id,
            sched.id(),
            sched.current_revision_id(),
            planned.local_calendar_date(),
            observed,
        )
        .await
        .unwrap()
    {
        synveil_core::BackupScheduleOccurrenceMaterializationResult::Created(o)
        | synveil_core::BackupScheduleOccurrenceMaterializationResult::Existing(o) => o,
        other => panic!("manual materialization must create occurrence, got {other:?}"),
    };
    let handoff = BackupService::new(seed.pool.clone())
        .handoff_backup_schedule_occurrence(seed.owner_user_id, occ.id(), observed)
        .await
        .unwrap();
    let run_id = handoff.maintenance_run().id();
    let out = run_packaged(&seed.db_url);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("packaged existing stdout: {stdout}");
    assert!(
        out.status.success(),
        "packaged existing-work run must exit 0 (stderr: {stderr})"
    );
    assert!(
        stdout.contains("tick=Idle"),
        "existing-work run must report tick=Idle (got: {stdout})"
    );
    assert!(
        stdout.contains("Stepped"),
        "existing-work run must advance the worker one step (got: {stdout})"
    );
    let run = BackupService::new(seed.pool.clone())
        .get_backup_maintenance_run(seed.owner_user_id, run_id)
        .await
        .unwrap();
    assert_eq!(run.state(), BackupMaintenanceRunState::SnapshotCaptured);
    seed.pool.close().await;
    seed.inspection.close().await;
}
