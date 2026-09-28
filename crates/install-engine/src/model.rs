use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallationIntent {
    Install,
    Upgrade,
    Repair,
    Verify,
    Uninstall,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetScope {
    CurrentUser,
    System,
    NativePackage,
    PortableUser,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResourceClass {
    PackageOwned,
    NativePackageState,
    PlatformIntegrationOwned,
    ApplicationConfig,
    CredentialState,
    ClientSyncState,
    UserLibraryContent,
    ServerConfig,
    ServerDatabase,
    ServerObjectData,
    ExternalDependency,
    EphemeralRuntimeState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectPhase {
    Install,
    Integrate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectAuthority {
    InstallerAdapter,
    NativePackageManager,
    PlatformIntegrationAdapter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivilegeRequirement {
    CurrentUser,
    NativeAuthorization,
    ElevatedSystem,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Reversibility {
    Reversible,
    PartiallyReversible,
    Irreversible,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RetryPolicy {
    Never,
    AfterReplan,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReconciliationPolicy {
    InspectByEffectId,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedArtifactEvidence {
    pub artifact_id: String,
    pub artifact_type: String,
    pub product_version: String,
    pub source_commit: String,
    pub artifact_sha256: String,
    pub manifest_sha256: String,
    pub authentication_method: String,
    pub verified_local_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "provenance", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArtifactEvidence {
    VerifiedReleaseArtifact {
        evidence: VerifiedArtifactEvidence,
    },
    LocallySuppliedArtifact {
        artifact_id: String,
        local_path: String,
    },
    NativeManagedArtifact {
        package_identity: String,
    },
}

impl ArtifactEvidence {
    pub fn identity(&self) -> &str {
        match self {
            Self::VerifiedReleaseArtifact { evidence } => &evidence.artifact_id,
            Self::LocallySuppliedArtifact { artifact_id, .. } => artifact_id,
            Self::NativeManagedArtifact { package_identity } => package_identity,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationRequest {
    pub intent: InstallationIntent,
    pub target_scope: TargetScope,
    pub verified_artifact: Option<ArtifactEvidence>,
    #[serde(default)]
    pub user_choices: BTreeMap<String, bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingSeverity {
    Information,
    Warning,
    Blocking,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightFinding {
    pub finding_id: String,
    pub severity: FindingSeverity,
    pub capability: String,
    pub redacted_evidence: String,
    pub blocks_planning: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Precondition {
    pub kind: String,
    pub expected: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationContract {
    pub verification_id: String,
    pub expected: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationEffect {
    pub effect_id: String,
    pub phase: EffectPhase,
    pub owner: ResourceClass,
    pub resource_class: ResourceClass,
    pub authority: EffectAuthority,
    pub target_scope: TargetScope,
    pub preconditions: Vec<Precondition>,
    pub privilege: PrivilegeRequirement,
    pub mutation_kind: String,
    pub verification: Option<VerificationContract>,
    pub retry_policy: RetryPolicy,
    pub reconciliation_policy: ReconciliationPolicy,
    pub reversibility: Reversibility,
    pub safe_inverse: Option<String>,
    pub preservation_constraints: Vec<ResourceClass>,
    pub artifact_identity: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub intent: InstallationIntent,
    pub target_scope: TargetScope,
    pub ordered_effects: Vec<InstallationEffect>,
    pub preservation_set: Vec<ResourceClass>,
    pub expected_completion_verification: VerificationContract,
    pub artifact: Option<ArtifactEvidence>,
}

impl InstallationPlan {
    pub fn from_json(input: &str) -> Result<Self, EngineErrorCode> {
        let plan: Self = serde_json::from_str(input).map_err(|_| EngineErrorCode::InvalidPlan)?;
        if plan.schema_version != crate::ENGINE_SCHEMA_VERSION {
            return Err(EngineErrorCode::UnsupportedSchema);
        }
        Ok(plan)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Stage {
    Preflight,
    Plan,
    Install,
    Integrate,
    Verify,
    Complete,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectResultState {
    VerifiedSuccess,
    VerifiedNoop,
    FailureBeforeMutation,
    KnownPartialMutation,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplyOutcome {
    Success,
    Noop,
    FailureBeforeMutation,
    KnownPartialMutation,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReconciliationOutcome {
    VerifiedApplied,
    VerifiedNotApplied,
    StillUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EngineErrorCode {
    InvalidPlan,
    UnsupportedSchema,
    UnauthorizedOwnership,
    PrivilegeUnavailable,
    PlanStale,
    EffectFailed,
    VerificationFailed,
    OutcomeUnknown,
    ReconciliationRequired,
    CompensationFailed,
    FinalVerificationFailed,
    PreflightBlocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum EngineEventKind {
    StageEntered {
        stage: Stage,
    },
    StageCompleted {
        stage: Stage,
    },
    EffectStarted {
        effect_id: String,
    },
    EffectResultRecorded {
        effect_id: String,
        result: EffectResultState,
    },
    VerificationStarted {
        verification_id: String,
    },
    VerificationRecorded {
        verification_id: String,
        succeeded: bool,
    },
    InstallationCompleted,
    InstallationStopped {
        code: EngineErrorCode,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineEvent {
    pub schema_version: u32,
    pub plan_id: String,
    pub sequence: u64,
    pub kind: EngineEventKind,
}

impl EngineEvent {
    pub fn from_json(input: &str) -> Result<Self, EngineErrorCode> {
        let event: Self = serde_json::from_str(input).map_err(|_| EngineErrorCode::InvalidPlan)?;
        if event.schema_version != crate::ENGINE_SCHEMA_VERSION {
            return Err(EngineErrorCode::UnsupportedSchema);
        }
        Ok(event)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRecord {
    pub effect_id: String,
    pub result: EffectResultState,
    pub redacted_evidence: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompensationRecord {
    pub effect_id: String,
    pub succeeded: bool,
    pub verified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationResult {
    pub schema_version: u32,
    pub plan_id: String,
    pub completed: bool,
    pub installation_verified: bool,
    pub launch_eligible: bool,
    pub first_run_complete: bool,
    pub sync_ready: bool,
    pub server_ready: bool,
    pub error: Option<EngineErrorCode>,
    pub safe_to_replan: bool,
    pub effects: Vec<EffectRecord>,
    pub compensations: Vec<CompensationRecord>,
    pub events: Vec<EngineEvent>,
}

impl InstallationResult {
    pub fn from_json(input: &str) -> Result<Self, EngineErrorCode> {
        let result: Self = serde_json::from_str(input).map_err(|_| EngineErrorCode::InvalidPlan)?;
        if result.schema_version != crate::ENGINE_SCHEMA_VERSION {
            return Err(EngineErrorCode::UnsupportedSchema);
        }
        Ok(result)
    }
}
