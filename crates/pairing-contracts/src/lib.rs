//! Non-production executable contracts for proposed device pairing.
//!
//! This crate is deliberately not depended on by the API, auth, metadata, or
//! client crates. `InvitationLedger` is an in-memory state-machine oracle for
//! deterministic tests; it is not persistence, a credential issuer, or a
//! production redemption implementation.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use url::Url;

pub mod ceremony;

pub const INVITATION_CONTRACT_VERSION: u16 = 1;
pub const ENROLLMENT_AUDIENCE: &str = "synveil-device-enrollment";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingQrPayload {
    pub contract_version: u16,
    pub invitation_id: String,
    pub installation_id: String,
    pub issuer_owner_id: String,
    pub audience: String,
    pub approved_endpoint: String,
    pub server_identity_fingerprint: String,
    pub target_device_id: String,
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub policy_revision: u64,
}

impl PairingQrPayload {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.contract_version != INVITATION_CONTRACT_VERSION
            || self.audience != ENROLLMENT_AUDIENCE
            || self.issued_at_unix < 0
            || self.expires_at_unix <= self.issued_at_unix
            || self.policy_revision == 0
            || [
                &self.invitation_id,
                &self.installation_id,
                &self.issuer_owner_id,
                &self.target_device_id,
            ]
            .into_iter()
            .any(|value| value.is_empty() || value.len() > 128)
        {
            return Err(ContractError::MalformedInvitation);
        }

        let endpoint =
            Url::parse(&self.approved_endpoint).map_err(|_| ContractError::MalformedInvitation)?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !is_sha256_fingerprint(&self.server_identity_fingerprint)
        {
            return Err(ContractError::MalformedInvitation);
        }
        Ok(())
    }
}

fn is_sha256_fingerprint(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvitationState {
    Created,
    Issued,
    Consumed,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvitationRecord {
    pub qr: PairingQrPayload,
    pub state: InvitationState,
}

impl InvitationRecord {
    pub fn create(qr: PairingQrPayload) -> Result<Self, ContractError> {
        qr.validate()?;
        Ok(Self {
            qr,
            state: InvitationState::Created,
        })
    }

    pub fn issue(&mut self, now_unix: i64) -> Result<(), ContractError> {
        if self.state != InvitationState::Created {
            return Err(ContractError::InvalidTransition);
        }
        if now_unix < self.qr.issued_at_unix {
            return Err(ContractError::NotYetValid);
        }
        if now_unix >= self.qr.expires_at_unix {
            self.state = InvitationState::Expired;
            return Err(ContractError::Expired);
        }
        self.state = InvitationState::Issued;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), ContractError> {
        if !matches!(
            self.state,
            InvitationState::Created | InvitationState::Issued
        ) {
            return Err(ContractError::InvalidTransition);
        }
        self.state = InvitationState::Cancelled;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedemptionEvidence {
    pub now_unix: i64,
    pub observed_installation_id: String,
    pub observed_owner_id: String,
    pub observed_audience: String,
    pub observed_https_endpoint: String,
    pub observed_server_fingerprint: String,
    pub candidate_device_id: String,
    pub observed_policy_revision: u64,
    pub issuer_is_active: bool,
    /// Test oracle for a separately authenticated one-time claim proof. It is
    /// not a client-supplied QR assertion or a selected wire mechanism.
    pub invitation_proof_verified: bool,
    pub tls_authenticated_https: bool,
    pub certificate_and_hostname_valid: bool,
    pub user_confirmed_displayed_identity: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectionReason {
    IssuerRevoked,
    InvitationNotYetValid,
    MissingInvitationProof,
    WrongInstallation,
    WrongIssuer,
    WrongAudience,
    WrongEndpoint,
    WrongServerIdentity,
    UnauthenticatedTransport,
    InvalidCertificate,
    DeviceMismatch,
    StalePolicy,
    UserDidNotConfirm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedemptionResult {
    Consumed { credential_id: String },
    Rejected(RejectionReason),
    Expired,
    Cancelled,
    AlreadyConsumed,
    NotIssued,
}

/// One-shot state-machine reference. A successful transaction's caller gets
/// only its generated credential identifier here; no secret is represented.
#[derive(Default)]
pub struct InvitationLedger {
    records: Mutex<Vec<InvitationRecord>>,
}

impl InvitationLedger {
    pub fn insert(&self, record: InvitationRecord) -> Result<(), ContractError> {
        let mut records = self.records.lock().map_err(|_| ContractError::Poisoned)?;
        if records
            .iter()
            .any(|stored| stored.qr.invitation_id == record.qr.invitation_id)
        {
            return Err(ContractError::DuplicateInvitation);
        }
        records.push(record);
        Ok(())
    }

    /// Serializes the modeled commit point. A real adapter must perform the
    /// same compare-and-consume + credential insert in one durable transaction
    /// and recheck owner/policy at that commit point.
    pub fn redeem(&self, invitation_id: &str, evidence: &RedemptionEvidence) -> RedemptionResult {
        let Ok(mut records) = self.records.lock() else {
            return RedemptionResult::NotIssued;
        };
        let Some(record) = records
            .iter_mut()
            .find(|record| record.qr.invitation_id == invitation_id)
        else {
            return RedemptionResult::NotIssued;
        };
        match record.state {
            InvitationState::Created => return RedemptionResult::NotIssued,
            InvitationState::Cancelled => return RedemptionResult::Cancelled,
            InvitationState::Expired => return RedemptionResult::Expired,
            InvitationState::Consumed => return RedemptionResult::AlreadyConsumed,
            InvitationState::Issued => {}
        }
        if evidence.now_unix < record.qr.issued_at_unix {
            return RedemptionResult::Rejected(RejectionReason::InvitationNotYetValid);
        }
        if evidence.now_unix >= record.qr.expires_at_unix {
            record.state = InvitationState::Expired;
            return RedemptionResult::Expired;
        }
        if let Some(reason) = rejection(&record.qr, evidence) {
            return RedemptionResult::Rejected(reason);
        }
        record.state = InvitationState::Consumed;
        RedemptionResult::Consumed {
            credential_id: format!("credential-for-{}", record.qr.invitation_id),
        }
    }

    pub fn state(&self, invitation_id: &str) -> Option<InvitationState> {
        self.records
            .lock()
            .ok()?
            .iter()
            .find(|record| record.qr.invitation_id == invitation_id)
            .map(|record| record.state)
    }
}

fn rejection(qr: &PairingQrPayload, evidence: &RedemptionEvidence) -> Option<RejectionReason> {
    if !evidence.issuer_is_active {
        return Some(RejectionReason::IssuerRevoked);
    }
    if !evidence.invitation_proof_verified {
        return Some(RejectionReason::MissingInvitationProof);
    }
    if evidence.observed_installation_id != qr.installation_id {
        return Some(RejectionReason::WrongInstallation);
    }
    if evidence.observed_owner_id != qr.issuer_owner_id {
        return Some(RejectionReason::WrongIssuer);
    }
    if evidence.observed_audience != qr.audience {
        return Some(RejectionReason::WrongAudience);
    }
    if evidence.observed_https_endpoint != qr.approved_endpoint {
        return Some(RejectionReason::WrongEndpoint);
    }
    if evidence.observed_server_fingerprint != qr.server_identity_fingerprint {
        return Some(RejectionReason::WrongServerIdentity);
    }
    if !evidence.tls_authenticated_https {
        return Some(RejectionReason::UnauthenticatedTransport);
    }
    if !evidence.certificate_and_hostname_valid {
        return Some(RejectionReason::InvalidCertificate);
    }
    if evidence.candidate_device_id != qr.target_device_id {
        return Some(RejectionReason::DeviceMismatch);
    }
    if evidence.observed_policy_revision != qr.policy_revision {
        return Some(RejectionReason::StalePolicy);
    }
    if !evidence.user_confirmed_displayed_identity {
        return Some(RejectionReason::UserDidNotConfirm);
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractError {
    MalformedInvitation,
    InvalidTransition,
    Expired,
    NotYetValid,
    DuplicateInvitation,
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn qr() -> PairingQrPayload {
        PairingQrPayload {
            contract_version: INVITATION_CONTRACT_VERSION,
            invitation_id: "inv-1".into(),
            installation_id: "install-1".into(),
            issuer_owner_id: "owner-1".into(),
            audience: ENROLLMENT_AUDIENCE.into(),
            approved_endpoint: "https://sync.example.test".into(),
            server_identity_fingerprint: format!("sha256:{}", "a".repeat(64)),
            target_device_id: "device-1".into(),
            issued_at_unix: 100,
            expires_at_unix: 200,
            policy_revision: 7,
        }
    }

    fn evidence() -> RedemptionEvidence {
        RedemptionEvidence {
            now_unix: 150,
            observed_installation_id: "install-1".into(),
            observed_owner_id: "owner-1".into(),
            observed_audience: ENROLLMENT_AUDIENCE.into(),
            observed_https_endpoint: "https://sync.example.test".into(),
            observed_server_fingerprint: format!("sha256:{}", "a".repeat(64)),
            candidate_device_id: "device-1".into(),
            observed_policy_revision: 7,
            issuer_is_active: true,
            invitation_proof_verified: true,
            tls_authenticated_https: true,
            certificate_and_hostname_valid: true,
            user_confirmed_displayed_identity: true,
        }
    }

    fn issued() -> InvitationRecord {
        let mut record = InvitationRecord::create(qr()).unwrap();
        record.issue(101).unwrap();
        record
    }

    fn ledger() -> InvitationLedger {
        let ledger = InvitationLedger::default();
        ledger.insert(issued()).unwrap();
        ledger
    }

    #[test]
    fn qr_is_versioned_bound_and_contains_no_bearer_or_command_field() {
        let value = serde_json::to_value(qr()).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 11);
        for forbidden in [
            "token",
            "enrollment_token",
            "device_credential",
            "admin_token",
            "command",
            "script",
        ] {
            assert!(!object.contains_key(forbidden));
        }
        assert!(
            serde_json::from_value::<PairingQrPayload>(serde_json::json!({
                "contract_version": 1,
                "invitation_id": "inv-1",
                "installation_id": "install-1",
                "issuer_owner_id": "owner-1",
                "audience": "synveil-device-enrollment",
                "approved_endpoint": "https://sync.example.test",
                "server_identity_fingerprint": format!("sha256:{}", "a".repeat(64)),
                "target_device_id": "device-1",
                "issued_at_unix": 100,
                "expires_at_unix": 200,
                "policy_revision": 7,
                "enrollment_token": "sve1_secret"
            }))
            .is_err()
        );
    }

    #[test]
    fn rejects_forged_or_unsafe_qr_endpoints_and_fingerprint() {
        let mut value = qr();
        value.approved_endpoint = "http://sync.example.test".into();
        assert_eq!(value.validate(), Err(ContractError::MalformedInvitation));
        value.approved_endpoint = "https://user:pass@sync.example.test".into();
        assert_eq!(value.validate(), Err(ContractError::MalformedInvitation));
        value.approved_endpoint = "https://sync.example.test".into();
        value.server_identity_fingerprint = "sha256:abc".into();
        assert_eq!(value.validate(), Err(ContractError::MalformedInvitation));
        value.server_identity_fingerprint = format!("sha256:{}", "A".repeat(64));
        assert_eq!(value.validate(), Err(ContractError::MalformedInvitation));
    }

    #[test]
    fn rejected_contexts_do_not_consume_and_exact_context_succeeds_once() {
        let ledger = ledger();
        let mut wrong = evidence();
        wrong.observed_installation_id = "other-install".into();
        assert_eq!(
            ledger.redeem("inv-1", &wrong),
            RedemptionResult::Rejected(RejectionReason::WrongInstallation)
        );
        assert_eq!(ledger.state("inv-1"), Some(InvitationState::Issued));
        assert!(matches!(
            ledger.redeem("inv-1", &evidence()),
            RedemptionResult::Consumed { .. }
        ));
        assert_eq!(
            ledger.redeem("inv-1", &evidence()),
            RedemptionResult::AlreadyConsumed
        );
    }

    #[test]
    fn wrong_issuer_tls_identity_device_revocation_and_stale_policy_are_rejected() {
        for (change, expected) in [
            (0, RejectionReason::WrongIssuer),
            (1, RejectionReason::WrongServerIdentity),
            (2, RejectionReason::InvalidCertificate),
            (3, RejectionReason::DeviceMismatch),
            (4, RejectionReason::IssuerRevoked),
            (5, RejectionReason::StalePolicy),
            (6, RejectionReason::UnauthenticatedTransport),
            (7, RejectionReason::WrongEndpoint),
            (8, RejectionReason::UserDidNotConfirm),
        ] {
            let ledger = ledger();
            let mut proof = evidence();
            match change {
                0 => proof.observed_owner_id = "other-owner".into(),
                1 => proof.observed_server_fingerprint = format!("sha256:{}", "b".repeat(64)),
                2 => proof.certificate_and_hostname_valid = false,
                3 => proof.candidate_device_id = "other-device".into(),
                4 => proof.issuer_is_active = false,
                5 => proof.observed_policy_revision += 1,
                6 => proof.tls_authenticated_https = false,
                7 => proof.observed_https_endpoint = "https://substitute.example.test".into(),
                _ => proof.user_confirmed_displayed_identity = false,
            }
            assert_eq!(
                ledger.redeem("inv-1", &proof),
                RedemptionResult::Rejected(expected)
            );
            assert_eq!(ledger.state("inv-1"), Some(InvitationState::Issued));
        }
    }

    #[test]
    fn expiry_cancellation_and_duplicate_scan_are_terminal_or_non_authorizing() {
        let mut premature = InvitationRecord::create(qr()).unwrap();
        assert_eq!(premature.issue(99), Err(ContractError::NotYetValid));
        assert_eq!(premature.state, InvitationState::Created);

        let ledger = ledger();
        let mut early = evidence();
        early.now_unix = 99;
        assert_eq!(
            ledger.redeem("inv-1", &early),
            RedemptionResult::Rejected(RejectionReason::InvitationNotYetValid)
        );
        assert_eq!(ledger.state("inv-1"), Some(InvitationState::Issued));
        let mut late = evidence();
        late.now_unix = 200;
        assert_eq!(ledger.redeem("inv-1", &late), RedemptionResult::Expired);
        assert_eq!(
            ledger.redeem("inv-1", &evidence()),
            RedemptionResult::Expired
        );

        let mut cancelled = issued();
        cancelled.qr.invitation_id = "inv-cancelled".into();
        cancelled.cancel().unwrap();
        let cancel_ledger = InvitationLedger::default();
        cancel_ledger.insert(cancelled.clone()).unwrap();
        assert_eq!(
            cancel_ledger.redeem("inv-cancelled", &evidence()),
            RedemptionResult::Cancelled
        );
        assert_eq!(
            cancel_ledger.redeem("missing", &evidence()),
            RedemptionResult::NotIssued
        );
        assert_eq!(cancelled.state, InvitationState::Cancelled);
        assert_eq!(cancelled.cancel(), Err(ContractError::InvalidTransition));
    }

    #[test]
    fn invitation_identifier_alone_is_not_redemption_authority() {
        let ledger = ledger();
        let mut proof = evidence();
        proof.invitation_proof_verified = false;
        assert_eq!(
            ledger.redeem("inv-1", &proof),
            RedemptionResult::Rejected(RejectionReason::MissingInvitationProof)
        );
        assert_eq!(ledger.state("inv-1"), Some(InvitationState::Issued));
    }

    #[test]
    fn concurrent_redemption_has_at_most_one_success() {
        let ledger = Arc::new(ledger());
        let barrier = Arc::new(Barrier::new(16));
        let mut threads = Vec::new();
        for _ in 0..16 {
            let ledger = Arc::clone(&ledger);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                ledger.redeem("inv-1", &evidence())
            }));
        }
        let successes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(|result| matches!(result, RedemptionResult::Consumed { .. }))
            .count();
        assert_eq!(successes, 1);
        assert_eq!(ledger.state("inv-1"), Some(InvitationState::Consumed));
    }
}
