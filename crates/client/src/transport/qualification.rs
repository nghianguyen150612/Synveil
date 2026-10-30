//! P018: real HTTPS/authentication with simulated route evidence, never a tailnet claim.
use super::{tests::*, *};
use UnavailableReason as R;
use serde_json::json;
use sqlx::{Connection, sqlite::SqliteConnectOptions};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::{collections::BTreeMap, sync::Arc};
use synveil_client_sync::{
    CanonicalBaseUrl, HttpEnrollmentClient, LocalStateConfig, LocalStateStore,
};
use synveil_core::{
    DeviceCredentialSecret, EnrollmentSecret,
    host_trust::{HostTrust, ManagedHostTrust},
};
use synveil_platform::{SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinSet,
};

#[derive(Default)]
struct MemorySecrets(Mutex<BTreeMap<SecretName, Vec<u8>>>);
impl SecretStore for MemorySecrets {
    fn state(&self) -> SecretStoreState {
        SecretStoreState::Available
    }
    fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
        self.0.lock().unwrap().insert(name.clone(), value.to_vec());
        Ok(())
    }
    fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .map(|v| SecretValue::new(v.as_slice())))
    }
    fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
        Ok(self.0.lock().unwrap().remove(name).is_some())
    }
}
struct HttpsFixture {
    profile: ServerProfile,
    scope: ReplicaScope,
    status: Arc<AtomicUsize>,
    spoof: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    bearers: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for HttpsFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl HttpsFixture {
    async fn new(server: &str, ipv6: bool) -> Option<Self> {
        Self::scoped(server, ipv6, None).await
    }
    async fn scoped(server: &str, ipv6: bool, scope: Option<ReplicaScope>) -> Option<Self> {
        let listener = match TcpListener::bind(if ipv6 { "[::1]:0" } else { "127.0.0.1:0" }).await {
            Ok(l) => l,
            Err(e)
                if ipv6
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
                    ) =>
            {
                return None;
            }
            Err(e) => panic!("fixture bind: {e}"),
        };
        assert!(listener.local_addr().unwrap().ip().is_loopback());
        let origin = format!("https://{}/", listener.local_addr().unwrap());
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
        let ca = params.self_signed(&ca_key).unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        let cert =
            rcgen::CertificateParams::new(vec![if ipv6 { "::1" } else { "127.0.0.1" }.into()])
                .unwrap()
                .signed_by(&key, &ca, &ca_key)
                .unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
        let network = uuid::Uuid::now_v7().to_string();
        let trust =
            ManagedHostTrust::from_ca(server.into(), network.clone(), origin.clone(), ca.pem(), 1)
                .unwrap();
        let profile = ServerProfile::new(
            CanonicalBaseUrl::parse(&origin).unwrap(),
            "P018 loopback fixture",
        )
        .unwrap()
        .with_managed_host_trust(HostTrust::Managed(trust))
        .unwrap();
        let scope = scope.unwrap_or_else(|| {
            ReplicaScope::new(
                uuid::Uuid::now_v7().to_string().parse().unwrap(),
                uuid::Uuid::now_v7().to_string().parse().unwrap(),
                uuid::Uuid::now_v7().to_string().parse().unwrap(),
            )
        });
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(uuid::Uuid::now_v7().as_bytes());
        bytes[16..].copy_from_slice(uuid::Uuid::now_v7().as_bytes());
        let secret = DeviceCredentialSecret::from_bytes(bytes);
        let status = Arc::new(AtomicUsize::new(200));
        let spoof = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let bearers = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let (st, sp, rq, br, ac) = (
            status.clone(),
            spoof.clone(),
            requests.clone(),
            bearers.clone(),
            active.clone(),
        );
        let server = server.to_owned();
        let task = tokio::spawn(async move {
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
            let mut children = JoinSet::new();
            loop {
                tokio::select! {
                    socket = listener.accept() => {
                        let (socket, _) = socket.unwrap();
                        let (acceptor, st, sp, rq, br, ac, server, network, secret) =
                            (acceptor.clone(), st.clone(), sp.clone(), rq.clone(), br.clone(), ac.clone(), server.clone(), network.clone(), secret.clone());
                        children.spawn(async move {
                            ac.fetch_add(1, Ordering::SeqCst);
                            let _active = Active(ac);
                            let Ok(mut socket) = acceptor.accept(socket).await else { return; };
                            let mut bytes = Vec::new();
                            loop {
                                let Ok(byte) = socket.read_u8().await else { return; };
                                bytes.push(byte);
                                assert!(bytes.len() < 16384);
                                if bytes.ends_with(b"\r\n\r\n") { break; }
                            }
                            let request = String::from_utf8(bytes).unwrap();
                            let lower = request.to_ascii_lowercase();
                            rq.fetch_add(1, Ordering::SeqCst);
                            let auth = lower.contains("authorization:");
                            if auth { br.fetch_add(1, Ordering::SeqCst); }
                            let enrollment = request.starts_with("POST /api/v1/device-enrollment/exchange ");
                            let ready = request.starts_with("GET /health/ready ");
                            let expected = format!("GET /api/v1/devices/{}/libraries/{}/checkpoint ", scope.device_id(), scope.library_id());
                            let (code, body) = if enrollment {
                                assert!(!auth);
                                (201, json!({"data":{"owner_user_id":scope.owner_user_id().to_string(),"device_id":scope.device_id().to_string(),"credential_id":uuid::Uuid::now_v7().to_string(),"device_credential":secret.expose_secret(),"created_at":"2026-08-28T00:00:00Z"},"meta":{"request_id":"p018-fixture"}}))
                            } else if ready {
                                assert!(!auth);
                                (200, json!({"status":"ready"}))
                            } else {
                                assert!(request.starts_with("GET "), "qualification is read only");
                                let code = if !request.starts_with(&expected) || !request.contains(&format!("Bearer {}",secret.expose_secret())) { 401 } else { st.load(Ordering::SeqCst) };
                                if code == 599 { let mut discarded = Vec::new(); let _ = socket.read_to_end(&mut discarded).await; return; }
                                if code == 200 {
                                    (code, json!({"data":{"device_id":scope.device_id().to_string(),"library_id":scope.library_id().to_string(),"epoch":"1","acknowledged_sequence":"0","created_at":"2026-08-28T00:00:00Z","updated_at":"2026-08-28T00:00:00Z"},"meta":{"request_id":"p018-fixture"}}))
                                } else {
                                    (code, json!({"error":{"code":if code == 403 {"device_revoked"} else {"authentication_failed"},"message":"fixture","request_id":"p018-fixture","retryable":false}}))
                                }
                            };
                            let body = body.to_string();
                            let response = format!("HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nx-synveil-server-installation: {}\r\nx-synveil-network-integration: {network}\r\n\r\n{body}",body.len(),if sp.load(Ordering::SeqCst) == 1 {"spoof"} else {&server});
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        });
                    }
                    _ = children.join_next(), if !children.is_empty() => {}
                }
            }
        });
        Some(Self {
            profile,
            scope,
            status,
            spoof,
            requests,
            bearers,
            active,
            task,
        })
    }
    async fn enroll(
        &self,
        state: &LocalStateStore,
        secrets: &MemorySecrets,
    ) -> LoadedDeviceCredential {
        state.save_server_profile(&self.profile).await.unwrap();
        state
            .approve_managed_host_trust(&self.profile)
            .await
            .unwrap();
        assert!(
            state
                .load_device_credential(self.profile.profile_id(), secrets)
                .await
                .unwrap()
                .is_none()
        );
        let enrollment =
            HttpEnrollmentClient::new(self.profile.clone(), HttpClientConfig::default())
                .unwrap()
                .exchange(&EnrollmentSecret::from_bytes([0x44; 32]))
                .await
                .unwrap();
        state.store_enrollment(&enrollment, secrets).await.unwrap();
        state
            .load_device_credential(self.profile.profile_id(), secrets)
            .await
            .unwrap()
            .unwrap()
    }
    // Loopback is deliberately not accepted by Endpoint::from_profile. Only this
    // private test module maps an actual HTTPS origin to simulated route evidence.
    fn bind(
        &self,
        verifier: &mut SynveilVerifier,
        credential: LoadedDeviceCredential,
        transport: Transport,
    ) -> Endpoint {
        let mut endpoint = endpoint(
            transport,
            if transport == Transport::Lan {
                "192.168.1.3"
            } else {
                "100.64.0.3"
            },
        );
        let trust = self.profile.managed_host_trust().unwrap().clone();
        endpoint.origin = self.profile.base_url().as_str().into();
        endpoint.profile_id = self.profile.profile_id();
        endpoint.server_id = trust.server_id().into();
        endpoint.network_id = trust.network_id().into();
        endpoint.ca_sha256 = trust.ca_sha256().map(str::to_owned);
        let remote = HttpSyncRemote::new(
            self.profile.clone(),
            self.scope.device_id(),
            credential,
            HttpClientConfig::default(),
        )
        .unwrap();
        verifier.endpoints.push(VerifiedBinding {
            endpoint: endpoint.clone(),
            remote,
            scope: self.scope,
            trust,
        });
        endpoint
    }
}

// SQLite owns its live database, WAL, SHM, and writer lock. Reading those
// bytes directly races platform locks on Windows and is not a valid logical
// database snapshot. The persistent SQLite invariant is checked separately
// through a stable read-only connection's PRAGMA data_version.
fn persisted_non_sqlite_files(dir: &std::path::Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(
                    "state.db"
                        | "state.db-wal"
                        | "state.db-shm"
                        | "state.db-journal"
                        | "state.sqlite3.writer.lock"
                )
            )
        })
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

#[tokio::test]
async fn p018_https_identity_auth_and_no_cached_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let state = LocalStateStore::open(&LocalStateConfig::new(dir.path().join("state.db")))
        .await
        .unwrap();
    let secrets = MemorySecrets::default();
    let a = HttpsFixture::new(&uuid::Uuid::now_v7().to_string(), false)
        .await
        .unwrap();
    let b = HttpsFixture::new(&uuid::Uuid::now_v7().to_string(), false)
        .await
        .unwrap();
    let ca = a.enroll(&state, &secrets).await;
    let cb = b.enroll(&state, &secrets).await;
    assert!(
        HttpSyncRemote::new(
            b.profile.clone(),
            b.scope.device_id(),
            state
                .load_device_credential(a.profile.profile_id(), &secrets)
                .await
                .unwrap()
                .unwrap(),
            HttpClientConfig::default()
        )
        .is_err()
    );
    let mut v = SynveilVerifier::new();
    let ea = a.bind(&mut v, ca, Transport::Lan);
    let eb = b.bind(&mut v, cb, Transport::Tailscale);
    let d = FakeDiscovery::new(vec![snapshot()]);
    for (e, policy) in [
        (&ea, SelectionPolicy::LanOnly),
        (&eb, SelectionPolicy::TailscaleOnly),
    ] {
        assert_eq!(
            select(&d, &v, std::slice::from_ref(e), policy)
                .await
                .transport(),
            e.transport()
        );
    }
    // Distinct installations/scopes must not become fallback alternatives.
    assert_eq!(
        select(&d, &v, &[ea.clone(), eb], SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::IdentityConflict)
    );
    let metadata = state
        .profile_enrollment(a.profile.profile_id())
        .await
        .unwrap();
    let saved = secrets.0.lock().unwrap().clone();
    let persisted = persisted_non_sqlite_files(dir.path());
    // A separate, long-lived read-only SQLite connection observes commits from
    // LocalStateStore's single writer connection without opening locked files.
    // SQLite data_version changes on other-connection commits; selection must
    // perform no durable SQLite writes.
    let mut db_observer = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(state.database_path())
            .read_only(true),
    )
    .await
    .unwrap();
    let data_version_before: i64 = sqlx::query_scalar("PRAGMA data_version")
        .fetch_one(&mut db_observer)
        .await
        .unwrap();
    let routes = std::fs::read("/proc/net/route").ok();
    for (status, reason) in [(401, R::AuthenticationRequired), (403, R::AccessRevoked)] {
        a.status.store(status, Ordering::SeqCst);
        assert_eq!(
            select(
                &d,
                &v,
                std::slice::from_ref(&ea),
                SelectionPolicy::PreferLan
            )
            .await,
            Selection::Unavailable(reason)
        );
    }
    a.status.store(200, Ordering::SeqCst);
    a.spoof.store(1, Ordering::SeqCst);
    let bearer_count = a.bearers.load(Ordering::SeqCst);
    assert_eq!(
        select(
            &d,
            &v,
            std::slice::from_ref(&ea),
            SelectionPolicy::PreferLan
        )
        .await,
        Selection::Unavailable(R::UntrustedEndpoint)
    );
    assert_eq!(a.bearers.load(Ordering::SeqCst), bearer_count);
    a.spoof.store(0, Ordering::SeqCst);
    assert_eq!(
        select(&d, &v, &[ea], SelectionPolicy::LanOnly)
            .await
            .transport(),
        Transport::Lan
    );
    assert_eq!(
        data_version_before,
        sqlx::query_scalar::<_, i64>("PRAGMA data_version")
            .fetch_one(&mut db_observer)
            .await
            .unwrap(),
        "selection committed an unexpected SQLite state change"
    );
    assert!(
        persisted == persisted_non_sqlite_files(dir.path()),
        "non-SQLite persisted files changed during selection"
    );
    assert_eq!(routes, std::fs::read("/proc/net/route").ok());
    assert_eq!(
        metadata,
        state
            .profile_enrollment(a.profile.profile_id())
            .await
            .unwrap()
    );
    assert!(saved == *secrets.0.lock().unwrap(), "secret state changed");
    assert!(a.requests.load(Ordering::SeqCst) > 4);
}

#[tokio::test]
async fn p018_tls_failure_precedes_bearer_and_ipv6_https_is_supported_when_available() {
    let dir = tempfile::tempdir().unwrap();
    let state = LocalStateStore::open(&LocalStateConfig::new(dir.path().join("state.db")))
        .await
        .unwrap();
    let secrets = MemorySecrets::default();
    for ipv6 in [false, true] {
        let Some(f) = HttpsFixture::new(&uuid::Uuid::now_v7().to_string(), ipv6).await else {
            eprintln!("P018_IPV6_HTTPS_UNAVAILABLE");
            continue;
        };
        let credential = f.enroll(&state, &secrets).await;
        let mut v = SynveilVerifier::new();
        let e = f.bind(
            &mut v,
            state
                .load_device_credential(f.profile.profile_id(), &secrets)
                .await
                .unwrap()
                .unwrap(),
            Transport::Lan,
        );
        assert_eq!(v.verify(&e).await, Ok(()));
        // Same origin/profile but an independently generated CA is not authority.
        let other = HttpsFixture::new(&uuid::Uuid::now_v7().to_string(), false)
            .await
            .unwrap();
        let trust = ManagedHostTrust::from_ca(
            e.server_id.clone(),
            e.network_id.clone(),
            e.origin.clone(),
            match other.profile.managed_host_trust().unwrap() {
                HostTrust::Managed(t) => t.ca_certificate_pem.clone(),
                _ => unreachable!(),
            },
            1,
        )
        .unwrap();
        let wrong = f
            .profile
            .clone()
            .with_managed_host_trust(HostTrust::Managed(trust))
            .unwrap();
        v.endpoints[0].remote = HttpSyncRemote::new(
            wrong,
            f.scope.device_id(),
            credential,
            HttpClientConfig::default(),
        )
        .unwrap();
        let count = f.bearers.load(Ordering::SeqCst);
        assert_eq!(v.verify(&e).await, Err(R::UntrustedEndpoint));
        assert_eq!(count, f.bearers.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn p018_route_change_unavailability_retry_and_cancellation_release_resources() {
    let dir = tempfile::tempdir().unwrap();
    let state = LocalStateStore::open(&LocalStateConfig::new(dir.path().join("state.db")))
        .await
        .unwrap();
    let secrets = MemorySecrets::default();
    let f = HttpsFixture::new(&uuid::Uuid::now_v7().to_string(), false)
        .await
        .unwrap();
    let credential = f.enroll(&state, &secrets).await;
    let mut v = SynveilVerifier::new();
    let e = f.bind(&mut v, credential, Transport::Lan);
    let mut changed = snapshot();
    changed.routes[0].interface_id = "wifi0".into();
    let d = FakeDiscovery::new(vec![snapshot(), changed]);
    assert_eq!(
        select(&d, &v, std::slice::from_ref(&e), SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::RouteChanged)
    );
    let mut absent = snapshot();
    absent.routes.remove(0);
    let count = f.requests.load(Ordering::SeqCst);
    assert_eq!(
        select(
            &FakeDiscovery::new(vec![absent]),
            &v,
            std::slice::from_ref(&e),
            SelectionPolicy::LanOnly
        )
        .await,
        Selection::Unavailable(R::Unreachable)
    );
    assert_eq!(count, f.requests.load(Ordering::SeqCst));
    f.status.store(599, Ordering::SeqCst);
    let (tx, rx) = watch::channel(false);
    let endpoints = [e.clone()];
    let d = FakeDiscovery::new(vec![snapshot()]);
    let future = discover_and_select(&d, &v, &endpoints, SelectionPolicy::LanOnly, rx);
    tokio::pin!(future);
    tokio::select! {
        result = &mut future => panic!("unexpected completion {result:?}"),
        _ = async { while f.bearers.load(Ordering::SeqCst) < 2 { tokio::task::yield_now().await; } } => {}
    }
    tx.send(true).unwrap();
    assert_eq!(future.await, Selection::Unavailable(R::Cancelled));
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.active.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    f.task.abort();
    let start = tokio::time::Instant::now();
    assert_eq!(
        select(&d, &v, &[e], SelectionPolicy::LanOnly).await,
        Selection::Unavailable(R::Unreachable)
    );
    assert!(start.elapsed() < SELECTION_TIMEOUT);
}

struct Counted(SynveilVerifier, AtomicUsize);
#[async_trait]
impl EndpointVerifier for Counted {
    async fn verify(&self, e: &Endpoint) -> Result<(), R> {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.verify(e).await
    }
}

#[tokio::test]
async fn p018_two_independently_approved_https_paths_obey_all_policies() {
    let dir = tempfile::tempdir().unwrap();
    let state = LocalStateStore::open(&LocalStateConfig::new(dir.path().join("state.db")))
        .await
        .unwrap();
    let secrets = MemorySecrets::default();
    let id = uuid::Uuid::now_v7().to_string();
    let a = HttpsFixture::new(&id, false).await.unwrap();
    let b = HttpsFixture::scoped(&id, false, Some(a.scope))
        .await
        .unwrap();
    let mut verifier = SynveilVerifier::new();
    let ea = a.bind(
        &mut verifier,
        a.enroll(&state, &secrets).await,
        Transport::Lan,
    );
    let eb = b.bind(
        &mut verifier,
        b.enroll(&state, &secrets).await,
        Transport::Tailscale,
    );
    let verifier = Counted(verifier, AtomicUsize::new(0));
    let endpoints = [ea, eb];
    let d = FakeDiscovery::new(vec![snapshot()]);
    for (policy, transport) in [
        (SelectionPolicy::LanOnly, Transport::Lan),
        (SelectionPolicy::PreferLan, Transport::Lan),
        (SelectionPolicy::PreferTailscale, Transport::Tailscale),
        (SelectionPolicy::TailscaleOnly, Transport::Tailscale),
    ] {
        assert_eq!(
            select(&d, &verifier, &endpoints, policy).await.transport(),
            transport
        );
    }
    for (fixture, policy, other) in [
        (&a, SelectionPolicy::PreferLan, &b),
        (&b, SelectionPolicy::PreferTailscale, &a),
    ] {
        for (status, reason) in [(401, R::AuthenticationRequired), (403, R::AccessRevoked)] {
            fixture.status.store(status, Ordering::SeqCst);
            let count = other.requests.load(Ordering::SeqCst);
            assert_eq!(
                select(&d, &verifier, &endpoints, policy).await,
                Selection::Unavailable(reason)
            );
            assert_eq!(count, other.requests.load(Ordering::SeqCst));
        }
        fixture.status.store(200, Ordering::SeqCst);
    }
    a.task.abort();
    tokio::task::yield_now().await;
    let count = verifier.1.load(Ordering::SeqCst);
    assert_eq!(
        select(&d, &verifier, &endpoints, SelectionPolicy::PreferLan)
            .await
            .transport(),
        Transport::Tailscale
    );
    assert_eq!(
        verifier.1.load(Ordering::SeqCst) - count,
        3,
        "two LAN attempts then independent overlay authentication"
    );
    b.status.store(401, Ordering::SeqCst);
    assert_eq!(
        select(&d, &verifier, &endpoints, SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::AuthenticationRequired)
    );
}
