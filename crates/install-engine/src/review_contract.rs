//! Plan-bound, in-memory installer review contracts.
//!
//! A review fingerprint detects plan changes. It is not a signature, a bearer
//! token, native authorization, or permission to execute an installation.

use crate::{
    ArtifactEvidence, ENGINE_SCHEMA_VERSION, EngineErrorCode, InstallationEffect,
    InstallationIntent, InstallationPlan, InstallationRequest, InstallerEngine, ResourceClass,
    TargetScope, VerificationContract,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const PLAN_REVIEW_SCHEMA_VERSION: u32 = 2;
const FINGERPRINT_DOMAIN: &[u8] = b"SYNVEIL-INSTALLER-PLAN-REVIEW\0";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanReviewStatus {
    NoPlanAvailable,
    PlanValidationFailed,
    ArtifactEvidenceUnavailable,
    AvailableForReview,
    ReviewDataIncomplete,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfirmationInvalidationReason {
    DeviceRoleChanged,
    ConnectionModeChanged,
    InstallationChannelChanged,
    TargetScopeChanged,
    ArtifactEvidenceChanged,
    PlannedEffectsChanged,
    PrivilegeClassificationChanged,
    PreflightFindingsChanged,
    PlatformCapabilityBecameStale,
    WizardCancelled,
    WizardRestarted,
    NewerPlanSuperseded,
    PolicyVersionChanged,
    ConfirmationExpired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewFinding {
    /// Stable, non-sensitive finding code. Do not place paths or command output here.
    pub code: String,
    pub severity: ReviewFindingSeverity,
    pub blocks_review: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewFindingSeverity {
    Information,
    Warning,
    Blocking,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanReviewContext {
    pub device_role: String,
    pub connection_mode: String,
    pub platform: String,
    pub architecture: String,
    pub channel: String,
    pub capability_generation: u64,
    /// Authenticated release-channel generation observed for this review.
    /// `None` is explicit absence and never means generation zero is trusted.
    pub manifest_generation: Option<u64>,
    /// Generation for the read-only existing-installation observation.
    pub installation_state_generation: u64,
    pub platform_policy_id: String,
    /// `None` means that no closed platform catalog is configured. It is
    /// fingerprinted so a later catalog cannot reuse an older acknowledgement.
    pub effect_catalog_version: Option<u32>,
    /// `None` means that the current security policy revision is unavailable.
    pub security_policy_version: Option<u32>,
    pub required_policy_ids: Vec<String>,
    pub preflight_findings: Vec<ReviewFinding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PlanReviewFingerprint([u8; 32]);

impl PlanReviewFingerprint {
    pub fn as_hex(&self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
        }
        output
    }

    /// A compact display hint only. The full digest is never an authorization value.
    pub fn display_summary(&self) -> String {
        format!("sha256:{}…", &self.as_hex()[..12])
    }
}

/// A validated review snapshot. It intentionally has no serialization or
/// `Debug` implementation, so evidence paths and raw plan values cannot leak
/// through ordinary UI/debug formatting.
pub struct PlanReviewSnapshot {
    schema_version: u32,
    plan_id: String,
    generation: u64,
    context: PlanReviewContext,
    fingerprint: PlanReviewFingerprint,
    status: PlanReviewStatus,
    request: InstallationRequest,
    plan: InstallationPlan,
}

impl PlanReviewSnapshot {
    /// Kept crate-private while no approved production plan source exists.
    /// A future plan builder can expose snapshots after its provenance and
    /// policy integration is approved; external callers cannot fabricate one.
    #[allow(dead_code)]
    pub(crate) fn validate_and_create(
        request: &InstallationRequest,
        plan: &InstallationPlan,
        context: PlanReviewContext,
    ) -> Result<Self, EngineErrorCode> {
        Self::validate_and_create_for_schema(PLAN_REVIEW_SCHEMA_VERSION, request, plan, context)
    }

    #[allow(dead_code)]
    pub(crate) fn validate_and_create_for_schema(
        schema_version: u32,
        request: &InstallationRequest,
        plan: &InstallationPlan,
        context: PlanReviewContext,
    ) -> Result<Self, EngineErrorCode> {
        if schema_version != PLAN_REVIEW_SCHEMA_VERSION {
            return Err(EngineErrorCode::UnsupportedSchema);
        }
        InstallerEngine.validate(request, plan)?;
        if context.device_role.is_empty()
            || context.connection_mode.is_empty()
            || context.platform.is_empty()
            || context.architecture.is_empty()
            || context.channel.is_empty()
            || context.platform_policy_id.is_empty()
            || context.required_policy_ids.iter().any(String::is_empty)
            || context
                .preflight_findings
                .iter()
                .any(|finding| finding.code.is_empty())
        {
            return Err(EngineErrorCode::InvalidPlan);
        }
        if plan.artifact.is_none() {
            return Err(EngineErrorCode::InvalidPlan);
        }
        let fingerprint = fingerprint_review(request, plan, &context)?;
        let status = if !matches!(
            plan.artifact,
            Some(ArtifactEvidence::VerifiedReleaseArtifact { .. })
        ) {
            PlanReviewStatus::ArtifactEvidenceUnavailable
        } else if context
            .preflight_findings
            .iter()
            .any(|finding| finding.blocks_review)
        {
            PlanReviewStatus::ReviewDataIncomplete
        } else {
            PlanReviewStatus::AvailableForReview
        };
        Ok(Self {
            schema_version: PLAN_REVIEW_SCHEMA_VERSION,
            plan_id: plan.plan_id.clone(),
            generation: context.capability_generation,
            context,
            fingerprint,
            status,
            request: request.clone(),
            plan: plan.clone(),
        })
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn context(&self) -> &PlanReviewContext {
        &self.context
    }

    pub(crate) fn matches_current(
        &self,
        request: &InstallationRequest,
        plan: &InstallationPlan,
        context: &PlanReviewContext,
    ) -> bool {
        self.status == PlanReviewStatus::AvailableForReview
            && self.request == *request
            && self.plan == *plan
            && self.context == *context
            && self.is_unchanged_and_valid()
            && fingerprint_review(request, plan, context)
                .is_ok_and(|fingerprint| fingerprint == self.fingerprint)
    }

    pub const fn fingerprint(&self) -> PlanReviewFingerprint {
        self.fingerprint
    }

    pub const fn status(&self) -> PlanReviewStatus {
        self.status
    }

    pub fn fingerprint_summary(&self) -> String {
        self.fingerprint.display_summary()
    }

    /// Revalidates through the existing engine and recomputes the commitment.
    pub fn is_unchanged_and_valid(&self) -> bool {
        InstallerEngine.validate(&self.request, &self.plan).is_ok()
            && fingerprint_review(&self.request, &self.plan, &self.context)
                .is_ok_and(|fingerprint| fingerprint == self.fingerprint)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfirmationState {
    Unacknowledged,
    AcknowledgedLocally,
    Invalidated(ConfirmationInvalidationReason),
    Cancelled,
}

/// Local, ephemeral acknowledgement state. It carries no execution capability.
pub struct ConfirmationAcknowledgement {
    plan_id: String,
    fingerprint: PlanReviewFingerprint,
    generation: u64,
    expires_at: u64,
}

impl ConfirmationAcknowledgement {
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    pub const fn fingerprint(&self) -> PlanReviewFingerprint {
        self.fingerprint
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

pub struct ConfirmationSession {
    generation: u64,
    plan_id: String,
    fingerprint: PlanReviewFingerprint,
    state: ConfirmationState,
    acknowledgement: Option<ConfirmationAcknowledgement>,
}

impl ConfirmationSession {
    pub fn new(snapshot: &PlanReviewSnapshot) -> Self {
        Self {
            generation: snapshot.generation,
            plan_id: snapshot.plan_id.clone(),
            fingerprint: snapshot.fingerprint,
            state: ConfirmationState::Unacknowledged,
            acknowledgement: None,
        }
    }

    pub const fn state(&self) -> ConfirmationState {
        self.state
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn acknowledgement(&mut self, now: u64) -> Option<&ConfirmationAcknowledgement> {
        if self
            .acknowledgement
            .as_ref()
            .is_some_and(|ack| now >= ack.expires_at)
        {
            self.acknowledgement = None;
            self.state =
                ConfirmationState::Invalidated(ConfirmationInvalidationReason::ConfirmationExpired);
            self.generation = self.generation.wrapping_add(1);
        }
        self.acknowledgement.as_ref()
    }

    pub(crate) fn acknowledgement_matches_snapshot(
        &mut self,
        snapshot: &PlanReviewSnapshot,
        now: u64,
    ) -> bool {
        if self.acknowledgement(now).is_none() {
            return false;
        }
        let Some(acknowledgement) = self.acknowledgement.as_ref() else {
            return false;
        };
        let acknowledgement_matches = acknowledgement.plan_id == snapshot.plan_id
            && acknowledgement.fingerprint == snapshot.fingerprint
            && acknowledgement.generation == snapshot.generation;
        self.state == ConfirmationState::AcknowledgedLocally
            && snapshot.is_unchanged_and_valid()
            && snapshot.status == PlanReviewStatus::AvailableForReview
            && acknowledgement_matches
            && self.generation == snapshot.generation
            && self.plan_id == snapshot.plan_id
            && self.fingerprint == snapshot.fingerprint
    }

    /// Records one idempotent acknowledgement for the same current plan.
    /// The result is deliberately only an acknowledgement record, never a token.
    pub fn acknowledge(
        &mut self,
        current: &PlanReviewSnapshot,
        now: u64,
        lifetime: u64,
    ) -> Result<bool, AcknowledgementError> {
        if self.state == ConfirmationState::Cancelled {
            return Err(AcknowledgementError::SessionClosed);
        }
        if lifetime == 0 {
            return Err(AcknowledgementError::InvalidLifetime);
        }
        if current.schema_version != PLAN_REVIEW_SCHEMA_VERSION || !current.is_unchanged_and_valid()
        {
            return Err(AcknowledgementError::InvalidPlan);
        }
        if current.status != PlanReviewStatus::AvailableForReview {
            return Err(AcknowledgementError::ReviewUnavailable);
        }
        if current.generation != self.generation
            || current.plan_id != self.plan_id
            || current.fingerprint != self.fingerprint
        {
            self.invalidate(ConfirmationInvalidationReason::NewerPlanSuperseded);
            return Err(AcknowledgementError::PlanChanged);
        }
        let expected_generation = self.generation;
        let expected_plan_id = self.plan_id.clone();
        let expected_fingerprint = self.fingerprint;
        let duplicate = self.acknowledgement(now).is_some_and(|existing| {
            existing.generation == expected_generation
                && existing.plan_id == expected_plan_id
                && existing.fingerprint == expected_fingerprint
        });
        if duplicate {
            return Ok(false);
        }
        if matches!(self.state, ConfirmationState::Invalidated(_)) {
            return Err(AcknowledgementError::Invalidated);
        }
        self.acknowledgement = Some(ConfirmationAcknowledgement {
            plan_id: self.plan_id.clone(),
            fingerprint: self.fingerprint,
            generation: self.generation,
            expires_at: now.saturating_add(lifetime),
        });
        self.state = ConfirmationState::AcknowledgedLocally;
        Ok(true)
    }

    /// Accepts a completion only for this exact live generation and snapshot.
    /// Old completions cannot resurrect an acknowledgement after invalidation.
    pub fn accept_async_snapshot(
        &mut self,
        source_generation: u64,
        snapshot: &PlanReviewSnapshot,
    ) -> bool {
        if source_generation != self.generation
            || self.state != ConfirmationState::Unacknowledged
            || !snapshot.is_unchanged_and_valid()
            || snapshot.status != PlanReviewStatus::AvailableForReview
        {
            return false;
        }
        if snapshot.generation != self.generation
            || snapshot.plan_id != self.plan_id
            || snapshot.fingerprint != self.fingerprint
        {
            self.invalidate(ConfirmationInvalidationReason::NewerPlanSuperseded);
            return false;
        }
        true
    }

    pub fn invalidate(&mut self, reason: ConfirmationInvalidationReason) {
        self.acknowledgement = None;
        self.state = ConfirmationState::Invalidated(reason);
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn cancel(&mut self) {
        self.acknowledgement = None;
        self.state = ConfirmationState::Cancelled;
        self.generation = self.generation.wrapping_add(1);
    }

    /// A restarted process always constructs a new unacknowledged session.
    pub fn restart(&mut self) {
        self.acknowledgement = None;
        self.state = ConfirmationState::Cancelled;
        self.generation = self.generation.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcknowledgementError {
    SessionClosed,
    InvalidLifetime,
    InvalidPlan,
    ReviewUnavailable,
    PlanChanged,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReadinessCategory {
    ReadyForReview,
    MissingVerifiedArtifact,
    UnsupportedPlatform,
    UnsupportedChannel,
    UnsupportedScope,
    BlockingPreflight,
    InvalidPlan,
    StalePlan,
    PrivilegePolicyPending,
    NativeAuthorizationUnavailable,
    PublisherPolicyPending,
    RecoveryContractUnqualified,
    ExecutionNotImplemented,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessInput {
    pub review_status: PlanReviewStatus,
    pub platform_supported: bool,
    pub channel_supported: bool,
    pub scope_supported: bool,
    pub blocking_preflight: bool,
    pub stale: bool,
    pub artifact: Option<ArtifactEvidence>,
    pub privilege_policy_approved: bool,
    pub publisher_policy_approved: bool,
    pub native_authorization_available: bool,
    pub recovery_contract_qualified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessEvaluation {
    pub review_status: PlanReviewStatus,
    pub review_category: ReadinessCategory,
    pub eligible_for_review: bool,
    /// Always includes `ExecutionNotImplemented`: acknowledgement never enables effects.
    pub execution_blockers: Vec<ReadinessCategory>,
}

pub fn evaluate_readiness(input: &ReadinessInput) -> ReadinessEvaluation {
    let mut execution_blockers = Vec::new();
    if !input.platform_supported {
        execution_blockers.push(ReadinessCategory::UnsupportedPlatform);
    }
    if !input.channel_supported {
        execution_blockers.push(ReadinessCategory::UnsupportedChannel);
    }
    if !input.scope_supported {
        execution_blockers.push(ReadinessCategory::UnsupportedScope);
    }
    if input.blocking_preflight {
        execution_blockers.push(ReadinessCategory::BlockingPreflight);
    }
    if input.stale {
        execution_blockers.push(ReadinessCategory::StalePlan);
    }
    if input.artifact.as_ref().is_none_or(|artifact| {
        !matches!(artifact, ArtifactEvidence::VerifiedReleaseArtifact { .. })
    }) {
        execution_blockers.push(ReadinessCategory::MissingVerifiedArtifact);
    }
    if !input.privilege_policy_approved {
        execution_blockers.push(ReadinessCategory::PrivilegePolicyPending);
    }
    if !input.publisher_policy_approved {
        execution_blockers.push(ReadinessCategory::PublisherPolicyPending);
    }
    if !input.native_authorization_available {
        execution_blockers.push(ReadinessCategory::NativeAuthorizationUnavailable);
    }
    if !input.recovery_contract_qualified {
        execution_blockers.push(ReadinessCategory::RecoveryContractUnqualified);
    }
    execution_blockers.push(ReadinessCategory::ExecutionNotImplemented);
    let eligible_for_review = input.review_status == PlanReviewStatus::AvailableForReview
        && !input.blocking_preflight
        && !input.stale
        && input.platform_supported
        && input.channel_supported
        && input.scope_supported
        && input.artifact.as_ref().is_some_and(|artifact| {
            matches!(artifact, ArtifactEvidence::VerifiedReleaseArtifact { .. })
        });
    let review_category = if eligible_for_review {
        ReadinessCategory::ReadyForReview
    } else if input.review_status == PlanReviewStatus::PlanValidationFailed {
        ReadinessCategory::InvalidPlan
    } else if input.stale {
        ReadinessCategory::StalePlan
    } else if input.blocking_preflight {
        ReadinessCategory::BlockingPreflight
    } else if !input.platform_supported {
        ReadinessCategory::UnsupportedPlatform
    } else if !input.channel_supported {
        ReadinessCategory::UnsupportedChannel
    } else if !input.scope_supported {
        ReadinessCategory::UnsupportedScope
    } else {
        ReadinessCategory::MissingVerifiedArtifact
    };
    ReadinessEvaluation {
        review_status: input.review_status,
        review_category,
        eligible_for_review,
        execution_blockers,
    }
}

#[derive(Serialize)]
struct CanonicalReviewDocument<'a> {
    review_schema_version: u32,
    engine_schema_version: u32,
    intent: InstallationIntent,
    plan_id: &'a str,
    target_scope: TargetScope,
    device_role: &'a str,
    connection_mode: &'a str,
    platform: &'a str,
    architecture: &'a str,
    channel: &'a str,
    capability_generation: u64,
    manifest_generation: Option<u64>,
    installation_state_generation: u64,
    platform_policy_id: &'a str,
    effect_catalog_version: Option<u32>,
    security_policy_version: Option<u32>,
    required_policy_ids: &'a [String],
    user_choices: &'a BTreeMap<String, bool>,
    artifact: CanonicalArtifact<'a>,
    ordered_effects: &'a [InstallationEffect],
    preservation_set: &'a [ResourceClass],
    expected_completion_verification: &'a VerificationContract,
    preflight_findings: Vec<CanonicalFinding<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "classification", rename_all = "SCREAMING_SNAKE_CASE")]
enum CanonicalArtifact<'a> {
    VerifiedRelease {
        evidence: CanonicalVerifiedArtifact<'a>,
    },
    LocallySupplied {
        artifact_id: &'a str,
    },
    NativeManaged {
        package_identity: &'a str,
    },
}

#[derive(Serialize)]
struct CanonicalVerifiedArtifact<'a> {
    artifact_id: &'a str,
    artifact_type: &'a str,
    product_version: &'a str,
    source_commit: &'a str,
    artifact_sha256: &'a str,
    artifact_size_bytes: u64,
    manifest_sha256: &'a str,
    authentication_method: &'a str,
}

#[derive(Serialize)]
struct CanonicalFinding<'a> {
    code: &'a str,
    severity: ReviewFindingSeverity,
    blocks_review: bool,
}

fn canonical_artifact(artifact: &ArtifactEvidence) -> CanonicalArtifact<'_> {
    match artifact {
        ArtifactEvidence::VerifiedReleaseArtifact { evidence } => {
            CanonicalArtifact::VerifiedRelease {
                evidence: CanonicalVerifiedArtifact {
                    artifact_id: &evidence.artifact_id,
                    artifact_type: &evidence.artifact_type,
                    product_version: &evidence.product_version,
                    source_commit: &evidence.source_commit,
                    artifact_sha256: &evidence.artifact_sha256,
                    artifact_size_bytes: evidence.artifact_size_bytes,
                    manifest_sha256: &evidence.manifest_sha256,
                    authentication_method: &evidence.authentication_method,
                },
            }
        }
        ArtifactEvidence::LocallySuppliedArtifact { artifact_id, .. } => {
            CanonicalArtifact::LocallySupplied { artifact_id }
        }
        ArtifactEvidence::NativeManagedArtifact { package_identity } => {
            CanonicalArtifact::NativeManaged { package_identity }
        }
    }
}

fn fingerprint_review(
    request: &InstallationRequest,
    plan: &InstallationPlan,
    context: &PlanReviewContext,
) -> Result<PlanReviewFingerprint, EngineErrorCode> {
    if plan.schema_version != ENGINE_SCHEMA_VERSION {
        return Err(EngineErrorCode::UnsupportedSchema);
    }
    let artifact = plan.artifact.as_ref().ok_or(EngineErrorCode::InvalidPlan)?;
    let document = CanonicalReviewDocument {
        review_schema_version: PLAN_REVIEW_SCHEMA_VERSION,
        engine_schema_version: plan.schema_version,
        intent: request.intent,
        plan_id: &plan.plan_id,
        target_scope: plan.target_scope,
        device_role: &context.device_role,
        connection_mode: &context.connection_mode,
        platform: &context.platform,
        architecture: &context.architecture,
        channel: &context.channel,
        capability_generation: context.capability_generation,
        manifest_generation: context.manifest_generation,
        installation_state_generation: context.installation_state_generation,
        platform_policy_id: &context.platform_policy_id,
        effect_catalog_version: context.effect_catalog_version,
        security_policy_version: context.security_policy_version,
        required_policy_ids: &context.required_policy_ids,
        user_choices: &request.user_choices,
        artifact: canonical_artifact(artifact),
        ordered_effects: &plan.ordered_effects,
        preservation_set: &plan.preservation_set,
        expected_completion_verification: &plan.expected_completion_verification,
        preflight_findings: context
            .preflight_findings
            .iter()
            .map(|finding| CanonicalFinding {
                code: &finding.code,
                severity: finding.severity,
                blocks_review: finding.blocks_review,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&document).map_err(|_| EngineErrorCode::InvalidPlan)?;
    let mut digest = Sha256::new();
    digest.update(FINGERPRINT_DOMAIN);
    digest.update(PLAN_REVIEW_SCHEMA_VERSION.to_le_bytes());
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    Ok(PlanReviewFingerprint(digest.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EffectAuthority, EffectPhase, Precondition, PrivilegeRequirement, ReconciliationPolicy,
        RetryPolicy, Reversibility, VerificationContract, VerifiedArtifactEvidence,
    };

    fn artifact() -> ArtifactEvidence {
        ArtifactEvidence::VerifiedReleaseArtifact {
            evidence: VerifiedArtifactEvidence {
                artifact_id: "synveil-linux-amd64-1.0.0".into(),
                artifact_type: "appimage".into(),
                product_version: "1.0.0".into(),
                source_commit: "abc123".into(),
                artifact_sha256: "a".repeat(64),
                artifact_size_bytes: 1024,
                manifest_sha256: "b".repeat(64),
                authentication_method: "fixture-only".into(),
                verified_local_path: "/private/user/downloads/synveil.AppImage".into(),
            },
        }
    }

    fn request_and_plan() -> (InstallationRequest, InstallationPlan) {
        let artifact = artifact();
        let request = InstallationRequest {
            intent: InstallationIntent::Install,
            target_scope: TargetScope::PortableUser,
            verified_artifact: Some(artifact.clone()),
            user_choices: BTreeMap::from([("create_shortcut".into(), true)]),
        };
        let plan = InstallationPlan {
            schema_version: ENGINE_SCHEMA_VERSION,
            plan_id: "fixture-plan-1".into(),
            intent: InstallationIntent::Install,
            target_scope: TargetScope::PortableUser,
            ordered_effects: vec![effect("payload", PrivilegeRequirement::CurrentUser)],
            preservation_set: crate::PROTECTED_RESOURCES.to_vec(),
            expected_completion_verification: VerificationContract {
                verification_id: "install.verify".into(),
                expected: "payload present".into(),
            },
            artifact: Some(artifact),
        };
        (request, plan)
    }

    fn effect(id: &str, privilege: PrivilegeRequirement) -> InstallationEffect {
        InstallationEffect {
            effect_id: id.into(),
            phase: EffectPhase::Install,
            owner: ResourceClass::PackageOwned,
            resource_class: ResourceClass::PackageOwned,
            authority: EffectAuthority::InstallerAdapter,
            target_scope: TargetScope::PortableUser,
            preconditions: vec![Precondition {
                kind: "destination_absent".into(),
                expected: "true".into(),
            }],
            privilege,
            mutation_kind: "place_payload".into(),
            verification: Some(VerificationContract {
                verification_id: format!("verify.{id}"),
                expected: "present".into(),
            }),
            retry_policy: RetryPolicy::Never,
            reconciliation_policy: ReconciliationPolicy::NotApplicable,
            reversibility: Reversibility::NotApplicable,
            safe_inverse: None,
            preservation_constraints: vec![ResourceClass::CredentialState],
            artifact_identity: Some("synveil-linux-amd64-1.0.0".into()),
        }
    }

    fn context() -> PlanReviewContext {
        PlanReviewContext {
            device_role: "host".into(),
            connection_mode: "local_only".into(),
            platform: "linux".into(),
            architecture: "x86_64".into(),
            channel: "linux_appimage".into(),
            capability_generation: 7,
            manifest_generation: Some(9),
            installation_state_generation: 4,
            platform_policy_id: "linux.appimage.policy.v1".into(),
            effect_catalog_version: Some(1),
            security_policy_version: Some(1),
            required_policy_ids: vec!["P004-D02".into(), "P004-D14".into()],
            preflight_findings: vec![],
        }
    }

    fn snapshot(
        request: &InstallationRequest,
        plan: &InstallationPlan,
        context: PlanReviewContext,
    ) -> Result<PlanReviewSnapshot, EngineErrorCode> {
        PlanReviewSnapshot::validate_and_create(request, plan, context)
    }

    #[test]
    fn valid_synthetic_plan_has_deterministic_fingerprint_and_redacted_summary() {
        let (request, plan) = request_and_plan();
        let first = snapshot(&request, &plan, context()).unwrap();
        let second = snapshot(&request, &plan, context()).unwrap();
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.fingerprint_summary(), second.fingerprint_summary());
        assert_eq!(first.status(), PlanReviewStatus::AvailableForReview);
        assert!(!first.fingerprint_summary().contains("/private/"));
        assert!(!first.fingerprint_summary().contains("downloads"));
    }

    #[test]
    fn fingerprint_binds_effect_order_artifact_scope_privilege_preservation_and_context() {
        let (request, plan) = request_and_plan();
        let original = snapshot(&request, &plan, context()).unwrap().fingerprint();

        let mut changed = plan.clone();
        changed
            .ordered_effects
            .push(effect("second", PrivilegeRequirement::CurrentUser));
        let with_second_review = snapshot(&request, &changed, context()).unwrap();
        let with_second = with_second_review.fingerprint();
        changed.ordered_effects.swap(0, 1);
        let reordered = snapshot(&request, &changed, context()).unwrap();
        assert_ne!(with_second, reordered.fingerprint());
        // A prior ordering cannot be acknowledged against the reordered plan.
        let mut stale_ack = ConfirmationSession::new(&with_second_review);
        assert_eq!(
            stale_ack.acknowledge(&reordered, 3, 20),
            Err(AcknowledgementError::PlanChanged)
        );

        let mut changed_artifact = plan.clone();
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) =
            changed_artifact.artifact.as_mut()
        {
            evidence.artifact_id.push_str("-other");
        }
        changed_artifact.ordered_effects[0].artifact_identity =
            Some("synveil-linux-amd64-1.0.0-other".into());
        let changed_request = InstallationRequest {
            verified_artifact: changed_artifact.artifact.clone(),
            ..request.clone()
        };
        assert_ne!(
            original,
            snapshot(&changed_request, &changed_artifact, context())
                .unwrap()
                .fingerprint()
        );

        let mut changed_scope = plan.clone();
        changed_scope.target_scope = TargetScope::CurrentUser;
        changed_scope.ordered_effects[0].target_scope = TargetScope::CurrentUser;
        let changed_scope_request = InstallationRequest {
            target_scope: TargetScope::CurrentUser,
            ..request.clone()
        };
        assert_ne!(
            original,
            snapshot(&changed_scope_request, &changed_scope, context())
                .unwrap()
                .fingerprint()
        );

        let mut changed_privilege = plan.clone();
        changed_privilege.ordered_effects[0].privilege = PrivilegeRequirement::NativeAuthorization;
        assert_ne!(
            original,
            snapshot(&request, &changed_privilege, context())
                .unwrap()
                .fingerprint()
        );

        let mut changed_preservation = plan.clone();
        changed_preservation.ordered_effects[0]
            .preservation_constraints
            .clear();
        assert_ne!(
            original,
            snapshot(&request, &changed_preservation, context())
                .unwrap()
                .fingerprint()
        );

        let mut changed_context = context();
        changed_context.platform_policy_id.push_str(".new");
        assert_ne!(
            original,
            snapshot(&request, &plan, changed_context)
                .unwrap()
                .fingerprint()
        );
    }

    #[test]
    fn invalid_and_unsupported_plans_cannot_be_reviewed_or_acknowledged() {
        let (request, mut plan) = request_and_plan();
        plan.ordered_effects[0].preconditions.clear();
        assert!(snapshot(&request, &plan, context()).is_err());
        plan.schema_version = ENGINE_SCHEMA_VERSION + 1;
        assert!(matches!(
            snapshot(&request, &plan, context()),
            Err(EngineErrorCode::UnsupportedSchema)
        ));
        let (request, mut plan) = request_and_plan();
        plan.schema_version = ENGINE_SCHEMA_VERSION;
        assert!(matches!(
            PlanReviewSnapshot::validate_and_create_for_schema(99, &request, &plan, context()),
            Err(EngineErrorCode::UnsupportedSchema)
        ));

        let (request, mut plan) = request_and_plan();
        plan.artifact = None;
        let request = InstallationRequest {
            verified_artifact: None,
            ..request
        };
        assert!(matches!(
            snapshot(&request, &plan, context()),
            Err(EngineErrorCode::InvalidPlan)
        ));
    }

    #[test]
    fn acknowledgement_rejects_changed_artifact_scope_effect_privilege_and_preservation() {
        let (request, plan) = request_and_plan();
        let original = snapshot(&request, &plan, context()).unwrap();
        let mut session = ConfirmationSession::new(&original);

        let mut changed_artifact = plan.clone();
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) =
            changed_artifact.artifact.as_mut()
        {
            evidence.artifact_id.push_str("-replacement");
        }
        changed_artifact.ordered_effects[0].artifact_identity =
            Some("synveil-linux-amd64-1.0.0-replacement".into());
        let changed_artifact_request = InstallationRequest {
            verified_artifact: changed_artifact.artifact.clone(),
            ..request.clone()
        };
        let replacement =
            snapshot(&changed_artifact_request, &changed_artifact, context()).unwrap();
        assert_eq!(
            session.acknowledge(&replacement, 1, 20),
            Err(AcknowledgementError::PlanChanged)
        );
        assert_eq!(
            session.state(),
            ConfirmationState::Invalidated(ConfirmationInvalidationReason::NewerPlanSuperseded)
        );

        for mutate in [0, 1, 2, 3] {
            let mut changed = plan.clone();
            match mutate {
                0 => {
                    changed.target_scope = TargetScope::CurrentUser;
                    changed.ordered_effects[0].target_scope = TargetScope::CurrentUser;
                    let changed_request = InstallationRequest {
                        target_scope: TargetScope::CurrentUser,
                        ..request.clone()
                    };
                    let current = snapshot(&changed_request, &changed, context()).unwrap();
                    assert_ne!(original.fingerprint(), current.fingerprint());
                    let mut stale_ack = ConfirmationSession::new(&original);
                    assert_eq!(
                        stale_ack.acknowledge(&current, 2, 20),
                        Err(AcknowledgementError::PlanChanged)
                    );
                }
                1 => changed.ordered_effects[0]
                    .mutation_kind
                    .push_str("_different"),
                2 => changed.ordered_effects[0].privilege = PrivilegeRequirement::ElevatedSystem,
                _ => {
                    changed
                        .preservation_set
                        .push(ResourceClass::EphemeralRuntimeState);
                }
            }
            if mutate != 0 {
                let current = snapshot(&request, &changed, context()).unwrap();
                assert_ne!(original.fingerprint(), current.fingerprint());
                let mut stale_ack = ConfirmationSession::new(&original);
                assert_eq!(
                    stale_ack.acknowledge(&current, 2, 20),
                    Err(AcknowledgementError::PlanChanged)
                );
            }
        }
    }

    #[test]
    fn changed_material_findings_policy_and_generation_change_fingerprint() {
        let (request, plan) = request_and_plan();
        let initial = snapshot(&request, &plan, context()).unwrap().fingerprint();
        let mut changed = context();
        changed.preflight_findings.push(ReviewFinding {
            code: "POLICY_PENDING".into(),
            severity: ReviewFindingSeverity::Warning,
            blocks_review: false,
        });
        assert_ne!(
            initial,
            snapshot(&request, &plan, changed).unwrap().fingerprint()
        );
        let mut changed = context();
        changed.capability_generation += 1;
        assert_ne!(
            initial,
            snapshot(&request, &plan, changed).unwrap().fingerprint()
        );
    }

    #[test]
    fn manifest_and_existing_installation_generations_change_fingerprint() {
        let (request, plan) = request_and_plan();
        let initial = snapshot(&request, &plan, context()).unwrap().fingerprint();
        let mut changed = context();
        changed.manifest_generation = Some(10);
        assert_ne!(
            initial,
            snapshot(&request, &plan, changed).unwrap().fingerprint()
        );
        let mut changed = context();
        changed.installation_state_generation += 1;
        assert_ne!(
            initial,
            snapshot(&request, &plan, changed).unwrap().fingerprint()
        );
    }

    #[test]
    fn replacement_artifact_digest_invalidates_previous_review() {
        let (request, plan) = request_and_plan();
        let original = snapshot(&request, &plan, context()).unwrap();
        let mut replacement_plan = plan.clone();
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) =
            replacement_plan.artifact.as_mut()
        {
            evidence.artifact_sha256 = "d".repeat(64);
        }
        let replacement_request = InstallationRequest {
            verified_artifact: replacement_plan.artifact.clone(),
            ..request
        };
        let replacement = snapshot(&replacement_request, &replacement_plan, context()).unwrap();
        assert_ne!(original.fingerprint(), replacement.fingerprint());

        let mut prior_review = ConfirmationSession::new(&original);
        prior_review.acknowledge(&original, 1, 20).unwrap();
        assert_eq!(
            prior_review.acknowledge(&replacement, 2, 20),
            Err(AcknowledgementError::PlanChanged)
        );
    }

    #[test]
    fn authenticated_artifact_size_is_part_of_the_review_fingerprint() {
        let (request, plan) = request_and_plan();
        let original = snapshot(&request, &plan, context()).unwrap().fingerprint();
        let mut resized_plan = plan.clone();
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) =
            resized_plan.artifact.as_mut()
        {
            evidence.artifact_size_bytes += 1;
        }
        let resized_request = InstallationRequest {
            verified_artifact: resized_plan.artifact.clone(),
            ..request
        };
        let resized = snapshot(&resized_request, &resized_plan, context()).unwrap();
        assert_ne!(original, resized.fingerprint());
    }

    #[test]
    fn changed_role_connection_channel_and_policy_reject_previous_acknowledgement() {
        let (request, plan) = request_and_plan();
        let original = snapshot(&request, &plan, context()).unwrap();
        for change in 0..4 {
            let mut changed_context = context();
            match change {
                0 => changed_context.device_role = "client".into(),
                1 => changed_context.connection_mode = "manual".into(),
                2 => changed_context.channel = "different_channel".into(),
                _ => changed_context
                    .required_policy_ids
                    .push("P004-D14-v2".into()),
            }
            let changed = snapshot(&request, &plan, changed_context).unwrap();
            let mut prior = ConfirmationSession::new(&original);
            prior.acknowledge(&original, 1, 20).unwrap();
            assert_eq!(
                prior.acknowledge(&changed, 2, 20),
                Err(AcknowledgementError::PlanChanged)
            );
        }
    }

    #[test]
    fn acknowledgement_is_idempotent_expires_and_cannot_replay_foreign_plan() {
        let (request, plan) = request_and_plan();
        let review = snapshot(&request, &plan, context()).unwrap();
        let mut session = ConfirmationSession::new(&review);
        assert_eq!(session.acknowledge(&review, 10, 50), Ok(true));
        assert_eq!(session.acknowledge(&review, 11, 50), Ok(false));
        let ack = session.acknowledgement(12).unwrap();
        assert_eq!(ack.plan_id(), review.plan_id());
        assert_eq!(ack.expires_at(), 60);

        let mut foreign = plan.clone();
        foreign.plan_id = "foreign-plan".into();
        let foreign = snapshot(&request, &foreign, context()).unwrap();
        assert_eq!(
            session.acknowledge(&foreign, 13, 10),
            Err(AcknowledgementError::PlanChanged)
        );

        let mut expiring = ConfirmationSession::new(&review);
        assert_eq!(expiring.acknowledge(&review, 100, 2), Ok(true));
        assert!(expiring.acknowledgement(102).is_none());
        assert_eq!(
            expiring.state(),
            ConfirmationState::Invalidated(ConfirmationInvalidationReason::ConfirmationExpired)
        );
    }

    #[test]
    fn invalidation_cancel_restart_and_old_completion_cannot_restore_acknowledgement() {
        let (request, plan) = request_and_plan();
        let review = snapshot(&request, &plan, context()).unwrap();
        let mut session = ConfirmationSession::new(&review);
        session.acknowledge(&review, 1, 20).unwrap();
        let generation = session.generation();
        session.invalidate(ConfirmationInvalidationReason::PreflightFindingsChanged);
        assert!(!session.accept_async_snapshot(generation, &review));
        assert!(session.acknowledgement(2).is_none());
        assert_eq!(
            session.acknowledge(&review, 2, 20),
            Err(AcknowledgementError::PlanChanged)
        );
        session.cancel();
        assert_eq!(session.state(), ConfirmationState::Cancelled);
        assert_eq!(
            session.acknowledge(&review, 3, 20),
            Err(AcknowledgementError::SessionClosed)
        );
        session.restart();
        assert!(session.acknowledgement(4).is_none());
    }

    #[test]
    fn paths_are_excluded_from_fingerprint_but_artifact_evidence_is_bound() {
        let (request, mut plan) = request_and_plan();
        let first = snapshot(&request, &plan, context()).unwrap().fingerprint();
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = plan.artifact.as_mut()
        {
            evidence.verified_local_path = "/another/private/path".into();
        }
        let changed_request = InstallationRequest {
            verified_artifact: plan.artifact.clone(),
            ..request
        };
        assert_eq!(
            first,
            snapshot(&changed_request, &plan, context())
                .unwrap()
                .fingerprint()
        );
    }

    #[test]
    fn unverified_artifact_and_blocking_finding_cannot_be_acknowledged() {
        let (mut request, mut plan) = request_and_plan();
        let artifact = plan.artifact.take().unwrap();
        plan.artifact = Some(ArtifactEvidence::LocallySuppliedArtifact {
            artifact_id: artifact.identity().to_owned(),
            local_path: "/private/unverified.file".into(),
        });
        request.verified_artifact = plan.artifact.clone();
        let untrusted = snapshot(&request, &plan, context()).unwrap();
        assert_eq!(
            untrusted.status(),
            PlanReviewStatus::ArtifactEvidenceUnavailable
        );
        let mut session = ConfirmationSession::new(&untrusted);
        assert_eq!(
            session.acknowledge(&untrusted, 1, 10),
            Err(AcknowledgementError::ReviewUnavailable)
        );

        let (request, plan) = request_and_plan();
        let mut blocked_context = context();
        blocked_context.preflight_findings.push(ReviewFinding {
            code: "BLOCKING_FINDING".into(),
            severity: ReviewFindingSeverity::Blocking,
            blocks_review: true,
        });
        let blocked = snapshot(&request, &plan, blocked_context).unwrap();
        assert_eq!(blocked.status(), PlanReviewStatus::ReviewDataIncomplete);
        let mut session = ConfirmationSession::new(&blocked);
        assert_eq!(
            session.acknowledge(&blocked, 1, 10),
            Err(AcknowledgementError::ReviewUnavailable)
        );
    }

    #[test]
    fn readiness_never_means_installation_and_does_not_infer_native_authority() {
        let input = ReadinessInput {
            review_status: PlanReviewStatus::AvailableForReview,
            platform_supported: true,
            channel_supported: true,
            scope_supported: true,
            blocking_preflight: false,
            stale: false,
            artifact: Some(artifact()),
            privilege_policy_approved: false,
            publisher_policy_approved: false,
            native_authorization_available: false,
            recovery_contract_qualified: false,
        };
        let result = evaluate_readiness(&input);
        assert!(result.eligible_for_review);
        assert_eq!(result.review_category, ReadinessCategory::ReadyForReview);
        assert!(
            result
                .execution_blockers
                .contains(&ReadinessCategory::NativeAuthorizationUnavailable)
        );
        assert!(
            result
                .execution_blockers
                .contains(&ReadinessCategory::PublisherPolicyPending)
        );
        assert!(
            result
                .execution_blockers
                .contains(&ReadinessCategory::ExecutionNotImplemented)
        );

        let mut every_other_gate = input.clone();
        every_other_gate.privilege_policy_approved = true;
        every_other_gate.publisher_policy_approved = true;
        every_other_gate.native_authorization_available = true;
        every_other_gate.recovery_contract_qualified = true;
        let result = evaluate_readiness(&every_other_gate);
        assert!(result.eligible_for_review);
        assert_eq!(
            result.execution_blockers,
            vec![ReadinessCategory::ExecutionNotImplemented]
        );

        let mut missing = input;
        missing.artifact = None;
        assert!(!evaluate_readiness(&missing).eligible_for_review);
        assert!(
            evaluate_readiness(&missing)
                .execution_blockers
                .contains(&ReadinessCategory::MissingVerifiedArtifact)
        );
    }

    #[test]
    fn fingerprint_does_not_claim_to_sign_or_authorize_anything() {
        let (request, plan) = request_and_plan();
        let review = snapshot(&request, &plan, context()).unwrap();
        let mut session = ConfirmationSession::new(&review);
        session.acknowledge(&review, 0, 10).unwrap();
        assert_eq!(session.state(), ConfirmationState::AcknowledgedLocally);
        assert!(review.is_unchanged_and_valid());
        // Acknowledgement remains a local review record and has no effect API.
    }
}
