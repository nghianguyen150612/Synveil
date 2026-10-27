//! Read-only P010 boundary between artifact provenance and platform planning.
//!
//! This module deliberately contains no downloader, process launcher, adapter,
//! journal, or installation effect executor. The P006 Python verifier remains
//! the implementation of release metadata authentication. Until its output is
//! bound to an approved production trust policy and a platform effect catalog,
//! production planning returns a typed blocker.

use crate::{
    ArtifactEvidence, EngineErrorCode, InstallationIntent, InstallationPlan, InstallationRequest,
    InstallerEngine, PlanReviewContext, PlanReviewSnapshot, TargetScope,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactEvidenceKind {
    AuthenticatedReleaseChannelMetadata,
    AuthenticatedReleaseManifest,
    DigestVerifiedArtifactBytes,
    NativePackageManagerManagedIdentity,
    LocallySuppliedUntrustedArtifact,
    UnknownOrMissingArtifact,
    InvalidRelease,
    StaleRelease,
    RevokedRelease,
    UnsupportedRelease,
    UnsupportedAuthenticationScheme,
    UntrustedPublisherKey,
    UnauthorizedDowngrade,
    IncompleteProvenance,
    AmbiguousArtifactSelection,
    ArtifactIdentityMismatch,
    WrongPlatformOrArchitecture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanPlatform {
    Ubuntu2404,
    Fedora42,
    OtherLinux,
    Windows,
    MacOs,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanArchitecture {
    X86_64,
    Aarch64,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanChannel {
    Unknown,
    UbuntuDeb,
    FedoraRpm,
    LinuxAppImage,
    WindowsPerUserSetup,
    WindowsPortableZip,
    ExistingInstallationMaintenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanBuildBlocker {
    InstallationChannelUnavailable,
    PlatformChannelMismatch,
    UnsupportedPlatformOrArchitecture,
    UnsupportedTargetScope,
    UnsupportedIntent,
    ArtifactEvidenceUnavailable,
    ProductionTrustRootUnavailable,
    ApprovedEffectCatalogUnavailable,
    PolicyPrerequisitesPending,
}

impl PlanBuildBlocker {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InstallationChannelUnavailable => "installation channel is not selected",
            Self::PlatformChannelMismatch => "installation channel does not match this platform",
            Self::UnsupportedPlatformOrArchitecture => "platform or architecture is unsupported",
            Self::UnsupportedTargetScope => "target scope is not approved for this channel",
            Self::UnsupportedIntent => "installation intent is unsupported for preview",
            Self::ArtifactEvidenceUnavailable => {
                "independently authenticated artifact evidence is unavailable"
            }
            Self::ProductionTrustRootUnavailable => {
                "production publisher trust root is unavailable"
            }
            Self::ApprovedEffectCatalogUnavailable => {
                "approved channel effect catalog is unavailable"
            }
            Self::PolicyPrerequisitesPending => "P004-D02, D11, and D14 remain pending",
        }
    }
}

/// A read-only observation request. It intentionally has no URL or local path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactSourceRequest {
    pub platform: PlanPlatform,
    pub architecture: PlanArchitecture,
    pub channel: PlanChannel,
    pub target_scope: TargetScope,
    pub intent: InstallationIntent,
    pub capability_generation: u64,
}

/// A source reports evidence class only; this value cannot establish trust.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactSourceObservation {
    pub evidence_kind: ArtifactEvidenceKind,
    pub manifest_generation: Option<u64>,
}

/// Implementations may inspect an already approved source. No production
/// acquisition implementation is supplied by P010 while origins and roots are
/// unapproved.
pub trait ReadOnlyArtifactSource {
    fn observe(&self, request: ArtifactSourceRequest) -> ArtifactSourceObservation;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanBuildOutcome {
    Blocked(Vec<PlanBuildBlocker>),
}

/// Crate-private handoff to P009. A platform-owned builder may call this only
/// after provenance and effect-policy gates pass. It reuses engine validation
/// and the existing snapshot constructor; desktop/QML cannot create snapshots.
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

/// Platform-owned plan boundary. QML and callers cannot supply operations.
#[derive(Default)]
pub struct AuthoritativePlanBuilder;

impl AuthoritativePlanBuilder {
    pub fn build<S: ReadOnlyArtifactSource>(
        &self,
        request: ArtifactSourceRequest,
        source: &S,
    ) -> PlanBuildOutcome {
        let mut blockers = Vec::new();
        if request.channel == PlanChannel::Unknown {
            blockers.push(PlanBuildBlocker::InstallationChannelUnavailable);
        }
        if !channel_matches_platform(request.platform, request.channel) {
            blockers.push(PlanBuildBlocker::PlatformChannelMismatch);
        }
        if !supported_platform_architecture(request) {
            blockers.push(PlanBuildBlocker::UnsupportedPlatformOrArchitecture);
        }
        if !supported_target_scope(request) {
            blockers.push(PlanBuildBlocker::UnsupportedTargetScope);
        }
        if request.intent != InstallationIntent::Install {
            blockers.push(PlanBuildBlocker::UnsupportedIntent);
        }
        // The interface reports one bounded fact, not a complete, cross-bound
        // VerifiedArtifactEvidence record. No individual category is sufficient
        // to bind the authenticated channel, manifest and exact artifact bytes.
        let _observation = source.observe(request);
        blockers.push(PlanBuildBlocker::ArtifactEvidenceUnavailable);

        // No approved production root or closed platform effect catalog is
        // provisioned in this repository state. A matching digest or synthetic
        // source observation must never promote itself into verified evidence.
        blockers.push(PlanBuildBlocker::ProductionTrustRootUnavailable);
        blockers.push(PlanBuildBlocker::ApprovedEffectCatalogUnavailable);
        blockers.push(PlanBuildBlocker::PolicyPrerequisitesPending);
        PlanBuildOutcome::Blocked(blockers)
    }
}

fn supported_target_scope(request: ArtifactSourceRequest) -> bool {
    match request.channel {
        PlanChannel::UbuntuDeb | PlanChannel::FedoraRpm => {
            request.target_scope == TargetScope::NativePackage
        }
        PlanChannel::LinuxAppImage | PlanChannel::WindowsPortableZip => {
            request.target_scope == TargetScope::PortableUser
        }
        PlanChannel::WindowsPerUserSetup => request.target_scope == TargetScope::CurrentUser,
        PlanChannel::ExistingInstallationMaintenance => matches!(
            request.target_scope,
            TargetScope::CurrentUser | TargetScope::NativePackage | TargetScope::PortableUser
        ),
        PlanChannel::Unknown => false,
    }
}

fn channel_matches_platform(platform: PlanPlatform, channel: PlanChannel) -> bool {
    matches!(
        (platform, channel),
        (PlanPlatform::Ubuntu2404, PlanChannel::UbuntuDeb)
            | (PlanPlatform::Fedora42, PlanChannel::FedoraRpm)
            | (
                PlanPlatform::Ubuntu2404 | PlanPlatform::Fedora42 | PlanPlatform::OtherLinux,
                PlanChannel::LinuxAppImage
            )
            | (
                PlanPlatform::Windows,
                PlanChannel::WindowsPerUserSetup | PlanChannel::WindowsPortableZip
            )
            | (
                PlanPlatform::Ubuntu2404 | PlanPlatform::Fedora42 | PlanPlatform::Windows,
                PlanChannel::ExistingInstallationMaintenance
            )
    )
}

fn supported_platform_architecture(request: ArtifactSourceRequest) -> bool {
    match request.channel {
        PlanChannel::Unknown => false,
        PlanChannel::UbuntuDeb => {
            request.platform == PlanPlatform::Ubuntu2404
                && request.architecture == PlanArchitecture::X86_64
        }
        PlanChannel::FedoraRpm => {
            request.platform == PlanPlatform::Fedora42
                && request.architecture == PlanArchitecture::X86_64
        }
        PlanChannel::LinuxAppImage => {
            matches!(
                request.platform,
                PlanPlatform::Ubuntu2404 | PlanPlatform::Fedora42 | PlanPlatform::OtherLinux
            ) && request.architecture == PlanArchitecture::X86_64
        }
        PlanChannel::WindowsPerUserSetup | PlanChannel::WindowsPortableZip => {
            request.platform == PlanPlatform::Windows
                && request.architecture == PlanArchitecture::X86_64
        }
        PlanChannel::ExistingInstallationMaintenance => {
            matches!(
                request.platform,
                PlanPlatform::Ubuntu2404 | PlanPlatform::Fedora42 | PlanPlatform::Windows
            ) && request.architecture == PlanArchitecture::X86_64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::collections::BTreeMap;

    struct SyntheticSource(ArtifactEvidenceKind);

    impl ReadOnlyArtifactSource for SyntheticSource {
        fn observe(&self, _request: ArtifactSourceRequest) -> ArtifactSourceObservation {
            ArtifactSourceObservation {
                evidence_kind: self.0,
                manifest_generation: Some(1),
            }
        }
    }

    fn supported_request(channel: PlanChannel) -> ArtifactSourceRequest {
        let platform = match channel {
            PlanChannel::UbuntuDeb => PlanPlatform::Ubuntu2404,
            PlanChannel::FedoraRpm => PlanPlatform::Fedora42,
            PlanChannel::WindowsPerUserSetup | PlanChannel::WindowsPortableZip => {
                PlanPlatform::Windows
            }
            PlanChannel::LinuxAppImage => PlanPlatform::Ubuntu2404,
            PlanChannel::ExistingInstallationMaintenance => PlanPlatform::Ubuntu2404,
            PlanChannel::Unknown => PlanPlatform::Other,
        };
        ArtifactSourceRequest {
            platform,
            architecture: PlanArchitecture::X86_64,
            channel,
            target_scope: match channel {
                PlanChannel::UbuntuDeb | PlanChannel::FedoraRpm => TargetScope::NativePackage,
                PlanChannel::LinuxAppImage | PlanChannel::WindowsPortableZip => {
                    TargetScope::PortableUser
                }
                PlanChannel::WindowsPerUserSetup => TargetScope::CurrentUser,
                PlanChannel::ExistingInstallationMaintenance => TargetScope::NativePackage,
                PlanChannel::Unknown => TargetScope::CurrentUser,
            },
            intent: InstallationIntent::Install,
            capability_generation: 4,
        }
    }

    fn synthetic_test_plan() -> (InstallationRequest, InstallationPlan) {
        let artifact = ArtifactEvidence::VerifiedReleaseArtifact {
            evidence: VerifiedArtifactEvidence {
                artifact_id: "fixture-ubuntu-deb".into(),
                artifact_type: "deb".into(),
                product_version: "1.0.0".into(),
                source_commit: "a".repeat(40),
                artifact_sha256: "b".repeat(64),
                manifest_sha256: "c".repeat(64),
                authentication_method: "synthetic-test-only".into(),
                verified_local_path: "/synthetic/fixture.deb".into(),
            },
        };
        let request = InstallationRequest {
            intent: InstallationIntent::Install,
            target_scope: TargetScope::NativePackage,
            verified_artifact: Some(artifact.clone()),
            user_choices: BTreeMap::new(),
        };
        let effect = InstallationEffect {
            effect_id: "fixture.install-package".into(),
            phase: EffectPhase::Install,
            owner: ResourceClass::NativePackageState,
            resource_class: ResourceClass::NativePackageState,
            authority: EffectAuthority::NativePackageManager,
            target_scope: TargetScope::NativePackage,
            preconditions: vec![Precondition {
                kind: "package_identity_absent".into(),
                expected: "fixture-ubuntu-deb".into(),
            }],
            privilege: PrivilegeRequirement::NativeAuthorization,
            mutation_kind: "install_native_package".into(),
            verification: Some(VerificationContract {
                verification_id: "fixture.package.verify".into(),
                expected: "package identity and version match".into(),
            }),
            retry_policy: RetryPolicy::Never,
            reconciliation_policy: ReconciliationPolicy::InspectByEffectId,
            reversibility: Reversibility::PartiallyReversible,
            safe_inverse: None,
            preservation_constraints: vec![ResourceClass::CredentialState],
            artifact_identity: Some("fixture-ubuntu-deb".into()),
        };
        let plan = InstallationPlan {
            schema_version: ENGINE_SCHEMA_VERSION,
            plan_id: "fixture.ubuntu.deb.install".into(),
            intent: InstallationIntent::Install,
            target_scope: TargetScope::NativePackage,
            ordered_effects: vec![effect],
            preservation_set: PROTECTED_RESOURCES.to_vec(),
            expected_completion_verification: VerificationContract {
                verification_id: "fixture.install.complete".into(),
                expected: "native package identity is present".into(),
            },
            artifact: Some(artifact),
        };
        (request, plan)
    }

    #[test]
    fn every_production_channel_fails_closed_without_roots_and_catalogs() {
        for channel in [
            PlanChannel::UbuntuDeb,
            PlanChannel::FedoraRpm,
            PlanChannel::LinuxAppImage,
            PlanChannel::WindowsPerUserSetup,
            PlanChannel::WindowsPortableZip,
            PlanChannel::ExistingInstallationMaintenance,
        ] {
            let result = AuthoritativePlanBuilder.build(
                supported_request(channel),
                &SyntheticSource(ArtifactEvidenceKind::AuthenticatedReleaseManifest),
            );
            let PlanBuildOutcome::Blocked(blockers) = result;
            assert!(blockers.contains(&PlanBuildBlocker::ProductionTrustRootUnavailable));
            assert!(blockers.contains(&PlanBuildBlocker::ApprovedEffectCatalogUnavailable));
            assert!(blockers.contains(&PlanBuildBlocker::PolicyPrerequisitesPending));
        }
        let unknown = ArtifactSourceRequest {
            platform: PlanPlatform::Ubuntu2404,
            architecture: PlanArchitecture::X86_64,
            channel: PlanChannel::Unknown,
            target_scope: TargetScope::CurrentUser,
            intent: InstallationIntent::Install,
            capability_generation: 4,
        };
        let PlanBuildOutcome::Blocked(blockers) = AuthoritativePlanBuilder.build(
            unknown,
            &SyntheticSource(ArtifactEvidenceKind::UnknownOrMissingArtifact),
        );
        assert!(blockers.contains(&PlanBuildBlocker::InstallationChannelUnavailable));
    }

    #[test]
    fn invalid_channel_pairing_is_explicitly_blocked() {
        let request = ArtifactSourceRequest {
            platform: PlanPlatform::Windows,
            architecture: PlanArchitecture::X86_64,
            channel: PlanChannel::UbuntuDeb,
            target_scope: TargetScope::NativePackage,
            intent: InstallationIntent::Install,
            capability_generation: 4,
        };
        let PlanBuildOutcome::Blocked(blockers) = AuthoritativePlanBuilder.build(
            request,
            &SyntheticSource(ArtifactEvidenceKind::UnknownOrMissingArtifact),
        );
        assert!(blockers.contains(&PlanBuildBlocker::PlatformChannelMismatch));
        assert!(blockers.contains(&PlanBuildBlocker::ArtifactEvidenceUnavailable));
    }

    #[test]
    fn unsupported_scope_and_intent_are_explicitly_blocked() {
        let mut request = supported_request(PlanChannel::WindowsPerUserSetup);
        request.target_scope = TargetScope::System;
        let PlanBuildOutcome::Blocked(blockers) = AuthoritativePlanBuilder.build(
            request,
            &SyntheticSource(ArtifactEvidenceKind::AuthenticatedReleaseManifest),
        );
        assert!(blockers.contains(&PlanBuildBlocker::UnsupportedTargetScope));
        request.target_scope = TargetScope::CurrentUser;
        request.intent = InstallationIntent::Uninstall;
        let PlanBuildOutcome::Blocked(blockers) = AuthoritativePlanBuilder.build(
            request,
            &SyntheticSource(ArtifactEvidenceKind::AuthenticatedReleaseManifest),
        );
        assert!(blockers.contains(&PlanBuildBlocker::UnsupportedIntent));
    }

    #[test]
    fn digest_only_and_untrusted_local_bytes_do_not_become_trusted() {
        for evidence in [
            ArtifactEvidenceKind::DigestVerifiedArtifactBytes,
            ArtifactEvidenceKind::LocallySuppliedUntrustedArtifact,
            ArtifactEvidenceKind::UnknownOrMissingArtifact,
            ArtifactEvidenceKind::InvalidRelease,
            ArtifactEvidenceKind::StaleRelease,
            ArtifactEvidenceKind::RevokedRelease,
            ArtifactEvidenceKind::UnsupportedRelease,
            ArtifactEvidenceKind::UnsupportedAuthenticationScheme,
            ArtifactEvidenceKind::UntrustedPublisherKey,
            ArtifactEvidenceKind::UnauthorizedDowngrade,
            ArtifactEvidenceKind::IncompleteProvenance,
            ArtifactEvidenceKind::AmbiguousArtifactSelection,
            ArtifactEvidenceKind::ArtifactIdentityMismatch,
            ArtifactEvidenceKind::WrongPlatformOrArchitecture,
        ] {
            let PlanBuildOutcome::Blocked(blockers) = AuthoritativePlanBuilder.build(
                supported_request(PlanChannel::UbuntuDeb),
                &SyntheticSource(evidence),
            );
            assert!(blockers.contains(&PlanBuildBlocker::ArtifactEvidenceUnavailable));
            assert!(blockers.contains(&PlanBuildBlocker::ProductionTrustRootUnavailable));
        }
    }

    #[test]
    fn synthetic_platform_fixture_is_bounded_and_engine_validated_only() {
        let (request, plan) = synthetic_test_plan();
        assert_eq!(plan.ordered_effects.len(), 1);
        assert_eq!(plan.target_scope, TargetScope::NativePackage);
        assert_eq!(
            plan.ordered_effects[0].authority,
            EffectAuthority::NativePackageManager
        );
        assert_eq!(
            plan.artifact.as_ref().map(ArtifactEvidence::identity),
            request
                .verified_artifact
                .as_ref()
                .map(ArtifactEvidence::identity)
        );
        assert!(InstallerEngine.validate(&request, &plan).is_ok());
        let Some(ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = &plan.artifact else {
            panic!("synthetic fixture must carry explicit artifact evidence");
        };
        assert_eq!(evidence.authentication_method, "synthetic-test-only");
    }

    #[test]
    fn verified_synthetic_candidate_uses_p009_snapshot_and_generation_binding() {
        let (request, plan) = synthetic_test_plan();
        let context = PlanReviewContext {
            device_role: "host".into(),
            connection_mode: "local_only".into(),
            platform: "ubuntu-24.04".into(),
            architecture: "x86_64".into(),
            channel: "ubuntu_deb".into(),
            capability_generation: 5,
            manifest_generation: Some(1),
            installation_state_generation: 7,
            platform_policy_id: "synthetic-test-policy".into(),
            effect_catalog_version: Some(1),
            security_policy_version: Some(1),
            required_policy_ids: vec!["synthetic-test-policy".into()],
            preflight_findings: vec![],
        };
        let snapshot = validate_plan_for_review(&request, &plan, context)
            .expect("synthetic, engine-validated fixture reaches P009 only in tests");
        assert_eq!(snapshot.status(), PlanReviewStatus::AvailableForReview);
        assert!(snapshot.is_unchanged_and_valid());
        assert!(snapshot.fingerprint_summary().starts_with("sha256:"));
    }
}
