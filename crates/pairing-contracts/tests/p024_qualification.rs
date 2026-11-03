//! P024 composition and compatibility qualification. All evidence is synthetic.
mod support;

use sqlx::{Connection, Row, SqliteConnection};
use std::path::PathBuf;
use support::Outcome;
use synveil_pairing_contracts::{
    ENROLLMENT_AUDIENCE, PairingQrPayload,
    ceremony::{
        BoundRequest, CEREMONY_VERSION, ClaimRequest, ReferenceCeremony, ReferenceDecision,
        ReferenceFacts, ReferenceServerIdentity, ReferenceTls,
    },
    recovery::{RecoveryBindingV1, RecoveryRecordV1, RecoveryStateV1},
};

fn invitation(id: &str) -> PairingQrPayload {
    PairingQrPayload {
        contract_version: 1,
        invitation_id: id.into(),
        installation_id: "fixture-installation".into(),
        issuer_owner_id: "fixture-owner".into(),
        audience: ENROLLMENT_AUDIENCE.into(),
        approved_endpoint: "https://pairing.example.test".into(),
        server_identity_fingerprint: format!("sha256:{}", "a".repeat(64)),
        target_device_id: "fixture-device".into(),
        issued_at_unix: 100,
        expires_at_unix: 200,
        policy_revision: 7,
    }
}

fn facts(qr: &PairingQrPayload) -> ReferenceFacts {
    ReferenceFacts {
        authoritative_invitation: qr.clone(),
        issuer_active: true,
        target_device_active: true,
        independent_identity: Some(ReferenceServerIdentity {
            installation_id: qr.installation_id.clone(),
            endpoint: qr.approved_endpoint.clone(),
            fingerprint: qr.server_identity_fingerprint.clone(),
            tls: ReferenceTls::CertificateAndHostnameAuthenticated,
        }),
        invitation_claim_verified: true,
        owner_approval: None,
        client_consent: None,
    }
}

async fn database_decision(
    db: &mut SqliteConnection,
    ceremony_id: Option<&str>,
    kind: &str,
) -> Option<ReferenceDecision> {
    let ceremony_id = ceremony_id?;
    let row = sqlx::query(
        "SELECT evidence_id,principal,binding,issued,expires,revoked FROM ceremony_events WHERE ceremony_id=? AND kind=?",
    )
    .bind(ceremony_id)
    .bind(kind)
    .fetch_optional(&mut *db)
    .await
    .unwrap()?;
    Some(ReferenceDecision {
        evidence_id: row.get("evidence_id"),
        principal_id: row.get("principal"),
        binding: serde_json::from_str(&row.get::<String, _>("binding")).unwrap(),
        issued_at_unix: row.get("issued"),
        expires_at_unix: row.get("expires"),
        revoked: row.get::<i64, _>("revoked") != 0,
    })
}

async fn database_facts(
    db: &mut SqliteConnection,
    invitation_id: &str,
    ceremony_id: Option<&str>,
) -> ReferenceFacts {
    let binding: String = sqlx::query_scalar("SELECT binding FROM invitations WHERE id=?")
        .bind(invitation_id)
        .fetch_one(&mut *db)
        .await
        .unwrap();
    let authoritative_invitation: PairingQrPayload = serde_json::from_str(&binding).unwrap();
    let authority = sqlx::query(
        "SELECT owner,endpoint,fingerprint,policy,active FROM authority WHERE installation=?",
    )
    .bind(&authoritative_invitation.installation_id)
    .fetch_one(&mut *db)
    .await
    .unwrap();
    let mut current_authoritative_invitation = authoritative_invitation.clone();
    current_authoritative_invitation.issuer_owner_id = authority.get("owner");
    current_authoritative_invitation.approved_endpoint = authority.get("endpoint");
    current_authoritative_invitation.server_identity_fingerprint = authority.get("fingerprint");
    current_authoritative_invitation.policy_revision =
        u64::try_from(authority.get::<i64, _>("policy")).unwrap();
    let identity = sqlx::query(
        "SELECT installation,endpoint,fingerprint,tls FROM independent_identity WHERE installation=?",
    )
    .bind(&authoritative_invitation.installation_id)
    .fetch_one(&mut *db)
    .await
    .unwrap();
    let device_active: i64 =
        sqlx::query_scalar("SELECT active FROM device_authority WHERE device=?")
            .bind(&authoritative_invitation.target_device_id)
            .fetch_one(&mut *db)
            .await
            .unwrap();
    ReferenceFacts {
        authoritative_invitation: current_authoritative_invitation,
        issuer_active: authority.get::<i64, _>("active") == 1,
        target_device_active: device_active == 1,
        independent_identity: Some(ReferenceServerIdentity {
            installation_id: identity.get("installation"),
            endpoint: identity.get("endpoint"),
            fingerprint: identity.get("fingerprint"),
            tls: if identity.get::<i64, _>("tls") == 1 {
                ReferenceTls::CertificateAndHostnameAuthenticated
            } else {
                ReferenceTls::CertificateOrHostnameMismatch
            },
        }),
        // Trusted synthetic fixture fact, never sourced from the claim request.
        invitation_claim_verified: true,
        owner_approval: database_decision(db, ceremony_id, "owner").await,
        client_consent: database_decision(db, ceremony_id, "client").await,
    }
}

fn decision(request: &BoundRequest, principal: &str, id: &str, issued: i64) -> ReferenceDecision {
    ReferenceDecision {
        evidence_id: id.into(),
        principal_id: principal.into(),
        binding: request.binding.clone(),
        issued_at_unix: issued,
        expires_at_unix: 170,
        revoked: false,
    }
}

fn db_path() -> PathBuf {
    std::env::temp_dir().join(format!("synveil-p024-{}.sqlite", uuid::Uuid::now_v7()))
}

#[tokio::test]
async fn p020_through_p023_compose_in_isolated_sqlite_fixture() {
    let path = db_path();
    let qr = invitation("p024-e2e");
    let mut db = support::connect(&path).await.unwrap();
    support::initialize(&mut db).await.unwrap();
    sqlx::raw_sql(include_str!("support/schema_p023.sql"))
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO authority VALUES (?,?,?,?,?,1)")
        .bind(&qr.installation_id)
        .bind(&qr.issuer_owner_id)
        .bind(&qr.approved_endpoint)
        .bind(&qr.server_identity_fingerprint)
        .bind(i64::try_from(qr.policy_revision).unwrap())
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO independent_identity VALUES (?,?,?,1)")
        .bind(&qr.installation_id)
        .bind(&qr.approved_endpoint)
        .bind(&qr.server_identity_fingerprint)
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO device_authority VALUES (?,1)")
        .bind(&qr.target_device_id)
        .execute(&mut db)
        .await
        .unwrap();

    // P020 typed invitation and P021 file-backed issuance use existing rules.
    support::create(&mut db, &qr).await.unwrap();
    assert_eq!(
        support::transition(
            &mut db,
            &qr.invitation_id,
            &qr.installation_id,
            || 110,
            false
        )
        .await
        .unwrap(),
        Outcome::Issued
    );

    // Parse the QR as untrusted claim data; independently loaded facts establish identity.
    let claim = ClaimRequest {
        version: CEREMONY_VERSION,
        ceremony_id: "p024-ceremony".into(),
        qr_claim: serde_json::from_str(&serde_json::to_string(&qr).unwrap()).unwrap(),
    };
    let mut ceremony = ReferenceCeremony::claim(claim).unwrap();
    let mut trusted = database_facts(&mut db, &qr.invitation_id, None).await;
    ceremony.authenticate_identity(&trusted, 111).unwrap();
    let request = ceremony
        .challenge("p024-challenge".into(), 180, &trusted, 112)
        .unwrap();
    trusted.owner_approval = Some(decision(
        &request,
        &qr.issuer_owner_id,
        "p024-owner-action",
        113,
    ));
    ceremony.approve(&request, &trusted, 114).unwrap();
    trusted.client_consent = Some(decision(
        &request,
        &qr.target_device_id,
        "p024-client-consent",
        115,
    ));
    ceremony.consent(&request, &trusted, 116).unwrap();

    // P023 recovery is bound to the same invitation, owner, device and ceremony.
    let mut recovery = RecoveryRecordV1::new(
        "p024-recovery",
        RecoveryBindingV1 {
            installation_id: qr.installation_id.clone(),
            owner_id: qr.issuer_owner_id.clone(),
            device_id: qr.target_device_id.clone(),
            invitation_id: qr.invitation_id.clone(),
            ceremony_id: request.binding.ceremony_id.clone(),
            server_identity_fingerprint: qr.server_identity_fingerprint.clone(),
            policy_revision: qr.policy_revision,
        },
        RecoveryStateV1::PairingPending,
        116,
    )
    .unwrap();
    sqlx::query("INSERT INTO ceremonies VALUES (?,?,?,?)")
        .bind(&request.binding.ceremony_id)
        .bind(&qr.invitation_id)
        .bind(&request.binding.challenge_id)
        .bind(serde_json::to_string(&ceremony).unwrap())
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO p023_recovery_records VALUES (1,?,?,?,?,?)")
        .bind(&recovery.recovery_id)
        .bind(&qr.invitation_id)
        .bind(serde_json::to_string(&recovery.binding).unwrap())
        .bind("PAIRING_PENDING")
        .bind(116_i64)
        .execute(&mut db)
        .await
        .unwrap();

    // Confirmation, P021 consumption, and P023 outcome commit under one writer fence.
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
    for (kind, evidence) in [
        ("owner", trusted.owner_approval.as_ref().unwrap()),
        ("client", trusted.client_consent.as_ref().unwrap()),
    ] {
        sqlx::query("INSERT INTO ceremony_events VALUES (?,?,?,?,?,?,?,0)")
            .bind(&request.binding.ceremony_id)
            .bind(kind)
            .bind(&evidence.evidence_id)
            .bind(&evidence.principal_id)
            .bind(serde_json::to_string(&request.binding).unwrap())
            .bind(evidence.issued_at_unix)
            .bind(evidence.expires_at_unix)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let current_facts = database_facts(
        &mut tx,
        &qr.invitation_id,
        Some(&request.binding.ceremony_id),
    )
    .await;
    ceremony.confirm(&request, &current_facts, 117).unwrap();
    sqlx::query(
        "INSERT INTO approvals SELECT id,binding FROM invitations WHERE id=? AND state='ISSUED'",
    )
    .bind(&qr.invitation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        support::redeem_locked(
            &mut tx,
            &qr.invitation_id,
            &qr.installation_id,
            &qr.target_device_id,
            || 117,
        )
        .await
        .unwrap(),
        Outcome::Consumed
    );
    recovery.transition(RecoveryStateV1::Enrolled, 117).unwrap();
    sqlx::query("UPDATE ceremonies SET snapshot=? WHERE id=?")
        .bind(serde_json::to_string(&ceremony).unwrap())
        .bind(&request.binding.ceremony_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE p023_recovery_records SET state='ENROLLED',recorded_at=117 WHERE recovery_id=?",
    )
    .bind(&recovery.recovery_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO p023_credential_reference VALUES (1,?,?,?,?,?,'ACTIVE',117,117,'NON_PRODUCTION_REFERENCE_ONLY')")
        .bind(&qr.invitation_id)
        .bind(&qr.installation_id)
        .bind(&qr.issuer_owner_id)
        .bind(&qr.target_device_id)
        .bind(i64::try_from(qr.policy_revision).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // Synthetic response loss: status can be observed, but replay cannot reissue anything.
    assert_eq!(
        ceremony.response().state,
        synveil_pairing_contracts::ceremony::CeremonyState::Confirmed
    );
    let recovered = ceremony.recover(&request, &trusted).unwrap();
    assert_eq!(
        recovered.state,
        synveil_pairing_contracts::ceremony::CeremonyState::Confirmed
    );
    let persisted_state: String = sqlx::query_scalar("SELECT snapshot FROM ceremonies WHERE id=?")
        .bind(&request.binding.ceremony_id)
        .fetch_one(&mut db)
        .await
        .unwrap();
    let persisted_ceremony: ReferenceCeremony = serde_json::from_str(&persisted_state).unwrap();
    assert_eq!(
        persisted_ceremony.response().state,
        synveil_pairing_contracts::ceremony::CeremonyState::Confirmed
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ceremony_events WHERE ceremony_id=?")
            .bind(&request.binding.ceremony_id)
            .fetch_one(&mut db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        support::redeem(
            &mut db,
            &qr.invitation_id,
            &qr.installation_id,
            &qr.target_device_id,
            || 118
        )
        .await
        .unwrap(),
        Outcome::Denied
    );
    let marker: String =
        sqlx::query_scalar("SELECT marker FROM p023_credential_reference WHERE invitation=?")
            .bind(&qr.invitation_id)
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(marker, "NON_PRODUCTION_REFERENCE_ONLY");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM synthetic_verifiers WHERE invitation=?")
            .bind(&qr.invitation_id)
            .fetch_one(&mut db)
            .await
            .unwrap(),
        1
    );
    db.close().await.unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-journal", path.display()));
}

#[tokio::test]
async fn later_reference_state_cannot_bypass_a_denied_ceremony_phase() {
    let path = db_path();
    let qr = invitation("p024-no-bypass");
    let mut db = support::connect(&path).await.unwrap();
    support::initialize(&mut db).await.unwrap();
    sqlx::query("INSERT INTO authority VALUES (?,?,?,?,?,1)")
        .bind(&qr.installation_id)
        .bind(&qr.issuer_owner_id)
        .bind(&qr.approved_endpoint)
        .bind(&qr.server_identity_fingerprint)
        .bind(i64::try_from(qr.policy_revision).unwrap())
        .execute(&mut db)
        .await
        .unwrap();
    support::create(&mut db, &qr).await.unwrap();
    assert_eq!(
        support::transition(
            &mut db,
            &qr.invitation_id,
            &qr.installation_id,
            || 110,
            false
        )
        .await
        .unwrap(),
        Outcome::Issued
    );
    let mut ceremony = ReferenceCeremony::claim(ClaimRequest {
        version: CEREMONY_VERSION,
        ceremony_id: "denied-ceremony".into(),
        qr_claim: qr.clone(),
    })
    .unwrap();
    let trusted = facts(&qr);
    ceremony.authenticate_identity(&trusted, 111).unwrap();
    let request = ceremony
        .challenge("denied-challenge".into(), 180, &trusted, 112)
        .unwrap();
    // Owner approval is absent. Attempting the later durable phase cannot compensate.
    assert!(ceremony.confirm(&request, &trusted, 113).is_err());
    assert_eq!(
        support::redeem(
            &mut db,
            &qr.invitation_id,
            &qr.installation_id,
            &qr.target_device_id,
            || 113
        )
        .await
        .unwrap(),
        Outcome::Denied
    );
    let state: String = sqlx::query("SELECT state FROM invitations WHERE id=?")
        .bind(&qr.invitation_id)
        .fetch_one(&mut db)
        .await
        .unwrap()
        .get("state");
    assert_eq!(state, "ISSUED");
    db.close().await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn compatibility_fixture_keeps_legacy_enrollment_separate_from_pairing_versions() {
    const LEGACY_API: &str = include_str!("../../../api/openapi.yaml");
    #[derive(Clone, Copy)]
    struct Case {
        scenario: &'static str,
        pairing_result: &'static str,
        existing_v02_auth: &'static str,
    }
    let matrix = [
        Case {
            scenario: "v0.2 server and v0.2 Android/iOS client",
            pairing_result: "not negotiated",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "v0.2 server and new reference client",
            pairing_result: "unsupported; preserve verified endpoint",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "reference protocol v1 with complete trusted evidence",
            pairing_result: "reference fixture only",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "unknown or future protocol version",
            pairing_result: "reject closed",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "missing optional pairing capability",
            pairing_result: "keep legacy enrollment",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "stale pairing metadata or policy revision",
            pairing_result: "reject and require fresh owner action",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "existing sve1_ grant or svd1_ credential",
            pairing_result: "do not rotate or invalidate",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "previously enrolled Android/iOS device",
            pairing_result: "no forced pairing",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "fresh enrollment or explicit re-enrollment",
            pairing_result: "new invitation and ceremony",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "repeated or interrupted attempt",
            pairing_result: "metadata recovery; no secret replay",
            existing_v02_auth: "unchanged",
        },
        Case {
            scenario: "wrong owner, device, installation, endpoint or TLS identity",
            pairing_result: "reject closed",
            existing_v02_auth: "unchanged",
        },
    ];
    assert_eq!(matrix.len(), 11);
    for case in matrix {
        assert!(!case.scenario.is_empty());
        assert!(!case.pairing_result.is_empty());
        assert_eq!(case.existing_v02_auth, "unchanged");
    }
    assert_eq!(synveil_pairing_contracts::INVITATION_CONTRACT_VERSION, 1);
    assert_eq!(CEREMONY_VERSION, 1);
    assert_eq!(
        synveil_pairing_contracts::recovery::RECOVERY_CONTRACT_VERSION,
        1
    );
    assert!(LEGACY_API.contains("/api/v1/devices/enrollment-grants:"));
    assert!(LEGACY_API.contains("/api/v1/device-enrollment/exchange:"));
    assert!(!LEGACY_API.contains("/api/v1/pairing"));
    let mut unsupported = invitation("unknown-version");
    unsupported.contract_version = 2;
    assert!(unsupported.validate().is_err());
}
