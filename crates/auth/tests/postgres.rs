use std::time::Duration;

use synveil_auth::{
    AuthError, AuthenticationService, PasswordHasherConfig, PasswordParameters,
    PasswordVerification, PlaintextPassword, SessionConfig, SessionId, SessionToken,
    StoredPasswordHash,
};
use synveil_core::{
    DedupDomainId, Library, LibraryId, LogicalName, LoginIdentifier, Node, NodeId, Revision,
    Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    AuthRepository, BootstrapState, DatabaseConfig, DatabaseError, DatabaseErrorKind, DatabasePool,
    DomainRepository, MetadataError, MigrationRunner, SessionRow, UserCredentialRow, UserRow,
};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn login(value: &str) -> LoginIdentifier {
    LoginIdentifier::new(value, format!("key:{value}")).expect("test login identifier is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

fn test_password_config() -> PasswordHasherConfig {
    PasswordHasherConfig::new(
        PasswordParameters::new(16 * 1_024, 2, 1, 32).expect("test Argon2id parameters are valid"),
    )
    .expect("test password config is valid")
}

/// This test requires a fresh, explicitly disposable PostgreSQL database. It
/// exercises rollback, separate-connection races, closure, and reconnect.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_authentication_bootstrap_is_atomic_and_race_safe() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(url).expect("test URL must use PostgreSQL");
    let primary = DatabasePool::connect(&config)
        .await
        .expect("primary test connection must succeed");
    let runner = MigrationRunner::new();
    let status = runner
        .run(&primary)
        .await
        .expect("auth migration must apply");
    assert!(status.is_current());

    let password_config = test_password_config();
    let winner = {
        let primary_auth = AuthenticationService::new(&primary, password_config);
        assert_eq!(
            primary_auth.bootstrap_state().await.unwrap(),
            BootstrapState::Open
        );

        let observed_at = timestamp("2026-08-22T00:00:00.123456Z");
        let failed_user_id = UserId::new();
        let failed_user = User::rehydrate_with_admin(
            failed_user_id,
            login("rollback-admin"),
            UserStatus::Active,
            true,
            observed_at,
            observed_at,
            Revision::new(0),
        );
        let failed_user_row = UserRow::from_domain(&failed_user).unwrap();
        let invalid_credential = UserCredentialRow::new(
            failed_user_id.into_uuid(),
            "x".repeat(4_097),
            observed_at.as_offset_datetime(),
            observed_at.as_offset_datetime(),
        );
        assert_eq!(
            AuthRepository::new(&primary)
                .create_first_admin(&failed_user_row, &invalid_credential)
                .await,
            Err(MetadataError::Database(DatabaseError::Failure(
                DatabaseErrorKind::QueryFailed,
            )))
        );
        assert_eq!(
            DomainRepository::new(&primary)
                .find_user(failed_user_id)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            primary_auth.bootstrap_state().await.unwrap(),
            BootstrapState::Open
        );

        let secondary = DatabasePool::connect(&config)
            .await
            .expect("secondary test connection must succeed");
        let secondary_auth = AuthenticationService::new(&secondary, password_config);
        let (first, second) = tokio::join!(
            primary_auth.create_first_admin(
                login("admin-a"),
                PlaintextPassword::new("alpha password").unwrap(),
                observed_at,
            ),
            secondary_auth.create_first_admin(
                login("admin-b"),
                PlaintextPassword::new("bravo password").unwrap(),
                observed_at,
            ),
        );

        let winner = match (first, second) {
            (Ok(user), Err(AuthError::BootstrapClosed))
            | (Err(AuthError::BootstrapClosed), Ok(user)) => user,
            other => panic!("expected exactly one bootstrap winner, got {other:?}"),
        };
        assert!(winner.is_instance_admin());
        assert_eq!(
            primary_auth.bootstrap_state().await.unwrap(),
            BootstrapState::Closed
        );

        let stored = primary_auth
            .load_password_hash(winner.id())
            .await
            .unwrap()
            .expect("winner credential must exist");
        let winner_password = if winner.login().value() == "admin-a" {
            "alpha password"
        } else {
            "bravo password"
        };
        assert_eq!(
            primary_auth
                .verify_password(&stored, &PlaintextPassword::new(winner_password).unwrap())
                .unwrap(),
            PasswordVerification::Verified {
                needs_rehash: false
            }
        );

        assert_eq!(
            primary_auth
                .create_first_admin(
                    login("retry-admin"),
                    PlaintextPassword::new("retry password").unwrap(),
                    observed_at,
                )
                .await,
            Err(AuthError::BootstrapClosed)
        );

        winner
    };

    primary.close().await;

    let reconnected = DatabasePool::connect(&config)
        .await
        .expect("reconnect must succeed");
    let reconnected_auth = AuthenticationService::new(&reconnected, password_config);
    assert_eq!(
        reconnected_auth.bootstrap_state().await.unwrap(),
        BootstrapState::Closed
    );
    assert!(
        reconnected_auth
            .load_password_hash(winner.id())
            .await
            .unwrap()
            .is_some()
    );

    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        name("auth-content-root"),
        timestamp("2026-08-22T00:00:02.123456Z"),
    );
    let library = Library::new(
        library_id,
        winner.id(),
        name("auth-content"),
        &root,
        DedupDomainId::new(),
        timestamp("2026-08-22T00:00:02.123456Z"),
    )
    .expect("test library root must satisfy domain invariants");
    DomainRepository::new(&reconnected)
        .insert_library_with_root(&library, &root)
        .await
        .expect("content owned by the winner must persist");

    let replacement = StoredPasswordHash::hash(
        password_config,
        &PlaintextPassword::new("replacement password").unwrap(),
    )
    .unwrap();
    reconnected_auth
        .replace_password_hash(
            winner.id(),
            &replacement,
            timestamp("2026-08-22T00:00:03.123456Z"),
        )
        .await
        .unwrap();
    assert_eq!(
        DomainRepository::new(&reconnected)
            .find_library(library.id())
            .await
            .unwrap(),
        Some(library)
    );
    reconnected.close().await;
}

/// This test requires a fresh, explicitly disposable PostgreSQL database. It
/// covers session persistence and service semantics without involving HTTP,
/// cookies, CSRF, or a scheduler.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_browser_sessions_are_opaque_expiring_and_revocable() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let runner = MigrationRunner::new();
    let status = runner
        .run(&pool)
        .await
        .expect("all forward migrations must apply");
    assert!(status.is_current());
    assert!(status.applied_versions().contains(&20260822000002));
    assert!(pool.table_exists("sessions").await.unwrap());

    let observed_at = timestamp("2026-08-22T01:00:00.123456Z");
    let session_config =
        SessionConfig::new(Duration::from_secs(1)).expect("one-second test TTL is valid");
    let auth =
        AuthenticationService::with_session_config(&pool, test_password_config(), session_config);
    let admin = auth
        .create_first_admin(
            login("session-admin"),
            PlaintextPassword::new("session password").unwrap(),
            observed_at,
        )
        .await
        .expect("bootstrap administrator must be created");
    assert_eq!(
        auth.bootstrap_state().await.unwrap(),
        BootstrapState::Closed
    );

    let content_root_id = NodeId::new();
    let content_library_id = LibraryId::new();
    let content_root = Node::new_root(
        content_root_id,
        content_library_id,
        name("session-content-root"),
        observed_at,
    );
    let content_library = Library::new(
        content_library_id,
        admin.id(),
        name("session-content"),
        &content_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("content library must satisfy domain invariants");
    DomainRepository::new(&pool)
        .insert_library_with_root(&content_library, &content_root)
        .await
        .expect("content history must persist before session operations");

    let repository = AuthRepository::new(&pool);
    let first = auth
        .authenticate_at(
            login("session-admin"),
            PlaintextPassword::new("session password").unwrap(),
            timestamp("2026-08-22T01:00:00.500000Z"),
        )
        .await
        .expect("valid credentials must create a session");
    let raw_token = first.token().to_hex();
    assert!(!format!("{first:?}").contains(&raw_token));
    let first_row = repository
        .load_session_by_token_digest(&first.token().digest())
        .await
        .unwrap()
        .expect("successful login must persist one session row");
    assert_eq!(first_row.token_digest, first.token().digest().to_vec());
    assert_ne!(first_row.token_digest, raw_token.as_bytes());
    assert_eq!(
        repository
            .count_sessions_for_user(admin.id())
            .await
            .unwrap(),
        1
    );

    let principal = auth
        .authenticate_session_at(first.token(), timestamp("2026-08-22T01:00:01.000000Z"))
        .await
        .expect("active raw token must resolve to a principal");
    assert_eq!(principal.user_id(), admin.id());
    assert!(principal.is_instance_admin());
    assert_eq!(principal.session_id(), first.session_id());

    let wrong_password = auth
        .authenticate_at(
            login("session-admin"),
            PlaintextPassword::new("wrong password").unwrap(),
            timestamp("2026-08-22T01:00:01.100000Z"),
        )
        .await;
    let unknown_identifier = auth
        .authenticate_at(
            login("unknown-session-user"),
            PlaintextPassword::new("wrong password").unwrap(),
            timestamp("2026-08-22T01:00:01.100000Z"),
        )
        .await;
    assert_eq!(wrong_password, Err(AuthError::InvalidCredentials));
    assert_eq!(unknown_identifier, Err(AuthError::InvalidCredentials));
    assert_eq!(
        repository
            .count_sessions_for_user(admin.id())
            .await
            .unwrap(),
        1
    );

    let expired = auth
        .authenticate_at(
            login("session-admin"),
            PlaintextPassword::new("session password").unwrap(),
            timestamp("2026-08-22T01:00:02.000000Z"),
        )
        .await
        .expect("a session can be created before its TTL elapses");
    assert_eq!(
        auth.authenticate_session_at(expired.token(), timestamp("2026-08-22T01:00:03.000000Z"),)
            .await,
        Err(AuthError::InvalidSession)
    );

    let revoked = auth
        .authenticate_at(
            login("session-admin"),
            PlaintextPassword::new("session password").unwrap(),
            timestamp("2026-08-22T01:00:04.000000Z"),
        )
        .await
        .expect("a second independent session must be created");
    auth.revoke_current_session_at(revoked.token(), timestamp("2026-08-22T01:00:04.100000Z"))
        .await
        .expect("revocation must persist");
    assert_eq!(
        auth.authenticate_session_at(revoked.token(), timestamp("2026-08-22T01:00:04.200000Z"),)
            .await,
        Err(AuthError::InvalidSession)
    );
    let revoked_digest = revoked.token().digest();
    assert!(
        repository
            .load_session_by_token_digest(&revoked_digest)
            .await
            .unwrap()
            .expect("revoked row remains durable until cleanup")
            .revoked_at
            .is_some()
    );

    let (concurrent_first, concurrent_second) = tokio::join!(
        auth.authenticate_at(
            login("session-admin"),
            PlaintextPassword::new("session password").unwrap(),
            timestamp("2026-08-22T01:00:05.000000Z"),
        ),
        auth.authenticate_at(
            login("session-admin"),
            PlaintextPassword::new("session password").unwrap(),
            timestamp("2026-08-22T01:00:05.000000Z"),
        ),
    );
    let concurrent_first = concurrent_first.expect("first concurrent login must succeed");
    let concurrent_second = concurrent_second.expect("second concurrent login must succeed");
    assert_ne!(
        concurrent_first.session_id(),
        concurrent_second.session_id()
    );
    assert_ne!(concurrent_first.token(), concurrent_second.token());
    assert_eq!(
        repository
            .count_sessions_for_user(admin.id())
            .await
            .unwrap(),
        5
    );

    let orphan = SessionRow::new(
        SessionId::new().into_uuid(),
        UserId::new().into_uuid(),
        SessionToken::generate().digest().to_vec(),
        observed_at.as_offset_datetime(),
        timestamp("2026-08-22T01:00:10.123456Z").as_offset_datetime(),
        None,
    );
    assert_eq!(
        repository.create_session(&orphan).await,
        Err(MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::QueryFailed,
        )))
    );

    let revoked_token = revoked.token().clone();
    pool.close().await;

    let reconnected = DatabasePool::connect(&config)
        .await
        .expect("reconnect must succeed");
    let reconnected_auth = AuthenticationService::with_session_config(
        &reconnected,
        test_password_config(),
        session_config,
    );
    assert_eq!(
        reconnected_auth.bootstrap_state().await.unwrap(),
        BootstrapState::Closed
    );
    assert_eq!(
        reconnected_auth
            .authenticate_session_at(&revoked_token, timestamp("2026-08-22T01:00:06.000000Z"),)
            .await,
        Err(AuthError::InvalidSession)
    );
    let cleaned = reconnected_auth
        .cleanup_sessions(timestamp("2026-08-22T01:00:20.000000Z"))
        .await
        .expect("cleanup boundary must be executable");
    assert!(cleaned >= 2);
    assert_eq!(
        DomainRepository::new(&reconnected)
            .find_library(content_library.id())
            .await
            .unwrap(),
        Some(content_library)
    );
    reconnected.close().await;
}
