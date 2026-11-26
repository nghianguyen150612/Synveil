//! Partial P030 test-only transactional projection qualification.
#[path = "support/p029.rs"]
mod audit;

use audit::p027::{Outcome, Request};
use audit::{execute_and_audit, new_fixture, read_complete, revoke_and_audit};

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
    let (records, _) = read_complete(&db.pool, "owner-record").await.unwrap();
    assert_eq!(
        db.scalar("SELECT count(*) FROM p030_delivery").await,
        records.len() as i64
    );
    db.close().await;
}
