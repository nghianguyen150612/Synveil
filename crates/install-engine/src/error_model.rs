//! Finite, fail-closed presentation policy for installer failures.

use crate::{
    EngineErrorCode, InstallationIntent, JournalErrorCode, LifecycleError, PurgeError,
    RecoveryDisposition, ResourceClass, TargetScope,
};
use serde::{Deserialize, Serialize};

pub const ERROR_MODEL_SCHEMA_VERSION: u32 = 1;
pub const MAX_DIAGNOSTIC_SERIALIZED_BYTES: usize = 4096;
pub const MAX_SAFE_IDENTIFIER_BYTES: usize = 96;
pub const MAX_SAFE_CODE_BYTES: usize = 64;
pub const MAX_SECONDARY_ACTIONS: usize = 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorStage {
    Preflight,
    Plan,
    Acquire,
    VerifyArtifact,
    Install,
    Integrate,
    VerifyInstallation,
    Complete,
    Recovery,
    LifecyclePolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallerErrorCategory {
    UnsupportedPlatform,
    UnsupportedArchitecture,
    ArtifactUnavailable,
    InsufficientDiskSpace,
    PermissionRequired,
    DownloadFailed,
    IntegrityVerificationFailed,
    StoragePreparationFailed,
    PackageInstallFailed,
    RuntimeDependencyFailed,
    IntegrationFailed,
    VerificationFailed,
    Interrupted,
    RecoveryRequired,
    ConcurrentOperation,
    UnsupportedLifecycle,
    ProtectedStateBlocked,
    InvalidSetupState,
    InternalFailure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecommendedAction {
    ChooseSupportedSystem,
    ChooseSupportedArtifact,
    UseSupportedVersion,
    FreeDiskSpace,
    RequestPermission,
    RetryDownload,
    GetFreshOfficialDownload,
    ChooseDifferentDestination,
    UseNativePackageRecovery,
    RepairInstallation,
    ResumeRecovery,
    InspectRecovery,
    CloseOtherInstaller,
    Replan,
    ShowDetails,
    ContactSupport,
    Cancel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserRetryPolicy {
    SafeImmediate,
    AfterUserAction,
    ReconcileFirst,
    FreshEvidenceRequired,
    NotRetryable,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceFamily {
    Engine,
    Journal,
    Recovery,
    Lifecycle,
    Purge,
    Acquisition,
    Preflight,
    Platform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformFailure {
    UnsupportedPlatform,
    UnsupportedArchitecture,
    InsufficientDiskSpace,
    NativePackageTransactionFailed,
    RuntimeDependencyFailed,
    IntegrationFailed,
    LaunchFailed,
}

pub type PreflightFailure = PlatformFailure;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerFailure {
    Engine(EngineErrorCode),
    Journal(JournalErrorCode),
    Recovery(RecoveryDisposition),
    Lifecycle(LifecycleError),
    Purge(PurgeError),
    Acquisition(AcquisitionFailureCode),
    Preflight(PreflightFailure),
    Platform(PlatformFailure),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AcquisitionFailureCode {
    AmbiguousArtifact,
    ArtifactDigestMismatch,
    ArtifactTooLarge,
    ArtifactTruncated,
    DestinationConflict,
    InvalidManifest,
    ManifestAuthFailed,
    ManifestTooLarge,
    NetworkError,
    NoMatchingArtifact,
    SourceCommitMismatch,
    StagingError,
    UnsafePath,
    UnsafeRedirect,
    UnsupportedArchitecture,
    UnsupportedAuthentication,
    UnsupportedPlatform,
    UntrustedOrigin,
    VersionMismatch,
}

impl AcquisitionFailureCode {
    pub const ALL: [Self; 19] = [
        Self::AmbiguousArtifact,
        Self::ArtifactDigestMismatch,
        Self::ArtifactTooLarge,
        Self::ArtifactTruncated,
        Self::DestinationConflict,
        Self::InvalidManifest,
        Self::ManifestAuthFailed,
        Self::ManifestTooLarge,
        Self::NetworkError,
        Self::NoMatchingArtifact,
        Self::SourceCommitMismatch,
        Self::StagingError,
        Self::UnsafePath,
        Self::UnsafeRedirect,
        Self::UnsupportedArchitecture,
        Self::UnsupportedAuthentication,
        Self::UnsupportedPlatform,
        Self::UntrustedOrigin,
        Self::VersionMismatch,
    ];
    pub const fn code(self) -> &'static str {
        match self {
            Self::AmbiguousArtifact => "AMBIGUOUS_ARTIFACT",
            Self::ArtifactDigestMismatch => "ARTIFACT_DIGEST_MISMATCH",
            Self::ArtifactTooLarge => "ARTIFACT_TOO_LARGE",
            Self::ArtifactTruncated => "ARTIFACT_TRUNCATED",
            Self::DestinationConflict => "DESTINATION_CONFLICT",
            Self::InvalidManifest => "INVALID_MANIFEST",
            Self::ManifestAuthFailed => "MANIFEST_AUTH_FAILED",
            Self::ManifestTooLarge => "MANIFEST_TOO_LARGE",
            Self::NetworkError => "NETWORK_ERROR",
            Self::NoMatchingArtifact => "NO_MATCHING_ARTIFACT",
            Self::SourceCommitMismatch => "SOURCE_COMMIT_MISMATCH",
            Self::StagingError => "STAGING_ERROR",
            Self::UnsafePath => "UNSAFE_PATH",
            Self::UnsafeRedirect => "UNSAFE_REDIRECT",
            Self::UnsupportedArchitecture => "UNSUPPORTED_ARCHITECTURE",
            Self::UnsupportedAuthentication => "UNSUPPORTED_AUTHENTICATION",
            Self::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            Self::UntrustedOrigin => "UNTRUSTED_ORIGIN",
            Self::VersionMismatch => "VERSION_MISMATCH",
        }
    }
}

impl TryFrom<&str> for AcquisitionFailureCode {
    type Error = UnknownAcquisitionCode;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(match value {
            "AMBIGUOUS_ARTIFACT" => Self::AmbiguousArtifact,
            "ARTIFACT_DIGEST_MISMATCH" => Self::ArtifactDigestMismatch,
            "ARTIFACT_TOO_LARGE" => Self::ArtifactTooLarge,
            "ARTIFACT_TRUNCATED" => Self::ArtifactTruncated,
            "DESTINATION_CONFLICT" => Self::DestinationConflict,
            "INVALID_MANIFEST" => Self::InvalidManifest,
            "MANIFEST_AUTH_FAILED" => Self::ManifestAuthFailed,
            "MANIFEST_TOO_LARGE" => Self::ManifestTooLarge,
            "NETWORK_ERROR" => Self::NetworkError,
            "NO_MATCHING_ARTIFACT" => Self::NoMatchingArtifact,
            "SOURCE_COMMIT_MISMATCH" => Self::SourceCommitMismatch,
            "STAGING_ERROR" => Self::StagingError,
            "UNSAFE_PATH" => Self::UnsafePath,
            "UNSAFE_REDIRECT" => Self::UnsafeRedirect,
            "UNSUPPORTED_ARCHITECTURE" => Self::UnsupportedArchitecture,
            "UNSUPPORTED_AUTHENTICATION" => Self::UnsupportedAuthentication,
            "UNSUPPORTED_PLATFORM" => Self::UnsupportedPlatform,
            "UNTRUSTED_ORIGIN" => Self::UntrustedOrigin,
            "VERSION_MISMATCH" => Self::VersionMismatch,
            _ => return Err(UnknownAcquisitionCode),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownAcquisitionCode;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SafeDiagnosticIdentifier(String);
impl SafeDiagnosticIdentifier {
    pub fn new(value: &str) -> Option<Self> {
        (!value.is_empty()
            && value.len() <= MAX_SAFE_IDENTIFIER_BYTES
            && value.is_ascii()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':')))
        .then(|| Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FailureContext {
    pub stage: Option<ErrorStage>,
    pub intent: Option<InstallationIntent>,
    pub target_scope: Option<TargetScope>,
    pub resource_class: Option<ResourceClass>,
    pub recovery_disposition: Option<RecoveryDisposition>,
    pub effect_id: Option<SafeDiagnosticIdentifier>,
    pub artifact_id: Option<SafeDiagnosticIdentifier>,
    pub preflight_reason: Option<PreflightFailure>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallerDiagnostic {
    pub support_reference: SafeDiagnosticIdentifier,
    pub category: InstallerErrorCategory,
    pub stage: ErrorStage,
    pub source_family: SourceFamily,
    pub source_code: SafeDiagnosticIdentifier,
    pub intent: Option<InstallationIntent>,
    pub target_scope: Option<TargetScope>,
    pub resource_class: Option<ResourceClass>,
    pub effect_id: Option<SafeDiagnosticIdentifier>,
    pub artifact_id: Option<SafeDiagnosticIdentifier>,
    pub recovery_disposition: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserFacingError {
    pub schema_version: u32,
    pub category: InstallerErrorCategory,
    pub stage: ErrorStage,
    pub message_key: String,
    pub retry_policy: UserRetryPolicy,
    pub primary_action: Option<RecommendedAction>,
    pub secondary_actions: Vec<RecommendedAction>,
    pub diagnostic: InstallerDiagnostic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorModelValidationError {
    WrongSchema,
    TooManySecondaryActions,
    InvalidCombination,
    DiagnosticTooLarge,
}

impl UserFacingError {
    pub fn from_json(input: &str) -> Result<Self, ErrorModelValidationError> {
        let value: Self = serde_json::from_str(input)
            .map_err(|_| ErrorModelValidationError::InvalidCombination)?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), ErrorModelValidationError> {
        if self.schema_version != ERROR_MODEL_SCHEMA_VERSION {
            return Err(ErrorModelValidationError::WrongSchema);
        }
        if self.secondary_actions.len() > MAX_SECONDARY_ACTIONS {
            return Err(ErrorModelValidationError::TooManySecondaryActions);
        }
        if self.retry_policy == UserRetryPolicy::SafeImmediate
            && self.category != InstallerErrorCategory::DownloadFailed
        {
            return Err(ErrorModelValidationError::InvalidCombination);
        }
        if self.category == InstallerErrorCategory::RecoveryRequired
            && (self.retry_policy != UserRetryPolicy::ReconcileFirst
                || self.primary_action == Some(RecommendedAction::RetryDownload))
        {
            return Err(ErrorModelValidationError::InvalidCombination);
        }
        if self.category == InstallerErrorCategory::IntegrityVerificationFailed
            && (self.retry_policy != UserRetryPolicy::FreshEvidenceRequired
                || self.primary_action != Some(RecommendedAction::GetFreshOfficialDownload))
        {
            return Err(ErrorModelValidationError::InvalidCombination);
        }
        if self.category == InstallerErrorCategory::PermissionRequired
            && self.primary_action != Some(RecommendedAction::RequestPermission)
        {
            return Err(ErrorModelValidationError::InvalidCombination);
        }
        if self.category == InstallerErrorCategory::ConcurrentOperation
            && self.retry_policy == UserRetryPolicy::SafeImmediate
        {
            return Err(ErrorModelValidationError::InvalidCombination);
        }
        let expected = format!(
            "SVE-{}-{}",
            family_code(self.diagnostic.source_family),
            self.diagnostic.source_code.as_str()
        );
        if self.diagnostic.support_reference.as_str() != expected
            || !known_source_code(self.diagnostic.source_code.as_str())
        {
            return Err(ErrorModelValidationError::InvalidCombination);
        }
        self.diagnostic_json().map(|_| ())
    }
    pub fn diagnostic_json(&self) -> Result<String, ErrorModelValidationError> {
        let value = serde_json::to_string(&self.diagnostic)
            .map_err(|_| ErrorModelValidationError::DiagnosticTooLarge)?;
        if value.len() > MAX_DIAGNOSTIC_SERIALIZED_BYTES {
            Err(ErrorModelValidationError::DiagnosticTooLarge)
        } else {
            Ok(value)
        }
    }
}

pub fn map_acquisition_code(code: &str, context: &FailureContext) -> UserFacingError {
    match AcquisitionFailureCode::try_from(code) {
        Ok(code) => map_failure(InstallerFailure::Acquisition(code), context)
            .expect("acquisition is an error"),
        Err(_) => build(
            InstallerErrorCategory::InternalFailure,
            ErrorStage::Acquire,
            UserRetryPolicy::NotRetryable,
            Some(RecommendedAction::ContactSupport),
            SourceFamily::Acquisition,
            "UNKNOWN",
            context,
        ),
    }
}

pub fn map_failure(failure: InstallerFailure, context: &FailureContext) -> Option<UserFacingError> {
    let (category, stage, retry, action, family, code) = match failure {
        InstallerFailure::Engine(code) => map_engine(code, context),
        InstallerFailure::Journal(code) => map_journal(code),
        InstallerFailure::Recovery(code) => return map_recovery(code, context),
        InstallerFailure::Lifecycle(code) => map_lifecycle(code),
        InstallerFailure::Purge(code) => map_purge(code),
        InstallerFailure::Acquisition(code) => map_acquisition(code),
        InstallerFailure::Preflight(code) => map_platform(code, SourceFamily::Preflight),
        InstallerFailure::Platform(code) => map_platform(code, SourceFamily::Platform),
    };
    Some(build(category, stage, retry, action, family, code, context))
}

type Mapping = (
    InstallerErrorCategory,
    ErrorStage,
    UserRetryPolicy,
    Option<RecommendedAction>,
    SourceFamily,
    &'static str,
);
fn map_engine(code: EngineErrorCode, c: &FailureContext) -> Mapping {
    use EngineErrorCode::*;
    match code {
        InvalidPlan => m(
            InstallerErrorCategory::InvalidSetupState,
            ErrorStage::Plan,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
            SourceFamily::Engine,
            "INVALID_PLAN",
        ),
        UnsupportedSchema => m(
            InstallerErrorCategory::UnsupportedLifecycle,
            ErrorStage::Plan,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::UseSupportedVersion,
            SourceFamily::Engine,
            "UNSUPPORTED_SCHEMA",
        ),
        UnauthorizedOwnership => m(
            InstallerErrorCategory::ProtectedStateBlocked,
            c.stage.unwrap_or(ErrorStage::Plan),
            UserRetryPolicy::NotRetryable,
            RecommendedAction::ContactSupport,
            SourceFamily::Engine,
            "UNAUTHORIZED_OWNERSHIP",
        ),
        PrivilegeUnavailable => m(
            InstallerErrorCategory::PermissionRequired,
            c.stage.unwrap_or(ErrorStage::Preflight),
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RequestPermission,
            SourceFamily::Engine,
            "PRIVILEGE_UNAVAILABLE",
        ),
        PlanStale => m(
            InstallerErrorCategory::InvalidSetupState,
            ErrorStage::Plan,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
            SourceFamily::Engine,
            "PLAN_STALE",
        ),
        EffectFailed => match (c.stage, c.resource_class) {
            (
                Some(ErrorStage::Install),
                Some(ResourceClass::PackageOwned | ResourceClass::NativePackageState),
            ) => m(
                InstallerErrorCategory::PackageInstallFailed,
                ErrorStage::Install,
                UserRetryPolicy::AfterUserAction,
                RecommendedAction::RepairInstallation,
                SourceFamily::Engine,
                "EFFECT_FAILED",
            ),
            (Some(ErrorStage::Integrate), Some(ResourceClass::PlatformIntegrationOwned)) => m(
                InstallerErrorCategory::IntegrationFailed,
                ErrorStage::Integrate,
                UserRetryPolicy::AfterUserAction,
                RecommendedAction::RepairInstallation,
                SourceFamily::Engine,
                "EFFECT_FAILED",
            ),
            _ => m(
                InstallerErrorCategory::InternalFailure,
                c.stage.unwrap_or(ErrorStage::Install),
                UserRetryPolicy::NotRetryable,
                RecommendedAction::ContactSupport,
                SourceFamily::Engine,
                "EFFECT_FAILED",
            ),
        },
        VerificationFailed => m(
            InstallerErrorCategory::VerificationFailed,
            ErrorStage::VerifyInstallation,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
            SourceFamily::Engine,
            "VERIFICATION_FAILED",
        ),
        OutcomeUnknown => recovery("OUTCOME_UNKNOWN", SourceFamily::Engine),
        ReconciliationRequired => m(
            InstallerErrorCategory::RecoveryRequired,
            ErrorStage::Recovery,
            UserRetryPolicy::ReconcileFirst,
            RecommendedAction::Replan,
            SourceFamily::Engine,
            "RECONCILIATION_REQUIRED",
        ),
        CompensationFailed => recovery("COMPENSATION_FAILED", SourceFamily::Engine),
        FinalVerificationFailed => m(
            InstallerErrorCategory::VerificationFailed,
            ErrorStage::VerifyInstallation,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
            SourceFamily::Engine,
            "FINAL_VERIFICATION_FAILED",
        ),
        PreflightBlocked => match c.preflight_reason {
            Some(reason) => map_platform(reason, SourceFamily::Preflight),
            None => m(
                InstallerErrorCategory::InvalidSetupState,
                ErrorStage::Preflight,
                UserRetryPolicy::NotRetryable,
                RecommendedAction::ContactSupport,
                SourceFamily::Engine,
                "PREFLIGHT_BLOCKED",
            ),
        },
    }
}

fn map_journal(code: JournalErrorCode) -> Mapping {
    use JournalErrorCode::*;
    match code {
        JournalMissing => recovery("MISSING", SourceFamily::Journal),
        JournalBusy => concurrent("BUSY", SourceFamily::Journal),
        JournalCorrupt => recovery("CORRUPT", SourceFamily::Journal),
        JournalUnsupportedSchema => m(
            InstallerErrorCategory::UnsupportedLifecycle,
            ErrorStage::Recovery,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::UseSupportedVersion,
            SourceFamily::Journal,
            "UNSUPPORTED_SCHEMA",
        ),
        JournalPlanMismatch => recovery("PLAN_MISMATCH", SourceFamily::Journal),
        JournalIoFailed => recovery("IO_FAILED", SourceFamily::Journal),
        JournalLimitExceeded => recovery("LIMIT_EXCEEDED", SourceFamily::Journal),
        ActiveTransactionExists => concurrent("ACTIVE_TRANSACTION", SourceFamily::Journal),
        RecoveryInspectionRequired => {
            recovery("RECOVERY_INSPECTION_REQUIRED", SourceFamily::Journal)
        }
    }
}
fn map_recovery(code: RecoveryDisposition, c: &FailureContext) -> Option<UserFacingError> {
    use RecoveryDisposition::*;
    match code {
        Fresh | ResumeReady | ReconciledApplied | AlreadyCompleted => None,
        ReplanRequired => Some(build(
            InstallerErrorCategory::RecoveryRequired,
            ErrorStage::Recovery,
            UserRetryPolicy::ReconcileFirst,
            Some(RecommendedAction::Replan),
            SourceFamily::Recovery,
            "REPLAN_REQUIRED",
            c,
        )),
        InspectionRequired => Some(build_mapping(
            recovery("INSPECTION_REQUIRED", SourceFamily::Recovery),
            c,
        )),
        StillUnknown => Some(build_mapping(
            recovery("STILL_UNKNOWN", SourceFamily::Recovery),
            c,
        )),
        JournalBusy => Some(build_mapping(
            concurrent("JOURNAL_BUSY", SourceFamily::Recovery),
            c,
        )),
        JournalCorrupt => Some(build_mapping(
            recovery("JOURNAL_CORRUPT", SourceFamily::Recovery),
            c,
        )),
    }
}
fn map_lifecycle(code: LifecycleError) -> Mapping {
    use LifecycleError::*;
    match code {
        IntentMismatch => invalid_lifecycle("INTENT_MISMATCH"),
        ActiveTransaction => concurrent("ACTIVE_TRANSACTION", SourceFamily::Lifecycle),
        UnknownState => recovery("UNKNOWN_STATE", SourceFamily::Lifecycle),
        UnsupportedSource => unsupported("UNSUPPORTED_SOURCE"),
        UnsupportedTarget => unsupported("UNSUPPORTED_TARGET"),
        DowngradeRejected => unsupported("DOWNGRADE_REJECTED"),
        IntermediateVersionRequired => unsupported("INTERMEDIATE_VERSION_REQUIRED"),
        ArtifactIdentityMismatch => m(
            InstallerErrorCategory::IntegrityVerificationFailed,
            ErrorStage::VerifyArtifact,
            UserRetryPolicy::FreshEvidenceRequired,
            RecommendedAction::GetFreshOfficialDownload,
            SourceFamily::Lifecycle,
            "ARTIFACT_IDENTITY_MISMATCH",
        ),
        VersionMismatch => unsupported("VERSION_MISMATCH"),
        ProtectedMutation => protected("PROTECTED_MUTATION", SourceFamily::Lifecycle),
        UnknownOwnership => protected("UNKNOWN_OWNERSHIP", SourceFamily::Lifecycle),
        InvalidOperation => invalid_lifecycle("INVALID_OPERATION"),
        StartupPreferenceChanged => {
            protected("STARTUP_PREFERENCE_CHANGED", SourceFamily::Lifecycle)
        }
        PreservationUnknown => protected("PRESERVATION_UNKNOWN", SourceFamily::Lifecycle),
        PreservationChanged => protected("PRESERVATION_CHANGED", SourceFamily::Lifecycle),
    }
}
fn map_purge(code: PurgeError) -> Mapping {
    use PurgeError::*;
    protected(
        match code {
            UnsupportedSchema => "UNSUPPORTED_SCHEMA",
            MissingIntent => "MISSING_INTENT",
            MissingAuthorization => "MISSING_AUTHORIZATION",
            MissingScope => "MISSING_SCOPE",
            UnauthorizedTarget => "UNAUTHORIZED_TARGET",
            UnknownOwnership => "UNKNOWN_OWNERSHIP",
            UnsafeContainment => "UNSAFE_CONTAINMENT",
            SymlinkRejected => "SYMLINK_REJECTED",
        },
        SourceFamily::Purge,
    )
}
fn map_acquisition(code: AcquisitionFailureCode) -> Mapping {
    use AcquisitionFailureCode::*;
    match code {
        NetworkError => m(
            InstallerErrorCategory::DownloadFailed,
            ErrorStage::Acquire,
            UserRetryPolicy::SafeImmediate,
            RecommendedAction::RetryDownload,
            SourceFamily::Acquisition,
            code.code(),
        ),
        UnsupportedPlatform => m(
            InstallerErrorCategory::UnsupportedPlatform,
            ErrorStage::Preflight,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::ChooseSupportedSystem,
            SourceFamily::Acquisition,
            code.code(),
        ),
        UnsupportedArchitecture => m(
            InstallerErrorCategory::UnsupportedArchitecture,
            ErrorStage::Preflight,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedArtifact,
            SourceFamily::Acquisition,
            code.code(),
        ),
        NoMatchingArtifact => m(
            InstallerErrorCategory::ArtifactUnavailable,
            ErrorStage::Acquire,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedArtifact,
            SourceFamily::Acquisition,
            code.code(),
        ),
        DestinationConflict => m(
            InstallerErrorCategory::StoragePreparationFailed,
            ErrorStage::Acquire,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseDifferentDestination,
            SourceFamily::Acquisition,
            code.code(),
        ),
        StagingError => m(
            InstallerErrorCategory::StoragePreparationFailed,
            ErrorStage::Acquire,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::ContactSupport,
            SourceFamily::Acquisition,
            code.code(),
        ),
        UnsafePath => m(
            InstallerErrorCategory::ProtectedStateBlocked,
            ErrorStage::Acquire,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::ContactSupport,
            SourceFamily::Acquisition,
            code.code(),
        ),
        AmbiguousArtifact => m(
            InstallerErrorCategory::InternalFailure,
            ErrorStage::Acquire,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::ContactSupport,
            SourceFamily::Acquisition,
            code.code(),
        ),
        ArtifactDigestMismatch
        | ArtifactTooLarge
        | ArtifactTruncated
        | InvalidManifest
        | ManifestAuthFailed
        | ManifestTooLarge
        | SourceCommitMismatch
        | UnsafeRedirect
        | UnsupportedAuthentication
        | UntrustedOrigin
        | VersionMismatch => m(
            InstallerErrorCategory::IntegrityVerificationFailed,
            ErrorStage::VerifyArtifact,
            UserRetryPolicy::FreshEvidenceRequired,
            RecommendedAction::GetFreshOfficialDownload,
            SourceFamily::Acquisition,
            code.code(),
        ),
    }
}
fn map_platform(code: PlatformFailure, family: SourceFamily) -> Mapping {
    use PlatformFailure::*;
    match code {
        UnsupportedPlatform => m(
            InstallerErrorCategory::UnsupportedPlatform,
            ErrorStage::Preflight,
            UserRetryPolicy::NotRetryable,
            RecommendedAction::ChooseSupportedSystem,
            family,
            "UNSUPPORTED_PLATFORM",
        ),
        UnsupportedArchitecture => m(
            InstallerErrorCategory::UnsupportedArchitecture,
            ErrorStage::Preflight,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedArtifact,
            family,
            "UNSUPPORTED_ARCHITECTURE",
        ),
        InsufficientDiskSpace => m(
            InstallerErrorCategory::InsufficientDiskSpace,
            ErrorStage::Preflight,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::FreeDiskSpace,
            family,
            "INSUFFICIENT_DISK_SPACE",
        ),
        NativePackageTransactionFailed => m(
            InstallerErrorCategory::PackageInstallFailed,
            ErrorStage::Install,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::UseNativePackageRecovery,
            family,
            "NATIVE_PACKAGE_TRANSACTION_FAILED",
        ),
        RuntimeDependencyFailed => m(
            InstallerErrorCategory::RuntimeDependencyFailed,
            ErrorStage::VerifyInstallation,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
            family,
            "RUNTIME_DEPENDENCY_FAILED",
        ),
        IntegrationFailed => m(
            InstallerErrorCategory::IntegrationFailed,
            ErrorStage::Integrate,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
            family,
            "INTEGRATION_FAILED",
        ),
        LaunchFailed => m(
            InstallerErrorCategory::VerificationFailed,
            ErrorStage::Complete,
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
            family,
            "LAUNCH_FAILED",
        ),
    }
}

const fn m(
    c: InstallerErrorCategory,
    s: ErrorStage,
    r: UserRetryPolicy,
    a: RecommendedAction,
    f: SourceFamily,
    code: &'static str,
) -> Mapping {
    (c, s, r, Some(a), f, code)
}
const fn recovery(code: &'static str, f: SourceFamily) -> Mapping {
    m(
        InstallerErrorCategory::RecoveryRequired,
        ErrorStage::Recovery,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
        f,
        code,
    )
}
const fn concurrent(code: &'static str, f: SourceFamily) -> Mapping {
    m(
        InstallerErrorCategory::ConcurrentOperation,
        ErrorStage::Recovery,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::CloseOtherInstaller,
        f,
        code,
    )
}
const fn protected(code: &'static str, f: SourceFamily) -> Mapping {
    m(
        InstallerErrorCategory::ProtectedStateBlocked,
        ErrorStage::LifecyclePolicy,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
        f,
        code,
    )
}
const fn unsupported(code: &'static str) -> Mapping {
    m(
        InstallerErrorCategory::UnsupportedLifecycle,
        ErrorStage::LifecyclePolicy,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::UseSupportedVersion,
        SourceFamily::Lifecycle,
        code,
    )
}
const fn invalid_lifecycle(code: &'static str) -> Mapping {
    m(
        InstallerErrorCategory::InvalidSetupState,
        ErrorStage::LifecyclePolicy,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
        SourceFamily::Lifecycle,
        code,
    )
}
fn build_mapping(x: Mapping, c: &FailureContext) -> UserFacingError {
    build(x.0, x.1, x.2, x.3, x.4, x.5, c)
}
fn build(
    category: InstallerErrorCategory,
    stage: ErrorStage,
    retry_policy: UserRetryPolicy,
    primary_action: Option<RecommendedAction>,
    source_family: SourceFamily,
    source_code: &'static str,
    c: &FailureContext,
) -> UserFacingError {
    let family = family_code(source_family);
    let diagnostic = InstallerDiagnostic {
        support_reference: SafeDiagnosticIdentifier::new(&format!("SVE-{family}-{source_code}"))
            .expect("finite support reference"),
        category,
        stage,
        source_family,
        source_code: SafeDiagnosticIdentifier::new(source_code).expect("finite source code"),
        intent: c.intent,
        target_scope: c.target_scope,
        resource_class: c.resource_class,
        effect_id: c.effect_id.clone(),
        artifact_id: c.artifact_id.clone(),
        recovery_disposition: c.recovery_disposition.map(recovery_code).map(str::to_owned),
    };
    let value = UserFacingError {
        schema_version: ERROR_MODEL_SCHEMA_VERSION,
        category,
        stage,
        message_key: message_key(category).to_owned(),
        retry_policy,
        primary_action,
        secondary_actions: vec![RecommendedAction::ShowDetails],
        diagnostic,
    };
    debug_assert!(value.validate().is_ok());
    value
}
const fn family_code(f: SourceFamily) -> &'static str {
    match f {
        SourceFamily::Engine => "ENGINE",
        SourceFamily::Journal => "JOURNAL",
        SourceFamily::Recovery => "RECOVERY",
        SourceFamily::Lifecycle => "LIFECYCLE",
        SourceFamily::Purge => "PURGE",
        SourceFamily::Acquisition => "ACQ",
        SourceFamily::Preflight => "PREFLIGHT",
        SourceFamily::Platform => "PLATFORM",
    }
}
fn known_source_code(code: &str) -> bool {
    AcquisitionFailureCode::try_from(code).is_ok()
        || matches!(
            code,
            "UNKNOWN"
                | "INVALID_PLAN"
                | "UNSUPPORTED_SCHEMA"
                | "UNAUTHORIZED_OWNERSHIP"
                | "PRIVILEGE_UNAVAILABLE"
                | "PLAN_STALE"
                | "EFFECT_FAILED"
                | "VERIFICATION_FAILED"
                | "OUTCOME_UNKNOWN"
                | "RECONCILIATION_REQUIRED"
                | "COMPENSATION_FAILED"
                | "FINAL_VERIFICATION_FAILED"
                | "PREFLIGHT_BLOCKED"
                | "MISSING"
                | "BUSY"
                | "CORRUPT"
                | "PLAN_MISMATCH"
                | "IO_FAILED"
                | "LIMIT_EXCEEDED"
                | "ACTIVE_TRANSACTION"
                | "RECOVERY_INSPECTION_REQUIRED"
                | "REPLAN_REQUIRED"
                | "INSPECTION_REQUIRED"
                | "STILL_UNKNOWN"
                | "JOURNAL_BUSY"
                | "JOURNAL_CORRUPT"
                | "INTENT_MISMATCH"
                | "UNKNOWN_STATE"
                | "UNSUPPORTED_SOURCE"
                | "UNSUPPORTED_TARGET"
                | "DOWNGRADE_REJECTED"
                | "INTERMEDIATE_VERSION_REQUIRED"
                | "ARTIFACT_IDENTITY_MISMATCH"
                | "PROTECTED_MUTATION"
                | "UNKNOWN_OWNERSHIP"
                | "INVALID_OPERATION"
                | "STARTUP_PREFERENCE_CHANGED"
                | "PRESERVATION_UNKNOWN"
                | "PRESERVATION_CHANGED"
                | "MISSING_INTENT"
                | "MISSING_AUTHORIZATION"
                | "MISSING_SCOPE"
                | "UNAUTHORIZED_TARGET"
                | "UNSAFE_CONTAINMENT"
                | "SYMLINK_REJECTED"
                | "INSUFFICIENT_DISK_SPACE"
                | "NATIVE_PACKAGE_TRANSACTION_FAILED"
                | "RUNTIME_DEPENDENCY_FAILED"
                | "INTEGRATION_FAILED"
                | "LAUNCH_FAILED"
        )
}
const fn recovery_code(x: RecoveryDisposition) -> &'static str {
    match x {
        RecoveryDisposition::Fresh => "FRESH",
        RecoveryDisposition::ResumeReady => "RESUME_READY",
        RecoveryDisposition::ReconciledApplied => "RECONCILED_APPLIED",
        RecoveryDisposition::ReplanRequired => "REPLAN_REQUIRED",
        RecoveryDisposition::InspectionRequired => "INSPECTION_REQUIRED",
        RecoveryDisposition::AlreadyCompleted => "ALREADY_COMPLETED",
        RecoveryDisposition::StillUnknown => "STILL_UNKNOWN",
        RecoveryDisposition::JournalBusy => "JOURNAL_BUSY",
        RecoveryDisposition::JournalCorrupt => "JOURNAL_CORRUPT",
    }
}
pub const fn message_key(c: InstallerErrorCategory) -> &'static str {
    match c {
        InstallerErrorCategory::UnsupportedPlatform => "installer.error.unsupported_platform",
        InstallerErrorCategory::UnsupportedArchitecture => {
            "installer.error.unsupported_architecture"
        }
        InstallerErrorCategory::ArtifactUnavailable => "installer.error.artifact_unavailable",
        InstallerErrorCategory::InsufficientDiskSpace => "installer.error.insufficient_disk_space",
        InstallerErrorCategory::PermissionRequired => "installer.error.permission_required",
        InstallerErrorCategory::DownloadFailed => "installer.error.download_failed",
        InstallerErrorCategory::IntegrityVerificationFailed => {
            "installer.error.integrity_verification_failed"
        }
        InstallerErrorCategory::StoragePreparationFailed => {
            "installer.error.storage_preparation_failed"
        }
        InstallerErrorCategory::PackageInstallFailed => "installer.error.package_install_failed",
        InstallerErrorCategory::RuntimeDependencyFailed => {
            "installer.error.runtime_dependency_failed"
        }
        InstallerErrorCategory::IntegrationFailed => "installer.error.integration_failed",
        InstallerErrorCategory::VerificationFailed => "installer.error.verification_failed",
        InstallerErrorCategory::Interrupted => "installer.error.interrupted",
        InstallerErrorCategory::RecoveryRequired => "installer.error.recovery_required",
        InstallerErrorCategory::ConcurrentOperation => "installer.error.concurrent_operation",
        InstallerErrorCategory::UnsupportedLifecycle => "installer.error.unsupported_lifecycle",
        InstallerErrorCategory::ProtectedStateBlocked => "installer.error.protected_state_blocked",
        InstallerErrorCategory::InvalidSetupState => "installer.error.invalid_setup_state",
        InstallerErrorCategory::InternalFailure => "installer.error.internal_failure",
    }
}
