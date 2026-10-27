//! Fail-closed boundary between read-only acquisition observations and plans.
//!
//! Release metadata and digest observations are deliberately not proof values:
//! callers cannot turn a locally computed checksum or a claimed authentication
//! state into `VerifiedReleaseArtifact`. Production plan construction remains
//! unavailable until an independently provisioned trust policy and an approved
//! channel-specific effect catalog are connected to this crate.

use crate::{
    ArtifactEvidence, EngineErrorCode, InstallationIntent, InstallationPlan, InstallationRequest,
    InstallerEngine, PlanReviewContext, PlanReviewSnapshot, TargetScope,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallationPlatform {
    Ubuntu2404,
    Fedora42,
    Windows11,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuArchitecture {
    X86_64,
    Aarch64,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallationChannel {
    UbuntuDeb,
    FedoraRpm,
    LinuxAppImage,
    WindowsPerUserSetup,
    WindowsPortableZip,
    ExistingInstallation,
    Unknown,
}

/// Observation categories mirror the release trust pipeline. They describe
/// evidence that a read-only source reported; they are not authentication
/// capabilities and cannot promote an artifact to verified provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactSourceEvidence {
    AuthenticatedReleaseChannelMetadataObserved,
    AuthenticatedReleaseManifestObserved,
    ArtifactBytesMatchAuthenticatedMetadataObserved,
    NativePackageManagerManagedIdentityObserved,
    LocallySuppliedUntrustedArtifactObserved,
    UnknownOrMissingArtifact,
    InvalidRelease,
    StaleRelease,
    RevokedRelease,
    UnsupportedRelease,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanBuildBlocker {
    ArtifactMissing,
    UntrustedLocalArtifact,
    AuthenticationEvidenceIncomplete,
    InvalidStaleRevokedOrUnsupportedRelease,
    AmbiguousArtifactSelection,
    UnsupportedPlatformOrArchitecture,
    PlatformChannelMismatch,
    UnsupportedTargetScope,
    TrustRootUnavailable,
    ApprovedEffectCatalogUnavailable,
    PolicyDecisionPending,
}

impl PlanBuildBlocker {
    /// Stable safe presentation code. Never includes local paths or diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::ArtifactMissing => "artifact_missing",
            Self::UntrustedLocalArtifact => "untrusted_local_artifact",
            Self::AuthenticationEvidenceIncomplete => "authentication_evidence_incomplete",
            Self::InvalidStaleRevokedOrUnsupportedRelease => "release_invalid_or_stale",
            Self::AmbiguousArtifactSelection => "artifact_selection_ambiguous",
            Self::UnsupportedPlatformOrArchitecture => "platform_or_architecture_unsupported",
            Self::PlatformChannelMismatch => "platform_channel_mismatch",
            Self::UnsupportedTargetScope => "target_scope_unsupported",
            Self::TrustRootUnavailable => "production_trust_root_unavailable",
            Self::ApprovedEffectCatalogUnavailable => "approved_effect_catalog_unavailable",
            Self::PolicyDecisionPending => "installer_policy_decision_pending",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ArtifactMissing => "no installation artifact is available",
            Self::UntrustedLocalArtifact => "locally supplied artifact is untrusted",
            Self::AuthenticationEvidenceIncomplete => {
                "publisher authentication and exact artifact identity are incomplete"
            }
            Self::InvalidStaleRevokedOrUnsupportedRelease => {
                "release is invalid, stale, revoked, or unsupported"
            }
            Self::AmbiguousArtifactSelection => "artifact selection is ambiguous",
            Self::UnsupportedPlatformOrArchitecture => "platform or architecture is unsupported",
            Self::PlatformChannelMismatch => "installation channel does not match the platform",
            Self::UnsupportedTargetScope => "target scope is not approved for this channel",
            Self::TrustRootUnavailable => "production publisher trust root is unavailable",
            Self::ApprovedEffectCatalogUnavailable => {
                "approved channel effect catalog is unavailable"
            }
            Self::PolicyDecisionPending => "P004 installer policy decisions remain pending",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanBuildRequest {
    pub platform: InstallationPlatform,
    pub architecture: CpuArchitecture,
    pub channel: InstallationChannel,
    pub target_scope: TargetScope,
    pub intent: InstallationIntent,
    pub capability_generation: u64,
    pub artifact_candidates: Vec<ArtifactSourceEvidence>,
}

/// Result of the read-only boundary. No input type accepts installation
/// effects, commands, artifact paths, publisher strings, or trust roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanBuildResult {
    Blocked(Vec<PlanBuildBlocker>),
}

/// The only plan-to-P009 handoff exposed inside the crate. Desktop/QML callers
/// cannot construct a snapshot. A future platform-owned builder may call this
/// only after its independent provenance and effect-policy gates succeed.
#[allow(dead_code)]
pub(crate) fn validate_plan_for_review(
    request: &InstallationRequest,
    plan: &InstallationPlan,
    context: PlanReviewContext,
) -> Result<PlanReviewSnapshot, EngineErrorCode> {
    if !matches!(
        &plan.artifact,
        Some(ArtifactEvidence::VerifiedReleaseArtifact { .. })
    ) {
        return Err(EngineErrorCode::InvalidPlan);
    }
    InstallerEngine.validate(request, plan)?;
    PlanReviewSnapshot::validate_and_create(request, plan, context)
}

fn blocked(blocker: PlanBuildBlocker) -> PlanBuildResult {
    let mut blockers = vec![blocker];
    for gate in [
        PlanBuildBlocker::TrustRootUnavailable,
        PlanBuildBlocker::ApprovedEffectCatalogUnavailable,
        PlanBuildBlocker::PolicyDecisionPending,
    ] {
        if !blockers.contains(&gate) {
            blockers.push(gate);
        }
    }
    PlanBuildResult::Blocked(blockers)
}

/// Inspect current inputs and return a bounded blocker until the production
/// trust root, complete verified artifact identity, and approved effect
/// catalog are available. No plan, acquisition, subprocess, or mutation is
/// produced by this function.
pub fn build_installation_plan_preview(request: &PlanBuildRequest) -> PlanBuildResult {
    if request.artifact_candidates.len() > 1 {
        return blocked(PlanBuildBlocker::AmbiguousArtifactSelection);
    }
    match request.artifact_candidates.first() {
        None | Some(ArtifactSourceEvidence::UnknownOrMissingArtifact) => {
            return blocked(PlanBuildBlocker::ArtifactMissing);
        }
        Some(ArtifactSourceEvidence::LocallySuppliedUntrustedArtifactObserved) => {
            return blocked(PlanBuildBlocker::UntrustedLocalArtifact);
        }
        Some(
            ArtifactSourceEvidence::InvalidRelease
            | ArtifactSourceEvidence::StaleRelease
            | ArtifactSourceEvidence::RevokedRelease
            | ArtifactSourceEvidence::UnsupportedRelease,
        ) => {
            return blocked(PlanBuildBlocker::InvalidStaleRevokedOrUnsupportedRelease);
        }
        Some(ArtifactSourceEvidence::AuthenticatedReleaseChannelMetadataObserved)
        | Some(ArtifactSourceEvidence::AuthenticatedReleaseManifestObserved)
        | Some(ArtifactSourceEvidence::ArtifactBytesMatchAuthenticatedMetadataObserved)
        | Some(ArtifactSourceEvidence::NativePackageManagerManagedIdentityObserved) => {}
    }

    if !supported_platform_channel(request.platform, request.architecture, request.channel) {
        return blocked(
            if request.channel == InstallationChannel::Unknown
                || request.platform == InstallationPlatform::Unknown
                || request.architecture == CpuArchitecture::Unknown
                || matches!(request.platform, InstallationPlatform::Other)
                || matches!(request.architecture, CpuArchitecture::Other)
            {
                PlanBuildBlocker::UnsupportedPlatformOrArchitecture
            } else {
                PlanBuildBlocker::PlatformChannelMismatch
            },
        );
    }

    if !supported_scope(request.channel, request.target_scope) {
        return blocked(PlanBuildBlocker::UnsupportedTargetScope);
    }

    // The standalone release verifier currently receives its production public
    // policy from an external authenticated CI input. No approved runtime source
    // for this desktop boundary is available in the repository.
    let evidence_blocker = match request.artifact_candidates.first() {
        Some(ArtifactSourceEvidence::AuthenticatedReleaseChannelMetadataObserved)
        | Some(ArtifactSourceEvidence::AuthenticatedReleaseManifestObserved) => {
            PlanBuildBlocker::TrustRootUnavailable
        }
        Some(ArtifactSourceEvidence::ArtifactBytesMatchAuthenticatedMetadataObserved) => {
            PlanBuildBlocker::AuthenticationEvidenceIncomplete
        }
        Some(ArtifactSourceEvidence::NativePackageManagerManagedIdentityObserved) => {
            PlanBuildBlocker::AuthenticationEvidenceIncomplete
        }
        _ => PlanBuildBlocker::ArtifactMissing,
    };
    blocked(evidence_blocker)
}

const fn supported_scope(channel: InstallationChannel, scope: TargetScope) -> bool {
    matches!(
        (channel, scope),
        (
            InstallationChannel::UbuntuDeb | InstallationChannel::FedoraRpm,
            TargetScope::NativePackage,
        ) | (
            InstallationChannel::LinuxAppImage,
            TargetScope::PortableUser
        ) | (
            InstallationChannel::WindowsPerUserSetup,
            TargetScope::CurrentUser
        ) | (
            InstallationChannel::WindowsPortableZip,
            TargetScope::PortableUser
        ) | (InstallationChannel::ExistingInstallation, _)
    )
}

const fn supported_platform_channel(
    platform: InstallationPlatform,
    architecture: CpuArchitecture,
    channel: InstallationChannel,
) -> bool {
    matches!(
        (platform, architecture, channel),
        (
            InstallationPlatform::Ubuntu2404,
            CpuArchitecture::X86_64,
            InstallationChannel::UbuntuDeb,
        ) | (
            InstallationPlatform::Fedora42,
            CpuArchitecture::X86_64,
            InstallationChannel::FedoraRpm,
        ) | (
            InstallationPlatform::Ubuntu2404,
            CpuArchitecture::X86_64,
            InstallationChannel::LinuxAppImage,
        ) | (
            InstallationPlatform::Fedora42,
            CpuArchitecture::X86_64,
            InstallationChannel::LinuxAppImage,
        ) | (
            InstallationPlatform::Windows11,
            CpuArchitecture::X86_64,
            InstallationChannel::WindowsPerUserSetup,
        ) | (
            InstallationPlatform::Windows11,
            CpuArchitecture::X86_64,
            InstallationChannel::WindowsPortableZip,
        ) | (
            InstallationPlatform::Ubuntu2404 | InstallationPlatform::Fedora42,
            CpuArchitecture::X86_64,
            InstallationChannel::ExistingInstallation,
        ) | (
            InstallationPlatform::Windows11,
            CpuArchitecture::X86_64,
            InstallationChannel::ExistingInstallation,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArtifactEvidence, EffectAuthority, EffectPhase, InstallationEffect, InstallationPlan,
        InstallationRequest, InstallerEngine, Precondition, PrivilegeRequirement,
        ReconciliationPolicy, ResourceClass, RetryPolicy, Reversibility, VerificationContract,
    };

    fn request(channel: InstallationChannel) -> PlanBuildRequest {
        PlanBuildRequest {
            platform: InstallationPlatform::Ubuntu2404,
            architecture: CpuArchitecture::X86_64,
            channel,
            target_scope: TargetScope::NativePackage,
            intent: InstallationIntent::Install,
            capability_generation: 4,
            artifact_candidates: vec![ArtifactSourceEvidence::AuthenticatedReleaseManifestObserved],
        }
    }

    #[test]
    fn production_boundary_never_promotes_observations_to_verified_artifacts() {
        for evidence in [
            ArtifactSourceEvidence::AuthenticatedReleaseChannelMetadataObserved,
            ArtifactSourceEvidence::AuthenticatedReleaseManifestObserved,
            ArtifactSourceEvidence::ArtifactBytesMatchAuthenticatedMetadataObserved,
            ArtifactSourceEvidence::NativePackageManagerManagedIdentityObserved,
        ] {
            let mut input = request(InstallationChannel::UbuntuDeb);
            input.artifact_candidates = vec![evidence];
            let PlanBuildResult::Blocked(blockers) = build_installation_plan_preview(&input);
            assert!(blockers.contains(&PlanBuildBlocker::TrustRootUnavailable));
            assert!(blockers.contains(&PlanBuildBlocker::ApprovedEffectCatalogUnavailable));
            assert!(blockers.contains(&PlanBuildBlocker::PolicyDecisionPending));
        }
    }

    #[test]
    fn missing_untrusted_invalid_and_ambiguous_inputs_are_blocked() {
        let mut input = request(InstallationChannel::UbuntuDeb);
        input.artifact_candidates.clear();
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::ArtifactMissing)
        );
        input.artifact_candidates =
            vec![ArtifactSourceEvidence::LocallySuppliedUntrustedArtifactObserved];
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::UntrustedLocalArtifact)
        );
        for evidence in [
            ArtifactSourceEvidence::InvalidRelease,
            ArtifactSourceEvidence::StaleRelease,
            ArtifactSourceEvidence::RevokedRelease,
            ArtifactSourceEvidence::UnsupportedRelease,
        ] {
            input.artifact_candidates = vec![evidence];
            assert_eq!(
                build_installation_plan_preview(&input),
                blocked(PlanBuildBlocker::InvalidStaleRevokedOrUnsupportedRelease)
            );
        }
        input.artifact_candidates =
            vec![ArtifactSourceEvidence::AuthenticatedReleaseManifestObserved; 2];
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::AmbiguousArtifactSelection)
        );
    }

    #[test]
    fn wrong_platform_and_architecture_are_blocked_before_trust() {
        let mut input = request(InstallationChannel::UbuntuDeb);
        input.platform = InstallationPlatform::Windows11;
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::PlatformChannelMismatch)
        );
        input.platform = InstallationPlatform::Ubuntu2404;
        input.architecture = CpuArchitecture::Aarch64;
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::PlatformChannelMismatch)
        );
    }

    #[test]
    fn synthetic_plan_fixture_is_bounded_and_passes_engine_validation() {
        // This fixture exercises plan shape only. It is local to tests and is
        // never returned by the production builder as artifact trust evidence.
        let evidence = ArtifactEvidence::LocallySuppliedArtifact {
            artifact_id: "synthetic-test-artifact".into(),
            local_path: "<test-only>".into(),
        };
        let request = InstallationRequest {
            intent: InstallationIntent::Install,
            target_scope: TargetScope::NativePackage,
            verified_artifact: Some(evidence.clone()),
            user_choices: Default::default(),
        };
        let preserved = vec![
            ResourceClass::ApplicationConfig,
            ResourceClass::CredentialState,
            ResourceClass::ClientSyncState,
            ResourceClass::UserLibraryContent,
            ResourceClass::ServerConfig,
            ResourceClass::ServerDatabase,
            ResourceClass::ServerObjectData,
            ResourceClass::ExternalDependency,
        ];
        let effect = InstallationEffect {
            effect_id: "synthetic-test-package-preview".into(),
            phase: EffectPhase::Install,
            owner: ResourceClass::PackageOwned,
            resource_class: ResourceClass::PackageOwned,
            authority: EffectAuthority::InstallerAdapter,
            target_scope: TargetScope::NativePackage,
            preconditions: vec![Precondition {
                kind: "synthetic-test-only".into(),
                expected: "present".into(),
            }],
            privilege: PrivilegeRequirement::CurrentUser,
            mutation_kind: "synthetic-test-only".into(),
            verification: Some(VerificationContract {
                verification_id: "synthetic-test-verification".into(),
                expected: "present".into(),
            }),
            retry_policy: RetryPolicy::Never,
            reconciliation_policy: ReconciliationPolicy::NotApplicable,
            reversibility: Reversibility::NotApplicable,
            safe_inverse: None,
            preservation_constraints: preserved.clone(),
            artifact_identity: Some(evidence.identity().to_owned()),
        };
        let plan = InstallationPlan {
            schema_version: crate::ENGINE_SCHEMA_VERSION,
            plan_id: "synthetic-test-plan".into(),
            intent: InstallationIntent::Install,
            target_scope: TargetScope::NativePackage,
            ordered_effects: vec![effect],
            preservation_set: preserved,
            expected_completion_verification: VerificationContract {
                verification_id: "synthetic-test-complete".into(),
                expected: "complete".into(),
            },
            artifact: Some(evidence),
        };
        assert!(InstallerEngine.validate(&request, &plan).is_ok());
        assert!(matches!(
            build_installation_plan_preview(&super::tests::request(InstallationChannel::UbuntuDeb)),
            PlanBuildResult::Blocked(_)
        ));
    }

    #[test]
    fn verified_candidate_uses_the_existing_plan_bound_review_snapshot() {
        use crate::{PlanReviewContext, ReviewFinding, VerifiedArtifactEvidence};

        let evidence = ArtifactEvidence::VerifiedReleaseArtifact {
            evidence: VerifiedArtifactEvidence {
                artifact_id: "synthetic-test-artifact".into(),
                artifact_type: "deb".into(),
                product_version: "1.2.3".into(),
                source_commit: "a".repeat(40),
                artifact_sha256: "b".repeat(64),
                manifest_sha256: "c".repeat(64),
                authentication_method: "synthetic-test-only".into(),
                verified_local_path: "<test-only-private-path>".into(),
            },
        };
        let (mut request, mut plan) = synthetic_install_plan(evidence.clone());
        request.verified_artifact = Some(evidence.clone());
        plan.artifact = Some(evidence.clone());
        plan.ordered_effects[0].artifact_identity = Some(evidence.identity().to_owned());
        let context = PlanReviewContext {
            device_role: "client".into(),
            connection_mode: "manual".into(),
            platform: "ubuntu-24.04".into(),
            architecture: "x86_64".into(),
            channel: "ubuntu_deb".into(),
            capability_generation: 4,
            platform_policy_id: "synthetic-test-policy".into(),
            required_policy_ids: vec!["synthetic-test-policy".into()],
            preflight_findings: vec![ReviewFinding {
                code: "synthetic_test_only".into(),
                severity: crate::ReviewFindingSeverity::Information,
                blocks_review: false,
            }],
        };
        let snapshot = validate_plan_for_review(&request, &plan, context)
            .expect("engine-validated synthetic candidate can enter the P009 contract");
        assert_eq!(
            snapshot.status(),
            crate::PlanReviewStatus::AvailableForReview
        );
        assert!(snapshot.is_unchanged_and_valid());
        assert!(snapshot.fingerprint_summary().starts_with("sha256:"));
    }

    fn synthetic_install_plan(
        evidence: ArtifactEvidence,
    ) -> (InstallationRequest, InstallationPlan) {
        let request = InstallationRequest {
            intent: InstallationIntent::Install,
            target_scope: TargetScope::NativePackage,
            verified_artifact: Some(evidence.clone()),
            user_choices: Default::default(),
        };
        let preservation_set = vec![
            ResourceClass::ApplicationConfig,
            ResourceClass::CredentialState,
            ResourceClass::ClientSyncState,
            ResourceClass::UserLibraryContent,
            ResourceClass::ServerConfig,
            ResourceClass::ServerDatabase,
            ResourceClass::ServerObjectData,
            ResourceClass::ExternalDependency,
        ];
        let effect = InstallationEffect {
            effect_id: "synthetic-test-review-effect".into(),
            phase: EffectPhase::Install,
            owner: ResourceClass::PackageOwned,
            resource_class: ResourceClass::PackageOwned,
            authority: EffectAuthority::InstallerAdapter,
            target_scope: TargetScope::NativePackage,
            preconditions: vec![Precondition {
                kind: "synthetic-test-only".into(),
                expected: "present".into(),
            }],
            privilege: PrivilegeRequirement::CurrentUser,
            mutation_kind: "synthetic-test-only".into(),
            verification: Some(VerificationContract {
                verification_id: "synthetic-test-review-verification".into(),
                expected: "present".into(),
            }),
            retry_policy: RetryPolicy::Never,
            reconciliation_policy: ReconciliationPolicy::NotApplicable,
            reversibility: Reversibility::NotApplicable,
            safe_inverse: None,
            preservation_constraints: preservation_set.clone(),
            artifact_identity: Some(evidence.identity().to_owned()),
        };
        let plan = InstallationPlan {
            schema_version: crate::ENGINE_SCHEMA_VERSION,
            plan_id: "synthetic-test-review-plan".into(),
            intent: InstallationIntent::Install,
            target_scope: TargetScope::NativePackage,
            ordered_effects: vec![effect],
            preservation_set,
            expected_completion_verification: VerificationContract {
                verification_id: "synthetic-test-review-complete".into(),
                expected: "complete".into(),
            },
            artifact: Some(evidence),
        };
        (request, plan)
    }

    #[test]
    fn unsupported_channels_cannot_reach_a_plan() {
        let mut input = request(InstallationChannel::UbuntuDeb);
        input.channel = InstallationChannel::FedoraRpm;
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::PlatformChannelMismatch)
        );
        input.channel = InstallationChannel::LinuxAppImage;
        input.target_scope = TargetScope::PortableUser;
        assert!(matches!(
            build_installation_plan_preview(&input),
            PlanBuildResult::Blocked(_)
        ));
        input.channel = InstallationChannel::WindowsPerUserSetup;
        input.platform = InstallationPlatform::Windows11;
        input.target_scope = TargetScope::CurrentUser;
        assert!(matches!(
            build_installation_plan_preview(&input),
            PlanBuildResult::Blocked(_)
        ));
        input.target_scope = TargetScope::System;
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::UnsupportedTargetScope)
        );
        input.platform = InstallationPlatform::Unknown;
        input.channel = InstallationChannel::ExistingInstallation;
        input.target_scope = TargetScope::CurrentUser;
        assert_eq!(
            build_installation_plan_preview(&input),
            blocked(PlanBuildBlocker::UnsupportedPlatformOrArchitecture)
        );
    }
}
