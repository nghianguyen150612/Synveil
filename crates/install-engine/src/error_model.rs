//! Finite, fail-closed installer error and diagnostics presentation contract.

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
pub enum FailureSourceFamily {
    Engine,
    Journal,
    Recovery,
    Lifecycle,
    Purge,
    Acquisition,
    Preflight,
    Platform,
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
pub enum PresentationStage {
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
pub enum PlatformFailureCode {
    UnsupportedPlatform,
    UnsupportedArchitecture,
    InsufficientDiskSpace,
    NativePackageTransactionFailed,
    RuntimeDependencyFailed,
    IntegrationFailed,
    LaunchFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

    pub const fn as_code(self) -> &'static str {
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
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "AMBIGUOUS_ARTIFACT" => Ok(Self::AmbiguousArtifact),
            "ARTIFACT_DIGEST_MISMATCH" => Ok(Self::ArtifactDigestMismatch),
            "ARTIFACT_TOO_LARGE" => Ok(Self::ArtifactTooLarge),
            "ARTIFACT_TRUNCATED" => Ok(Self::ArtifactTruncated),
            "DESTINATION_CONFLICT" => Ok(Self::DestinationConflict),
            "INVALID_MANIFEST" => Ok(Self::InvalidManifest),
            "MANIFEST_AUTH_FAILED" => Ok(Self::ManifestAuthFailed),
            "MANIFEST_TOO_LARGE" => Ok(Self::ManifestTooLarge),
            "NETWORK_ERROR" => Ok(Self::NetworkError),
            "NO_MATCHING_ARTIFACT" => Ok(Self::NoMatchingArtifact),
            "SOURCE_COMMIT_MISMATCH" => Ok(Self::SourceCommitMismatch),
            "STAGING_ERROR" => Ok(Self::StagingError),
            "UNSAFE_PATH" => Ok(Self::UnsafePath),
            "UNSAFE_REDIRECT" => Ok(Self::UnsafeRedirect),
            "UNSUPPORTED_ARCHITECTURE" => Ok(Self::UnsupportedArchitecture),
            "UNSUPPORTED_AUTHENTICATION" => Ok(Self::UnsupportedAuthentication),
            "UNSUPPORTED_PLATFORM" => Ok(Self::UnsupportedPlatform),
            "UNTRUSTED_ORIGIN" => Ok(Self::UntrustedOrigin),
            "VERSION_MISMATCH" => Ok(Self::VersionMismatch),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureContext {
    pub stage: PresentationStage,
    pub intent: Option<InstallationIntent>,
    pub target_scope: Option<TargetScope>,
    pub resource_class: Option<ResourceClass>,
    pub effect_id: Option<String>,
    pub artifact_id: Option<String>,
    pub preflight_reason: Option<PlatformFailureCode>,
    pub recovery_disposition: Option<RecoveryDisposition>,
}

impl FailureContext {
    pub fn new(stage: PresentationStage) -> Self {
        Self {
            stage,
            intent: None,
            target_scope: None,
            resource_class: None,
            effect_id: None,
            artifact_id: None,
            preflight_reason: None,
            recovery_disposition: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallerDiagnostic {
    support_reference: String,
    category: InstallerErrorCategory,
    stage: PresentationStage,
    source_family: FailureSourceFamily,
    source_code: String,
    intent: Option<InstallationIntent>,
    target_scope: Option<TargetScope>,
    resource_class: Option<ResourceClass>,
    effect_id: Option<String>,
    artifact_id: Option<String>,
    recovery_disposition: Option<String>,
}

impl InstallerDiagnostic {
    pub fn support_reference(&self) -> &str {
        &self.support_reference
    }
    pub fn source_family(&self) -> FailureSourceFamily {
        self.source_family
    }
    pub fn source_code(&self) -> &str {
        &self.source_code
    }
    pub fn effect_id(&self) -> Option<&str> {
        self.effect_id.as_deref()
    }
    pub fn artifact_id(&self) -> Option<&str> {
        self.artifact_id.as_deref()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserFacingError {
    schema_version: u32,
    category: InstallerErrorCategory,
    stage: PresentationStage,
    message_key: String,
    retry_policy: UserRetryPolicy,
    primary_action: RecommendedAction,
    secondary_actions: Vec<RecommendedAction>,
    diagnostic: InstallerDiagnostic,
}

impl UserFacingError {
    pub fn category(&self) -> InstallerErrorCategory {
        self.category
    }
    pub fn stage(&self) -> PresentationStage {
        self.stage
    }
    pub fn message_key(&self) -> &str {
        &self.message_key
    }
    pub fn retry_policy(&self) -> UserRetryPolicy {
        self.retry_policy
    }
    pub fn primary_action(&self) -> RecommendedAction {
        self.primary_action
    }
    pub fn secondary_actions(&self) -> &[RecommendedAction] {
        &self.secondary_actions
    }
    pub fn diagnostic(&self) -> &InstallerDiagnostic {
        &self.diagnostic
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(input: &str) -> Result<Self, ErrorModelValidationError> {
        let value: Self =
            serde_json::from_str(input).map_err(|_| ErrorModelValidationError::InvalidEncoding)?;
        validate_user_facing_error(&value)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorModelValidationError {
    UnsupportedSchema,
    InvalidEncoding,
    InvalidMessageKey,
    InvalidRetryPolicy,
    InvalidAction,
    TooManySecondaryActions,
    UnsafeIdentifier,
    UnsafeSourceCode,
    InvalidSupportReference,
    DiagnosticTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceCopy {
    pub category: InstallerErrorCategory,
    pub message_key: &'static str,
    pub english: &'static str,
    pub vietnamese: &'static str,
}

pub const REFERENCE_COPY: [ReferenceCopy; 19] = [
    ReferenceCopy {
        category: InstallerErrorCategory::UnsupportedPlatform,
        message_key: "installer.error.unsupported_platform",
        english: "This operating system is not supported for this Synveil installer.",
        vietnamese: "Hệ điều hành này chưa được hỗ trợ bởi bộ cài Synveil này.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::UnsupportedArchitecture,
        message_key: "installer.error.unsupported_architecture",
        english: "This Synveil installer does not match this computer.",
        vietnamese: "Bộ cài Synveil này không phù hợp với kiến trúc của máy.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::ArtifactUnavailable,
        message_key: "installer.error.artifact_unavailable",
        english: "A compatible Synveil package is not available for this setup.",
        vietnamese: "Hiện không có gói Synveil tương thích với cấu hình cài đặt này.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::InsufficientDiskSpace,
        message_key: "installer.error.insufficient_disk_space",
        english: "There is not enough free space to continue installation.",
        vietnamese: "Không đủ dung lượng trống để tiếp tục cài đặt.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::PermissionRequired,
        message_key: "installer.error.permission_required",
        english: "Synveil needs permission from this computer to continue this step.",
        vietnamese: "Synveil cần quyền từ máy tính để tiếp tục bước này.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::DownloadFailed,
        message_key: "installer.error.download_failed",
        english: "The Synveil download did not complete. Check the connection and try the download again.",
        vietnamese: "Tải Synveil chưa hoàn tất. Hãy kiểm tra kết nối rồi thử tải lại.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::IntegrityVerificationFailed,
        message_key: "installer.error.integrity_verification_failed",
        english: "Synveil could not verify this download. Get a fresh official download before continuing.",
        vietnamese: "Synveil không thể xác minh bản tải xuống này. Hãy lấy bản tải chính thức mới trước khi tiếp tục.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::StoragePreparationFailed,
        message_key: "installer.error.storage_preparation_failed",
        english: "Synveil could not safely prepare the installation files.",
        vietnamese: "Synveil không thể chuẩn bị tệp cài đặt một cách an toàn.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::PackageInstallFailed,
        message_key: "installer.error.package_install_failed",
        english: "The Synveil package could not be installed safely.",
        vietnamese: "Gói Synveil không thể được cài đặt an toàn.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::RuntimeDependencyFailed,
        message_key: "installer.error.runtime_dependency_failed",
        english: "A required Synveil component is unavailable. Repair Synveil to continue.",
        vietnamese: "Một thành phần cần thiết của Synveil chưa sẵn sàng. Hãy sửa chữa Synveil để tiếp tục.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::IntegrationFailed,
        message_key: "installer.error.integration_failed",
        english: "Synveil was installed but could not finish computer integration.",
        vietnamese: "Synveil đã được cài nhưng chưa thể hoàn tất tích hợp với máy tính.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::VerificationFailed,
        message_key: "installer.error.verification_failed",
        english: "Synveil could not verify that installation completed correctly. Repair Synveil before using it.",
        vietnamese: "Synveil không thể xác minh việc cài đặt đã hoàn tất đúng cách. Hãy sửa chữa Synveil trước khi sử dụng.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::Interrupted,
        message_key: "installer.error.interrupted",
        english: "Setup was interrupted and needs recovery before continuing.",
        vietnamese: "Quá trình thiết lập đã bị gián đoạn và cần được phục hồi trước khi tiếp tục.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::RecoveryRequired,
        message_key: "installer.error.recovery_required",
        english: "Synveil needs to check the previous setup state before anything is repeated.",
        vietnamese: "Synveil cần kiểm tra trạng thái thiết lập trước đó trước khi lặp lại bất kỳ thao tác nào.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::ConcurrentOperation,
        message_key: "installer.error.concurrent_operation",
        english: "Another Synveil setup or recovery operation is already active.",
        vietnamese: "Một thao tác cài đặt hoặc phục hồi Synveil khác đang hoạt động.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::UnsupportedLifecycle,
        message_key: "installer.error.unsupported_lifecycle",
        english: "This install, repair, upgrade, or removal path is not supported for the current state.",
        vietnamese: "Quy trình cài đặt, sửa chữa, nâng cấp hoặc gỡ cài đặt này không được hỗ trợ với trạng thái hiện tại.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::ProtectedStateBlocked,
        message_key: "installer.error.protected_state_blocked",
        english: "Synveil stopped to protect existing data or settings.",
        vietnamese: "Synveil đã dừng để bảo vệ dữ liệu hoặc cài đặt hiện có.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::InvalidSetupState,
        message_key: "installer.error.invalid_setup_state",
        english: "The current setup state cannot be used safely. Replan this operation before continuing.",
        vietnamese: "Trạng thái thiết lập hiện tại không thể được sử dụng an toàn. Hãy lập lại kế hoạch thao tác trước khi tiếp tục.",
    },
    ReferenceCopy {
        category: InstallerErrorCategory::InternalFailure,
        message_key: "installer.error.internal_failure",
        english: "Synveil could not safely continue this setup operation.",
        vietnamese: "Synveil không thể tiếp tục thao tác thiết lập này một cách an toàn.",
    },
];

pub fn reference_copy(category: InstallerErrorCategory) -> &'static ReferenceCopy {
    REFERENCE_COPY
        .iter()
        .find(|copy| copy.category == category)
        .expect("all finite categories have reference copy")
}

pub fn map_engine(error: EngineErrorCode, context: &FailureContext) -> UserFacingError {
    match error {
        EngineErrorCode::InvalidPlan => make_error(
            InstallerErrorCategory::InvalidSetupState,
            context,
            FailureSourceFamily::Engine,
            "INVALID_PLAN",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
        ),
        EngineErrorCode::UnsupportedSchema => {
            unsupported_lifecycle(context, FailureSourceFamily::Engine, "UNSUPPORTED_SCHEMA")
        }
        EngineErrorCode::UnauthorizedOwnership => protected(
            context,
            FailureSourceFamily::Engine,
            "UNAUTHORIZED_OWNERSHIP",
        ),
        EngineErrorCode::PrivilegeUnavailable => make_error(
            InstallerErrorCategory::PermissionRequired,
            context,
            FailureSourceFamily::Engine,
            "PRIVILEGE_UNAVAILABLE",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RequestPermission,
        ),
        EngineErrorCode::PlanStale => make_error(
            InstallerErrorCategory::InvalidSetupState,
            context,
            FailureSourceFamily::Engine,
            "PLAN_STALE",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
        ),
        EngineErrorCode::EffectFailed => match (context.stage, context.resource_class) {
            (PresentationStage::Install, Some(ResourceClass::PackageOwned))
            | (PresentationStage::Install, Some(ResourceClass::NativePackageState)) => make_error(
                InstallerErrorCategory::PackageInstallFailed,
                context,
                FailureSourceFamily::Engine,
                "EFFECT_FAILED",
                UserRetryPolicy::AfterUserAction,
                RecommendedAction::RepairInstallation,
            ),
            (PresentationStage::Integrate, Some(ResourceClass::PlatformIntegrationOwned)) => {
                make_error(
                    InstallerErrorCategory::IntegrationFailed,
                    context,
                    FailureSourceFamily::Engine,
                    "EFFECT_FAILED",
                    UserRetryPolicy::AfterUserAction,
                    RecommendedAction::RepairInstallation,
                )
            }
            _ => internal(context, FailureSourceFamily::Engine, "EFFECT_FAILED"),
        },
        EngineErrorCode::VerificationFailed => {
            verification(context, FailureSourceFamily::Engine, "VERIFICATION_FAILED")
        }
        EngineErrorCode::OutcomeUnknown => recovery(
            context,
            FailureSourceFamily::Engine,
            "OUTCOME_UNKNOWN",
            RecommendedAction::InspectRecovery,
        ),
        EngineErrorCode::ReconciliationRequired => recovery(
            context,
            FailureSourceFamily::Engine,
            "RECONCILIATION_REQUIRED",
            RecommendedAction::InspectRecovery,
        ),
        EngineErrorCode::CompensationFailed => recovery(
            context,
            FailureSourceFamily::Engine,
            "COMPENSATION_FAILED",
            RecommendedAction::InspectRecovery,
        ),
        EngineErrorCode::FinalVerificationFailed => verification(
            context,
            FailureSourceFamily::Engine,
            "FINAL_VERIFICATION_FAILED",
        ),
        EngineErrorCode::PreflightBlocked => match context.preflight_reason {
            Some(reason) => {
                map_platform_with_family(reason, context, FailureSourceFamily::Preflight)
            }
            None => internal(context, FailureSourceFamily::Engine, "PREFLIGHT_BLOCKED"),
        },
    }
}

pub fn map_journal(error: JournalErrorCode, context: &FailureContext) -> UserFacingError {
    match error {
        JournalErrorCode::JournalMissing => recovery(
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_MISSING",
            RecommendedAction::InspectRecovery,
        ),
        JournalErrorCode::JournalBusy => concurrent(
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_BUSY",
            RecommendedAction::CloseOtherInstaller,
        ),
        JournalErrorCode::JournalCorrupt => recovery(
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_CORRUPT",
            RecommendedAction::InspectRecovery,
        ),
        JournalErrorCode::JournalUnsupportedSchema => unsupported_lifecycle(
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_UNSUPPORTED_SCHEMA",
        ),
        JournalErrorCode::JournalPlanMismatch => recovery(
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_PLAN_MISMATCH",
            RecommendedAction::Replan,
        ),
        JournalErrorCode::JournalIoFailed => make_error(
            InstallerErrorCategory::Interrupted,
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_IO_FAILED",
            UserRetryPolicy::ReconcileFirst,
            RecommendedAction::InspectRecovery,
        ),
        JournalErrorCode::JournalLimitExceeded => recovery(
            context,
            FailureSourceFamily::Journal,
            "JOURNAL_LIMIT_EXCEEDED",
            RecommendedAction::InspectRecovery,
        ),
        JournalErrorCode::ActiveTransactionExists => concurrent(
            context,
            FailureSourceFamily::Journal,
            "ACTIVE_TRANSACTION_EXISTS",
            RecommendedAction::ResumeRecovery,
        ),
        JournalErrorCode::RecoveryInspectionRequired => recovery(
            context,
            FailureSourceFamily::Journal,
            "RECOVERY_INSPECTION_REQUIRED",
            RecommendedAction::InspectRecovery,
        ),
    }
}

pub fn map_recovery(
    disposition: RecoveryDisposition,
    context: &FailureContext,
) -> Option<UserFacingError> {
    match disposition {
        RecoveryDisposition::Fresh
        | RecoveryDisposition::ResumeReady
        | RecoveryDisposition::ReconciledApplied
        | RecoveryDisposition::AlreadyCompleted => None,
        RecoveryDisposition::ReplanRequired => Some(make_error(
            InstallerErrorCategory::InvalidSetupState,
            context,
            FailureSourceFamily::Recovery,
            "REPLAN_REQUIRED",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
        )),
        RecoveryDisposition::InspectionRequired => Some(recovery(
            context,
            FailureSourceFamily::Recovery,
            "INSPECTION_REQUIRED",
            RecommendedAction::InspectRecovery,
        )),
        RecoveryDisposition::StillUnknown => Some(recovery(
            context,
            FailureSourceFamily::Recovery,
            "STILL_UNKNOWN",
            RecommendedAction::InspectRecovery,
        )),
        RecoveryDisposition::JournalBusy => Some(concurrent(
            context,
            FailureSourceFamily::Recovery,
            "JOURNAL_BUSY",
            RecommendedAction::CloseOtherInstaller,
        )),
        RecoveryDisposition::JournalCorrupt => Some(recovery(
            context,
            FailureSourceFamily::Recovery,
            "JOURNAL_CORRUPT",
            RecommendedAction::InspectRecovery,
        )),
    }
}

pub fn map_lifecycle(error: LifecycleError, context: &FailureContext) -> UserFacingError {
    match error {
        LifecycleError::IntentMismatch => make_error(
            InstallerErrorCategory::InvalidSetupState,
            context,
            FailureSourceFamily::Lifecycle,
            "INTENT_MISMATCH",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
        ),
        LifecycleError::ActiveTransaction => concurrent(
            context,
            FailureSourceFamily::Lifecycle,
            "ACTIVE_TRANSACTION",
            RecommendedAction::ResumeRecovery,
        ),
        LifecycleError::UnknownState => recovery(
            context,
            FailureSourceFamily::Lifecycle,
            "UNKNOWN_STATE",
            RecommendedAction::InspectRecovery,
        ),
        LifecycleError::UnsupportedSource => unsupported_lifecycle(
            context,
            FailureSourceFamily::Lifecycle,
            "UNSUPPORTED_SOURCE",
        ),
        LifecycleError::UnsupportedTarget => unsupported_lifecycle(
            context,
            FailureSourceFamily::Lifecycle,
            "UNSUPPORTED_TARGET",
        ),
        LifecycleError::DowngradeRejected => unsupported_lifecycle(
            context,
            FailureSourceFamily::Lifecycle,
            "DOWNGRADE_REJECTED",
        ),
        LifecycleError::IntermediateVersionRequired => unsupported_lifecycle(
            context,
            FailureSourceFamily::Lifecycle,
            "INTERMEDIATE_VERSION_REQUIRED",
        ),
        LifecycleError::ArtifactIdentityMismatch => integrity(
            context,
            FailureSourceFamily::Lifecycle,
            "ARTIFACT_IDENTITY_MISMATCH",
        ),
        LifecycleError::VersionMismatch => {
            unsupported_lifecycle(context, FailureSourceFamily::Lifecycle, "VERSION_MISMATCH")
        }
        LifecycleError::ProtectedMutation => protected(
            context,
            FailureSourceFamily::Lifecycle,
            "PROTECTED_MUTATION",
        ),
        LifecycleError::UnknownOwnership => {
            protected(context, FailureSourceFamily::Lifecycle, "UNKNOWN_OWNERSHIP")
        }
        LifecycleError::InvalidOperation => make_error(
            InstallerErrorCategory::InvalidSetupState,
            context,
            FailureSourceFamily::Lifecycle,
            "INVALID_OPERATION",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::Replan,
        ),
        LifecycleError::StartupPreferenceChanged => protected(
            context,
            FailureSourceFamily::Lifecycle,
            "STARTUP_PREFERENCE_CHANGED",
        ),
        LifecycleError::PreservationUnknown => protected(
            context,
            FailureSourceFamily::Lifecycle,
            "PRESERVATION_UNKNOWN",
        ),
        LifecycleError::PreservationChanged => protected(
            context,
            FailureSourceFamily::Lifecycle,
            "PRESERVATION_CHANGED",
        ),
    }
}

pub fn map_purge(error: PurgeError, context: &FailureContext) -> UserFacingError {
    let code = match error {
        PurgeError::UnsupportedSchema => "UNSUPPORTED_SCHEMA",
        PurgeError::MissingIntent => "MISSING_INTENT",
        PurgeError::MissingAuthorization => "MISSING_AUTHORIZATION",
        PurgeError::MissingScope => "MISSING_SCOPE",
        PurgeError::UnauthorizedTarget => "UNAUTHORIZED_TARGET",
        PurgeError::UnknownOwnership => "UNKNOWN_OWNERSHIP",
        PurgeError::UnsafeContainment => "UNSAFE_CONTAINMENT",
        PurgeError::SymlinkRejected => "SYMLINK_REJECTED",
    };
    protected(context, FailureSourceFamily::Purge, code)
}

pub fn map_acquisition(code: AcquisitionFailureCode, context: &FailureContext) -> UserFacingError {
    match code {
        AcquisitionFailureCode::NetworkError => make_error(
            InstallerErrorCategory::DownloadFailed,
            context,
            FailureSourceFamily::Acquisition,
            code.as_code(),
            UserRetryPolicy::SafeImmediate,
            RecommendedAction::RetryDownload,
        ),
        AcquisitionFailureCode::UnsupportedPlatform => make_error(
            InstallerErrorCategory::UnsupportedPlatform,
            context,
            FailureSourceFamily::Acquisition,
            code.as_code(),
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedSystem,
        ),
        AcquisitionFailureCode::UnsupportedArchitecture => make_error(
            InstallerErrorCategory::UnsupportedArchitecture,
            context,
            FailureSourceFamily::Acquisition,
            code.as_code(),
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedArtifact,
        ),
        AcquisitionFailureCode::NoMatchingArtifact => make_error(
            InstallerErrorCategory::ArtifactUnavailable,
            context,
            FailureSourceFamily::Acquisition,
            code.as_code(),
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedArtifact,
        ),
        AcquisitionFailureCode::DestinationConflict
        | AcquisitionFailureCode::UnsafePath
        | AcquisitionFailureCode::StagingError => make_error(
            InstallerErrorCategory::StoragePreparationFailed,
            context,
            FailureSourceFamily::Acquisition,
            code.as_code(),
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseDifferentDestination,
        ),
        AcquisitionFailureCode::AmbiguousArtifact
        | AcquisitionFailureCode::ArtifactDigestMismatch
        | AcquisitionFailureCode::ArtifactTooLarge
        | AcquisitionFailureCode::ArtifactTruncated
        | AcquisitionFailureCode::InvalidManifest
        | AcquisitionFailureCode::ManifestAuthFailed
        | AcquisitionFailureCode::ManifestTooLarge
        | AcquisitionFailureCode::SourceCommitMismatch
        | AcquisitionFailureCode::UnsafeRedirect
        | AcquisitionFailureCode::UnsupportedAuthentication
        | AcquisitionFailureCode::UntrustedOrigin
        | AcquisitionFailureCode::VersionMismatch => {
            integrity(context, FailureSourceFamily::Acquisition, code.as_code())
        }
    }
}

pub fn map_acquisition_code(raw_code: &str, context: &FailureContext) -> UserFacingError {
    match AcquisitionFailureCode::try_from(raw_code) {
        Ok(code) => map_acquisition(code, context),
        Err(()) => internal(context, FailureSourceFamily::Acquisition, "UNKNOWN_CODE"),
    }
}

pub fn map_platform(code: PlatformFailureCode, context: &FailureContext) -> UserFacingError {
    map_platform_with_family(code, context, FailureSourceFamily::Platform)
}

fn map_platform_with_family(
    code: PlatformFailureCode,
    context: &FailureContext,
    family: FailureSourceFamily,
) -> UserFacingError {
    match code {
        PlatformFailureCode::UnsupportedPlatform => make_error(
            InstallerErrorCategory::UnsupportedPlatform,
            context,
            family,
            "UNSUPPORTED_PLATFORM",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedSystem,
        ),
        PlatformFailureCode::UnsupportedArchitecture => make_error(
            InstallerErrorCategory::UnsupportedArchitecture,
            context,
            family,
            "UNSUPPORTED_ARCHITECTURE",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::ChooseSupportedArtifact,
        ),
        PlatformFailureCode::InsufficientDiskSpace => make_error(
            InstallerErrorCategory::InsufficientDiskSpace,
            context,
            family,
            "INSUFFICIENT_DISK_SPACE",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::FreeDiskSpace,
        ),
        PlatformFailureCode::NativePackageTransactionFailed => make_error(
            InstallerErrorCategory::PackageInstallFailed,
            context,
            family,
            "NATIVE_PACKAGE_TRANSACTION_FAILED",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::UseNativePackageRecovery,
        ),
        PlatformFailureCode::RuntimeDependencyFailed => make_error(
            InstallerErrorCategory::RuntimeDependencyFailed,
            context,
            family,
            "RUNTIME_DEPENDENCY_FAILED",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
        ),
        PlatformFailureCode::IntegrationFailed => make_error(
            InstallerErrorCategory::IntegrationFailed,
            context,
            family,
            "INTEGRATION_FAILED",
            UserRetryPolicy::AfterUserAction,
            RecommendedAction::RepairInstallation,
        ),
        PlatformFailureCode::LaunchFailed => verification(context, family, "LAUNCH_FAILED"),
    }
}

pub fn validate_user_facing_error(
    error: &UserFacingError,
) -> Result<(), ErrorModelValidationError> {
    if error.schema_version != ERROR_MODEL_SCHEMA_VERSION {
        return Err(ErrorModelValidationError::UnsupportedSchema);
    }
    if error.message_key != message_key(error.category) {
        return Err(ErrorModelValidationError::InvalidMessageKey);
    }
    if error.secondary_actions.len() > MAX_SECONDARY_ACTIONS {
        return Err(ErrorModelValidationError::TooManySecondaryActions);
    }
    if error.secondary_actions.contains(&error.primary_action) {
        return Err(ErrorModelValidationError::InvalidAction);
    }
    validate_diagnostic(&error.diagnostic)?;

    match error.category {
        InstallerErrorCategory::DownloadFailed => {
            if error.retry_policy != UserRetryPolicy::SafeImmediate
                || error.primary_action != RecommendedAction::RetryDownload
            {
                return Err(ErrorModelValidationError::InvalidRetryPolicy);
            }
        }
        InstallerErrorCategory::IntegrityVerificationFailed => {
            if error.retry_policy != UserRetryPolicy::FreshEvidenceRequired
                || error.primary_action != RecommendedAction::GetFreshOfficialDownload
            {
                return Err(ErrorModelValidationError::InvalidRetryPolicy);
            }
        }
        InstallerErrorCategory::RecoveryRequired => {
            if error.retry_policy != UserRetryPolicy::ReconcileFirst
                || !matches!(
                    error.primary_action,
                    RecommendedAction::InspectRecovery
                        | RecommendedAction::ResumeRecovery
                        | RecommendedAction::Replan
                )
            {
                return Err(ErrorModelValidationError::InvalidRetryPolicy);
            }
        }
        InstallerErrorCategory::ProtectedStateBlocked => {
            if matches!(
                error.retry_policy,
                UserRetryPolicy::SafeImmediate | UserRetryPolicy::FreshEvidenceRequired
            ) {
                return Err(ErrorModelValidationError::InvalidRetryPolicy);
            }
        }
        InstallerErrorCategory::ConcurrentOperation => {
            if error.retry_policy != UserRetryPolicy::AfterUserAction
                || !matches!(
                    error.primary_action,
                    RecommendedAction::CloseOtherInstaller | RecommendedAction::ResumeRecovery
                )
            {
                return Err(ErrorModelValidationError::InvalidRetryPolicy);
            }
        }
        _ => {
            if error.retry_policy == UserRetryPolicy::SafeImmediate {
                return Err(ErrorModelValidationError::InvalidRetryPolicy);
            }
        }
    }
    Ok(())
}

fn validate_diagnostic(diagnostic: &InstallerDiagnostic) -> Result<(), ErrorModelValidationError> {
    if diagnostic.source_code.len() > MAX_SAFE_CODE_BYTES
        || !is_safe_code(&diagnostic.source_code)
        || !source_code_allowed(diagnostic.source_family, &diagnostic.source_code)
    {
        return Err(ErrorModelValidationError::UnsafeSourceCode);
    }
    let expected = support_reference(diagnostic.source_family, &diagnostic.source_code);
    if diagnostic.support_reference != expected {
        return Err(ErrorModelValidationError::InvalidSupportReference);
    }
    for value in [&diagnostic.effect_id, &diagnostic.artifact_id]
        .into_iter()
        .flatten()
    {
        if !is_safe_identifier(value) {
            return Err(ErrorModelValidationError::UnsafeIdentifier);
        }
    }
    let size = serde_json::to_vec(diagnostic)
        .map_err(|_| ErrorModelValidationError::InvalidEncoding)?
        .len();
    if size > MAX_DIAGNOSTIC_SERIALIZED_BYTES {
        return Err(ErrorModelValidationError::DiagnosticTooLarge);
    }
    Ok(())
}

fn make_error(
    category: InstallerErrorCategory,
    context: &FailureContext,
    family: FailureSourceFamily,
    source_code: &str,
    retry_policy: UserRetryPolicy,
    primary_action: RecommendedAction,
) -> UserFacingError {
    let message_key = message_key(category).to_owned();
    let diagnostic = InstallerDiagnostic {
        support_reference: support_reference(family, source_code),
        category,
        stage: context.stage,
        source_family: family,
        source_code: source_code.to_owned(),
        intent: context.intent,
        target_scope: context.target_scope,
        resource_class: context.resource_class,
        effect_id: context.effect_id.as_deref().and_then(safe_identifier),
        artifact_id: context.artifact_id.as_deref().and_then(safe_identifier),
        recovery_disposition: context
            .recovery_disposition
            .map(recovery_code)
            .map(str::to_owned),
    };
    UserFacingError {
        schema_version: ERROR_MODEL_SCHEMA_VERSION,
        category,
        stage: context.stage,
        message_key,
        retry_policy,
        primary_action,
        secondary_actions: vec![RecommendedAction::ShowDetails],
        diagnostic,
    }
}

fn integrity(context: &FailureContext, family: FailureSourceFamily, code: &str) -> UserFacingError {
    make_error(
        InstallerErrorCategory::IntegrityVerificationFailed,
        context,
        family,
        code,
        UserRetryPolicy::FreshEvidenceRequired,
        RecommendedAction::GetFreshOfficialDownload,
    )
}

fn recovery(
    context: &FailureContext,
    family: FailureSourceFamily,
    code: &str,
    action: RecommendedAction,
) -> UserFacingError {
    make_error(
        InstallerErrorCategory::RecoveryRequired,
        context,
        family,
        code,
        UserRetryPolicy::ReconcileFirst,
        action,
    )
}

fn concurrent(
    context: &FailureContext,
    family: FailureSourceFamily,
    code: &str,
    action: RecommendedAction,
) -> UserFacingError {
    make_error(
        InstallerErrorCategory::ConcurrentOperation,
        context,
        family,
        code,
        UserRetryPolicy::AfterUserAction,
        action,
    )
}

fn unsupported_lifecycle(
    context: &FailureContext,
    family: FailureSourceFamily,
    code: &str,
) -> UserFacingError {
    make_error(
        InstallerErrorCategory::UnsupportedLifecycle,
        context,
        family,
        code,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::UseSupportedVersion,
    )
}

fn protected(context: &FailureContext, family: FailureSourceFamily, code: &str) -> UserFacingError {
    make_error(
        InstallerErrorCategory::ProtectedStateBlocked,
        context,
        family,
        code,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
    )
}

fn verification(
    context: &FailureContext,
    family: FailureSourceFamily,
    code: &str,
) -> UserFacingError {
    make_error(
        InstallerErrorCategory::VerificationFailed,
        context,
        family,
        code,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::RepairInstallation,
    )
}

fn internal(context: &FailureContext, family: FailureSourceFamily, code: &str) -> UserFacingError {
    make_error(
        InstallerErrorCategory::InternalFailure,
        context,
        family,
        code,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
    )
}

fn message_key(category: InstallerErrorCategory) -> &'static str {
    reference_copy(category).message_key
}

fn safe_identifier(value: &str) -> Option<String> {
    if is_safe_identifier(value) {
        Some(value.to_owned())
    } else {
        None
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SAFE_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
        && !looks_sensitive(value)
}

fn is_safe_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SAFE_CODE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn looks_sensitive(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "bearer",
        "authorization",
        "cookie",
        "privatekey",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn support_reference(family: FailureSourceFamily, code: &str) -> String {
    let family = match family {
        FailureSourceFamily::Engine => "ENGINE",
        FailureSourceFamily::Journal => "JOURNAL",
        FailureSourceFamily::Recovery => "RECOVERY",
        FailureSourceFamily::Lifecycle => "LIFECYCLE",
        FailureSourceFamily::Purge => "PURGE",
        FailureSourceFamily::Acquisition => "ACQ",
        FailureSourceFamily::Preflight => "PREFLIGHT",
        FailureSourceFamily::Platform => "PLATFORM",
    };
    format!("SVE-{family}-{code}")
}

fn recovery_code(disposition: RecoveryDisposition) -> &'static str {
    match disposition {
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

fn source_code_allowed(family: FailureSourceFamily, code: &str) -> bool {
    match family {
        FailureSourceFamily::Engine => matches!(
            code,
            "INVALID_PLAN"
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
        ),
        FailureSourceFamily::Journal => matches!(
            code,
            "JOURNAL_MISSING"
                | "JOURNAL_BUSY"
                | "JOURNAL_CORRUPT"
                | "JOURNAL_UNSUPPORTED_SCHEMA"
                | "JOURNAL_PLAN_MISMATCH"
                | "JOURNAL_IO_FAILED"
                | "JOURNAL_LIMIT_EXCEEDED"
                | "ACTIVE_TRANSACTION_EXISTS"
                | "RECOVERY_INSPECTION_REQUIRED"
        ),
        FailureSourceFamily::Recovery => matches!(
            code,
            "REPLAN_REQUIRED"
                | "INSPECTION_REQUIRED"
                | "STILL_UNKNOWN"
                | "JOURNAL_BUSY"
                | "JOURNAL_CORRUPT"
        ),
        FailureSourceFamily::Lifecycle => matches!(
            code,
            "INTENT_MISMATCH"
                | "ACTIVE_TRANSACTION"
                | "UNKNOWN_STATE"
                | "UNSUPPORTED_SOURCE"
                | "UNSUPPORTED_TARGET"
                | "DOWNGRADE_REJECTED"
                | "INTERMEDIATE_VERSION_REQUIRED"
                | "ARTIFACT_IDENTITY_MISMATCH"
                | "VERSION_MISMATCH"
                | "PROTECTED_MUTATION"
                | "UNKNOWN_OWNERSHIP"
                | "INVALID_OPERATION"
                | "STARTUP_PREFERENCE_CHANGED"
                | "PRESERVATION_UNKNOWN"
                | "PRESERVATION_CHANGED"
        ),
        FailureSourceFamily::Purge => matches!(
            code,
            "UNSUPPORTED_SCHEMA"
                | "MISSING_INTENT"
                | "MISSING_AUTHORIZATION"
                | "MISSING_SCOPE"
                | "UNAUTHORIZED_TARGET"
                | "UNKNOWN_OWNERSHIP"
                | "UNSAFE_CONTAINMENT"
                | "SYMLINK_REJECTED"
        ),
        FailureSourceFamily::Acquisition => {
            code == "UNKNOWN_CODE"
                || AcquisitionFailureCode::ALL
                    .iter()
                    .any(|candidate| candidate.as_code() == code)
        }
        FailureSourceFamily::Preflight | FailureSourceFamily::Platform => matches!(
            code,
            "UNSUPPORTED_PLATFORM"
                | "UNSUPPORTED_ARCHITECTURE"
                | "INSUFFICIENT_DISK_SPACE"
                | "NATIVE_PACKAGE_TRANSACTION_FAILED"
                | "RUNTIME_DEPENDENCY_FAILED"
                | "INTEGRATION_FAILED"
                | "LAUNCH_FAILED"
        ),
    }
}
