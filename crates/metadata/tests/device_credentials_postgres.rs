use synveil_core::{
    Device, DeviceCredentialId, DeviceCredentialSecret, DeviceEnrollmentGrantId, DeviceId,
    DeviceStatus, EnrollmentSecret, LogicalName, LoginIdentifier, Timestamp, User, UserId,
    UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DeviceCredentialRepository, DeviceCredentialRepositoryError,
    DomainRepository, MigrationRunner, NewDeviceEnrollmentGrant,
};

fn at(seconds: i64) -> Timestamp {
    let start = Timestamp::parse("2026-08-28T00:00:00.123456Z").unwrap();
    Timestamp::from_offset_datetime(start.as_offset_datetime() + time::Duration::seconds(seconds))
}

fn grant(owner: UserId, device: DeviceId, entropy: u8) -> NewDeviceEnrollmentGrant {
    NewDeviceEnrollmentGrant {
        grant_id: DeviceEnrollmentGrantId::new(),
        owner_user_id: owner,
        device_id: device,
        secret_digest: EnrollmentSecret::from_bytes([entropy; 32]).digest(),
        created_at: at(0),
        expires_at: at(600),
    }
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_device_credential_constraints_and_atomic_rollback() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable test database");
    let pool = DatabasePool::connect(&DatabaseConfig::from_url(&url).unwrap())
        .await
        .unwrap();
    let status = MigrationRunner::new().run(&pool).await.unwrap();
    assert!(status.is_current());
    assert!(status.applied_versions().contains(&20260828000000));
    let inspection = sqlx::PgPool::connect(&url).await.unwrap();
    let owner = UserId::new();
    let other_owner = UserId::new();
    for (id, label) in [(owner, "rollback-owner"), (other_owner, "foreign-owner")] {
        DomainRepository::new(&pool)
            .insert_user(&User::new(
                id,
                LoginIdentifier::new(label, format!("key:{label}")).unwrap(),
                UserStatus::Active,
                at(0),
            ))
            .await
            .unwrap();
    }
    let repository = DeviceCredentialRepository::new(&pool);
    let first = Device::new(
        DeviceId::new(),
        owner,
        LogicalName::new("first").unwrap(),
        at(0),
    );
    let initial = grant(owner, first.id(), 1);
    repository
        .create_grant(&initial, Some(&first))
        .await
        .unwrap();
    let first_credential = DeviceCredentialId::new();
    repository
        .consume_grant(
            &initial.secret_digest,
            first_credential,
            &DeviceCredentialSecret::from_bytes([2; 32]).digest(),
            at(1),
        )
        .await
        .unwrap();

    // A failing grant insert must roll back the simultaneously created Device.
    let rolled_back = Device::new(
        DeviceId::new(),
        owner,
        LogicalName::new("rolled back").unwrap(),
        at(0),
    );
    let mut duplicate_grant = grant(owner, rolled_back.id(), 3);
    duplicate_grant.grant_id = initial.grant_id;
    assert!(matches!(
        repository
            .create_grant(&duplicate_grant, Some(&rolled_back))
            .await,
        Err(DeviceCredentialRepositoryError::Persistence(_))
    ));
    assert!(
        DomainRepository::new(&pool)
            .find_device(rolled_back.id())
            .await
            .unwrap()
            .is_none()
    );

    let pending = Device::new(
        DeviceId::new(),
        owner,
        LogicalName::new("pending").unwrap(),
        at(0),
    );
    let retryable_grant = grant(owner, pending.id(), 4);
    repository
        .create_grant(&retryable_grant, Some(&pending))
        .await
        .unwrap();
    // Inject a late failure after the PENDING -> ACTIVE update: duplicate
    // credential ID causes insert failure before grant consumption commits.
    assert!(matches!(
        repository
            .consume_grant(
                &retryable_grant.secret_digest,
                first_credential,
                &DeviceCredentialSecret::from_bytes([5; 32]).digest(),
                at(1)
            )
            .await,
        Err(DeviceCredentialRepositoryError::Persistence(_))
    ));
    let after_failure = DomainRepository::new(&pool)
        .find_device(pending.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_failure.status(), DeviceStatus::Pending);
    assert_eq!(after_failure.revision().get(), 0);
    let consumed: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT consumed_at FROM device_enrollment_grants WHERE grant_id = $1")
            .bind(retryable_grant.grant_id.into_uuid())
            .fetch_one(&inspection)
            .await
            .unwrap();
    assert!(consumed.is_none());
    let second_credential = DeviceCredentialId::new();
    let second_secret = DeviceCredentialSecret::from_bytes([6; 32]);
    repository
        .consume_grant(
            &retryable_grant.secret_digest,
            second_credential,
            &second_secret.digest(),
            at(2),
        )
        .await
        .unwrap();
    let stored = repository
        .load_credential_by_digest(&second_secret.digest())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.credential_id, second_credential);
    assert_eq!(stored.owner_user_id, owner);
    assert_eq!(stored.device_id, pending.id());
    assert_eq!(stored.device_status, DeviceStatus::Active);
    assert_eq!(stored.secret_digest, second_secret.digest());
    assert!(!format!("{stored:?}").contains(second_secret.expose_secret()));
    assert_eq!(
        repository
            .consume_grant(
                &retryable_grant.secret_digest,
                DeviceCredentialId::new(),
                &DeviceCredentialSecret::from_bytes([7; 32]).digest(),
                at(3)
            )
            .await,
        Err(DeviceCredentialRepositoryError::InvalidEnrollment)
    );

    // The database itself enforces owner/device scope, not just the service.
    let cross_owner = sqlx::query(
        "INSERT INTO device_credentials (credential_id, owner_user_id, device_id, secret_digest, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    ).bind(DeviceCredentialId::new().into_uuid()).bind(other_owner.into_uuid())
        .bind(pending.id().into_uuid()).bind([8_u8; 32].as_slice()).bind(at(4).as_offset_datetime())
        .execute(&inspection).await;
    assert!(cross_owner.is_err());
    let invalid_digest = sqlx::query(
        "INSERT INTO device_credentials (credential_id, owner_user_id, device_id, secret_digest, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    ).bind(DeviceCredentialId::new().into_uuid()).bind(owner.into_uuid())
        .bind(pending.id().into_uuid()).bind([8_u8; 31].as_slice()).bind(at(4).as_offset_datetime())
        .execute(&inspection).await;
    assert!(invalid_digest.is_err());
    let invalid_ttl = sqlx::query(
        "INSERT INTO device_enrollment_grants
            (grant_id, owner_user_id, device_id, secret_digest, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(DeviceEnrollmentGrantId::new().into_uuid())
    .bind(owner.into_uuid())
    .bind(pending.id().into_uuid())
    .bind([9_u8; 32].as_slice())
    .bind(at(0).as_offset_datetime())
    .bind(at(901).as_offset_datetime())
    .execute(&inspection)
    .await;
    assert!(invalid_ttl.is_err());
    let credential_count: i64 = sqlx::query_scalar("SELECT count(*) FROM device_credentials")
        .fetch_one(&inspection)
        .await
        .unwrap();
    assert_eq!(credential_count, 2);
    let plaintext_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name IN ('device_credentials', 'device_enrollment_grants')
           AND data_type IN ('text', 'character varying')",
    )
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!(
        plaintext_columns, 0,
        "credential tables cannot store plaintext secret strings"
    );
    let snapshot: String = sqlx::query_scalar(
        "SELECT json_build_object('credentials', (SELECT json_agg(c) FROM device_credentials c),
                                  'grants', (SELECT json_agg(g) FROM device_enrollment_grants g))::TEXT",
    ).fetch_one(&inspection).await.unwrap();
    assert!(!snapshot.contains(second_secret.expose_secret()));
    assert!(!snapshot.contains(EnrollmentSecret::from_bytes([4; 32]).expose_secret()));
    inspection.close().await;
    pool.close().await;
}
