//! Partial P030 test-only transactional projection qualification.
#[path = "support/p029.rs"]
mod audit;

use audit::p027::{Outcome, Request};
use audit::{
    execute_and_audit, new_fixture, read_complete, read_owner_page, revoke_and_audit,
    revoke_device_and_audit, revoke_session_and_audit,
};

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn projection_covers_each_committed_evidence_record() {
    let db = new_fixture().await;
    sqlx::raw_sql(include_str!("support/p030.sql"))
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        execute_and_audit(&db.pool, &Request::normal())
            .await
            .unwrap(),
        Outcome::Executed
    );
    revoke_and_audit(&db.pool, "root").await.unwrap();
    revoke_session_and_audit(&db.pool, "session-bob")
        .await
        .unwrap();
    revoke_device_and_audit(&db.pool, "device-actor")
        .await
        .unwrap();
    let page = read_owner_page(&db.pool, "owner-record", "owner-record", 0, 1)
        .await
        .unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(
        read_owner_page(&db.pool, "owner-record", "foreign-owner", 0, 1)
            .await
            .is_err()
    );
    let (records, _) = read_complete(&db.pool, "owner-record").await.unwrap();
    assert_eq!(
        db.scalar("SELECT count(*) FROM p030_delivery").await,
        records.len() as i64
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 17 required"]
async fn installation_rejects_existing_events_without_partial_projection() {
    let db = new_fixture().await;
    execute_and_audit(&db.pool, &Request::normal())
        .await
        .unwrap();
    assert!(
        sqlx::raw_sql(include_str!("support/p030.sql"))
            .execute(&db.pool)
            .await
            .is_err()
    );
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('p030_delivery') IS NOT NULL")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(!exists);
    db.close().await;
}
