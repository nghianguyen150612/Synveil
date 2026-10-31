//! P022 NON-PRODUCTION first-contact oracle. No TLS, signatures, enrollment,
//! credentials or production authority are implemented here. `ReferenceFacts`
//! MUST come from a trusted test harness, never from a decoded client request.

use serde::{Deserialize, Serialize};

use crate::PairingQrPayload;

pub const CEREMONY_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRequest {
    pub version: u16,
    pub ceremony_id: String,
    /// Untrusted claim, including the fingerprint. Possession grants no trust.
    pub qr_claim: PairingQrPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeBinding {
    pub version: u16,
    pub ceremony_id: String,
    pub invitation: PairingQrPayload,
    /// Synthetic unique fixture locator, NOT a cryptographic nonce/proof.
    pub challenge_id: String,
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
}

/// Approval, consent, confirmation, cancellation and recovery all refer to the
/// entire immutable challenge. This request itself is never evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundRequest {
    pub version: u16,
    pub binding: ChallengeBinding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeremonyState {
    Claimed,
    IdentityAuthenticated,
    AwaitingOwnerApproval,
    AwaitingClientConsent,
    ReadyToConfirm,
    Confirmed,
    Cancelled,
    Expired,
    Revoked,
}

impl CeremonyState {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Confirmed | Self::Cancelled | Self::Expired | Self::Revoked
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CeremonyResponse {
    pub version: u16,
    pub ceremony_id: String,
    pub state: CeremonyState,
    pub challenge: Option<ChallengeBinding>,
}

impl CeremonyResponse {
    /// Structural validation only; a received response never establishes trust.
    pub fn validate(&self) -> Result<(), CeremonyError> {
        if self.version != CEREMONY_VERSION || !locator(&self.ceremony_id) {
            return Err(CeremonyError::Malformed);
        }
        let requires_challenge = matches!(
            self.state,
            CeremonyState::AwaitingOwnerApproval
                | CeremonyState::AwaitingClientConsent
                | CeremonyState::ReadyToConfirm
                | CeremonyState::Confirmed
                | CeremonyState::Cancelled
        );
        if requires_challenge && self.challenge.is_none() {
            return Err(CeremonyError::Malformed);
        }
        if let Some(c) = &self.challenge
            && (matches!(
                self.state,
                CeremonyState::Claimed | CeremonyState::IdentityAuthenticated
            ) || c.version != CEREMONY_VERSION
                || c.ceremony_id != self.ceremony_id
                || !locator(&c.challenge_id)
                || c.issued_at_unix < c.invitation.issued_at_unix
                || c.expires_at_unix <= c.issued_at_unix
                || c.expires_at_unix > c.invitation.expires_at_unix
                || c.invitation.validate().is_err())
        {
            return Err(CeremonyError::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CeremonyError {
    Malformed,
    WrongBinding,
    InvalidTransition,
    ClockRegression,
    Expired,
    Revoked,
    DeviceRevoked,
    MissingIndependentIdentity,
    IdentityMismatch,
    InvalidTls,
    MissingClaimProof,
    MissingOwnerApproval,
    MissingClientConsent,
    InvalidDecision,
}

/// Harness observation, deliberately NOT deserializable as a client assertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceTls {
    CertificateAndHostnameAuthenticated,
    CertificateOrHostnameMismatch,
    Unauthenticated,
    Redirected,
}

/// Provisioned independently of the scanned QR. D12 must approve how a real
/// implementation obtains and authenticates this installation identity.
#[derive(Clone, Debug)]
pub struct ReferenceServerIdentity {
    pub installation_id: String,
    pub endpoint: String,
    pub fingerprint: String,
    pub tls: ReferenceTls,
}

/// Separate authenticated owner action / explicit client UI event, simulated
/// by trusted fixtures. There is no constructor from a wire approval boolean.
#[derive(Clone, Debug)]
pub struct ReferenceDecision {
    pub evidence_id: String,
    pub principal_id: String,
    pub binding: ChallengeBinding,
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub revoked: bool,
}

/// A transaction-local snapshot of authoritative fixture state. Not a request
/// or response DTO. Fresh facts must be read under the same fence as consume.
pub struct ReferenceFacts {
    pub authoritative_invitation: PairingQrPayload,
    pub issuer_active: bool,
    pub target_device_active: bool,
    pub independent_identity: Option<ReferenceServerIdentity>,
    pub invitation_claim_verified: bool,
    pub owner_approval: Option<ReferenceDecision>,
    pub client_consent: Option<ReferenceDecision>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionReceipt {
    evidence_id: String,
    principal_id: String,
    issued_at_unix: i64,
    expires_at_unix: i64,
}

impl From<&ReferenceDecision> for DecisionReceipt {
    fn from(value: &ReferenceDecision) -> Self {
        Self {
            evidence_id: value.evidence_id.clone(),
            principal_id: value.principal_id.clone(),
            issued_at_unix: value.issued_at_unix,
            expires_at_unix: value.expires_at_unix,
        }
    }
}

/// Serializable solely for trusted P021/P022 test-database snapshots. NEVER
/// accept a serialized instance from a client. SQL serialization and consume
/// atomicity are responsibilities of the integration-test adapter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCeremony {
    claim: ClaimRequest,
    state: CeremonyState,
    challenge: Option<ChallengeBinding>,
    approval: Option<DecisionReceipt>,
    consent: Option<DecisionReceipt>,
    last_observed_unix: i64,
}

fn locator(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

impl ReferenceCeremony {
    pub fn claim(request: ClaimRequest) -> Result<Self, CeremonyError> {
        if request.version != CEREMONY_VERSION || !locator(&request.ceremony_id) {
            return Err(CeremonyError::Malformed);
        }
        request
            .qr_claim
            .validate()
            .map_err(|_| CeremonyError::Malformed)?;
        Ok(Self {
            claim: request,
            state: CeremonyState::Claimed,
            challenge: None,
            approval: None,
            consent: None,
            last_observed_unix: 0,
        })
    }

    pub fn response(&self) -> CeremonyResponse {
        CeremonyResponse {
            version: CEREMONY_VERSION,
            ceremony_id: self.claim.ceremony_id.clone(),
            state: self.state,
            challenge: self.challenge.clone(),
        }
    }

    pub fn authenticate_identity(
        &mut self,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<(), CeremonyError> {
        self.require_state(CeremonyState::Claimed)?;
        self.check_facts(facts, now)?;
        self.state = CeremonyState::IdentityAuthenticated;
        Ok(())
    }

    /// Caller supplies deterministic fixture IDs and times. Production entropy,
    /// transcript encoding, lifetime and proof mechanisms remain unselected.
    pub fn challenge(
        &mut self,
        challenge_id: String,
        expires_at_unix: i64,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<BoundRequest, CeremonyError> {
        self.require_state(CeremonyState::IdentityAuthenticated)?;
        self.check_facts(facts, now)?;
        if !locator(&challenge_id)
            || expires_at_unix <= now
            || expires_at_unix > self.claim.qr_claim.expires_at_unix
        {
            return Err(CeremonyError::Malformed);
        }
        let binding = ChallengeBinding {
            version: CEREMONY_VERSION,
            ceremony_id: self.claim.ceremony_id.clone(),
            invitation: self.claim.qr_claim.clone(),
            challenge_id,
            issued_at_unix: now,
            expires_at_unix,
        };
        self.challenge = Some(binding.clone());
        self.state = CeremonyState::AwaitingOwnerApproval;
        Ok(BoundRequest {
            version: CEREMONY_VERSION,
            binding,
        })
    }

    pub fn approve(
        &mut self,
        request: &BoundRequest,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<(), CeremonyError> {
        self.require_state(CeremonyState::AwaitingOwnerApproval)?;
        self.check_request(request)?;
        self.check_facts(facts, now)?;
        self.approval = Some(self.owner_decision(facts, now)?);
        self.state = CeremonyState::AwaitingClientConsent;
        Ok(())
    }

    pub fn consent(
        &mut self,
        request: &BoundRequest,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<(), CeremonyError> {
        self.require_state(CeremonyState::AwaitingClientConsent)?;
        self.check_request(request)?;
        self.check_facts(facts, now)?;
        self.recheck_approval(facts, now)?;
        let consent = self.client_decision(facts, now)?;
        if self.approval.as_ref().is_none_or(|approval| {
            consent.issued_at_unix < approval.issued_at_unix
                || consent.evidence_id == approval.evidence_id
        }) {
            return Err(CeremonyError::InvalidDecision);
        }
        self.consent = Some(consent);
        self.state = CeremonyState::ReadyToConfirm;
        Ok(())
    }

    /// Only a synthetic ceremony outcome. An adapter must atomically persist
    /// this transition with the P021 compare-and-consume; no bearer is returned.
    pub fn confirm(
        &mut self,
        request: &BoundRequest,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<(), CeremonyError> {
        self.require_state(CeremonyState::ReadyToConfirm)?;
        self.check_request(request)?;
        self.check_facts(facts, now)?;
        self.recheck_approval(facts, now)?;
        if self.consent.as_ref() != Some(&self.client_decision(facts, now)?) {
            return Err(CeremonyError::InvalidDecision);
        }
        self.state = CeremonyState::Confirmed;
        Ok(())
    }

    /// Trusted harness cancellation, modeling an authenticated participant's
    /// withdrawal. No issuer-active check: revoked participants can still abort.
    pub fn cancel(&mut self, request: &BoundRequest) -> Result<(), CeremonyError> {
        self.check_request(request)?;
        self.abort(CeremonyState::Cancelled)
    }

    /// Also covers withdrawal before a challenge exists. Fixture control only.
    pub fn revoke(&mut self) -> Result<(), CeremonyError> {
        self.abort(CeremonyState::Revoked)
    }

    /// Metadata-only lost-response recovery; NEVER re-confirms or returns a
    /// credential. Caller authentication is modeled by fresh trusted facts.
    pub fn recover(
        &self,
        request: &BoundRequest,
        facts: &ReferenceFacts,
    ) -> Result<CeremonyResponse, CeremonyError> {
        self.check_request(request)?;
        self.check_identity(facts)?;
        Ok(self.response())
    }

    fn abort(&mut self, state: CeremonyState) -> Result<(), CeremonyError> {
        if self.state.terminal() {
            return Err(CeremonyError::InvalidTransition);
        }
        self.state = state;
        Ok(())
    }

    fn require_state(&self, state: CeremonyState) -> Result<(), CeremonyError> {
        if self.state != state {
            return Err(CeremonyError::InvalidTransition);
        }
        Ok(())
    }

    fn check_request(&self, request: &BoundRequest) -> Result<(), CeremonyError> {
        if request.version != CEREMONY_VERSION || self.challenge.as_ref() != Some(&request.binding)
        {
            return Err(CeremonyError::WrongBinding);
        }
        Ok(())
    }

    fn check_identity(&self, facts: &ReferenceFacts) -> Result<(), CeremonyError> {
        let qr = &self.claim.qr_claim;
        if !facts.issuer_active {
            return Err(CeremonyError::Revoked);
        }
        if !facts.target_device_active {
            return Err(CeremonyError::DeviceRevoked);
        }
        if &facts.authoritative_invitation != qr {
            return Err(CeremonyError::WrongBinding);
        }
        let identity = facts
            .independent_identity
            .as_ref()
            .ok_or(CeremonyError::MissingIndependentIdentity)?;
        if identity.tls != ReferenceTls::CertificateAndHostnameAuthenticated {
            return Err(CeremonyError::InvalidTls);
        }
        if identity.installation_id != qr.installation_id
            || identity.endpoint != qr.approved_endpoint
            || identity.fingerprint != qr.server_identity_fingerprint
        {
            return Err(CeremonyError::IdentityMismatch);
        }
        if !facts.invitation_claim_verified {
            return Err(CeremonyError::MissingClaimProof);
        }
        Ok(())
    }

    fn check_facts(&mut self, facts: &ReferenceFacts, now: i64) -> Result<(), CeremonyError> {
        if now < self.last_observed_unix || now < self.claim.qr_claim.issued_at_unix {
            return Err(CeremonyError::ClockRegression);
        }
        self.last_observed_unix = now;
        if now >= self.claim.qr_claim.expires_at_unix
            || self
                .challenge
                .as_ref()
                .is_some_and(|challenge| now >= challenge.expires_at_unix)
        {
            self.state = CeremonyState::Expired;
            return Err(CeremonyError::Expired);
        }
        if !facts.issuer_active || !facts.target_device_active {
            self.state = CeremonyState::Revoked;
        }
        self.check_identity(facts)?;
        if (self.approval.is_some() && facts.owner_approval.as_ref().is_some_and(|e| e.revoked))
            || (self.consent.is_some() && facts.client_consent.as_ref().is_some_and(|e| e.revoked))
        {
            self.state = CeremonyState::Revoked;
            return Err(CeremonyError::Revoked);
        }
        Ok(())
    }

    fn decision(
        &self,
        evidence: &ReferenceDecision,
        principal: &str,
        now: i64,
    ) -> Result<DecisionReceipt, CeremonyError> {
        let challenge = self
            .challenge
            .as_ref()
            .ok_or(CeremonyError::InvalidTransition)?;
        if evidence.revoked
            || !locator(&evidence.evidence_id)
            || evidence.principal_id != principal
            || &evidence.binding != challenge
            || evidence.issued_at_unix < challenge.issued_at_unix
            || evidence.issued_at_unix > now
            || evidence.expires_at_unix <= evidence.issued_at_unix
            || evidence.expires_at_unix > challenge.expires_at_unix
            || now >= evidence.expires_at_unix
        {
            return Err(CeremonyError::InvalidDecision);
        }
        Ok(evidence.into())
    }

    fn owner_decision(
        &self,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<DecisionReceipt, CeremonyError> {
        self.decision(
            facts
                .owner_approval
                .as_ref()
                .ok_or(CeremonyError::MissingOwnerApproval)?,
            &self.claim.qr_claim.issuer_owner_id,
            now,
        )
    }

    fn client_decision(
        &self,
        facts: &ReferenceFacts,
        now: i64,
    ) -> Result<DecisionReceipt, CeremonyError> {
        self.decision(
            facts
                .client_consent
                .as_ref()
                .ok_or(CeremonyError::MissingClientConsent)?,
            &self.claim.qr_claim.target_device_id,
            now,
        )
    }

    fn recheck_approval(&self, facts: &ReferenceFacts, now: i64) -> Result<(), CeremonyError> {
        if self.approval.as_ref() != Some(&self.owner_decision(facts, now)?) {
            return Err(CeremonyError::InvalidDecision);
        }
        Ok(())
    }
}
