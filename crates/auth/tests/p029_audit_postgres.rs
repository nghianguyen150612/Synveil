//! Genuine PostgreSQL qualification of P029's evidence-only reference schema.
#[path = "support/p029.rs"]
mod audit;

use audit::p027::{Outcome as AuthorityOutcome, Request, begin};
use audit::*;
use sqlx::Row;
use std::net::SocketAddr;
use synveil_auth::authorization_reference::{Decision, audit_v1::*};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy},
    net::{TcpListener, TcpStream},
};

const PG_REQUIRED: &str = "explicit disposable PostgreSQL 17 required";

async fn drop_postgres_commit_ack(
    listener: TcpListener,
    upstream: SocketAddr,
) -> std::io::Result<()> {
    let (client, _) = listener.accept().await?;
    let server = TcpStream::connect(upstream).await?;
    let (mut client_read, mut client_write) = client.into_split();
    let (mut server_read, mut server_write) = server.into_split();
    let forward = copy(&mut client_read, &mut server_write);
    let reverse = async {
        let mut pending = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut ssl_response_pending = true;
        loop {
            let count = server_read.read(&mut buffer).await?;
            if count == 0 {
                return Ok::<(), std::io::Error>(());
            }
            pending.extend_from_slice(&buffer[..count]);
            loop {
                if ssl_response_pending && pending.first() == Some(&b'N') {
                    client_write.write_all(&pending[..1]).await?;
                    pending.drain(..1);
                    ssl_response_pending = false;
                    continue;
                }
                if pending.len() < 5 {
                    break;
                }
                let length = u32::from_be_bytes(pending[1..5].try_into().unwrap()) as usize;
                let frame_length = length + 1;
                if length < 4 || pending.len() < frame_length {
                    break;
                }
                let is_commit_complete = pending[0] == b'C'
                    && pending[5..frame_length].strip_suffix(&[0]) == Some(&b"COMMIT"[..]);
                if is_commit_complete {
                    // PostgreSQL emits this only after the COMMIT command has completed.
                    return Ok(());
                }
                client_write.write_all(&pending[..frame_length]).await?;
                pending.drain(..frame_length);
                ssl_response_pending = false;
            }
        }
    };
    tokio::select! {
        result = forward => result.map(|_| ()),
        result = reverse => result,
    }
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn committed_authority_mutation_and_audit_share_one_transaction() {
    let db = new_fixture().await;
    let request = Request::normal();
    assert_eq!(
        execute_and_audit(&db.pool, &request).await.unwrap(),
        AuthorityOutcome::Executed
    );
    let (records, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].event_type, EventType::ApprovalConsumed);
    assert_eq!(records[0].actor_id.as_deref(), Some("bob"));
    assert_eq!(records[0].outcome, Outcome::Committed);
    assert_eq!(records[1].event_type, EventType::Authorization);
    assert_eq!(records[1].outcome, Outcome::Committed);
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 1);
    assert_eq!(db.scalar("SELECT count(*) FROM outbox").await, 1);
    assert_eq!(
        db.scalar("SELECT count(*) FROM approvals WHERE consumed_by IS NOT NULL")
            .await,
        1
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn rejection_replay_and_revoked_reuse_never_create_false_success() {
    let db = new_fixture().await;
    let mut rejected = Request::normal();
    rejected.context.principal = audit::p027::browser("carol");
    assert!(matches!(
        execute_and_audit(&db.pool, &rejected).await.unwrap(),
        AuthorityOutcome::Rejected(_)
    ));
    let (denied, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    assert_eq!(denied[0].outcome, Outcome::Denied);
    assert!(denied[0].denial.is_some());
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 0);
    verify_complete("owner-record", &denied, &checkpoint).unwrap();

    let request = Request::normal();
    assert_eq!(
        execute_and_audit(&db.pool, &request).await.unwrap(),
        AuthorityOutcome::Executed
    );
    assert_eq!(
        execute_and_audit(&db.pool, &request).await.unwrap(),
        AuthorityOutcome::Recovered
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM p029_events WHERE event_type='authorization' AND outcome='committed'")
            .await,
        1
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM p029_events WHERE event_type='approval-consumed'")
            .await,
        1
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM p029_events WHERE outcome='recovered'")
            .await,
        1
    );
    revoke_and_audit(&db.pool, "root").await.unwrap();
    let mut later = Request::normal();
    later.operation = "reuse-after-revoke".into();
    assert!(matches!(
        execute_and_audit(&db.pool, &later).await.unwrap(),
        AuthorityOutcome::Rejected(Decision::StaleOrRevoked)
    ));
    let (records, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|r| r.outcome == Outcome::Committed && r.event_type == EventType::Authorization)
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.event_type == EventType::GrantRevocation)
            .count(),
        1
    );
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn unknown_commit_response_recovers_without_duplicate_success() {
    let db = new_fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let proxy = tokio::spawn(drop_postgres_commit_ack(
        listener,
        "127.0.0.1:5432".parse().unwrap(),
    ));
    let proxy_url = format!("postgresql://postgres:postgres@{address}/p027?sslmode=disable");
    let proxy_pool = audit::p027::Ledger::connect(&proxy_url, &db.schema).await;
    let request = Request::normal();
    assert!(execute_and_audit(&proxy_pool, &request).await.is_err());
    proxy_pool.close().await;
    proxy.await.unwrap().unwrap();

    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 1);
    assert_eq!(db.scalar("SELECT count(*) FROM p029_events WHERE event_type='authorization' AND outcome='committed'").await, 1);
    assert_eq!(
        execute_and_audit(&db.pool, &request).await.unwrap(),
        AuthorityOutcome::Recovered
    );
    let (records, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| {
                record.event_type == EventType::Authorization
                    && record.outcome == Outcome::Committed
            })
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.outcome == Outcome::Recovered)
            .count(),
        1
    );
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn session_and_device_revocation_are_logged_before_reuse_attempts() {
    let session_db = new_fixture().await;
    revoke_session_and_audit(&session_db.pool, "session-bob")
        .await
        .unwrap();
    assert!(matches!(
        execute_and_audit(&session_db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Rejected(Decision::Denied)
    ));
    let (session_events, session_checkpoint) = read_complete(&session_db.pool, "owner-record")
        .await
        .unwrap();
    assert_eq!(session_events[0].event_type, EventType::SessionRevocation);
    let session_reference = session_events[0].related_reference.as_deref().unwrap();
    assert!(session_reference.starts_with("p027-audit-"));
    assert_ne!(session_reference, "session-bob");
    assert_eq!(session_events[1].denial, Some(DenialCategory::Revoked));
    verify_complete("owner-record", &session_events, &session_checkpoint).unwrap();
    session_db.close().await;

    let device_db = new_fixture().await;
    revoke_device_and_audit(&device_db.pool, "device-actor")
        .await
        .unwrap();
    let mut device_request = Request::normal();
    device_request.session = "session-device-actor".into();
    device_request.context.principal = synveil_auth::authorization_reference::Principal::Device {
        id: "device-actor".into(),
        device: "device-a".into(),
    };
    device_request.context.authentication_method = "device".into();
    device_request.context.binding.device = Some("device-a".into());
    assert!(matches!(
        execute_and_audit(&device_db.pool, &device_request)
            .await
            .unwrap(),
        AuthorityOutcome::Rejected(Decision::Denied)
    ));
    let (device_events, device_checkpoint) = read_complete(&device_db.pool, "owner-record")
        .await
        .unwrap();
    assert_eq!(device_events[0].event_type, EventType::DeviceRevocation);
    assert_eq!(
        device_events[0].related_reference.as_deref(),
        Some("device-a")
    );
    assert_eq!(device_events[1].principal_kind, PrincipalKind::Device);
    assert_eq!(device_events[1].device_id.as_deref(), Some("device-a"));
    assert_eq!(device_events[1].denial, Some(DenialCategory::Revoked));
    verify_complete("owner-record", &device_events, &device_checkpoint).unwrap();
    device_db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn device_revocation_waits_for_in_flight_authority_and_audit_commit() {
    let db = new_fixture().await;
    sqlx::query("UPDATE ownership SET device='device-a'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO grants SELECT 'device-root','device-actor','owner-actor',NULL,owner_id,host,resource,resource_type,action,'device-a',role,revision,role_revision,issued,expires,deny,maximum FROM grants WHERE id='root'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO approvals SELECT 'device-approval','device-request','device-in-flight','device-actor',approver,owner_id,host,resource,resource_type,action,'device-a',revision,issued,expires,false,NULL FROM approvals WHERE id='approval'")
        .execute(&db.pool)
        .await
        .unwrap();

    let mut request = Request::normal();
    request.session = "session-device-actor".into();
    request.grant = "device-root".into();
    request.operation = "device-in-flight".into();
    request.context.principal = synveil_auth::authorization_reference::Principal::Device {
        id: "device-actor".into(),
        device: "device-a".into(),
    };
    request.context.authentication_method = "device".into();
    request.context.binding.device = Some("device-a".into());

    let mut tx = begin(&db.pool).await.unwrap();
    assert_eq!(
        audit::p027::execute_in(&mut tx, &request).await.unwrap(),
        AuthorityOutcome::Executed
    );
    let row = sqlx::query("SELECT o.actor,o.revision,own.owner_id,own.resource,own.action,o.approval_id FROM operations o JOIN grants g ON g.id=o.grant_id JOIN ownership own ON own.resource=g.resource WHERE o.id=$1")
        .bind(&request.operation)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let actor: String = row.get("actor");
    let owner: String = row.get("owner_id");
    let resource: String = row.get("resource");
    let revision: u64 = row.get::<i64, _>("revision") as u64;
    let approval: String = row.get("approval_id");
    let approval_order: i64 = sqlx::query_scalar(
        "SELECT seq FROM audit WHERE event='approval-consumed' AND identity=$1 AND detail=$2",
    )
    .bind(&approval)
    .bind(&request.operation)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    append_ordered(
        &mut tx,
        &owner,
        "device-approval-consumed",
        EventType::ApprovalConsumed,
        Some(&actor),
        Some(&resource),
        Operation::Approve,
        Some(revision),
        Outcome::Committed,
        None,
        &request.operation,
        &request.operation,
        Some(&approval),
        Some(approval_order as u64),
    )
    .await
    .unwrap();
    append(
        &mut tx,
        &owner,
        "device-operation-committed",
        EventType::Authorization,
        Some(&actor),
        Some(&resource),
        Operation::Read,
        Some(revision),
        Outcome::Committed,
        None,
        &request.operation,
        &request.operation,
        Some(&approval),
    )
    .await
    .unwrap();

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let revoke_pool = db.pool.clone();
    let mut revocation = tokio::spawn(async move {
        let _ = started_tx.send(());
        revoke_device_and_audit(&revoke_pool, "device-actor").await
    });
    started_rx.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut revocation)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    revocation.await.unwrap().unwrap();

    let mut later = request;
    later.operation = "device-after-revoke".into();
    assert!(matches!(
        execute_and_audit(&db.pool, &later).await.unwrap(),
        AuthorityOutcome::Rejected(Decision::Denied)
    ));
    let (records, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    assert_eq!(records[0].event_type, EventType::ApprovalConsumed);
    assert_eq!(records[1].event_type, EventType::Authorization);
    assert_eq!(records[1].outcome, Outcome::Committed);
    assert_eq!(records[2].event_type, EventType::DeviceRevocation);
    assert_eq!(records[3].outcome, Outcome::Denied);
    assert_eq!(records[3].denial, Some(DenialCategory::Revoked));
    assert_eq!(records[3].resource_id, None);
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn rollback_and_audit_failure_rollback_authority_and_approval_together() {
    let db = new_fixture().await;
    let mut tx = begin(&db.pool).await.unwrap();
    assert_eq!(
        audit::p027::execute_in(&mut tx, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    append(
        &mut tx,
        "owner-record",
        "rolled-back-event",
        EventType::Authorization,
        Some("bob"),
        Some("lib"),
        Operation::Read,
        Some(7),
        Outcome::Committed,
        None,
        "operation",
        "operation",
        Some("approval"),
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 0);
    assert_eq!(db.scalar("SELECT count(*) FROM p029_events").await, 0);
    assert_eq!(
        db.scalar("SELECT count(*) FROM approvals WHERE consumed_by IS NOT NULL")
            .await,
        0
    );

    sqlx::raw_sql("CREATE FUNCTION p029_test_reject_insert() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected audit storage failure'; END $$; CREATE TRIGGER p029_test_reject_insert BEFORE INSERT ON p029_events FOR EACH ROW EXECUTE FUNCTION p029_test_reject_insert();")
        .execute(&db.pool).await.unwrap();
    assert!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .is_err()
    );
    sqlx::raw_sql("DROP TRIGGER p029_test_reject_insert ON p029_events; DROP FUNCTION p029_test_reject_insert();")
        .execute(&db.pool).await.unwrap();
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 0);
    assert_eq!(db.scalar("SELECT count(*) FROM p029_events").await, 0);
    assert_eq!(
        db.scalar("SELECT count(*) FROM approvals WHERE consumed_by IS NOT NULL")
            .await,
        0
    );
    sqlx::raw_sql("CREATE FUNCTION p029_test_reject_outbox() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected outbox storage failure'; END $$; CREATE TRIGGER p029_test_reject_outbox BEFORE INSERT ON outbox FOR EACH ROW EXECUTE FUNCTION p029_test_reject_outbox();")
        .execute(&db.pool).await.unwrap();
    assert!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .is_err()
    );
    sqlx::raw_sql(
        "DROP TRIGGER p029_test_reject_outbox ON outbox; DROP FUNCTION p029_test_reject_outbox();",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 0);
    assert_eq!(db.scalar("SELECT count(*) FROM outbox").await, 0);
    assert_eq!(db.scalar("SELECT count(*) FROM p029_events").await, 0);
    assert_eq!(
        db.scalar("SELECT count(*) FROM approvals WHERE consumed_by IS NOT NULL")
            .await,
        0
    );
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn concurrent_grant_use_and_revocation_have_one_durable_order() {
    let db = new_fixture().await;
    let request = Request::normal();
    let (execution, revocation) = tokio::join!(
        execute_and_audit(&db.pool, &request),
        revoke_and_audit(&db.pool, "root")
    );
    let execution = execution.unwrap();
    revocation.unwrap();
    assert!(matches!(
        execution,
        AuthorityOutcome::Executed | AuthorityOutcome::Rejected(Decision::StaleOrRevoked)
    ));
    let (records, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    let revocation_index = records
        .iter()
        .position(|r| r.event_type == EventType::GrantRevocation)
        .unwrap();
    if execution == AuthorityOutcome::Executed {
        let execution_index = records
            .iter()
            .position(|r| {
                r.event_type == EventType::Authorization && r.outcome == Outcome::Committed
            })
            .unwrap();
        assert!(
            execution_index < revocation_index,
            "a committed use must precede the winning revoke"
        );
    }
    let mut later = Request::normal();
    later.operation = "race-follow-up".into();
    assert_eq!(
        execute_and_audit(&db.pool, &later).await.unwrap(),
        AuthorityOutcome::Rejected(Decision::StaleOrRevoked)
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn verifier_detects_missing_duplicate_reordered_and_tampered_database_records() {
    let db = new_fixture().await;
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    let (mut records, checkpoint) = read_complete(&db.pool, "owner-record").await.unwrap();
    let mut tampered = records.clone();
    tampered[0].actor_id = Some("attacker".into());
    assert_eq!(
        verify_complete("owner-record", &tampered, &checkpoint),
        Err(VerifyError::Integrity)
    );
    let duplicate = vec![records[0].clone(), records[0].clone()];
    let duplicate_checkpoint = Checkpoint {
        owner_id: "owner-record".into(),
        record_count: 2,
        head_digest: records[0].digest.clone(),
    };
    assert_eq!(
        verify_complete("owner-record", &duplicate, &duplicate_checkpoint),
        Err(VerifyError::DuplicateRecord)
    );

    assert!(
        sqlx::query(
            "UPDATE p029_heads SET last_sequence=last_sequence+1 WHERE owner_id='owner-record'"
        )
        .execute(&db.pool)
        .await
        .is_err()
    );
    assert!(read_complete(&db.pool, "owner-record").await.is_ok());
    assert!(
        sqlx::query("UPDATE p029_events SET digest=$1 WHERE owner_id='owner-record'")
            .bind(ZERO_DIGEST)
            .execute(&db.pool)
            .await
            .is_err()
    );
    records[0].digest = ZERO_DIGEST.to_owned();
    assert_eq!(
        verify_complete(
            "owner-record",
            &records,
            &Checkpoint {
                owner_id: "owner-record".into(),
                record_count: 2,
                head_digest: ZERO_DIGEST.into()
            }
        ),
        Err(VerifyError::Integrity)
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn verifier_detects_missing_success_evidence_after_checkpoint_rewrite() {
    let db = new_fixture().await;
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    let first_digest: String = sqlx::query_scalar(
        "SELECT digest FROM p029_events WHERE owner_id='owner-record' AND durable_sequence=1",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    sqlx::raw_sql("ALTER TABLE p029_events DISABLE TRIGGER p029_events_immutable; ALTER TABLE p029_heads DISABLE TRIGGER p029_heads_advance; DELETE FROM p029_events WHERE owner_id='owner-record' AND event_type='authorization'; UPDATE p029_heads SET last_sequence=1,last_digest='0000000000000000000000000000000000000000000000000000000000000000' WHERE owner_id='owner-record';")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE p029_heads SET last_digest=$1 WHERE owner_id='owner-record'")
        .bind(first_digest)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql("ALTER TABLE p029_events ENABLE TRIGGER p029_events_immutable; ALTER TABLE p029_heads ENABLE TRIGGER p029_heads_advance;")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        read_complete(&db.pool, "owner-record").await.is_err(),
        "P027 committed authority history must reveal a missing P029 success record"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn owner_read_scope_is_trusted_bounded_and_complete() {
    let db = new_fixture().await;
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    assert_eq!(
        read_owner_page(&db.pool, "owner-record", "foreign-owner", 0, 10).await,
        Err(VerifyError::ScopeMismatch)
    );
    assert_eq!(
        read_owner_page(
            &db.pool,
            "owner-record",
            "owner-record",
            0,
            MAX_PAGE_SIZE + 1
        )
        .await,
        Err(VerifyError::InvalidLimit)
    );
    let page = read_owner_page(&db.pool, "owner-record", "owner-record", 0, 1)
        .await
        .unwrap();
    assert_eq!(page.records.len(), 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn backend_interruption_rolls_back_uncommitted_evidence_then_recovery_succeeds() {
    let db = new_fixture().await;
    let mut tx = begin(&db.pool).await.unwrap();
    let backend_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        audit::p027::execute_in(&mut tx, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    append(
        &mut tx,
        "owner-record",
        "interrupted-event",
        EventType::Authorization,
        Some("bob"),
        Some("lib"),
        Operation::Read,
        Some(7),
        Outcome::Committed,
        None,
        "operation",
        "operation",
        Some("approval"),
    )
    .await
    .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT pg_terminate_backend($1)")
            .bind(backend_pid)
            .fetch_one(&db.pool)
            .await
            .unwrap()
    );
    assert!(tx.commit().await.is_err());
    assert_eq!(db.scalar("SELECT count(*) FROM operations").await, 0);
    assert_eq!(db.scalar("SELECT count(*) FROM p029_events").await, 0);
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    db.pool.close().await;
    let reconnected = audit::p027::Ledger::connect(&db.url, &db.schema).await;
    let (records, checkpoint) = read_complete(&reconnected, "owner-record").await.unwrap();
    assert_eq!(records.len(), 2);
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    reconnected.close().await;
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn unsupported_stored_event_type_fails_closed() {
    let db = new_fixture().await;
    let bad = sqlx::query("INSERT INTO p029_events(owner_id,durable_sequence,authority_order,event_id,schema_version,event_type,principal_kind,operation,outcome,operation_id,correlation_id,observed_at_unix_ns,timestamp_provenance,previous_digest,digest) VALUES ('owner-record',1,999,'bad-event',1,'future-event','unauthenticated','write','committed','operation','operation',1,'database-clock',$1,$1)")
        .bind(ZERO_DIGEST).execute(&db.pool).await;
    assert!(
        bad.is_err(),
        "schema must reject event types it does not support"
    );
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(version.starts_with("17."), "{PG_REQUIRED}: {version}");
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn p029_authority_and_evidence_survive_database_restart() {
    let phase =
        std::env::var("SYNVEIL_P029_RESTART_PHASE").unwrap_or_else(|_| "roundtrip".to_owned());
    if phase == "verify" {
        let schema_file = std::env::var("SYNVEIL_P029_RESTART_SCHEMA_FILE")
            .expect("restart verification requires the prepared schema path");
        let schema = std::fs::read_to_string(schema_file).unwrap();
        let schema = schema.trim();
        let url = std::env::var("SYNVEIL_P027_DATABASE_URL").unwrap();
        let pool = audit::p027::Ledger::connect(&url, schema).await;
        let (records, checkpoint) = read_complete(&pool, "owner-record").await.unwrap();
        assert_eq!(records.len(), 2);
        verify_complete("owner-record", &records, &checkpoint).unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        audit::p027::Ledger {
            pool,
            schema: schema.to_owned(),
            url,
        }
        .close()
        .await;
        return;
    }

    let db = new_fixture().await;
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        AuthorityOutcome::Executed
    );
    if phase == "prepare" {
        let schema_file = std::env::var("SYNVEIL_P029_RESTART_SCHEMA_FILE")
            .expect("restart preparation requires the schema path");
        std::fs::write(schema_file, &db.schema).unwrap();
        db.pool.close().await;
        return;
    }
    let pool = audit::p027::Ledger::connect(&db.url, &db.schema).await;
    let (records, checkpoint) = read_complete(&pool, "owner-record").await.unwrap();
    assert_eq!(records.len(), 2);
    verify_complete("owner-record", &records, &checkpoint).unwrap();
    pool.close().await;
    db.close().await;
}
