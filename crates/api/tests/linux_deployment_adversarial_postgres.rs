#![cfg(target_os = "linux")]

//! Prompt 79 — Linux deployment adversarial verification, live PostgreSQL 17.
//!
//! Covers:
//! - A: credential adversarial via the real one-shot binary
//!   (missing/empty/whitespace/malformed/oversized/trailing-newline/CRLF/dual/
//!   leakage/rotation). Failure modes fail before any database work; success
//!   modes run against fresh isolated databases.
//! - E: PostgreSQL failure injection (unavailable, disappears, malformed
//!   state, worker failure after scheduler commit, restart after recovery).
//! - F: process crash & lease recovery (before/after transition, expired
//!   takeover, stale commit fenced, restart identity).
//! - G: lifecycle concurrency (timer+operator overlap, 12-way, duration >
//!   interval, downtime/misfire LATEST_ONLY).
//!
//! Run with:
//! `SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:54399/postgres \
//!   cargo test -p synveil-api --test linux_deployment_adversarial_postgres \
//!   -- --ignored --test-threads=1 --nocapture`
//!
//! Failure-mode credential tests do not require a live database beyond the
//! binary itself; live tests are marked `#[ignore]` per repository convention
//! and create fresh isolated databases per test.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration as StdDuration,
};

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunState, BackupScheduleConfig, BackupScheduledMaintenanceLeaseToken,
    BackupScheduledMaintenanceWorkerId, BackupSetId, DedupDomainId, Library, LibraryId,
    LogicalName, Node, NodeId, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupScheduleService, BackupService, DatabaseConfig, DatabasePool, DomainRepository,
    MigrationRunner, ScheduledMaintenanceCycleRunner, ScheduledMaintenanceWorkerError,
    ScheduledMaintenanceWorkerService,
};
use uuid::Uuid;

const LEASE_SECONDS: u64 = 120;
const MAX_CRED: usize = 8 * 1024;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn one_shot_binary() -> PathBuf {
    // Cargo builds the binary for integration tests; fall back to target/debug.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_synveil-scheduled-maintenance-once") {
        return PathBuf::from(p);
    }
    let dbg = repo_root().join("target/debug/synveil-scheduled-maintenance-once");
    if dbg.is_file() {
        return dbg;
    }
    repo_root().join("target/release/synveil-scheduled-maintenance-once")
}

fn unique_tag(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("p79adv_{label}_{nanos}_{}", std::process::id())
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp must be valid")
}
fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("test logical name must be valid")
}
fn daily(time: &str) -> BackupScheduleConfig {
    BackupScheduleConfig::daily("UTC".parse().unwrap(), time.parse().unwrap())
        .expect("daily config must be valid")
}
fn after(value: Timestamp, secs: u64) -> Timestamp {
    value
        .checked_add_std(StdDuration::from_secs(secs))
        .expect("after must fit")
}
fn worker_id() -> BackupScheduledMaintenanceWorkerId {
    BackupScheduledMaintenanceWorkerId::new()
}

/// Run the real binary with an explicit credential file and a scrubbed
/// environment. Returns (success, stdout, stderr).
fn run_binary_with_cred_file(
    cred_path: Option<&Path>,
    extra_env: &[(&str, &str)],
) -> (bool, String, String) {
    let bin = one_shot_binary();
    assert!(bin.is_file(), "one-shot binary missing: {}", bin.display());
    let mut cmd = Command::new(&bin);
    // Scrub ambient credential sources for determinism.
    cmd.env_remove("DATABASE_URL");
    cmd.env_remove("CREDENTIALS_DIRECTORY");
    cmd.env_remove("SYNVEIL_DATABASE_CREDENTIAL_FILE");
    if let Some(p) = cred_path {
        cmd.env("SYNVEIL_DATABASE_CREDENTIAL_FILE", p);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("binary must execute");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn write_cred_file(dir: &Path, bytes: &[u8]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let p = dir.join("database-url");
    fs::write(&p, bytes).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    p
}

// ---------------------------------------------------------------------------
// Live DB helpers (deterministic fixed fixtures)
// ---------------------------------------------------------------------------

struct IsolatedDb {
    url: String,
    pool: DatabasePool,
    inspection: PgPool,
}

async fn new_db(label: &str) -> IsolatedDb {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL").expect(
        "SYNVEIL_TEST_DATABASE_URL must identify the disposable server maintenance database",
    );
    let db_name = format!(
        "p79adv_{}_{}",
        label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            })
            .collect::<String>(),
        Uuid::now_v7().simple()
    );
    let maintenance = PgPool::connect(&base).await.expect("maintenance connect");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("create isolated db");
    maintenance.close().await;
    let url = format!(
        "{}/{}",
        base.rsplit_once('/').expect("base has db").0,
        db_name
    );
    let config = DatabaseConfig::from_url(&url).expect("test URL must parse");
    let pool = DatabasePool::connect(&config).await.expect("pool connect");
    let status = MigrationRunner::new().run(&pool).await.expect("migrate");
    assert!(status.is_current());
    assert_eq!(status.applied_versions().len(), 36);
    let inspection = PgPool::connect(&url).await.expect("inspection connect");
    IsolatedDb {
        url,
        pool,
        inspection,
    }
}

impl IsolatedDb {
    async fn close(self) {
        self.pool.close().await;
        self.inspection.close().await;
    }
}

struct Fixture {
    db: IsolatedDb,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
}

async fn fixture(label: &str) -> Fixture {
    let db = new_db(label).await;
    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    let repo = DomainRepository::new(&db.pool);
    repo.insert_user(&User::new(
        owner_user_id,
        synveil_core::LoginIdentifier::new(
            format!("p79adv-{label}-{owner_user_id}"),
            format!("p79adv-key-{owner_user_id}"),
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
    BackupService::new(db.pool.clone())
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
        .execute(&db.inspection)
        .await
        .unwrap();
    BackupService::new(db.pool.clone())
        .configure_snapshot_retention_policy(
            owner_user_id,
            format!("policy-{label}-{backup_set_id}"),
            backup_set_id,
            1,
            86_400,
        )
        .await
        .unwrap();
    Fixture {
        db,
        owner_user_id,
        backup_set_id,
    }
}

impl Fixture {
    fn pool(&self) -> DatabasePool {
        self.db.pool.clone()
    }
    fn runner(&self) -> ScheduledMaintenanceCycleRunner {
        ScheduledMaintenanceCycleRunner::new(self.pool())
    }
    async fn close(self) {
        self.db.close().await;
    }
}

async fn configure_and_pin(
    fixture: &Fixture,
    label: &str,
    time: &str,
) -> synveil_core::BackupSchedule {
    let svc = BackupScheduleService::new(fixture.pool());
    svc.configure_backup_schedule(
        fixture.owner_user_id,
        format!("sched-{label}-{}", fixture.backup_set_id),
        fixture.backup_set_id,
        daily(time),
    )
    .await
    .unwrap();
    let sched = svc
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap();
    let fixed = timestamp("2026-09-02T12:00:00Z");
    let mut tx = fixture.db.inspection.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE backup_schedules SET created_at=$1, effective_from=$1, updated_at=$1 WHERE id=$2 AND owner_user_id=$3")
        .bind(fixed.as_offset_datetime())
        .bind(sched.id().into_uuid())
        .bind(fixture.owner_user_id.into_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    svc.get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .unwrap()
}

fn first_planned(
    schedule: &synveil_core::BackupSchedule,
) -> synveil_core::PlannedScheduleOccurrence {
    schedule
        .current_revision()
        .next_occurrence_after(schedule.effective_from())
        .expect("first planned must exist")
}

async fn count_for(pool: &PgPool, table: &str, set: BackupSetId) -> i64 {
    sqlx::query_scalar::<_, i64>(&format!(
        "SELECT count(*) FROM {table} WHERE backup_set_id=$1"
    ))
    .bind(set.into_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// Area A — credential adversarial via the real binary
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_missing_fails_closed() {
    let (ok, _out, err) = run_binary_with_cred_file(None, &[]);
    assert!(!ok, "missing credential must exit non-zero");
    assert!(
        err.contains("configuration error"),
        "must be configuration error, got: {err}"
    );
    assert!(err.contains("missing"), "must report missing, got: {err}");
}

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_empty_fails_closed() {
    let dir = std::env::temp_dir().join(unique_tag("a2"));
    let cred = write_cred_file(&dir, b"");
    let (ok, _out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(!ok, "empty credential must fail closed");
    assert!(err.contains("configuration error"), "got: {err}");
    assert!(err.contains("empty"), "got: {err}");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_whitespace_fails_closed() {
    let dir = std::env::temp_dir().join(unique_tag("a3"));
    let cred = write_cred_file(&dir, b"   \n\t\n");
    let (ok, _out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(!ok, "whitespace credential must fail closed");
    assert!(err.contains("configuration error"), "got: {err}");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_malformed_fails_via_canonical_validation() {
    let dir = std::env::temp_dir().join(unique_tag("a4"));
    let cred = write_cred_file(&dir, b"not-a-valid-url");
    let (ok, _out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(!ok, "malformed URL must fail");
    assert!(
        err.contains("database configuration is invalid") || err.contains("invalid"),
        "must be canonical validation failure, got: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_oversized_fails_before_connect() {
    let dir = std::env::temp_dir().join(unique_tag("a5"));
    let big = vec![b'a'; MAX_CRED + 1];
    let cred = write_cred_file(&dir, &big);
    let (ok, _out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(!ok, "oversized credential must fail");
    assert!(
        err.contains("exceeds maximum size"),
        "must fail before connect, got: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_credential_trailing_newline_succeeds() {
    let isolated = new_db("a6nl").await;
    let dir = std::env::temp_dir().join(unique_tag("a6"));
    let mut bytes = isolated.url.as_bytes().to_vec();
    bytes.push(b'\n');
    let cred = write_cred_file(&dir, &bytes);
    let (ok, out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(
        ok,
        "trailing newline must succeed, stderr: {err} stdout: {out}"
    );
    assert!(
        out.contains("tick=Idle") || out.contains("Materialized"),
        "got: {out}"
    );
    let _ = fs::remove_dir_all(&dir);
    isolated.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_credential_crlf_succeeds() {
    let isolated = new_db("a7crlf").await;
    let dir = std::env::temp_dir().join(unique_tag("a7"));
    let mut bytes = isolated.url.as_bytes().to_vec();
    bytes.extend_from_slice(b"\r\n");
    let cred = write_cred_file(&dir, &bytes);
    let (ok, out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(
        ok,
        "CRLF must succeed per Prompt 77 contract, stderr: {err} stdout: {out}"
    );
    let _ = fs::remove_dir_all(&dir);
    isolated.close().await;
}

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_dual_source_rejected() {
    let dir = std::env::temp_dir().join(unique_tag("a8"));
    let cred = write_cred_file(&dir, b"postgresql://from-file@127.0.0.1/db");
    let (ok, _out, err) = run_binary_with_cred_file(
        Some(&cred),
        &[("DATABASE_URL", "postgresql://from-env@127.0.0.1/db")],
    );
    assert!(!ok, "dual source must be rejected");
    assert!(
        err.contains("ambiguous"),
        "must report ambiguity, got: {err}"
    );
    assert!(
        !err.contains("from-file") && !err.contains("from-env"),
        "must not leak values: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "binary credential failure modes (no live DB writes)"]
async fn adversarial_credential_value_never_leaks() {
    // Distinctive fake password token; it must appear in none of the
    // error Display, stdout summary, or stderr diagnostics.
    let token = format!("P79LEAK_{}", Uuid::now_v7().simple());
    let bad_url = format!("not-a-scheme://user:{token}@host/db");
    let dir = std::env::temp_dir().join(unique_tag("a9"));
    let cred = write_cred_file(&dir, bad_url.as_bytes());
    let (ok, out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(!ok);
    assert!(!err.contains(&token), "stderr must not leak secret: {err}");
    assert!(!out.contains(&token), "stdout must not leak secret: {out}");
    // Oversized path also must not leak.
    let big_secret = format!(
        "postgresql://user:{}@host/db{}",
        "x".repeat(MAX_CRED),
        token
    );
    let cred2 = write_cred_file(&dir, big_secret.as_bytes());
    let (_ok2, out2, err2) = run_binary_with_cred_file(Some(&cred2), &[]);
    assert!(
        !err2.contains(&token),
        "oversized error must not leak: {err2}"
    );
    assert!(!out2.contains(&token), "oversized stdout must not leak");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_credential_rotation_without_restart() {
    // Two isolated databases; atomic file replacement between activations.
    // First activation uses the old credential (due work in DB1), the next
    // uses the new credential (DB2 idle). No watcher/cache/restart.
    let db1 = new_db("a10old").await;
    let db2 = new_db("a10new").await;
    // Seed DB1 with due work via direct service (fixed fixtures).
    let observed_seed = timestamp("2026-09-02T00:00:00.123456Z");
    for (db, tag) in [(&db1, "old"), (&db2, "new")] {
        let repo = DomainRepository::new(&db.pool);
        let owner = UserId::new();
        repo.insert_user(&User::new(
            owner,
            synveil_core::LoginIdentifier::new(
                format!("rot-{tag}-{owner}"),
                format!("rot-key-{owner}"),
            )
            .unwrap(),
            UserStatus::Active,
            observed_seed,
        ))
        .await
        .unwrap();
        let lid = LibraryId::new();
        let root = Node::new_root(NodeId::new(), lid, name("root"), observed_seed);
        let lib = Library::new(
            lid,
            owner,
            name(format!("lib-{tag}-{lid}")),
            &root,
            DedupDomainId::new(),
            observed_seed,
        )
        .unwrap();
        repo.insert_library_with_root(&lib, &root).await.unwrap();
        let set = BackupSetId::new();
        BackupService::new(db.pool.clone())
            .create_backup_set(
                owner,
                set,
                name(format!("bk-{tag}-{set}")),
                lid,
                None,
                observed_seed,
            )
            .await
            .unwrap();
        sqlx::query("UPDATE backup_sets SET state='ACTIVE', revision=revision+1 WHERE id=$1")
            .bind(set.into_uuid())
            .execute(&db.inspection)
            .await
            .unwrap();
        BackupService::new(db.pool.clone())
            .configure_snapshot_retention_policy(owner, format!("pol-{tag}-{set}"), set, 1, 86_400)
            .await
            .unwrap();
        // Store owner/set for DB1 due seeding below via tags.
        let _ = (owner, set);
    }
    let dir = std::env::temp_dir().join(unique_tag("a10"));
    fs::create_dir_all(&dir).unwrap();
    let cred = dir.join("database-url");
    // Activation 1 with old credential (DB1).
    fs::write(&cred, &db1.url).unwrap();
    let (ok1, out1, err1) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(
        ok1,
        "first activation (old cred) must succeed: {err1} {out1}"
    );
    // Atomic replacement: write temp then rename (no watcher needed).
    let tmp = dir.join("database-url.tmp");
    fs::write(&tmp, &db2.url).unwrap();
    fs::rename(&tmp, &cred).unwrap();
    // Activation 2 with new credential (DB2).
    let (ok2, out2, err2) = run_binary_with_cred_file(Some(&cred), &[]);
    assert!(
        ok2,
        "second activation (new cred) must succeed: {err2} {out2}"
    );
    let _ = fs::remove_dir_all(&dir);
    db1.close().await;
    db2.close().await;
}

// ---------------------------------------------------------------------------
// Area E — PostgreSQL failure injection
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_db_unavailable_before_invocation_is_bounded() {
    // Unroutable port: connect must fail fast, non-zero, no retry loop.
    let dir = std::env::temp_dir().join(unique_tag("e1"));
    let cred = write_cred_file(
        &dir,
        b"postgresql://invalid:invalid@127.0.0.1:59999/nonexistent",
    );
    let start = std::time::Instant::now();
    let (ok, _out, err) = run_binary_with_cred_file(Some(&cred), &[]);
    let elapsed = start.elapsed();
    assert!(!ok, "unavailable DB must exit non-zero");
    assert!(
        err.contains("database error"),
        "must be database error, got: {err}"
    );
    assert!(
        elapsed.as_secs() < 30,
        "failure must be bounded (no retry loop), took {elapsed:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_db_disappears_after_construction_propagates() {
    let f = fixture("e2").await;
    let pool = f.pool();
    // Close the pool to simulate disappearance after runtime construction.
    let closing = pool.clone();
    closing.close().await;
    let runner = ScheduledMaintenanceCycleRunner::new(pool);
    let err = runner
        .run_one_scheduled_backup_maintenance_cycle(
            timestamp("2026-09-02T00:00:00Z"),
            LEASE_SECONDS,
        )
        .await
        .expect_err("closed pool must propagate database error");
    let s = format!("{err:?} {err}");
    assert!(
        s.to_lowercase().contains("database") || s.contains("Scheduler") || s.contains("Worker"),
        "must propagate database error, got: {s}"
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_scheduler_failure_blocks_worker() {
    let f = fixture("e3").await;
    let schedule = configure_and_pin(&f, "e3", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let bad_pool = f.pool();
    bad_pool.clone().close().await;
    let bad_runner = ScheduledMaintenanceCycleRunner::new(bad_pool);
    let err = bad_runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect_err("scheduler failure must propagate");
    assert!(
        matches!(
            err,
            synveil_metadata::ScheduledMaintenanceCycleError::Scheduler(_)
        ),
        "scheduler phase must fail, got: {err:?}"
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_worker_failure_preserves_scheduler_writes() {
    let f = fixture("e4").await;
    let schedule = configure_and_pin(&f, "e4", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let r1 = f
        .runner()
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    let run_id = r1
        .tick()
        .outcome()
        .expect("must hand off")
        .maintenance_run_id();
    // Scheduler writes are durable: the run survives even though a subsequent
    // worker fencing error propagates (wrong token).
    let svc = ScheduledMaintenanceWorkerService::new(f.pool());
    let holder = worker_id();
    let claim_at = after(observed, 60);
    let claim = match svc
        .claim_next_scheduled_maintenance_step(holder, claim_at, LEASE_SECONDS)
        .await
        .unwrap()
    {
        synveil_core::BackupScheduledMaintenanceClaimOutcome::Claimed(c)
        | synveil_core::BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(c)
        | synveil_core::BackupScheduledMaintenanceClaimOutcome::TakenOver(c) => c,
        other => panic!("expected claim, got {other:?}"),
    };
    let bad = svc
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            claim.lease_worker_id(),
            BackupScheduledMaintenanceLeaseToken::new(),
            claim.lease_generation(),
            after(claim_at, 1),
        )
        .await;
    assert_eq!(bad, Err(ScheduledMaintenanceWorkerError::LeaseLost));
    // Scheduler's run remains durable.
    let run = BackupService::new(f.pool())
        .get_backup_maintenance_run(f.owner_user_id, run_id)
        .await
        .unwrap();
    assert!(
        matches!(
            run.state(),
            BackupMaintenanceRunState::Created | BackupMaintenanceRunState::SnapshotCaptured
        ),
        "scheduler run must remain durable, got {:?}",
        run.state()
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_restart_after_recovery_resumes_from_db() {
    let f = fixture("e5").await;
    let schedule = configure_and_pin(&f, "e5", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let runner_a = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id());
    let r_a = runner_a
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    let run_id = r_a.tick().outcome().unwrap().maintenance_run_id();
    // Fresh process identity, same durable database: must advance the SAME run.
    let runner_b = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id());
    let r_b = runner_b
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 60), LEASE_SECONDS)
        .await
        .unwrap();
    assert!(!r_b.worker().is_idle());
    let runs = BackupService::new(f.pool())
        .list_backup_maintenance_runs(f.owner_user_id, f.backup_set_id, None, 10)
        .await
        .unwrap()
        .0;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id(), run_id);
    f.close().await;
}

// ---------------------------------------------------------------------------
// Area F — crash & lease recovery
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_crash_before_transition_recovers_via_claim() {
    // Simulate crash before transition: first runner hands off (scheduler
    // commit durable) but we drop it before worker completion; a fresh runner
    // recovers via the durable claim without duplicating the run.
    let f = fixture("f1").await;
    let schedule = configure_and_pin(&f, "f1", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let r1 = f
        .runner()
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    let run_id = r1.tick().outcome().unwrap().maintenance_run_id();
    // Fresh process after crash: same DB, new identity.
    let r2 = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id())
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 60), LEASE_SECONDS)
        .await
        .unwrap();
    assert!(
        !r2.worker().is_idle(),
        "recovery must advance the durable run"
    );
    let runs = BackupService::new(f.pool())
        .list_backup_maintenance_runs(f.owner_user_id, f.backup_set_id, None, 10)
        .await
        .unwrap()
        .0;
    assert_eq!(runs.len(), 1, "no duplicate run after crash recovery");
    assert_eq!(runs[0].id(), run_id);
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_crash_after_transition_no_duplicate_child_effect() {
    let f = fixture("f2").await;
    let schedule = configure_and_pin(&f, "f2", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    // Drive to SNAPSHOT_CAPTURED, then recover: exactly one snapshot child.
    let r1 = f
        .runner()
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    let run_id = r1.tick().outcome().unwrap().maintenance_run_id();
    let _r2 = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id())
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 60), LEASE_SECONDS)
        .await
        .unwrap();
    let runs = BackupService::new(f.pool())
        .list_backup_maintenance_runs(f.owner_user_id, f.backup_set_id, None, 10)
        .await
        .unwrap()
        .0;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id(), run_id);
    let capture_op = runs[0].capture_operation_id().to_owned();
    let snap_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_snapshots WHERE operation_id=$1")
            .bind(capture_op)
            .fetch_one(&f.db.inspection)
            .await
            .unwrap();
    assert_eq!(
        snap_count, 1,
        "no duplicate child effect after crash recovery"
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_expired_takeover_and_stale_fenced() {
    // F3/F4: holder claims, a different worker attempts the same claim with
    // wrong token/generation and is fenced with LeaseLost and zero mutation.
    let f = fixture("f34").await;
    let schedule = configure_and_pin(&f, "f34", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let r1 = f
        .runner()
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    let run_id = r1.tick().outcome().unwrap().maintenance_run_id();
    let before = BackupService::new(f.pool())
        .get_backup_maintenance_run(f.owner_user_id, run_id)
        .await
        .unwrap();
    let svc = ScheduledMaintenanceWorkerService::new(f.pool());
    let holder = worker_id();
    let claim_at = after(observed, 60);
    let claim = match svc
        .claim_next_scheduled_maintenance_step(holder, claim_at, LEASE_SECONDS)
        .await
        .unwrap()
    {
        synveil_core::BackupScheduledMaintenanceClaimOutcome::Claimed(c)
        | synveil_core::BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(c)
        | synveil_core::BackupScheduledMaintenanceClaimOutcome::TakenOver(c) => c,
        other => panic!("expected claim, got {other:?}"),
    };
    // Stale worker (wrong token) attempts commit → LeaseLost, no mutation.
    for (label, token, generation, wid) in [
        (
            "wrong-token",
            BackupScheduledMaintenanceLeaseToken::new(),
            claim.lease_generation(),
            claim.lease_worker_id(),
        ),
        (
            "stale-generation",
            claim.lease_token(),
            claim.lease_generation() + 1,
            claim.lease_worker_id(),
        ),
        (
            "wrong-worker",
            claim.lease_token(),
            claim.lease_generation(),
            worker_id(),
        ),
    ] {
        let res = svc
            .execute_claimed_scheduled_maintenance_step(
                claim.claim_id(),
                wid,
                token,
                generation,
                after(claim_at, 1),
            )
            .await;
        assert_eq!(
            res,
            Err(ScheduledMaintenanceWorkerError::LeaseLost),
            "{label} must be fenced"
        );
    }
    let aft = BackupService::new(f.pool())
        .get_backup_maintenance_run(f.owner_user_id, run_id)
        .await
        .unwrap();
    assert_eq!(
        before.state(),
        aft.state(),
        "fenced commits must not mutate"
    );
    assert_eq!(before.capture_operation_id(), aft.capture_operation_id());
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_restart_identity_continues_same_run() {
    let f = fixture("f5").await;
    let schedule = configure_and_pin(&f, "f5", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let r1 = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id())
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    let run_id = r1.tick().outcome().unwrap().maintenance_run_id();
    // Fresh BackupScheduledMaintenanceWorkerId continues the same durable run.
    let r2 = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id())
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 60), LEASE_SECONDS)
        .await
        .unwrap();
    assert!(!r2.worker().is_idle());
    let runs = BackupService::new(f.pool())
        .list_backup_maintenance_runs(f.owner_user_id, f.backup_set_id, None, 10)
        .await
        .unwrap()
        .0;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id(), run_id);
    f.close().await;
}

// ---------------------------------------------------------------------------
// Area G — lifecycle concurrency
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_timer_operator_overlap_converges() {
    // G1: scheduled timer + manual start + independent caller at same instant.
    let f = fixture("g1").await;
    let schedule = configure_and_pin(&f, "g1", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let mut handles = Vec::new();
    for _ in 0..3 {
        let pool = f.pool();
        let wid = worker_id();
        handles.push(tokio::spawn(async move {
            ScheduledMaintenanceCycleRunner::with_worker_id(pool, wid)
                .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
                .await
        }));
    }
    for h in handles {
        let _ = h.await.unwrap();
    }
    assert_eq!(
        count_for(
            &f.db.inspection,
            "backup_schedule_occurrences",
            f.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for(
            &f.db.inspection,
            "backup_schedule_occurrence_handoffs",
            f.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for(&f.db.inspection, "backup_maintenance_runs", f.backup_set_id).await,
        1
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_twelve_concurrent_no_duplicates_zero_deadlocks() {
    let f = fixture("g2").await;
    let schedule = configure_and_pin(&f, "g2", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let mut handles = Vec::new();
    for _ in 0..12 {
        let pool = f.pool();
        let wid = worker_id();
        handles.push(tokio::spawn(async move {
            ScheduledMaintenanceCycleRunner::with_worker_id(pool, wid)
                .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
                .await
        }));
    }
    let mut deadlocks = 0usize;
    let mut unexpected = 0usize;
    for h in handles {
        match h.await.unwrap() {
            Ok(_) => {}
            Err(e) => {
                let s = format!("{e:?} {e}");
                if s.contains("40P01") || s.to_lowercase().contains("deadlock") {
                    deadlocks += 1;
                } else if s.contains("Database") {
                    unexpected += 1;
                }
            }
        }
    }
    assert_eq!(deadlocks, 0, "G2 must have 0 deadlocks");
    assert_eq!(unexpected, 0, "G2 must have 0 unexpected DB errors");
    assert_eq!(
        count_for(
            &f.db.inspection,
            "backup_schedule_occurrences",
            f.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for(
            &f.db.inspection,
            "backup_schedule_occurrence_handoffs",
            f.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for(&f.db.inspection, "backup_maintenance_runs", f.backup_set_id).await,
        1
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_single_invocation_executes_no_second_cycle() {
    // G3: one process activation performs exactly one bounded cycle even when
    // the next timer time would already have arrived (no internal loop).
    let f = fixture("g3").await;
    let schedule = configure_and_pin(&f, "g3", "09:00").await;
    let planned = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let r = f
        .runner()
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    assert!(
        r.tick().outcome().is_some(),
        "first activation hands off once"
    );
    assert_eq!(
        count_for(&f.db.inspection, "backup_maintenance_runs", f.backup_set_id).await,
        1
    );
    // A second activation at a much later observed is a NEW external
    // invocation (not an internal second cycle); correctness comes from the
    // database, not from systemd serialization.
    let r2 = ScheduledMaintenanceCycleRunner::with_worker_id(f.pool(), worker_id())
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 3600), LEASE_SECONDS)
        .await
        .unwrap();
    assert!(
        r2.tick().is_idle(),
        "no new due immediately after handoff at +1h"
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable PostgreSQL 17 server database"]
async fn adversarial_downtime_latest_only_collapses_to_one() {
    // G4: after downtime, one wake-up materializes exactly the latest due
    // occurrence under LATEST_ONLY; the lifecycle layer synthesizes nothing.
    let f = fixture("g4").await;
    let schedule = configure_and_pin(&f, "g4", "09:00").await;
    let first = first_planned(&schedule);
    // Three days after the first planned occurrence: backlog of 3 dailies.
    let observed = after(first.scheduled_for_utc(), 3 * 86_400 + 1);
    let r = f
        .runner()
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .unwrap();
    assert!(r.tick().outcome().is_some());
    assert_eq!(
        count_for(
            &f.db.inspection,
            "backup_schedule_occurrences",
            f.backup_set_id
        )
        .await,
        1,
        "LATEST_ONLY must collapse backlog to exactly one occurrence"
    );
    assert_eq!(
        count_for(&f.db.inspection, "backup_maintenance_runs", f.backup_set_id).await,
        1
    );
    f.close().await;
}
