//! Typed native adapter capability contracts layered over P011's closed
//! effect IDs and read-only authorization requirements. This module describes
//! ownership and policy boundaries only; it has no command runner or adapter
//! execution entrypoint.

use crate::{
    EffectAdapter, EffectCatalogEntry, EffectCatalogViolation, EffectId, InstallationPlan,
    InstallationRequest, PlanReviewContext, PlanReviewFingerprint, PlanReviewSnapshot,
    PrivilegeRequirement, ResourceClass, TargetScope, validate_effect_catalog,
};

pub const NATIVE_ADAPTER_CONTRACT_VERSION: u32 = 1;

const UBUNTU_DEB_EFFECTS: &[EffectId] = &[
    EffectId::PackagePayloadInstall,
    EffectId::NativeDebPackageTransaction,
];
const FEDORA_RPM_EFFECTS: &[EffectId] = &[
    EffectId::PackagePayloadInstall,
    EffectId::NativeRpmPackageTransaction,
];
const APPIMAGE_EFFECTS: &[EffectId] = &[
    EffectId::PackagePayloadInstall,
    EffectId::AppImageCurrentUserIntegration,
];
const WINDOWS_SETUP_EFFECTS: &[EffectId] = &[
    EffectId::PackagePayloadInstall,
    EffectId::WindowsPerUserSetup,
];
const WINDOWS_PORTABLE_EFFECTS: &[EffectId] = &[EffectId::PackagePayloadInstall];
const LINUX_HOST_EFFECTS: &[EffectId] = &[EffectId::BoundedLinuxHostAction];
const NO_EFFECTS: &[EffectId] = &[];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAdapterKind {
    UbuntuDebAptDpkg,
    FedoraRpmDnfRpm,
    LinuxCurrentUserAppImage,
    BoundedLinuxHostHelper,
    WindowsCurrentUserSetup,
    WindowsPortableZip,
    ExistingInstallationMaintenance,
    UnsupportedElevatedOperation,
    Unsupported,
}

impl NativeAdapterKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UbuntuDebAptDpkg => "ubuntu_deb_apt_dpkg_contract",
            Self::FedoraRpmDnfRpm => "fedora_rpm_dnf_rpm_contract",
            Self::LinuxCurrentUserAppImage => "linux_current_user_appimage_contract",
            Self::BoundedLinuxHostHelper => "bounded_linux_host_helper_contract",
            Self::WindowsCurrentUserSetup => "windows_current_user_setup_contract",
            Self::WindowsPortableZip => "windows_portable_zip_contract",
            Self::ExistingInstallationMaintenance => "existing_installation_maintenance_contract",
            Self::UnsupportedElevatedOperation => "unsupported_elevated_operation",
            Self::Unsupported => "unsupported_adapter",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterScopePolicy {
    NativePackageOnly,
    PortableUserOnly,
    CurrentUserOnly,
    BoundedSystemHelperOnly,
    ExistingInstallationMustBeIdentified,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterAuthorizationCategory {
    CurrentUserOperation,
    NativePackageManagerAuthorization,
    BoundedHostHelperAuthorization,
    UnsupportedAuthorization,
    AuthorizationPolicyUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterAuthorizationAvailability {
    CurrentUserContractOnly,
    PackageManagerContractOnly,
    BoundedHelperContractOnly,
    Unsupported,
    PolicyUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterAuthorizationRequirement {
    pub category: AdapterAuthorizationCategory,
    pub availability: AdapterAuthorizationAvailability,
    pub target_scope: TargetScope,
    pub required_privilege: PrivilegeRequirement,
}

/// A descriptive capability contract. It does not contain executable code,
/// a process name, a shell command, or an authorization result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeAdapterCapability {
    pub kind: NativeAdapterKind,
    pub platform: String,
    pub architecture: String,
    pub channel: String,
    pub scope_policy: AdapterScopePolicy,
    pub target_scope: TargetScope,
    pub effect_adapter: Option<EffectAdapter>,
    pub effect_ids: Vec<EffectId>,
    pub resource_owner: Option<ResourceClass>,
    pub authorization: AdapterAuthorizationRequirement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterValidationStatus {
    ContractOnly,
    Unsupported,
    Stale,
    CatalogMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterValidationBlocker {
    UnsupportedChannel,
    PlatformChannelMismatch,
    UnsupportedArchitecture,
    UnsupportedScope,
    UnsupportedElevatedOperation,
    ExistingInstallationIdentityUnavailable,
    UnknownEffectId,
    EffectNotOwnedByAdapter,
    EffectOwnerMismatch,
    ClosedCatalogUnavailable,
    ClosedCatalogMismatch,
    PlanFingerprintUnavailable,
    ArtifactIdentityUnavailable,
    ManifestGenerationUnavailable,
    CapabilityGenerationUnavailable,
    InstallationStateGenerationUnavailable,
    EffectCatalogVersionUnavailable,
    SecurityPolicyVersionUnavailable,
    PlanContextStale,
    NativeAuthorizationUnavailable,
    PrivilegeMismatch,
}

impl AdapterValidationBlocker {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedChannel => "unsupported_channel",
            Self::PlatformChannelMismatch => "platform_channel_mismatch",
            Self::UnsupportedArchitecture => "unsupported_architecture",
            Self::UnsupportedScope => "unsupported_scope",
            Self::UnsupportedElevatedOperation => "unsupported_elevated_operation",
            Self::ExistingInstallationIdentityUnavailable => {
                "existing_installation_identity_unavailable"
            }
            Self::UnknownEffectId => "unknown_effect_id",
            Self::EffectNotOwnedByAdapter => "effect_not_owned_by_adapter",
            Self::EffectOwnerMismatch => "effect_owner_mismatch",
            Self::ClosedCatalogUnavailable => "closed_catalog_unavailable",
            Self::ClosedCatalogMismatch => "closed_catalog_mismatch",
            Self::PlanFingerprintUnavailable => "plan_fingerprint_unavailable",
            Self::ArtifactIdentityUnavailable => "artifact_identity_unavailable",
            Self::ManifestGenerationUnavailable => "manifest_generation_unavailable",
            Self::CapabilityGenerationUnavailable => "capability_generation_unavailable",
            Self::InstallationStateGenerationUnavailable => {
                "installation_state_generation_unavailable"
            }
            Self::EffectCatalogVersionUnavailable => "effect_catalog_version_unavailable",
            Self::SecurityPolicyVersionUnavailable => "security_policy_version_unavailable",
            Self::PlanContextStale => "plan_context_stale",
            Self::NativeAuthorizationUnavailable => "native_authorization_unavailable",
            Self::PrivilegeMismatch => "privilege_mismatch",
        }
    }
}

/// Per-effect identity binding produced by pure validation. It must be
/// recomputed when any plan, artifact, catalog, policy, capability, or
/// installation-state generation changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterEffectBinding {
    pub effect_id: EffectId,
    pub adapter: NativeAdapterKind,
    pub plan_fingerprint_sha256: String,
    pub artifact_identity: String,
    pub artifact_sha256: String,
    pub artifact_size_bytes: u64,
    pub manifest_identity: String,
    pub source_provenance_reference: String,
    pub trust_policy_identity: String,
    pub manifest_generation: u64,
    pub capability_generation: u64,
    pub installation_state_generation: u64,
    pub effect_catalog_version: u32,
    pub security_policy_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterValidationResult {
    pub schema_version: u32,
    pub(crate) status: AdapterValidationStatus,
    pub(crate) capability: NativeAdapterCapability,
    pub(crate) authorization_requirement: AdapterAuthorizationRequirement,
    pub(crate) effect_bindings: Vec<AdapterEffectBinding>,
    pub(crate) blockers: Vec<AdapterValidationBlocker>,
    fingerprint: Option<PlanReviewFingerprint>,
    plan_id: String,
}

impl AdapterValidationResult {
    pub const fn status(&self) -> AdapterValidationStatus {
        self.status
    }

    pub const fn capability_kind(&self) -> NativeAdapterKind {
        self.capability.kind
    }

    pub fn blockers(&self) -> &[AdapterValidationBlocker] {
        &self.blockers
    }

    pub fn matches_current(
        &self,
        request: &InstallationRequest,
        plan: &InstallationPlan,
        review: &PlanReviewSnapshot,
        context: &PlanReviewContext,
    ) -> bool {
        let Some(crate::ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = &plan.artifact
        else {
            return false;
        };
        self.status == AdapterValidationStatus::ContractOnly
            && self.fingerprint == Some(review.fingerprint())
            && self.plan_id == plan.plan_id
            && review.matches_current(request, plan, context)
            && self.capability.platform == context.platform
            && self.capability.architecture == context.architecture
            && self.capability.channel == context.channel
            && self.capability.target_scope == plan.target_scope
            && self.effect_bindings.iter().all(|binding| {
                binding.plan_fingerprint_sha256 == review.fingerprint().as_hex()
                    && binding.artifact_identity == evidence.artifact_id
                    && binding.artifact_sha256 == evidence.artifact_sha256
                    && binding.artifact_size_bytes == evidence.artifact_size_bytes
                    && binding.manifest_identity == evidence.manifest_sha256
                    && binding.source_provenance_reference == evidence.source_provenance_reference
                    && binding.trust_policy_identity == evidence.trust_policy_identity
                    && binding.manifest_generation == context.manifest_generation.unwrap_or(0)
                    && binding.capability_generation == context.capability_generation
                    && binding.installation_state_generation
                        == context.installation_state_generation
                    && Some(binding.effect_catalog_version) == context.effect_catalog_version
                    && Some(binding.security_policy_version) == context.security_policy_version
            })
    }
}

pub fn validate_native_adapter_capability(
    request: &InstallationRequest,
    plan: &InstallationPlan,
    review: &PlanReviewSnapshot,
    context: &PlanReviewContext,
    catalog: &[EffectCatalogEntry],
) -> AdapterValidationResult {
    let capability = classify_native_adapter(
        &context.channel,
        &context.platform,
        &context.architecture,
        plan.target_scope,
    );
    let mut blockers = Vec::new();
    let mut status = AdapterValidationStatus::ContractOnly;

    if capability.kind == NativeAdapterKind::Unsupported {
        blockers.push(AdapterValidationBlocker::UnsupportedChannel);
        status = AdapterValidationStatus::Unsupported;
    }
    if capability.kind == NativeAdapterKind::UnsupportedElevatedOperation {
        blockers.push(AdapterValidationBlocker::UnsupportedElevatedOperation);
        status = AdapterValidationStatus::Unsupported;
    }
    if !platform_channel_matches(&capability) {
        blockers.push(AdapterValidationBlocker::PlatformChannelMismatch);
        status = AdapterValidationStatus::Unsupported;
    }
    if context.architecture != "x86_64" {
        blockers.push(AdapterValidationBlocker::UnsupportedArchitecture);
        status = AdapterValidationStatus::Unsupported;
    }
    if !scope_matches(capability.scope_policy, plan.target_scope) {
        blockers.push(AdapterValidationBlocker::UnsupportedScope);
        status = AdapterValidationStatus::Unsupported;
    }
    if capability.kind == NativeAdapterKind::ExistingInstallationMaintenance {
        blockers.push(AdapterValidationBlocker::ExistingInstallationIdentityUnavailable);
        status = AdapterValidationStatus::Unsupported;
    }
    if context.manifest_generation.is_none() {
        blockers.push(AdapterValidationBlocker::ManifestGenerationUnavailable);
    }
    if context.capability_generation == 0 {
        blockers.push(AdapterValidationBlocker::CapabilityGenerationUnavailable);
    }
    if context.installation_state_generation == 0 {
        blockers.push(AdapterValidationBlocker::InstallationStateGenerationUnavailable);
    }
    if context.effect_catalog_version.is_none() {
        blockers.push(AdapterValidationBlocker::EffectCatalogVersionUnavailable);
    }
    if context.security_policy_version.is_none() {
        blockers.push(AdapterValidationBlocker::SecurityPolicyVersionUnavailable);
    }
    if !review.matches_current(request, plan, context) {
        blockers.push(AdapterValidationBlocker::PlanContextStale);
        if status == AdapterValidationStatus::ContractOnly {
            status = AdapterValidationStatus::Stale;
        }
    }

    let artifact = plan.artifact.as_ref();
    let artifact_evidence = match artifact {
        Some(crate::ArtifactEvidence::VerifiedReleaseArtifact { evidence }) => Some(evidence),
        _ => None,
    };
    if artifact_evidence.is_none() {
        blockers.push(AdapterValidationBlocker::ArtifactIdentityUnavailable);
    }
    if review.fingerprint().as_hex().len() != 64 {
        blockers.push(AdapterValidationBlocker::PlanFingerprintUnavailable);
    }

    if catalog.is_empty() {
        blockers.push(AdapterValidationBlocker::ClosedCatalogUnavailable);
        status = AdapterValidationStatus::CatalogMismatch;
    } else {
        let violations = validate_effect_catalog(plan, &context.channel, catalog);
        if !violations.is_empty()
            || catalog.len() != plan.ordered_effects.len()
            || plan.ordered_effects.iter().any(|effect| {
                EffectId::parse(&effect.effect_id).is_none_or(|id| {
                    !capability.effect_ids.contains(&id)
                        || catalog.iter().filter(|entry| entry.id == id).count() != 1
                })
            })
        {
            blockers.push(AdapterValidationBlocker::ClosedCatalogMismatch);
            if plan
                .ordered_effects
                .iter()
                .any(|effect| EffectId::parse(&effect.effect_id).is_none())
            {
                blockers.push(AdapterValidationBlocker::UnknownEffectId);
            }
            if plan.ordered_effects.iter().any(|effect| {
                EffectId::parse(&effect.effect_id)
                    .is_some_and(|id| !capability.effect_ids.contains(&id))
            }) {
                blockers.push(AdapterValidationBlocker::EffectNotOwnedByAdapter);
            }
            if violations.iter().any(|violation| {
                matches!(
                    violation,
                    EffectCatalogViolation::ResourceOwnerMismatch
                        | EffectCatalogViolation::ResourceClassMismatch
                )
            }) {
                blockers.push(AdapterValidationBlocker::EffectOwnerMismatch);
            }
            status = AdapterValidationStatus::CatalogMismatch;
        }
    }

    if plan.ordered_effects.iter().any(|effect| {
        EffectId::parse(&effect.effect_id)
            .and_then(|id| expected_privilege(&capability, id))
            .is_some_and(|privilege| privilege != effect.privilege)
    }) {
        blockers.push(AdapterValidationBlocker::PrivilegeMismatch);
        status = AdapterValidationStatus::CatalogMismatch;
    }

    if capability.authorization.availability
        != AdapterAuthorizationAvailability::CurrentUserContractOnly
    {
        blockers.push(AdapterValidationBlocker::NativeAuthorizationUnavailable);
    }
    // Neither the P011 description catalog nor a test-supplied catalog is an
    // approved production catalog. This result stays contract-only.
    if status == AdapterValidationStatus::ContractOnly {
        blockers.push(AdapterValidationBlocker::ClosedCatalogUnavailable);
    }

    let plan_fingerprint_sha256 = review.fingerprint().as_hex();
    let effect_bindings = plan
        .ordered_effects
        .iter()
        .filter_map(|effect| {
            let effect_id = EffectId::parse(&effect.effect_id)?;
            let artifact = artifact_evidence?;
            Some(AdapterEffectBinding {
                effect_id,
                adapter: capability.kind,
                plan_fingerprint_sha256: plan_fingerprint_sha256.clone(),
                artifact_identity: artifact.artifact_id.clone(),
                artifact_sha256: artifact.artifact_sha256.clone(),
                artifact_size_bytes: artifact.artifact_size_bytes,
                manifest_identity: artifact.manifest_sha256.clone(),
                source_provenance_reference: artifact.source_provenance_reference.clone(),
                trust_policy_identity: artifact.trust_policy_identity.clone(),
                manifest_generation: context.manifest_generation.unwrap_or(0),
                capability_generation: context.capability_generation,
                installation_state_generation: context.installation_state_generation,
                effect_catalog_version: context.effect_catalog_version.unwrap_or(0),
                security_policy_version: context.security_policy_version.unwrap_or(0),
            })
        })
        .collect();

    AdapterValidationResult {
        schema_version: NATIVE_ADAPTER_CONTRACT_VERSION,
        status,
        authorization_requirement: capability.authorization,
        capability,
        effect_bindings,
        blockers,
        fingerprint: Some(review.fingerprint()),
        plan_id: plan.plan_id.clone(),
    }
}

pub fn classify_native_adapter(
    channel: &str,
    platform: &str,
    architecture: &str,
    scope: TargetScope,
) -> NativeAdapterCapability {
    let (
        kind,
        scope_policy,
        effect_adapter,
        effect_ids,
        resource_owner,
        auth_category,
        auth_status,
        privilege,
    ) = match (channel, scope) {
        ("ubuntu_deb", TargetScope::NativePackage) => (
            NativeAdapterKind::UbuntuDebAptDpkg,
            AdapterScopePolicy::NativePackageOnly,
            Some(EffectAdapter::NativePackageManager),
            UBUNTU_DEB_EFFECTS,
            Some(ResourceClass::NativePackageState),
            AdapterAuthorizationCategory::NativePackageManagerAuthorization,
            AdapterAuthorizationAvailability::PackageManagerContractOnly,
            PrivilegeRequirement::NativeAuthorization,
        ),
        ("fedora_rpm", TargetScope::NativePackage) => (
            NativeAdapterKind::FedoraRpmDnfRpm,
            AdapterScopePolicy::NativePackageOnly,
            Some(EffectAdapter::NativePackageManager),
            FEDORA_RPM_EFFECTS,
            Some(ResourceClass::NativePackageState),
            AdapterAuthorizationCategory::NativePackageManagerAuthorization,
            AdapterAuthorizationAvailability::PackageManagerContractOnly,
            PrivilegeRequirement::NativeAuthorization,
        ),
        ("linux_appimage", TargetScope::PortableUser) => (
            NativeAdapterKind::LinuxCurrentUserAppImage,
            AdapterScopePolicy::PortableUserOnly,
            Some(EffectAdapter::LinuxAppImageIntegration),
            APPIMAGE_EFFECTS,
            Some(ResourceClass::PlatformIntegrationOwned),
            AdapterAuthorizationCategory::CurrentUserOperation,
            AdapterAuthorizationAvailability::CurrentUserContractOnly,
            PrivilegeRequirement::CurrentUser,
        ),
        ("managed_linux_host_action", TargetScope::System) => (
            NativeAdapterKind::BoundedLinuxHostHelper,
            AdapterScopePolicy::BoundedSystemHelperOnly,
            Some(EffectAdapter::BoundedLinuxHostHelper),
            LINUX_HOST_EFFECTS,
            Some(ResourceClass::PlatformIntegrationOwned),
            AdapterAuthorizationCategory::BoundedHostHelperAuthorization,
            AdapterAuthorizationAvailability::BoundedHelperContractOnly,
            PrivilegeRequirement::ElevatedSystem,
        ),
        ("windows_per_user_setup", TargetScope::CurrentUser) => (
            NativeAdapterKind::WindowsCurrentUserSetup,
            AdapterScopePolicy::CurrentUserOnly,
            Some(EffectAdapter::WindowsPerUserSetup),
            WINDOWS_SETUP_EFFECTS,
            Some(ResourceClass::PackageOwned),
            AdapterAuthorizationCategory::CurrentUserOperation,
            AdapterAuthorizationAvailability::CurrentUserContractOnly,
            PrivilegeRequirement::CurrentUser,
        ),
        ("windows_portable_zip", TargetScope::PortableUser) => (
            NativeAdapterKind::WindowsPortableZip,
            AdapterScopePolicy::PortableUserOnly,
            Some(EffectAdapter::InstallerEngine),
            WINDOWS_PORTABLE_EFFECTS,
            Some(ResourceClass::PackageOwned),
            AdapterAuthorizationCategory::CurrentUserOperation,
            AdapterAuthorizationAvailability::CurrentUserContractOnly,
            PrivilegeRequirement::CurrentUser,
        ),
        ("existing_installation" | "existing_installation_maintenance", TargetScope::System) => (
            NativeAdapterKind::UnsupportedElevatedOperation,
            AdapterScopePolicy::Unsupported,
            None,
            NO_EFFECTS,
            None,
            AdapterAuthorizationCategory::UnsupportedAuthorization,
            AdapterAuthorizationAvailability::Unsupported,
            PrivilegeRequirement::ElevatedSystem,
        ),
        ("existing_installation" | "existing_installation_maintenance", _) => (
            NativeAdapterKind::ExistingInstallationMaintenance,
            AdapterScopePolicy::ExistingInstallationMustBeIdentified,
            None,
            NO_EFFECTS,
            None,
            AdapterAuthorizationCategory::AuthorizationPolicyUnavailable,
            AdapterAuthorizationAvailability::PolicyUnavailable,
            PrivilegeRequirement::NativeAuthorization,
        ),
        ("windows_per_user_setup", TargetScope::System) => (
            NativeAdapterKind::UnsupportedElevatedOperation,
            AdapterScopePolicy::Unsupported,
            None,
            NO_EFFECTS,
            None,
            AdapterAuthorizationCategory::UnsupportedAuthorization,
            AdapterAuthorizationAvailability::Unsupported,
            PrivilegeRequirement::ElevatedSystem,
        ),
        ("linux_appimage" | "windows_portable_zip", TargetScope::System) => (
            NativeAdapterKind::UnsupportedElevatedOperation,
            AdapterScopePolicy::Unsupported,
            None,
            NO_EFFECTS,
            None,
            AdapterAuthorizationCategory::UnsupportedAuthorization,
            AdapterAuthorizationAvailability::Unsupported,
            PrivilegeRequirement::ElevatedSystem,
        ),
        (_, TargetScope::System) => (
            NativeAdapterKind::UnsupportedElevatedOperation,
            AdapterScopePolicy::Unsupported,
            None,
            NO_EFFECTS,
            None,
            AdapterAuthorizationCategory::UnsupportedAuthorization,
            AdapterAuthorizationAvailability::Unsupported,
            PrivilegeRequirement::ElevatedSystem,
        ),
        _ => (
            NativeAdapterKind::Unsupported,
            AdapterScopePolicy::Unsupported,
            None,
            NO_EFFECTS,
            None,
            AdapterAuthorizationCategory::UnsupportedAuthorization,
            AdapterAuthorizationAvailability::Unsupported,
            PrivilegeRequirement::ElevatedSystem,
        ),
    };
    NativeAdapterCapability {
        kind,
        platform: platform.to_owned(),
        architecture: architecture.to_owned(),
        channel: channel.to_owned(),
        scope_policy,
        target_scope: scope,
        effect_adapter,
        effect_ids: effect_ids.to_vec(),
        resource_owner,
        authorization: AdapterAuthorizationRequirement {
            category: auth_category,
            availability: auth_status,
            target_scope: scope,
            required_privilege: privilege,
        },
    }
}

fn platform_channel_matches(capability: &NativeAdapterCapability) -> bool {
    match capability.kind {
        NativeAdapterKind::UbuntuDebAptDpkg => capability.platform == "ubuntu-24.04",
        NativeAdapterKind::FedoraRpmDnfRpm => capability.platform == "fedora-42",
        NativeAdapterKind::LinuxCurrentUserAppImage => matches!(
            capability.platform.as_str(),
            "linux" | "ubuntu-24.04" | "fedora-42"
        ),
        NativeAdapterKind::BoundedLinuxHostHelper => {
            matches!(capability.platform.as_str(), "ubuntu-24.04" | "fedora-42")
        }
        NativeAdapterKind::WindowsCurrentUserSetup | NativeAdapterKind::WindowsPortableZip => {
            capability.platform == "windows"
        }
        NativeAdapterKind::ExistingInstallationMaintenance
        | NativeAdapterKind::UnsupportedElevatedOperation
        | NativeAdapterKind::Unsupported => false,
    }
}

fn scope_matches(policy: AdapterScopePolicy, scope: TargetScope) -> bool {
    match policy {
        AdapterScopePolicy::NativePackageOnly => scope == TargetScope::NativePackage,
        AdapterScopePolicy::PortableUserOnly => scope == TargetScope::PortableUser,
        AdapterScopePolicy::CurrentUserOnly => scope == TargetScope::CurrentUser,
        AdapterScopePolicy::BoundedSystemHelperOnly => scope == TargetScope::System,
        AdapterScopePolicy::ExistingInstallationMustBeIdentified => false,
        AdapterScopePolicy::Unsupported => false,
    }
}

fn expected_privilege(
    capability: &NativeAdapterCapability,
    effect_id: EffectId,
) -> Option<PrivilegeRequirement> {
    match effect_id {
        EffectId::NativeDebPackageTransaction | EffectId::NativeRpmPackageTransaction => {
            Some(PrivilegeRequirement::NativeAuthorization)
        }
        EffectId::AppImageCurrentUserIntegration | EffectId::WindowsPerUserSetup => {
            Some(PrivilegeRequirement::CurrentUser)
        }
        EffectId::BoundedLinuxHostAction => Some(PrivilegeRequirement::ElevatedSystem),
        EffectId::PackagePayloadInstall => Some(capability.authorization.required_privilege),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ENGINE_SCHEMA_VERSION, EffectAuthority, EffectPhase, InstallationEffect,
        InstallationIntent, InstallationPlan, PROTECTED_RESOURCES, Precondition,
        ReconciliationPolicy, RetryPolicy, Reversibility, VerificationContract,
        VerifiedArtifactEvidence,
    };

    fn fixture() -> (InstallationRequest, InstallationPlan, PlanReviewContext) {
        let artifact = crate::ArtifactEvidence::VerifiedReleaseArtifact {
            evidence: VerifiedArtifactEvidence {
                artifact_id: "fixture-appimage".into(),
                artifact_type: "appimage".into(),
                product_version: "1.2.3".into(),
                source_commit: "a".repeat(40),
                artifact_sha256: "b".repeat(64),
                artifact_size_bytes: 1024,
                manifest_sha256: "c".repeat(64),
                authentication_method: "synthetic-test-only".into(),
                source_provenance_reference: "release-artifact:fixture-appimage".into(),
                trust_policy_identity: "synthetic-test-policy-only".into(),
                verified_local_path: "/private/fixture/AppImage".into(),
            },
        };
        let request = InstallationRequest {
            intent: InstallationIntent::Install,
            target_scope: TargetScope::PortableUser,
            verified_artifact: Some(artifact.clone()),
            user_choices: Default::default(),
        };
        let effect = InstallationEffect {
            effect_id: EffectId::AppImageCurrentUserIntegration.as_str().into(),
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
            device_role: "client".into(),
            connection_mode: "manual".into(),
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

    fn catalog(plan: &InstallationPlan) -> Vec<EffectCatalogEntry> {
        plan.ordered_effects
            .iter()
            .map(|effect| EffectCatalogEntry {
                id: EffectId::parse(&effect.effect_id).expect("closed fixture effect"),
                channel: "linux_appimage".into(),
                adapter: EffectAdapter::LinuxAppImageIntegration,
                resource_owner: effect.owner,
                resource_class: effect.resource_class,
                target_scope: effect.target_scope,
                phase: effect.phase,
                preconditions: effect.preconditions.clone(),
                privilege: effect.privilege,
                mutation_kind: effect.mutation_kind.clone(),
                verification: effect.verification.clone().expect("verification"),
                reconciliation: effect.reconciliation_policy,
                retry: effect.retry_policy,
                reversibility: effect.reversibility,
                safe_inverse: effect.safe_inverse.clone(),
                preservation_constraints: effect.preservation_constraints.clone(),
            })
            .collect()
    }

    #[test]
    fn channel_matrix_keeps_platform_scope_and_adapter_ownership_distinct() {
        let deb = classify_native_adapter(
            "ubuntu_deb",
            "ubuntu-24.04",
            "x86_64",
            TargetScope::NativePackage,
        );
        assert_eq!(deb.kind, NativeAdapterKind::UbuntuDebAptDpkg);
        assert_eq!(
            deb.authorization.availability,
            AdapterAuthorizationAvailability::PackageManagerContractOnly
        );

        let rpm = classify_native_adapter(
            "fedora_rpm",
            "fedora-42",
            "x86_64",
            TargetScope::NativePackage,
        );
        assert_eq!(rpm.kind, NativeAdapterKind::FedoraRpmDnfRpm);

        let appimage = classify_native_adapter(
            "linux_appimage",
            "ubuntu-24.04",
            "x86_64",
            TargetScope::PortableUser,
        );
        assert_eq!(appimage.kind, NativeAdapterKind::LinuxCurrentUserAppImage);
        assert_eq!(
            appimage.authorization.category,
            AdapterAuthorizationCategory::CurrentUserOperation
        );

        let setup = classify_native_adapter(
            "windows_per_user_setup",
            "windows",
            "x86_64",
            TargetScope::CurrentUser,
        );
        assert_eq!(setup.kind, NativeAdapterKind::WindowsCurrentUserSetup);

        let portable = classify_native_adapter(
            "windows_portable_zip",
            "windows",
            "x86_64",
            TargetScope::PortableUser,
        );
        assert_eq!(portable.kind, NativeAdapterKind::WindowsPortableZip);

        let maintenance = classify_native_adapter(
            "existing_installation_maintenance",
            "windows",
            "x86_64",
            TargetScope::CurrentUser,
        );
        assert_eq!(
            maintenance.kind,
            NativeAdapterKind::ExistingInstallationMaintenance
        );

        let host = classify_native_adapter(
            "managed_linux_host_action",
            "fedora-42",
            "x86_64",
            TargetScope::System,
        );
        assert_eq!(host.kind, NativeAdapterKind::BoundedLinuxHostHelper);
        assert_eq!(
            host.authorization.availability,
            AdapterAuthorizationAvailability::BoundedHelperContractOnly
        );
    }

    #[test]
    fn system_elevation_and_platform_architecture_mismatch_fail_closed() {
        let windows_system = classify_native_adapter(
            "windows_per_user_setup",
            "windows",
            "x86_64",
            TargetScope::System,
        );
        assert_eq!(
            windows_system.kind,
            NativeAdapterKind::UnsupportedElevatedOperation
        );
        assert_eq!(
            windows_system.authorization.availability,
            AdapterAuthorizationAvailability::Unsupported
        );

        let wrong_platform = classify_native_adapter(
            "ubuntu_deb",
            "fedora-42",
            "x86_64",
            TargetScope::NativePackage,
        );
        assert!(!platform_channel_matches(&wrong_platform));

        let wrong_architecture = classify_native_adapter(
            "linux_appimage",
            "ubuntu-24.04",
            "aarch64",
            TargetScope::PortableUser,
        );
        assert_ne!(wrong_architecture.architecture, "x86_64");

        let elevated_appimage = classify_native_adapter(
            "linux_appimage",
            "ubuntu-24.04",
            "x86_64",
            TargetScope::System,
        );
        assert_eq!(
            elevated_appimage.kind,
            NativeAdapterKind::UnsupportedElevatedOperation
        );
    }

    #[test]
    fn wrong_platform_architecture_and_channel_fail_closed_in_validation() {
        let (request, plan, mut context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let entries = catalog(&plan);

        context.platform = "windows".into();
        let wrong_platform =
            validate_native_adapter_capability(&request, &plan, &review, &context, &entries);
        assert_eq!(
            wrong_platform.status(),
            AdapterValidationStatus::Unsupported
        );
        assert!(
            wrong_platform
                .blockers()
                .contains(&AdapterValidationBlocker::PlatformChannelMismatch)
        );

        let (request, plan, mut context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let entries = catalog(&plan);
        context.architecture = "aarch64".into();
        let wrong_architecture =
            validate_native_adapter_capability(&request, &plan, &review, &context, &entries);
        assert_eq!(
            wrong_architecture.status(),
            AdapterValidationStatus::Unsupported
        );
        assert!(
            wrong_architecture
                .blockers()
                .contains(&AdapterValidationBlocker::UnsupportedArchitecture)
        );

        let (request, plan, mut context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let entries = catalog(&plan);
        context.channel = "windows_portable_zip".into();
        context.platform = "windows".into();
        let wrong_channel =
            validate_native_adapter_capability(&request, &plan, &review, &context, &entries);
        assert_eq!(
            wrong_channel.status(),
            AdapterValidationStatus::CatalogMismatch
        );
        assert!(
            wrong_channel
                .blockers()
                .contains(&AdapterValidationBlocker::EffectNotOwnedByAdapter)
        );
    }

    #[test]
    fn exact_catalog_is_bound_but_never_promoted_to_production_authority() {
        let (request, plan, context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let entries = catalog(&plan);
        let validation =
            validate_native_adapter_capability(&request, &plan, &review, &context, &entries);
        assert_eq!(validation.status(), AdapterValidationStatus::ContractOnly);
        assert_eq!(
            validation.capability_kind(),
            NativeAdapterKind::LinuxCurrentUserAppImage
        );
        assert!(
            validation
                .blockers()
                .contains(&AdapterValidationBlocker::ClosedCatalogUnavailable)
        );
        assert!(validation.matches_current(&request, &plan, &review, &context));

        let mut changed_binding = validation.clone();
        changed_binding.effect_bindings[0]
            .source_provenance_reference
            .push_str("-changed");
        assert!(!changed_binding.matches_current(&request, &plan, &review, &context));
        let mut changed_binding = validation.clone();
        changed_binding.effect_bindings[0]
            .trust_policy_identity
            .push_str("-changed");
        assert!(!changed_binding.matches_current(&request, &plan, &review, &context));

        let mut wrong_owner = entries;
        wrong_owner[0].adapter = EffectAdapter::NativePackageManager;
        let rejected =
            validate_native_adapter_capability(&request, &plan, &review, &context, &wrong_owner);
        assert_eq!(rejected.status(), AdapterValidationStatus::CatalogMismatch);
        assert!(
            rejected
                .blockers()
                .contains(&AdapterValidationBlocker::ClosedCatalogMismatch)
        );
    }

    #[test]
    fn unknown_effect_stale_context_and_unidentified_maintenance_are_blocked() {
        let (request, mut plan, context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let entries = catalog(&plan);
        plan.ordered_effects[0].effect_id = "run.arbitrary.command".into();
        let invalidated =
            validate_native_adapter_capability(&request, &plan, &review, &context, &entries);
        assert_eq!(
            invalidated.status(),
            AdapterValidationStatus::CatalogMismatch
        );
        assert!(
            invalidated
                .blockers()
                .contains(&AdapterValidationBlocker::UnknownEffectId)
        );

        let (request, plan, context) = fixture();
        let review = PlanReviewSnapshot::validate_and_create(&request, &plan, context.clone())
            .expect("synthetic review snapshot");
        let entries = catalog(&plan);
        let mut changed_context = context.clone();
        changed_context.manifest_generation = Some(4);
        let stale = validate_native_adapter_capability(
            &request,
            &plan,
            &review,
            &changed_context,
            &entries,
        );
        assert_eq!(stale.status(), AdapterValidationStatus::Stale);
        assert!(
            stale
                .blockers()
                .contains(&AdapterValidationBlocker::PlanContextStale)
        );

        let maintenance = classify_native_adapter(
            "existing_installation_maintenance",
            "ubuntu-24.04",
            "x86_64",
            TargetScope::NativePackage,
        );
        assert_eq!(
            maintenance.kind,
            NativeAdapterKind::ExistingInstallationMaintenance
        );
        assert!(!scope_matches(
            maintenance.scope_policy,
            TargetScope::NativePackage
        ));
    }

    #[test]
    fn adapter_contract_has_no_command_or_execution_surface() {
        let source = include_str!("native_adapter.rs");
        let process_namespace = ["std", "::", "process"].concat();
        let effect_method = ["apply_", "effect("].concat();
        let journal_name = ["Installation", "Journal"].concat();
        let sudo_command = ["sudo", " "].concat();
        let polkit_command = ["pkexec", " "].concat();
        assert!(!source.contains(&process_namespace));
        assert!(!source.contains(&effect_method));
        assert!(!source.contains(&journal_name));
        assert!(!source.contains(&sudo_command));
        assert!(!source.contains(&polkit_command));
    }
}
