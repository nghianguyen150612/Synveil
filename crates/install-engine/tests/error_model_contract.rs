use std::collections::BTreeSet;
use synveil_install_engine::*;

fn ctx(stage: PresentationStage) -> FailureContext {
    FailureContext::new(stage)
}

fn assert_shape(
    error: UserFacingError,
    category: InstallerErrorCategory,
    retry: UserRetryPolicy,
    action: RecommendedAction,
) -> UserFacingError {
    assert_eq!(error.category(), category);
    assert_eq!(error.retry_policy(), retry);
    assert_eq!(error.primary_action(), action);
    assert_eq!(validate_user_facing_error(&error), Ok(()));
    error
}

fn install_package_ctx() -> FailureContext {
    let mut c = ctx(PresentationStage::Install);
    c.resource_class = Some(ResourceClass::PackageOwned);
    c
}

fn integrate_ctx() -> FailureContext {
    let mut c = ctx(PresentationStage::Integrate);
    c.resource_class = Some(ResourceClass::PlatformIntegrationOwned);
    c
}

// 001-015: P007 engine mapping.
#[test]
fn t001_engine_invalid_plan() {
    assert_shape(
        map_engine(EngineErrorCode::InvalidPlan, &ctx(PresentationStage::Plan)),
        InstallerErrorCategory::InvalidSetupState,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::Replan,
    );
}
#[test]
fn t002_engine_unsupported_schema() {
    assert_shape(
        map_engine(
            EngineErrorCode::UnsupportedSchema,
            &ctx(PresentationStage::Plan),
        ),
        InstallerErrorCategory::UnsupportedLifecycle,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::UseSupportedVersion,
    );
}
#[test]
fn t003_engine_unauthorized_ownership() {
    assert_shape(
        map_engine(
            EngineErrorCode::UnauthorizedOwnership,
            &ctx(PresentationStage::Plan),
        ),
        InstallerErrorCategory::ProtectedStateBlocked,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
    );
}
#[test]
fn t004_engine_privilege_unavailable() {
    assert_shape(
        map_engine(
            EngineErrorCode::PrivilegeUnavailable,
            &ctx(PresentationStage::Install),
        ),
        InstallerErrorCategory::PermissionRequired,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::RequestPermission,
    );
}
#[test]
fn t005_engine_plan_stale() {
    assert_shape(
        map_engine(EngineErrorCode::PlanStale, &ctx(PresentationStage::Plan)),
        InstallerErrorCategory::InvalidSetupState,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::Replan,
    );
}
#[test]
fn t006_engine_effect_install() {
    assert_shape(
        map_engine(EngineErrorCode::EffectFailed, &install_package_ctx()),
        InstallerErrorCategory::PackageInstallFailed,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::RepairInstallation,
    );
}
#[test]
fn t007_engine_effect_integrate() {
    assert_shape(
        map_engine(EngineErrorCode::EffectFailed, &integrate_ctx()),
        InstallerErrorCategory::IntegrationFailed,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::RepairInstallation,
    );
}
#[test]
fn t008_engine_effect_insufficient_context() {
    assert_shape(
        map_engine(
            EngineErrorCode::EffectFailed,
            &ctx(PresentationStage::Install),
        ),
        InstallerErrorCategory::InternalFailure,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
    );
}
#[test]
fn t009_engine_verification_failed() {
    assert_shape(
        map_engine(
            EngineErrorCode::VerificationFailed,
            &ctx(PresentationStage::VerifyInstallation),
        ),
        InstallerErrorCategory::VerificationFailed,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::RepairInstallation,
    );
}
#[test]
fn t010_engine_outcome_unknown() {
    assert_shape(
        map_engine(
            EngineErrorCode::OutcomeUnknown,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t011_engine_reconciliation_required() {
    assert_shape(
        map_engine(
            EngineErrorCode::ReconciliationRequired,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t012_engine_compensation_failed() {
    assert_shape(
        map_engine(
            EngineErrorCode::CompensationFailed,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t013_engine_final_verification_failed() {
    assert_shape(
        map_engine(
            EngineErrorCode::FinalVerificationFailed,
            &ctx(PresentationStage::VerifyInstallation),
        ),
        InstallerErrorCategory::VerificationFailed,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::RepairInstallation,
    );
}
#[test]
fn t014_engine_preflight_typed_reason() {
    let mut c = ctx(PresentationStage::Preflight);
    c.preflight_reason = Some(PlatformFailureCode::InsufficientDiskSpace);
    let e = map_engine(EngineErrorCode::PreflightBlocked, &c);
    assert_shape(
        e,
        InstallerErrorCategory::InsufficientDiskSpace,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::FreeDiskSpace,
    );
}
#[test]
fn t015_engine_preflight_without_reason() {
    assert_shape(
        map_engine(
            EngineErrorCode::PreflightBlocked,
            &ctx(PresentationStage::Preflight),
        ),
        InstallerErrorCategory::InternalFailure,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::ContactSupport,
    );
}

// 016-024: P008 journal mapping.
#[test]
fn t016_journal_missing() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalMissing,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t017_journal_busy() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalBusy,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::ConcurrentOperation,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::CloseOtherInstaller,
    );
}
#[test]
fn t018_journal_corrupt() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalCorrupt,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t019_journal_unsupported_schema() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalUnsupportedSchema,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::UnsupportedLifecycle,
        UserRetryPolicy::NotRetryable,
        RecommendedAction::UseSupportedVersion,
    );
}
#[test]
fn t020_journal_plan_mismatch() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalPlanMismatch,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::Replan,
    );
}
#[test]
fn t021_journal_io_failed() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalIoFailed,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::Interrupted,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t021a_journal_disk_full_before_mutation_is_actionable() {
    let mut context = ctx(PresentationStage::Install);
    context.recovery_disposition = Some(RecoveryDisposition::ReplanRequired);
    assert_shape(
        map_journal(JournalErrorCode::JournalDiskFull, &context),
        InstallerErrorCategory::InsufficientDiskSpace,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::FreeDiskSpace,
    );
}
#[test]
fn t021b_journal_disk_full_after_mutation_requires_reconciliation() {
    let mut context = ctx(PresentationStage::Recovery);
    context.recovery_disposition = Some(RecoveryDisposition::InspectionRequired);
    assert_shape(
        map_journal(JournalErrorCode::JournalDiskFull, &context),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t022_journal_limit_exceeded() {
    assert_shape(
        map_journal(
            JournalErrorCode::JournalLimitExceeded,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t023_journal_active_transaction() {
    assert_shape(
        map_journal(
            JournalErrorCode::ActiveTransactionExists,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::ConcurrentOperation,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::ResumeRecovery,
    );
}
#[test]
fn t024_journal_inspection_required() {
    assert_shape(
        map_journal(
            JournalErrorCode::RecoveryInspectionRequired,
            &ctx(PresentationStage::Recovery),
        ),
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}

// 025-033: recovery dispositions.
#[test]
fn t025_recovery_fresh_is_success() {
    assert!(
        map_recovery(
            RecoveryDisposition::Fresh,
            &ctx(PresentationStage::Recovery)
        )
        .is_none()
    );
}
#[test]
fn t026_recovery_resume_ready_is_success() {
    assert!(
        map_recovery(
            RecoveryDisposition::ResumeReady,
            &ctx(PresentationStage::Recovery)
        )
        .is_none()
    );
}
#[test]
fn t027_recovery_reconciled_applied_is_success() {
    assert!(
        map_recovery(
            RecoveryDisposition::ReconciledApplied,
            &ctx(PresentationStage::Recovery)
        )
        .is_none()
    );
}
#[test]
fn t028_recovery_already_completed_is_success() {
    assert!(
        map_recovery(
            RecoveryDisposition::AlreadyCompleted,
            &ctx(PresentationStage::Recovery)
        )
        .is_none()
    );
}
#[test]
fn t029_recovery_replan_required() {
    let e = map_recovery(
        RecoveryDisposition::ReplanRequired,
        &ctx(PresentationStage::Recovery),
    )
    .unwrap();
    assert_shape(
        e,
        InstallerErrorCategory::InvalidSetupState,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::Replan,
    );
}
#[test]
fn t030_recovery_inspection_required() {
    let e = map_recovery(
        RecoveryDisposition::InspectionRequired,
        &ctx(PresentationStage::Recovery),
    )
    .unwrap();
    assert_shape(
        e,
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t031_recovery_still_unknown() {
    let e = map_recovery(
        RecoveryDisposition::StillUnknown,
        &ctx(PresentationStage::Recovery),
    )
    .unwrap();
    assert_shape(
        e,
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}
#[test]
fn t032_recovery_journal_busy() {
    let e = map_recovery(
        RecoveryDisposition::JournalBusy,
        &ctx(PresentationStage::Recovery),
    )
    .unwrap();
    assert_shape(
        e,
        InstallerErrorCategory::ConcurrentOperation,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::CloseOtherInstaller,
    );
}
#[test]
fn t033_recovery_journal_corrupt() {
    let e = map_recovery(
        RecoveryDisposition::JournalCorrupt,
        &ctx(PresentationStage::Recovery),
    )
    .unwrap();
    assert_shape(
        e,
        InstallerErrorCategory::RecoveryRequired,
        UserRetryPolicy::ReconcileFirst,
        RecommendedAction::InspectRecovery,
    );
}

// 034-048: P009 lifecycle mapping.
macro_rules! lifecycle_case {
    ($name:ident, $source:ident, $category:ident, $retry:ident, $action:ident) => {
        #[test]
        fn $name() {
            assert_shape(
                map_lifecycle(
                    LifecycleError::$source,
                    &ctx(PresentationStage::LifecyclePolicy),
                ),
                InstallerErrorCategory::$category,
                UserRetryPolicy::$retry,
                RecommendedAction::$action,
            );
        }
    };
}
lifecycle_case!(
    t034_lifecycle_intent_mismatch,
    IntentMismatch,
    InvalidSetupState,
    AfterUserAction,
    Replan
);
lifecycle_case!(
    t035_lifecycle_active_transaction,
    ActiveTransaction,
    ConcurrentOperation,
    AfterUserAction,
    ResumeRecovery
);
lifecycle_case!(
    t036_lifecycle_unknown_state,
    UnknownState,
    RecoveryRequired,
    ReconcileFirst,
    InspectRecovery
);
lifecycle_case!(
    t037_lifecycle_unsupported_source,
    UnsupportedSource,
    UnsupportedLifecycle,
    NotRetryable,
    UseSupportedVersion
);
lifecycle_case!(
    t038_lifecycle_unsupported_target,
    UnsupportedTarget,
    UnsupportedLifecycle,
    NotRetryable,
    UseSupportedVersion
);
lifecycle_case!(
    t039_lifecycle_downgrade,
    DowngradeRejected,
    UnsupportedLifecycle,
    NotRetryable,
    UseSupportedVersion
);
lifecycle_case!(
    t040_lifecycle_intermediate,
    IntermediateVersionRequired,
    UnsupportedLifecycle,
    NotRetryable,
    UseSupportedVersion
);
lifecycle_case!(
    t041_lifecycle_artifact_identity,
    ArtifactIdentityMismatch,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
lifecycle_case!(
    t042_lifecycle_version,
    VersionMismatch,
    UnsupportedLifecycle,
    NotRetryable,
    UseSupportedVersion
);
lifecycle_case!(
    t043_lifecycle_protected_mutation,
    ProtectedMutation,
    ProtectedStateBlocked,
    NotRetryable,
    ContactSupport
);
lifecycle_case!(
    t044_lifecycle_unknown_ownership,
    UnknownOwnership,
    ProtectedStateBlocked,
    NotRetryable,
    ContactSupport
);
lifecycle_case!(
    t045_lifecycle_invalid_operation,
    InvalidOperation,
    InvalidSetupState,
    AfterUserAction,
    Replan
);
lifecycle_case!(
    t046_lifecycle_startup_changed,
    StartupPreferenceChanged,
    ProtectedStateBlocked,
    NotRetryable,
    ContactSupport
);
lifecycle_case!(
    t047_lifecycle_preservation_unknown,
    PreservationUnknown,
    ProtectedStateBlocked,
    NotRetryable,
    ContactSupport
);
lifecycle_case!(
    t048_lifecycle_preservation_changed,
    PreservationChanged,
    ProtectedStateBlocked,
    NotRetryable,
    ContactSupport
);

// 049-056: purge mapping.
macro_rules! purge_case {
    ($name:ident, $source:ident) => {
        #[test]
        fn $name() {
            assert_shape(
                map_purge(
                    PurgeError::$source,
                    &ctx(PresentationStage::LifecyclePolicy),
                ),
                InstallerErrorCategory::ProtectedStateBlocked,
                UserRetryPolicy::NotRetryable,
                RecommendedAction::ContactSupport,
            );
        }
    };
}
purge_case!(t049_purge_unsupported_schema, UnsupportedSchema);
purge_case!(t050_purge_missing_intent, MissingIntent);
purge_case!(t051_purge_missing_authorization, MissingAuthorization);
purge_case!(t052_purge_missing_scope, MissingScope);
purge_case!(t053_purge_unauthorized_target, UnauthorizedTarget);
purge_case!(t054_purge_unknown_ownership, UnknownOwnership);
purge_case!(t055_purge_unsafe_containment, UnsafeContainment);
purge_case!(t056_purge_symlink, SymlinkRejected);

// 057-075: exact P006 acquisition codes.
macro_rules! acquisition_case {
    ($name:ident, $code:ident, $category:ident, $retry:ident, $action:ident) => {
        #[test]
        fn $name() {
            assert_shape(
                map_acquisition(
                    AcquisitionFailureCode::$code,
                    &ctx(PresentationStage::Acquire),
                ),
                InstallerErrorCategory::$category,
                UserRetryPolicy::$retry,
                RecommendedAction::$action,
            );
        }
    };
}
acquisition_case!(
    t057_acq_ambiguous,
    AmbiguousArtifact,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t058_acq_digest,
    ArtifactDigestMismatch,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t059_acq_too_large,
    ArtifactTooLarge,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t060_acq_truncated,
    ArtifactTruncated,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t061_acq_destination_conflict,
    DestinationConflict,
    StoragePreparationFailed,
    AfterUserAction,
    ChooseDifferentDestination
);
acquisition_case!(
    t062_acq_invalid_manifest,
    InvalidManifest,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t063_acq_manifest_auth,
    ManifestAuthFailed,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t064_acq_manifest_too_large,
    ManifestTooLarge,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t065_acq_network,
    NetworkError,
    DownloadFailed,
    SafeImmediate,
    RetryDownload
);
acquisition_case!(
    t066_acq_no_match,
    NoMatchingArtifact,
    ArtifactUnavailable,
    AfterUserAction,
    ChooseSupportedArtifact
);
acquisition_case!(
    t067_acq_source_commit,
    SourceCommitMismatch,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t068_acq_staging,
    StagingError,
    StoragePreparationFailed,
    AfterUserAction,
    ChooseDifferentDestination
);
acquisition_case!(
    t069_acq_unsafe_path,
    UnsafePath,
    StoragePreparationFailed,
    AfterUserAction,
    ChooseDifferentDestination
);
acquisition_case!(
    t070_acq_unsafe_redirect,
    UnsafeRedirect,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t071_acq_unsupported_arch,
    UnsupportedArchitecture,
    UnsupportedArchitecture,
    AfterUserAction,
    ChooseSupportedArtifact
);
acquisition_case!(
    t072_acq_unsupported_auth,
    UnsupportedAuthentication,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t073_acq_unsupported_platform,
    UnsupportedPlatform,
    UnsupportedPlatform,
    AfterUserAction,
    ChooseSupportedSystem
);
acquisition_case!(
    t074_acq_untrusted_origin,
    UntrustedOrigin,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);
acquisition_case!(
    t075_acq_version,
    VersionMismatch,
    IntegrityVerificationFailed,
    FreshEvidenceRequired,
    GetFreshOfficialDownload
);

#[test]
fn t076_unknown_acquisition_code_rejected() {
    assert_eq!(
        AcquisitionFailureCode::try_from("FUTURE_REMOTE_CODE"),
        Err(())
    );
}
#[test]
fn t077_acquisition_fixture_exact() {
    let fixture: Vec<String> = serde_json::from_str(include_str!(
        "../../../tests/install-error-model/p006-acquisition-codes.json"
    ))
    .unwrap();
    let actual: Vec<String> = AcquisitionFailureCode::ALL
        .iter()
        .map(|code| code.as_code().to_owned())
        .collect();
    assert_eq!(fixture, actual);
}
#[test]
fn t078_every_acquisition_enum_is_in_fixture() {
    let fixture: Vec<String> = serde_json::from_str(include_str!(
        "../../../tests/install-error-model/p006-acquisition-codes.json"
    ))
    .unwrap();
    for code in AcquisitionFailureCode::ALL {
        assert!(fixture.iter().any(|value| value == code.as_code()));
    }
}
#[test]
fn t079_acquisition_fixture_has_no_duplicates() {
    let fixture: Vec<String> = serde_json::from_str(include_str!(
        "../../../tests/install-error-model/p006-acquisition-codes.json"
    ))
    .unwrap();
    let unique: BTreeSet<_> = fixture.iter().collect();
    assert_eq!(fixture.len(), unique.len());
}
#[test]
fn t079a_acquisition_disk_full_is_actionable_before_mutation() {
    assert_shape(
        map_acquisition(
            AcquisitionFailureCode::InsufficientDiskSpace,
            &ctx(PresentationStage::Acquire),
        ),
        InstallerErrorCategory::InsufficientDiskSpace,
        UserRetryPolicy::AfterUserAction,
        RecommendedAction::FreeDiskSpace,
    );
}

// 080-086: platform/preflight bridge.
macro_rules! platform_case {
    ($name:ident, $code:ident, $category:ident, $action:ident) => {
        #[test]
        fn $name() {
            assert_shape(
                map_platform(
                    PlatformFailureCode::$code,
                    &ctx(PresentationStage::Preflight),
                ),
                InstallerErrorCategory::$category,
                UserRetryPolicy::AfterUserAction,
                RecommendedAction::$action,
            );
        }
    };
}
platform_case!(
    t080_platform_unsupported,
    UnsupportedPlatform,
    UnsupportedPlatform,
    ChooseSupportedSystem
);
platform_case!(
    t081_platform_arch,
    UnsupportedArchitecture,
    UnsupportedArchitecture,
    ChooseSupportedArtifact
);
platform_case!(
    t082_platform_disk,
    InsufficientDiskSpace,
    InsufficientDiskSpace,
    FreeDiskSpace
);
platform_case!(
    t083_platform_native_package,
    NativePackageTransactionFailed,
    PackageInstallFailed,
    UseNativePackageRecovery
);
platform_case!(
    t084_platform_runtime,
    RuntimeDependencyFailed,
    RuntimeDependencyFailed,
    RepairInstallation
);
platform_case!(
    t085_platform_integration,
    IntegrationFailed,
    IntegrationFailed,
    RepairInstallation
);
platform_case!(
    t086_platform_launch,
    LaunchFailed,
    VerificationFailed,
    RepairInstallation
);

// 087-115: safety, copy, diagnostics and schema invariants.
#[test]
fn t087_outcome_unknown_reconcile_first() {
    assert_eq!(
        map_engine(
            EngineErrorCode::OutcomeUnknown,
            &ctx(PresentationStage::Recovery)
        )
        .retry_policy(),
        UserRetryPolicy::ReconcileFirst
    );
}
#[test]
fn t088_outcome_unknown_has_no_retry_download() {
    let e = map_engine(
        EngineErrorCode::OutcomeUnknown,
        &ctx(PresentationStage::Recovery),
    );
    assert_ne!(e.primary_action(), RecommendedAction::RetryDownload);
    assert!(
        !e.secondary_actions()
            .contains(&RecommendedAction::RetryDownload)
    );
}
#[test]
fn t089_still_unknown_has_no_immediate_retry() {
    let e = map_recovery(
        RecoveryDisposition::StillUnknown,
        &ctx(PresentationStage::Recovery),
    )
    .unwrap();
    assert_ne!(e.retry_policy(), UserRetryPolicy::SafeImmediate);
}
#[test]
fn t090_integrity_is_not_immediate_retry() {
    let e = map_acquisition(
        AcquisitionFailureCode::ManifestAuthFailed,
        &ctx(PresentationStage::VerifyArtifact),
    );
    assert_ne!(e.retry_policy(), UserRetryPolicy::SafeImmediate);
}
#[test]
fn t091_integrity_requires_fresh_official_download() {
    let e = map_acquisition(
        AcquisitionFailureCode::ArtifactDigestMismatch,
        &ctx(PresentationStage::VerifyArtifact),
    );
    assert_eq!(
        e.primary_action(),
        RecommendedAction::GetFreshOfficialDownload
    );
}
#[test]
fn t092_network_is_safe_immediate() {
    let e = map_acquisition(
        AcquisitionFailureCode::NetworkError,
        &ctx(PresentationStage::Acquire),
    );
    assert_eq!(e.retry_policy(), UserRetryPolicy::SafeImmediate);
}
#[test]
fn t093_network_uses_retry_download() {
    let e = map_acquisition(
        AcquisitionFailureCode::NetworkError,
        &ctx(PresentationStage::Acquire),
    );
    assert_eq!(e.primary_action(), RecommendedAction::RetryDownload);
}
#[test]
fn t094_destination_conflict_never_overwrites() {
    let e = map_acquisition(
        AcquisitionFailureCode::DestinationConflict,
        &ctx(PresentationStage::Acquire),
    );
    assert_eq!(
        e.primary_action(),
        RecommendedAction::ChooseDifferentDestination
    );
}
#[test]
fn t095_ownership_failure_is_non_destructive() {
    let e = map_lifecycle(
        LifecycleError::UnknownOwnership,
        &ctx(PresentationStage::LifecyclePolicy),
    );
    assert_eq!(e.primary_action(), RecommendedAction::ContactSupport);
}
#[test]
fn t096_preservation_failure_is_non_destructive() {
    let e = map_lifecycle(
        LifecycleError::PreservationChanged,
        &ctx(PresentationStage::LifecyclePolicy),
    );
    assert_eq!(e.category(), InstallerErrorCategory::ProtectedStateBlocked);
}
#[test]
fn t097_purge_failure_is_non_destructive() {
    let e = map_purge(
        PurgeError::UnsafeContainment,
        &ctx(PresentationStage::LifecyclePolicy),
    );
    assert_eq!(e.retry_policy(), UserRetryPolicy::NotRetryable);
}
#[test]
fn t098_unknown_code_not_echoed() {
    let raw = "PASSWORD_TOKEN_FROM_REMOTE";
    let e = map_acquisition_code(raw, &ctx(PresentationStage::Acquire));
    let json = e.to_json().unwrap();
    assert!(!json.contains(raw));
    assert_eq!(e.diagnostic().source_code(), "UNKNOWN_CODE");
}
#[test]
fn t099_raw_path_identifier_is_omitted() {
    let mut c = ctx(PresentationStage::Install);
    c.effect_id = Some("/home/user/private".into());
    let e = map_engine(EngineErrorCode::EffectFailed, &c);
    assert_eq!(e.diagnostic().effect_id(), None);
}
#[test]
fn t100_raw_url_identifier_is_omitted() {
    let mut c = ctx(PresentationStage::Acquire);
    c.artifact_id = Some("https://user@example.test/a".into());
    let e = map_acquisition(AcquisitionFailureCode::NetworkError, &c);
    assert_eq!(e.diagnostic().artifact_id(), None);
}
#[test]
fn t101_token_shaped_identifier_is_omitted() {
    let mut c = ctx(PresentationStage::Install);
    c.effect_id = Some("BearerToken".into());
    let e = map_engine(EngineErrorCode::EffectFailed, &c);
    assert_eq!(e.diagnostic().effect_id(), None);
}
#[test]
fn t102_diagnostic_is_bounded() {
    let mut c = ctx(PresentationStage::Acquire);
    c.effect_id = Some("a".repeat(MAX_SAFE_IDENTIFIER_BYTES));
    c.artifact_id = Some("b".repeat(MAX_SAFE_IDENTIFIER_BYTES));
    let e = map_acquisition(AcquisitionFailureCode::NetworkError, &c);
    let bytes = serde_json::to_vec(e.diagnostic()).unwrap();
    assert!(bytes.len() <= MAX_DIAGNOSTIC_SERIALIZED_BYTES);
}
#[test]
fn t103_secondary_actions_are_bounded() {
    let e = map_engine(EngineErrorCode::InvalidPlan, &ctx(PresentationStage::Plan));
    assert!(e.secondary_actions().len() <= MAX_SECONDARY_ACTIONS);
}
#[test]
fn t104_support_reference_is_deterministic() {
    let a = map_engine(
        EngineErrorCode::OutcomeUnknown,
        &ctx(PresentationStage::Recovery),
    );
    let b = map_engine(
        EngineErrorCode::OutcomeUnknown,
        &ctx(PresentationStage::Recovery),
    );
    assert_eq!(
        a.diagnostic().support_reference(),
        b.diagnostic().support_reference()
    );
    assert_eq!(
        a.diagnostic().support_reference(),
        "SVE-ENGINE-OUTCOME_UNKNOWN"
    );
}
#[test]
fn t105_success_disposition_has_no_error() {
    assert_eq!(
        map_recovery(
            RecoveryDisposition::Fresh,
            &ctx(PresentationStage::Recovery)
        ),
        None
    );
}
#[test]
fn t106_ordinary_copy_has_no_implementation_jargon() {
    let forbidden = [
        "DATABASE_URL",
        "SQLite",
        "PostgreSQL",
        "systemd",
        "Task Scheduler",
        "IPC",
        "SecretStore",
        "OutcomeUnknown",
        "EngineErrorCode",
        "JournalErrorCode",
    ];
    for copy in REFERENCE_COPY {
        for term in forbidden {
            assert!(
                !copy.english.contains(term),
                "{term} leaked into English copy"
            );
            assert!(
                !copy.vietnamese.contains(term),
                "{term} leaked into Vietnamese copy"
            );
        }
    }
}
#[test]
fn t107_message_keys_are_unique() {
    let keys: BTreeSet<_> = REFERENCE_COPY.iter().map(|copy| copy.message_key).collect();
    assert_eq!(keys.len(), REFERENCE_COPY.len());
}
#[test]
fn t108_reference_copy_covers_all_categories() {
    assert_eq!(REFERENCE_COPY.len(), 19);
    let categories: BTreeSet<_> = REFERENCE_COPY
        .iter()
        .map(|copy| format!("{:?}", copy.category))
        .collect();
    assert_eq!(categories.len(), 19);
}
#[test]
fn t109_vietnamese_copy_is_complete() {
    assert!(
        REFERENCE_COPY
            .iter()
            .all(|copy| !copy.vietnamese.trim().is_empty())
    );
}
#[test]
fn t110_english_copy_is_complete() {
    assert!(
        REFERENCE_COPY
            .iter()
            .all(|copy| !copy.english.trim().is_empty())
    );
}
#[test]
fn t111_json_roundtrip_is_validated() {
    let e = map_acquisition(
        AcquisitionFailureCode::NetworkError,
        &ctx(PresentationStage::Acquire),
    );
    let json = e.to_json().unwrap();
    assert_eq!(UserFacingError::from_json(&json).unwrap(), e);
}
#[test]
fn t112_json_unknown_field_is_rejected() {
    let e = map_acquisition(
        AcquisitionFailureCode::NetworkError,
        &ctx(PresentationStage::Acquire),
    );
    let mut value = serde_json::to_value(e).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unexpected".into(), serde_json::json!(true));
    assert_eq!(
        UserFacingError::from_json(&value.to_string()),
        Err(ErrorModelValidationError::InvalidEncoding)
    );
}
#[test]
fn t113_schema_mismatch_is_rejected() {
    let e = map_acquisition(
        AcquisitionFailureCode::NetworkError,
        &ctx(PresentationStage::Acquire),
    );
    let mut value = serde_json::to_value(e).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("schema_version".into(), serde_json::json!(2));
    assert_eq!(
        UserFacingError::from_json(&value.to_string()),
        Err(ErrorModelValidationError::UnsupportedSchema)
    );
}
#[test]
fn t114_action_vocabulary_has_no_escape_hatch() {
    let actions = [
        RecommendedAction::ChooseSupportedSystem,
        RecommendedAction::ChooseSupportedArtifact,
        RecommendedAction::UseSupportedVersion,
        RecommendedAction::FreeDiskSpace,
        RecommendedAction::RequestPermission,
        RecommendedAction::RetryDownload,
        RecommendedAction::GetFreshOfficialDownload,
        RecommendedAction::ChooseDifferentDestination,
        RecommendedAction::UseNativePackageRecovery,
        RecommendedAction::RepairInstallation,
        RecommendedAction::ResumeRecovery,
        RecommendedAction::InspectRecovery,
        RecommendedAction::CloseOtherInstaller,
        RecommendedAction::Replan,
        RecommendedAction::ShowDetails,
        RecommendedAction::ContactSupport,
        RecommendedAction::Cancel,
    ];
    let forbidden = [
        "FORCE",
        "IGNORE",
        "BYPASS",
        "RESETDATABASE",
        "DELETELIBRARY",
        "RUNASROOT",
        "BLINDRETRY",
    ];
    for action in actions {
        let name = format!("{action:?}").to_ascii_uppercase();
        assert!(forbidden.iter().all(|word| !name.contains(word)));
    }
}
#[test]
fn t115_all_reference_categories_validate_through_representative_errors() {
    let representative = [
        map_platform(
            PlatformFailureCode::UnsupportedPlatform,
            &ctx(PresentationStage::Preflight),
        ),
        map_platform(
            PlatformFailureCode::UnsupportedArchitecture,
            &ctx(PresentationStage::Preflight),
        ),
        map_acquisition(
            AcquisitionFailureCode::NoMatchingArtifact,
            &ctx(PresentationStage::Acquire),
        ),
        map_platform(
            PlatformFailureCode::InsufficientDiskSpace,
            &ctx(PresentationStage::Preflight),
        ),
        map_engine(
            EngineErrorCode::PrivilegeUnavailable,
            &ctx(PresentationStage::Install),
        ),
        map_acquisition(
            AcquisitionFailureCode::NetworkError,
            &ctx(PresentationStage::Acquire),
        ),
        map_acquisition(
            AcquisitionFailureCode::ManifestAuthFailed,
            &ctx(PresentationStage::VerifyArtifact),
        ),
        map_acquisition(
            AcquisitionFailureCode::StagingError,
            &ctx(PresentationStage::Acquire),
        ),
        map_platform(
            PlatformFailureCode::NativePackageTransactionFailed,
            &ctx(PresentationStage::Install),
        ),
        map_platform(
            PlatformFailureCode::RuntimeDependencyFailed,
            &ctx(PresentationStage::Install),
        ),
        map_platform(
            PlatformFailureCode::IntegrationFailed,
            &ctx(PresentationStage::Integrate),
        ),
        map_platform(
            PlatformFailureCode::LaunchFailed,
            &ctx(PresentationStage::VerifyInstallation),
        ),
        map_journal(
            JournalErrorCode::JournalIoFailed,
            &ctx(PresentationStage::Recovery),
        ),
        map_recovery(
            RecoveryDisposition::StillUnknown,
            &ctx(PresentationStage::Recovery),
        )
        .unwrap(),
        map_journal(
            JournalErrorCode::JournalBusy,
            &ctx(PresentationStage::Recovery),
        ),
        map_lifecycle(
            LifecycleError::UnsupportedSource,
            &ctx(PresentationStage::LifecyclePolicy),
        ),
        map_lifecycle(
            LifecycleError::ProtectedMutation,
            &ctx(PresentationStage::LifecyclePolicy),
        ),
        map_engine(EngineErrorCode::InvalidPlan, &ctx(PresentationStage::Plan)),
        map_engine(
            EngineErrorCode::EffectFailed,
            &ctx(PresentationStage::Install),
        ),
    ];
    let seen: BTreeSet<_> = representative
        .iter()
        .map(|e| format!("{:?}", e.category()))
        .collect();
    assert_eq!(seen.len(), 19);
    assert!(
        representative
            .iter()
            .all(|e| validate_user_facing_error(e).is_ok())
    );
}
