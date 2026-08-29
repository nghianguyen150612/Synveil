use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use synveil_core::{FileVersionId, LibraryId, NodeId, Sequence, SyncBootstrap, SyncBootstrapId};
use synveil_platform::{SecretStoreError, SecretValue, UnsupportedSecureSecretStore};

use super::*;
use crate::{
    BootstrapCompletion, BootstrapPage, EngineConfig, FilesystemLocalReplica, InboundSyncEngine,
    LOCAL_SCHEMA_VERSION, LocalReplica, LocalStateConfig, OpaqueEvidence, RemoteCheckpoint,
    RemoteContent, RemoteError, RemoteErrorKind, RemoteFeedPage, ReplicaScope, RootBindingId,
    SyncOutcome, SyncRemote,
    test_support::{read_file_bounded, remove_dir_all_bounded},
};

struct BoundFailingRemote {
    profile_id: ServerProfileId,
    credential_id: DeviceCredentialId,
    kind: RemoteErrorKind,
    calls: AtomicUsize,
}

impl BoundFailingRemote {
    fn fail<T>(&self) -> Result<T, RemoteError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(RemoteError::new(self.kind))
    }
}

#[async_trait::async_trait]
impl SyncRemote for BoundFailingRemote {
    fn server_profile_id(&self) -> Option<ServerProfileId> {
        Some(self.profile_id)
    }
    fn device_credential_id(&self) -> Option<DeviceCredentialId> {
        Some(self.credential_id)
    }
    async fn get_checkpoint(&self, _: ReplicaScope) -> Result<RemoteCheckpoint, RemoteError> {
        self.fail()
    }
    async fn fetch_changes(&self, _: ReplicaScope, _: u32) -> Result<RemoteFeedPage, RemoteError> {
        self.fail()
    }
    async fn acknowledge_changes(
        &self,
        _: ReplicaScope,
        _: &OpaqueEvidence,
    ) -> Result<RemoteCheckpoint, RemoteError> {
        self.fail()
    }
    async fn start_rebaseline(&self, _: ReplicaScope) -> Result<SyncBootstrap, RemoteError> {
        self.fail()
    }
    async fn fetch_rebaseline_page(
        &self,
        _: ReplicaScope,
        _: SyncBootstrapId,
        _: Option<&OpaqueEvidence>,
        _: u32,
    ) -> Result<BootstrapPage, RemoteError> {
        self.fail()
    }
    async fn complete_rebaseline(
        &self,
        _: ReplicaScope,
        _: SyncBootstrapId,
        _: &OpaqueEvidence,
    ) -> Result<BootstrapCompletion, RemoteError> {
        self.fail()
    }
    async fn download_current_content(
        &self,
        _: ReplicaScope,
        _: NodeId,
        _: FileVersionId,
    ) -> Result<RemoteContent, RemoteError> {
        self.fail()
    }
}

#[derive(Default)]
struct TestSecretStore {
    values: Mutex<BTreeMap<SecretName, SecretValue>>,
    fail_put: AtomicBool,
    fail_get: AtomicBool,
    fail_get_when_present: AtomicBool,
    fail_delete: AtomicBool,
}

impl TestSecretStore {
    fn count(&self) -> usize {
        self.values.lock().unwrap().len()
    }
}

impl SecretStore for TestSecretStore {
    fn state(&self) -> SecretStoreState {
        SecretStoreState::Available
    }
    fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
        if self.fail_put.load(Ordering::SeqCst) {
            return Err(SecretStoreError::Unavailable);
        }
        self.values
            .lock()
            .unwrap()
            .insert(name.clone(), SecretValue::new(value));
        Ok(())
    }
    fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
        if self.fail_get.load(Ordering::SeqCst)
            || (self.fail_get_when_present.load(Ordering::SeqCst)
                && self.values.lock().unwrap().contains_key(name))
        {
            return Err(SecretStoreError::Unavailable);
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(name)
            .map(|value| SecretValue::new(value.as_bytes())))
    }
    fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(SecretStoreError::Unavailable);
        }
        Ok(self.values.lock().unwrap().remove(name).is_some())
    }
}

fn database() -> (PathBuf, LocalStateConfig) {
    let directory = std::env::temp_dir().join(format!("synveil-profile-tests-{}", Uuid::now_v7()));
    fs::create_dir(&directory).unwrap();
    let config = LocalStateConfig::new(directory.join("state.sqlite3"));
    (directory, config)
}

fn profile(origin: &str) -> ServerProfile {
    ServerProfile::new(
        CanonicalBaseUrl::parse(origin).unwrap(),
        "Synthetic test profile",
    )
    .unwrap()
}

fn scope() -> ReplicaScope {
    ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new())
}

async fn enroll(
    state: &LocalStateStore,
    secret_store: &TestSecretStore,
    profile: &ServerProfile,
    scope: ReplicaScope,
    secret: &DeviceCredentialSecret,
) -> DeviceCredentialId {
    state.save_server_profile(profile).await.unwrap();
    let credential_id = DeviceCredentialId::new();
    state
        .store_device_enrollment(
            profile.profile_id(),
            scope.owner_user_id(),
            scope.device_id(),
            credential_id,
            secret,
            secret_store,
        )
        .await
        .unwrap();
    credential_id
}

async fn assert_no_secret_on_disk(state: &LocalStateStore, secrets: &[&DeviceCredentialSecret]) {
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&state.pool)
        .await
        .unwrap();
    state.close_pool().await;
    for entry in fs::read_dir(state.database_path().parent().unwrap()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Transient SQLite control files. After wal_checkpoint(TRUNCATE) the
            // WAL is zero bytes and SHM stores only lock metadata, never page
            // data, so plaintext secrets can never be durably hidden there. On
            // Windows these descriptors (and the fs2 writer-lock target) are
            // byte-range locked for the process lifetime (ERROR_LOCK_VIOLATION),
            // so they are skipped here; the durable main database and every
            // other file are still scanned.
            if name.ends_with("-wal")
                || name.ends_with("-shm")
                || name.ends_with("sqlite3.writer.lock")
            {
                continue;
            }
            let bytes = read_file_bounded(&entry.path()).unwrap();
            for secret in secrets {
                assert!(
                    !bytes
                        .windows(secret.expose_secret().len())
                        .any(|window| window == secret.expose_secret().as_bytes())
                );
            }
        }
    }
    let debug = format!("{state:?}");
    for secret in secrets {
        assert!(!debug.contains(secret.expose_secret()));
    }
}

#[test]
fn canonical_url_normalizes_scheme_host_port_ipv6_and_idna() {
    for (raw, canonical) in [
        ("HTTPS://EXAMPLE.COM:443", "https://example.com/"),
        ("https://example.com/", "https://example.com/"),
        ("https://[2001:DB8::1]:8443", "https://[2001:db8::1]:8443/"),
        ("https://bücher.example", "https://xn--bcher-kva.example/"),
    ] {
        assert_eq!(CanonicalBaseUrl::parse(raw).unwrap().as_str(), canonical);
    }
}

#[test]
fn canonical_url_rejects_userinfo_non_https_paths_queries_fragments_and_invalid_ports() {
    for raw in [
        "http://example.com",
        "http://127.0.0.1",
        "ftp://example.com",
        "file:///tmp",
        "https://user:password@example.com",
        "https://user@example.com",
        "https://@example.com",
        "https://:password@example.com",
        "https://example.com/api",
        "https://example.com/.",
        "https://example.com/api/../",
        "https://example.com/%2e",
        "https://example.com:/./",
        "https://-example.com",
        "https://example-.com",
        "https://exa_mple.com",
        "https://example..com",
        "https://example.com/?",
        "https://example.com?token=synthetic",
        "https://example.com/#",
        "https://example.com/#fragment",
        "https://example.com:invalid",
        "https://example.com:65536",
        "https://example.com:0",
        "https://example.com:",
        "https://example.com:/",
        "https://",
        "https://[broken]",
        "https://exa mple.com",
        " https://example.com",
        "https://example.com\n",
        "https:\\example.com",
        "https:example.com",
        "//example.com",
        "https://exa\tmple.com",
    ] {
        assert!(
            CanonicalBaseUrl::parse(raw).is_err(),
            "accepted invalid URL configuration"
        );
    }
    let invalid =
        CanonicalBaseUrl::parse("https://user:synthetic-private-value@example.com").unwrap_err();
    assert!(!format!("{invalid:?} {invalid}").contains("synthetic-private-value"));
}

#[test]
fn http_exception_is_explicit_and_limited_to_numeric_loopback() {
    for raw in [
        "http://127.0.0.1:12345",
        "http://[::1]:12345",
        "http://127.0.0.2",
    ] {
        assert!(CanonicalBaseUrl::parse(raw).is_err());
        assert!(
            CanonicalBaseUrl::parse_for_loopback_test(raw)
                .unwrap()
                .is_loopback_test_http()
        );
    }
    for raw in [
        "http://localhost",
        "http://0.0.0.0",
        "http://example.com",
        "http://192.168.1.1",
        "http://[::]",
    ] {
        assert!(CanonicalBaseUrl::parse_for_loopback_test(raw).is_err());
    }
    assert!(
        !CanonicalBaseUrl::parse_for_loopback_test("https://example.com")
            .unwrap()
            .is_loopback_test_http()
    );
}

#[test]
fn profile_identifiers_and_labels_are_bounded_non_secret_types() {
    let id = ServerProfileId::new();
    assert_eq!(ServerProfileId::parse_str(&id.to_string()).unwrap(), id);
    for value in [
        "not-an-id",
        "00000000-0000-0000-0000-000000000000",
        "018f12b8-558a-7000-0000-000000000000",
    ] {
        assert!(ServerProfileId::parse_str(value).is_err());
    }
    let origin = CanonicalBaseUrl::parse("https://example.com").unwrap();
    for value in [
        "".to_string(),
        " ".to_string(),
        "a".repeat(257),
        "label\n".to_string(),
    ] {
        assert!(ServerProfile::new(origin.clone(), value).is_err());
    }
}

#[tokio::test]
async fn profile_and_secret_survive_reopen_without_plaintext_in_sqlite_or_debug() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server-a.example");
    let scope = scope();
    let secret = DeviceCredentialSecret::from_bytes([0xa1; 32]);
    let credential_id = enroll(&state, &store, &profile, scope, &secret).await;
    assert_eq!(state.schema_version().await.unwrap(), LOCAL_SCHEMA_VERSION);
    assert_eq!(
        state.server_profile(profile.profile_id()).await.unwrap(),
        Some(profile.clone())
    );
    state
        .mark_profile_connected(profile.profile_id())
        .await
        .unwrap();
    let loaded = state
        .load_device_credential(profile.profile_id(), &store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.profile_id(), profile.profile_id());
    assert_eq!(loaded.owner_user_id(), scope.owner_user_id());
    assert_eq!(loaded.device_id(), scope.device_id());
    assert_eq!(loaded.credential_id(), credential_id);
    assert_eq!(loaded.secret(), &secret);
    assert!(!format!("{loaded:?} {profile:?}").contains(secret.expose_secret()));
    assert_no_secret_on_disk(&state, &[&secret]).await;
    drop(loaded);
    drop(state);
    let state = LocalStateStore::open(&config).await.unwrap();
    assert_eq!(state.server_profiles().await.unwrap().len(), 1);
    assert!(
        state
            .server_profile(profile.profile_id())
            .await
            .unwrap()
            .unwrap()
            .last_connected_at_ms()
            .is_some()
    );
    assert_eq!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret
    );
    assert_no_secret_on_disk(&state, &[&secret]).await;
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn multiple_profiles_and_origin_aliases_never_share_credential_keys() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile_a = profile("https://server-a.example");
    let profile_b = profile("https://server-b.example");
    let profile_alias = profile("https://SERVER-A.example:443/");
    let secret_a = DeviceCredentialSecret::from_bytes([0xa2; 32]);
    let secret_b = DeviceCredentialSecret::from_bytes([0xb2; 32]);
    enroll(&state, &store, &profile_a, scope(), &secret_a).await;
    enroll(&state, &store, &profile_b, scope(), &secret_b).await;
    state.save_server_profile(&profile_alias).await.unwrap();
    assert!(
        state
            .load_device_credential(profile_alias.profile_id(), &store)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        state
            .load_device_credential(profile_a.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret_a
    );
    assert_eq!(
        state
            .load_device_credential(profile_b.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret_b
    );
    assert_eq!(store.count(), 2);
    for key in store.values.lock().unwrap().keys() {
        assert!(!key.as_str().contains("server-"));
        assert!(key.as_str().starts_with("desktop-device-v1/"));
    }
    state
        .forget_device_credential(profile_a.profile_id(), &store)
        .await
        .unwrap();
    assert_eq!(store.count(), 1);
    assert!(
        state
            .load_device_credential(profile_a.profile_id(), &store)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        state
            .load_device_credential(profile_b.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret_b
    );
    assert_eq!(state.server_profiles().await.unwrap().len(), 3);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn enrollment_receipt_cannot_be_imported_under_another_profile_or_origin() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let a = profile("https://server-a.example");
    let b = profile("https://server-b.example");
    let scope = scope();
    let secret = DeviceCredentialSecret::from_bytes([0xbd; 32]);
    let receipt = EnrollmentCredentials::for_test(
        a.clone(),
        scope.owner_user_id(),
        scope.device_id(),
        DeviceCredentialId::new(),
        secret.clone(),
        synveil_core::Timestamp::now(),
    );
    state.save_server_profile(&b).await.unwrap();
    assert!(matches!(
        state.store_enrollment(&receipt, &store).await,
        Err(ClientSyncError::InvalidServerProfile)
    ));
    assert_eq!(store.count(), 0);
    state.save_server_profile(&a).await.unwrap();
    // Test-only access simulates a corrupted receipt. Production callers have
    // no constructor/setter for either the receipt or its immutable identity.
    let mut forged = a.clone();
    forged.base_url = b.base_url().clone();
    let forged = EnrollmentCredentials::for_test(
        forged,
        scope.owner_user_id(),
        scope.device_id(),
        DeviceCredentialId::new(),
        secret.clone(),
        synveil_core::Timestamp::now(),
    );
    assert!(matches!(
        state.store_enrollment(&forged, &store).await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert_eq!(store.count(), 0);
    assert!(
        state
            .profile_enrollment(a.profile_id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .profile_enrollment(b.profile_id())
            .await
            .unwrap()
            .is_none()
    );
    state.store_enrollment(&receipt, &store).await.unwrap();
    assert_eq!(
        state
            .load_device_credential(a.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret
    );
    assert!(
        state
            .load_device_credential(b.profile_id(), &store)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        state.replace_enrollment(&forged, &store).await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert_eq!(store.count(), 1);
    assert_no_secret_on_disk(&state, &[&secret]).await;
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn reconstructed_sqlite_cannot_load_overwrite_or_forget_another_origins_secret() {
    let (directory_a, config_a) = database();
    let (directory_b, config_b) = database();
    let state_a = LocalStateStore::open(&config_a).await.unwrap();
    let state_b = LocalStateStore::open(&config_b).await.unwrap();
    let store = TestSecretStore::default();
    let profile_a = profile("https://server-a.example");
    let scope = scope();
    let secret_a = DeviceCredentialSecret::from_bytes([0xc1; 32]);
    let credential_id = enroll(&state_a, &store, &profile_a, scope, &secret_a).await;
    // A new independent database can reconstruct arbitrary non-secret IDs.
    // Its trigger never saw the original origin, so SQLite alone cannot bind
    // the credential; the trusted envelope must reject this substitution.
    let mut profile_b = profile_a.clone();
    profile_b.base_url = CanonicalBaseUrl::parse("https://server-b.example").unwrap();
    state_b.save_server_profile(&profile_b).await.unwrap();
    let conflicting_receipt = EnrollmentCredentials::for_test(
        profile_b.clone(),
        scope.owner_user_id(),
        scope.device_id(),
        credential_id,
        DeviceCredentialSecret::from_bytes([0xc2; 32]),
        synveil_core::Timestamp::now(),
    );
    assert!(matches!(
        state_b.store_enrollment(&conflicting_receipt, &store).await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert_eq!(store.count(), 1);
    assert!(
        state_b
            .profile_enrollment(profile_b.profile_id())
            .await
            .unwrap()
            .is_none()
    );
    let original = state_a
        .profile_enrollment(profile_a.profile_id())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("INSERT INTO profile_device_enrollments(profile_id, owner_user_id, device_id, credential_id, completed_at_ms) VALUES (?,?,?,?,?)")
        .bind(original.profile_id().to_string()).bind(original.owner_user_id().to_string())
        .bind(original.device_id().to_string()).bind(original.credential_id().to_string())
        .bind(original.completed_at_ms()).execute(&state_b.pool).await.unwrap();
    assert!(matches!(
        state_b
            .load_device_credential(profile_b.profile_id(), &store)
            .await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(matches!(
        state_b
            .forget_device_credential(profile_b.profile_id(), &store)
            .await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert_eq!(store.count(), 1);
    assert_eq!(
        state_a
            .load_device_credential(profile_a.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret_a
    );
    assert_no_secret_on_disk(&state_a, &[&secret_a]).await;
    assert_no_secret_on_disk(&state_b, &[&secret_a]).await;
    state_a.close_pool().await;
    state_b.close_pool().await;
    drop(state_a);
    drop(state_b);
    remove_dir_all_bounded(&directory_a).unwrap();
    remove_dir_all_bounded(&directory_b).unwrap();
}

#[tokio::test]
async fn loaded_secure_origin_cannot_be_relabelled_in_http_constructor() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile_a = profile("https://server-a.example");
    let scope = scope();
    let secret = DeviceCredentialSecret::from_bytes([0xc3; 32]);
    enroll(&state, &store, &profile_a, scope, &secret).await;
    let loaded = state
        .load_device_credential(profile_a.profile_id(), &store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.base_url(), profile_a.base_url());
    let mut profile_b = profile_a.clone();
    profile_b.base_url = CanonicalBaseUrl::parse("https://server-b.example").unwrap();
    assert!(
        matches!(crate::HttpSyncRemote::new(profile_b, scope.device_id(), loaded, crate::HttpClientConfig::default()),
        Err(error) if error.kind() == RemoteErrorKind::Rejected)
    );
    assert_eq!(store.count(), 1);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn secure_envelope_requires_version_shape_origin_and_every_bound_identity() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let scope = scope();
    let secret = DeviceCredentialSecret::from_bytes([0xc4; 32]);
    let credential_id = enroll(&state, &store, &profile, scope, &secret).await;
    let name = credential_secret_name(profile.profile_id(), credential_id).unwrap();
    let original = store.get_secret(&name).unwrap().unwrap();
    let encoded: serde_json::Value = serde_json::from_slice(original.as_bytes()).unwrap();
    for (field, value) in [
        ("version", serde_json::json!(2)),
        (
            "profile_id",
            serde_json::json!(ServerProfileId::new().to_string()),
        ),
        (
            "owner_user_id",
            serde_json::json!(UserId::new().to_string()),
        ),
        ("device_id", serde_json::json!(DeviceId::new().to_string())),
        (
            "credential_id",
            serde_json::json!(DeviceCredentialId::new().to_string()),
        ),
        (
            "canonical_base_url",
            serde_json::json!("https://other.example/"),
        ),
        ("transport_policy", serde_json::json!("LOOPBACK_TEST_HTTP")),
        ("unknown_required_field", serde_json::json!(true)),
    ] {
        let mut changed = encoded.clone();
        changed[field] = value;
        store
            .put_secret(&name, &serde_json::to_vec(&changed).unwrap())
            .unwrap();
        let error = state
            .load_device_credential(profile.profile_id(), &store)
            .await
            .unwrap_err();
        assert!(!format!("{error:?} {error}").contains(secret.expose_secret()));
    }
    store
        .put_secret(&name, secret.expose_secret().as_bytes())
        .unwrap();
    assert!(matches!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    store
        .put_secret(&name, &[b'x'; MAX_SECRET_ENVELOPE_BYTES + 1])
        .unwrap();
    assert!(matches!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    store.put_secret(&name, original.as_bytes()).unwrap();
    assert_eq!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await
            .unwrap()
            .unwrap()
            .secret(),
        &secret
    );
    state
        .forget_device_credential(profile.profile_id(), &store)
        .await
        .unwrap();
    assert_eq!(store.count(), 0);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn forget_preserves_replica_progress_and_explicit_reenrollment_keeps_device_identity() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let scope = scope();
    let first = DeviceCredentialSecret::from_bytes([0xa3; 32]);
    let first_id = enroll(&state, &store, &profile, scope, &first).await;
    state
        .bind_replica_to_profile(scope, RootBindingId::new(), profile.profile_id())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE replicas SET applied_sequence=9, acknowledged_sequence=8 WHERE library_id=?",
    )
    .bind(scope.library_id().to_string())
    .execute(&state.pool)
    .await
    .unwrap();
    let replica = state.replica(scope.library_id()).await.unwrap().unwrap();
    state
        .forget_device_credential(profile.profile_id(), &store)
        .await
        .unwrap();
    assert_eq!(store.count(), 0);
    assert_eq!(
        state.replica(scope.library_id()).await.unwrap().unwrap(),
        replica
    );
    assert_eq!(replica.applied_sequence(), Sequence::new(9));
    let second = DeviceCredentialSecret::from_bytes([0xa4; 32]);
    assert!(matches!(
        state
            .store_device_enrollment(
                profile.profile_id(),
                scope.owner_user_id(),
                scope.device_id(),
                DeviceCredentialId::new(),
                &second,
                &store
            )
            .await,
        Err(ClientSyncError::CredentialReplacementRequired)
    ));
    assert!(matches!(
        state
            .replace_device_enrollment(
                profile.profile_id(),
                scope.owner_user_id(),
                DeviceId::new(),
                DeviceCredentialId::new(),
                &second,
                &store
            )
            .await,
        Err(ClientSyncError::WrongScope)
    ));
    assert!(matches!(
        state
            .replace_device_enrollment(
                profile.profile_id(),
                UserId::new(),
                scope.device_id(),
                DeviceCredentialId::new(),
                &second,
                &store
            )
            .await,
        Err(ClientSyncError::WrongScope)
    ));
    assert!(matches!(
        state
            .replace_device_enrollment(
                profile.profile_id(),
                scope.owner_user_id(),
                scope.device_id(),
                first_id,
                &second,
                &store
            )
            .await,
        Err(ClientSyncError::CredentialReplacementRequired)
    ));
    let second_id = DeviceCredentialId::new();
    state
        .replace_device_enrollment(
            profile.profile_id(),
            scope.owner_user_id(),
            scope.device_id(),
            second_id,
            &second,
            &store,
        )
        .await
        .unwrap();
    let loaded = state
        .load_device_credential(profile.profile_id(), &store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.credential_id(), second_id);
    assert_eq!(loaded.device_id(), scope.device_id());
    assert_eq!(loaded.secret(), &second);
    assert_eq!(store.count(), 1);
    assert_eq!(
        state.replica(scope.library_id()).await.unwrap().unwrap(),
        replica
    );
    assert_no_secret_on_disk(&state, &[&first, &second]).await;
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn unavailable_secret_store_never_falls_back_to_plaintext() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let profile = profile("https://server.example");
    let secret = DeviceCredentialSecret::from_bytes([0xa5; 32]);
    state.save_server_profile(&profile).await.unwrap();
    let result = state
        .store_device_enrollment(
            profile.profile_id(),
            UserId::new(),
            DeviceId::new(),
            DeviceCredentialId::new(),
            &secret,
            &UnsupportedSecureSecretStore::new(),
        )
        .await;
    assert!(matches!(
        result,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    assert!(
        state
            .profile_enrollment(profile.profile_id())
            .await
            .unwrap()
            .is_none()
    );
    assert_no_secret_on_disk(&state, &[&secret]).await;
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn interrupted_first_store_has_only_non_secret_cleanup_intent() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let secret = DeviceCredentialSecret::from_bytes([0xa6; 32]);
    state.save_server_profile(&profile).await.unwrap();
    store.fail_get_when_present.store(true, Ordering::SeqCst);
    assert!(matches!(
        state
            .store_device_enrollment(
                profile.profile_id(),
                UserId::new(),
                DeviceId::new(),
                DeviceCredentialId::new(),
                &secret,
                &store
            )
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    assert_eq!(store.count(), 1);
    assert!(
        state
            .profile_enrollment(profile.profile_id())
            .await
            .unwrap()
            .is_none()
    );
    assert_no_secret_on_disk(&state, &[&secret]).await;
    drop(state);
    store.fail_get_when_present.store(false, Ordering::SeqCst);
    let state = LocalStateStore::open(&config).await.unwrap();
    assert!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.count(), 0);
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM profile_secret_cleanup")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(pending, 0);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn failed_put_never_marks_enrollment_completed_and_can_be_retried_locally() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let secret = DeviceCredentialSecret::from_bytes([0xa7; 32]);
    let scope = scope();
    state.save_server_profile(&profile).await.unwrap();
    store.fail_put.store(true, Ordering::SeqCst);
    let id = DeviceCredentialId::new();
    assert!(matches!(
        state
            .store_device_enrollment(
                profile.profile_id(),
                scope.owner_user_id(),
                scope.device_id(),
                id,
                &secret,
                &store
            )
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    assert_eq!(store.count(), 0);
    assert!(
        state
            .profile_enrollment(profile.profile_id())
            .await
            .unwrap()
            .is_none()
    );
    store.fail_put.store(false, Ordering::SeqCst);
    state
        .store_device_enrollment(
            profile.profile_id(),
            scope.owner_user_id(),
            scope.device_id(),
            id,
            &secret,
            &store,
        )
        .await
        .unwrap();
    assert_eq!(store.count(), 1);
    assert_no_secret_on_disk(&state, &[&secret]).await;
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn failed_forget_is_durable_and_restart_never_reloads_the_old_credential() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let secret = DeviceCredentialSecret::from_bytes([0xa8; 32]);
    enroll(&state, &store, &profile, scope(), &secret).await;
    store.fail_delete.store(true, Ordering::SeqCst);
    assert!(matches!(
        state
            .forget_device_credential(profile.profile_id(), &store)
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    assert!(
        state
            .profile_enrollment(profile.profile_id())
            .await
            .unwrap()
            .unwrap()
            .forgotten_at_ms()
            .is_some()
    );
    assert_eq!(store.count(), 1);
    drop(state);
    let state = LocalStateStore::open(&config).await.unwrap();
    assert!(matches!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    store.fail_delete.store(false, Ordering::SeqCst);
    state
        .forget_device_credential(profile.profile_id(), &store)
        .await
        .unwrap();
    assert!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.count(), 0);
    assert_no_secret_on_disk(&state, &[&secret]).await;
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn replacement_cleanup_is_replayed_after_restart_without_changing_active_identity() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let scope = scope();
    let first = DeviceCredentialSecret::from_bytes([0xa9; 32]);
    enroll(&state, &store, &profile, scope, &first).await;
    let second = DeviceCredentialSecret::from_bytes([0xaa; 32]);
    let second_id = DeviceCredentialId::new();
    store.fail_delete.store(true, Ordering::SeqCst);
    assert!(matches!(
        state
            .replace_device_enrollment(
                profile.profile_id(),
                scope.owner_user_id(),
                scope.device_id(),
                second_id,
                &second,
                &store
            )
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    assert_eq!(store.count(), 2);
    assert_eq!(
        state
            .profile_enrollment(profile.profile_id())
            .await
            .unwrap()
            .unwrap()
            .credential_id(),
        second_id
    );
    drop(state);
    let state = LocalStateStore::open(&config).await.unwrap();
    assert!(matches!(
        state
            .load_device_credential(profile.profile_id(), &store)
            .await,
        Err(ClientSyncError::SecureStoreUnavailable)
    ));
    store.fail_delete.store(false, Ordering::SeqCst);
    let loaded = state
        .load_device_credential(profile.profile_id(), &store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.credential_id(), second_id);
    assert_eq!(loaded.secret(), &second);
    assert_eq!(store.count(), 1);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn simultaneous_initial_stores_cannot_silently_replace_credentials() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let scope = scope();
    let first = DeviceCredentialSecret::from_bytes([0xab; 32]);
    let second = DeviceCredentialSecret::from_bytes([0xac; 32]);
    state.save_server_profile(&profile).await.unwrap();
    let (a, b) = tokio::join!(
        state.store_device_enrollment(
            profile.profile_id(),
            scope.owner_user_id(),
            scope.device_id(),
            DeviceCredentialId::new(),
            &first,
            &store
        ),
        state.store_device_enrollment(
            profile.profile_id(),
            scope.owner_user_id(),
            scope.device_id(),
            DeviceCredentialId::new(),
            &second,
            &store
        ),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(store.count(), 1);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn replica_profile_binding_is_durable_in_database_and_physical_root() {
    let (directory, config) = database();
    let state = LocalStateStore::open(&config).await.unwrap();
    let store = TestSecretStore::default();
    let a = profile("https://server-a.example");
    let b = profile("https://server-b.example");
    let scope = scope();
    enroll(
        &state,
        &store,
        &a,
        scope,
        &DeviceCredentialSecret::from_bytes([0xad; 32]),
    )
    .await;
    enroll(
        &state,
        &store,
        &b,
        scope,
        &DeviceCredentialSecret::from_bytes([0xae; 32]),
    )
    .await;
    let root = directory.join("managed");
    fs::create_dir(&root).unwrap();
    let replica =
        FilesystemLocalReplica::initialize_for_profile(&root, scope, a.profile_id()).unwrap();
    state
        .bind_replica_to_profile(scope, replica.binding_id(), a.profile_id())
        .await
        .unwrap();
    assert_eq!(
        state
            .replica(scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .server_profile_id(),
        Some(a.profile_id())
    );
    assert!(matches!(
        state
            .bind_replica_to_profile(scope, replica.binding_id(), b.profile_id())
            .await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(matches!(
        state.bind_replica(scope, replica.binding_id()).await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(FilesystemLocalReplica::open_for_profile(&root, scope, a.profile_id()).is_ok());
    assert!(matches!(
        FilesystemLocalReplica::open_for_profile(&root, scope, b.profile_id()),
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(matches!(
        FilesystemLocalReplica::open(&root, scope),
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(
        sqlx::query("UPDATE replicas SET server_profile_id=? WHERE library_id=?")
            .bind(b.profile_id().to_string())
            .bind(scope.library_id().to_string())
            .execute(&state.pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("UPDATE server_profiles SET canonical_base_url='https://changed.example/' WHERE profile_id=?")
        .bind(a.profile_id().to_string()).execute(&state.pool).await.is_err());
    drop(state);
    let state = LocalStateStore::open(&config).await.unwrap();
    assert_eq!(
        state
            .replica(scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .server_profile_id(),
        Some(a.profile_id())
    );
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn auth_revocation_and_offline_failures_preserve_files_sequences_and_pending_ack() {
    for kind in [
        RemoteErrorKind::AuthRequired,
        RemoteErrorKind::DeviceRevoked,
        RemoteErrorKind::Offline,
        RemoteErrorKind::Unavailable,
    ] {
        let (directory, config) = database();
        let state = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let store = TestSecretStore::default();
        let profile = profile("https://server.example");
        let scope = scope();
        let id = enroll(
            &state,
            &store,
            &profile,
            scope,
            &DeviceCredentialSecret::from_bytes([0xba; 32]),
        )
        .await;
        let root = directory.join("managed");
        fs::create_dir(&root).unwrap();
        let replica = Arc::new(
            FilesystemLocalReplica::initialize_for_profile(&root, scope, profile.profile_id())
                .unwrap(),
        );
        let remote = Arc::new(BoundFailingRemote {
            profile_id: profile.profile_id(),
            credential_id: id,
            kind,
            calls: AtomicUsize::new(0),
        });
        let engine = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica,
            state.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        fs::write(root.join("preserved.txt"), b"unchanged local content").unwrap();
        sqlx::query("UPDATE replicas SET root_node_id=?, applied_sequence=5, acknowledged_sequence=4 WHERE library_id=?")
            .bind(NodeId::new().to_string()).bind(scope.library_id().to_string()).execute(&state.pool).await.unwrap();
        sqlx::query("INSERT INTO pending_acknowledgements(library_id,epoch,from_sequence,through_sequence,high_watermark,evidence,created_at_ms) VALUES (?,0,4,5,5,?,0)")
            .bind(scope.library_id().to_string()).bind(b"synthetic-ack-proof".as_slice()).execute(&state.pool).await.unwrap();
        let before = state.replica(scope.library_id()).await.unwrap().unwrap();
        let pending = state.pending_ack(scope.library_id()).await.unwrap();
        let result = engine.synchronize_once().await;
        if matches!(
            kind,
            RemoteErrorKind::Offline | RemoteErrorKind::Unavailable
        ) {
            assert!(matches!(result, Ok(SyncOutcome::Offline)));
        } else {
            assert!(matches!(result, Err(ClientSyncError::Remote(error)) if error.kind() == kind));
        }
        assert_eq!(remote.calls.load(Ordering::SeqCst), 1);
        let after = state.replica(scope.library_id()).await.unwrap().unwrap();
        assert_eq!(before.applied_sequence(), after.applied_sequence());
        assert_eq!(
            before.acknowledged_sequence(),
            after.acknowledged_sequence()
        );
        assert_eq!(before.journal_epoch(), after.journal_epoch());
        assert_eq!(
            state.pending_ack(scope.library_id()).await.unwrap(),
            pending
        );
        assert_eq!(
            fs::read(root.join("preserved.txt")).unwrap(),
            b"unchanged local content"
        );
        drop(engine);
        state.close_pool().await;
        drop(state);
        remove_dir_all_bounded(&directory).unwrap();
    }
}

#[tokio::test]
async fn live_engine_stops_before_network_after_local_forget_or_reenrollment() {
    let (directory, config) = database();
    let state = Arc::new(LocalStateStore::open(&config).await.unwrap());
    let store = TestSecretStore::default();
    let profile = profile("https://server.example");
    let scope = scope();
    let id = enroll(
        &state,
        &store,
        &profile,
        scope,
        &DeviceCredentialSecret::from_bytes([0xbb; 32]),
    )
    .await;
    let root = directory.join("managed");
    fs::create_dir(&root).unwrap();
    let replica = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&root, scope, profile.profile_id()).unwrap(),
    );
    let remote = Arc::new(BoundFailingRemote {
        profile_id: profile.profile_id(),
        credential_id: id,
        kind: RemoteErrorKind::AuthRequired,
        calls: AtomicUsize::new(0),
    });
    let engine = InboundSyncEngine::new(
        scope,
        remote.clone(),
        replica.clone(),
        state.clone(),
        EngineConfig::default(),
    )
    .await
    .unwrap();
    state
        .forget_device_credential(profile.profile_id(), &store)
        .await
        .unwrap();
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::AuthenticationRequired)
    ));
    assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    let new_id = DeviceCredentialId::new();
    state
        .replace_device_enrollment(
            profile.profile_id(),
            scope.owner_user_id(),
            scope.device_id(),
            new_id,
            &DeviceCredentialSecret::from_bytes([0xbc; 32]),
            &store,
        )
        .await
        .unwrap();
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::AuthenticationRequired)
    ));
    assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica,
            state.clone(),
            EngineConfig::default()
        )
        .await,
        Err(ClientSyncError::AuthenticationRequired)
    ));
    assert_eq!(
        state
            .replica(scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .applied_sequence(),
        Sequence::new(0)
    );
    assert_eq!(
        state
            .replica(scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .acknowledged_sequence(),
        Sequence::new(0)
    );
    assert_eq!(store.count(), 1);
    drop(engine);
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}

#[tokio::test]
async fn forward_migration_preserves_v1_and_refuses_inferred_profile_rebind() {
    let (directory, config) = database();
    let scope = scope();
    let binding_id = RootBindingId::new();
    let mut migrations = sqlx::migrate!("./migrations");
    migrations.migrations = std::borrow::Cow::Owned(
        migrations
            .iter()
            .filter(|migration| migration.version == 1)
            .cloned()
            .collect(),
    );
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(config.database_path())
            .create_if_missing(true),
    )
    .await
    .unwrap();
    migrations.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO replicas(library_id,owner_user_id,device_id,root_binding_id,created_at_ms,updated_at_ms) VALUES (?,?,?,?,0,0)")
        .bind(scope.library_id().to_string()).bind(scope.owner_user_id().to_string()).bind(scope.device_id().to_string())
        .bind(binding_id.to_string()).execute(&pool).await.unwrap();
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=1")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;
    let state = LocalStateStore::open(&config).await.unwrap();
    assert_eq!(state.schema_version().await.unwrap(), LOCAL_SCHEMA_VERSION);
    let migrated_checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=1")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(checksum, migrated_checksum);
    assert_eq!(
        state
            .replica(scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .server_profile_id(),
        None
    );
    let profile = profile("https://server.example");
    enroll(
        &state,
        &TestSecretStore::default(),
        &profile,
        scope,
        &DeviceCredentialSecret::from_bytes([0xaf; 32]),
    )
    .await;
    assert!(matches!(
        state
            .bind_replica_to_profile(scope, binding_id, profile.profile_id())
            .await,
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(state.bind_replica(scope, binding_id).await.is_ok());
    let root = directory.join("legacy-managed");
    fs::create_dir(&root).unwrap();
    FilesystemLocalReplica::initialize(&root, scope).unwrap();
    assert!(matches!(
        FilesystemLocalReplica::initialize_for_profile(&root, scope, profile.profile_id()),
        Err(ClientSyncError::WrongServerProfile)
    ));
    assert!(FilesystemLocalReplica::open(&root, scope).is_ok());
    state.close_pool().await;
    drop(state);
    remove_dir_all_bounded(&directory).unwrap();
}
