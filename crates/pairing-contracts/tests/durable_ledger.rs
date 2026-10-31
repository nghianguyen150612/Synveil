mod support;
use sqlx::{Connection, SqliteConnection};
use std::{path::PathBuf, sync::Arc};
use support::*;
use synveil_pairing_contracts::{ENROLLMENT_AUDIENCE, PairingQrPayload};

struct Fixture(PathBuf);
impl Fixture {
    async fn new() -> Self {
        let f = Self(
            std::env::temp_dir().join(format!("synveil-p021-{}.sqlite", uuid::Uuid::now_v7())),
        );
        let mut db = f.open().await;
        initialize(&mut db).await.unwrap();
        let qr = qr();
        sqlx::query("INSERT INTO authority VALUES (?,?,?,?,?,1)")
            .bind(&qr.installation_id)
            .bind(&qr.issuer_owner_id)
            .bind(&qr.approved_endpoint)
            .bind(&qr.server_identity_fingerprint)
            .bind(7_i64)
            .execute(&mut db)
            .await
            .unwrap();
        create(&mut db, &qr).await.unwrap();
        sqlx::query("INSERT INTO approvals SELECT id,binding FROM invitations")
            .execute(&mut db)
            .await
            .unwrap();
        db.close().await.unwrap();
        f
    }
    async fn open(&self) -> SqliteConnection {
        connect(&self.0).await.unwrap()
    }
    async fn issued(&self) {
        let mut db = self.open().await;
        transition(&mut db, "inv", "install", || 101, false)
            .await
            .unwrap();
        db.close().await.unwrap();
    }
    async fn check(&self, state: &str, verifiers: i64) {
        let mut db = self.open().await;
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT state FROM invitations")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            state
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM synthetic_verifiers")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            verifiers
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            "ok"
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut db)
                .await
                .unwrap()
                .is_empty()
        );
        db.close().await.unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn qr() -> PairingQrPayload {
    PairingQrPayload {
        contract_version: 1,
        invitation_id: "inv".into(),
        installation_id: "install".into(),
        issuer_owner_id: "owner".into(),
        audience: ENROLLMENT_AUDIENCE.into(),
        approved_endpoint: "https://reference.example.test".into(),
        server_identity_fingerprint: format!("sha256:{}", "a".repeat(64)),
        target_device_id: "device".into(),
        issued_at_unix: 100,
        expires_at_unix: 200,
        policy_revision: 7,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_connections_commit_exactly_one_and_replay_after_restart() {
    let f = Fixture::new().await;
    f.issued().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let mut db = f.open().await;
        let b = barrier.clone();
        tasks.push(tokio::spawn(async move {
            b.wait().await;
            let r = redeem(&mut db, "inv", "install", "device", || 150)
                .await
                .unwrap();
            db.close().await.unwrap();
            r
        }));
    }
    let mut wins = 0;
    for t in tasks {
        if t.await.unwrap() == Outcome::Consumed {
            wins += 1;
        }
    }
    assert_eq!(wins, 1);
    f.check("CONSUMED", 1).await;
    let mut db = f.open().await;
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 150)
            .await
            .unwrap(),
        Outcome::Denied
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn transitions_expiry_and_terminal_integrity() {
    for (cancel, now, state) in [(false, 200, "EXPIRED"), (true, 150, "CANCELLED")] {
        let f = Fixture::new().await;
        let mut db = f.open().await;
        assert_eq!(
            redeem(&mut db, "inv", "install", "device", || 150)
                .await
                .unwrap(),
            Outcome::Denied
        );
        transition(&mut db, "inv", "install", || 99, false)
            .await
            .unwrap();
        f.check("CREATED", 0).await;
        transition(&mut db, "inv", "install", || now, cancel)
            .await
            .unwrap();
        for next in ["CREATED", "ISSUED", "CONSUMED", "CANCELLED", "EXPIRED"] {
            assert!(
                sqlx::query("UPDATE invitations SET state=?")
                    .bind(next)
                    .execute(&mut db)
                    .await
                    .is_err()
            );
        }
        assert!(
            sqlx::query("DELETE FROM invitations")
                .execute(&mut db)
                .await
                .is_err()
        );
        assert!(initialize(&mut db).await.is_err());
        db.close().await.unwrap();
        f.check(state, 0).await;
    }
    let f = Fixture::new().await;
    f.issued().await;
    let mut db = f.open().await;
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 200)
            .await
            .unwrap(),
        Outcome::Expired
    );
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 150)
            .await
            .unwrap(),
        Outcome::Denied
    );
    db.close().await.unwrap();
    f.check("EXPIRED", 0).await;
}

#[tokio::test]
async fn current_authority_and_exact_approval_required() {
    for change in [
        "UPDATE authority SET active=0",
        "UPDATE authority SET policy=8",
        "UPDATE authority SET owner='other'",
        "UPDATE authority SET endpoint='https://other.test'",
        "UPDATE authority SET fingerprint='other'",
        "DELETE FROM approvals",
        "UPDATE approvals SET binding='{}'",
    ] {
        let f = Fixture::new().await;
        f.issued().await;
        let mut db = f.open().await;
        sqlx::query(change).execute(&mut db).await.unwrap();
        assert_eq!(
            redeem(&mut db, "inv", "install", "device", || 150)
                .await
                .unwrap(),
            Outcome::Denied
        );
        db.close().await.unwrap();
        f.check("ISSUED", 0).await;
    }
    let f = Fixture::new().await;
    f.issued().await;
    let mut db = f.open().await;
    for (id, install, device, now) in [
        ("missing", "install", "device", 150),
        ("inv", "other", "device", 150),
        ("inv", "install", "other", 150),
        ("inv", "install", "device", 99),
    ] {
        assert_eq!(
            redeem(&mut db, id, install, device, || now).await.unwrap(),
            Outcome::Denied
        );
    }
    assert!(create(&mut db, &qr()).await.is_err());
    assert!(
        sqlx::query("UPDATE invitations SET binding='{}'")
            .execute(&mut db)
            .await
            .is_err()
    );
    db.close().await.unwrap();
    f.check("ISSUED", 0).await;
}

#[tokio::test]
async fn failed_insert_rolls_back_and_deferred_constraint_prevents_half_commit() {
    let f = Fixture::new().await;
    f.issued().await;
    let mut db = f.open().await;
    sqlx::raw_sql("CREATE TRIGGER inject_failure BEFORE INSERT ON synthetic_verifiers BEGIN SELECT RAISE(ABORT,'injected'); END;").execute(&mut db).await.unwrap();
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 150).await,
        Err(LedgerError)
    );
    db.close().await.unwrap();
    f.check("ISSUED", 0).await;
    let mut db = f.open().await;
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE invitations SET state='CONSUMED',verifier_id=id")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(tx.commit().await.is_err());
    db.close().await.unwrap();
    f.check("ISSUED", 0).await;
}

// Invoked by the parent in a fresh OS process. exit bypasses Rust destructors.
#[test]
fn crash_worker() {
    let Some(path) = std::env::var_os("SYNVEIL_P021_CHILD_DB") else {
        return;
    };
    let mode = std::env::var("SYNVEIL_P021_CHILD_MODE").unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut db = connect(std::path::Path::new(&path)).await.unwrap();
        if mode == "committed" {
            assert_eq!(
                redeem(&mut db, "inv", "install", "device", || 150)
                    .await
                    .unwrap(),
                Outcome::Consumed
            );
        } else {
            let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
            sqlx::query("UPDATE invitations SET state='CONSUMED',verifier_id=id")
                .execute(&mut *tx)
                .await
                .unwrap();
            if mode == "both_uncommitted" {
                sqlx::query(
                    "INSERT INTO synthetic_verifiers VALUES ('inv','NON_PRODUCTION_SYNTHETIC_V1')",
                )
                .execute(&mut *tx)
                .await
                .unwrap();
            }
            // Force dirty pages to the journal path before abrupt process exit.
            sqlx::query("PRAGMA cache_size=1")
                .execute(&mut *tx)
                .await
                .unwrap();
            std::process::exit(73);
        }
        std::process::exit(73);
    });
}

#[tokio::test]
async fn process_exit_recovery_and_lost_response() {
    for mode in ["half_uncommitted", "both_uncommitted", "committed"] {
        let f = Fixture::new().await;
        f.issued().await;
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--nocapture"])
            .env("SYNVEIL_P021_CHILD_DB", &f.0)
            .env("SYNVEIL_P021_CHILD_MODE", mode)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let committed = mode == "committed";
        f.check(
            if committed { "CONSUMED" } else { "ISSUED" },
            i64::from(committed),
        )
        .await;
        let mut db = f.open().await;
        assert_eq!(
            redeem(&mut db, "inv", "install", "device", || 150)
                .await
                .unwrap(),
            if committed {
                Outcome::Denied
            } else {
                Outcome::Consumed
            }
        );
        db.close().await.unwrap();
        f.check("CONSUMED", 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_race_has_one_immutable_outcome() {
    let f = Fixture::new().await;
    f.issued().await;
    let mut a = f.open().await;
    let mut b = f.open().await;
    let (r, c) = tokio::join!(
        redeem(&mut a, "inv", "install", "device", || 150),
        transition(&mut b, "inv", "install", || 150, true)
    );
    c.unwrap();
    let won = r.unwrap() == Outcome::Consumed;
    a.close().await.unwrap();
    b.close().await.unwrap();
    f.check(if won { "CONSUMED" } else { "CANCELLED" }, i64::from(won))
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn authority_and_clock_are_read_after_writer_fence() {
    for update in [
        "UPDATE authority SET active=0",
        "UPDATE authority SET policy=8",
    ] {
        let f = Fixture::new().await;
        f.issued().await;
        let mut admin = f.open().await;
        let mut caller = f.open().await;
        let mut tx = admin.begin_with("BEGIN IMMEDIATE").await.unwrap();
        sqlx::query(update).execute(&mut *tx).await.unwrap();
        let (send, recv) = tokio::sync::oneshot::channel();
        let t = tokio::spawn(async move {
            send.send(()).unwrap();
            let r = redeem(&mut caller, "inv", "install", "device", || 150)
                .await
                .unwrap();
            caller.close().await.unwrap();
            r
        });
        recv.await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(t.await.unwrap(), Outcome::Denied);
        admin.close().await.unwrap();
        f.check("ISSUED", 0).await;
    }
    let f = Fixture::new().await;
    f.issued().await;
    let mut db = f.open().await;
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 200)
            .await
            .unwrap(),
        Outcome::Expired
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn rollback_and_consumed_rows_are_immutable() {
    let f = Fixture::new().await;
    f.issued().await;
    let mut db = f.open().await;
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE invitations SET state='CONSUMED',verifier_id=id")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO synthetic_verifiers VALUES ('inv','NON_PRODUCTION_SYNTHETIC_V1')")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    f.check("ISSUED", 0).await;
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 150)
            .await
            .unwrap(),
        Outcome::Consumed
    );
    for query in [
        "UPDATE invitations SET state='ISSUED',verifier_id=NULL",
        "DELETE FROM invitations",
        "DELETE FROM synthetic_verifiers",
        "UPDATE synthetic_verifiers SET marker='NON_PRODUCTION_SYNTHETIC_V1'",
        "INSERT INTO synthetic_verifiers VALUES ('inv','NON_PRODUCTION_SYNTHETIC_V1')",
    ] {
        assert!(sqlx::query(query).execute(&mut db).await.is_err());
    }
    db.close().await.unwrap();
    f.check("CONSUMED", 1).await;
}

#[tokio::test]
async fn issuance_requires_current_authority_and_fixed_bindings() {
    for change in [
        "UPDATE authority SET active=0",
        "UPDATE authority SET policy=8",
        "DELETE FROM authority",
    ] {
        let f = Fixture::new().await;
        let mut db = f.open().await;
        sqlx::query(change).execute(&mut db).await.unwrap();
        assert_eq!(
            transition(&mut db, "inv", "install", || 150, false)
                .await
                .unwrap(),
            Outcome::Denied
        );
        db.close().await.unwrap();
        f.check("CREATED", 0).await;
    }
    let f = Fixture::new().await;
    let mut db = f.open().await;
    assert_eq!(
        transition(&mut db, "inv", "other", || 150, false)
            .await
            .unwrap(),
        Outcome::Denied
    );
    assert_eq!(
        transition(&mut db, "inv", "install", || 150, false)
            .await
            .unwrap(),
        Outcome::Issued
    );
    assert_eq!(
        transition(&mut db, "inv", "install", || 150, false)
            .await
            .unwrap(),
        Outcome::Denied
    );
    assert_eq!(
        transition(&mut db, "inv", "install", || 150, true)
            .await
            .unwrap(),
        Outcome::Cancelled
    );
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 150)
            .await
            .unwrap(),
        Outcome::Denied
    );
    db.close().await.unwrap();
    f.check("CANCELLED", 0).await;
}

#[tokio::test]
async fn no_secret_surface_or_cryptographic_verifier_is_introduced() {
    let f = Fixture::new().await;
    f.issued().await;
    let mut db = f.open().await;
    assert_eq!(
        redeem(&mut db, "inv", "install", "device", || 150)
            .await
            .unwrap(),
        Outcome::Consumed
    );
    let marker: String = sqlx::query_scalar("SELECT marker FROM synthetic_verifiers")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(marker, "NON_PRODUCTION_SYNTHETIC_V1");
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('synthetic_verifiers')")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(columns, vec!["invitation", "marker"]);
    assert_eq!(
        format!("{:?}", create(&mut db, &qr()).await.unwrap_err()),
        "LedgerError"
    );
    db.close().await.unwrap();
    let bytes = std::fs::read(&f.0).unwrap();
    for forbidden in [
        "sve1_",
        "svc1_",
        "enrollment_token",
        "device_credential",
        "secret_digest",
    ] {
        assert!(
            !bytes
                .windows(forbidden.len())
                .any(|w| w == forbidden.as_bytes())
        );
    }
}
