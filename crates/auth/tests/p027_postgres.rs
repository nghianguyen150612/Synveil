//! Real PostgreSQL 17 qualification, opt-in and fail hard without a database.
#[path = "support/p027.rs"]
mod ledger;
use ledger::*;
use sqlx::Connection;
use synveil_auth::authorization_reference::{Decision, Principal};

const REQUIRES_PG: &str = "explicit disposable PostgreSQL 17 required";

async fn consistent(db: &Ledger, count: i64) {
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, count);
    assert_eq!(db.scalar("SELECT count(*) FROM outbox").await, count);
    assert_eq!(
        db.scalar("SELECT count(*) FROM audit WHERE event='executed'")
            .await,
        count
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM approvals WHERE consumed_by IS NOT NULL")
            .await,
        count
    );
    assert_eq!(db.scalar("SELECT count(*) FROM operations o JOIN approvals a ON o.approval_id=a.id AND a.consumed_by=o.id JOIN outbox b ON b.operation=o.id JOIN audit e ON e.identity=o.id AND e.event='executed' JOIN audit c ON c.identity=a.id AND c.event='approval-consumed' AND c.transaction_id=e.transaction_id").await,count);
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn execution_consumption_idempotency_and_recovery() {
    let db = Ledger::new().await;
    let r = Request::normal();
    assert_eq!(execute(&db.pool, &r).await.unwrap(), Outcome::Executed);
    consistent(&db, 1).await;
    db.pool.close().await;
    let pool = Ledger::connect(&db.url, &db.schema).await;
    let (recovery, revocation) = tokio::join!(execute(&pool, &r), revoke(&pool, "root"));
    assert_eq!(recovery.unwrap(), Outcome::Recovered);
    revocation.unwrap();
    // Status recovery is permitted for the original authenticated requester;
    // it is not a second execution, including after authority revocation.
    assert_eq!(execute(&pool, &r).await.unwrap(), Outcome::Recovered);
    let mut different = r.clone();
    different.context.binding.resource_id = "other".into();
    assert_eq!(
        execute(&pool, &different).await.unwrap(),
        Outcome::IdentityConflict
    );
    different = r.clone();
    different.operation = "another-operation".into();
    assert_eq!(
        execute(&pool, &different).await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn concurrent_consumers_and_duplicate_operations() {
    let db = Ledger::new().await;
    let r = Request::normal();
    let (a, b) = tokio::join!(execute(&db.pool, &r), execute(&db.pool, &r));
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes.iter().filter(|o| **o == Outcome::Executed).count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == Outcome::Recovered)
            .count(),
        1
    );
    consistent(&db, 1).await;
    let mut next = r.clone();
    next.operation = "different-operation".into();
    assert_eq!(
        execute(&db.pool, &next).await.unwrap(),
        Outcome::Rejected(Decision::RequiresApproval)
    );
    assert!(
        sqlx::query("UPDATE approvals SET consumed_by=NULL")
            .execute(&db.pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO approvals SELECT 'copy','copy-request','different-operation',requester,approver,owner_id,host,resource,resource_type,action,device,revision,issued,expires,false,'operation' FROM approvals").execute(&db.pool).await.is_err());
    consistent(&db, 1).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn immutable_ids_parent_integrity_and_history() {
    let db = Ledger::new().await;
    for sql in [
        "INSERT INTO grants SELECT * FROM grants WHERE id='root'",
        "UPDATE grants SET role='editor' WHERE id='root'",
        "DELETE FROM grants WHERE id='child'",
        "INSERT INTO grants SELECT 'missing','bob','alice','absent',owner_id,host,resource,resource_type,action,device,role,revision,role_revision,issued,expires,deny,maximum FROM grants WHERE id='root'",
        "INSERT INTO grants SELECT 'cycle','bob','alice','cycle',owner_id,host,resource,resource_type,action,device,role,revision,role_revision,issued,expires,deny,maximum FROM grants WHERE id='root'",
        "UPDATE approvals SET approver=requester",
        "UPDATE audit SET detail='forged'",
        "DELETE FROM audit",
        "TRUNCATE audit",
        "UPDATE policy SET revision=6",
    ] {
        assert!(
            sqlx::query(sql).execute(&db.pool).await.is_err(),
            "accepted {sql}"
        );
    }
    revoke(&db.pool, "root").await.unwrap();
    for sql in [
        "DELETE FROM revocations",
        "UPDATE revocations SET reason='restore'",
        "UPDATE grants SET id='new-root' WHERE id='root'",
        "INSERT INTO grants SELECT * FROM grants WHERE id='root'",
    ] {
        assert!(
            sqlx::query(sql).execute(&db.pool).await.is_err(),
            "accepted {sql}"
        );
    }
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    consistent(&db, 0).await;
    db.close().await;
}

// Deterministic lock coordination: the blocker holds the fence, the contender
// publishes its backend PID, and pg_blocking_pids proves it is waiting. No sleeps.
async fn wait_blocked(pool: &sqlx::PgPool, pid: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0")
                .bind(pid)
                .fetch_one(pool)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("contender must actually block on a database lock");
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn ordered_authorization_revocation_and_stale_retry() {
    for mutation in [
        "INSERT INTO revocations(grant_id,reason) VALUES ('root','ordered')",
        "UPDATE policy SET role_revision=2",
        "UPDATE policy SET revision=8",
        "UPDATE ownership SET owner_id='different-owner'",
        "UPDATE actors SET revoked=true WHERE id='bob'",
    ] {
        let db = Ledger::new().await;
        let mut before = begin(&db.pool).await.unwrap();
        sqlx::query(mutation).execute(&mut *before).await.unwrap();
        let pool = db.pool.clone();
        let (send, recv) = tokio::sync::oneshot::channel();
        let contender = tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *tx)
                .await
                .unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            send.send(pid).unwrap();
            let error = sqlx::query("UPDATE policy SET epoch=epoch+1 WHERE id=1")
                .execute(&mut *tx)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("40001")
            );
            tx.rollback().await.unwrap();
        });
        let pid = recv.await.unwrap();
        wait_blocked(&db.pool, pid).await;
        before.commit().await.unwrap(); // precise happens-before contender retry
        contender.await.unwrap();
        assert!(
            matches!(
                execute(&db.pool, &Request::normal()).await.unwrap(),
                Outcome::Rejected(_)
            ),
            "post-commit bypass: {mutation}"
        );
        consistent(&db, 0).await;
        db.close().await;
    }
    // Reverse ordering: execution commits before a waiting revocation. The prior
    // outcome is valid, while a new operation after revocation must fail.
    let db = Ledger::new().await;
    let mut tx = begin(&db.pool).await.unwrap();
    assert_eq!(
        execute_in(&mut tx, &Request::normal()).await.unwrap(),
        Outcome::Executed
    );
    let pool = db.pool.clone();
    let (send, recv) = tokio::sync::oneshot::channel();
    let revoke_task = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        send.send(pid).unwrap();
        sqlx::query("UPDATE policy SET epoch=epoch+1 WHERE id=1")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO revocations(grant_id,reason) VALUES ('root','after execution')")
            .execute(&mut *tx)
            .await
            .unwrap();
        audit(&mut tx, "revoked", "root", "ordered after execute commit")
            .await
            .unwrap();
        tx.commit().await.unwrap();
    });
    wait_blocked(&db.pool, recv.await.unwrap()).await;
    tx.commit().await.unwrap();
    revoke_task.await.unwrap();
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event FROM audit WHERE event IN ('executed','revoked') ORDER BY seq",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(events, vec!["executed", "revoked"]);
    let mut r = Request::normal();
    r.operation = "after-revoke".into();
    assert_eq!(
        execute(&db.pool, &r).await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    consistent(&db, 1).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn delegation_issuance_revocation_and_duplicate_races() {
    let db = Ledger::new().await;
    let (issued, revoked) = tokio::join!(delegate(&db.pool, "new-child"), revoke(&db.pool, "root"));
    revoked.unwrap();
    let d = issued.unwrap();
    assert!(matches!(d, Decision::Allowed | Decision::StaleOrRevoked));
    assert_eq!(
        db.scalar("SELECT count(*) FROM grants WHERE id='new-child'")
            .await,
        i64::from(d == Decision::Allowed)
    );
    if d == Decision::Allowed {
        let events: Vec<String> = sqlx::query_scalar(
            "SELECT event FROM audit WHERE identity='new-child' OR event='revoked' ORDER BY seq",
        )
        .fetch_all(&db.pool)
        .await
        .unwrap();
        assert_eq!(events, vec!["delegated", "revoked"]);
        let mut issued_request = Request::normal();
        issued_request.grant = "new-child".into();
        assert_eq!(
            execute(&db.pool, &issued_request).await.unwrap(),
            Outcome::Rejected(Decision::StaleOrRevoked)
        );
    }
    assert_eq!(
        delegate(&db.pool, "post-revoke").await.unwrap(),
        Decision::StaleOrRevoked
    );
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    db.close().await;
    let db = Ledger::new().await;
    let (a, b) = tokio::join!(
        delegate(&db.pool, "duplicate"),
        delegate(&db.pool, "duplicate")
    );
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(), 1);
    let error = a.err().or_else(|| b.err()).unwrap();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    let (a, b) = tokio::join!(
        delegate(&db.pool, "parallel-a"),
        delegate(&db.pool, "parallel-b")
    );
    assert_eq!(a.unwrap(), Decision::Allowed);
    assert_eq!(b.unwrap(), Decision::Allowed);
    sqlx::query("UPDATE actors SET revoked=true WHERE id='alice'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        delegate(&db.pool, "revoked-actor").await.unwrap(),
        Decision::Denied
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn approval_withdrawal_expiry_and_lock_time_expiry() {
    let db = Ledger::new().await;
    sqlx::query("UPDATE approvals SET withdrawn=true")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::RequiresApproval)
    );
    assert!(
        sqlx::query("UPDATE approvals SET withdrawn=false")
            .execute(&db.pool)
            .await
            .is_err()
    );
    consistent(&db, 0).await;
    db.close().await;
    // Expiry ordered while execution waits: fixture authority uses DB wall clock
    // after lock acquisition. No sleep or caller clock can extend validity.
    let db = Ledger::modified(&[("4102444800", "2")]).await;
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    consistent(&db, 0).await;
    db.close().await;
    let db = Ledger::modified(&[(
        "'approval','request','operation','bob','carol','owner-record','host','lib',7,1,4102444800",
        "'approval','request','operation','bob','carol','owner-record','host','lib',7,1,2",
    )])
    .await;
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::RequiresApproval)
    );
    consistent(&db, 0).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn adversarial_persisted_authority_matrix() {
    type Mutation = (&'static str, &'static str, &'static str, Decision);
    let cases: &[Mutation] = &[
        (
            "wrong issuer",
            "'root','alice','owner-actor',NULL",
            "'root','alice','carol',NULL",
            Decision::Denied,
        ),
        (
            "stale role",
            "'lib',7,1,1,4102444800",
            "'lib',7,2,1,4102444800",
            Decision::StaleOrRevoked,
        ),
        (
            "forged role",
            "DEFAULT 'viewer'",
            "DEFAULT 'root'",
            Decision::Unsupported,
        ),
        (
            "policy version",
            "VALUES (1,2,7,1,true)",
            "VALUES (1,3,7,1,true)",
            Decision::Unsupported,
        ),
        (
            "ancestor expired",
            "'lib',7,1,1,4102444800",
            "'lib',7,1,1,2",
            Decision::StaleOrRevoked,
        ),
        (
            "cross host",
            "'root','alice','owner-actor',NULL,'owner-record','host'",
            "'root','alice','owner-actor',NULL,'owner-record','foreign-host'",
            Decision::Denied,
        ),
        (
            "cross owner",
            "'root','alice','owner-actor',NULL,'owner-record'",
            "'root','alice','owner-actor',NULL,'foreign-owner'",
            Decision::Denied,
        ),
        (
            "wrong approver",
            "'bob','carol','owner-record'",
            "'bob','alice','owner-record'",
            Decision::RequiresApproval,
        ),
        (
            "same human session",
            "'bob','carol','owner-record'",
            "'bob','alice-session2','owner-record'",
            Decision::RequiresApproval,
        ),
        (
            "wrong approval operation",
            "'approval','request','operation'",
            "'approval','request','other-operation'",
            Decision::RequiresApproval,
        ),
        (
            "wrong approval scope",
            "'approval','request','operation','bob','carol','owner-record','host'",
            "'approval','request','operation','bob','carol','owner-record','foreign-host'",
            Decision::RequiresApproval,
        ),
        (
            "stale approval",
            "'host','lib',7,1,4102444800);",
            "'host','lib',6,1,4102444800);",
            Decision::RequiresApproval,
        ),
        (
            "legacy denial",
            "legacy_allow boolean NOT NULL DEFAULT true",
            "legacy_allow boolean NOT NULL DEFAULT false",
            Decision::Denied,
        ),
        (
            "unknown resource type",
            "DEFAULT 'library'",
            "DEFAULT 'unknown'",
            Decision::Denied,
        ),
        (
            "unknown action",
            "DEFAULT 'read'",
            "DEFAULT 'root-shell'",
            Decision::Denied,
        ),
    ];
    for (name, from, to, expected) in cases {
        let db = Ledger::modified(&[(from, to)]).await;
        assert_eq!(
            execute(&db.pool, &Request::normal()).await.unwrap(),
            Outcome::Rejected(*expected),
            "{name}"
        );
        consistent(&db, 0).await;
        db.close().await;
    }
    let db = Ledger::new().await;
    for case in [
        "principal",
        "owner",
        "host",
        "device",
        "resource",
        "action",
        "revision",
        "session",
        "root",
    ] {
        let mut r = Request::normal();
        match case {
            "principal" => r.context.principal = browser("carol"),
            "owner" => r.context.binding.owner = "forged".into(),
            "host" => r.context.binding.host = "other-host".into(),
            "device" => {
                r.context.binding.device = Some("other-device".into());
                r.context.principal = Principal::Device {
                    id: "bob".into(),
                    device: "other-device".into(),
                };
            }
            "resource" => r.context.binding.resource_id = "other-resource".into(),
            "action" => r.context.binding.action = "delete".into(),
            "revision" => r.context.revision = 6,
            "session" => r.session = "forged".into(),
            "root" => r.context.binding.action = "os-root".into(),
            _ => unreachable!(),
        }
        assert!(
            matches!(execute(&db.pool, &r).await.unwrap(), Outcome::Rejected(_)),
            "{case}"
        );
    }
    consistent(&db, 0).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn connection_termination_rolls_back_effect_approval_and_audit() {
    let db = Ledger::new().await;
    let mut tx = begin(&db.pool).await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        execute_in(&mut tx, &Request::normal()).await.unwrap(),
        Outcome::Executed
    );
    sqlx::query("INSERT INTO grants SELECT 'uncommitted','bob','alice','root',owner_id,host,resource,resource_type,action,device,role,revision,role_revision,issued,expires,deny,maximum FROM grants WHERE id='root'").execute(&mut *tx).await.unwrap();
    let killed: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(pid)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(killed);
    assert!(tx.commit().await.is_err());
    consistent(&db, 0).await;
    assert_eq!(
        db.scalar("SELECT count(*) FROM grants WHERE id='uncommitted'")
            .await,
        0
    );
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Executed
    );
    consistent(&db, 1).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn subprocess_exit_without_destructors_recovery() {
    const CHILD_SCHEMA: &str = "SYNVEIL_P027_CRASH_SCHEMA";
    if let Ok(schema) = std::env::var(CHILD_SCHEMA) {
        let url = std::env::var("SYNVEIL_P027_DATABASE_URL").unwrap();
        let pool = Ledger::connect(&url, &schema).await;
        let mut tx = begin(&pool).await.unwrap();
        assert_eq!(
            execute_in(&mut tx, &Request::normal()).await.unwrap(),
            Outcome::Executed
        );
        // Real process termination: no transaction, pool or runtime destructors.
        std::process::exit(27);
    }
    let db = Ledger::new().await;
    delegate(&db.pool, "durable-grant").await.unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "subprocess_exit_without_destructors_recovery",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_SCHEMA, &db.schema)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(27));
    // Acquire the fence to await backend rollback after the OS closes sockets.
    let tx = begin(&db.pool).await.unwrap();
    tx.commit().await.unwrap();
    consistent(&db, 0).await;
    assert_eq!(
        db.scalar("SELECT count(*) FROM grants WHERE id='durable-grant'")
            .await,
        1
    );
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Executed
    );
    revoke(&db.pool, "root").await.unwrap();
    db.pool.close().await;
    let pool = Ledger::connect(&db.url, &db.schema).await;
    assert_eq!(
        execute(&pool, &Request::normal()).await.unwrap(),
        Outcome::Recovered
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM revocations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM approvals WHERE consumed_by='operation'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pool.close().await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn real_deadlock_sqlstate_and_bounded_retry_classification() {
    let db = Ledger::new().await;
    // Deliberately violate the documented order to demonstrate 40P01. This is
    // a qualification probe, never the adapter's authorization lock strategy.
    let mut a = db.pool.begin().await.unwrap();
    let mut b = db.pool.begin().await.unwrap();
    sqlx::query("SELECT * FROM actors WHERE id='alice' FOR UPDATE")
        .fetch_one(&mut *a)
        .await
        .unwrap();
    sqlx::query("SELECT * FROM actors WHERE id='bob' FOR UPDATE")
        .fetch_one(&mut *b)
        .await
        .unwrap();
    let (x, y) = tokio::join!(
        sqlx::query("SELECT * FROM actors WHERE id='bob' FOR UPDATE").fetch_one(&mut *a),
        sqlx::query("SELECT * FROM actors WHERE id='alice' FOR UPDATE").fetch_one(&mut *b)
    );
    let e = x.err().or_else(|| y.err()).expect("one deadlock victim");
    assert_eq!(
        e.as_database_error().unwrap().code().as_deref(),
        Some("40P01")
    );
    assert!(retryable(&e));
    a.rollback().await.unwrap();
    b.rollback().await.unwrap();
    assert!(!retryable(&sqlx::Error::RowNotFound));
    let mut c = sqlx::PgConnection::connect(&db.url).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert!(version.starts_with("17."), "{REQUIRES_PG}: {version}");
    c.close().await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn same_human_self_approval_and_deny_precedence() {
    let db = Ledger::modified(&[
        ("('bob','bob-human',NULL)", "('bob','alice-human',NULL)"),
        (
            "'owner-actor','carol');",
            "'owner-actor','alice-session2');",
        ),
        (
            "'approval','request','operation','bob','carol'",
            "'approval','request','operation','bob','alice-session2'",
        ),
    ])
    .await;
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::RequiresApproval)
    );
    consistent(&db, 0).await;
    db.close().await;
    let db = Ledger::new().await;
    sqlx::query("INSERT INTO grants SELECT 'deny','bob','owner-actor',NULL,owner_id,host,resource,resource_type,action,device,role,revision,role_revision,issued,expires,true,maximum FROM grants WHERE id='root'").execute(&db.pool).await.unwrap();
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Rejected(Decision::Denied)
    );
    consistent(&db, 0).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn exact_device_binding_positive_and_replay() {
    let db = Ledger::new().await;
    sqlx::query("UPDATE ownership SET device='device-a'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO grants SELECT 'device-root','device-actor','owner-actor',NULL,owner_id,host,resource,resource_type,action,'device-a',role,revision,role_revision,issued,expires,deny,maximum FROM grants WHERE id='root'").execute(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO approvals SELECT 'device-approval','device-request','device-operation','device-actor',approver,owner_id,host,resource,resource_type,action,'device-a',revision,issued,expires,false,NULL FROM approvals WHERE id='approval'").execute(&db.pool).await.unwrap();
    let mut r = Request::normal();
    r.session = "session-device-actor".into();
    r.grant = "device-root".into();
    r.operation = "device-operation".into();
    r.context.principal = Principal::Device {
        id: "device-actor".into(),
        device: "device-a".into(),
    };
    r.context.authentication_method = "device".into();
    r.context.binding.device = Some("device-a".into());
    assert_eq!(execute(&db.pool, &r).await.unwrap(), Outcome::Executed);
    r.operation = "device-replay".into();
    r.context.binding.device = Some("device-b".into());
    assert_eq!(
        execute(&db.pool, &r).await.unwrap(),
        Outcome::Rejected(Decision::Denied)
    );
    consistent(&db, 1).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn expiry_is_reloaded_after_a_proven_lock_wait() {
    let db = Ledger::modified(&[(
        "4102444800",
        "(floor(extract(epoch FROM clock_timestamp()))::bigint+3)",
    )])
    .await;
    let holder = begin(&db.pool).await.unwrap();
    let pool = db.pool.clone();
    let (send, recv) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        send.send(pid).unwrap();
        // READ COMMITTED probe waits on the same fence, then uses the common
        // execution body. SERIALIZABLE retry behavior is qualified separately.
        sqlx::query("UPDATE policy SET epoch=epoch+1 WHERE id=1")
            .execute(&mut *tx)
            .await
            .unwrap();
        let outcome = execute_in(&mut tx, &Request::normal()).await.unwrap();
        tx.commit().await.unwrap();
        outcome
    });
    wait_blocked(&db.pool, recv.await.unwrap()).await;
    tokio::time::timeout(std::time::Duration::from_secs(15),async {
        loop {
            let expired:bool=sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint >= max(expires) FROM grants").fetch_one(&db.pool).await.unwrap();
            if expired {break;}tokio::task::yield_now().await;
        }
    }).await.unwrap();
    holder.commit().await.unwrap();
    assert_eq!(
        task.await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    consistent(&db, 0).await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn durable_reconnect_and_database_restart_probe() {
    assert_eq!(std::env::var("SYNVEIL_P027_DISPOSABLE").as_deref(), Ok("1"));
    let phase = std::env::var("SYNVEIL_P027_RESTART_PHASE").unwrap_or_else(|_| "reconnect".into());
    let db = if phase == "verify" {
        let path = std::env::var("SYNVEIL_P027_RESTART_FILE").unwrap();
        let schema = std::fs::read_to_string(path).unwrap();
        let url = std::env::var("SYNVEIL_P027_DATABASE_URL").unwrap();
        let pool = Ledger::connect(&url, &schema).await;
        Ledger { pool, schema, url }
    } else {
        assert!(phase == "prepare" || phase == "reconnect");
        let db = Ledger::new().await;
        assert_eq!(
            execute(&db.pool, &Request::normal()).await.unwrap(),
            Outcome::Executed
        );
        revoke(&db.pool, "root").await.unwrap();
        if phase == "prepare" {
            std::fs::write(
                std::env::var("SYNVEIL_P027_RESTART_FILE").unwrap(),
                &db.schema,
            )
            .unwrap();
            db.pool.close().await;
            return; // external runner now restarts this disposable server
        }
        db.pool.close().await;
        let pool = Ledger::connect(&db.url, &db.schema).await;
        Ledger {
            pool,
            schema: db.schema,
            url: db.url,
        }
    };
    consistent(&db, 1).await;
    assert_eq!(
        db.scalar("SELECT count(*) FROM grants WHERE id='root'")
            .await,
        1
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM revocations WHERE grant_id='root'")
            .await,
        1
    );
    assert_eq!(
        execute(&db.pool, &Request::normal()).await.unwrap(),
        Outcome::Recovered
    );
    let mut fresh = Request::normal();
    fresh.operation = "after-restart".into();
    assert_eq!(
        execute(&db.pool, &fresh).await.unwrap(),
        Outcome::Rejected(Decision::StaleOrRevoked)
    );
    db.close().await;
}
