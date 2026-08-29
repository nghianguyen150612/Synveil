use std::time::Duration;

use synveil_auth::{DeviceAuthError, DeviceAuthenticationService, DeviceEnrollmentTarget};
use synveil_core::{
    DeviceCredentialSecret, DeviceId, DeviceStatus, EnrollmentSecret, LogicalName, LoginIdentifier,
    Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner};

fn at(seconds: i64) -> Timestamp {
    let start = Timestamp::parse("2026-08-28T00:00:00.123456Z").unwrap();
    Timestamp::from_offset_datetime(start.as_offset_datetime() + time::Duration::seconds(seconds))
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).unwrap()
}

async fn fixture() -> (DatabasePool, sqlx::PgPool, UserId, UserId) {
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
    let inspection = sqlx::PgPool::connect(&url).await.unwrap();
    let owner = UserId::new();
    let other_owner = UserId::new();
    for (id, label) in [(owner, "device-owner"), (other_owner, "other-owner")] {
        let user = User::new(
            id,
            LoginIdentifier::new(label, format!("key:{label}")).unwrap(),
            UserStatus::Active,
            at(0),
        );
        DomainRepository::new(&pool)
            .insert_user(&user)
            .await
            .unwrap();
    }
    (pool, inspection, owner, other_owner)
}

/// Fresh PostgreSQL proof of the production service, including immediate
/// revocation without cache and the explicit lost-response recovery contract.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_device_credentials_enroll_once_and_revoke_immediately() {
    let (pool, inspection, owner, other_owner) = fixture().await;
    let auth = DeviceAuthenticationService::new(&pool);
    let grant = auth
        .create_grant_at(owner, DeviceEnrollmentTarget::New(name("My laptop")), at(0))
        .await
        .unwrap();
    let device = DomainRepository::new(&pool)
        .find_device(grant.device_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(device.owner_user_id(), owner);
    assert_eq!(device.display_name(), &name("My laptop"));
    assert_eq!(device.status(), DeviceStatus::Pending);
    assert_eq!(grant.expires_at, at(600));
    assert!(!format!("{grant:?}").contains(grant.token.expose_secret()));

    let stored_grant: (Vec<u8>, String) = sqlx::query_as(
        "SELECT secret_digest, row_to_json(g)::TEXT FROM device_enrollment_grants g WHERE grant_id = $1",
    ).bind(grant.grant_id.into_uuid()).fetch_one(&inspection).await.unwrap();
    assert_eq!(stored_grant.0, grant.token.digest());
    assert!(!stored_grant.1.contains(grant.token.expose_secret()));
    assert_eq!(
        auth.create_grant_at(
            other_owner,
            DeviceEnrollmentTarget::Existing(grant.device_id),
            at(1)
        )
        .await
        .unwrap_err(),
        DeviceAuthError::DeviceNotFound
    );
    assert_eq!(
        auth.create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(DeviceId::new()),
            at(1)
        )
        .await
        .unwrap_err(),
        DeviceAuthError::DeviceNotFound
    );
    assert_eq!(
        auth.exchange_at(&EnrollmentSecret::from_bytes([5; 32]), at(1))
            .await
            .unwrap_err(),
        DeviceAuthError::InvalidEnrollment
    );

    let credential = auth.exchange_at(&grant.token, at(1)).await.unwrap();
    assert_eq!(credential.owner_user_id, owner);
    assert_eq!(credential.device_id, grant.device_id);
    assert!(!format!("{credential:?}").contains(credential.secret.expose_secret()));
    let active = DomainRepository::new(&pool)
        .find_device(grant.device_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.status(), DeviceStatus::Active);
    assert_eq!(active.revision().get(), 1);
    assert_eq!(
        auth.exchange_at(&grant.token, at(2)).await.unwrap_err(),
        DeviceAuthError::InvalidEnrollment
    );
    let principal = auth.authenticate(&credential.secret).await.unwrap();
    assert_eq!(principal.owner_user_id, owner);
    assert_eq!(principal.device_id, grant.device_id);
    assert_eq!(principal.credential_id, credential.credential_id);
    let stored_credential: (Vec<u8>, String) = sqlx::query_as(
        "SELECT secret_digest, row_to_json(c)::TEXT FROM device_credentials c WHERE credential_id = $1",
    ).bind(credential.credential_id.into_uuid()).fetch_one(&inspection).await.unwrap();
    assert_eq!(stored_credential.0, credential.secret.digest());
    assert!(
        !stored_credential
            .1
            .contains(credential.secret.expose_secret())
    );
    assert_eq!(
        auth.authenticate(&DeviceCredentialSecret::from_bytes([6; 32]))
            .await,
        Err(DeviceAuthError::InvalidCredential)
    );
    assert_eq!(
        auth.authenticate_raw(&credential.credential_id.to_string())
            .await,
        Err(DeviceAuthError::InvalidCredential)
    );
    assert_eq!(
        auth.authenticate_raw(&"x".repeat(4096)).await,
        Err(DeviceAuthError::InvalidCredential)
    );

    let expired = auth
        .create_grant_at(owner, DeviceEnrollmentTarget::New(name("Expiry")), at(2))
        .await
        .unwrap();
    assert_eq!(
        auth.exchange_at(&expired.token, expired.expires_at)
            .await
            .unwrap_err(),
        DeviceAuthError::InvalidEnrollment
    );
    assert_eq!(
        DomainRepository::new(&pool)
            .find_device(expired.device_id)
            .await
            .unwrap()
            .unwrap()
            .status(),
        DeviceStatus::Pending
    );
    assert_eq!(
        auth.revoke_credential_at(
            other_owner,
            credential.device_id,
            credential.credential_id,
            at(3)
        )
        .await,
        Err(DeviceAuthError::DeviceNotFound)
    );
    assert_eq!(
        auth.revoke_credential_at(owner, expired.device_id, credential.credential_id, at(3))
            .await,
        Err(DeviceAuthError::DeviceNotFound)
    );

    let additional = auth
        .create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(credential.device_id),
            at(4),
        )
        .await
        .unwrap();
    let second = auth.exchange_at(&additional.token, at(5)).await.unwrap();
    assert_ne!(credential.secret, second.secret);
    auth.revoke_credential_at(owner, credential.device_id, credential.credential_id, at(6))
        .await
        .unwrap();
    auth.revoke_credential_at(owner, credential.device_id, credential.credential_id, at(7))
        .await
        .unwrap();
    assert_eq!(
        auth.authenticate(&credential.secret).await,
        Err(DeviceAuthError::DeviceRevoked)
    );
    assert!(auth.authenticate(&second.secret).await.is_ok());

    // A valid unrevoked credential is rejected for a non-ACTIVE Device and
    // disabled owner on the very next call, independently of credential rows.
    sqlx::query("UPDATE devices SET status = 'PAUSED' WHERE id = $1")
        .bind(credential.device_id.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    assert_eq!(
        auth.authenticate(&second.secret).await,
        Err(DeviceAuthError::InvalidCredential)
    );
    assert_eq!(
        auth.create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(credential.device_id),
            at(8)
        )
        .await
        .unwrap_err(),
        DeviceAuthError::DeviceNotFound
    );
    sqlx::query("UPDATE devices SET status = 'ACTIVE' WHERE id = $1")
        .bind(credential.device_id.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET status = 'DISABLED' WHERE id = $1")
        .bind(owner.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    assert_eq!(
        auth.authenticate(&second.secret).await,
        Err(DeviceAuthError::InvalidCredential)
    );
    sqlx::query("UPDATE users SET status = 'ACTIVE' WHERE id = $1")
        .bind(owner.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    assert!(auth.authenticate(&second.secret).await.is_ok());

    // Simulate committed issuance whose response never reaches its claimant.
    let lost = auth
        .create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(credential.device_id),
            at(9),
        )
        .await
        .unwrap();
    drop(auth.exchange_at(&lost.token, at(10)).await.unwrap());
    assert_eq!(
        auth.exchange_at(&lost.token, at(11)).await.unwrap_err(),
        DeviceAuthError::InvalidEnrollment
    );
    let outstanding = auth
        .create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(credential.device_id),
            at(12),
        )
        .await
        .unwrap();
    assert_eq!(
        auth.revoke_all_credentials_at(other_owner, credential.device_id, at(13))
            .await,
        Err(DeviceAuthError::DeviceNotFound)
    );
    auth.revoke_all_credentials_at(owner, credential.device_id, at(13))
        .await
        .unwrap();
    assert_eq!(
        auth.authenticate(&second.secret).await,
        Err(DeviceAuthError::DeviceRevoked)
    );
    assert_eq!(
        auth.exchange_at(&outstanding.token, at(14))
            .await
            .unwrap_err(),
        DeviceAuthError::InvalidEnrollment
    );
    assert_eq!(
        DomainRepository::new(&pool)
            .find_device(credential.device_id)
            .await
            .unwrap()
            .unwrap()
            .status(),
        DeviceStatus::Active
    );
    let recovery = auth
        .create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(credential.device_id),
            at(15),
        )
        .await
        .unwrap();
    let recovered = auth.exchange_at(&recovery.token, at(16)).await.unwrap();
    assert_eq!(recovered.device_id, credential.device_id);
    assert!(auth.authenticate(&recovered.secret).await.is_ok());

    // Device-only revocation is sufficient; no positive auth cache and no
    // reliance on updating every credential's revoked_at is permitted.
    sqlx::query("UPDATE devices SET status = 'REVOKED' WHERE id = $1")
        .bind(credential.device_id.into_uuid())
        .execute(&inspection)
        .await
        .unwrap();
    assert_eq!(
        auth.authenticate(&recovered.secret).await,
        Err(DeviceAuthError::DeviceRevoked)
    );
    let credential_revoked: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT revoked_at FROM device_credentials WHERE credential_id = $1")
            .bind(recovered.credential_id.into_uuid())
            .fetch_one(&inspection)
            .await
            .unwrap();
    assert!(credential_revoked.is_none());
    assert_eq!(
        auth.create_grant_at(
            owner,
            DeviceEnrollmentTarget::Existing(credential.device_id),
            at(17)
        )
        .await
        .unwrap_err(),
        DeviceAuthError::DeviceNotFound
    );
    auth.revoke_device_at(owner, credential.device_id, at(18))
        .await
        .unwrap();
    assert_eq!(
        auth.authenticate(&recovered.secret).await,
        Err(DeviceAuthError::DeviceRevoked)
    );
    inspection.close().await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_device_enrollment_concurrent_exchange_has_one_winner() {
    let (pool, inspection, owner, _) = fixture().await;
    let auth = DeviceAuthenticationService::new(&pool);
    let grant = auth
        .create_grant_at(
            owner,
            DeviceEnrollmentTarget::New(name("Concurrent desktop")),
            at(0),
        )
        .await
        .unwrap();
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL").unwrap();
    let independent = DatabasePool::connect(&DatabaseConfig::from_url(url).unwrap())
        .await
        .unwrap();
    let competing_auth = DeviceAuthenticationService::new(&independent);
    let results = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            auth.exchange_at(&grant.token, at(1)),
            competing_auth.exchange_at(&grant.token, at(1))
        )
    })
    .await
    .expect("exchange race must not deadlock");
    let winner = match results {
        (Ok(winner), Err(DeviceAuthError::InvalidEnrollment))
        | (Err(DeviceAuthError::InvalidEnrollment), Ok(winner)) => winner,
        other => panic!("exactly one enrollment exchange must succeed: {other:?}"),
    };
    assert!(auth.authenticate(&winner.secret).await.is_ok());
    let (count, consumed): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM device_credentials),
                (SELECT count(*) FROM device_enrollment_grants WHERE consumed_at IS NOT NULL)",
    )
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!((count, consumed), (1, 1));
    let device = DomainRepository::new(&pool)
        .find_device(grant.device_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(device.status(), DeviceStatus::Active);
    assert_eq!(device.revision().get(), 1);
    independent.close().await;
    inspection.close().await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_enrollment_expiry_is_rechecked_after_waiting_for_device_lock() {
    let (pool, inspection, owner, _) = fixture().await;
    let auth = DeviceAuthenticationService::new(&pool);
    // Exercise the production clock path. The grant remains live when the
    // exchange starts, but expires while a real competing transaction holds
    // its Device lock. A request-start timestamp is not consume authority.
    let created = Timestamp::from_offset_datetime(
        time::OffsetDateTime::now_utc() - time::Duration::seconds(598),
    );
    let grant = auth
        .create_grant_at(
            owner,
            DeviceEnrollmentTarget::New(name("Queued desktop")),
            created,
        )
        .await
        .unwrap();
    let mut blocker = inspection.begin().await.unwrap();
    sqlx::query("SELECT id FROM devices WHERE id = $1 FOR UPDATE")
        .bind(grant.device_id.into_uuid())
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    assert!(Timestamp::now() < grant.expires_at);
    let exchange_pool = pool.clone();
    let token = grant.token.clone();
    let exchange = tokio::spawn(async move {
        DeviceAuthenticationService::new(&exchange_pool)
            .exchange(&token)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database() AND wait_event_type = 'Lock'
                   AND query LIKE '%FROM devices WHERE id =%FOR UPDATE%'",
            )
            .fetch_one(&inspection)
            .await
            .unwrap();
            if waiting != 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("exchange must actually wait on the competing Device lock");
    assert!(!exchange.is_finished());
    let remaining = (grant.expires_at.as_offset_datetime() - time::OffsetDateTime::now_utc())
        .whole_milliseconds()
        .max(0) as u64;
    tokio::time::sleep(Duration::from_millis(remaining + 25)).await;
    assert!(Timestamp::now() >= grant.expires_at);
    blocker.commit().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(result, Err(DeviceAuthError::InvalidEnrollment)),
        "expiry must be checked after all authorization locks are acquired"
    );
    let (credentials, consumed): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM device_credentials WHERE device_id = $1),
                (SELECT count(*) FROM device_enrollment_grants WHERE device_id = $1 AND consumed_at IS NOT NULL)",
    )
    .bind(grant.device_id.into_uuid())
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!((credentials, consumed), (0, 0));
    assert_eq!(
        DomainRepository::new(&pool)
            .find_device(grant.device_id)
            .await
            .unwrap()
            .unwrap()
            .status(),
        DeviceStatus::Pending
    );
    inspection.close().await;
    pool.close().await;
}
