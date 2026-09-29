use std::collections::BTreeSet;
use synveil_install_engine::*;
const OLD: &str = "fa23232ff0154f627ebdd221ec5435134f177af0";
fn id(v: &str, c: &str) -> ProductIdentity {
    ProductIdentity {
        version: v.into(),
        source_commit: Some(c.into()),
    }
}
fn art(v: &str, c: &str) -> VerifiedArtifactEvidence {
    VerifiedArtifactEvidence {
        artifact_id: "artifact".into(),
        artifact_type: "archive".into(),
        product_version: v.into(),
        source_commit: c.into(),
        artifact_sha256: "aa".repeat(32),
        manifest_sha256: "bb".repeat(32),
        authentication_method: "P006".into(),
        verified_local_path: "/verified".into(),
    }
}
fn preserve() -> PreservationSnapshot {
    PreservationSnapshot {
        application_config: Some("a".into()),
        credential_presence: CredentialPresence::Present,
        credential_identity: Some("ref".into()),
        client_sync_state: Some("c".into()),
        user_library_content: Some("l".into()),
        server_config: Some("s".into()),
        server_database: Some("d".into()),
        server_object_data: Some("o".into()),
        external_dependency: Some("e".into()),
    }
}
fn state() -> InstalledStateSnapshot {
    InstalledStateSnapshot {
        product: Some(id("0.1.0", OLD)),
        package_payload: StateClassification::Damaged,
        native_package: StateClassification::Healthy,
        platform_integration: StateClassification::Healthy,
        startup_preference: StartupPreference::Disabled,
        runtime: RuntimeState::Inactive,
        journal: JournalState::None,
        preservation: preserve(),
    }
}
fn effect(class: ResourceClass, authority: EffectAuthority) -> InstallationEffect {
    InstallationEffect {
        effect_id: "e".into(),
        phase: EffectPhase::Install,
        owner: class,
        resource_class: class,
        authority,
        target_scope: TargetScope::CurrentUser,
        preconditions: vec![Precondition {
            kind: "owned".into(),
            expected: "true".into(),
        }],
        privilege: PrivilegeRequirement::CurrentUser,
        mutation_kind: "typed".into(),
        verification: Some(VerificationContract {
            verification_id: "v".into(),
            expected: "ok".into(),
        }),
        retry_policy: RetryPolicy::Never,
        reconciliation_policy: ReconciliationPolicy::InspectByEffectId,
        reversibility: Reversibility::PartiallyReversible,
        safe_inverse: None,
        preservation_constraints: vec![],
        artifact_identity: None,
    }
}
fn plan(
    intent: InstallationIntent,
    effects: Vec<InstallationEffect>,
    a: Option<VerifiedArtifactEvidence>,
) -> InstallationPlan {
    InstallationPlan {
        schema_version: 1,
        plan_id: "p".into(),
        intent,
        target_scope: TargetScope::CurrentUser,
        ordered_effects: effects,
        preservation_set: PROTECTED_RESOURCES.to_vec(),
        expected_completion_verification: VerificationContract {
            verification_id: "done".into(),
            expected: "ok".into(),
        },
        artifact: a.map(|e| ArtifactEvidence::VerifiedReleaseArtifact { evidence: e }),
    }
}
fn upgrade() -> LifecycleRequest {
    let a = art("0.2.0-test", "2222222222222222222222222222222222222222");
    LifecycleRequest::Upgrade(UpgradeRequest {
        compatibility: UpgradeCompatibility {
            source: id("0.1.0", OLD),
            target: id(&a.product_version, &a.source_commit),
            disposition: CompatibilityDisposition::Supported,
        },
        target_artifact: a,
    })
}
fn repair() -> LifecycleRequest {
    LifecycleRequest::Repair(RepairRequest {
        expected: id("0.1.0", OLD),
        target_artifact: Some(art("0.1.0", OLD)),
    })
}
fn sem(op: LifecycleOperation, owned: bool) -> Vec<LifecycleEffect> {
    vec![LifecycleEffect {
        effect_id: "e".into(),
        operation: op,
        ownership_proven: owned,
        startup_preference: None,
    }]
}
#[test]
fn supported_upgrade() {
    let r = upgrade();
    let a = match &r {
        LifecycleRequest::Upgrade(u) => u.target_artifact.clone(),
        _ => unreachable!(),
    };
    assert_eq!(
        LifecyclePolicy.validate(
            &r,
            &state(),
            &plan(
                InstallationIntent::Upgrade,
                vec![effect(
                    ResourceClass::PackageOwned,
                    EffectAuthority::InstallerAdapter
                )],
                Some(a)
            ),
            &sem(LifecycleOperation::ReplacePayload, true)
        ),
        Ok(LifecycleDecision::Compatible)
    );
}
macro_rules! compat{($($n:ident:$d:ident=>$e:ident),+)=>{$(#[test]fn $n(){let mut r=upgrade();if let LifecycleRequest::Upgrade(u)=&mut r{u.compatibility.disposition=CompatibilityDisposition::$d}let a=match &r{LifecycleRequest::Upgrade(u)=>u.target_artifact.clone(),_=>unreachable!()};assert_eq!(LifecyclePolicy.validate(&r,&state(),&plan(InstallationIntent::Upgrade,vec![],Some(a)),&[]),Err(LifecycleError::$e));})+}}
compat!(unknown_compatibility:Unknown=>UnknownState,unsupported_source:UnsupportedSource=>UnsupportedSource,unsupported_target:UnsupportedTarget=>UnsupportedTarget,downgrade_rejected:DowngradeRejected=>DowngradeRejected,intermediate_required:RequiresIntermediateVersion=>IntermediateVersionRequired);
macro_rules! protected{($($n:ident:$c:ident),+)=>{$(#[test]fn $n(){assert_eq!(LifecyclePolicy.validate(&repair(),&state(),&plan(InstallationIntent::Repair,vec![effect(ResourceClass::$c,EffectAuthority::InstallerAdapter)],None),&sem(LifecycleOperation::RestorePayload,true)),Err(LifecycleError::ProtectedMutation));})+}}
protected!(upgrade_config:ApplicationConfig,upgrade_credentials:CredentialState,upgrade_client:ClientSyncState,upgrade_library:UserLibraryContent,upgrade_server_config:ServerConfig,upgrade_database:ServerDatabase,upgrade_objects:ServerObjectData,upgrade_external:ExternalDependency,repair_config:ApplicationConfig,repair_credentials:CredentialState,repair_client:ClientSyncState,repair_library:UserLibraryContent,repair_server_config:ServerConfig,repair_database:ServerDatabase,repair_objects:ServerObjectData,repair_external:ExternalDependency,uninstall_config:ApplicationConfig,uninstall_credentials:CredentialState,uninstall_client:ClientSyncState,uninstall_library:UserLibraryContent,uninstall_server_config:ServerConfig,uninstall_database:ServerDatabase,uninstall_objects:ServerObjectData,uninstall_external:ExternalDependency);
macro_rules! allowed{($($n:ident:$r:expr,$i:ident,$c:ident,$a:ident,$o:ident,$d:ident),+)=>{$(#[test]fn $n(){let r=$r;let artifact=match &r{LifecycleRequest::Upgrade(u)=>Some(u.target_artifact.clone()),_=>None};assert_eq!(LifecyclePolicy.validate(&r,&state(),&plan(InstallationIntent::$i,vec![effect(ResourceClass::$c,EffectAuthority::$a)],artifact),&sem(LifecycleOperation::$o,true)),Ok(LifecycleDecision::$d));})+}}
allowed!(repair_missing:repair(),Repair,PackageOwned,InstallerAdapter,RestorePayload,Repairable,repair_corrupt:repair(),Repair,PackageOwned,InstallerAdapter,RestorePayload,Repairable,repair_integration:repair(),Repair,PlatformIntegrationOwned,PlatformIntegrationAdapter,ReconcileIntegration,Repairable,uninstall_payload:LifecycleRequest::Uninstall,Uninstall,PackageOwned,InstallerAdapter,RemovePayload,RemovalReady,uninstall_integration:LifecycleRequest::Uninstall,Uninstall,PlatformIntegrationOwned,PlatformIntegrationAdapter,RemoveIntegration,RemovalReady,uninstall_native:LifecycleRequest::Uninstall,Uninstall,NativePackageState,NativePackageManager,NativePackageOperation,RemovalReady);
#[test]
fn repair_noop() {
    let mut s = state();
    s.package_payload = StateClassification::Healthy;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(InstallationIntent::Repair, vec![], None),
            &[]
        ),
        Ok(LifecycleDecision::VerifiedNoop)
    );
}
#[test]
fn different_repair_version() {
    let mut r = repair();
    if let LifecycleRequest::Repair(x) = &mut r {
        x.expected.version = "9".into()
    }
    assert_eq!(
        LifecyclePolicy.validate(
            &r,
            &state(),
            &plan(InstallationIntent::Repair, vec![], None),
            &[]
        ),
        Err(LifecycleError::VersionMismatch)
    );
}
#[test]
fn unknown_owner() {
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &state(),
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::PackageOwned,
                    EffectAuthority::InstallerAdapter
                )],
                None
            ),
            &sem(LifecycleOperation::RestorePayload, false)
        ),
        Err(LifecycleError::UnknownOwnership)
    );
}
#[test]
fn arbitrary_operation() {
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &state(),
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::PackageOwned,
                    EffectAuthority::InstallerAdapter
                )],
                None
            ),
            &sem(LifecycleOperation::RemovePayload, true)
        ),
        Err(LifecycleError::InvalidOperation)
    );
}
#[test]
fn interrupted_blocks() {
    let mut s = state();
    s.journal = JournalState::ActiveIncomplete;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(InstallationIntent::Repair, vec![], None),
            &[]
        ),
        Err(LifecycleError::ActiveTransaction)
    );
}
#[test]
fn outcome_unknown_blocks() {
    let mut s = state();
    s.journal = JournalState::OutcomeUnknown;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(InstallationIntent::Repair, vec![], None),
            &[]
        ),
        Err(LifecycleError::ActiveTransaction)
    );
}
#[test]
fn already_removed() {
    let mut s = state();
    s.package_payload = StateClassification::Absent;
    s.platform_integration = StateClassification::Absent;
    assert_eq!(
        LifecyclePolicy.validate(
            &LifecycleRequest::Uninstall,
            &s,
            &plan(InstallationIntent::Uninstall, vec![], None),
            &[]
        ),
        Ok(LifecycleDecision::VerifiedNoop)
    );
}
fn purge() -> PurgeRequest {
    PurgeRequest {
        schema_version: 1,
        destructive_intent: true,
        authorization: Some(PurgeAuthorization {
            confirmation_id: "yes".into(),
            authorized_classes: BTreeSet::from([PurgeResourceClass::ApplicationConfig]),
            authorized_target_ids: BTreeSet::from(["config".into()]),
        }),
        targets: vec![PurgeTarget {
            target_id: "config".into(),
            resource_class: PurgeResourceClass::ApplicationConfig,
            canonical_path: "/home/u/.config/synveil".into(),
            expected_root: "/home/u/.config".into(),
            ownership: TargetOwnership::CanonicalOwner,
            containment_proven: true,
            symlink_free: true,
            traversal_free: true,
        }],
    }
}
#[test]
fn explicit_purge_valid() {
    assert_eq!(LifecyclePolicy.validate_purge(&purge()), Ok(()));
}
macro_rules! purgebad{($($n:ident:$m:expr=>$e:ident),+)=>{$(#[test]fn $n(){let mut p=purge();($m)(&mut p);assert_eq!(LifecyclePolicy.validate_purge(&p),Err(PurgeError::$e));})+}}
purgebad!(purge_schema:|p:&mut PurgeRequest|p.schema_version=2=>UnsupportedSchema,purge_intent:|p:&mut PurgeRequest|p.destructive_intent=false=>MissingIntent,purge_confirmation:|p:&mut PurgeRequest|p.authorization=None=>MissingAuthorization,purge_confirmation_id:|p:&mut PurgeRequest|p.authorization.as_mut().unwrap().confirmation_id.clear()=>MissingScope,purge_classes:|p:&mut PurgeRequest|p.authorization.as_mut().unwrap().authorized_classes.clear()=>MissingScope,purge_ids:|p:&mut PurgeRequest|p.authorization.as_mut().unwrap().authorized_target_ids.clear()=>MissingScope,purge_targets:|p:&mut PurgeRequest|p.targets.clear()=>MissingScope,purge_extra_target:|p:&mut PurgeRequest|p.targets[0].target_id="x".into()=>UnauthorizedTarget,purge_unknown:|p:&mut PurgeRequest|p.targets[0].ownership=TargetOwnership::Unknown=>UnknownOwnership,purge_shared:|p:&mut PurgeRequest|p.targets[0].ownership=TargetOwnership::Shared=>UnknownOwnership,purge_symlink:|p:&mut PurgeRequest|p.targets[0].symlink_free=false=>SymlinkRejected,purge_escape:|p:&mut PurgeRequest|p.targets[0].containment_proven=false=>UnsafeContainment,purge_traversal:|p:&mut PurgeRequest|p.targets[0].traversal_free=false=>UnsafeContainment,purge_empty_path:|p:&mut PurgeRequest|p.targets[0].canonical_path.clear()=>UnauthorizedTarget,purge_empty_root:|p:&mut PurgeRequest|p.targets[0].expected_root.clear()=>UnauthorizedTarget);
#[test]
fn preservation_equal() {
    let p = preserve();
    assert_eq!(LifecyclePolicy.verify_preservation(&p, &p), Ok(()));
}
macro_rules! changed{($($n:ident:$m:expr),+)=>{$(#[test]fn $n(){let b=preserve();let mut a=b.clone();($m)(&mut a);assert_eq!(LifecyclePolicy.verify_preservation(&b,&a),Err(LifecycleError::PreservationChanged));})+}}
changed!(changed_config:|p:&mut PreservationSnapshot|p.application_config=Some("x".into()),changed_credential:|p:&mut PreservationSnapshot|p.credential_identity=Some("x".into()),changed_client:|p:&mut PreservationSnapshot|p.client_sync_state=Some("x".into()),changed_library:|p:&mut PreservationSnapshot|p.user_library_content=Some("x".into()),changed_server_config:|p:&mut PreservationSnapshot|p.server_config=Some("x".into()),changed_database:|p:&mut PreservationSnapshot|p.server_database=Some("x".into()),changed_objects:|p:&mut PreservationSnapshot|p.server_object_data=Some("x".into()),changed_external:|p:&mut PreservationSnapshot|p.external_dependency=Some("x".into()));
#[test]
fn preservation_unknown() {
    let mut p = preserve();
    p.client_sync_state = None;
    assert_eq!(
        LifecyclePolicy.verify_preservation(&p, &p),
        Err(LifecycleError::PreservationUnknown)
    );
}
#[test]
fn disabled_startup_preserved() {
    assert_eq!(state().startup_preference, StartupPreference::Disabled);
}
#[test]
fn disabled_startup_cannot_be_enabled_by_repair() {
    let mut semantics = sem(LifecycleOperation::ReconcileIntegration, true);
    semantics[0].startup_preference = Some(StartupPreference::Enabled);
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &state(),
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::PlatformIntegrationOwned,
                    EffectAuthority::PlatformIntegrationAdapter
                )],
                None
            ),
            &semantics
        ),
        Err(LifecycleError::StartupPreferenceChanged)
    );
}
#[test]
fn mismatched_integration_replace_payload_rejected() {
    assert_invalid_binding(
        upgrade(),
        InstallationIntent::Upgrade,
        ResourceClass::PlatformIntegrationOwned,
        EffectAuthority::PlatformIntegrationAdapter,
        LifecycleOperation::ReplacePayload,
    );
}
#[test]
fn mismatched_integration_restore_payload_rejected() {
    assert_invalid_binding(
        repair(),
        InstallationIntent::Repair,
        ResourceClass::PlatformIntegrationOwned,
        EffectAuthority::PlatformIntegrationAdapter,
        LifecycleOperation::RestorePayload,
    );
}
#[test]
fn mismatched_integration_remove_payload_rejected() {
    assert_invalid_binding(
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        ResourceClass::PlatformIntegrationOwned,
        EffectAuthority::PlatformIntegrationAdapter,
        LifecycleOperation::RemovePayload,
    );
}
#[test]
fn mismatched_package_reconcile_integration_rejected() {
    assert_invalid_binding(
        repair(),
        InstallationIntent::Repair,
        ResourceClass::PackageOwned,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::ReconcileIntegration,
    );
}
#[test]
fn mismatched_package_remove_integration_rejected() {
    assert_invalid_binding(
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        ResourceClass::PackageOwned,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::RemoveIntegration,
    );
}
#[test]
fn mismatched_package_native_operation_rejected() {
    assert_invalid_binding(
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        ResourceClass::PackageOwned,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::NativePackageOperation,
    );
}
#[test]
fn mismatched_native_replace_payload_rejected() {
    let r = upgrade();
    assert_invalid_binding(
        r,
        InstallationIntent::Upgrade,
        ResourceClass::NativePackageState,
        EffectAuthority::NativePackageManager,
        LifecycleOperation::ReplacePayload,
    );
}
#[test]
fn mismatched_native_remove_payload_rejected() {
    assert_invalid_binding(
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        ResourceClass::NativePackageState,
        EffectAuthority::NativePackageManager,
        LifecycleOperation::RemovePayload,
    );
}
#[test]
fn mismatched_runtime_replace_payload_rejected() {
    let r = upgrade();
    assert_invalid_binding(
        r,
        InstallationIntent::Upgrade,
        ResourceClass::EphemeralRuntimeState,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::ReplacePayload,
    );
}
#[test]
fn mismatched_runtime_remove_integration_rejected() {
    assert_invalid_binding(
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        ResourceClass::EphemeralRuntimeState,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::RemoveIntegration,
    );
}

fn assert_invalid_binding(
    request: LifecycleRequest,
    intent: InstallationIntent,
    class: ResourceClass,
    authority: EffectAuthority,
    op: LifecycleOperation,
) {
    let artifact = match &request {
        LifecycleRequest::Upgrade(u) => Some(u.target_artifact.clone()),
        _ => None,
    };
    assert_eq!(
        LifecyclePolicy.validate(
            &request,
            &state(),
            &plan(intent, vec![effect(class, authority)], artifact),
            &sem(op, true),
        ),
        Err(LifecycleError::InvalidOperation)
    );
}

#[test]
fn wrong_package_authority_rejected() {
    assert_invalid_binding(
        repair(),
        InstallationIntent::Repair,
        ResourceClass::PackageOwned,
        EffectAuthority::PlatformIntegrationAdapter,
        LifecycleOperation::RestorePayload,
    );
}
#[test]
fn wrong_integration_authority_rejected() {
    assert_invalid_binding(
        repair(),
        InstallationIntent::Repair,
        ResourceClass::PlatformIntegrationOwned,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::ReconcileIntegration,
    );
}
#[test]
fn wrong_native_authority_rejected() {
    assert_invalid_binding(
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        ResourceClass::NativePackageState,
        EffectAuthority::InstallerAdapter,
        LifecycleOperation::NativePackageOperation,
    );
}
#[test]
fn wrong_runtime_authority_rejected() {
    let mut s = state();
    s.runtime = RuntimeState::ActiveOwned;
    let e = effect(
        ResourceClass::EphemeralRuntimeState,
        EffectAuthority::PlatformIntegrationAdapter,
    );
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(InstallationIntent::Repair, vec![e], None),
            &sem(LifecycleOperation::StopOwnedRuntime, true),
        ),
        Err(LifecycleError::InvalidOperation)
    );
}
#[test]
fn mismatched_owner_and_resource_rejected() {
    let mut e = effect(
        ResourceClass::PlatformIntegrationOwned,
        EffectAuthority::PlatformIntegrationAdapter,
    );
    e.owner = ResourceClass::PackageOwned;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &state(),
            &plan(InstallationIntent::Repair, vec![e], None),
            &sem(LifecycleOperation::ReconcileIntegration, true),
        ),
        Err(LifecycleError::InvalidOperation)
    );
}

#[test]
fn owned_runtime_stop_allowed_when_active_owned() {
    let mut s = state();
    s.runtime = RuntimeState::ActiveOwned;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::EphemeralRuntimeState,
                    EffectAuthority::InstallerAdapter,
                )],
                None,
            ),
            &sem(LifecycleOperation::StopOwnedRuntime, true),
        ),
        Ok(LifecycleDecision::Repairable)
    );
}
#[test]
fn owned_runtime_cleanup_allowed_when_inactive() {
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &state(),
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::EphemeralRuntimeState,
                    EffectAuthority::InstallerAdapter,
                )],
                None,
            ),
            &sem(LifecycleOperation::CleanOwnedRuntimeState, true),
        ),
        Ok(LifecycleDecision::Repairable)
    );
}

fn assert_native_unknown(classification: StateClassification) {
    let mut s = state();
    s.native_package = classification;
    assert_eq!(
        LifecyclePolicy.validate(
            &LifecycleRequest::Uninstall,
            &s,
            &plan(
                InstallationIntent::Uninstall,
                vec![effect(
                    ResourceClass::NativePackageState,
                    EffectAuthority::NativePackageManager,
                )],
                None,
            ),
            &sem(LifecycleOperation::NativePackageOperation, true),
        ),
        Err(LifecycleError::UnknownState)
    );
}
#[test]
fn native_package_unknown_rejected() {
    assert_native_unknown(StateClassification::Unknown);
}
#[test]
fn native_package_interrupted_rejected() {
    assert_native_unknown(StateClassification::Interrupted);
}
#[test]
fn native_package_newer_rejected() {
    assert_native_unknown(StateClassification::NewerThanSupported);
}

fn assert_integration_unknown(
    classification: StateClassification,
    request: LifecycleRequest,
    intent: InstallationIntent,
    operation: LifecycleOperation,
) {
    let mut s = state();
    s.platform_integration = classification;
    assert_eq!(
        LifecyclePolicy.validate(
            &request,
            &s,
            &plan(
                intent,
                vec![effect(
                    ResourceClass::PlatformIntegrationOwned,
                    EffectAuthority::PlatformIntegrationAdapter,
                )],
                None,
            ),
            &sem(operation, true),
        ),
        Err(LifecycleError::UnknownState)
    );
}
#[test]
fn integration_unknown_reconcile_rejected() {
    assert_integration_unknown(
        StateClassification::Unknown,
        repair(),
        InstallationIntent::Repair,
        LifecycleOperation::ReconcileIntegration,
    );
}
#[test]
fn integration_unknown_remove_rejected() {
    assert_integration_unknown(
        StateClassification::Unknown,
        LifecycleRequest::Uninstall,
        InstallationIntent::Uninstall,
        LifecycleOperation::RemoveIntegration,
    );
}
#[test]
fn integration_interrupted_rejected() {
    assert_integration_unknown(
        StateClassification::Interrupted,
        repair(),
        InstallationIntent::Repair,
        LifecycleOperation::ReconcileIntegration,
    );
}
#[test]
fn integration_newer_rejected() {
    assert_integration_unknown(
        StateClassification::NewerThanSupported,
        repair(),
        InstallationIntent::Repair,
        LifecycleOperation::ReconcileIntegration,
    );
}
#[test]
fn runtime_active_unknown_stop_rejected() {
    let mut s = state();
    s.runtime = RuntimeState::ActiveUnknown;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::EphemeralRuntimeState,
                    EffectAuthority::InstallerAdapter,
                )],
                None,
            ),
            &sem(LifecycleOperation::StopOwnedRuntime, true),
        ),
        Err(LifecycleError::UnknownState)
    );
}
#[test]
fn runtime_inactive_stop_rejected() {
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &state(),
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::EphemeralRuntimeState,
                    EffectAuthority::InstallerAdapter,
                )],
                None,
            ),
            &sem(LifecycleOperation::StopOwnedRuntime, true),
        ),
        Err(LifecycleError::InvalidOperation)
    );
}
#[test]
fn runtime_active_owned_cleanup_rejected_until_stopped() {
    let mut s = state();
    s.runtime = RuntimeState::ActiveOwned;
    assert_eq!(
        LifecyclePolicy.validate(
            &repair(),
            &s,
            &plan(
                InstallationIntent::Repair,
                vec![effect(
                    ResourceClass::EphemeralRuntimeState,
                    EffectAuthority::InstallerAdapter,
                )],
                None,
            ),
            &sem(LifecycleOperation::CleanOwnedRuntimeState, true),
        ),
        Err(LifecycleError::InvalidOperation)
    );
}

#[test]
fn purge_resource_class_is_application_config_only() {
    fn exhaustive(resource: PurgeResourceClass) {
        match resource {
            PurgeResourceClass::ApplicationConfig => {}
        }
    }
    exhaustive(PurgeResourceClass::ApplicationConfig);
}
#[test]
fn purge_authorized_scope_stays_exact() {
    let mut p = purge();
    p.targets[0].target_id = "other-config".into();
    assert_eq!(
        LifecyclePolicy.validate_purge(&p),
        Err(PurgeError::UnauthorizedTarget)
    );
}

#[test]
fn credential_present_with_identity_is_known() {
    let p = preserve();
    assert_eq!(LifecyclePolicy.verify_preservation(&p, &p), Ok(()));
}
#[test]
fn credential_present_without_identity_is_unknown() {
    let mut p = preserve();
    p.credential_identity = None;
    assert_eq!(
        LifecyclePolicy.verify_preservation(&p, &p),
        Err(LifecycleError::PreservationUnknown)
    );
}
#[test]
fn credential_present_with_empty_identity_is_unknown() {
    let mut p = preserve();
    p.credential_identity = Some(String::new());
    assert_eq!(
        LifecyclePolicy.verify_preservation(&p, &p),
        Err(LifecycleError::PreservationUnknown)
    );
}
#[test]
fn credential_unknown_presence_is_unknown() {
    let mut p = preserve();
    p.credential_presence = CredentialPresence::Unknown;
    p.credential_identity = None;
    assert_eq!(
        LifecyclePolicy.verify_preservation(&p, &p),
        Err(LifecycleError::PreservationUnknown)
    );
}
#[test]
fn credential_absent_without_identity_is_known() {
    let mut p = preserve();
    p.credential_presence = CredentialPresence::Absent;
    p.credential_identity = None;
    assert_eq!(LifecyclePolicy.verify_preservation(&p, &p), Ok(()));
}
#[test]
fn credential_absent_with_identity_is_unknown() {
    let mut p = preserve();
    p.credential_presence = CredentialPresence::Absent;
    assert_eq!(
        LifecyclePolicy.verify_preservation(&p, &p),
        Err(LifecycleError::PreservationUnknown)
    );
}

#[test]
fn historical_fixture() {
    assert_eq!(state().product.unwrap().source_commit.as_deref(), Some(OLD));
}
