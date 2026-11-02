mod support;

use sqlx::{Connection, Row, SqliteConnection};
use std::{path::PathBuf, sync::Arc};
use support::{LedgerError, Outcome};
use synveil_pairing_contracts::{
    ENROLLMENT_AUDIENCE, PairingQrPayload,
    ceremony::{
        BoundRequest, CEREMONY_VERSION, CeremonyError, CeremonyState, ClaimRequest,
        ReferenceCeremony, ReferenceDecision, ReferenceFacts, ReferenceServerIdentity,
        ReferenceTls,
    },
    recovery::{
        CredentialReferenceStateV1, OwnerReenrollmentApproval, RECOVERY_CONTRACT_VERSION,
        RecoveryActionV1, RecoveryBindingV1, RecoveryDisposition, RecoveryRecordV1,
        RecoveryRejection, RecoveryRequestV1, RecoveryStateV1, TrustedRecoveryEvidence,
        qualify_recovery,
    },
};

fn fingerprint(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn qr(invitation_id: &str, _ceremony_id: &str) -> PairingQrPayload {
    PairingQrPayload {
        contract_version: 1,
        invitation_id: invitation_id.into(),
        installation_id: "install".into(),
        issuer_owner_id: "owner".into(),
        audience: ENROLLMENT_AUDIENCE.into(),
        approved_endpoint: "https://reference.example.test".into(),
        server_identity_fingerprint: fingerprint('a'),
        target_device_id: "device".into(),
        issued_at_unix: 100,
        expires_at_unix: 200,
        policy_revision: 7,
    }
}

fn recovery_binding(qr: &PairingQrPayload, ceremony_id: &str) -> RecoveryBindingV1 {
    RecoveryBindingV1 {
        installation_id: qr.installation_id.clone(),
        owner_id: qr.issuer_owner_id.clone(),
        device_id: qr.target_device_id.clone(),
        invitation_id: qr.invitation_id.clone(),
        ceremony_id: ceremony_id.into(),
        server_identity_fingerprint: qr.server_identity_fingerprint.clone(),
        policy_revision: qr.policy_revision,
    }
}

fn base_facts(qr: &PairingQrPayload) -> ReferenceFacts {
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

fn prepared_pairing(
    qr: &PairingQrPayload,
    ceremony_id: &str,
) -> (
    ReferenceCeremony,
    BoundRequest,
    ReferenceDecision,
    ReferenceDecision,
) {
    let mut ceremony = ReferenceCeremony::claim(ClaimRequest {
        version: CEREMONY_VERSION,
        ceremony_id: ceremony_id.into(),
        qr_claim: qr.clone(),
    })
    .unwrap();
    let mut facts = base_facts(qr);
    ceremony.authenticate_identity(&facts, 110).unwrap();
    let request = ceremony
        .challenge(format!("challenge-{ceremony_id}"), 180, &facts, 111)
        .unwrap();
    let approval = ReferenceDecision {
        evidence_id: format!("approval-{ceremony_id}"),
        principal_id: qr.issuer_owner_id.clone(),
        binding: request.binding.clone(),
        issued_at_unix: 112,
        expires_at_unix: 170,
        revoked: false,
    };
    let consent = ReferenceDecision {
        evidence_id: format!("consent-{ceremony_id}"),
        principal_id: qr.target_device_id.clone(),
        binding: request.binding.clone(),
        issued_at_unix: 114,
        expires_at_unix: 170,
        revoked: false,
    };
    facts.owner_approval = Some(approval.clone());
    facts.client_consent = Some(consent.clone());
    ceremony.approve(&request, &facts, 113).unwrap();
    ceremony.consent(&request, &facts, 115).unwrap();
    (ceremony, request, approval, consent)
}

#[derive(Clone, Copy)]
enum Invalidation {
    CancelPending,
    RevokeInvitation,
    RevokeOwner,
    RevokeDevice,
}

struct Fixture {
    path: PathBuf,
    owns_file: bool,
}

impl Fixture {
    async fn new() -> Self {
        let file =
            std::env::temp_dir().join(format!("synveil-p023-{}.sqlite", uuid::Uuid::now_v7()));
        let mut db = support::connect(&file).await.unwrap();
        support::initialize(&mut db).await.unwrap();
        sqlx::raw_sql(include_str!("support/schema_p023.sql"))
            .execute(&mut db)
            .await
            .unwrap();
        let q = qr("inv", "ceremony");
        sqlx::query("INSERT INTO authority VALUES (?,?,?,?,?,1)")
            .bind(&q.installation_id)
            .bind(&q.issuer_owner_id)
            .bind(&q.approved_endpoint)
            .bind(&q.server_identity_fingerprint)
            .bind(7_i64)
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO independent_identity VALUES (?,?,?,1)")
            .bind(&q.installation_id)
            .bind(&q.approved_endpoint)
            .bind(&q.server_identity_fingerprint)
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO device_authority VALUES ('device',1)")
            .execute(&mut db)
            .await
            .unwrap();
        insert_pairing(&mut db, &q, "recovery-inv", 101)
            .await
            .unwrap();
        db.close().await.unwrap();
        Self {
            path: file,
            owns_file: true,
        }
    }

    async fn open(&self) -> SqliteConnection {
        support::connect(&self.path).await.unwrap()
    }

    async fn confirm(&self, invitation_id: &str, ceremony_id: &str) -> Result<(), CeremonyError> {
        self.confirm_at(invitation_id, ceremony_id, 116).await
    }

    async fn confirm_at(
        &self,
        invitation_id: &str,
        ceremony_id: &str,
        now: i64,
    ) -> Result<(), CeremonyError> {
        let mut db = self.open().await;
        let (_, request, _, _) = prepared_pairing(&qr(invitation_id, ceremony_id), ceremony_id);
        let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let result = confirm_locked(&mut tx, &request, now).await.unwrap();
        if result.is_ok() {
            tx.commit().await.unwrap();
        } else {
            tx.rollback().await.unwrap();
        }
        db.close().await.unwrap();
        result
    }

    async fn integrity(&self) {
        let mut db = self.open().await;
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
        if self.owns_file {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(format!("{}-journal", self.path.display()));
        }
    }
}

async fn insert_pairing(
    db: &mut SqliteConnection,
    qr: &PairingQrPayload,
    recovery_id: &str,
    now: i64,
) -> Result<(), LedgerError> {
    support::create(db, qr).await?;
    if support::transition(db, &qr.invitation_id, &qr.installation_id, || now, false).await?
        != Outcome::Issued
    {
        return Err(LedgerError);
    }
    insert_pairing_metadata(db, qr, recovery_id, now).await
}

async fn insert_pairing_metadata(
    db: &mut SqliteConnection,
    qr: &PairingQrPayload,
    recovery_id: &str,
    now: i64,
) -> Result<(), LedgerError> {
    let (ceremony, request, approval, consent) =
        prepared_pairing(qr, &format!("ceremony-{}", qr.invitation_id));
    sqlx::query("INSERT INTO ceremonies VALUES (?,?,?,?)")
        .bind(&request.binding.ceremony_id)
        .bind(&qr.invitation_id)
        .bind(&request.binding.challenge_id)
        .bind(serde_json::to_string(&ceremony)?)
        .execute(&mut *db)
        .await?;
    for (kind, event) in [("owner", approval), ("client", consent)] {
        sqlx::query("INSERT INTO ceremony_events VALUES (?,?,?,?,?,?,?,0)")
            .bind(&request.binding.ceremony_id)
            .bind(kind)
            .bind(event.evidence_id)
            .bind(event.principal_id)
            .bind(serde_json::to_string(&request.binding)?)
            .bind(event.issued_at_unix)
            .bind(event.expires_at_unix)
            .execute(&mut *db)
            .await?;
    }
    let record = RecoveryRecordV1::new(
        recovery_id,
        recovery_binding(qr, &request.binding.ceremony_id),
        RecoveryStateV1::PairingPending,
        now,
    )
    .map_err(|_| LedgerError)?;
    sqlx::query("INSERT INTO p023_recovery_records VALUES (?,?,?,?,?,?)")
        .bind(i64::from(record.contract_version))
        .bind(&record.recovery_id)
        .bind(&qr.invitation_id)
        .bind(serde_json::to_string(&record.binding)?)
        .bind(state_sql(record.state))
        .bind(record.recorded_at_unix)
        .execute(&mut *db)
        .await?;
    Ok(())
}

async fn ceremony_snapshot(
    db: &mut SqliteConnection,
    ceremony_id: &str,
) -> Result<ReferenceCeremony, LedgerError> {
    let snapshot: String = sqlx::query_scalar("SELECT snapshot FROM ceremonies WHERE id=?")
        .bind(ceremony_id)
        .fetch_one(&mut *db)
        .await?;
    Ok(serde_json::from_str(&snapshot)?)
}

async fn recovery_record(
    db: &mut SqliteConnection,
    invitation_id: &str,
) -> Result<RecoveryRecordV1, LedgerError> {
    // Persisted state uses SQL enum names; the authoritative versioned JSON is
    // reconstructed from the immutable binding plus the typed SQL state.
    let row = sqlx::query(
        "SELECT contract_version,recovery_id,binding,state,recorded_at FROM p023_recovery_records WHERE invitation=?",
    )
    .bind(invitation_id)
    .fetch_one(&mut *db)
    .await?;
    let contract_version: i64 = row.get("contract_version");
    if contract_version != i64::from(RECOVERY_CONTRACT_VERSION) {
        return Err(LedgerError);
    }
    let recovery_id: String = row.get("recovery_id");
    let binding_json: String = row.get("binding");
    let state: String = row.get("state");
    let recorded_at: i64 = row.get("recorded_at");
    let binding: RecoveryBindingV1 = serde_json::from_str(&binding_json)?;
    RecoveryRecordV1::new(recovery_id, binding, state_from_sql(&state), recorded_at)
        .map_err(|_| LedgerError)
}

async fn update_recovery_record(
    db: &mut SqliteConnection,
    record: &RecoveryRecordV1,
) -> Result<(), LedgerError> {
    sqlx::query("UPDATE p023_recovery_records SET state=?,recorded_at=? WHERE recovery_id=?")
        .bind(state_sql(record.state))
        .bind(record.recorded_at_unix)
        .bind(&record.recovery_id)
        .execute(&mut *db)
        .await?;
    Ok(())
}

async fn event(
    db: &mut SqliteConnection,
    ceremony_id: &str,
    kind: &str,
) -> Result<Option<ReferenceDecision>, LedgerError> {
    let row = sqlx::query("SELECT * FROM ceremony_events WHERE ceremony_id=? AND kind=?")
        .bind(ceremony_id)
        .bind(kind)
        .fetch_optional(&mut *db)
        .await?;
    row.map(|row| {
        Ok(ReferenceDecision {
            evidence_id: row.get("evidence_id"),
            principal_id: row.get("principal"),
            binding: serde_json::from_str(row.get("binding"))?,
            issued_at_unix: row.get("issued"),
            expires_at_unix: row.get("expires"),
            revoked: row.get::<i64, _>("revoked") != 0,
        })
    })
    .transpose()
}

async fn current_facts(
    db: &mut SqliteConnection,
    ceremony_id: &str,
) -> Result<ReferenceFacts, LedgerError> {
    let ceremony_row = sqlx::query("SELECT invitation FROM ceremonies WHERE id=?")
        .bind(ceremony_id)
        .fetch_one(&mut *db)
        .await?;
    let invitation_id: String = ceremony_row.get("invitation");
    let binding: String = sqlx::query_scalar("SELECT binding FROM invitations WHERE id=?")
        .bind(&invitation_id)
        .fetch_one(&mut *db)
        .await?;
    let mut invitation: PairingQrPayload = serde_json::from_str(&binding)?;
    let authority = sqlx::query("SELECT * FROM authority WHERE installation=?")
        .bind(&invitation.installation_id)
        .fetch_optional(&mut *db)
        .await?;
    let issuer_is_active = if let Some(authority) = authority {
        invitation.issuer_owner_id = authority.get("owner");
        invitation.approved_endpoint = authority.get("endpoint");
        invitation.server_identity_fingerprint = authority.get("fingerprint");
        let policy = u64::try_from(authority.get::<i64, _>("policy")).map_err(|_| LedgerError)?;
        invitation.policy_revision = policy;
        authority.get::<i64, _>("active") == 1
    } else {
        false
    };
    let independent_identity = sqlx::query(
        "SELECT installation,endpoint,fingerprint,tls FROM independent_identity WHERE installation=?",
    )
    .bind(&invitation.installation_id)
    .fetch_optional(&mut *db)
    .await?
    .map(|row| ReferenceServerIdentity {
        installation_id: row.get("installation"),
        endpoint: row.get("endpoint"),
        fingerprint: row.get("fingerprint"),
        tls: if row.get::<i64, _>("tls") == 1 {
            ReferenceTls::CertificateAndHostnameAuthenticated
        } else {
            ReferenceTls::CertificateOrHostnameMismatch
        },
    });
    let target_device_active =
        sqlx::query_scalar::<_, i64>("SELECT active FROM device_authority WHERE device=?")
            .bind(&invitation.target_device_id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or_default()
            == 1;
    Ok(ReferenceFacts {
        authoritative_invitation: invitation,
        issuer_active: issuer_is_active,
        target_device_active,
        independent_identity,
        invitation_claim_verified: true,
        owner_approval: event(db, ceremony_id, "owner").await?,
        client_consent: event(db, ceremony_id, "client").await?,
    })
}

async fn confirm_locked(
    db: &mut SqliteConnection,
    request: &BoundRequest,
    now: i64,
) -> Result<Result<(), CeremonyError>, LedgerError> {
    let mut ceremony = ceremony_snapshot(db, &request.binding.ceremony_id).await?;
    let facts = current_facts(db, &request.binding.ceremony_id).await?;
    if let Err(error) = ceremony.confirm(request, &facts, now) {
        return Ok(Err(error));
    }
    sqlx::query(
        "INSERT INTO approvals SELECT id,binding FROM invitations WHERE id=? AND state='ISSUED'",
    )
    .bind(&request.binding.invitation.invitation_id)
    .execute(&mut *db)
    .await?;
    if support::redeem_locked(
        db,
        &request.binding.invitation.invitation_id,
        &request.binding.invitation.installation_id,
        &request.binding.invitation.target_device_id,
        || now,
    )
    .await?
        != Outcome::Consumed
    {
        return Ok(Err(CeremonyError::Revoked));
    }
    sqlx::query("UPDATE ceremonies SET snapshot=? WHERE id=?")
        .bind(serde_json::to_string(&ceremony)?)
        .bind(&request.binding.ceremony_id)
        .execute(&mut *db)
        .await?;
    let invitation = &request.binding.invitation;
    let mut record = recovery_record(db, &invitation.invitation_id).await?;
    record
        .transition(RecoveryStateV1::Enrolled, now)
        .map_err(|_| LedgerError)?;
    update_recovery_record(db, &record).await?;
    sqlx::query("INSERT INTO p023_credential_reference VALUES (?,?,?,?,?,?,'ACTIVE',?,?,'NON_PRODUCTION_REFERENCE_ONLY')")
        .bind(i64::from(RECOVERY_CONTRACT_VERSION))
        .bind(&invitation.invitation_id)
        .bind(&invitation.installation_id)
        .bind(&invitation.issuer_owner_id)
        .bind(&invitation.target_device_id)
        .bind(i64::try_from(invitation.policy_revision).map_err(|_| LedgerError)?)
        .bind(now)
        .bind(now)
        .execute(&mut *db)
        .await?;
    Ok(Ok(()))
}

async fn mark_credential_state(
    fixture: &Fixture,
    invitation_id: &str,
    state: CredentialReferenceStateV1,
    now: i64,
) -> Result<(), LedgerError> {
    let (sql_state, recovery_state) = match state {
        CredentialReferenceStateV1::Expired => ("EXPIRED", RecoveryStateV1::CredentialExpired),
        CredentialReferenceStateV1::SecureStorageLost => {
            ("SECURE_STORAGE_LOST", RecoveryStateV1::SecureStorageLost)
        }
        CredentialReferenceStateV1::SecureStorageInaccessible => (
            "SECURE_STORAGE_INACCESSIBLE",
            RecoveryStateV1::SecureStorageInaccessible,
        ),
        CredentialReferenceStateV1::AmbiguousIssuance => (
            "AMBIGUOUS_ISSUANCE",
            RecoveryStateV1::AmbiguousCredentialIssuance,
        ),
        _ => return Err(LedgerError),
    };
    let mut db = fixture.open().await;
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let changed = sqlx::query(
        "UPDATE p023_credential_reference SET state=?,changed_at=? WHERE invitation=? AND state='ACTIVE'",
    )
    .bind(sql_state)
    .bind(now)
    .bind(invitation_id)
    .execute(&mut *tx)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(LedgerError);
    }
    let mut record = recovery_record(&mut tx, invitation_id).await?;
    record
        .transition(recovery_state, now)
        .map_err(|_| LedgerError)?;
    update_recovery_record(&mut tx, &record).await?;
    tx.commit().await?;
    db.close().await?;
    Ok(())
}

async fn invalidate_pending(
    fixture: &Fixture,
    invitation_id: &str,
    invalidation: Invalidation,
    now: i64,
) -> Result<(), LedgerError> {
    let mut db = fixture.open().await;
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let mut record = recovery_record(&mut tx, invitation_id).await?;
    if record.state == RecoveryStateV1::Enrolled
        && matches!(
            invalidation,
            Invalidation::RevokeOwner | Invalidation::RevokeDevice
        )
    {
        let (next_state, update) = match invalidation {
            Invalidation::RevokeOwner => (
                RecoveryStateV1::OwnerRevoked,
                "UPDATE authority SET active=0 WHERE installation=?",
            ),
            Invalidation::RevokeDevice => (
                RecoveryStateV1::DeviceRevoked,
                "UPDATE device_authority SET active=0 WHERE device=?",
            ),
            Invalidation::CancelPending | Invalidation::RevokeInvitation => {
                return Err(LedgerError);
            }
        };
        let key = if matches!(invalidation, Invalidation::RevokeOwner) {
            &record.binding.installation_id
        } else {
            &record.binding.device_id
        };
        sqlx::query(update).bind(key).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE p023_credential_reference SET state='REVOKED',changed_at=? WHERE invitation=? AND state!='REVOKED'",
        )
        .bind(now)
        .bind(invitation_id)
        .execute(&mut *tx)
        .await?;
        record
            .transition(next_state, now)
            .map_err(|_| LedgerError)?;
        update_recovery_record(&mut tx, &record).await?;
        tx.commit().await?;
        db.close().await?;
        return Ok(());
    }
    if record.state != RecoveryStateV1::PairingPending {
        return Err(LedgerError);
    }
    let ceremony_id = record.binding.ceremony_id.clone();
    let mut ceremony = ceremony_snapshot(&mut tx, &ceremony_id).await?;
    match invalidation {
        Invalidation::CancelPending => {
            let (_, request, _, _) =
                prepared_pairing(&qr(invitation_id, &ceremony_id), &ceremony_id);
            ceremony.cancel(&request).map_err(|_| LedgerError)?;
            record
                .transition(RecoveryStateV1::PairingCancellationPending, now)
                .map_err(|_| LedgerError)?;
            update_recovery_record(&mut tx, &record).await?;
            record
                .transition(RecoveryStateV1::PairingCancelled, now)
                .map_err(|_| LedgerError)?;
        }
        Invalidation::RevokeInvitation => {
            ceremony.revoke().map_err(|_| LedgerError)?;
            record
                .transition(RecoveryStateV1::InvitationRevoked, now)
                .map_err(|_| LedgerError)?;
        }
        Invalidation::RevokeOwner => {
            sqlx::query("UPDATE authority SET active=0 WHERE installation=?")
                .bind(&record.binding.installation_id)
                .execute(&mut *tx)
                .await?;
            ceremony.revoke().map_err(|_| LedgerError)?;
            record
                .transition(RecoveryStateV1::OwnerRevoked, now)
                .map_err(|_| LedgerError)?;
        }
        Invalidation::RevokeDevice => {
            sqlx::query("UPDATE device_authority SET active=0 WHERE device=?")
                .bind(&record.binding.device_id)
                .execute(&mut *tx)
                .await?;
            ceremony.revoke().map_err(|_| LedgerError)?;
            record
                .transition(RecoveryStateV1::DeviceRevoked, now)
                .map_err(|_| LedgerError)?;
        }
    }
    sqlx::query("UPDATE invitations SET state='CANCELLED' WHERE id=? AND state='ISSUED'")
        .bind(invitation_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE ceremonies SET snapshot=? WHERE id=?")
        .bind(serde_json::to_string(&ceremony)?)
        .bind(&ceremony_id)
        .execute(&mut *tx)
        .await?;
    update_recovery_record(&mut tx, &record).await?;
    tx.commit().await?;
    db.close().await?;
    Ok(())
}

async fn revoke_enrolled(
    fixture: &Fixture,
    invitation_id: &str,
    revoke_owner: bool,
    now: i64,
) -> Result<(), LedgerError> {
    let mut db = fixture.open().await;
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let mut record = recovery_record(&mut tx, invitation_id).await?;
    if record.state != RecoveryStateV1::Enrolled {
        return Err(LedgerError);
    }
    let (state, table, key) = if revoke_owner {
        (
            RecoveryStateV1::OwnerRevoked,
            "authority",
            record.binding.installation_id.clone(),
        )
    } else {
        (
            RecoveryStateV1::DeviceRevoked,
            "device_authority",
            record.binding.device_id.clone(),
        )
    };
    let update = if table == "authority" {
        "UPDATE authority SET active=0 WHERE installation=?"
    } else {
        "UPDATE device_authority SET active=0 WHERE device=?"
    };
    sqlx::query(update).bind(key).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE p023_credential_reference SET state='REVOKED',changed_at=? WHERE invitation=? AND state!='REVOKED'",
    )
    .bind(now)
    .bind(invitation_id)
    .execute(&mut *tx)
    .await?;
    record.transition(state, now).map_err(|_| LedgerError)?;
    update_recovery_record(&mut tx, &record).await?;
    tx.commit().await?;
    db.close().await?;
    Ok(())
}

async fn load_recovery_evidence(
    db: &mut SqliteConnection,
    recovery_id: &str,
    now: i64,
    requested_new_invitation_id: Option<&str>,
) -> Result<TrustedRecoveryEvidence, LedgerError> {
    let recovery_row =
        sqlx::query("SELECT invitation FROM p023_recovery_records WHERE recovery_id=?")
            .bind(recovery_id)
            .fetch_one(&mut *db)
            .await?;
    let invitation_id: String = recovery_row.get("invitation");
    let record = recovery_record(&mut *db, &invitation_id).await?;
    let authority =
        sqlx::query("SELECT active,policy,fingerprint FROM authority WHERE installation=?")
            .bind(&record.binding.installation_id)
            .fetch_optional(&mut *db)
            .await?;
    let (owner_active, current_policy_revision, current_server_identity_fingerprint) =
        if let Some(authority) = authority {
            (
                authority.get::<i64, _>("active") == 1,
                u64::try_from(authority.get::<i64, _>("policy")).map_err(|_| LedgerError)?,
                authority.get("fingerprint"),
            )
        } else {
            (false, 0, String::new())
        };
    let device_active =
        sqlx::query_scalar::<_, i64>("SELECT active FROM device_authority WHERE device=?")
            .bind(&record.binding.device_id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or_default()
            == 1;
    let credential_status = sqlx::query_scalar::<_, String>(
        "SELECT state FROM p023_credential_reference WHERE invitation=?",
    )
    .bind(&invitation_id)
    .fetch_optional(&mut *db)
    .await?;
    let credential_state = match credential_status.as_deref() {
        None => CredentialReferenceStateV1::NotIssued,
        Some("ACTIVE") => CredentialReferenceStateV1::Active,
        Some("REVOKED") => CredentialReferenceStateV1::Revoked,
        Some("EXPIRED") => CredentialReferenceStateV1::Expired,
        Some("SECURE_STORAGE_LOST") => CredentialReferenceStateV1::SecureStorageLost,
        Some("SECURE_STORAGE_INACCESSIBLE") => {
            CredentialReferenceStateV1::SecureStorageInaccessible
        }
        Some("AMBIGUOUS_ISSUANCE") => CredentialReferenceStateV1::AmbiguousIssuance,
        Some(_) => return Err(LedgerError),
    };
    let owner_reenrollment_approval = if let Some(new_invitation) = requested_new_invitation_id {
        sqlx::query("SELECT * FROM p023_owner_reenrollment_actions WHERE recovery_id=? AND new_invitation=?")
            .bind(recovery_id).bind(new_invitation).fetch_optional(&mut *db).await?
            .map(|row| OwnerReenrollmentApproval {
                evidence_id: row.get("evidence_id"),
                owner_id: row.get("owner"),
                recovery_id: row.get("recovery_id"),
                source_invitation_id: row.get("source_invitation"),
                requested_new_invitation_id: row.get("new_invitation"),
                device_id: row.get("device"),
                approved_at_unix: row.get("approved_at"),
                expires_at_unix: row.get("expires_at"),
                revoked: row.get::<i64, _>("revoked") != 0,
            })
    } else {
        None
    };
    Ok(TrustedRecoveryEvidence {
        record,
        current_policy_revision,
        current_server_identity_fingerprint,
        owner_active,
        device_active,
        credential_state,
        observed_at_unix: now,
        owner_reenrollment_approval,
    })
}

fn client_request(
    record: &RecoveryRecordV1,
    action: RecoveryActionV1,
    new_invitation_id: Option<&str>,
) -> RecoveryRequestV1 {
    RecoveryRequestV1 {
        contract_version: 1,
        recovery_id: record.recovery_id.clone(),
        invitation_id: record.binding.invitation_id.clone(),
        claimed_installation_id: record.binding.installation_id.clone(),
        claimed_owner_id: record.binding.owner_id.clone(),
        claimed_device_id: record.binding.device_id.clone(),
        claimed_policy_revision: record.binding.policy_revision,
        action,
        requested_new_invitation_id: new_invitation_id.map(str::to_owned),
    }
}

async fn insert_owner_action(
    fixture: &Fixture,
    record: &RecoveryRecordV1,
    new_invitation_id: &str,
    now: i64,
) -> Result<(), LedgerError> {
    let mut db = fixture.open().await;
    sqlx::query("INSERT INTO p023_owner_reenrollment_actions VALUES (?,?,?,?,?,?,?,?,0)")
        .bind(format!("owner-action-{new_invitation_id}"))
        .bind(&record.recovery_id)
        .bind(&record.binding.owner_id)
        .bind(&record.binding.invitation_id)
        .bind(new_invitation_id)
        .bind(&record.binding.device_id)
        .bind(now)
        .bind(now + 20)
        .execute(&mut db)
        .await?;
    db.close().await?;
    Ok(())
}

async fn begin_reenrollment(
    fixture: &Fixture,
    request: &RecoveryRequestV1,
    new_qr: &PairingQrPayload,
    request_id: &str,
    now: i64,
) -> Result<Outcome, LedgerError> {
    let mut db = fixture.open().await;
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let Some(new_invitation_id) = request.requested_new_invitation_id.as_deref() else {
        return Err(LedgerError);
    };
    let evidence =
        load_recovery_evidence(&mut tx, &request.recovery_id, now, Some(new_invitation_id)).await?;
    if qualify_recovery(request, &evidence) != RecoveryDisposition::FreshPairingMayStart
        || new_qr.invitation_id != new_invitation_id
        || new_qr.invitation_id == evidence.record.binding.invitation_id
        || new_qr.installation_id != evidence.record.binding.installation_id
        || new_qr.issuer_owner_id != evidence.record.binding.owner_id
        || new_qr.target_device_id != evidence.record.binding.device_id
        || new_qr.server_identity_fingerprint != evidence.record.binding.server_identity_fingerprint
        || new_qr.policy_revision != evidence.current_policy_revision
        || new_qr.validate().is_err()
    {
        return Ok(Outcome::Denied);
    }
    let attempt_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM p023_reenrollment_attempts WHERE source_invitation=? OR request_id=?",
    )
    .bind(&evidence.record.binding.invitation_id)
    .bind(request_id)
    .fetch_one(&mut *tx)
    .await?;
    if attempt_count != 0 {
        return Ok(Outcome::Denied);
    }
    support::create(&mut tx, new_qr).await?;
    if support::issue_locked(
        &mut tx,
        &new_qr.invitation_id,
        &new_qr.installation_id,
        || now,
    )
    .await?
        != Outcome::Issued
    {
        return Ok(Outcome::Denied);
    }
    sqlx::query("INSERT INTO p023_reenrollment_attempts VALUES (?,?,?,?)")
        .bind(&evidence.record.binding.invitation_id)
        .bind(request_id)
        .bind(&new_qr.invitation_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    insert_pairing_metadata(
        &mut tx,
        new_qr,
        &format!("recovery-{}", new_qr.invitation_id),
        now,
    )
    .await?;
    tx.commit().await?;
    db.close().await?;
    Ok(Outcome::Issued)
}

fn state_sql(state: RecoveryStateV1) -> &'static str {
    match state {
        RecoveryStateV1::PairingPending => "PAIRING_PENDING",
        RecoveryStateV1::PairingCancellationPending => "PAIRING_CANCELLATION_PENDING",
        RecoveryStateV1::PairingCancelled => "PAIRING_CANCELLED",
        RecoveryStateV1::InvitationRevoked => "INVITATION_REVOKED",
        RecoveryStateV1::OwnerRevoked => "OWNER_REVOKED",
        RecoveryStateV1::DeviceRevoked => "DEVICE_REVOKED",
        RecoveryStateV1::Enrolled => "ENROLLED",
        RecoveryStateV1::CredentialExpired => "CREDENTIAL_EXPIRED",
        RecoveryStateV1::SecureStorageLost => "SECURE_STORAGE_LOST",
        RecoveryStateV1::SecureStorageInaccessible => "SECURE_STORAGE_INACCESSIBLE",
        RecoveryStateV1::AmbiguousCredentialIssuance => "AMBIGUOUS_CREDENTIAL_ISSUANCE",
        RecoveryStateV1::ExplicitReenrollmentRequired => "EXPLICIT_REENROLLMENT_REQUIRED",
        RecoveryStateV1::ReenrollmentStarted => "REENROLLMENT_STARTED",
    }
}

fn state_from_sql(state: &str) -> RecoveryStateV1 {
    match state {
        "PAIRING_PENDING" => RecoveryStateV1::PairingPending,
        "PAIRING_CANCELLATION_PENDING" => RecoveryStateV1::PairingCancellationPending,
        "PAIRING_CANCELLED" => RecoveryStateV1::PairingCancelled,
        "INVITATION_REVOKED" => RecoveryStateV1::InvitationRevoked,
        "OWNER_REVOKED" => RecoveryStateV1::OwnerRevoked,
        "DEVICE_REVOKED" => RecoveryStateV1::DeviceRevoked,
        "ENROLLED" => RecoveryStateV1::Enrolled,
        "CREDENTIAL_EXPIRED" => RecoveryStateV1::CredentialExpired,
        "SECURE_STORAGE_LOST" => RecoveryStateV1::SecureStorageLost,
        "SECURE_STORAGE_INACCESSIBLE" => RecoveryStateV1::SecureStorageInaccessible,
        "AMBIGUOUS_CREDENTIAL_ISSUANCE" => RecoveryStateV1::AmbiguousCredentialIssuance,
        "EXPLICIT_REENROLLMENT_REQUIRED" => RecoveryStateV1::ExplicitReenrollmentRequired,
        "REENROLLMENT_STARTED" => RecoveryStateV1::ReenrollmentStarted,
        _ => panic!("unknown P023 test state"),
    }
}

#[tokio::test]
async fn interrupted_pairing_and_client_restart_recover_only_metadata() {
    let fixture = Fixture::new().await;
    let mut db = fixture.open().await;
    let record = recovery_record(&mut db, "inv").await.unwrap();
    let request = client_request(&record, RecoveryActionV1::Inspect, None);
    let evidence = load_recovery_evidence(&mut db, &record.recovery_id, 116, None)
        .await
        .unwrap();
    assert_eq!(
        qualify_recovery(&request, &evidence),
        RecoveryDisposition::MetadataOnly(RecoveryStateV1::PairingPending)
    );
    let ceremony = ceremony_snapshot(&mut db, "ceremony-inv").await.unwrap();
    assert_eq!(
        ceremony
            .recover(
                &prepared_pairing(&qr("inv", "ceremony-inv"), "ceremony-inv").1,
                &current_facts(&mut db, "ceremony-inv").await.unwrap()
            )
            .unwrap()
            .state,
        CeremonyState::ReadyToConfirm
    );
    db.close().await.unwrap();
    assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
    fixture.integrity().await;
}

#[tokio::test]
async fn committed_confirmation_response_loss_requires_fresh_enrollment_and_never_replays() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
    mark_credential_state(
        &fixture,
        "inv",
        CredentialReferenceStateV1::AmbiguousIssuance,
        117,
    )
    .await
    .unwrap();
    let mut db = fixture.open().await;
    let record = recovery_record(&mut db, "inv").await.unwrap();
    let inspect = client_request(&record, RecoveryActionV1::Inspect, None);
    let evidence = load_recovery_evidence(&mut db, &record.recovery_id, 118, None)
        .await
        .unwrap();
    assert_eq!(
        qualify_recovery(&inspect, &evidence),
        RecoveryDisposition::FreshEnrollmentRequired(RecoveryStateV1::AmbiguousCredentialIssuance)
    );
    assert_eq!(
        support::redeem(&mut db, "inv", "install", "device", || 119)
            .await
            .unwrap(),
        Outcome::Denied
    );
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('p023_credential_reference') ORDER BY cid",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    assert!(
        !columns
            .iter()
            .any(|name| name.contains("secret") || name.contains("token"))
    );
    db.close().await.unwrap();
    let sqlite_bytes = std::fs::read(&fixture.path).unwrap();
    for forbidden in [
        "sve1_",
        "svd1_",
        "credential_token",
        "device_credential",
        "secret_digest",
    ] {
        assert!(
            !sqlite_bytes
                .windows(forbidden.len())
                .any(|window| window == forbidden.as_bytes())
        );
    }
    fixture.integrity().await;
}

#[tokio::test]
async fn credential_expiry_storage_loss_and_storage_lock_require_explicit_reenrollment() {
    for state in [
        CredentialReferenceStateV1::Expired,
        CredentialReferenceStateV1::SecureStorageLost,
        CredentialReferenceStateV1::SecureStorageInaccessible,
    ] {
        let fixture = Fixture::new().await;
        assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
        mark_credential_state(&fixture, "inv", state, 117)
            .await
            .unwrap();
        let mut db = fixture.open().await;
        let record = recovery_record(&mut db, "inv").await.unwrap();
        let request = client_request(
            &record,
            RecoveryActionV1::BeginExplicitReenrollment,
            Some("new-inv"),
        );
        let evidence = load_recovery_evidence(&mut db, &record.recovery_id, 118, Some("new-inv"))
            .await
            .unwrap();
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::ExplicitOwnerActionRequired)
        );
        db.close().await.unwrap();
        fixture.integrity().await;
    }
}

#[tokio::test]
async fn explicit_reenrollment_uses_fresh_p020_p021_p022_ids_and_keeps_old_state_terminal() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
    mark_credential_state(
        &fixture,
        "inv",
        CredentialReferenceStateV1::SecureStorageLost,
        117,
    )
    .await
    .unwrap();
    let mut db = fixture.open().await;
    let old = recovery_record(&mut db, "inv").await.unwrap();
    db.close().await.unwrap();
    insert_owner_action(&fixture, &old, "new-inv", 118)
        .await
        .unwrap();
    let request = client_request(
        &old,
        RecoveryActionV1::BeginExplicitReenrollment,
        Some("new-inv"),
    );
    let new_qr = qr("new-inv", "ceremony-new-inv");
    assert_eq!(
        begin_reenrollment(&fixture, &request, &new_qr, "attempt-1", 119)
            .await
            .unwrap(),
        Outcome::Issued
    );
    assert_eq!(
        begin_reenrollment(&fixture, &request, &new_qr, "attempt-2", 120)
            .await
            .unwrap(),
        Outcome::Denied
    );
    let mut db = fixture.open().await;
    let old_state: String = sqlx::query_scalar("SELECT state FROM invitations WHERE id='inv'")
        .fetch_one(&mut db)
        .await
        .unwrap();
    let new_state: String = sqlx::query_scalar("SELECT state FROM invitations WHERE id='new-inv'")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(old_state, "CONSUMED");
    assert_eq!(new_state, "ISSUED");
    assert_eq!(
        ceremony_snapshot(&mut db, "ceremony-new-inv")
            .await
            .unwrap()
            .response()
            .state,
        CeremonyState::ReadyToConfirm
    );
    db.close().await.unwrap();
    assert_eq!(
        fixture.confirm_at("new-inv", "ceremony-new-inv", 120).await,
        Ok(())
    );
    fixture.integrity().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reenrollment_attempts_reserve_only_one_fresh_invitation() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
    mark_credential_state(
        &fixture,
        "inv",
        CredentialReferenceStateV1::SecureStorageLost,
        117,
    )
    .await
    .unwrap();
    let mut db = fixture.open().await;
    let old = recovery_record(&mut db, "inv").await.unwrap();
    db.close().await.unwrap();
    for new_id in ["new-a", "new-b"] {
        insert_owner_action(&fixture, &old, new_id, 118)
            .await
            .unwrap();
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for new_id in ["new-a", "new-b"] {
        let fixture_path = fixture.path.clone();
        let barrier = barrier.clone();
        let request = client_request(
            &old,
            RecoveryActionV1::BeginExplicitReenrollment,
            Some(new_id),
        );
        let new_qr = qr(new_id, &format!("ceremony-{new_id}"));
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let fixture = Fixture {
                path: fixture_path,
                owns_file: false,
            };
            begin_reenrollment(
                &fixture,
                &request,
                &new_qr,
                &format!("request-{new_id}"),
                119,
            )
            .await
            .unwrap()
        }));
    }
    let mut issued = 0;
    for task in tasks {
        if task.await.unwrap() == Outcome::Issued {
            issued += 1;
        }
    }
    assert_eq!(issued, 1);
    let mut db = fixture.open().await;
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM p023_reenrollment_attempts")
        .fetch_one(&mut db)
        .await
        .unwrap();
    let new_invitations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM invitations WHERE id IN ('new-a','new-b')")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(attempts, 1);
    assert_eq!(new_invitations, 1);
    db.close().await.unwrap();
    fixture.integrity().await;
}

#[tokio::test]
async fn revocation_owner_device_identity_rotation_and_stale_policy_fail_closed() {
    for invalidation in [Invalidation::CancelPending, Invalidation::RevokeInvitation] {
        let fixture = Fixture::new().await;
        invalidate_pending(&fixture, "inv", invalidation, 116)
            .await
            .unwrap();
        assert_ne!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
        let mut db = fixture.open().await;
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT state FROM invitations WHERE id='inv'")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            "CANCELLED"
        );
        assert_eq!(
            ceremony_snapshot(&mut db, "ceremony-inv")
                .await
                .unwrap()
                .response()
                .state,
            if matches!(invalidation, Invalidation::CancelPending) {
                CeremonyState::Cancelled
            } else {
                CeremonyState::Revoked
            }
        );
        db.close().await.unwrap();
    }

    for invalidation in [Invalidation::RevokeOwner, Invalidation::RevokeDevice] {
        let fixture = Fixture::new().await;
        invalidate_pending(&fixture, "inv", invalidation, 116)
            .await
            .unwrap();
        assert_ne!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
        let mut db = fixture.open().await;
        let record = recovery_record(&mut db, "inv").await.unwrap();
        let req = client_request(
            &record,
            RecoveryActionV1::BeginExplicitReenrollment,
            Some("replacement"),
        );
        let evidence =
            load_recovery_evidence(&mut db, &record.recovery_id, 117, Some("replacement"))
                .await
                .unwrap();
        assert!(matches!(
            qualify_recovery(&req, &evidence),
            RecoveryDisposition::Denied(
                RecoveryRejection::OwnerRevoked | RecoveryRejection::DeviceRevoked
            )
        ));
        db.close().await.unwrap();
    }

    for (column, value, expected) in [
        ("policy", "8", RecoveryRejection::StalePolicy),
        (
            "fingerprint",
            "rotated",
            RecoveryRejection::ServerIdentityRotated,
        ),
    ] {
        let fixture = Fixture::new().await;
        let mut db = fixture.open().await;
        if column == "policy" {
            sqlx::query("UPDATE authority SET policy=8")
                .execute(&mut db)
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE authority SET fingerprint='rotated'")
                .execute(&mut db)
                .await
                .unwrap();
        }
        let record = recovery_record(&mut db, "inv").await.unwrap();
        let req = client_request(&record, RecoveryActionV1::Inspect, None);
        let evidence = load_recovery_evidence(&mut db, &record.recovery_id, 116, None)
            .await
            .unwrap();
        assert_eq!(
            qualify_recovery(&req, &evidence),
            RecoveryDisposition::Denied(expected),
            "{column}={value}"
        );
        let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert_ne!(
            confirm_locked(
                &mut tx,
                &prepared_pairing(&qr("inv", "ceremony-inv"), "ceremony-inv").1,
                116,
            )
            .await
            .unwrap(),
            Ok(())
        );
        tx.rollback().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT state FROM invitations WHERE id='inv'")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            "ISSUED"
        );
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn device_or_owner_revocation_after_confirmation_revokes_reference_credential() {
    for revoke_owner in [false, true] {
        let fixture = Fixture::new().await;
        assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
        revoke_enrolled(&fixture, "inv", revoke_owner, 117)
            .await
            .unwrap();
        let mut db = fixture.open().await;
        let record = recovery_record(&mut db, "inv").await.unwrap();
        let request = client_request(
            &record,
            RecoveryActionV1::BeginExplicitReenrollment,
            Some("replacement"),
        );
        let evidence =
            load_recovery_evidence(&mut db, &record.recovery_id, 118, Some("replacement"))
                .await
                .unwrap();
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(if revoke_owner {
                RecoveryRejection::OwnerRevoked
            } else {
                RecoveryRejection::DeviceRevoked
            })
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM p023_credential_reference WHERE invitation='inv'",
            )
            .fetch_one(&mut db)
            .await
            .unwrap(),
            "REVOKED"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT state FROM invitations WHERE id='inv'")
                .fetch_one(&mut db)
                .await
                .unwrap(),
            "CONSUMED"
        );
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn cached_owner_reenrollment_evidence_cannot_bypass_later_device_revocation() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
    mark_credential_state(
        &fixture,
        "inv",
        CredentialReferenceStateV1::SecureStorageLost,
        117,
    )
    .await
    .unwrap();
    let mut db = fixture.open().await;
    let record = recovery_record(&mut db, "inv").await.unwrap();
    db.close().await.unwrap();
    insert_owner_action(&fixture, &record, "new-inv", 118)
        .await
        .unwrap();
    let request = client_request(
        &record,
        RecoveryActionV1::BeginExplicitReenrollment,
        Some("new-inv"),
    );
    let mut db = fixture.open().await;
    let previously_approved =
        load_recovery_evidence(&mut db, &record.recovery_id, 119, Some("new-inv"))
            .await
            .unwrap();
    assert_eq!(
        qualify_recovery(&request, &previously_approved),
        RecoveryDisposition::FreshPairingMayStart
    );
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE device_authority SET active=0 WHERE device='device'")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE p023_credential_reference SET state='REVOKED',changed_at=120 WHERE invitation='inv'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    db.close().await.unwrap();
    assert_eq!(
        begin_reenrollment(
            &fixture,
            &request,
            &qr("new-inv", "ceremony-new-inv"),
            "request-new",
            121,
        )
        .await
        .unwrap(),
        Outcome::Denied
    );
    let mut db = fixture.open().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM invitations WHERE id='new-inv'")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        0
    );
    db.close().await.unwrap();
    fixture.integrity().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revocation_competing_with_confirmation_has_one_durable_denial_outcome() {
    for _ in 0..6 {
        let fixture = Fixture::new().await;
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let first_path = fixture.path.clone();
        let first_barrier = barrier.clone();
        let confirm = tokio::spawn(async move {
            let fixture = Fixture {
                path: first_path,
                owns_file: false,
            };
            first_barrier.wait().await;
            fixture.confirm("inv", "ceremony-inv").await
        });
        let second_path = fixture.path.clone();
        let second_barrier = barrier.clone();
        let revoke = tokio::spawn(async move {
            let fixture = Fixture {
                path: second_path,
                owns_file: false,
            };
            second_barrier.wait().await;
            invalidate_pending(&fixture, "inv", Invalidation::RevokeDevice, 116).await
        });
        let _ = confirm.await.unwrap();
        let _ = revoke.await.unwrap();
        let mut db = fixture.open().await;
        let invite: String = sqlx::query_scalar("SELECT state FROM invitations WHERE id='inv'")
            .fetch_one(&mut db)
            .await
            .unwrap();
        let credential: Option<String> = sqlx::query_scalar(
            "SELECT state FROM p023_credential_reference WHERE invitation='inv'",
        )
        .fetch_optional(&mut db)
        .await
        .unwrap();
        assert!(matches!(invite.as_str(), "CANCELLED" | "CONSUMED"));
        assert!(credential.is_none() || credential.as_deref() == Some("REVOKED"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT active FROM device_authority WHERE device='device'"
            )
            .fetch_one(&mut db)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM p023_recovery_records WHERE invitation='inv'"
            )
            .fetch_one(&mut db)
            .await
            .unwrap(),
            "DEVICE_REVOKED"
        );
        db.close().await.unwrap();
        fixture.integrity().await;
    }
}

#[test]
fn crash_worker() {
    let Some(path) = std::env::var_os("SYNVEIL_P023_CHILD_DB") else {
        return;
    };
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let file = PathBuf::from(path);
        let mut db = support::connect(&file).await.unwrap();
        let invitation: String =
            sqlx::query_scalar("SELECT binding FROM invitations WHERE id='inv'")
                .fetch_one(&mut db)
                .await
                .unwrap();
        let qr: PairingQrPayload = serde_json::from_str(&invitation).unwrap();
        let request = prepared_pairing(&qr, "ceremony-inv").1;
        let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert_eq!(
            confirm_locked(&mut tx, &request, 116).await.unwrap(),
            Ok(())
        );
        sqlx::query("PRAGMA cache_size=1")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE p023_crash_pressure (padding BLOB)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO p023_crash_pressure VALUES (zeroblob(1048576))")
            .execute(&mut *tx)
            .await
            .unwrap();
        let journal = PathBuf::from(format!("{}-journal", file.display()));
        assert!(std::fs::metadata(journal).unwrap().len() > 512);
        std::process::exit(73);
    });
}

#[tokio::test]
async fn process_crash_before_commit_rolls_back_confirmation_and_allows_metadata_recovery() {
    let fixture = Fixture::new().await;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_worker", "--nocapture"])
        .env("SYNVEIL_P023_CHILD_DB", &fixture.path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73));
    let mut db = fixture.open().await;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM invitations WHERE id='inv'")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        "ISSUED"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM synthetic_verifiers")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        ceremony_snapshot(&mut db, "ceremony-inv")
            .await
            .unwrap()
            .response()
            .state,
        CeremonyState::ReadyToConfirm
    );
    db.close().await.unwrap();
    assert_eq!(fixture.confirm("inv", "ceremony-inv").await, Ok(()));
    fixture.integrity().await;
}
