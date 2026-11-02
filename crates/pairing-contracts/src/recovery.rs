//! P023 non-production recovery and revocation reference contracts.
//!
//! This module defines bounded reference states and deterministic qualification
//! rules only. It is not linked into production authorization, creates no bearer
//! credential, and does not select the pending P004-D12/D13 mechanisms.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const RECOVERY_CONTRACT_VERSION: u16 = 1;

/// Versioned state recorded by a trusted qualification harness.
///
/// A new enrollment always receives a new record and fresh invitation/ceremony
/// identifiers. Terminal states from an earlier invitation or credential are
/// never changed back to pending or active.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStateV1 {
    PairingPending,
    PairingCancellationPending,
    PairingCancelled,
    InvitationRevoked,
    OwnerRevoked,
    DeviceRevoked,
    Enrolled,
    CredentialExpired,
    SecureStorageLost,
    SecureStorageInaccessible,
    AmbiguousCredentialIssuance,
    ExplicitReenrollmentRequired,
    ReenrollmentStarted,
}

impl RecoveryStateV1 {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::PairingCancelled
                | Self::InvitationRevoked
                | Self::OwnerRevoked
                | Self::DeviceRevoked
                | Self::CredentialExpired
                | Self::SecureStorageLost
                | Self::SecureStorageInaccessible
                | Self::AmbiguousCredentialIssuance
                | Self::ExplicitReenrollmentRequired
        )
    }

    const fn permits_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::PairingPending, Self::PairingCancellationPending)
                | (Self::PairingPending, Self::Enrolled)
                | (Self::PairingPending, Self::InvitationRevoked)
                | (Self::PairingPending, Self::OwnerRevoked)
                | (Self::PairingPending, Self::DeviceRevoked)
                | (Self::PairingCancellationPending, Self::PairingCancelled)
                | (Self::Enrolled, Self::OwnerRevoked)
                | (Self::Enrolled, Self::DeviceRevoked)
                | (Self::Enrolled, Self::CredentialExpired)
                | (Self::Enrolled, Self::SecureStorageLost)
                | (Self::Enrolled, Self::SecureStorageInaccessible)
                | (Self::Enrolled, Self::AmbiguousCredentialIssuance)
        )
    }
}

/// Non-secret immutable scope used by trusted P021/P022/P023 fixture records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBindingV1 {
    pub installation_id: String,
    pub owner_id: String,
    pub device_id: String,
    pub invitation_id: String,
    pub ceremony_id: String,
    pub server_identity_fingerprint: String,
    pub policy_revision: u64,
}

impl RecoveryBindingV1 {
    fn validate(&self) -> Result<(), RecoveryContractError> {
        if [
            &self.installation_id,
            &self.owner_id,
            &self.device_id,
            &self.invitation_id,
            &self.ceremony_id,
        ]
        .into_iter()
        .any(|value| !locator(value))
            || self.policy_revision == 0
            || !sha256_fingerprint(&self.server_identity_fingerprint)
        {
            return Err(RecoveryContractError::Malformed);
        }
        Ok(())
    }
}

/// Trusted, versioned metadata only. Never deserialize this snapshot from a
/// client; the serialized form belongs in the isolated qualification database.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRecordV1 {
    pub contract_version: u16,
    pub recovery_id: String,
    pub binding: RecoveryBindingV1,
    pub state: RecoveryStateV1,
    pub recorded_at_unix: i64,
}

impl RecoveryRecordV1 {
    pub fn new(
        recovery_id: impl Into<String>,
        binding: RecoveryBindingV1,
        state: RecoveryStateV1,
        recorded_at_unix: i64,
    ) -> Result<Self, RecoveryContractError> {
        let record = Self {
            contract_version: RECOVERY_CONTRACT_VERSION,
            recovery_id: recovery_id.into(),
            binding,
            state,
            recorded_at_unix,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), RecoveryContractError> {
        if self.contract_version != RECOVERY_CONTRACT_VERSION
            || !locator(&self.recovery_id)
            || self.recorded_at_unix < 0
        {
            return Err(RecoveryContractError::Malformed);
        }
        self.binding.validate()
    }

    /// Transitions are monotonic. Terminal outcomes can only be retained; fresh
    /// enrollment creates a separate P020/P022-bound record.
    pub fn transition(
        &mut self,
        next: RecoveryStateV1,
        observed_at_unix: i64,
    ) -> Result<(), RecoveryContractError> {
        if observed_at_unix < self.recorded_at_unix {
            return Err(RecoveryContractError::ClockRegression);
        }
        if !self.state.permits_transition_to(next) {
            return Err(RecoveryContractError::InvalidTransition);
        }
        self.state = next;
        self.recorded_at_unix = observed_at_unix;
        Ok(())
    }
}

/// Client request contains only locators and claimed bindings. It has no
/// positive authorization, owner-approval, secure-store, TLS, or proof flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionV1 {
    Inspect,
    BeginExplicitReenrollment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRequestV1 {
    pub contract_version: u16,
    pub recovery_id: String,
    pub invitation_id: String,
    pub claimed_installation_id: String,
    pub claimed_owner_id: String,
    pub claimed_device_id: String,
    pub claimed_policy_revision: u64,
    pub action: RecoveryActionV1,
    /// An untrusted new invitation locator. It grants nothing and must match
    /// a separate, exact-bound owner action before a fresh pairing may start.
    pub requested_new_invitation_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialReferenceStateV1 {
    NotIssued,
    Active,
    Revoked,
    Expired,
    SecureStorageLost,
    SecureStorageInaccessible,
    AmbiguousIssuance,
}

/// Facts read by a trusted reference harness under its transaction fence.
/// This type deliberately has no serde implementation and must never be built
/// from client request assertions.
#[derive(Clone, Debug)]
pub struct TrustedRecoveryEvidence {
    pub record: RecoveryRecordV1,
    pub current_policy_revision: u64,
    pub current_server_identity_fingerprint: String,
    pub owner_active: bool,
    pub device_active: bool,
    pub credential_state: CredentialReferenceStateV1,
    pub observed_at_unix: i64,
    /// Test-harness evidence from a separately authenticated explicit owner
    /// action. This type is not deserializable from a client request.
    pub owner_reenrollment_approval: Option<OwnerReenrollmentApproval>,
}

/// Trusted, exact-bound reference evidence for one explicit re-enrollment.
/// It models a harness observation only; it is not an approved proof format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerReenrollmentApproval {
    pub evidence_id: String,
    pub owner_id: String,
    pub recovery_id: String,
    pub source_invitation_id: String,
    pub requested_new_invitation_id: String,
    pub device_id: String,
    pub approved_at_unix: i64,
    pub expires_at_unix: i64,
    pub revoked: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryRejection {
    Malformed,
    BindingMismatch,
    OwnerRevoked,
    DeviceRevoked,
    CredentialRevoked,
    InvitationTerminal,
    StalePolicy,
    ServerIdentityRotated,
    CredentialStillActive,
    ExplicitOwnerActionRequired,
}

impl fmt::Display for RecoveryRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "recovery request is invalid",
            Self::BindingMismatch => "recovery binding is invalid",
            Self::OwnerRevoked => "owner authorization is unavailable",
            Self::DeviceRevoked => "device authorization is unavailable",
            Self::CredentialRevoked => "credential is revoked",
            Self::InvitationTerminal => "pairing is no longer available",
            Self::StalePolicy => "pairing policy is stale",
            Self::ServerIdentityRotated => "server identity requires fresh verification",
            Self::CredentialStillActive => "credential recovery is not required",
            Self::ExplicitOwnerActionRequired => "explicit owner action is required",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDisposition {
    /// Read-only P022/P023 metadata. This does not restore authorization or a
    /// bearer value; the ceremony must still enforce its own current state.
    MetadataOnly(RecoveryStateV1),
    FreshEnrollmentRequired(RecoveryStateV1),
    /// The trusted harness may create a new P020 invitation and P022 ceremony.
    /// This result is not a production enrollment grant or credential.
    FreshPairingMayStart,
    Denied(RecoveryRejection),
}

/// Qualify an untrusted recovery request against independently loaded trusted
/// state. No credential material is accepted, returned, reconstructed or
/// logged. Fresh enrollment always gets a separate invitation and ceremony.
#[must_use]
pub fn qualify_recovery(
    request: &RecoveryRequestV1,
    evidence: &TrustedRecoveryEvidence,
) -> RecoveryDisposition {
    let binding = &evidence.record.binding;
    if request.contract_version != RECOVERY_CONTRACT_VERSION
        || evidence.record.validate().is_err()
        || !locator(&request.recovery_id)
        || !locator(&request.invitation_id)
        || !locator(&request.claimed_installation_id)
        || !locator(&request.claimed_owner_id)
        || !locator(&request.claimed_device_id)
        || evidence.observed_at_unix < 0
    {
        return RecoveryDisposition::Denied(RecoveryRejection::Malformed);
    }
    let request_action_is_well_formed = match request.action {
        RecoveryActionV1::Inspect => request.requested_new_invitation_id.is_none(),
        RecoveryActionV1::BeginExplicitReenrollment => request
            .requested_new_invitation_id
            .as_deref()
            .is_some_and(|id| locator(id) && id != request.invitation_id),
    };
    if !request_action_is_well_formed {
        return RecoveryDisposition::Denied(RecoveryRejection::Malformed);
    }
    if request.recovery_id != evidence.record.recovery_id
        || request.invitation_id != binding.invitation_id
        || request.claimed_installation_id != binding.installation_id
        || request.claimed_owner_id != binding.owner_id
        || request.claimed_device_id != binding.device_id
        || request.claimed_policy_revision != binding.policy_revision
    {
        return RecoveryDisposition::Denied(RecoveryRejection::BindingMismatch);
    }
    if evidence.current_policy_revision != binding.policy_revision {
        return RecoveryDisposition::Denied(RecoveryRejection::StalePolicy);
    }
    if evidence.current_server_identity_fingerprint != binding.server_identity_fingerprint {
        return RecoveryDisposition::Denied(RecoveryRejection::ServerIdentityRotated);
    }
    if !evidence.owner_active || evidence.record.state == RecoveryStateV1::OwnerRevoked {
        return RecoveryDisposition::Denied(RecoveryRejection::OwnerRevoked);
    }
    if !evidence.device_active || evidence.record.state == RecoveryStateV1::DeviceRevoked {
        return RecoveryDisposition::Denied(RecoveryRejection::DeviceRevoked);
    }
    if matches!(
        evidence.record.state,
        RecoveryStateV1::PairingCancelled
            | RecoveryStateV1::PairingCancellationPending
            | RecoveryStateV1::InvitationRevoked
    ) {
        return RecoveryDisposition::Denied(RecoveryRejection::InvitationTerminal);
    }
    if evidence.credential_state == CredentialReferenceStateV1::Revoked {
        return RecoveryDisposition::Denied(RecoveryRejection::CredentialRevoked);
    }

    let failed_state = match evidence.credential_state {
        CredentialReferenceStateV1::Expired => Some(RecoveryStateV1::CredentialExpired),
        CredentialReferenceStateV1::SecureStorageLost => Some(RecoveryStateV1::SecureStorageLost),
        CredentialReferenceStateV1::SecureStorageInaccessible => {
            Some(RecoveryStateV1::SecureStorageInaccessible)
        }
        CredentialReferenceStateV1::AmbiguousIssuance => {
            Some(RecoveryStateV1::AmbiguousCredentialIssuance)
        }
        CredentialReferenceStateV1::NotIssued | CredentialReferenceStateV1::Active => None,
        CredentialReferenceStateV1::Revoked => {
            return RecoveryDisposition::Denied(RecoveryRejection::CredentialRevoked);
        }
    };

    match request.action {
        RecoveryActionV1::Inspect => match failed_state {
            Some(state) => RecoveryDisposition::FreshEnrollmentRequired(state),
            None => RecoveryDisposition::MetadataOnly(evidence.record.state),
        },
        RecoveryActionV1::BeginExplicitReenrollment => {
            if !matches!(
                evidence.record.state,
                RecoveryStateV1::Enrolled
                    | RecoveryStateV1::CredentialExpired
                    | RecoveryStateV1::SecureStorageLost
                    | RecoveryStateV1::SecureStorageInaccessible
                    | RecoveryStateV1::AmbiguousCredentialIssuance
                    | RecoveryStateV1::ExplicitReenrollmentRequired
            ) {
                return RecoveryDisposition::Denied(RecoveryRejection::InvitationTerminal);
            }
            let Some(_failed_state) = failed_state else {
                return RecoveryDisposition::Denied(
                    if evidence.credential_state == CredentialReferenceStateV1::Active {
                        RecoveryRejection::CredentialStillActive
                    } else {
                        RecoveryRejection::InvitationTerminal
                    },
                );
            };
            if evidence
                .owner_reenrollment_approval
                .as_ref()
                .is_some_and(|approval| {
                    Some(approval.requested_new_invitation_id.as_str())
                        == request.requested_new_invitation_id.as_deref()
                        && approval.owner_id == binding.owner_id
                        && approval.recovery_id == evidence.record.recovery_id
                        && approval.source_invitation_id == binding.invitation_id
                        && approval.device_id == binding.device_id
                        && locator(&approval.evidence_id)
                        && approval.approved_at_unix >= 0
                        && approval.expires_at_unix > approval.approved_at_unix
                        && evidence.observed_at_unix >= approval.approved_at_unix
                        && evidence.observed_at_unix < approval.expires_at_unix
                        && !approval.revoked
                })
            {
                RecoveryDisposition::FreshPairingMayStart
            } else {
                RecoveryDisposition::Denied(RecoveryRejection::ExplicitOwnerActionRequired)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryContractError {
    Malformed,
    ClockRegression,
    InvalidTransition,
}

impl fmt::Display for RecoveryContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "recovery record is invalid",
            Self::ClockRegression => "recovery observation time is invalid",
            Self::InvalidTransition => "recovery state transition is invalid",
        })
    }
}

impl std::error::Error for RecoveryContractError {}

fn locator(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn sha256_fingerprint(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> RecoveryBindingV1 {
        RecoveryBindingV1 {
            installation_id: "install".into(),
            owner_id: "owner".into(),
            device_id: "device".into(),
            invitation_id: "inv".into(),
            ceremony_id: "ceremony".into(),
            server_identity_fingerprint: format!("sha256:{}", "a".repeat(64)),
            policy_revision: 7,
        }
    }

    fn request(action: RecoveryActionV1) -> RecoveryRequestV1 {
        RecoveryRequestV1 {
            contract_version: RECOVERY_CONTRACT_VERSION,
            recovery_id: "recovery".into(),
            invitation_id: "inv".into(),
            claimed_installation_id: "install".into(),
            claimed_owner_id: "owner".into(),
            claimed_device_id: "device".into(),
            claimed_policy_revision: 7,
            action,
            requested_new_invitation_id: match action {
                RecoveryActionV1::Inspect => None,
                RecoveryActionV1::BeginExplicitReenrollment => Some("new-inv".into()),
            },
        }
    }

    fn evidence(
        state: RecoveryStateV1,
        credential_state: CredentialReferenceStateV1,
    ) -> TrustedRecoveryEvidence {
        TrustedRecoveryEvidence {
            record: RecoveryRecordV1::new("recovery", binding(), state, 100).unwrap(),
            current_policy_revision: 7,
            current_server_identity_fingerprint: format!("sha256:{}", "a".repeat(64)),
            owner_active: true,
            device_active: true,
            credential_state,
            observed_at_unix: 120,
            owner_reenrollment_approval: None,
        }
    }

    #[test]
    fn states_are_versioned_strict_and_have_no_bearer_fields() {
        let record = RecoveryRecordV1::new(
            "recovery",
            binding(),
            RecoveryStateV1::CredentialExpired,
            110,
        )
        .unwrap();
        let mut value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["contract_version"], RECOVERY_CONTRACT_VERSION);
        value["credential"] = "sve1_never-serialize".into();
        assert!(serde_json::from_value::<RecoveryRecordV1>(value).is_err());

        let mut value = serde_json::to_value(request(RecoveryActionV1::Inspect)).unwrap();
        value["owner_authorized"] = true.into();
        assert!(serde_json::from_value::<RecoveryRequestV1>(value).is_err());
    }

    #[test]
    fn terminal_states_do_not_resurrect_and_clock_cannot_move_backwards() {
        let mut record =
            RecoveryRecordV1::new("recovery", binding(), RecoveryStateV1::PairingPending, 100)
                .unwrap();
        record
            .transition(RecoveryStateV1::PairingCancellationPending, 101)
            .unwrap();
        record
            .transition(RecoveryStateV1::PairingCancelled, 102)
            .unwrap();
        assert!(record.state.is_terminal());
        assert_eq!(
            record.transition(RecoveryStateV1::PairingPending, 103),
            Err(RecoveryContractError::InvalidTransition)
        );
        assert_eq!(
            record.transition(RecoveryStateV1::PairingPending, 99),
            Err(RecoveryContractError::ClockRegression)
        );
    }

    #[test]
    fn lost_or_unavailable_credential_requires_fresh_enrollment_without_owner_action() {
        for (credential, state) in [
            (
                CredentialReferenceStateV1::Expired,
                RecoveryStateV1::CredentialExpired,
            ),
            (
                CredentialReferenceStateV1::SecureStorageLost,
                RecoveryStateV1::SecureStorageLost,
            ),
            (
                CredentialReferenceStateV1::SecureStorageInaccessible,
                RecoveryStateV1::SecureStorageInaccessible,
            ),
            (
                CredentialReferenceStateV1::AmbiguousIssuance,
                RecoveryStateV1::AmbiguousCredentialIssuance,
            ),
        ] {
            let evidence = evidence(RecoveryStateV1::Enrolled, credential);
            assert_eq!(
                qualify_recovery(&request(RecoveryActionV1::Inspect), &evidence),
                RecoveryDisposition::FreshEnrollmentRequired(state)
            );
            assert_eq!(
                qualify_recovery(
                    &request(RecoveryActionV1::BeginExplicitReenrollment),
                    &evidence
                ),
                RecoveryDisposition::Denied(RecoveryRejection::ExplicitOwnerActionRequired)
            );
        }
    }

    #[test]
    fn a_trusted_explicit_owner_action_can_only_start_a_new_pairing() {
        let request = request(RecoveryActionV1::BeginExplicitReenrollment);
        let mut evidence = evidence(
            RecoveryStateV1::AmbiguousCredentialIssuance,
            CredentialReferenceStateV1::AmbiguousIssuance,
        );
        evidence.owner_reenrollment_approval = Some(OwnerReenrollmentApproval {
            evidence_id: "owner-action".into(),
            owner_id: "owner".into(),
            recovery_id: "recovery".into(),
            source_invitation_id: "inv".into(),
            requested_new_invitation_id: "new-inv".into(),
            device_id: "device".into(),
            approved_at_unix: 119,
            expires_at_unix: 130,
            revoked: false,
        });
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::FreshPairingMayStart
        );
        evidence.credential_state = CredentialReferenceStateV1::Active;
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::CredentialStillActive)
        );
    }

    #[test]
    fn wrong_binding_policy_identity_owner_or_device_fails_closed() {
        let request = request(RecoveryActionV1::BeginExplicitReenrollment);
        let mut evidence = evidence(
            RecoveryStateV1::Enrolled,
            CredentialReferenceStateV1::SecureStorageLost,
        );
        evidence.owner_reenrollment_approval = Some(OwnerReenrollmentApproval {
            evidence_id: "owner-action".into(),
            owner_id: "owner".into(),
            recovery_id: "recovery".into(),
            source_invitation_id: "inv".into(),
            requested_new_invitation_id: "new-inv".into(),
            device_id: "device".into(),
            approved_at_unix: 119,
            expires_at_unix: 130,
            revoked: false,
        });
        let mut wrong_request = request.clone();
        wrong_request.claimed_installation_id = "other".into();
        assert_eq!(
            qualify_recovery(&wrong_request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::BindingMismatch)
        );
        for (field, value) in [("owner", "different-owner"), ("device", "different-device")] {
            let mut wrong_request = request.clone();
            if field == "owner" {
                wrong_request.claimed_owner_id = value.into();
            } else {
                wrong_request.claimed_device_id = value.into();
            }
            assert_eq!(
                qualify_recovery(&wrong_request, &evidence),
                RecoveryDisposition::Denied(RecoveryRejection::BindingMismatch)
            );
        }
        evidence.current_policy_revision = 8;
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::StalePolicy)
        );
        evidence.current_policy_revision = 7;
        evidence.current_server_identity_fingerprint = format!("sha256:{}", "b".repeat(64));
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::ServerIdentityRotated)
        );
        evidence.current_server_identity_fingerprint = format!("sha256:{}", "a".repeat(64));
        evidence.owner_active = false;
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::OwnerRevoked)
        );
        evidence.owner_active = true;
        evidence.device_active = false;
        assert_eq!(
            qualify_recovery(&request, &evidence),
            RecoveryDisposition::Denied(RecoveryRejection::DeviceRevoked)
        );
    }

    #[test]
    fn cancelled_or_revoked_enrollment_cannot_be_reused_for_reenrollment() {
        for state in [
            RecoveryStateV1::PairingCancelled,
            RecoveryStateV1::InvitationRevoked,
            RecoveryStateV1::OwnerRevoked,
            RecoveryStateV1::DeviceRevoked,
        ] {
            let evidence = evidence(state, CredentialReferenceStateV1::SecureStorageLost);
            assert!(matches!(
                qualify_recovery(&request(RecoveryActionV1::Inspect), &evidence),
                RecoveryDisposition::Denied(_)
            ));
        }
        let evidence = evidence(
            RecoveryStateV1::Enrolled,
            CredentialReferenceStateV1::Revoked,
        );
        assert_eq!(
            qualify_recovery(
                &request(RecoveryActionV1::BeginExplicitReenrollment),
                &evidence
            ),
            RecoveryDisposition::Denied(RecoveryRejection::CredentialRevoked)
        );
    }
}
