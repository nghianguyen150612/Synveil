//! Side-effect-free execution readiness and recovery contracts.
//!
//! This module classifies whether a future install operation could be planned.
//! It has no adapter, journal handle, process launcher, authorization prompt,
//! or execution entrypoint. Production execution is deliberately disabled.

use crate::{
    AdapterValidationBlocker, AdapterValidationResult, AdapterValidationStatus, ArtifactEvidence,
    ByteIntegrityStatus, ConfirmationInvalidationReason, ConfirmationSession, ConfirmationState,
    EffectAuthority, EffectPhase, InstallationPlan, InstallationRequest, InstallerEngine,
    JournalEnvelope, JournalErrorCode, LastUseVerificationBlocker, LastUseVerificationResult,
    LastUseVerificationStatus, PlanReviewContext, PlanReviewFingerprint, PlanReviewSnapshot,
    PlanReviewStatus, Precondition, PrivilegeRequirement, PublisherTrustStatus,
    ReconciliationPolicy, RecoveryDisposition, ResourceClass, RetryPolicy, Reversibility,
    TargetScope, UseTimeIdentityStatus, VerificationContract,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EXECUTION_GATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionGateStatus {
    Unavailable,
    AwaitingVerifiedPlan,
    AwaitingReview,
    ReviewAcknowledged,
    RevalidationRequired,
    AuthorizationPolicyPending,
    NativeAuthorizationUnavailable,
    RecoveryBlocked,
    EligibleForFutureExecution,
    Invalidated,
    Cancelled,
}

impl ExecutionGateStatus {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::AwaitingVerifiedPlan => "awaiting_verified_plan",
            Self::AwaitingReview => "awaiting_review",
            Self::ReviewAcknowledged => "review_acknowledged",
            Self::RevalidationRequired => "revalidation_required",
            Self::AuthorizationPolicyPending => "authorization_policy_pending",
            Self::NativeAuthorizationUnavailable => "native_authorization_unavailable",
            Self::RecoveryBlocked => "recovery_blocked",
            Self::EligibleForFutureExecution => "eligible_for_future_execution",
            Self::Invalidated => "invalidated",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionBlocker {
    VerifiedPlanUnavailable,
    PlanValidationFailed,
    ArtifactProvenanceUnavailable,
    SyntheticArtifactRejected,
    UnsupportedPlatformChannel,
    UnsupportedTargetScope,
    ApprovedEffectCatalogUnavailable,
    UnknownEffectId,
    EffectOwnerMismatch,
    ReviewAcknowledgementRequired,
    ReviewAcknowledgementExpired,
    ReviewAcknowledgementMismatch,
    ReviewContextChanged,
    ManifestGenerationUnavailable,
    CapabilityGenerationUnavailable,
    EffectCatalogVersionUnavailable,
    SecurityPolicyVersionUnavailable,
    RequiredPolicyApprovalPending,
    NativeAuthorizationUnavailable,
    RecoveryPolicyPending,
    JournalEvidenceUnavailable,
    RecoveryReadinessBlocked,
    LastUseVerificationUnavailable,
    LastUseVerificationStale,
    LastUseVerificationSizeMismatch,
    LastUseVerificationDigestMismatch,
    LastUseVerificationIdentityChanged,
    LastUseExpectedDigestUnavailable,
    PublisherTrustPolicyUnavailable,
    UseTimeIdentityNotGuaranteed,
    InstallationStateGenerationUnavailable,
    NativeAdapterCapabilityUnavailable,
    NativeAdapterCapabilityUnsupported,
    NativeAdapterBindingStale,
    NativeAdapterCatalogMismatch,
    ProductionExecutionCapabilityDisabled,
}

impl ExecutionBlocker {
    pub const fn label(self) -> &'static str {
        match self {
            Self::VerifiedPlanUnavailable => "No authoritative production plan is available.",
            Self::PlanValidationFailed => "The current plan failed installer-engine validation.",
            Self::ArtifactProvenanceUnavailable => {
                "Authenticated publisher and exact artifact-byte evidence is unavailable."
            }
            Self::SyntheticArtifactRejected => {
                "Synthetic test artifact evidence cannot authorize production readiness."
            }
            Self::UnsupportedPlatformChannel => {
                "The platform and installation channel are not an approved pair."
            }
            Self::UnsupportedTargetScope => {
                "The selected target scope is unsupported for this channel."
            }
            Self::ApprovedEffectCatalogUnavailable => {
                "No approved closed effect catalog is available."
            }
            Self::UnknownEffectId => "The plan contains an effect outside the closed catalog.",
            Self::EffectOwnerMismatch => {
                "An effect is assigned to a component that does not own it."
            }
            Self::ReviewAcknowledgementRequired => {
                "The exact current plan has not been acknowledged for review."
            }
            Self::ReviewAcknowledgementExpired => {
                "The review acknowledgement expired and must be renewed."
            }
            Self::ReviewAcknowledgementMismatch => {
                "The acknowledgement does not match the current plan and review fingerprint."
            }
            Self::ReviewContextChanged => {
                "Plan, artifact, role, channel, scope, or capability evidence changed."
            }
            Self::ManifestGenerationUnavailable => {
                "Authenticated manifest generation evidence is unavailable."
            }
            Self::CapabilityGenerationUnavailable => {
                "Platform capability generation evidence is unavailable."
            }
            Self::EffectCatalogVersionUnavailable => {
                "The effect catalog version is not established."
            }
            Self::SecurityPolicyVersionUnavailable => {
                "The current security policy version is not established."
            }
            Self::RequiredPolicyApprovalPending => {
                "Required installer security policy decisions remain pending."
            }
            Self::NativeAuthorizationUnavailable => {
                "No approved native authorization implementation is available."
            }
            Self::RecoveryPolicyPending => {
                "Journal ownership and recovery policy are not qualified."
            }
            Self::JournalEvidenceUnavailable => {
                "Recovery evidence is unavailable without opening or creating a journal."
            }
            Self::RecoveryReadinessBlocked => {
                "Recovery evidence does not permit a safe automatic continuation."
            }
            Self::LastUseVerificationUnavailable => {
                "Staged artifact bytes and trusted policy cannot be rechecked at last use."
            }
            Self::LastUseVerificationStale => {
                "Last-use artifact evidence belongs to a different plan or generation."
            }
            Self::LastUseVerificationSizeMismatch => {
                "Last-use artifact bytes do not have the exact authenticated size."
            }
            Self::LastUseVerificationDigestMismatch => {
                "Last-use artifact bytes do not match the trusted expected SHA-256."
            }
            Self::LastUseVerificationIdentityChanged => {
                "The artifact or staging identity changed during last-use verification."
            }
            Self::LastUseExpectedDigestUnavailable => {
                "A trusted expected artifact digest is unavailable."
            }
            Self::PublisherTrustPolicyUnavailable => {
                "An approved publisher trust root and policy are unavailable."
            }
            Self::UseTimeIdentityNotGuaranteed => {
                "The future installer consumer cannot yet be bound to the verified file handle."
            }
            Self::InstallationStateGenerationUnavailable => {
                "The current installation-state generation is unavailable."
            }
            Self::NativeAdapterCapabilityUnavailable => {
                "No typed platform adapter capability is bound to this plan."
            }
            Self::NativeAdapterCapabilityUnsupported => {
                "The platform adapter does not support this channel or target scope."
            }
            Self::NativeAdapterBindingStale => {
                "The adapter capability was evaluated for an older plan or generation."
            }
            Self::NativeAdapterCatalogMismatch => {
                "The effect catalog does not match the exact adapter and resource owner."
            }
            Self::ProductionExecutionCapabilityDisabled => {
                "Production Wizard execution is disabled in this contract-only milestone."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorizationRequirementCategory {
    CurrentUserOperation,
    NativePackageManagerAuthorization,
    ScopedElevatedSystemOperation,
    UnsupportedAuthorization,
    AuthorizationPolicyUnavailable,
}

impl AuthorizationRequirementCategory {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CurrentUserOperation => "current_user_operation",
            Self::NativePackageManagerAuthorization => "native_package_manager_authorization",
            Self::ScopedElevatedSystemOperation => "scoped_elevated_system_operation",
            Self::UnsupportedAuthorization => "unsupported_authorization",
            Self::AuthorizationPolicyUnavailable => "authorization_policy_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NativeAuthorizationAvailability {
    CurrentUserContractOnly,
    PackageManagerContractOnly,
    ScopedSystemUnavailable,
    Unsupported,
    PolicyUnavailable,
}

impl NativeAuthorizationAvailability {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CurrentUserContractOnly => "current_user_contract_only",
            Self::PackageManagerContractOnly => "package_manager_contract_only",
            Self::ScopedSystemUnavailable => "scoped_system_unavailable",
            Self::Unsupported => "unsupported",
            Self::PolicyUnavailable => "policy_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorizationActor {
    CurrentUser,
    NativePackageManager,
    BoundedPlatformHelper,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NativeAuthorizationOutcome {
    Approved,
    Refused,
    Cancelled,
    StalePlan,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorizationOutcomeDisposition {
    ExactRequirementStillNeedsIndependentRevalidation,
    DeniedWithoutScopeFallback,
    CancelledWithoutScopeFallback,
    RevalidatePlanAndPolicy,
    AuthorizationUnavailable,
}

/// Classifies a future native result. Even an approval is only a statement
/// about the exact requirement; P011 has no effect execution consumer.
pub fn classify_authorization_outcome(
    outcome: NativeAuthorizationOutcome,
    requirement_still_matches: bool,
) -> AuthorizationOutcomeDisposition {
    if !requirement_still_matches || outcome == NativeAuthorizationOutcome::StalePlan {
        return AuthorizationOutcomeDisposition::RevalidatePlanAndPolicy;
    }
    match outcome {
        NativeAuthorizationOutcome::Approved => {
            AuthorizationOutcomeDisposition::ExactRequirementStillNeedsIndependentRevalidation
        }
        NativeAuthorizationOutcome::Refused => {
            AuthorizationOutcomeDisposition::DeniedWithoutScopeFallback
        }
        NativeAuthorizationOutcome::Cancelled => {
            AuthorizationOutcomeDisposition::CancelledWithoutScopeFallback
        }
        NativeAuthorizationOutcome::StalePlan => {
            AuthorizationOutcomeDisposition::RevalidatePlanAndPolicy
        }
        NativeAuthorizationOutcome::Unavailable => {
            AuthorizationOutcomeDisposition::AuthorizationUnavailable
        }
    }
}

/// A requirement for one closed-catalog operation. This is descriptive
/// evidence only; the fingerprint is not a token and no OS authorization is
/// requested by P011.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectAuthorizationRequirement {
    pub effect_id: EffectId,
    pub category: AuthorizationRequirementCategory,
    pub availability: NativeAuthorizationAvailability,
    pub actor: AuthorizationActor,
    pub target_scope: TargetScope,
    pub plan_id: String,
    pub review_fingerprint_sha256: String,
    pub artifact_id: Option<String>,
    pub manifest_generation: Option<u64>,
    pub capability_generation: u64,
    pub effect_catalog_version: Option<u32>,
    pub security_policy_version: Option<u32>,
    pub installation_state_generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryReadiness {
    Ready,
    JournalUnavailable,
    JournalBusy,
    JournalCorrupt,
    PlanMismatch,
    InspectionRequired,
    ReplanRequired,
    UnknownPriorOutcome,
    CompensationUnavailable,
    FinalVerificationUnavailable,
    RecoveryPolicyPending,
}

impl RecoveryReadiness {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::JournalUnavailable => "journal_unavailable",
            Self::JournalBusy => "journal_busy",
            Self::JournalCorrupt => "journal_corrupt",
            Self::PlanMismatch => "plan_mismatch",
            Self::InspectionRequired => "inspection_required",
            Self::ReplanRequired => "replan_required",
            Self::UnknownPriorOutcome => "unknown_prior_outcome",
            Self::CompensationUnavailable => "compensation_unavailable",
            Self::FinalVerificationUnavailable => "final_verification_unavailable",
            Self::RecoveryPolicyPending => "recovery_policy_pending",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalEvidence<'a> {
    NotInspected,
    Busy,
    Corrupt,
    Records(&'a [JournalEnvelope]),
}

pub struct ExecutionGateInput<'a> {
    pub request: Option<&'a InstallationRequest>,
    pub plan: Option<&'a InstallationPlan>,
    pub review: Option<&'a PlanReviewSnapshot>,
    pub current_review_context: Option<&'a PlanReviewContext>,
    pub acknowledgement: Option<&'a mut ConfirmationSession>,
    pub last_use_verification: Option<&'a LastUseVerificationResult>,
    pub adapter_validation: Option<&'a AdapterValidationResult>,
    pub recovery_readiness: RecoveryReadiness,
    pub now: u64,
}

impl<'a> ExecutionGateInput<'a> {
    /// Empty input for UI presentation while no authoritative production plan
    /// source exists. It always evaluates to blocked readiness.
    pub const fn unavailable() -> Self {
        Self {
            request: None,
            plan: None,
            review: None,
            current_review_context: None,
            acknowledgement: None,
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::RecoveryPolicyPending,
            now: 0,
        }
    }
}

#[derive(Clone)]
pub struct ExecutionReadinessReport {
    pub schema_version: u32,
    pub status: ExecutionGateStatus,
    pub review_status: PlanReviewStatus,
    pub acknowledgement_status: ConfirmationState,
    pub recovery_readiness: RecoveryReadiness,
    pub native_authorization_availability: NativeAuthorizationAvailability,
    pub byte_integrity_status: ByteIntegrityStatus,
    pub publisher_trust_status: PublisherTrustStatus,
    pub last_use_verification_status: LastUseVerificationStatus,
    pub adapter_validation: Option<AdapterValidationResult>,
    pub required_policy_approvals: Vec<&'static str>,
    pub blockers: Vec<ExecutionBlocker>,
    pub authorization_requirements: Vec<EffectAuthorizationRequirement>,
    pub plan_freshness_verified: bool,
    pub current_plan_id: Option<String>,
    fingerprint: Option<PlanReviewFingerprint>,
}

impl ExecutionReadinessReport {
    pub fn blocker_summary(&self) -> String {
        let labels: Vec<_> = self
            .blockers
            .iter()
            .map(|blocker| blocker.label())
            .collect();
        if labels.is_empty() {
            "No execution blockers were reported.".to_owned()
        } else {
            labels.join(" ")
        }
    }

    pub const fn review_fingerprint(&self) -> Option<PlanReviewFingerprint> {
        self.fingerprint
    }
}

pub fn evaluate_execution_readiness(mut input: ExecutionGateInput<'_>) -> ExecutionReadinessReport {
    let mut blockers = Vec::new();
    let mut requirements = Vec::new();
    let mut review_status = PlanReviewStatus::NoPlanAvailable;
    let mut acknowledgement_status = ConfirmationState::Unacknowledged;
    let mut plan_freshness_verified = false;
    let mut current_plan_id = None;
    let mut fingerprint = None;
    let mut native_authorization_availability = NativeAuthorizationAvailability::PolicyUnavailable;
    let mut status = ExecutionGateStatus::AwaitingVerifiedPlan;
    let byte_integrity_status = input.last_use_verification.map_or(
        ByteIntegrityStatus::Unavailable,
        LastUseVerificationResult::byte_integrity_status,
    );
    let publisher_trust_status = PublisherTrustStatus::Unavailable;
    let last_use_verification_status = input.last_use_verification.map_or(
        LastUseVerificationStatus::Unavailable,
        LastUseVerificationResult::status,
    );
    let adapter_validation = input.adapter_validation.cloned();

    match (input.request, input.plan) {
        (Some(request), Some(plan)) => {
            current_plan_id = Some(plan.plan_id.clone());
            if InstallerEngine.validate(request, plan).is_err() {
                blockers.push(ExecutionBlocker::PlanValidationFailed);
                status = ExecutionGateStatus::Unavailable;
            } else {
                status = ExecutionGateStatus::AwaitingReview;
            }

            if !matches!(
                &plan.artifact,
                Some(ArtifactEvidence::VerifiedReleaseArtifact { .. })
            ) {
                blockers.push(ExecutionBlocker::ArtifactProvenanceUnavailable);
            } else if matches!(
                &plan.artifact,
                Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence })
                    if evidence.authentication_method.to_ascii_lowercase().contains("synthetic")
            ) {
                blockers.push(ExecutionBlocker::SyntheticArtifactRejected);
                blockers.push(ExecutionBlocker::ArtifactProvenanceUnavailable);
            } else {
                // P012's byte verifier is available only behind a crate-owned
                // trusted-source handoff. No authenticated source or approved
                // production root is connected to the Wizard yet.
                blockers.push(ExecutionBlocker::ArtifactProvenanceUnavailable);
            }

            let context = input.current_review_context;
            if let Some(context) = context {
                if context.manifest_generation.is_none() {
                    blockers.push(ExecutionBlocker::ManifestGenerationUnavailable);
                }
                if context.capability_generation == 0 {
                    blockers.push(ExecutionBlocker::CapabilityGenerationUnavailable);
                }
                if context.installation_state_generation == 0 {
                    blockers.push(ExecutionBlocker::InstallationStateGenerationUnavailable);
                }
                if context.effect_catalog_version.is_none() {
                    blockers.push(ExecutionBlocker::EffectCatalogVersionUnavailable);
                }
                if context.security_policy_version.is_none() {
                    blockers.push(ExecutionBlocker::SecurityPolicyVersionUnavailable);
                }
                if !context_channel_and_scope_supported(context, plan.target_scope) {
                    blockers.push(ExecutionBlocker::UnsupportedPlatformChannel);
                    if !context_scope_supported(context, plan.target_scope) {
                        blockers.push(ExecutionBlocker::UnsupportedTargetScope);
                    }
                }
                if context
                    .required_policy_ids
                    .iter()
                    .any(|policy| matches!(policy.as_str(), "P004-D02" | "P004-D11" | "P004-D14"))
                    || !context.required_policy_ids.is_empty()
                {
                    blockers.push(ExecutionBlocker::RequiredPolicyApprovalPending);
                }
            } else {
                blockers.push(ExecutionBlocker::ManifestGenerationUnavailable);
                blockers.push(ExecutionBlocker::CapabilityGenerationUnavailable);
                blockers.push(ExecutionBlocker::EffectCatalogVersionUnavailable);
                blockers.push(ExecutionBlocker::SecurityPolicyVersionUnavailable);
                blockers.push(ExecutionBlocker::ReviewContextChanged);
            }

            if plan
                .ordered_effects
                .iter()
                .any(|effect| EffectId::parse(&effect.effect_id).is_none())
            {
                blockers.push(ExecutionBlocker::UnknownEffectId);
            }
            // No production catalog is approved while P004-D02/D11 are pending.
            blockers.push(ExecutionBlocker::ApprovedEffectCatalogUnavailable);

            if let (Some(review), Some(context)) = (input.review, context) {
                review_status = review.status();
                fingerprint = Some(review.fingerprint());
                let current = review.matches_current(request, plan, context);
                if !current {
                    blockers.push(ExecutionBlocker::ReviewContextChanged);
                    status = ExecutionGateStatus::RevalidationRequired;
                } else {
                    plan_freshness_verified = true;
                }

                match input.acknowledgement.as_deref_mut() {
                    Some(session) => {
                        if session.acknowledgement_matches_snapshot(review, input.now) {
                            acknowledgement_status = session.state();
                            if status == ExecutionGateStatus::AwaitingReview {
                                status = ExecutionGateStatus::ReviewAcknowledged;
                            }
                        } else {
                            acknowledgement_status = session.state();
                            if matches!(
                                acknowledgement_status,
                                ConfirmationState::Invalidated(
                                    ConfirmationInvalidationReason::ConfirmationExpired
                                )
                            ) {
                                blockers.push(ExecutionBlocker::ReviewAcknowledgementExpired);
                            } else {
                                blockers.push(ExecutionBlocker::ReviewAcknowledgementMismatch);
                            }
                            if status != ExecutionGateStatus::RevalidationRequired {
                                status = ExecutionGateStatus::AwaitingReview;
                            }
                        }
                    }
                    None => {
                        blockers.push(ExecutionBlocker::ReviewAcknowledgementRequired);
                    }
                }
            } else {
                review_status = PlanReviewStatus::NoPlanAvailable;
                blockers.push(ExecutionBlocker::ReviewAcknowledgementRequired);
            }

            match (input.adapter_validation, input.review, context) {
                (Some(adapter), Some(review), Some(context)) => {
                    if !adapter.matches_current(request, plan, review, context) {
                        blockers.push(ExecutionBlocker::NativeAdapterBindingStale);
                    }
                    if adapter.status == AdapterValidationStatus::Unsupported {
                        blockers.push(ExecutionBlocker::NativeAdapterCapabilityUnsupported);
                    }
                    if adapter.status == AdapterValidationStatus::CatalogMismatch
                        || adapter.blockers.iter().any(|blocker| {
                            matches!(
                                blocker,
                                AdapterValidationBlocker::ClosedCatalogMismatch
                                    | AdapterValidationBlocker::EffectOwnerMismatch
                                    | AdapterValidationBlocker::EffectNotOwnedByAdapter
                                    | AdapterValidationBlocker::UnknownEffectId
                            )
                        })
                    {
                        blockers.push(ExecutionBlocker::NativeAdapterCatalogMismatch);
                    }
                }
                _ => blockers.push(ExecutionBlocker::NativeAdapterCapabilityUnavailable),
            }

            for effect in &plan.ordered_effects {
                let Some(effect_id) = EffectId::parse(&effect.effect_id) else {
                    continue;
                };
                if !effect_owner_matches_context(effect_id, effect, context) {
                    blockers.push(ExecutionBlocker::EffectOwnerMismatch);
                }
                let (category, availability, actor) = classify_authorization(
                    effect_id,
                    context.map(|value| value.channel.as_str()).unwrap_or(""),
                    context.map(|value| value.platform.as_str()).unwrap_or(""),
                    plan.target_scope,
                );
                native_authorization_availability = availability;
                requirements.push(EffectAuthorizationRequirement {
                    effect_id,
                    category,
                    availability,
                    actor,
                    target_scope: effect.target_scope,
                    plan_id: plan.plan_id.clone(),
                    review_fingerprint_sha256: fingerprint
                        .map(|value| value.as_hex())
                        .unwrap_or_default(),
                    artifact_id: plan
                        .artifact
                        .as_ref()
                        .map(|artifact| artifact.identity().to_owned()),
                    manifest_generation: context.and_then(|value| value.manifest_generation),
                    capability_generation: context.map_or(0, |value| value.capability_generation),
                    effect_catalog_version: context.and_then(|value| value.effect_catalog_version),
                    security_policy_version: context
                        .and_then(|value| value.security_policy_version),
                    installation_state_generation: context
                        .map_or(0, |value| value.installation_state_generation),
                });
            }
            blockers.push(ExecutionBlocker::NativeAuthorizationUnavailable);
        }
        _ => blockers.push(ExecutionBlocker::VerifiedPlanUnavailable),
    }

    let recovery_readiness = input.recovery_readiness;
    match recovery_readiness {
        RecoveryReadiness::Ready => {}
        RecoveryReadiness::RecoveryPolicyPending => {
            blockers.push(ExecutionBlocker::RecoveryPolicyPending)
        }
        RecoveryReadiness::JournalUnavailable => {
            blockers.push(ExecutionBlocker::JournalEvidenceUnavailable)
        }
        _ => {
            blockers.push(ExecutionBlocker::RecoveryReadinessBlocked);
            if status != ExecutionGateStatus::RevalidationRequired {
                status = ExecutionGateStatus::RecoveryBlocked;
            }
        }
    }
    if !blockers.contains(&ExecutionBlocker::RequiredPolicyApprovalPending) {
        blockers.push(ExecutionBlocker::RequiredPolicyApprovalPending);
    }
    match (
        input.last_use_verification,
        input.request,
        input.plan,
        input.review,
        input.current_review_context,
    ) {
        (
            Some(LastUseVerificationResult::ByteIntegrityVerified(handle)),
            Some(request),
            Some(plan),
            Some(review),
            Some(context),
        ) => {
            if !handle.context_matches(request, plan, review, context) {
                blockers.push(ExecutionBlocker::LastUseVerificationStale);
            }
            match handle.publisher_trust_status() {
                PublisherTrustStatus::Unavailable => {
                    blockers.push(ExecutionBlocker::PublisherTrustPolicyUnavailable)
                }
            }
            if handle.use_time_identity_status() == UseTimeIdentityStatus::NotGuaranteed {
                blockers.push(ExecutionBlocker::UseTimeIdentityNotGuaranteed);
            }
        }
        (Some(LastUseVerificationResult::Blocked(blocker)), _, _, _, _) => {
            blockers.extend(last_use_blockers(*blocker));
        }
        _ => blockers.push(ExecutionBlocker::LastUseVerificationUnavailable),
    }
    if input.adapter_validation.is_some_and(|adapter| {
        adapter
            .blockers
            .contains(&AdapterValidationBlocker::ClosedCatalogMismatch)
    }) {
        blockers.push(ExecutionBlocker::NativeAdapterCatalogMismatch);
    }
    // There is intentionally no input that can turn on production execution.
    blockers.push(ExecutionBlocker::ProductionExecutionCapabilityDisabled);

    if status != ExecutionGateStatus::RevalidationRequired
        && matches!(status, ExecutionGateStatus::ReviewAcknowledged)
        && (blockers.contains(&ExecutionBlocker::RequiredPolicyApprovalPending)
            || blockers.contains(&ExecutionBlocker::ApprovedEffectCatalogUnavailable))
    {
        status = ExecutionGateStatus::AuthorizationPolicyPending;
    }

    ExecutionReadinessReport {
        schema_version: EXECUTION_GATE_SCHEMA_VERSION,
        status,
        review_status,
        acknowledgement_status,
        recovery_readiness,
        native_authorization_availability,
        byte_integrity_status,
        publisher_trust_status,
        last_use_verification_status,
        adapter_validation,
        required_policy_approvals: if input
            .current_review_context
            .is_some_and(|context| context.connection_mode == "tailscale")
        {
            vec!["P004-D02", "P004-D03", "P004-D11", "P004-D14"]
        } else {
            vec!["P004-D02", "P004-D11", "P004-D14"]
        },
        blockers,
        authorization_requirements: requirements,
        plan_freshness_verified,
        current_plan_id,
        fingerprint,
    }
}

fn last_use_blockers(blocker: LastUseVerificationBlocker) -> Vec<ExecutionBlocker> {
    match blocker {
        LastUseVerificationBlocker::TrustedExpectedDigestUnavailable
        | LastUseVerificationBlocker::ExpectedSizeUnavailable => {
            vec![ExecutionBlocker::LastUseExpectedDigestUnavailable]
        }
        LastUseVerificationBlocker::DigestMismatch => {
            vec![ExecutionBlocker::LastUseVerificationDigestMismatch]
        }
        LastUseVerificationBlocker::ExactSizeMismatch
        | LastUseVerificationBlocker::UnsupportedArtifactSize => {
            vec![ExecutionBlocker::LastUseVerificationSizeMismatch]
        }
        LastUseVerificationBlocker::AuthenticatedManifestUnavailable
        | LastUseVerificationBlocker::ApprovedTrustPolicyUnavailable => {
            vec![ExecutionBlocker::PublisherTrustPolicyUnavailable]
        }
        LastUseVerificationBlocker::FileIdentityChanged
        | LastUseVerificationBlocker::ConcurrentModification => {
            vec![ExecutionBlocker::LastUseVerificationIdentityChanged]
        }
        LastUseVerificationBlocker::StaleVerificationContext => {
            vec![ExecutionBlocker::LastUseVerificationStale]
        }
        LastUseVerificationBlocker::UseTimeIdentityNotGuaranteed => {
            vec![ExecutionBlocker::UseTimeIdentityNotGuaranteed]
        }
        LastUseVerificationBlocker::MissingArtifactBytes
        | LastUseVerificationBlocker::InvalidExpectedEvidence
        | LastUseVerificationBlocker::UnsafePath
        | LastUseVerificationBlocker::UnsafePathAncestry
        | LastUseVerificationBlocker::UnsupportedFileType
        | LastUseVerificationBlocker::OwnershipOrAccessRejected
        | LastUseVerificationBlocker::PlatformFileIdentityUnavailable
        | LastUseVerificationBlocker::IoUnavailable => {
            vec![ExecutionBlocker::LastUseVerificationUnavailable]
        }
    }
}

fn context_scope_supported(context: &PlanReviewContext, scope: TargetScope) -> bool {
    match context.channel.as_str() {
        "ubuntu_deb" | "fedora_rpm" => scope == TargetScope::NativePackage,
        "linux_appimage" | "windows_portable_zip" => scope == TargetScope::PortableUser,
        "windows_per_user_setup" => scope == TargetScope::CurrentUser,
        "managed_linux_host_action" => scope == TargetScope::System,
        "existing_installation" | "existing_installation_maintenance" => matches!(
            scope,
            TargetScope::CurrentUser | TargetScope::NativePackage | TargetScope::PortableUser
        ),
        _ => false,
    }
}

fn context_channel_and_scope_supported(context: &PlanReviewContext, scope: TargetScope) -> bool {
    if !context_scope_supported(context, scope) || context.architecture != "x86_64" {
        return false;
    }
    match context.channel.as_str() {
        "ubuntu_deb" => context.platform == "ubuntu-24.04",
        "fedora_rpm" => context.platform == "fedora-42",
        "linux_appimage" => matches!(
            context.platform.as_str(),
            "linux" | "ubuntu-24.04" | "fedora-42"
        ),
        "windows_per_user_setup" | "windows_portable_zip" => context.platform == "windows",
        "managed_linux_host_action" => {
            matches!(context.platform.as_str(), "ubuntu-24.04" | "fedora-42")
        }
        "existing_installation" | "existing_installation_maintenance" => matches!(
            context.platform.as_str(),
            "ubuntu-24.04" | "fedora-42" | "windows"
        ),
        _ => false,
    }
}

fn classify_authorization(
    effect_id: EffectId,
    channel: &str,
    platform: &str,
    scope: TargetScope,
) -> (
    AuthorizationRequirementCategory,
    NativeAuthorizationAvailability,
    AuthorizationActor,
) {
    match (channel, scope) {
        ("ubuntu_deb" | "fedora_rpm", TargetScope::NativePackage) => (
            AuthorizationRequirementCategory::NativePackageManagerAuthorization,
            NativeAuthorizationAvailability::PackageManagerContractOnly,
            AuthorizationActor::NativePackageManager,
        ),
        ("linux_appimage", TargetScope::PortableUser) if platform != "windows" => (
            AuthorizationRequirementCategory::CurrentUserOperation,
            NativeAuthorizationAvailability::CurrentUserContractOnly,
            AuthorizationActor::CurrentUser,
        ),
        (
            "windows_per_user_setup" | "windows_portable_zip",
            TargetScope::CurrentUser | TargetScope::PortableUser,
        ) if platform == "windows" => (
            AuthorizationRequirementCategory::CurrentUserOperation,
            NativeAuthorizationAvailability::CurrentUserContractOnly,
            AuthorizationActor::CurrentUser,
        ),
        (
            "existing_installation" | "existing_installation_maintenance",
            TargetScope::NativePackage,
        ) if matches!(
            effect_id,
            EffectId::NativeDebPackageTransaction | EffectId::NativeRpmPackageTransaction
        ) && ((effect_id == EffectId::NativeDebPackageTransaction
            && platform == "ubuntu-24.04")
            || (effect_id == EffectId::NativeRpmPackageTransaction && platform == "fedora-42")) =>
        {
            (
                AuthorizationRequirementCategory::NativePackageManagerAuthorization,
                NativeAuthorizationAvailability::PolicyUnavailable,
                AuthorizationActor::NativePackageManager,
            )
        }
        (
            "existing_installation" | "existing_installation_maintenance",
            TargetScope::PortableUser,
        ) if effect_id == EffectId::AppImageCurrentUserIntegration
            && matches!(platform, "linux" | "ubuntu-24.04" | "fedora-42") =>
        {
            (
                AuthorizationRequirementCategory::CurrentUserOperation,
                NativeAuthorizationAvailability::CurrentUserContractOnly,
                AuthorizationActor::CurrentUser,
            )
        }
        (
            "existing_installation" | "existing_installation_maintenance",
            TargetScope::CurrentUser,
        ) if effect_id == EffectId::WindowsPerUserSetup && platform == "windows" => (
            AuthorizationRequirementCategory::CurrentUserOperation,
            NativeAuthorizationAvailability::CurrentUserContractOnly,
            AuthorizationActor::CurrentUser,
        ),
        ("managed_linux_host_action", TargetScope::System)
            if effect_id == EffectId::BoundedLinuxHostAction
                && matches!(platform, "ubuntu-24.04" | "fedora-42") =>
        {
            (
                AuthorizationRequirementCategory::ScopedElevatedSystemOperation,
                NativeAuthorizationAvailability::PolicyUnavailable,
                AuthorizationActor::BoundedPlatformHelper,
            )
        }
        ("windows_per_user_setup", TargetScope::System)
            if effect_id == EffectId::WindowsPerUserSetup =>
        {
            (
                AuthorizationRequirementCategory::ScopedElevatedSystemOperation,
                NativeAuthorizationAvailability::ScopedSystemUnavailable,
                AuthorizationActor::Unsupported,
            )
        }
        (_, TargetScope::System) => (
            AuthorizationRequirementCategory::UnsupportedAuthorization,
            NativeAuthorizationAvailability::ScopedSystemUnavailable,
            AuthorizationActor::Unsupported,
        ),
        _ => (
            AuthorizationRequirementCategory::UnsupportedAuthorization,
            NativeAuthorizationAvailability::Unsupported,
            AuthorizationActor::Unsupported,
        ),
    }
}

fn effect_owner_matches_context(
    id: EffectId,
    effect: &crate::InstallationEffect,
    context: Option<&PlanReviewContext>,
) -> bool {
    let channel = context.map_or("", |value| value.channel.as_str());
    match id {
        EffectId::PackagePayloadInstall => {
            effect.owner == ResourceClass::PackageOwned
                && effect.resource_class == ResourceClass::PackageOwned
                && effect.authority == EffectAuthority::InstallerAdapter
        }
        EffectId::NativeDebPackageTransaction => {
            matches!(
                channel,
                "ubuntu_deb" | "existing_installation" | "existing_installation_maintenance"
            ) && context.is_some_and(|value| value.platform == "ubuntu-24.04")
                && effect.owner == ResourceClass::NativePackageState
                && effect.resource_class == ResourceClass::NativePackageState
                && effect.authority == EffectAuthority::NativePackageManager
        }
        EffectId::NativeRpmPackageTransaction => {
            matches!(
                channel,
                "fedora_rpm" | "existing_installation" | "existing_installation_maintenance"
            ) && context.is_some_and(|value| value.platform == "fedora-42")
                && effect.owner == ResourceClass::NativePackageState
                && effect.resource_class == ResourceClass::NativePackageState
                && effect.authority == EffectAuthority::NativePackageManager
        }
        EffectId::AppImageCurrentUserIntegration => {
            matches!(
                channel,
                "linux_appimage" | "existing_installation" | "existing_installation_maintenance"
            ) && context.is_some_and(|value| {
                matches!(
                    value.platform.as_str(),
                    "linux" | "ubuntu-24.04" | "fedora-42"
                )
            }) && effect.owner == ResourceClass::PlatformIntegrationOwned
                && effect.resource_class == ResourceClass::PlatformIntegrationOwned
                && effect.authority == EffectAuthority::PlatformIntegrationAdapter
                && effect.target_scope == TargetScope::PortableUser
        }
        EffectId::WindowsPerUserSetup => {
            matches!(
                channel,
                "windows_per_user_setup"
                    | "existing_installation"
                    | "existing_installation_maintenance"
            ) && context.is_some_and(|value| value.platform == "windows")
                && effect.owner == ResourceClass::PackageOwned
                && effect.resource_class == ResourceClass::PackageOwned
                && effect.authority == EffectAuthority::InstallerAdapter
                && effect.target_scope == TargetScope::CurrentUser
        }
        EffectId::BoundedLinuxHostAction => {
            channel == "managed_linux_host_action"
                && context.is_some_and(|value| {
                    matches!(value.platform.as_str(), "ubuntu-24.04" | "fedora-42")
                })
                && effect.target_scope == TargetScope::System
                && effect.owner == ResourceClass::PlatformIntegrationOwned
                && effect.resource_class == ResourceClass::PlatformIntegrationOwned
                && effect.authority == EffectAuthority::PlatformIntegrationAdapter
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectId {
    PackagePayloadInstall,
    NativeDebPackageTransaction,
    NativeRpmPackageTransaction,
    AppImageCurrentUserIntegration,
    WindowsPerUserSetup,
    BoundedLinuxHostAction,
}

impl EffectId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PackagePayloadInstall => "package.payload.install",
            Self::NativeDebPackageTransaction => "native.deb.package.transaction",
            Self::NativeRpmPackageTransaction => "native.rpm.package.transaction",
            Self::AppImageCurrentUserIntegration => "appimage.current-user.integration",
            Self::WindowsPerUserSetup => "windows.per-user.setup",
            Self::BoundedLinuxHostAction => "linux.host.bounded-action",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "package.payload.install" => Some(Self::PackagePayloadInstall),
            "native.deb.package.transaction" => Some(Self::NativeDebPackageTransaction),
            "native.rpm.package.transaction" => Some(Self::NativeRpmPackageTransaction),
            "appimage.current-user.integration" => Some(Self::AppImageCurrentUserIntegration),
            "windows.per-user.setup" => Some(Self::WindowsPerUserSetup),
            "linux.host.bounded-action" => Some(Self::BoundedLinuxHostAction),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectAdapter {
    InstallerEngine,
    NativePackageManager,
    LinuxAppImageIntegration,
    WindowsPerUserSetup,
    BoundedLinuxHostHelper,
}

/// All fields which must be reviewed for every operation in a future closed
/// catalog. P011 validates these values but does not publish an approved
/// production catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectCatalogEntry {
    pub id: EffectId,
    pub channel: String,
    pub adapter: EffectAdapter,
    pub resource_owner: ResourceClass,
    pub resource_class: ResourceClass,
    pub target_scope: TargetScope,
    pub phase: EffectPhase,
    pub preconditions: Vec<Precondition>,
    pub privilege: PrivilegeRequirement,
    pub mutation_kind: String,
    pub verification: VerificationContract,
    pub reconciliation: ReconciliationPolicy,
    pub retry: RetryPolicy,
    pub reversibility: Reversibility,
    pub safe_inverse: Option<String>,
    pub preservation_constraints: Vec<ResourceClass>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectCatalogViolation {
    UnknownEffectId,
    DuplicateCatalogEntry,
    MissingCatalogEntry,
    ChannelMismatch,
    AdapterMismatch,
    ResourceOwnerMismatch,
    ResourceClassMismatch,
    UnsupportedScope,
    ScopeMismatch,
    PhaseMismatch,
    PreconditionsMismatch,
    PrivilegeMismatch,
    MutationKindMismatch,
    VerificationMismatch,
    ReconciliationMismatch,
    RetryMismatch,
    ReversibilityMismatch,
    SafeInverseMismatch,
    PreservationMismatch,
}

/// Pure comparison against a caller-supplied catalog description. This does
/// not approve the catalog or alter execution readiness.
pub fn validate_effect_catalog(
    plan: &InstallationPlan,
    channel: &str,
    entries: &[EffectCatalogEntry],
) -> Vec<EffectCatalogViolation> {
    let mut violations = Vec::new();
    let mut catalog_ids = BTreeSet::new();
    for entry in entries {
        if !catalog_ids.insert(entry.id) {
            violations.push(EffectCatalogViolation::DuplicateCatalogEntry);
        }
    }
    for effect in &plan.ordered_effects {
        let Some(id) = EffectId::parse(&effect.effect_id) else {
            violations.push(EffectCatalogViolation::UnknownEffectId);
            continue;
        };
        let Some(entry) = entries.iter().find(|entry| entry.id == id) else {
            violations.push(EffectCatalogViolation::MissingCatalogEntry);
            continue;
        };
        if entry.channel != channel {
            violations.push(EffectCatalogViolation::ChannelMismatch);
        }
        let (required_adapter, required_owner, required_class) = effect_identity(id);
        if entry.adapter != required_adapter {
            violations.push(EffectCatalogViolation::AdapterMismatch);
        }
        if entry.resource_owner != required_owner {
            violations.push(EffectCatalogViolation::ResourceOwnerMismatch);
        }
        if entry.resource_class != required_class {
            violations.push(EffectCatalogViolation::ResourceClassMismatch);
        }
        if !effect_channel_supported(id, channel) {
            violations.push(EffectCatalogViolation::ChannelMismatch);
        }
        if !scope_supported_by_channel(channel, entry.target_scope) {
            violations.push(EffectCatalogViolation::UnsupportedScope);
        }
        if effect.target_scope != entry.target_scope {
            violations.push(EffectCatalogViolation::ScopeMismatch);
        }
        if effect.phase != entry.phase {
            violations.push(EffectCatalogViolation::PhaseMismatch);
        }
        if effect.owner != entry.resource_owner {
            violations.push(EffectCatalogViolation::ResourceOwnerMismatch);
        }
        if effect.resource_class != entry.resource_class {
            violations.push(EffectCatalogViolation::ResourceClassMismatch);
        }
        if effect.preconditions != entry.preconditions {
            violations.push(EffectCatalogViolation::PreconditionsMismatch);
        }
        if effect.privilege != entry.privilege {
            violations.push(EffectCatalogViolation::PrivilegeMismatch);
        }
        if effect.mutation_kind != entry.mutation_kind {
            violations.push(EffectCatalogViolation::MutationKindMismatch);
        }
        if effect.verification.as_ref() != Some(&entry.verification) {
            violations.push(EffectCatalogViolation::VerificationMismatch);
        }
        if effect.reconciliation_policy != entry.reconciliation {
            violations.push(EffectCatalogViolation::ReconciliationMismatch);
        }
        if effect.retry_policy != entry.retry {
            violations.push(EffectCatalogViolation::RetryMismatch);
        }
        if effect.reversibility != entry.reversibility {
            violations.push(EffectCatalogViolation::ReversibilityMismatch);
        }
        if effect.safe_inverse != entry.safe_inverse {
            violations.push(EffectCatalogViolation::SafeInverseMismatch);
        }
        if effect.preservation_constraints != entry.preservation_constraints {
            violations.push(EffectCatalogViolation::PreservationMismatch);
        }
        if !adapter_matches(entry.adapter, effect.authority, effect.owner) {
            violations.push(EffectCatalogViolation::AdapterMismatch);
        }
    }
    violations
}

fn adapter_matches(
    adapter: EffectAdapter,
    authority: EffectAuthority,
    owner: ResourceClass,
) -> bool {
    match adapter {
        EffectAdapter::InstallerEngine => {
            authority == EffectAuthority::InstallerAdapter
                && owner != ResourceClass::NativePackageState
                && owner != ResourceClass::PlatformIntegrationOwned
        }
        EffectAdapter::NativePackageManager => {
            authority == EffectAuthority::NativePackageManager
                && owner == ResourceClass::NativePackageState
        }
        EffectAdapter::LinuxAppImageIntegration | EffectAdapter::BoundedLinuxHostHelper => {
            authority == EffectAuthority::PlatformIntegrationAdapter
                && owner == ResourceClass::PlatformIntegrationOwned
        }
        EffectAdapter::WindowsPerUserSetup => {
            authority == EffectAuthority::InstallerAdapter && owner == ResourceClass::PackageOwned
        }
    }
}

fn effect_identity(id: EffectId) -> (EffectAdapter, ResourceClass, ResourceClass) {
    match id {
        EffectId::PackagePayloadInstall => (
            EffectAdapter::InstallerEngine,
            ResourceClass::PackageOwned,
            ResourceClass::PackageOwned,
        ),
        EffectId::NativeDebPackageTransaction | EffectId::NativeRpmPackageTransaction => (
            EffectAdapter::NativePackageManager,
            ResourceClass::NativePackageState,
            ResourceClass::NativePackageState,
        ),
        EffectId::AppImageCurrentUserIntegration => (
            EffectAdapter::LinuxAppImageIntegration,
            ResourceClass::PlatformIntegrationOwned,
            ResourceClass::PlatformIntegrationOwned,
        ),
        EffectId::WindowsPerUserSetup => (
            EffectAdapter::WindowsPerUserSetup,
            ResourceClass::PackageOwned,
            ResourceClass::PackageOwned,
        ),
        EffectId::BoundedLinuxHostAction => (
            EffectAdapter::BoundedLinuxHostHelper,
            ResourceClass::PlatformIntegrationOwned,
            ResourceClass::PlatformIntegrationOwned,
        ),
    }
}

fn effect_channel_supported(id: EffectId, channel: &str) -> bool {
    match id {
        EffectId::PackagePayloadInstall => matches!(
            channel,
            "ubuntu_deb"
                | "fedora_rpm"
                | "linux_appimage"
                | "windows_per_user_setup"
                | "windows_portable_zip"
                | "existing_installation"
                | "existing_installation_maintenance"
        ),
        EffectId::NativeDebPackageTransaction => matches!(
            channel,
            "ubuntu_deb" | "existing_installation" | "existing_installation_maintenance"
        ),
        EffectId::NativeRpmPackageTransaction => matches!(
            channel,
            "fedora_rpm" | "existing_installation" | "existing_installation_maintenance"
        ),
        EffectId::AppImageCurrentUserIntegration => matches!(
            channel,
            "linux_appimage" | "existing_installation" | "existing_installation_maintenance"
        ),
        EffectId::WindowsPerUserSetup => matches!(
            channel,
            "windows_per_user_setup"
                | "existing_installation"
                | "existing_installation_maintenance"
        ),
        EffectId::BoundedLinuxHostAction => channel == "managed_linux_host_action",
    }
}

fn scope_supported_by_channel(channel: &str, scope: TargetScope) -> bool {
    match channel {
        "ubuntu_deb" | "fedora_rpm" => scope == TargetScope::NativePackage,
        "linux_appimage" | "windows_portable_zip" => scope == TargetScope::PortableUser,
        "windows_per_user_setup" => scope == TargetScope::CurrentUser,
        "managed_linux_host_action" => scope == TargetScope::System,
        "existing_installation" | "existing_installation_maintenance" => matches!(
            scope,
            TargetScope::CurrentUser | TargetScope::NativePackage | TargetScope::PortableUser
        ),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionGateTicket {
    generation: u64,
}

/// Ephemeral state machine, independent from the Wizard's page controller.
/// It stores no authority across process restart and exposes no execute method.
pub struct ExecutionGateStateMachine {
    generation: u64,
    state: ExecutionGateStatus,
    fingerprint: Option<PlanReviewFingerprint>,
}

impl ExecutionGateStateMachine {
    pub fn new() -> Self {
        Self {
            generation: 0,
            state: ExecutionGateStatus::AwaitingVerifiedPlan,
            fingerprint: None,
        }
    }

    pub const fn state(&self) -> ExecutionGateStatus {
        self.state
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn observe(&mut self, report: &ExecutionReadinessReport) {
        if self.state == ExecutionGateStatus::Cancelled {
            return;
        }
        if self.state == report.status && self.fingerprint == report.fingerprint {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.state = report.status;
        self.fingerprint = report.fingerprint;
    }

    pub fn acknowledge_review(
        &mut self,
        snapshot: &PlanReviewSnapshot,
        confirmation: &mut ConfirmationSession,
        now: u64,
    ) -> bool {
        if self.state != ExecutionGateStatus::AwaitingReview
            || self.fingerprint != Some(snapshot.fingerprint())
            || !confirmation.acknowledgement_matches_snapshot(snapshot, now)
        {
            return false;
        }
        self.state = ExecutionGateStatus::ReviewAcknowledged;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    pub fn begin_revalidation(&mut self) -> Option<ExecutionGateTicket> {
        if self.state == ExecutionGateStatus::Cancelled {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.state = ExecutionGateStatus::RevalidationRequired;
        Some(ExecutionGateTicket {
            generation: self.generation,
        })
    }

    pub fn accept_revalidation(
        &mut self,
        ticket: ExecutionGateTicket,
        report: &ExecutionReadinessReport,
    ) -> bool {
        if ticket.generation != self.generation
            || self.state != ExecutionGateStatus::RevalidationRequired
        {
            return false;
        }
        if self.fingerprint != report.fingerprint {
            self.invalidate();
            return false;
        }
        self.state = report.status;
        true
    }

    pub fn invalidate(&mut self) {
        self.fingerprint = None;
        self.state = ExecutionGateStatus::Invalidated;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn cancel(&mut self) {
        self.fingerprint = None;
        self.state = ExecutionGateStatus::Cancelled;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn restart(&mut self) {
        self.fingerprint = None;
        self.state = ExecutionGateStatus::AwaitingVerifiedPlan;
        self.generation = self.generation.wrapping_add(1);
    }
}

impl Default for ExecutionGateStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluates already-inspected journal evidence. It never opens a journal or
/// touches the filesystem. Use `JournalEvidence::NotInspected` when the only
/// available API would create directories, locks, or checkpoints.
pub fn classify_recovery_readiness(
    plan: &InstallationPlan,
    evidence: JournalEvidence<'_>,
    recovery_policy_qualified: bool,
) -> RecoveryReadiness {
    let inspection = match evidence {
        JournalEvidence::NotInspected => return RecoveryReadiness::JournalUnavailable,
        JournalEvidence::Busy => return RecoveryReadiness::JournalBusy,
        JournalEvidence::Corrupt => return RecoveryReadiness::JournalCorrupt,
        JournalEvidence::Records(records) => {
            match crate::journal::inspect_records_read_only(plan, records) {
                Ok(summary) => summary,
                Err(JournalErrorCode::JournalPlanMismatch) => {
                    return RecoveryReadiness::PlanMismatch;
                }
                Err(JournalErrorCode::JournalBusy) => return RecoveryReadiness::JournalBusy,
                Err(JournalErrorCode::JournalMissing) => {
                    return RecoveryReadiness::JournalUnavailable;
                }
                Err(_) => return RecoveryReadiness::JournalCorrupt,
            }
        }
    };
    match inspection.disposition {
        RecoveryDisposition::StillUnknown => return RecoveryReadiness::UnknownPriorOutcome,
        RecoveryDisposition::ReplanRequired => return RecoveryReadiness::ReplanRequired,
        RecoveryDisposition::InspectionRequired => return RecoveryReadiness::InspectionRequired,
        RecoveryDisposition::JournalBusy => return RecoveryReadiness::JournalBusy,
        RecoveryDisposition::JournalCorrupt => return RecoveryReadiness::JournalCorrupt,
        RecoveryDisposition::Fresh
        | RecoveryDisposition::ResumeReady
        | RecoveryDisposition::ReconciledApplied
        | RecoveryDisposition::AlreadyCompleted => {}
    }
    if inspection.compensation_unavailable {
        return RecoveryReadiness::CompensationUnavailable;
    }
    if inspection.final_verification_missing {
        return RecoveryReadiness::FinalVerificationUnavailable;
    }
    if recovery_policy_qualified {
        RecoveryReadiness::Ready
    } else {
        RecoveryReadiness::RecoveryPolicyPending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArtifactIdentity, ArtifactVerificationContext, AuthenticatedManifestArtifact,
        ENGINE_SCHEMA_VERSION, InstallationEffect, InstallationIntent, JournalRecord,
        LastUseVerificationRequest, PROTECTED_RESOURCES, PlanReviewContext, Precondition,
        VerificationContract, VerifiedArtifactEvidence,
    };
    #[cfg(target_os = "linux")]
    use sha2::Digest;
    use std::collections::BTreeMap;
    #[cfg(target_os = "linux")]
    use std::fs;

    fn fixture() -> (InstallationRequest, InstallationPlan, PlanReviewContext) {
        let artifact = ArtifactEvidence::VerifiedReleaseArtifact {
            evidence: VerifiedArtifactEvidence {
                artifact_id: "fixture-appimage".into(),
                artifact_type: "appimage".into(),
                product_version: "1.0.0".into(),
                source_commit: "a".repeat(40),
                artifact_sha256: "b".repeat(64),
                artifact_size_bytes: 1024,
                manifest_sha256: "c".repeat(64),
                authentication_method: "synthetic-test-only".into(),
                verified_local_path: "/fixture/AppImage".into(),
            },
        };
        let request = InstallationRequest {
            intent: InstallationIntent::Install,
            target_scope: TargetScope::PortableUser,
            verified_artifact: Some(artifact.clone()),
            user_choices: BTreeMap::new(),
        };
        let effect = InstallationEffect {
            effect_id: "appimage.current-user.integration".into(),
            phase: EffectPhase::Integrate,
            owner: ResourceClass::PlatformIntegrationOwned,
            resource_class: ResourceClass::PlatformIntegrationOwned,
            authority: EffectAuthority::PlatformIntegrationAdapter,
            target_scope: TargetScope::PortableUser,
            preconditions: vec![Precondition {
                kind: "owned_surface_available".into(),
                expected: "true".into(),
            }],
            privilege: PrivilegeRequirement::CurrentUser,
            mutation_kind: "integrate_current_user".into(),
            verification: Some(VerificationContract {
                verification_id: "verify.integration".into(),
                expected: "owned integration present".into(),
            }),
            retry_policy: RetryPolicy::Never,
            reconciliation_policy: ReconciliationPolicy::InspectByEffectId,
            reversibility: Reversibility::Reversible,
            safe_inverse: Some("remove_owned_integration".into()),
            preservation_constraints: PROTECTED_RESOURCES.to_vec(),
            artifact_identity: Some("fixture-appimage".into()),
        };
        let plan = InstallationPlan {
            schema_version: ENGINE_SCHEMA_VERSION,
            plan_id: "fixture-plan".into(),
            intent: InstallationIntent::Install,
            target_scope: TargetScope::PortableUser,
            ordered_effects: vec![effect],
            preservation_set: PROTECTED_RESOURCES.to_vec(),
            expected_completion_verification: VerificationContract {
                verification_id: "verify.complete".into(),
                expected: "payload and integration present".into(),
            },
            artifact: Some(artifact),
        };
        let context = PlanReviewContext {
            device_role: "host".into(),
            connection_mode: "local_only".into(),
            platform: "ubuntu-24.04".into(),
            architecture: "x86_64".into(),
            channel: "linux_appimage".into(),
            capability_generation: 8,
            manifest_generation: Some(3),
            installation_state_generation: 5,
            platform_policy_id: "fixture-policy".into(),
            effect_catalog_version: Some(1),
            security_policy_version: Some(1),
            required_policy_ids: vec!["P004-D02".into(), "P004-D11".into(), "P004-D14".into()],
            preflight_findings: vec![],
        };
        (request, plan, context)
    }

    fn catalog_entry(plan: &InstallationPlan) -> EffectCatalogEntry {
        let effect = &plan.ordered_effects[0];
        EffectCatalogEntry {
            id: EffectId::AppImageCurrentUserIntegration,
            channel: "linux_appimage".into(),
            adapter: EffectAdapter::LinuxAppImageIntegration,
            resource_owner: effect.owner,
            resource_class: effect.resource_class,
            target_scope: effect.target_scope,
            phase: effect.phase,
            preconditions: effect.preconditions.clone(),
            privilege: effect.privilege,
            mutation_kind: effect.mutation_kind.clone(),
            verification: effect.verification.clone().unwrap(),
            reconciliation: effect.reconciliation_policy,
            retry: effect.retry_policy,
            reversibility: effect.reversibility,
            safe_inverse: effect.safe_inverse.clone(),
            preservation_constraints: effect.preservation_constraints.clone(),
        }
    }

    #[cfg(target_os = "linux")]
    fn verified_last_use_fixture(
        request: &mut InstallationRequest,
        plan: &mut InstallationPlan,
        context: &PlanReviewContext,
        review: &PlanReviewSnapshot,
        bytes: &[u8],
    ) -> (tempfile::TempDir, LastUseVerificationResult) {
        use std::os::unix::fs::PermissionsExt;

        let digest = format!("{:x}", sha2::Sha256::digest(bytes));
        let (artifact_id, artifact_type, product_version, source_commit, manifest_identity) =
            match &mut plan.artifact {
                Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) => {
                    evidence.artifact_sha256 = digest.clone();
                    evidence.artifact_size_bytes = bytes.len() as u64;
                    (
                        evidence.artifact_id.clone(),
                        evidence.artifact_type.clone(),
                        evidence.product_version.clone(),
                        evidence.source_commit.clone(),
                        evidence.manifest_sha256.clone(),
                    )
                }
                _ => panic!("fixture has release artifact evidence"),
            };
        request.verified_artifact = plan.artifact.clone();

        let identity =
            ArtifactIdentity::from_authenticated_manifest(AuthenticatedManifestArtifact {
                artifact_id,
                artifact_type,
                product_version,
                source_commit,
                expected_sha256: Some(digest),
                expected_size_bytes: Some(bytes.len() as u64),
                manifest_identity,
                manifest_generation: context.manifest_generation,
                trust_policy_identity: "synthetic-test-policy-only".into(),
                source_provenance_reference: "release-artifact:fixture-appimage".into(),
            })
            .expect("test-only expected evidence");
        let verification_context = ArtifactVerificationContext::from_current_review(
            context.platform.clone(),
            context.architecture.clone(),
            context.channel.clone(),
            review.fingerprint().as_hex(),
            context.manifest_generation,
            context.capability_generation,
            context.installation_state_generation,
            context.effect_catalog_version,
            context.security_policy_version,
        )
        .expect("exact review context");
        let directory = tempfile::tempdir().expect("isolated temporary directory");
        let path = directory.path().join("artifact.bin");
        fs::write(&path, bytes).expect("synthetic bytes");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private fixture");
        let verification_request = LastUseVerificationRequest::from_authenticated_source(
            identity,
            verification_context,
            path,
        );
        (directory, crate::verify_last_use(verification_request))
    }

    #[test]
    fn missing_plan_and_synthetic_artifact_fail_closed() {
        let report = evaluate_execution_readiness(ExecutionGateInput::unavailable());
        assert_eq!(report.status, ExecutionGateStatus::AwaitingVerifiedPlan);
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::VerifiedPlanUnavailable)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ProductionExecutionCapabilityDisabled)
        );

        let (request, plan, context) = fixture();
        let review =
            PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone()).unwrap();
        let report = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: None,
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::RecoveryPolicyPending,
            now: 1,
        });
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::SyntheticArtifactRejected)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ArtifactProvenanceUnavailable)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ReviewAcknowledgementRequired)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ApprovedEffectCatalogUnavailable)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::LastUseVerificationUnavailable)
        );
        assert_ne!(
            report.status,
            ExecutionGateStatus::EligibleForFutureExecution
        );
    }

    #[test]
    fn zero_or_missing_authenticated_artifact_size_invalidates_plan_validation() {
        let (mut request, mut plan, _) = fixture();
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = &mut plan.artifact {
            evidence.artifact_size_bytes = 0;
        }
        request.verified_artifact = plan.artifact.clone();
        assert_eq!(
            InstallerEngine.validate(&request, &plan),
            Err(crate::EngineErrorCode::InvalidPlan)
        );
    }

    #[test]
    fn catalog_rejects_unknown_effect_owner_mismatch_and_scope_mismatch() {
        let (request, plan, context) = fixture();
        let catalog = catalog_entry(&plan);
        assert!(
            validate_effect_catalog(&plan, "linux_appimage", std::slice::from_ref(&catalog))
                .is_empty()
        );
        let mut existing_installation_catalog = catalog.clone();
        existing_installation_catalog.channel = "existing_installation".into();
        assert!(
            !validate_effect_catalog(
                &plan,
                "existing_installation",
                &[existing_installation_catalog]
            )
            .contains(&EffectCatalogViolation::UnsupportedScope)
        );
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("fixture review snapshot");
        let report = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: None,
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::RecoveryPolicyPending,
            now: 1,
        });
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ApprovedEffectCatalogUnavailable)
        );

        let mut unknown = plan.clone();
        unknown.ordered_effects[0].effect_id = "run.arbitrary.command".into();
        assert!(
            validate_effect_catalog(&unknown, "linux_appimage", std::slice::from_ref(&catalog))
                .contains(&EffectCatalogViolation::UnknownEffectId)
        );

        let mut wrong_owner = catalog.clone();
        wrong_owner.adapter = EffectAdapter::NativePackageManager;
        assert!(
            validate_effect_catalog(&plan, "linux_appimage", &[wrong_owner])
                .contains(&EffectCatalogViolation::AdapterMismatch)
        );

        let mut package_manager_id_with_appimage_owner = plan.clone();
        package_manager_id_with_appimage_owner.ordered_effects[0].effect_id =
            EffectId::NativeDebPackageTransaction.as_str().into();
        let mut appimage_catalog_for_package_manager = catalog.clone();
        appimage_catalog_for_package_manager.id = EffectId::NativeDebPackageTransaction;
        let violations = validate_effect_catalog(
            &package_manager_id_with_appimage_owner,
            "linux_appimage",
            &[appimage_catalog_for_package_manager],
        );
        assert!(violations.contains(&EffectCatalogViolation::AdapterMismatch));
        assert!(violations.contains(&EffectCatalogViolation::ChannelMismatch));

        let mut windows_plan = plan.clone();
        windows_plan.ordered_effects[0].effect_id = EffectId::WindowsPerUserSetup.as_str().into();
        windows_plan.ordered_effects[0].owner = ResourceClass::PackageOwned;
        windows_plan.ordered_effects[0].resource_class = ResourceClass::PackageOwned;
        windows_plan.ordered_effects[0].authority = EffectAuthority::InstallerAdapter;
        windows_plan.ordered_effects[0].target_scope = TargetScope::CurrentUser;
        let mut windows_catalog = catalog_entry(&windows_plan);
        windows_catalog.id = EffectId::WindowsPerUserSetup;
        windows_catalog.channel = "windows_per_user_setup".into();
        windows_catalog.adapter = EffectAdapter::WindowsPerUserSetup;
        assert!(
            validate_effect_catalog(&windows_plan, "windows_per_user_setup", &[windows_catalog])
                .is_empty()
        );

        let mut wrong_scope = catalog;
        wrong_scope.target_scope = TargetScope::System;
        let violations = validate_effect_catalog(&plan, "linux_appimage", &[wrong_scope]);
        assert!(violations.contains(&EffectCatalogViolation::UnsupportedScope));
        assert!(violations.contains(&EffectCatalogViolation::ScopeMismatch));

        let mut owner_mismatch = plan.clone();
        owner_mismatch.ordered_effects[0].owner = ResourceClass::PackageOwned;
        owner_mismatch.ordered_effects[0].resource_class = ResourceClass::PackageOwned;
        owner_mismatch.ordered_effects[0].authority = EffectAuthority::InstallerAdapter;
        let review =
            PlanReviewSnapshot::validate_and_create(&request, &owner_mismatch, context.clone())
                .expect("engine-valid plan can still violate closed owner mapping");
        let report = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&owner_mismatch),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: None,
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::RecoveryPolicyPending,
            now: 1,
        });
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::EffectOwnerMismatch)
        );
    }

    #[test]
    fn valid_review_acknowledgement_never_becomes_execution_authority() {
        let (request, plan, context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic fixture can exercise review contracts only");
        let mut acknowledgement = ConfirmationSession::new(&review);
        assert!(acknowledgement.acknowledge(&review, 10, 60).unwrap());
        let report = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: Some(&mut acknowledgement),
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::Ready,
            now: 11,
        });
        assert_eq!(
            report.acknowledgement_status,
            ConfirmationState::AcknowledgedLocally
        );
        assert!(report.plan_freshness_verified);
        assert_eq!(
            report.authorization_requirements[0].review_fingerprint_sha256,
            review.fingerprint().as_hex()
        );
        assert_eq!(
            report.authorization_requirements[0].category,
            AuthorizationRequirementCategory::CurrentUserOperation
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::SyntheticArtifactRejected)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::NativeAuthorizationUnavailable)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ApprovedEffectCatalogUnavailable)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ProductionExecutionCapabilityDisabled)
        );
        assert!(
            !report
                .blockers
                .contains(&ExecutionBlocker::RecoveryPolicyPending)
        );
        assert_ne!(
            report.status,
            ExecutionGateStatus::EligibleForFutureExecution
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn typed_last_use_and_adapter_evidence_reach_gate_but_never_enable_production() {
        let (mut request, mut plan, context) = fixture();
        let bytes = b"synthetic bytes for a non-production verification fixture";
        let expected_digest = format!("{:x}", sha2::Sha256::digest(bytes));
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = &mut plan.artifact {
            evidence.artifact_sha256 = expected_digest;
            evidence.artifact_size_bytes = bytes.len() as u64;
        }
        request.verified_artifact = plan.artifact.clone();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let (directory, last_use) =
            verified_last_use_fixture(&mut request, &mut plan, &context, &review, bytes);
        let adapter = crate::validate_native_adapter_capability(
            &request,
            &plan,
            &review,
            &context,
            &[catalog_entry(&plan)],
        );
        let mut acknowledgement = ConfirmationSession::new(&review);
        assert!(acknowledgement.acknowledge(&review, 10, 60).unwrap());
        let report = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: Some(&mut acknowledgement),
            last_use_verification: Some(&last_use),
            adapter_validation: Some(&adapter),
            recovery_readiness: RecoveryReadiness::Ready,
            now: 11,
        });
        assert_eq!(report.byte_integrity_status, ByteIntegrityStatus::Verified);
        assert_eq!(
            report.last_use_verification_status,
            LastUseVerificationStatus::ByteIntegrityVerified
        );
        assert_eq!(
            report.publisher_trust_status,
            PublisherTrustStatus::Unavailable
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::PublisherTrustPolicyUnavailable)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::UseTimeIdentityNotGuaranteed)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ProductionExecutionCapabilityDisabled)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::ApprovedEffectCatalogUnavailable)
        );
        assert_ne!(
            report.status,
            ExecutionGateStatus::EligibleForFutureExecution
        );
        assert!(report.adapter_validation.is_some());
        drop(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn last_use_and_adapter_bindings_stale_on_plan_and_generation_changes() {
        let (mut request, mut plan, context) = fixture();
        let bytes = b"context binding fixture";
        let digest = format!("{:x}", sha2::Sha256::digest(bytes));
        if let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = &mut plan.artifact {
            evidence.artifact_sha256 = digest;
            evidence.artifact_size_bytes = bytes.len() as u64;
        }
        request.verified_artifact = plan.artifact.clone();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let (directory, last_use) =
            verified_last_use_fixture(&mut request, &mut plan, &context, &review, bytes);
        let adapter = crate::validate_native_adapter_capability(
            &request,
            &plan,
            &review,
            &context,
            &[catalog_entry(&plan)],
        );

        for (field, changed_context) in [
            (
                "manifest",
                PlanReviewContext {
                    manifest_generation: Some(4),
                    ..context.clone()
                },
            ),
            (
                "capability",
                PlanReviewContext {
                    capability_generation: 9,
                    ..context.clone()
                },
            ),
            (
                "installation",
                PlanReviewContext {
                    installation_state_generation: 6,
                    ..context.clone()
                },
            ),
        ] {
            let report = evaluate_execution_readiness(ExecutionGateInput {
                request: Some(&request),
                plan: Some(&plan),
                review: Some(&review),
                current_review_context: Some(&changed_context),
                acknowledgement: None,
                last_use_verification: Some(&last_use),
                adapter_validation: Some(&adapter),
                recovery_readiness: RecoveryReadiness::Ready,
                now: 1,
            });
            assert!(
                report
                    .blockers
                    .contains(&ExecutionBlocker::LastUseVerificationStale),
                "{field} generation must invalidate byte evidence"
            );
            assert!(
                report
                    .blockers
                    .contains(&ExecutionBlocker::NativeAdapterBindingStale)
            );
        }

        let mut changed_plan = plan.clone();
        changed_plan.plan_id.push_str("-changed");
        let report = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&changed_plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: None,
            last_use_verification: Some(&last_use),
            adapter_validation: Some(&adapter),
            recovery_readiness: RecoveryReadiness::Ready,
            now: 1,
        });
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::LastUseVerificationStale)
        );
        assert!(
            report
                .blockers
                .contains(&ExecutionBlocker::NativeAdapterBindingStale)
        );
        drop(directory);
    }

    #[test]
    fn package_manager_and_windows_scope_requirements_stay_distinct() {
        let (category, availability, actor) = classify_authorization(
            EffectId::NativeDebPackageTransaction,
            "ubuntu_deb",
            "ubuntu-24.04",
            TargetScope::NativePackage,
        );
        assert_eq!(
            category,
            AuthorizationRequirementCategory::NativePackageManagerAuthorization
        );
        assert_eq!(
            availability,
            NativeAuthorizationAvailability::PackageManagerContractOnly
        );
        assert_eq!(actor, AuthorizationActor::NativePackageManager);

        let (category, availability, actor) = classify_authorization(
            EffectId::WindowsPerUserSetup,
            "windows_per_user_setup",
            "windows",
            TargetScope::System,
        );
        assert_eq!(
            category,
            AuthorizationRequirementCategory::ScopedElevatedSystemOperation
        );
        assert_eq!(
            availability,
            NativeAuthorizationAvailability::ScopedSystemUnavailable
        );
        assert_eq!(actor, AuthorizationActor::Unsupported);

        let (category, availability, actor) = classify_authorization(
            EffectId::BoundedLinuxHostAction,
            "managed_linux_host_action",
            "ubuntu-24.04",
            TargetScope::System,
        );
        assert_eq!(
            category,
            AuthorizationRequirementCategory::ScopedElevatedSystemOperation
        );
        assert_eq!(
            availability,
            NativeAuthorizationAvailability::PolicyUnavailable
        );
        assert_eq!(actor, AuthorizationActor::BoundedPlatformHelper);

        let (category, availability, actor) = classify_authorization(
            EffectId::AppImageCurrentUserIntegration,
            "linux_appimage",
            "ubuntu-24.04",
            TargetScope::System,
        );
        assert_eq!(
            category,
            AuthorizationRequirementCategory::UnsupportedAuthorization
        );
        assert_eq!(
            availability,
            NativeAuthorizationAvailability::ScopedSystemUnavailable
        );
        assert_eq!(actor, AuthorizationActor::Unsupported);
    }

    #[test]
    fn authorization_refusal_and_cancellation_never_downgrade_scope() {
        assert_eq!(
            classify_authorization_outcome(NativeAuthorizationOutcome::Refused, true),
            AuthorizationOutcomeDisposition::DeniedWithoutScopeFallback
        );
        assert_eq!(
            classify_authorization_outcome(NativeAuthorizationOutcome::Cancelled, true),
            AuthorizationOutcomeDisposition::CancelledWithoutScopeFallback
        );
        assert_eq!(
            classify_authorization_outcome(NativeAuthorizationOutcome::Approved, false),
            AuthorizationOutcomeDisposition::RevalidatePlanAndPolicy
        );
        assert_eq!(
            classify_authorization_outcome(NativeAuthorizationOutcome::Approved, true),
            AuthorizationOutcomeDisposition::ExactRequirementStillNeedsIndependentRevalidation
        );
    }

    #[test]
    fn acknowledgement_is_plan_bound_expires_and_changed_context_invalidates() {
        let (request, plan, context) = fixture();
        let review =
            PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone()).unwrap();
        let mut session = ConfirmationSession::new(&review);
        assert!(session.acknowledge(&review, 10, 5).unwrap());
        assert!(session.acknowledgement_matches_snapshot(&review, 12));
        assert!(!session.acknowledgement_matches_snapshot(&review, 15));
        assert_eq!(
            session.state(),
            ConfirmationState::Invalidated(ConfirmationInvalidationReason::ConfirmationExpired)
        );

        let mut changed_context = context.clone();
        changed_context.effect_catalog_version = Some(2);
        assert!(!review.matches_current(&request, &plan, &changed_context));
    }

    #[test]
    fn state_machine_ignores_stale_revalidation_and_cancel_discards_state() {
        let (request, plan, context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic fixture can exercise state transitions");
        let mut confirmation = ConfirmationSession::new(&review);
        let mut machine = ExecutionGateStateMachine::new();
        let awaiting_review = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: None,
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::RecoveryPolicyPending,
            now: 1,
        });
        machine.observe(&awaiting_review);
        assert_eq!(machine.state(), ExecutionGateStatus::AwaitingReview);
        let generation = machine.generation();
        machine.observe(&awaiting_review);
        assert_eq!(machine.generation(), generation);

        assert!(confirmation.acknowledge(&review, 10, 60).unwrap());
        assert!(machine.acknowledge_review(&review, &mut confirmation, 11));
        assert_eq!(machine.state(), ExecutionGateStatus::ReviewAcknowledged);

        let acknowledged = evaluate_execution_readiness(ExecutionGateInput {
            request: Some(&request),
            plan: Some(&plan),
            review: Some(&review),
            current_review_context: Some(&context),
            acknowledgement: Some(&mut confirmation),
            last_use_verification: None,
            adapter_validation: None,
            recovery_readiness: RecoveryReadiness::RecoveryPolicyPending,
            now: 12,
        });
        let old = machine.begin_revalidation().unwrap();
        let current = machine.begin_revalidation().unwrap();
        assert!(!machine.accept_revalidation(old, &acknowledged));
        assert!(machine.accept_revalidation(current, &acknowledged));
        assert_eq!(
            machine.state(),
            ExecutionGateStatus::AuthorizationPolicyPending
        );
        machine.cancel();
        assert_eq!(machine.state(), ExecutionGateStatus::Cancelled);
        assert!(machine.begin_revalidation().is_none());
        assert_eq!(machine.state(), ExecutionGateStatus::Cancelled);
        machine.restart();
        assert_eq!(machine.state(), ExecutionGateStatus::AwaitingVerifiedPlan);
    }

    #[test]
    fn recovery_requires_read_only_evidence_and_never_treats_unknown_as_ready() {
        let (_, plan, _) = fixture();
        assert_eq!(
            classify_recovery_readiness(&plan, JournalEvidence::NotInspected, false),
            RecoveryReadiness::JournalUnavailable
        );
        let opened = JournalRecord::TransactionOpened {
            plan_id: plan.plan_id.clone(),
            intent: plan.intent,
            target_scope: plan.target_scope,
        };
        let mut mismatch = crate::journal::seal_test_records(&plan, [opened.clone()]);
        mismatch[0].plan_fingerprint_sha256 = "wrong".into();
        assert_eq!(
            classify_recovery_readiness(&plan, JournalEvidence::Records(&mismatch), false),
            RecoveryReadiness::PlanMismatch
        );

        let unknown = crate::journal::seal_test_records(
            &plan,
            [
                opened.clone(),
                JournalRecord::EffectMutationStarted {
                    effect_id: plan.ordered_effects[0].effect_id.clone(),
                },
            ],
        );
        assert_eq!(
            classify_recovery_readiness(&plan, JournalEvidence::Records(&unknown), true),
            RecoveryReadiness::UnknownPriorOutcome
        );

        let all_effects_verified = crate::journal::seal_test_records(
            &plan,
            [
                opened.clone(),
                JournalRecord::EffectMutationStarted {
                    effect_id: plan.ordered_effects[0].effect_id.clone(),
                },
                JournalRecord::EffectVerified {
                    effect_id: plan.ordered_effects[0].effect_id.clone(),
                    result: crate::JournalVerifiedResult::VerifiedSuccess,
                },
            ],
        );
        assert_eq!(
            classify_recovery_readiness(
                &plan,
                JournalEvidence::Records(&all_effects_verified),
                true,
            ),
            RecoveryReadiness::FinalVerificationUnavailable
        );

        let mut damaged = all_effects_verified.clone();
        damaged[1].record_sha256.push('0');
        assert_eq!(
            classify_recovery_readiness(&plan, JournalEvidence::Records(&damaged), true),
            RecoveryReadiness::JournalCorrupt
        );

        let mut partial_plan = plan.clone();
        partial_plan.ordered_effects[0].reversibility = Reversibility::Irreversible;
        partial_plan.ordered_effects[0].safe_inverse = None;
        let mut next_effect = partial_plan.ordered_effects[0].clone();
        next_effect.effect_id = "appimage.second.integration".into();
        next_effect.verification.as_mut().unwrap().verification_id = "verify.second".into();
        partial_plan.ordered_effects.push(next_effect);
        let first_effect = partial_plan.ordered_effects[0].effect_id.clone();
        let partial_records = crate::journal::seal_test_records(
            &partial_plan,
            [
                JournalRecord::TransactionOpened {
                    plan_id: partial_plan.plan_id.clone(),
                    intent: partial_plan.intent,
                    target_scope: partial_plan.target_scope,
                },
                JournalRecord::EffectMutationStarted {
                    effect_id: first_effect.clone(),
                },
                JournalRecord::EffectVerified {
                    effect_id: first_effect,
                    result: crate::JournalVerifiedResult::VerifiedSuccess,
                },
            ],
        );
        assert_eq!(
            classify_recovery_readiness(
                &partial_plan,
                JournalEvidence::Records(&partial_records),
                true,
            ),
            RecoveryReadiness::CompensationUnavailable
        );

        let empty = Vec::new();
        assert_eq!(
            classify_recovery_readiness(&plan, JournalEvidence::Records(&empty), false),
            RecoveryReadiness::JournalUnavailable
        );
    }

    #[test]
    fn final_verification_record_types_are_not_success_claims() {
        let (_, plan, _) = fixture();
        let outcome = JournalRecord::EffectOutcome {
            effect_id: plan.ordered_effects[0].effect_id.clone(),
            outcome: crate::EffectResultState::OutcomeUnknown,
        };
        let records = crate::journal::seal_test_records(
            &plan,
            [
                JournalRecord::TransactionOpened {
                    plan_id: plan.plan_id.clone(),
                    intent: plan.intent,
                    target_scope: plan.target_scope,
                },
                JournalRecord::EffectMutationStarted {
                    effect_id: plan.ordered_effects[0].effect_id.clone(),
                },
                outcome,
            ],
        );
        assert_eq!(
            classify_recovery_readiness(&plan, JournalEvidence::Records(&records), true),
            RecoveryReadiness::UnknownPriorOutcome
        );
    }

    #[test]
    fn production_gate_boundary_has_no_effectful_api_or_qml_input() {
        let gate_source = include_str!("execution_gate.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production execution gate source exists");
        for forbidden in [
            "InstallerEngine::execute(",
            "execute_journaled(",
            "execute_journaled_with_options(",
            ".apply_effect(",
            "InstallationJournal::open(",
            "Command::new(",
            "apt-get ",
            "dnf ",
            "msiexec ",
            "pkexec ",
            "sudo ",
            "runas",
            "systemctl ",
            "TcpListener::bind(",
            ".listen(",
            "Tailscale::enroll",
            "is_root",
            "is_admin",
            "systemd::",
            "firewall",
        ] {
            assert!(!gate_source.contains(forbidden), "found {forbidden}");
        }
        assert!(gate_source.contains("No production catalog is approved"));
        assert!(gate_source.contains("ProductionExecutionCapabilityDisabled"));

        let wizard_qml = include_str!("../../desktop/qml/InstallerWizard.qml");
        assert!(!wizard_qml.contains("effectId"));
        assert!(!wizard_qml.contains("commandString"));
        assert!(!wizard_qml.contains("qsTr(\"Install\")"));
        assert!(wizard_qml.contains("Preview only — installation is not started"));
    }
}
