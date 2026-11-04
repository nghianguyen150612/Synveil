//! Isolated SQLite state qualification. No production migrations or credentials.
#[path = "support/p026.rs"]
mod fixtures;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use synveil_auth::authorization_reference::{rbac_v2::*, *};

async fn database() -> (SqlitePool, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("p026-{}.sqlite", uuid::Uuid::now_v7()));
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("support/p026.sql"))
        .execute(&pool)
        .await
        .unwrap();
    (pool, path)
}
// Drain every permit before closing: SQLx 0.8.6 can still have asynchronous
// return-to-pool work when Pool::close reports completion. Holding all permits
// fences that work; explicit connection close awaits each SQLite worker.
async fn shutdown(pool: &SqlitePool) {
    let mut connections = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        connections.push(pool.acquire().await.expect("drain fixture connection"));
    }
    let (busy, _, _): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut **connections.first_mut().unwrap())
        .await
        .unwrap();
    assert_eq!(
        busy, 0,
        "fixture checkpoint must have no active readers/writers"
    );
    for connection in connections {
        connection
            .close()
            .await
            .expect("close fixture SQLite worker");
    }
    pool.close().await;
}
async fn close(pool: SqlitePool, path: std::path::PathBuf) {
    shutdown(&pool).await;
    // The last SQLite worker closes the WAL/SHM handles and removes sidecars.
    // Do not unlink a live database just because Unix permits it.
    for suffix in ["-wal", "-shm"] {
        let sidecar = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
        assert!(
            !sidecar.exists(),
            "SQLite sidecar still live: {}",
            sidecar.display()
        );
    }
    std::fs::remove_file(&path)
        .unwrap_or_else(|error| panic!("remove closed fixture {}: {error}", path.display()));
}
async fn state(pool: &SqlitePool, id: &str) -> i64 {
    sqlx::query_scalar("SELECT revoked FROM grants WHERE id=?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn revoke(pool: &SqlitePool, id: &str) {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE grants SET revoked=1 WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO audit(event) VALUES ('revoke')")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
async fn execute(pool: &SqlitePool, require_approval: bool) -> Decision {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let (mut c, mut p) = fixtures::fixture();
    let mut child = p.records[0].clone();
    child.id = "child".into();
    child.parent = Some("root".into());
    child.grantor = fixtures::browser("alice");
    child.recipient = fixtures::browser("bob");
    p.records.push(child);
    c.principal = fixtures::browser("bob");
    for r in &mut p.records {
        let row = sqlx::query("SELECT revoked,revision FROM grants WHERE id=?")
            .bind(&r.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        r.revoked = row.get::<i64, _>(0) != 0;
        r.revision = PolicyRevision(row.get::<i64, _>(1) as u64);
    }
    if require_approval {
        p.approval_actions.push(Action::Read);
        p.approvers
            .push((fixtures::browser("carol"), p.ownership.clone()));
        let row = sqlx::query("SELECT consumed,withdrawn FROM approvals WHERE id='approval'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        p.approvals.push(Approval {
            id: "approval".into(),
            operation: "operation".into(),
            requester: c.principal.clone(),
            approver: fixtures::browser("carol"),
            scope: p.ownership.clone(),
            revision: p.revision,
            issued: 1,
            expires: 100,
            withdrawn: row.get::<i64, _>(1) != 0,
            consumed: row.get::<i64, _>(0) != 0,
        });
    }
    let decision = qualify(&c, &p, "child", "operation", 10, true);
    if decision == Decision::Allowed {
        if require_approval {
            assert_eq!(sqlx::query("UPDATE approvals SET consumed=1 WHERE id='approval' AND consumed=0 AND withdrawn=0").execute(&mut *tx).await.unwrap().rows_affected(),1);
        }
        sqlx::query("INSERT INTO audit(event) VALUES ('execute')")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    decision
}
#[tokio::test]
async fn atomic_state_rollback_visibility_and_restart() {
    let (pool, path) = database().await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("INSERT INTO grants VALUES ('uncommitted',0,7)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM grants WHERE id='uncommitted'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    tx.rollback().await.unwrap();
    assert!(
        sqlx::query("INSERT INTO grants VALUES ('root',0,7)")
            .execute(&pool)
            .await
            .is_err()
    );
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE grants SET revoked=1 WHERE id='root'")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(state(&pool, "root").await, 0);
    tx.rollback().await.unwrap();
    assert_eq!(state(&pool, "root").await, 0);
    revoke(&pool, "root").await;
    assert_eq!(state(&pool, "root").await, 1);
    assert_eq!(execute(&pool, false).await, Decision::StaleOrRevoked);
    assert!(
        sqlx::query("UPDATE grants SET revoked=0 WHERE id='root'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("INSERT OR REPLACE INTO grants VALUES ('root',0,7)")
            .execute(&pool)
            .await
            .is_err()
    );
    // Drop an uncommitted connection: process-loss equivalent at transaction boundary.
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO audit(event) VALUES ('crash')")
            .execute(&mut *conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
    }
    shutdown(&pool).await;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&path);
    let reopened = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    assert_eq!(state(&reopened, "root").await, 1);
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM audit")
        .fetch_all(&reopened)
        .await
        .unwrap();
    assert_eq!(events, vec!["revoke"]);
    assert_eq!(execute(&reopened, false).await, Decision::StaleOrRevoked);
    close(reopened, path).await;
}
#[tokio::test]
async fn concurrent_authorization_revocation_and_stale_snapshot() {
    let (pool, path) = database().await;
    // Two independent connections contend for the same write fence. Either serial
    // order is legal; no post-revocation execution is allowed.
    let (decision, ()) = tokio::join!(execute(&pool, false), revoke(&pool, "root"));
    assert!(matches!(
        decision,
        Decision::Allowed | Decision::StaleOrRevoked
    ));
    assert_eq!(execute(&pool, false).await, Decision::StaleOrRevoked);
    assert_eq!(state(&pool, "child").await, 0); // parent tombstone alone propagates
    close(pool, path).await;
    let (pool, path) = database().await;
    sqlx::query("UPDATE grants SET revision=6 WHERE id='root'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(execute(&pool, false).await, Decision::StaleOrRevoked);
    close(pool, path).await;
}
#[tokio::test]
async fn approval_consumption_race_lost_response_and_withdrawal() {
    let (pool, path) = database().await;
    let (a, b) = tokio::join!(execute(&pool, true), execute(&pool, true));
    assert_eq!(
        [a, b].iter().filter(|d| **d == Decision::Allowed).count(),
        1
    );
    assert_eq!(
        [a, b]
            .iter()
            .filter(|d| **d == Decision::RequiresApproval)
            .count(),
        1
    );
    // A lost response cannot restore approval; reopening preserves consumption.
    shutdown(&pool).await;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&path);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    assert_eq!(execute(&pool, true).await, Decision::RequiresApproval);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit WHERE event='execute'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    close(pool, path).await;
    let (pool, path) = database().await;
    sqlx::query("UPDATE approvals SET withdrawn=1")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(execute(&pool, true).await, Decision::RequiresApproval);
    close(pool, path).await;
}

#[tokio::test]
async fn atomic_grant_creation_contends_with_revocation() {
    let (pool, path) = database().await;
    let create = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let revoked: i64 = sqlx::query_scalar("SELECT revoked FROM grants WHERE id='root'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        if revoked != 0 {
            tx.rollback().await.unwrap();
            return Decision::StaleOrRevoked;
        }
        sqlx::query("INSERT INTO grants VALUES ('new-child',0,7)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO audit(event) VALUES ('create')")
            .execute(&mut *tx)
            .await
            .unwrap();
        let invisible: i64 = sqlx::query_scalar("SELECT count(*) FROM grants WHERE id='new-child'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(invisible, 0);
        tx.commit().await.unwrap();
        Decision::Allowed
    };
    let (d, ()) = tokio::join!(create, revoke(&pool, "root"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM grants WHERE id='new-child'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, if d == Decision::Allowed { 1 } else { 0 });
    assert!(matches!(d, Decision::Allowed | Decision::StaleOrRevoked));
    assert_eq!(state(&pool, "root").await, 1);
    assert_eq!(execute(&pool, false).await, Decision::StaleOrRevoked);
    close(pool, path).await;
}

#[tokio::test]
async fn process_crash_recovers_committed_tombstone_and_discards_uncommitted_audit() {
    const CHILD_DATABASE: &str = "SYNVEIL_P026_CRASH_FIXTURE";
    if let Some(path) = std::env::var_os(CHILD_DATABASE) {
        let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        sqlx::query("INSERT INTO audit(event) VALUES ('uncommitted-crash')")
            .execute(&mut *tx)
            .await
            .unwrap();
        // No connection/transaction destructors run. OS closes the file handles.
        std::process::exit(26);
    }
    let (pool, path) = database().await;
    revoke(&pool, "root").await;
    shutdown(&pool).await;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process_crash_recovers_committed_tombstone_and_discards_uncommitted_audit",
            "--nocapture",
        ])
        .env(CHILD_DATABASE, &path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(26));
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&path);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    assert_eq!(state(&pool, "root").await, 1);
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM audit")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, vec!["revoke"]);
    assert_eq!(execute(&pool, false).await, Decision::StaleOrRevoked);
    close(pool, path).await;
}
