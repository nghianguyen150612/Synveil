mod support;

use sqlx::{Connection, Row, SqliteConnection};
use std::{path::PathBuf, sync::Arc};
use support::{LedgerError, Outcome};
use synveil_pairing_contracts::{ENROLLMENT_AUDIENCE, PairingQrPayload, ceremony::*};

fn qr() -> PairingQrPayload {
    PairingQrPayload {
        contract_version: 1,
        invitation_id: "inv".into(),
        installation_id: "install".into(),
        issuer_owner_id: "owner".into(),
        audience: ENROLLMENT_AUDIENCE.into(),
        approved_endpoint: "https://reference.example.test".into(),
        server_identity_fingerprint: format!("sha256:{}", "a".repeat(64)),
        target_device_id: "device".into(),
        issued_at_unix: 100,
        expires_at_unix: 200,
        policy_revision: 7,
    }
}

fn facts() -> ReferenceFacts {
    ReferenceFacts {
        authoritative_invitation: qr(),
        issuer_active: true,
        target_device_active: true,
        independent_identity: Some(ReferenceServerIdentity {
            installation_id: "install".into(),
            endpoint: "https://reference.example.test".into(),
            fingerprint: format!("sha256:{}", "a".repeat(64)),
            tls: ReferenceTls::CertificateAndHostnameAuthenticated,
        }),
        invitation_claim_verified: true,
        owner_approval: None,
        client_consent: None,
    }
}

fn claim() -> ClaimRequest {
    ClaimRequest {
        version: CEREMONY_VERSION,
        ceremony_id: "ceremony".into(),
        qr_claim: qr(),
    }
}

fn challenged() -> (ReferenceCeremony, BoundRequest, ReferenceFacts) {
    let mut ceremony = ReferenceCeremony::claim(claim()).unwrap();
    let mut f = facts();
    ceremony.authenticate_identity(&f, 110).unwrap();
    let request = ceremony
        .challenge("challenge".into(), 180, &f, 111)
        .unwrap();
    f.owner_approval = Some(decision(&request, "owner", "approval", 112));
    f.client_consent = Some(decision(&request, "device", "consent", 114));
    (ceremony, request, f)
}

fn decision(r: &BoundRequest, principal: &str, id: &str, issued: i64) -> ReferenceDecision {
    ReferenceDecision {
        evidence_id: id.into(),
        principal_id: principal.into(),
        binding: r.binding.clone(),
        issued_at_unix: issued,
        expires_at_unix: 170,
        revoked: false,
    }
}

fn ready() -> (ReferenceCeremony, BoundRequest, ReferenceFacts) {
    let (mut c, r, f) = challenged();
    c.approve(&r, &f, 113).unwrap();
    c.consent(&r, &f, 115).unwrap();
    (c, r, f)
}

#[test]
fn strict_dtos_have_no_authority_flags_or_credentials() {
    let (_, r, _) = challenged();
    let claim_value = serde_json::to_value(claim()).unwrap();
    let bound_value = serde_json::to_value(&r).unwrap();
    for field in [
        "owner_approved",
        "client_consented",
        "tls_valid",
        "proof_verified",
        "target_device_active",
        "token",
        "credential",
    ] {
        let mut value = claim_value.clone();
        value[field] = true.into();
        assert!(serde_json::from_value::<ClaimRequest>(value).is_err());
        let mut value = bound_value.clone();
        value[field] = true.into();
        assert!(serde_json::from_value::<BoundRequest>(value).is_err());
    }
    let mut nested = bound_value.clone();
    nested["binding"]["invitation"]["enrollment_token"] = "sve1_forbidden".into();
    assert!(serde_json::from_value::<BoundRequest>(nested).is_err());
    let mut duplicate = serde_json::to_string(&r).unwrap();
    duplicate.insert_str(1, "\"version\":1,");
    assert!(serde_json::from_str::<BoundRequest>(&duplicate).is_err());
    let mut bad = claim();
    bad.version = 2;
    assert_eq!(
        ReferenceCeremony::claim(bad).unwrap_err(),
        CeremonyError::Malformed
    );
    let response = ready().0.response();
    let mut value = serde_json::to_value(response).unwrap();
    value["credential"] = "forbidden".into();
    assert!(serde_json::from_value::<CeremonyResponse>(value).is_err());
}

#[test]
fn qr_even_matching_valid_tls_peer_cannot_supply_independent_identity() {
    let mut c = ReferenceCeremony::claim(claim()).unwrap();
    let mut f = facts();
    f.independent_identity = None;
    assert_eq!(
        c.authenticate_identity(&f, 110),
        Err(CeremonyError::MissingIndependentIdentity)
    );
    assert_eq!(c.response().state, CeremonyState::Claimed);
    // Attacker substitutes both QR and invitation claim to name their valid
    // HTTPS server. The independent installation reference remains unchanged.
    let mut substituted = claim();
    substituted.qr_claim.approved_endpoint = "https://attacker.example.test".into();
    substituted.qr_claim.server_identity_fingerprint = format!("sha256:{}", "b".repeat(64));
    f = facts();
    f.authoritative_invitation = substituted.qr_claim.clone();
    let mut c = ReferenceCeremony::claim(substituted).unwrap();
    assert_eq!(
        c.authenticate_identity(&f, 110),
        Err(CeremonyError::IdentityMismatch)
    );
}

#[test]
fn every_invitation_field_is_bound_and_every_identity_failure_denies() {
    for field in 0..11 {
        let (mut c, r, mut f) = ready();
        let q = &mut f.authoritative_invitation;
        match field {
            0 => q.contract_version += 1,
            1 => q.invitation_id.push('x'),
            2 => q.installation_id.push('x'),
            3 => q.issuer_owner_id.push('x'),
            4 => q.audience.push('x'),
            5 => q.approved_endpoint.push('/'),
            6 => q.server_identity_fingerprint = format!("sha256:{}", "b".repeat(64)),
            7 => q.target_device_id.push('x'),
            8 => q.issued_at_unix += 1,
            9 => q.expires_at_unix += 1,
            _ => q.policy_revision += 1,
        }
        assert_eq!(c.confirm(&r, &f, 116), Err(CeremonyError::WrongBinding));
    }
    for case in 0..9 {
        let (mut c, r, mut f) = ready();
        let identity = f.independent_identity.as_mut().unwrap();
        let expected = match case {
            0 => {
                identity.fingerprint = format!("sha256:{}", "b".repeat(64));
                CeremonyError::IdentityMismatch
            }
            1 => {
                identity.endpoint = "https://other.test".into();
                CeremonyError::IdentityMismatch
            }
            2 => {
                identity.installation_id = "other".into();
                CeremonyError::IdentityMismatch
            }
            3 => {
                identity.tls = ReferenceTls::CertificateOrHostnameMismatch;
                CeremonyError::InvalidTls
            }
            4 => {
                identity.tls = ReferenceTls::Unauthenticated;
                CeremonyError::InvalidTls
            }
            5 => {
                identity.tls = ReferenceTls::Redirected;
                CeremonyError::InvalidTls
            }
            6 => {
                f.invitation_claim_verified = false;
                CeremonyError::MissingClaimProof
            }
            7 => {
                f.issuer_active = false;
                CeremonyError::Revoked
            }
            _ => {
                f.independent_identity = None;
                CeremonyError::MissingIndependentIdentity
            }
        };
        assert_eq!(c.confirm(&r, &f, 116), Err(expected));
        assert_ne!(c.response().state, CeremonyState::Confirmed);
    }
}

#[test]
fn approval_and_consent_are_separate_and_cannot_skip_or_reorder_steps() {
    let (mut c, r, mut f) = challenged();
    assert_eq!(
        c.confirm(&r, &f, 115),
        Err(CeremonyError::InvalidTransition)
    );
    assert_eq!(
        c.consent(&r, &f, 115),
        Err(CeremonyError::InvalidTransition)
    );
    f.owner_approval = None;
    assert_eq!(
        c.approve(&r, &f, 113),
        Err(CeremonyError::MissingOwnerApproval)
    );
    f.owner_approval = Some(decision(&r, "owner", "approval", 112));
    c.approve(&r, &f, 113).unwrap();
    f.client_consent = None;
    assert_eq!(
        c.consent(&r, &f, 115),
        Err(CeremonyError::MissingClientConsent)
    );
    assert_eq!(
        c.confirm(&r, &f, 115),
        Err(CeremonyError::InvalidTransition)
    );
    f.client_consent = Some(decision(&r, "device", "consent", 111));
    assert_eq!(c.consent(&r, &f, 115), Err(CeremonyError::InvalidDecision));
    f.client_consent = Some(decision(&r, "device", "approval", 114));
    assert_eq!(c.consent(&r, &f, 115), Err(CeremonyError::InvalidDecision));
}

#[test]
fn challenge_substitution_and_replayed_decisions_fail_exact_binding() {
    for case in 0..7 {
        let (mut c, mut r, f) = ready();
        match case {
            0 => r.version += 1,
            1 => r.binding.version += 1,
            2 => r.binding.ceremony_id.push('x'),
            3 => r.binding.challenge_id.push('x'),
            4 => r.binding.issued_at_unix += 1,
            5 => r.binding.expires_at_unix += 1,
            _ => r.binding.invitation.target_device_id.push('x'),
        }
        assert_eq!(c.confirm(&r, &f, 116), Err(CeremonyError::WrongBinding));
    }
    for owner in [true, false] {
        for case in 0..10 {
            let (mut c, r, mut f) = ready();
            let e = if owner {
                f.owner_approval.as_mut().unwrap()
            } else {
                f.client_consent.as_mut().unwrap()
            };
            match case {
                0 => e.principal_id = "other".into(),
                1 => e.binding.invitation.installation_id.push('x'),
                2 => e.binding.challenge_id.push('x'),
                3 => e.issued_at_unix = 110,
                4 => e.issued_at_unix = 117,
                5 => e.expires_at_unix = 116,
                6 => e.expires_at_unix = 181,
                7 => e.evidence_id = "replacement".into(),
                8 => e.revoked = true,
                _ => e.expires_at_unix = 169, // same ID cannot rewrite accepted window
            }
            assert!(c.confirm(&r, &f, 116).is_err());
            assert_ne!(c.response().state, CeremonyState::Confirmed);
        }
    }
}

#[test]
fn time_boundaries_revocation_and_terminal_states_never_revive() {
    let (mut c, r, f) = ready();
    assert_eq!(c.confirm(&r, &f, 114), Err(CeremonyError::ClockRegression));
    assert_eq!(c.confirm(&r, &f, 170), Err(CeremonyError::InvalidDecision));
    assert_eq!(c.confirm(&r, &f, 180), Err(CeremonyError::Expired));
    assert_eq!(
        c.confirm(&r, &f, 116),
        Err(CeremonyError::InvalidTransition)
    );
    for state in [
        CeremonyState::Cancelled,
        CeremonyState::Revoked,
        CeremonyState::Confirmed,
    ] {
        let (mut c, r, f) = ready();
        match state {
            CeremonyState::Cancelled => c.cancel(&r).unwrap(),
            CeremonyState::Revoked => c.revoke().unwrap(),
            _ => c.confirm(&r, &f, 116).unwrap(),
        }
        assert_eq!(
            c.confirm(&r, &f, 117),
            Err(CeremonyError::InvalidTransition)
        );
        assert_eq!(c.cancel(&r), Err(CeremonyError::InvalidTransition));
        assert_eq!(c.recover(&r, &f).unwrap().state, state);
    }
    let (mut c, r, mut f) = ready();
    f.client_consent.as_mut().unwrap().revoked = true;
    assert_eq!(c.confirm(&r, &f, 116), Err(CeremonyError::Revoked));
    f.client_consent.as_mut().unwrap().revoked = false;
    assert_eq!(
        c.confirm(&r, &f, 117),
        Err(CeremonyError::InvalidTransition)
    );
}

#[test]
fn response_validation_challenge_lifetimes_and_early_withdrawal() {
    let mut c = ReferenceCeremony::claim(claim()).unwrap();
    c.response().validate().unwrap();
    c.revoke().unwrap();
    assert_eq!(
        c.authenticate_identity(&facts(), 110),
        Err(CeremonyError::InvalidTransition)
    );
    let mut c = ReferenceCeremony::claim(claim()).unwrap();
    c.authenticate_identity(&facts(), 110).unwrap();
    for expiry in [110, 111, 201] {
        assert_eq!(
            c.challenge("new".into(), expiry, &facts(), 111),
            Err(CeremonyError::Malformed)
        );
    }
    assert_eq!(
        c.challenge("".into(), 180, &facts(), 111),
        Err(CeremonyError::Malformed)
    );
    for case in 0..6 {
        let mut response = ready().0.response();
        response.validate().unwrap();
        match case {
            0 => response.version += 1,
            1 => response.ceremony_id.push('x'),
            2 => response.challenge = None,
            3 => response.challenge.as_mut().unwrap().expires_at_unix = 201,
            4 => response.challenge.as_mut().unwrap().version += 1,
            _ => response.state = CeremonyState::Claimed,
        }
        assert_eq!(response.validate(), Err(CeremonyError::Malformed));
    }
}

#[test]
fn device_revocation_denies_independently_of_issuer_and_consent_and_never_revives() {
    let (mut c, r, mut f) = ready();
    f.target_device_active = false;
    assert!(f.issuer_active);
    assert!(!f.client_consent.as_ref().unwrap().revoked);
    assert_eq!(c.confirm(&r, &f, 116), Err(CeremonyError::DeviceRevoked));
    assert_eq!(c.response().state, CeremonyState::Revoked);
    assert_eq!(c.recover(&r, &f), Err(CeremonyError::DeviceRevoked));
    f.target_device_active = true;
    assert_eq!(
        c.confirm(&r, &f, 117),
        Err(CeremonyError::InvalidTransition)
    );
    let mut c = ReferenceCeremony::claim(claim()).unwrap();
    f.target_device_active = false;
    assert_eq!(
        c.authenticate_identity(&f, 110),
        Err(CeremonyError::DeviceRevoked)
    );
}

#[test]
fn decisions_from_previous_ceremony_cannot_authorize_a_new_ceremony() {
    let (_, old, mut f) = challenged();
    let mut request = claim();
    request.ceremony_id = "different-ceremony".into();
    let mut c = ReferenceCeremony::claim(request).unwrap();
    c.authenticate_identity(&f, 110).unwrap();
    let new = c
        .challenge("different-challenge".into(), 180, &f, 111)
        .unwrap();
    assert_eq!(
        c.approve(&new, &f, 113),
        Err(CeremonyError::InvalidDecision)
    );
    assert_eq!(c.approve(&old, &f, 113), Err(CeremonyError::WrongBinding));
    f.owner_approval = Some(decision(&new, "owner", "new-approval", 112));
    c.approve(&new, &f, 113).unwrap();
    assert_eq!(
        c.consent(&new, &f, 115),
        Err(CeremonyError::InvalidDecision)
    );
}

// Only this test harness writes identity observations and participant events.
// These tables are never production migrations or client-controlled requests.
struct Fixture(PathBuf);
impl Fixture {
    async fn new() -> Self {
        let f = Self(
            std::env::temp_dir().join(format!("synveil-p022-{}.sqlite", uuid::Uuid::now_v7())),
        );
        let mut db = f.open().await;
        support::initialize(&mut db).await.unwrap();
        sqlx::raw_sql("CREATE TABLE ceremonies(id TEXT PRIMARY KEY, invitation TEXT UNIQUE, challenge TEXT UNIQUE, snapshot TEXT NOT NULL);
            CREATE TABLE independent_identity(installation TEXT, endpoint TEXT, fingerprint TEXT, tls INTEGER);
            CREATE TABLE device_authority(device TEXT PRIMARY KEY, active INTEGER NOT NULL CHECK(active IN (0,1)));
            INSERT INTO device_authority VALUES ('device',1);
            CREATE TABLE ceremony_events(kind TEXT PRIMARY KEY, evidence_id TEXT UNIQUE, principal TEXT, binding TEXT, issued INTEGER, expires INTEGER, revoked INTEGER);")
            .execute(&mut db).await.unwrap();
        let q = qr();
        sqlx::query("INSERT INTO authority VALUES (?,?,?,?,?,1)")
            .bind(&q.installation_id)
            .bind(&q.issuer_owner_id)
            .bind(&q.approved_endpoint)
            .bind(&q.server_identity_fingerprint)
            .bind(7_i64)
            .execute(&mut db)
            .await
            .unwrap();
        // Independent trusted setup, deliberately not copied from a scanned QR.
        sqlx::query("INSERT INTO independent_identity VALUES ('install','https://reference.example.test',?,1)")
            .bind(format!("sha256:{}", "a".repeat(64))).execute(&mut db).await.unwrap();
        support::create(&mut db, &q).await.unwrap();
        assert_eq!(
            support::transition(&mut db, "inv", "install", || 101, false)
                .await
                .unwrap(),
            Outcome::Issued
        );
        let (c, r, facts) = challenged();
        sqlx::query("INSERT INTO ceremonies VALUES ('ceremony','inv','challenge',?)")
            .bind(serde_json::to_string(&c).unwrap())
            .execute(&mut db)
            .await
            .unwrap();
        for (kind, event) in [
            ("owner", facts.owner_approval.unwrap()),
            ("client", facts.client_consent.unwrap()),
        ] {
            sqlx::query("INSERT INTO ceremony_events VALUES (?,?,?,?,?,?,0)")
                .bind(kind)
                .bind(event.evidence_id)
                .bind(event.principal_id)
                .bind(serde_json::to_string(&r.binding).unwrap())
                .bind(event.issued_at_unix)
                .bind(event.expires_at_unix)
                .execute(&mut db)
                .await
                .unwrap();
        }
        db.close().await.unwrap();
        f
    }

    async fn open(&self) -> SqliteConnection {
        support::connect(&self.0).await.unwrap()
    }

    async fn ready(&self) {
        let mut db = self.open().await;
        let r = challenged().1;
        assert_eq!(
            step(&mut db, &r, Action::Approve, || 113).await.unwrap(),
            Ok(())
        );
        assert_eq!(
            step(&mut db, &r, Action::Consent, || 115).await.unwrap(),
            Ok(())
        );
        db.close().await.unwrap();
    }

    async fn check(&self, expected: CeremonyState, verifiers: i64) {
        let mut db = self.open().await;
        assert_eq!(snapshot(&mut db).await.unwrap().response().state, expected);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM synthetic_verifiers")
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(count, verifiers);
        let ledger: String = sqlx::query_scalar("SELECT state FROM invitations")
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(ledger, if verifiers == 1 { "CONSUMED" } else { "ISSUED" });
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            "ok"
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut db)
                .await
                .unwrap()
                .is_empty()
        );
        db.close().await.unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn snapshot(db: &mut SqliteConnection) -> Result<ReferenceCeremony, LedgerError> {
    let json: String = sqlx::query_scalar("SELECT snapshot FROM ceremonies WHERE id='ceremony'")
        .fetch_one(db)
        .await?;
    Ok(serde_json::from_str(&json)?)
}

async fn event(
    db: &mut SqliteConnection,
    kind: &str,
) -> Result<Option<ReferenceDecision>, LedgerError> {
    let row = sqlx::query("SELECT * FROM ceremony_events WHERE kind=?")
        .bind(kind)
        .fetch_optional(db)
        .await?;
    row.map(|r| {
        Ok(ReferenceDecision {
            evidence_id: r.get("evidence_id"),
            principal_id: r.get("principal"),
            binding: serde_json::from_str(r.get("binding"))?,
            issued_at_unix: r.get("issued"),
            expires_at_unix: r.get("expires"),
            revoked: r.get::<i64, _>("revoked") != 0,
        })
    })
    .transpose()
}

async fn current_facts(db: &mut SqliteConnection) -> Result<ReferenceFacts, LedgerError> {
    let json: String = sqlx::query_scalar("SELECT binding FROM invitations WHERE id='inv'")
        .fetch_one(&mut *db)
        .await?;
    let mut invitation: PairingQrPayload = serde_json::from_str(&json)?;
    // Installation is trusted server context, never an argument from the request.
    let authority = sqlx::query("SELECT * FROM authority WHERE installation='install'")
        .fetch_one(&mut *db)
        .await?;
    invitation.issuer_owner_id = authority.get("owner");
    invitation.approved_endpoint = authority.get("endpoint");
    invitation.server_identity_fingerprint = authority.get("fingerprint");
    invitation.policy_revision =
        u64::try_from(authority.get::<i64, _>("policy")).map_err(|_| LedgerError)?;
    let identity = sqlx::query("SELECT * FROM independent_identity")
        .fetch_optional(&mut *db)
        .await?;
    let target_device_active =
        sqlx::query_scalar::<_, i64>("SELECT active FROM device_authority WHERE device=?")
            .bind(&invitation.target_device_id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or(0)
            == 1;
    Ok(ReferenceFacts {
        authoritative_invitation: invitation,
        issuer_active: authority.get::<i64, _>("active") == 1,
        target_device_active,
        independent_identity: identity.map(|r| ReferenceServerIdentity {
            installation_id: r.get("installation"),
            endpoint: r.get("endpoint"),
            fingerprint: r.get("fingerprint"),
            tls: if r.get::<i64, _>("tls") == 1 {
                ReferenceTls::CertificateAndHostnameAuthenticated
            } else {
                ReferenceTls::CertificateOrHostnameMismatch
            },
        }),
        // Trusted fixture observation only, not a selected proof mechanism.
        invitation_claim_verified: true,
        owner_approval: event(db, "owner").await?,
        client_consent: event(db, "client").await?,
    })
}

#[derive(Clone, Copy)]
enum Action {
    Approve,
    Consent,
    Confirm,
    Cancel,
}

async fn step(
    db: &mut SqliteConnection,
    request: &BoundRequest,
    action: Action,
    clock: impl FnOnce() -> i64,
) -> Result<Result<(), CeremonyError>, LedgerError> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let now = clock();
    let mut c = snapshot(&mut tx).await?;
    let facts = current_facts(&mut tx).await?;
    let result = match action {
        Action::Approve => c.approve(request, &facts, now),
        Action::Consent => c.consent(request, &facts, now),
        Action::Confirm => c.confirm(request, &facts, now),
        Action::Cancel => c.cancel(request),
    };
    if result.is_ok() && matches!(action, Action::Confirm) {
        // P021's collapsed approval is a projection of the completed P022
        // checks. It is created only inside this same transaction, then consumed.
        sqlx::query("INSERT INTO approvals SELECT id,binding FROM invitations WHERE id='inv'")
            .execute(&mut *tx)
            .await?;
        if support::redeem_locked(&mut tx, "inv", "install", "device", || now).await?
            != Outcome::Consumed
        {
            return Err(LedgerError);
        }
    }
    sqlx::query("UPDATE ceremonies SET snapshot=? WHERE id='ceremony'")
        .bind(serde_json::to_string(&c)?)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result)
}

#[tokio::test]
async fn ceremony_gate_consumes_p021_only_after_both_decisions_and_recovers_metadata() {
    let f = Fixture::new().await;
    let mut db = f.open().await;
    assert_eq!(
        support::redeem(&mut db, "inv", "install", "device", || 115)
            .await
            .unwrap(),
        Outcome::Denied
    );
    db.close().await.unwrap();
    f.ready().await;
    f.check(CeremonyState::ReadyToConfirm, 0).await;
    let mut db = f.open().await;
    // Model a committed confirmation whose response is lost, then reconnect.
    step(&mut db, &challenged().1, Action::Confirm, || 116)
        .await
        .unwrap()
        .unwrap();
    db.close().await.unwrap();
    f.check(CeremonyState::Confirmed, 1).await;
    let mut db = f.open().await;
    let c = snapshot(&mut db).await.unwrap();
    let r = challenged().1;
    assert_eq!(
        c.recover(&r, &current_facts(&mut db).await.unwrap())
            .unwrap()
            .state,
        CeremonyState::Confirmed
    );
    assert_eq!(
        step(&mut db, &r, Action::Confirm, || 117).await.unwrap(),
        Err(CeremonyError::InvalidTransition)
    );
    assert_eq!(
        support::redeem(&mut db, "inv", "install", "device", || 117)
            .await
            .unwrap(),
        Outcome::Denied
    );
    db.close().await.unwrap();
    f.check(CeremonyState::Confirmed, 1).await;
}

#[tokio::test]
async fn lost_approval_and_consent_responses_recover_without_reissuing_or_consuming() {
    let f = Fixture::new().await;
    let r = challenged().1;
    for (action, now, state) in [
        (Action::Approve, 113, CeremonyState::AwaitingClientConsent),
        (Action::Consent, 115, CeremonyState::ReadyToConfirm),
    ] {
        let mut db = f.open().await;
        step(&mut db, &r, action, || now).await.unwrap().unwrap();
        db.close().await.unwrap();
        let mut db = f.open().await;
        let c = snapshot(&mut db).await.unwrap();
        assert_eq!(
            c.recover(&r, &current_facts(&mut db).await.unwrap())
                .unwrap()
                .state,
            state
        );
        assert_eq!(
            step(&mut db, &r, action, || now).await.unwrap(),
            Err(CeremonyError::InvalidTransition)
        );
        db.close().await.unwrap();
        f.check(state, 0).await;
    }
    let mut db = f.open().await;
    for (id, inv, challenge) in [("other", "inv", "other"), ("other", "other", "challenge")] {
        assert!(
            sqlx::query("INSERT INTO ceremonies VALUES (?,?,?,'{}')")
                .bind(id)
                .bind(inv)
                .bind(challenge)
                .execute(&mut db)
                .await
                .is_err()
        );
    }
    db.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixteen_independent_confirmations_commit_once() {
    let f = Fixture::new().await;
    f.ready().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let mut db = f.open().await;
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let result = step(&mut db, &challenged().1, Action::Confirm, || 116)
                .await
                .unwrap();
            db.close().await.unwrap();
            result
        }));
    }
    let mut wins = 0;
    for task in tasks {
        if task.await.unwrap().is_ok() {
            wins += 1;
        }
    }
    assert_eq!(wins, 1);
    f.check(CeremonyState::Confirmed, 1).await;
}

#[tokio::test]
async fn committed_authority_changes_stale_or_revoked_decisions_and_expiry_deny() {
    for change in [
        "UPDATE authority SET active=0",
        "UPDATE device_authority SET active=0",
        "DELETE FROM device_authority",
        "UPDATE authority SET policy=8",
        "UPDATE authority SET endpoint='https://other.test'",
        "UPDATE authority SET fingerprint='wrong'",
        "UPDATE ceremony_events SET revoked=1 WHERE kind='owner'",
        "UPDATE ceremony_events SET revoked=1 WHERE kind='client'",
        "UPDATE ceremony_events SET expires=116 WHERE kind='owner'",
        "UPDATE ceremony_events SET expires=116 WHERE kind='client'",
        "UPDATE ceremony_events SET binding='{}'",
        "DELETE FROM ceremony_events",
        "UPDATE independent_identity SET tls=0",
        "DELETE FROM independent_identity",
    ] {
        let f = Fixture::new().await;
        f.ready().await;
        let mut db = f.open().await;
        sqlx::query(change).execute(&mut db).await.unwrap();
        let result = step(&mut db, &challenged().1, Action::Confirm, || 116).await;
        assert!(!matches!(result, Ok(Ok(()))), "{change}");
        let state = snapshot(&mut db).await.unwrap().response().state;
        db.close().await.unwrap();
        f.check(state, 0).await;
    }
    let f = Fixture::new().await;
    f.ready().await;
    let mut db = f.open().await;
    assert_eq!(
        step(&mut db, &challenged().1, Action::Confirm, || 180)
            .await
            .unwrap(),
        Err(CeremonyError::Expired)
    );
    db.close().await.unwrap();
    f.check(CeremonyState::Expired, 0).await;
}

#[tokio::test]
async fn failed_p021_insert_rolls_back_ceremony_approval_and_consumption() {
    let f = Fixture::new().await;
    f.ready().await;
    let mut db = f.open().await;
    sqlx::raw_sql("CREATE TRIGGER fail_p022 BEFORE INSERT ON synthetic_verifiers BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")
        .execute(&mut db).await.unwrap();
    assert!(
        step(&mut db, &challenged().1, Action::Confirm, || 116)
            .await
            .is_err()
    );
    db.close().await.unwrap();
    f.check(CeremonyState::ReadyToConfirm, 0).await;
    let mut db = f.open().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM approvals")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        0
    );
    sqlx::query("DROP TRIGGER fail_p022")
        .execute(&mut db)
        .await
        .unwrap();
    step(&mut db, &challenged().1, Action::Confirm, || 117)
        .await
        .unwrap()
        .unwrap();
    db.close().await.unwrap();
    f.check(CeremonyState::Confirmed, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_and_confirmation_race_has_one_terminal_outcome() {
    let f = Fixture::new().await;
    f.ready().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for action in [Action::Cancel, Action::Confirm] {
        let mut db = f.open().await;
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let result = step(&mut db, &challenged().1, action, || 116)
                .await
                .unwrap();
            db.close().await.unwrap();
            result
        }));
    }
    let mut wins = 0;
    for task in tasks {
        if task.await.unwrap().is_ok() {
            wins += 1;
        }
    }
    assert_eq!(wins, 1);
    let mut db = f.open().await;
    let state = snapshot(&mut db).await.unwrap().response().state;
    db.close().await.unwrap();
    assert!(matches!(
        state,
        CeremonyState::Cancelled | CeremonyState::Confirmed
    ));
    f.check(state, i64::from(state == CeremonyState::Confirmed))
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revocation_writer_fence_and_post_lock_clock_are_observed() {
    for scenario in 0..3 {
        let f = Fixture::new().await;
        f.ready().await;
        let mut writer = f.open().await;
        let mut tx = writer.begin_with("BEGIN IMMEDIATE").await.unwrap();
        if scenario != 2 {
            sqlx::query(if scenario == 0 {
                "UPDATE ceremony_events SET revoked=1 WHERE kind='owner'"
            } else {
                "UPDATE device_authority SET active=0"
            })
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(116));
        let clock2 = clock.clone();
        let mut other = f.open().await;
        let (started, waiting) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            started.send(()).unwrap();
            let result = step(&mut other, &challenged().1, Action::Confirm, || {
                clock2.load(std::sync::atomic::Ordering::SeqCst)
            })
            .await
            .unwrap();
            other.close().await.unwrap();
            result
        });
        waiting.await.unwrap();
        if scenario == 2 {
            clock.store(180, std::sync::atomic::Ordering::SeqCst);
        }
        tx.commit().await.unwrap();
        writer.close().await.unwrap();
        assert_eq!(
            task.await.unwrap(),
            Err(match scenario {
                0 => CeremonyError::Revoked,
                1 => CeremonyError::DeviceRevoked,
                _ => CeremonyError::Expired,
            })
        );
        f.check(
            if scenario != 2 {
                CeremonyState::Revoked
            } else {
                CeremonyState::Expired
            },
            0,
        )
        .await;
    }
}
