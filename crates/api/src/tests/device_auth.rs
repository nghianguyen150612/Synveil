use super::*;
use crate::DeviceAuthenticationBackend;
use synveil_auth::{
    DeviceAuthError, DeviceCredentialPrincipal, DeviceEnrollmentTarget, IssuedDeviceCredential,
    IssuedEnrollmentGrant,
};
use synveil_core::{
    DeviceCredentialId, DeviceCredentialSecret, EnrollmentSecret, OutboundIntentId, Sha256Digest,
};

struct TestDeviceAuth {
    principal: DeviceCredentialPrincipal,
    secret: DeviceCredentialSecret,
    revoked: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl DeviceAuthenticationBackend for TestDeviceAuth {
    async fn create_grant(
        &self,
        _owner: UserId,
        _target: DeviceEnrollmentTarget,
    ) -> Result<IssuedEnrollmentGrant, DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
    async fn exchange(
        &self,
        _token: &EnrollmentSecret,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError> {
        Err(DeviceAuthError::InvalidEnrollment)
    }
    async fn authenticate(
        &self,
        secret: &DeviceCredentialSecret,
    ) -> Result<DeviceCredentialPrincipal, DeviceAuthError> {
        if !secret.digest_matches(&self.secret.digest()) {
            return Err(DeviceAuthError::InvalidCredential);
        }
        if self.revoked.load(Ordering::SeqCst) {
            return Err(DeviceAuthError::DeviceRevoked);
        }
        Ok(self.principal)
    }
    async fn revoke_credential(
        &self,
        _owner: UserId,
        _device: DeviceId,
        _credential: DeviceCredentialId,
    ) -> Result<(), DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
    async fn revoke_all_credentials(
        &self,
        _owner: UserId,
        _device: DeviceId,
    ) -> Result<(), DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
}

fn device_request(
    method: Method,
    uri: &str,
    body: Value,
    secret: &DeviceCredentialSecret,
) -> Request<Body> {
    let mut request = json_request(method, uri, &body.to_string());
    let mut authorization =
        HeaderValue::from_str(&format!("Bearer {}", secret.expose_secret())).unwrap();
    authorization.set_sensitive(true);
    request.headers_mut().insert("authorization", authorization);
    request
}

fn fixture() -> (ApiState, Arc<TestDeviceAuth>, DeviceId, LibraryId) {
    let browser = Arc::new(TestAuthenticationBackend::new());
    let device = DeviceId::new();
    let library = LibraryId::new();
    let now = Timestamp::now();
    let checkpoint = DeviceSyncCheckpoint::new(
        browser.user_id,
        device,
        library,
        Sequence::new(1),
        Sequence::new(0),
        now,
        now,
        None,
    );
    let auth = Arc::new(TestDeviceAuth {
        principal: DeviceCredentialPrincipal {
            owner_user_id: browser.user_id,
            device_id: device,
            credential_id: DeviceCredentialId::new(),
        },
        secret: DeviceCredentialSecret::from_bytes([0x71; 32]),
        revoked: std::sync::atomic::AtomicBool::new(false),
    });
    let state = state(true)
        .with_auth_backend(browser)
        .with_device_auth_backend(auth.clone())
        .with_sync_backend(Arc::new(TestSyncBackend {
            checkpoint,
            feed: None,
            ack_error: None,
        }))
        .with_allowed_origin("https://app.example");
    (state, auth, device, library)
}

fn outbound_fixture() -> (ApiState, Arc<TestDeviceAuth>, DeviceId, LibraryId) {
    let browser = Arc::new(TestAuthenticationBackend::new());
    let device = DeviceId::new();
    let library = LibraryId::new();
    let auth = Arc::new(TestDeviceAuth {
        principal: DeviceCredentialPrincipal {
            owner_user_id: browser.user_id,
            device_id: device,
            credential_id: DeviceCredentialId::new(),
        },
        secret: DeviceCredentialSecret::from_bytes([0x72; 32]),
        revoked: std::sync::atomic::AtomicBool::new(false),
    });
    let root = Node::new_root(
        NodeId::new(),
        library,
        LogicalName::new("root").unwrap(),
        Timestamp::now(),
    );
    let state = state(true)
        .with_auth_backend(browser.clone())
        .with_device_auth_backend(auth.clone())
        .with_client_mutation_backend(Arc::new(TestClientMutationBackend::applied(
            browser.user_id,
            device,
            library,
            root,
        )))
        .with_upload_backend(Arc::new(TestUploadBackend::new(8)))
        .with_allowed_origin("https://app.example");
    (state, auth, device, library)
}

#[tokio::test]
async fn device_principal_is_scoped_and_only_authenticated_bearer_bypasses_csrf() {
    let (state, auth, device, library) = fixture();
    let scope = format!("/api/v1/devices/{device}/libraries/{library}");
    let checkpoint = router(state.clone())
        .oneshot(device_request(
            Method::GET,
            &format!("{scope}/checkpoint"),
            Value::Null,
            &auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(checkpoint.status(), StatusCode::OK);
    assert_eq!(
        json_body(checkpoint).await["data"]["device_id"],
        device.to_string()
    );

    let evidence = SyncAckEvidence::new(
        auth.principal.owner_user_id,
        device,
        library,
        Sequence::new(1),
        Sequence::new(0),
        Sequence::new(0),
        Sequence::new(0),
    );
    let ack = serde_json::json!({"ack_token": state.sync_ack_key().issue(evidence)});
    let response = router(state.clone())
        .oneshot(device_request(
            Method::POST,
            &format!("{scope}/changes/ack"),
            ack.clone(),
            &auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(!response.headers().contains_key("set-cookie"));

    let (session, csrf) = login_cookies(&state).await;
    let mut cookie_request = authenticated_body_request(
        Method::POST,
        &format!("{scope}/changes/ack"),
        Body::from(ack.to_string()),
        &session,
        &csrf,
    );
    mark_same_origin(&mut cookie_request);
    let response = router(state.clone()).oneshot(cookie_request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "browser CSRF remains enforced"
    );

    let mut mixed = device_request(
        Method::POST,
        &format!("{scope}/changes/ack"),
        ack,
        &auth.secret,
    );
    mixed
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    let response = router(state.clone()).oneshot(mixed).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "mixed auth never falls back to cookies"
    );

    for route in [
        format!(
            "/api/v1/devices/{}/libraries/{library}/checkpoint",
            DeviceId::new()
        ),
        format!(
            "/api/v1/devices/{device}/libraries/{}/checkpoint",
            LibraryId::new()
        ),
    ] {
        let response = router(state.clone())
            .oneshot(device_request(
                Method::GET,
                &route,
                Value::Null,
                &auth.secret,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let mut invalid_with_cookie = authenticated_body_request(
        Method::POST,
        &format!("{scope}/changes/ack"),
        Body::from("{}"),
        &session,
        &csrf,
    );
    invalid_with_cookie
        .headers_mut()
        .insert("authorization", HeaderValue::from_static("Bearer invalid"));
    let response = router(state).oneshot(invalid_with_cookie).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn device_bearer_authorizes_only_outbound_mutation_and_upload_routes_without_csrf() {
    let (state, auth, device, library) = outbound_fixture();
    let scope = format!("/api/v1/devices/{device}/libraries/{library}");
    let node_id = NodeId::new();
    let mutation = serde_json::json!({
        "mutation_id": synveil_core::ClientMutationId::new().to_string(),
        "base_epoch": "1",
        "base_sequence": "0",
        "kind": "TRASH_NODE",
        "payload": {"node_id": node_id.to_string(), "expected_revision": "0"}
    });
    let response = router(state.clone())
        .oneshot(device_request(
            Method::POST,
            &format!("{scope}/mutations"),
            mutation,
            &auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let create = serde_json::json!({
        "operation":"CREATE_FILE",
        "idempotency_key": OutboundIntentId::new().to_string(),
        "library_id": library.to_string(),
        "parent_id": NodeId::new().to_string(),
        "name":"device.bin",
        "expected_bytes":"3",
        "expected_sha256": Sha256Digest::from_bytes(Sha256::digest(b"abc").into()).to_string()
    });
    let response = router(state.clone())
        .oneshot(device_request(
            Method::POST,
            "/api/v1/upload-sessions",
            create,
            &auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let upload_id = json_body(response).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let mut append = request(
        Method::PATCH,
        &format!("/api/v1/upload-sessions/{upload_id}"),
        Body::from("abc"),
    );
    append.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", auth.secret.expose_secret())).unwrap(),
    );
    append.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    append
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    let response = router(state.clone()).oneshot(append).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = router(state.clone())
        .oneshot(device_request(
            Method::POST,
            &format!("/api/v1/upload-sessions/{upload_id}/complete"),
            serde_json::json!({}),
            &auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = router(state)
        .oneshot(device_request(
            Method::GET,
            &format!("{scope}/conflicts"),
            serde_json::json!({}),
            &auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_cannot_authorize_browser_admin_or_conflict_routes() {
    let (state, auth, device, library) = fixture();
    let scope = format!("/api/v1/devices/{device}/libraries/{library}");
    for (method, path) in [
        (Method::GET, "/api/v1/auth/session".to_owned()),
        (Method::GET, "/api/v1/auth/csrf".to_owned()),
        (Method::POST, "/api/v1/auth/logout".to_owned()),
        (Method::POST, "/api/v1/devices/enrollment-grants".to_owned()),
        (
            Method::POST,
            format!("/api/v1/devices/{device}/credentials/revoke-all"),
        ),
        (Method::GET, format!("{scope}/conflicts")),
    ] {
        let response = router(state.clone())
            .oneshot(device_request(
                method,
                &path,
                serde_json::json!({}),
                &auth.secret,
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "route must stay browser-only"
        );
    }
}

#[tokio::test]
async fn device_bearer_can_use_library_catalog_without_browser_csrf() {
    let browser = Arc::new(TestAuthenticationBackend::new());
    let file_backend = Arc::new(TestFileMetadataBackend::new(browser.user_id));
    let library_id = file_backend.library_id();
    let device = DeviceId::new();
    let device_auth = Arc::new(TestDeviceAuth {
        principal: DeviceCredentialPrincipal {
            owner_user_id: browser.user_id,
            device_id: device,
            credential_id: DeviceCredentialId::new(),
        },
        secret: DeviceCredentialSecret::from_bytes([0x73; 32]),
        revoked: std::sync::atomic::AtomicBool::new(false),
    });
    let state = state(true)
        .with_auth_backend(browser)
        .with_device_auth_backend(device_auth.clone())
        .with_file_metadata_backend(file_backend);

    let response = router(state.clone())
        .oneshot(device_request(
            Method::GET,
            "/api/v1/libraries",
            Value::Null,
            &device_auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"][0]["id"],
        library_id.to_string()
    );

    let response = router(state)
        .oneshot(device_request(
            Method::POST,
            "/api/v1/libraries",
            serde_json::json!({"id": library_id.to_string(), "name": "Primary"}),
            &device_auth.secret,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        json_body(response).await["data"]["id"],
        library_id.to_string()
    );
}

#[tokio::test]
async fn bearer_revocation_applies_to_every_inbound_route_and_parsing_is_bounded() {
    let (state, auth, device, library) = fixture();
    let scope = format!("/api/v1/devices/{device}/libraries/{library}");
    auth.revoked.store(true, Ordering::SeqCst);
    for (method, path) in [
        (Method::GET, format!("{scope}/checkpoint")),
        (Method::GET, format!("{scope}/changes")),
        (Method::POST, format!("{scope}/changes/ack")),
        (Method::POST, format!("{scope}/rebaseline")),
        (
            Method::GET,
            format!("{scope}/rebaseline/{}/nodes", SyncBootstrapId::new()),
        ),
        (
            Method::POST,
            format!("{scope}/rebaseline/{}/complete", SyncBootstrapId::new()),
        ),
        (Method::GET, format!("/api/v1/nodes/{}", NodeId::new())),
        (
            Method::GET,
            format!("/api/v1/nodes/{}/content", NodeId::new()),
        ),
        (
            Method::GET,
            format!("/api/v1/versions/{}", FileVersionId::new()),
        ),
        (
            Method::GET,
            format!("/api/v1/versions/{}/content", FileVersionId::new()),
        ),
    ] {
        let response = router(state.clone())
            .oneshot(device_request(
                method,
                &path,
                serde_json::json!({}),
                &auth.secret,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json_body(response).await["error"]["code"], "device_revoked");
    }
    auth.revoked.store(false, Ordering::SeqCst);
    for candidate in [
        "Bearer invalid".to_owned(),
        "Basic abc".to_owned(),
        format!("Bearer {} ", auth.secret.expose_secret()),
        format!("Bearer {}", "x".repeat(8192)),
    ] {
        let mut request = request(Method::GET, &format!("{scope}/checkpoint"), Body::empty());
        request
            .headers_mut()
            .insert("authorization", HeaderValue::from_str(&candidate).unwrap());
        let response = router(state.clone()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!format!("{:?}", json_body(response).await).contains(&candidate));
    }
    let mut duplicate = device_request(
        Method::GET,
        &format!("{scope}/checkpoint"),
        Value::Null,
        &auth.secret,
    );
    duplicate
        .headers_mut()
        .append("authorization", HeaderValue::from_static("Bearer invalid"));
    assert_eq!(
        router(state).oneshot(duplicate).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn enrollment_routes_are_strict_bounded_private_and_do_not_echo_tokens() {
    let (state, _, _, _) = fixture();
    let (session, csrf) = login_cookies(&state).await;
    let grant_path = "/api/v1/devices/enrollment-grants";
    let payload = serde_json::json!({"target":{"kind":"new","display_name":"Desktop"}});
    let no_csrf = authenticated_body_request(
        Method::POST,
        grant_path,
        Body::from(payload.to_string()),
        &session,
        &csrf,
    );
    assert_eq!(
        router(state.clone())
            .oneshot(no_csrf)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for payload in [
        serde_json::json!({"enrollment_token":"secret-that-must-not-be-echoed"}),
        serde_json::json!({"enrollment_token":"x".repeat(crate::DEVICE_ENROLLMENT_BODY_LIMIT_BYTES + 1)}),
        serde_json::json!({"enrollment_token":"invalid","unexpected":true}),
    ] {
        let response = router(state.clone())
            .oneshot(json_request(
                Method::POST,
                "/api/v1/device-enrollment/exchange",
                &payload.to_string(),
            ))
            .await
            .unwrap();
        assert!(response.status().is_client_error());
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let body = json_body(response).await;
        assert!(!body.to_string().contains("secret-that-must-not-be-echoed"));
    }
}

#[derive(Clone)]
struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn request_traces_and_errors_redact_headers_bodies_and_raw_uri_tokens() {
    // tracing's process-wide callsite cache is shared with concurrent router
    // tests. Exercise the real subscriber and router in a fresh test process,
    // keeping the nonempty-capture assertion instead of accepting empty logs.
    const CHILD_ENV: &str = "SYNVEIL_TRACE_CAPTURE_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::device_auth::request_traces_and_errors_redact_headers_bodies_and_raw_uri_tokens", "--nocapture"])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "isolated real-router trace capture failed"
        );
        return;
    }

    let output = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = CapturedLogs(output.clone());
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let (state, auth, device, library) = fixture();
    let grant = EnrollmentSecret::from_bytes([0x45; 32]);
    let body_token = "synthetic-ack-evidence-never-log";
    let cookie_token = "synthetic-cookie-secret-never-log";
    let raw_path_token = "synthetic-unmatched-path-never-log";
    let raw_query_token = "synthetic-query-secret-never-log";
    let scope = format!("/api/v1/devices/{device}/libraries/{library}");
    async {
        let response = router(state.clone())
            .oneshot(device_request(
                Method::GET,
                &format!("{scope}/checkpoint"),
                Value::Null,
                &auth.secret,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = router(state.clone())
            .oneshot(device_request(
                Method::POST,
                &format!("{scope}/changes/ack"),
                serde_json::json!({"ack_token": body_token}),
                &auth.secret,
            ))
            .await
            .unwrap();
        assert!(response.status().is_client_error());
        assert!(!json_body(response).await.to_string().contains(body_token));
        let response = router(state.clone())
            .oneshot(json_request(
                Method::POST,
                "/api/v1/device-enrollment/exchange",
                &serde_json::json!({"enrollment_token": grant.expose_secret()}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(
            !json_body(response)
                .await
                .to_string()
                .contains(grant.expose_secret())
        );
        let mut request = device_request(
            Method::GET,
            &format!("/api/v1/{raw_path_token}?opaque={raw_query_token}"),
            Value::Null,
            &auth.secret,
        );
        request
            .headers_mut()
            .insert("cookie", HeaderValue::from_str(cookie_token).unwrap());
        let response = router(state.clone()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let error = json_body(response).await.to_string();
        assert!(!error.contains(raw_path_token));
        assert!(!error.contains(raw_query_token));
        for secret in [auth.secret.expose_secret(), grant.expose_secret()] {
            let mut request = device_request(
                Method::GET,
                &format!("{scope}/checkpoint"),
                Value::Null,
                &auth.secret,
            );
            request.headers_mut().insert(
                "x-request-id",
                HeaderValue::from_str(&format!("copied-{secret}")).unwrap(),
            );
            let response = router(state.clone()).oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(
                !response.headers()["x-request-id"]
                    .to_str()
                    .unwrap()
                    .contains(secret),
                "machine secrets cannot become logged correlation IDs"
            );
            assert!(!json_body(response).await.to_string().contains(secret));
        }
    }
    .await;
    let logs = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("request completed"),
        "capture must observe real tracing"
    );
    for forbidden in [
        auth.secret.expose_secret(),
        grant.expose_secret(),
        body_token,
        cookie_token,
        raw_path_token,
        raw_query_token,
    ] {
        assert!(
            !logs.contains(forbidden),
            "trace must contain no secret input"
        );
    }
}
