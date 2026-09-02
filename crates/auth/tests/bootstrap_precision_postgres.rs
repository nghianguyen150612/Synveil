use synveil_auth::{
    AuthenticationService, PasswordHasherConfig, PasswordParameters, PlaintextPassword,
};
use synveil_core::{LoginIdentifier, Timestamp};
use synveil_metadata::{DatabaseConfig, DatabasePool, MigrationRunner};

/// Real server clocks carry nanoseconds. First-admin creation must normalize
/// once before the strict microsecond metadata mapping, not fail HTTP bootstrap.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_first_admin_accepts_nanosecond_server_timestamp() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable test database");
    let pool = DatabasePool::connect(&DatabaseConfig::from_url(&url).unwrap())
        .await
        .unwrap();
    assert!(
        MigrationRunner::new()
            .run(&pool)
            .await
            .unwrap()
            .is_current()
    );
    let password_config =
        PasswordHasherConfig::new(PasswordParameters::new(16 * 1_024, 2, 1, 32).unwrap()).unwrap();
    let auth = AuthenticationService::new(&pool, password_config);
    let observed = Timestamp::parse("2026-08-28T01:02:03.987654321Z").unwrap();
    let expected = Timestamp::parse("2026-08-28T01:02:03.987654Z").unwrap();
    let user = auth
        .create_first_admin(
            LoginIdentifier::new("precision-owner", "key:precision-owner").unwrap(),
            PlaintextPassword::new("synthetic test owner password").unwrap(),
            observed,
        )
        .await
        .expect("nanosecond server time must not reject browser bootstrap");
    assert_eq!(user.created_at(), expected);
    assert_eq!(user.updated_at(), expected);
    let inspection = sqlx::PgPool::connect(&url).await.unwrap();
    let times: (
        time::OffsetDateTime,
        time::OffsetDateTime,
        time::OffsetDateTime,
        time::OffsetDateTime,
    ) = sqlx::query_as(
        "SELECT u.created_at, c.created_at, l.created_at, n.created_at
             FROM users u JOIN user_credentials c ON c.user_id = u.id
             JOIN libraries l ON l.owner_user_id = u.id
             JOIN nodes n ON n.id = l.root_node_id WHERE u.id = $1",
    )
    .bind(user.id().into_uuid())
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!(
        times,
        (
            expected.as_offset_datetime(),
            expected.as_offset_datetime(),
            expected.as_offset_datetime(),
            expected.as_offset_datetime()
        )
    );
    inspection.close().await;
    pool.close().await;
}
