//! Policy constraints applied before the P007 engine or P008 journal is entered.

use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub const LIFECYCLE_SCHEMA_VERSION: u32 = 1;

pub const PROTECTED_RESOURCES: [ResourceClass; 8] = [
    ResourceClass::ApplicationConfig,
    ResourceClass::CredentialState,
    ResourceClass::ClientSyncState,
    ResourceClass::UserLibraryContent,
    ResourceClass::ServerConfig,
    ResourceClass::ServerDatabase,
    ResourceClass::ServerObjectData,
    ResourceClass::ExternalDependency,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateClassification {
    Absent,
    Healthy,
    Missing,
    Damaged,
    Drifted,
    Unknown,
    NewerThanSupported,
    Interrupted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupPreference {
    Enabled,
    Disabled,
    Unset,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialPresence {
    Present,
    Absent,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeState {
    Inactive,
    ActiveOwned,
    ActiveUnknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalState {
    None,
    Complete,
    ActiveIncomplete,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductIdentity {
    pub version: String,
    pub source_commit: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreservationSnapshot {
    pub application_config: Option<String>,
    pub credential_presence: CredentialPresence,
    pub credential_identity: Option<String>,
    pub client_sync_state: Option<String>,
    pub user_library_content: Option<String>,
    pub server_config: Option<String>,
    pub server_database: Option<String>,
    pub server_object_data: Option<String>,
    pub external_dependency: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledStateSnapshot {
    pub product: Option<ProductIdentity>,
    pub package_payload: StateClassification,
    pub native_package: StateClassification,
    pub platform_integration: StateClassification,
    pub startup_preference: StartupPreference,
    pub runtime: RuntimeState,
    pub journal: JournalState,
    pub preservation: PreservationSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityDisposition {
    Supported,
    UnsupportedSource,
    UnsupportedTarget,
    RequiresIntermediateVersion,
    Unknown,
    DowngradeRejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeCompatibility {
    pub source: ProductIdentity,
    pub target: ProductIdentity,
    pub disposition: CompatibilityDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeRequest {
    pub target_artifact: VerifiedArtifactEvidence,
    pub compatibility: UpgradeCompatibility,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairRequest {
    pub expected: ProductIdentity,
    pub target_artifact: Option<VerifiedArtifactEvidence>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleRequest {
    Upgrade(UpgradeRequest),
    Repair(RepairRequest),
    Uninstall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleOperation {
    ReplacePayload,
    RestorePayload,
    RemovePayload,
    ReconcileIntegration,
    RemoveIntegration,
    NativePackageOperation,
    StopOwnedRuntime,
    CleanOwnedRuntimeState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleEffect {
    pub effect_id: String,
    pub operation: LifecycleOperation,
    pub ownership_proven: bool,
    pub startup_preference: Option<StartupPreference>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleDecision {
    MutationReady,
    VerifiedNoop,
    Compatible,
    Repairable,
    RemovalReady,
    NativeManagerRequired,
    RuntimeStopRequired,
    InterruptedRecoveryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    IntentMismatch,
    ActiveTransaction,
    UnknownState,
    UnsupportedSource,
    UnsupportedTarget,
    DowngradeRejected,
    IntermediateVersionRequired,
    ArtifactIdentityMismatch,
    VersionMismatch,
    ProtectedMutation,
    UnknownOwnership,
    InvalidOperation,
    StartupPreferenceChanged,
    PreservationUnknown,
    PreservationChanged,
}

#[derive(Default)]
pub struct LifecyclePolicy;

impl LifecyclePolicy {
    pub fn validate(
        &self,
        request: &LifecycleRequest,
        state: &InstalledStateSnapshot,
        plan: &InstallationPlan,
        effects: &[LifecycleEffect],
    ) -> Result<LifecycleDecision, LifecycleError> {
        if matches!(
            state.journal,
            JournalState::ActiveIncomplete | JournalState::OutcomeUnknown
        ) {
            return Err(LifecycleError::ActiveTransaction);
        }
        if matches!(
            state.package_payload,
            StateClassification::Unknown
                | StateClassification::NewerThanSupported
                | StateClassification::Interrupted
        ) {
            return Err(LifecycleError::UnknownState);
        }
        if PROTECTED_RESOURCES
            .iter()
            .any(|r| !plan.preservation_set.contains(r))
            || plan.ordered_effects.iter().any(|e| {
                PROTECTED_RESOURCES.contains(&e.resource_class)
                    || PROTECTED_RESOURCES.contains(&e.owner)
            })
        {
            return Err(LifecycleError::ProtectedMutation);
        }
        if effects.len() != plan.ordered_effects.len() {
            return Err(LifecycleError::InvalidOperation);
        }
        let semantics: BTreeMap<&str, &LifecycleEffect> =
            effects.iter().map(|e| (e.effect_id.as_str(), e)).collect();
        for effect in &plan.ordered_effects {
            let semantic = semantics
                .get(effect.effect_id.as_str())
                .ok_or(LifecycleError::InvalidOperation)?;
            if !semantic.ownership_proven {
                return Err(LifecycleError::UnknownOwnership);
            }
            if matches!(
                request,
                LifecycleRequest::Upgrade(_) | LifecycleRequest::Repair(_)
            ) && state.startup_preference == StartupPreference::Disabled
                && semantic
                    .startup_preference
                    .is_some_and(|p| p != StartupPreference::Disabled)
            {
                return Err(LifecycleError::StartupPreferenceChanged);
            }
            validate_effect(request, effect, semantic.operation)?;
            validate_effect_state(state, semantic.operation)?;
        }
        match request {
            LifecycleRequest::Upgrade(upgrade) => validate_upgrade(upgrade, state, plan),
            LifecycleRequest::Repair(repair) => validate_repair(repair, state, plan),
            LifecycleRequest::Uninstall => {
                if plan.intent != InstallationIntent::Uninstall {
                    return Err(LifecycleError::IntentMismatch);
                }
                if plan.ordered_effects.is_empty()
                    && state.package_payload == StateClassification::Absent
                    && state.platform_integration == StateClassification::Absent
                {
                    Ok(LifecycleDecision::VerifiedNoop)
                } else {
                    Ok(LifecycleDecision::RemovalReady)
                }
            }
        }
    }

    pub fn verify_preservation(
        &self,
        before: &PreservationSnapshot,
        after: &PreservationSnapshot,
    ) -> Result<(), LifecycleError> {
        if preservation_unknown(before) || preservation_unknown(after) {
            return Err(LifecycleError::PreservationUnknown);
        }
        if before != after {
            return Err(LifecycleError::PreservationChanged);
        }
        Ok(())
    }

    pub fn validate_purge(&self, request: &PurgeRequest) -> Result<(), PurgeError> {
        validate_purge(request)
    }
}

fn validate_upgrade(
    upgrade: &UpgradeRequest,
    state: &InstalledStateSnapshot,
    plan: &InstallationPlan,
) -> Result<LifecycleDecision, LifecycleError> {
    if plan.intent != InstallationIntent::Upgrade {
        return Err(LifecycleError::IntentMismatch);
    }
    let installed = state.product.as_ref().ok_or(LifecycleError::UnknownState)?;
    if installed != &upgrade.compatibility.source {
        return Err(LifecycleError::UnsupportedSource);
    }
    // Compatibility is explicit policy, never permission to downgrade or repair
    // through an Upgrade intent. Compare independently before any effects.
    let source = stable_version(&installed.version)?;
    let target = stable_version(&upgrade.compatibility.target.version)?;
    if target < source {
        return Err(LifecycleError::DowngradeRejected);
    }
    if target == source {
        return Err(LifecycleError::IntentMismatch);
    }
    match upgrade.compatibility.disposition {
        CompatibilityDisposition::Supported => {}
        CompatibilityDisposition::UnsupportedSource => {
            return Err(LifecycleError::UnsupportedSource);
        }
        CompatibilityDisposition::UnsupportedTarget => {
            return Err(LifecycleError::UnsupportedTarget);
        }
        CompatibilityDisposition::RequiresIntermediateVersion => {
            return Err(LifecycleError::IntermediateVersionRequired);
        }
        CompatibilityDisposition::DowngradeRejected => {
            return Err(LifecycleError::DowngradeRejected);
        }
        CompatibilityDisposition::Unknown => return Err(LifecycleError::UnknownState),
    }
    let a = &upgrade.target_artifact;
    if a.product_version != upgrade.compatibility.target.version
        || upgrade.compatibility.target.source_commit.as_deref() != Some(a.source_commit.as_str())
        || plan.artifact.as_ref()
            != Some(&ArtifactEvidence::VerifiedReleaseArtifact {
                evidence: a.clone(),
            })
    {
        return Err(LifecycleError::ArtifactIdentityMismatch);
    }
    if installed == &upgrade.compatibility.target
        && state.package_payload == StateClassification::Healthy
        && state.platform_integration == StateClassification::Healthy
    {
        return Ok(LifecycleDecision::VerifiedNoop);
    }
    Ok(LifecycleDecision::Compatible)
}

fn validate_repair(
    repair: &RepairRequest,
    state: &InstalledStateSnapshot,
    plan: &InstallationPlan,
) -> Result<LifecycleDecision, LifecycleError> {
    if plan.intent != InstallationIntent::Repair {
        return Err(LifecycleError::IntentMismatch);
    }
    stable_version(&repair.expected.version)?;
    if state.product.as_ref() != Some(&repair.expected) {
        return Err(LifecycleError::VersionMismatch);
    }
    if let Some(a) = &repair.target_artifact
        && (a.product_version != repair.expected.version
            || repair
                .expected
                .source_commit
                .as_deref()
                .is_some_and(|c| c != a.source_commit))
    {
        return Err(LifecycleError::VersionMismatch);
    }
    if plan.ordered_effects.is_empty()
        && state.package_payload == StateClassification::Healthy
        && state.platform_integration == StateClassification::Healthy
    {
        Ok(LifecycleDecision::VerifiedNoop)
    } else {
        Ok(LifecycleDecision::Repairable)
    }
}

fn stable_version(value: &str) -> Result<[u32; 3], LifecycleError> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 {
        return Err(LifecycleError::UnknownState);
    }
    let mut version = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty()
            || part.len() > 10
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(LifecycleError::UnknownState);
        }
        version[index] = part.parse().map_err(|_| LifecycleError::UnknownState)?;
    }
    Ok(version)
}

fn validate_effect(
    request: &LifecycleRequest,
    effect: &InstallationEffect,
    op: LifecycleOperation,
) -> Result<(), LifecycleError> {
    let allowed = match request {
        LifecycleRequest::Upgrade(_) => matches!(
            op,
            LifecycleOperation::ReplacePayload
                | LifecycleOperation::ReconcileIntegration
                | LifecycleOperation::NativePackageOperation
                | LifecycleOperation::StopOwnedRuntime
                | LifecycleOperation::CleanOwnedRuntimeState
        ),
        LifecycleRequest::Repair(_) => matches!(
            op,
            LifecycleOperation::RestorePayload
                | LifecycleOperation::ReconcileIntegration
                | LifecycleOperation::NativePackageOperation
                | LifecycleOperation::StopOwnedRuntime
                | LifecycleOperation::CleanOwnedRuntimeState
        ),
        LifecycleRequest::Uninstall => matches!(
            op,
            LifecycleOperation::RemovePayload
                | LifecycleOperation::RemoveIntegration
                | LifecycleOperation::NativePackageOperation
                | LifecycleOperation::StopOwnedRuntime
                | LifecycleOperation::CleanOwnedRuntimeState
        ),
    };
    if !allowed {
        return Err(LifecycleError::InvalidOperation);
    }

    let (required_class, required_authority) = match op {
        LifecycleOperation::ReplacePayload
        | LifecycleOperation::RestorePayload
        | LifecycleOperation::RemovePayload => (
            ResourceClass::PackageOwned,
            EffectAuthority::InstallerAdapter,
        ),
        LifecycleOperation::ReconcileIntegration | LifecycleOperation::RemoveIntegration => (
            ResourceClass::PlatformIntegrationOwned,
            EffectAuthority::PlatformIntegrationAdapter,
        ),
        LifecycleOperation::NativePackageOperation => (
            ResourceClass::NativePackageState,
            EffectAuthority::NativePackageManager,
        ),
        LifecycleOperation::StopOwnedRuntime | LifecycleOperation::CleanOwnedRuntimeState => (
            ResourceClass::EphemeralRuntimeState,
            EffectAuthority::InstallerAdapter,
        ),
    };

    if effect.owner != required_class
        || effect.resource_class != required_class
        || effect.authority != required_authority
    {
        return Err(LifecycleError::InvalidOperation);
    }
    Ok(())
}

fn validate_effect_state(
    state: &InstalledStateSnapshot,
    op: LifecycleOperation,
) -> Result<(), LifecycleError> {
    match op {
        LifecycleOperation::NativePackageOperation
            if matches!(
                state.native_package,
                StateClassification::Unknown
                    | StateClassification::NewerThanSupported
                    | StateClassification::Interrupted
            ) =>
        {
            Err(LifecycleError::UnknownState)
        }
        LifecycleOperation::ReconcileIntegration | LifecycleOperation::RemoveIntegration
            if matches!(
                state.platform_integration,
                StateClassification::Unknown
                    | StateClassification::NewerThanSupported
                    | StateClassification::Interrupted
            ) =>
        {
            Err(LifecycleError::UnknownState)
        }
        LifecycleOperation::StopOwnedRuntime => match state.runtime {
            RuntimeState::ActiveOwned => Ok(()),
            RuntimeState::ActiveUnknown => Err(LifecycleError::UnknownState),
            RuntimeState::Inactive => Err(LifecycleError::InvalidOperation),
        },
        LifecycleOperation::CleanOwnedRuntimeState => match state.runtime {
            RuntimeState::Inactive => Ok(()),
            RuntimeState::ActiveUnknown => Err(LifecycleError::UnknownState),
            RuntimeState::ActiveOwned => Err(LifecycleError::InvalidOperation),
        },
        _ => Ok(()),
    }
}

fn preservation_unknown(p: &PreservationSnapshot) -> bool {
    let credential_unknown = match p.credential_presence {
        CredentialPresence::Present => p
            .credential_identity
            .as_deref()
            .is_none_or(|identity| identity.is_empty()),
        CredentialPresence::Absent => p.credential_identity.is_some(),
        CredentialPresence::Unknown => true,
    };
    credential_unknown
        || p.application_config.is_none()
        || p.client_sync_state.is_none()
        || p.user_library_content.is_none()
        || p.server_config.is_none()
        || p.server_database.is_none()
        || p.server_object_data.is_none()
        || p.external_dependency.is_none()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PurgeResourceClass {
    ApplicationConfig,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetOwnership {
    CanonicalOwner,
    Shared,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeTarget {
    pub target_id: String,
    pub resource_class: PurgeResourceClass,
    pub canonical_path: String,
    pub expected_root: String,
    pub ownership: TargetOwnership,
    pub containment_proven: bool,
    pub symlink_free: bool,
    pub traversal_free: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeAuthorization {
    pub confirmation_id: String,
    pub authorized_classes: BTreeSet<PurgeResourceClass>,
    pub authorized_target_ids: BTreeSet<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeRequest {
    pub schema_version: u32,
    pub destructive_intent: bool,
    pub authorization: Option<PurgeAuthorization>,
    pub targets: Vec<PurgeTarget>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurgeError {
    UnsupportedSchema,
    MissingIntent,
    MissingAuthorization,
    MissingScope,
    UnauthorizedTarget,
    UnknownOwnership,
    UnsafeContainment,
    SymlinkRejected,
}

fn validate_purge(request: &PurgeRequest) -> Result<(), PurgeError> {
    if request.schema_version != LIFECYCLE_SCHEMA_VERSION {
        return Err(PurgeError::UnsupportedSchema);
    }
    if !request.destructive_intent {
        return Err(PurgeError::MissingIntent);
    }
    let auth = request
        .authorization
        .as_ref()
        .ok_or(PurgeError::MissingAuthorization)?;
    if auth.confirmation_id.is_empty()
        || auth.authorized_classes.is_empty()
        || auth.authorized_target_ids.is_empty()
        || request.targets.is_empty()
    {
        return Err(PurgeError::MissingScope);
    }
    for target in &request.targets {
        if target.target_id.is_empty()
            || target.canonical_path.is_empty()
            || target.expected_root.is_empty()
            || !auth.authorized_classes.contains(&target.resource_class)
            || !auth.authorized_target_ids.contains(&target.target_id)
        {
            return Err(PurgeError::UnauthorizedTarget);
        }
        if target.ownership != TargetOwnership::CanonicalOwner {
            return Err(PurgeError::UnknownOwnership);
        }
        if !target.symlink_free {
            return Err(PurgeError::SymlinkRejected);
        }
        if !target.containment_proven || !target.traversal_free {
            return Err(PurgeError::UnsafeContainment);
        }
    }
    Ok(())
}
