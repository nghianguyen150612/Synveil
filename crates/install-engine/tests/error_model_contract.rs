use std::collections::BTreeSet;
use synveil_install_engine::*;

fn context() -> FailureContext {
    FailureContext::default()
}
fn mapped(f: InstallerFailure) -> UserFacingError {
    map_failure(f, &context()).expect("failure maps")
}

macro_rules! engine_case {
    ($name:ident,$variant:ident) => {
        #[test]
        fn $name() {
            let e = mapped(InstallerFailure::Engine(EngineErrorCode::$variant));
            assert!(e.validate().is_ok());
        }
    };
}
engine_case!(engine_invalid_plan, InvalidPlan);
engine_case!(engine_unsupported_schema, UnsupportedSchema);
engine_case!(engine_unauthorized_ownership, UnauthorizedOwnership);
engine_case!(engine_privilege_unavailable, PrivilegeUnavailable);
engine_case!(engine_plan_stale, PlanStale);
engine_case!(engine_verification_failed, VerificationFailed);
engine_case!(engine_outcome_unknown, OutcomeUnknown);
engine_case!(engine_reconciliation_required, ReconciliationRequired);
engine_case!(engine_compensation_failed, CompensationFailed);
engine_case!(engine_final_verification_failed, FinalVerificationFailed);
engine_case!(engine_preflight_without_reason, PreflightBlocked);

#[test]
fn engine_effect_install_context() {
    let c = FailureContext {
        stage: Some(ErrorStage::Install),
        resource_class: Some(ResourceClass::PackageOwned),
        ..context()
    };
    assert_eq!(
        map_failure(InstallerFailure::Engine(EngineErrorCode::EffectFailed), &c)
            .unwrap()
            .category,
        InstallerErrorCategory::PackageInstallFailed
    );
}
#[test]
fn engine_effect_integrate_context() {
    let c = FailureContext {
        stage: Some(ErrorStage::Integrate),
        resource_class: Some(ResourceClass::PlatformIntegrationOwned),
        ..context()
    };
    assert_eq!(
        map_failure(InstallerFailure::Engine(EngineErrorCode::EffectFailed), &c)
            .unwrap()
            .category,
        InstallerErrorCategory::IntegrationFailed
    );
}
#[test]
fn engine_effect_without_context() {
    assert_eq!(
        mapped(InstallerFailure::Engine(EngineErrorCode::EffectFailed)).category,
        InstallerErrorCategory::InternalFailure
    );
}
#[test]
fn engine_preflight_with_reason() {
    let c = FailureContext {
        preflight_reason: Some(PreflightFailure::InsufficientDiskSpace),
        ..context()
    };
    assert_eq!(
        map_failure(
            InstallerFailure::Engine(EngineErrorCode::PreflightBlocked),
            &c
        )
        .unwrap()
        .category,
        InstallerErrorCategory::InsufficientDiskSpace
    );
}

macro_rules! journal_case {
    ($name:ident,$variant:ident) => {
        #[test]
        fn $name() {
            assert!(
                mapped(InstallerFailure::Journal(JournalErrorCode::$variant))
                    .validate()
                    .is_ok()
            );
        }
    };
}
journal_case!(journal_missing, JournalMissing);
journal_case!(journal_busy, JournalBusy);
journal_case!(journal_corrupt, JournalCorrupt);
journal_case!(journal_unsupported_schema, JournalUnsupportedSchema);
journal_case!(journal_plan_mismatch, JournalPlanMismatch);
journal_case!(journal_io_failed, JournalIoFailed);
journal_case!(journal_limit_exceeded, JournalLimitExceeded);
journal_case!(journal_active_transaction, ActiveTransactionExists);
journal_case!(journal_inspection, RecoveryInspectionRequired);

macro_rules! recovery_case {
    ($name:ident,$variant:ident,$some:expr) => {
        #[test]
        fn $name() {
            assert_eq!(
                map_failure(
                    InstallerFailure::Recovery(RecoveryDisposition::$variant),
                    &context()
                )
                .is_some(),
                $some
            );
        }
    };
}
recovery_case!(recovery_fresh, Fresh, false);
recovery_case!(recovery_resume_ready, ResumeReady, false);
recovery_case!(recovery_reconciled, ReconciledApplied, false);
recovery_case!(recovery_replan, ReplanRequired, true);
recovery_case!(recovery_inspection, InspectionRequired, true);
recovery_case!(recovery_completed, AlreadyCompleted, false);
recovery_case!(recovery_unknown, StillUnknown, true);
recovery_case!(recovery_busy, JournalBusy, true);
recovery_case!(recovery_corrupt, JournalCorrupt, true);

macro_rules! lifecycle_case {
    ($name:ident,$variant:ident) => {
        #[test]
        fn $name() {
            assert!(
                mapped(InstallerFailure::Lifecycle(LifecycleError::$variant))
                    .validate()
                    .is_ok()
            );
        }
    };
}
lifecycle_case!(lifecycle_intent, IntentMismatch);
lifecycle_case!(lifecycle_active, ActiveTransaction);
lifecycle_case!(lifecycle_unknown, UnknownState);
lifecycle_case!(lifecycle_source, UnsupportedSource);
lifecycle_case!(lifecycle_target, UnsupportedTarget);
lifecycle_case!(lifecycle_downgrade, DowngradeRejected);
lifecycle_case!(lifecycle_intermediate, IntermediateVersionRequired);
lifecycle_case!(lifecycle_artifact, ArtifactIdentityMismatch);
lifecycle_case!(lifecycle_version, VersionMismatch);
lifecycle_case!(lifecycle_protected, ProtectedMutation);
lifecycle_case!(lifecycle_ownership, UnknownOwnership);
lifecycle_case!(lifecycle_invalid, InvalidOperation);
lifecycle_case!(lifecycle_startup, StartupPreferenceChanged);
lifecycle_case!(lifecycle_preservation_unknown, PreservationUnknown);
lifecycle_case!(lifecycle_preservation_changed, PreservationChanged);

macro_rules! purge_case {
    ($name:ident,$variant:ident) => {
        #[test]
        fn $name() {
            let e = mapped(InstallerFailure::Purge(PurgeError::$variant));
            assert_eq!(e.category, InstallerErrorCategory::ProtectedStateBlocked);
            assert_eq!(e.retry_policy, UserRetryPolicy::NotRetryable);
        }
    };
}
purge_case!(purge_schema, UnsupportedSchema);
purge_case!(purge_intent, MissingIntent);
purge_case!(purge_authorization, MissingAuthorization);
purge_case!(purge_scope, MissingScope);
purge_case!(purge_target, UnauthorizedTarget);
purge_case!(purge_ownership, UnknownOwnership);
purge_case!(purge_containment, UnsafeContainment);
purge_case!(purge_symlink, SymlinkRejected);

macro_rules! acquisition_case {
    ($name:ident,$variant:ident,$code:literal) => {
        #[test]
        fn $name() {
            let parsed = AcquisitionFailureCode::try_from($code).unwrap();
            assert_eq!(parsed, AcquisitionFailureCode::$variant);
            let e = mapped(InstallerFailure::Acquisition(parsed));
            assert!(e.validate().is_ok());
            assert_eq!(parsed.code(), $code);
        }
    };
}
acquisition_case!(acq_ambiguous, AmbiguousArtifact, "AMBIGUOUS_ARTIFACT");
acquisition_case!(
    acq_digest,
    ArtifactDigestMismatch,
    "ARTIFACT_DIGEST_MISMATCH"
);
acquisition_case!(acq_too_large, ArtifactTooLarge, "ARTIFACT_TOO_LARGE");
acquisition_case!(acq_truncated, ArtifactTruncated, "ARTIFACT_TRUNCATED");
acquisition_case!(acq_destination, DestinationConflict, "DESTINATION_CONFLICT");
acquisition_case!(acq_invalid_manifest, InvalidManifest, "INVALID_MANIFEST");
acquisition_case!(acq_auth, ManifestAuthFailed, "MANIFEST_AUTH_FAILED");
acquisition_case!(acq_manifest_large, ManifestTooLarge, "MANIFEST_TOO_LARGE");
acquisition_case!(acq_network, NetworkError, "NETWORK_ERROR");
acquisition_case!(acq_no_match, NoMatchingArtifact, "NO_MATCHING_ARTIFACT");
acquisition_case!(acq_commit, SourceCommitMismatch, "SOURCE_COMMIT_MISMATCH");
acquisition_case!(acq_staging, StagingError, "STAGING_ERROR");
acquisition_case!(acq_path, UnsafePath, "UNSAFE_PATH");
acquisition_case!(acq_redirect, UnsafeRedirect, "UNSAFE_REDIRECT");
acquisition_case!(
    acq_arch,
    UnsupportedArchitecture,
    "UNSUPPORTED_ARCHITECTURE"
);
acquisition_case!(
    acq_auth_type,
    UnsupportedAuthentication,
    "UNSUPPORTED_AUTHENTICATION"
);
acquisition_case!(acq_platform, UnsupportedPlatform, "UNSUPPORTED_PLATFORM");
acquisition_case!(acq_origin, UntrustedOrigin, "UNTRUSTED_ORIGIN");
acquisition_case!(acq_version, VersionMismatch, "VERSION_MISMATCH");

macro_rules! platform_case {
    ($name:ident,$variant:ident,$category:ident) => {
        #[test]
        fn $name() {
            let e = mapped(InstallerFailure::Platform(PlatformFailure::$variant));
            assert_eq!(e.category, InstallerErrorCategory::$category);
        }
    };
}
platform_case!(
    platform_unsupported,
    UnsupportedPlatform,
    UnsupportedPlatform
);
platform_case!(
    platform_arch,
    UnsupportedArchitecture,
    UnsupportedArchitecture
);
platform_case!(platform_disk, InsufficientDiskSpace, InsufficientDiskSpace);
platform_case!(
    platform_package,
    NativePackageTransactionFailed,
    PackageInstallFailed
);
platform_case!(
    platform_runtime,
    RuntimeDependencyFailed,
    RuntimeDependencyFailed
);
platform_case!(platform_integration, IntegrationFailed, IntegrationFailed);
platform_case!(platform_launch, LaunchFailed, VerificationFailed);

#[test]
fn unknown_acquisition_is_rejected() {
    assert!(AcquisitionFailureCode::try_from("REMOTE_SUPPLIED").is_err());
}
#[test]
fn unknown_acquisition_is_not_echoed() {
    let e = map_acquisition_code("token=secret/REMOTE", &context());
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("token=secret"));
    assert_eq!(e.category, InstallerErrorCategory::InternalFailure);
}
#[test]
fn acquisition_enum_has_unique_codes() {
    let set: BTreeSet<_> = AcquisitionFailureCode::ALL
        .iter()
        .map(|x| x.code())
        .collect();
    assert_eq!(set.len(), AcquisitionFailureCode::ALL.len());
}
#[test]
fn acquisition_fixture_is_exact() {
    let fixture: Vec<String> = serde_json::from_str(include_str!(
        "../../../tests/install-error-model/p006-acquisition-codes.json"
    ))
    .unwrap();
    let expected: BTreeSet<_> = AcquisitionFailureCode::ALL
        .iter()
        .map(|x| x.code())
        .collect();
    assert_eq!(
        fixture.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(fixture.len(), expected.len());
}
#[test]
fn network_is_only_immediate_retry() {
    for c in AcquisitionFailureCode::ALL {
        let e = mapped(InstallerFailure::Acquisition(c));
        assert_eq!(
            e.retry_policy == UserRetryPolicy::SafeImmediate,
            c == AcquisitionFailureCode::NetworkError
        );
    }
}
#[test]
fn network_retry_is_download_only() {
    let e = mapped(InstallerFailure::Acquisition(
        AcquisitionFailureCode::NetworkError,
    ));
    assert_eq!(e.primary_action, Some(RecommendedAction::RetryDownload));
}
#[test]
fn outcome_unknown_reconciles() {
    let e = mapped(InstallerFailure::Engine(EngineErrorCode::OutcomeUnknown));
    assert_eq!(
        (e.category, e.retry_policy, e.primary_action),
        (
            InstallerErrorCategory::RecoveryRequired,
            UserRetryPolicy::ReconcileFirst,
            Some(RecommendedAction::InspectRecovery)
        )
    );
}
#[test]
fn still_unknown_reconciles() {
    let e = mapped(InstallerFailure::Recovery(
        RecoveryDisposition::StillUnknown,
    ));
    assert_eq!(e.retry_policy, UserRetryPolicy::ReconcileFirst);
    assert_ne!(e.primary_action, Some(RecommendedAction::RetryDownload));
}
#[test]
fn integrity_requires_fresh_evidence() {
    for c in [
        AcquisitionFailureCode::ManifestAuthFailed,
        AcquisitionFailureCode::ArtifactDigestMismatch,
        AcquisitionFailureCode::UnsafeRedirect,
    ] {
        let e = mapped(InstallerFailure::Acquisition(c));
        assert_eq!(
            (e.retry_policy, e.primary_action),
            (
                UserRetryPolicy::FreshEvidenceRequired,
                Some(RecommendedAction::GetFreshOfficialDownload)
            )
        );
    }
}
#[test]
fn destination_never_authorizes_overwrite() {
    let e = mapped(InstallerFailure::Acquisition(
        AcquisitionFailureCode::DestinationConflict,
    ));
    assert_eq!(
        e.primary_action,
        Some(RecommendedAction::ChooseDifferentDestination)
    );
}
#[test]
fn action_vocabulary_is_finite_and_safe() {
    let published = format!(
        "{:?}",
        [
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
            RecommendedAction::Cancel
        ]
    );
    for bad in [
        "Bypass",
        "Ignore",
        "Force",
        "ResetDatabase",
        "DeleteLibrary",
        "BlindRetry",
        "RunAsRoot",
        "DisableSecurity",
    ] {
        assert!(!published.contains(bad));
    }
}
#[test]
fn safe_identifier_accepts_contract_alphabet() {
    assert_eq!(
        SafeDiagnosticIdentifier::new("effect_1:a-b.c")
            .unwrap()
            .as_str(),
        "effect_1:a-b.c"
    );
}
#[test]
fn safe_identifier_rejects_path() {
    assert!(SafeDiagnosticIdentifier::new("/home/user/file").is_none());
}
#[test]
fn safe_identifier_rejects_windows_path() {
    assert!(SafeDiagnosticIdentifier::new(r"C:\Users\name").is_none());
}
#[test]
fn safe_identifier_rejects_url() {
    assert!(SafeDiagnosticIdentifier::new("https://host/a").is_none());
}
#[test]
fn safe_identifier_rejects_userinfo() {
    assert!(SafeDiagnosticIdentifier::new("user:pass@host").is_none());
}
#[test]
fn safe_identifier_rejects_newline() {
    assert!(SafeDiagnosticIdentifier::new("safe\nsecret").is_none());
}
#[test]
fn safe_identifier_rejects_tab() {
    assert!(SafeDiagnosticIdentifier::new("safe\tsecret").is_none());
}
#[test]
fn safe_identifier_rejects_nul() {
    assert!(SafeDiagnosticIdentifier::new("safe\0secret").is_none());
}
#[test]
fn safe_identifier_rejects_secret_shape() {
    assert!(SafeDiagnosticIdentifier::new("password=secret").is_none());
}
#[test]
fn safe_identifier_is_bounded() {
    assert!(SafeDiagnosticIdentifier::new(&"a".repeat(MAX_SAFE_IDENTIFIER_BYTES + 1)).is_none());
}
#[test]
fn diagnostic_is_bounded() {
    let e = mapped(InstallerFailure::Engine(EngineErrorCode::InvalidPlan));
    assert!(e.diagnostic_json().unwrap().len() <= MAX_DIAGNOSTIC_SERIALIZED_BYTES);
}
#[test]
fn support_reference_is_deterministic() {
    let a = mapped(InstallerFailure::Engine(EngineErrorCode::OutcomeUnknown));
    let b = mapped(InstallerFailure::Engine(EngineErrorCode::OutcomeUnknown));
    assert_eq!(
        a.diagnostic.support_reference,
        b.diagnostic.support_reference
    );
}
#[test]
fn source_code_is_bounded() {
    for c in AcquisitionFailureCode::ALL {
        assert!(c.code().len() <= MAX_SAFE_CODE_BYTES);
    }
}
#[test]
fn secondary_actions_are_bounded() {
    let e = mapped(InstallerFailure::Engine(EngineErrorCode::InvalidPlan));
    assert!(e.secondary_actions.len() <= MAX_SECONDARY_ACTIONS);
}
#[test]
fn validator_rejects_extra_secondary_actions() {
    let mut e = mapped(InstallerFailure::Engine(EngineErrorCode::InvalidPlan));
    e.secondary_actions = vec![RecommendedAction::ShowDetails; 3];
    assert_eq!(
        e.validate(),
        Err(ErrorModelValidationError::TooManySecondaryActions)
    );
}
#[test]
fn validator_rejects_integrity_immediate_retry() {
    let mut e = mapped(InstallerFailure::Acquisition(
        AcquisitionFailureCode::ManifestAuthFailed,
    ));
    e.retry_policy = UserRetryPolicy::SafeImmediate;
    assert!(e.validate().is_err());
}
#[test]
fn validator_rejects_recovery_download_retry() {
    let mut e = mapped(InstallerFailure::Engine(EngineErrorCode::OutcomeUnknown));
    e.primary_action = Some(RecommendedAction::RetryDownload);
    assert!(e.validate().is_err());
}
#[test]
fn validator_rejects_wrong_schema() {
    let mut e = mapped(InstallerFailure::Engine(EngineErrorCode::InvalidPlan));
    e.schema_version = 2;
    assert_eq!(e.validate(), Err(ErrorModelValidationError::WrongSchema));
}
#[test]
fn serialized_contract_denies_unknown_fields() {
    let e = mapped(InstallerFailure::Engine(EngineErrorCode::InvalidPlan));
    let mut v = serde_json::to_value(e).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("raw_message".into(), "secret".into());
    assert!(UserFacingError::from_json(&v.to_string()).is_err());
}
#[test]
fn message_keys_are_semantic() {
    assert_eq!(
        message_key(InstallerErrorCategory::DownloadFailed),
        "installer.error.download_failed"
    );
}
#[test]
fn preservation_has_no_destructive_action() {
    for x in [
        LifecycleError::StartupPreferenceChanged,
        LifecycleError::PreservationUnknown,
        LifecycleError::PreservationChanged,
    ] {
        assert_eq!(
            mapped(InstallerFailure::Lifecycle(x)).primary_action,
            Some(RecommendedAction::ContactSupport)
        );
    }
}
#[test]
fn privilege_requests_scoped_permission() {
    let e = mapped(InstallerFailure::Engine(
        EngineErrorCode::PrivilegeUnavailable,
    ));
    assert_eq!(
        (e.category, e.primary_action),
        (
            InstallerErrorCategory::PermissionRequired,
            Some(RecommendedAction::RequestPermission)
        )
    );
}
