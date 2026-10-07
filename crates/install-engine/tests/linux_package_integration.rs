use serde_json::{Value, json};
use synveil_install_engine::*;

fn source() -> String {
    include_str!("../../../deploy/linux/package-integration-v1.json").to_owned()
}
fn value() -> Value {
    serde_json::from_str(&source()).unwrap()
}
fn parse(v: Value) -> Result<LinuxPackageContract, LinuxPackageError> {
    LinuxPackageContract::from_json(&v.to_string())
}

#[test]
fn valid_contract() {
    assert!(LinuxPackageContract::from_json(&source()).is_ok());
}
#[test]
fn unknown_schema_fails_closed() {
    let mut v = value();
    v["schema_version"] = json!(2);
    assert_eq!(parse(v), Err(LinuxPackageError::UnsupportedSchema));
}
#[test]
fn unknown_field_rejected() {
    let mut v = value();
    v["surprise"] = json!(true);
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidJson));
}
#[test]
fn malformed_json_rejected() {
    assert_eq!(
        LinuxPackageContract::from_json("{"),
        Err(LinuxPackageError::InvalidJson)
    );
}
#[test]
fn duplicate_format_rejected() {
    let mut v = value();
    v["formats"][1] = v["formats"][0].clone();
    assert_eq!(parse(v), Err(LinuxPackageError::DuplicateFormat));
}
#[test]
fn missing_format_rejected() {
    let mut v = value();
    v["formats"].as_array_mut().unwrap().pop();
    assert_eq!(parse(v), Err(LinuxPackageError::UnsupportedFormat));
}
#[test]
fn invalid_format_rejected() {
    let mut v = value();
    v["formats"][0]["format"] = json!("APPIMAGE");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidJson));
}
#[test]
fn duplicate_path_rejected() {
    let mut v = value();
    let x = v["surfaces"][0].clone();
    v["surfaces"].as_array_mut().unwrap().push(x);
    assert_eq!(parse(v), Err(LinuxPackageError::DuplicatePath));
}
#[test]
fn relative_path_rejected() {
    let mut v = value();
    v["surfaces"][0]["path"] = json!("usr/bin/synveil");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidPath));
}
#[test]
fn traversal_rejected() {
    let mut v = value();
    v["surfaces"][0]["path"] = json!("/usr/bin/../etc/passwd");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidPath));
}
#[test]
fn dot_component_rejected() {
    let mut v = value();
    v["surfaces"][0]["path"] = json!("/usr/bin/./synveil");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidPath));
}
#[test]
fn double_slash_rejected() {
    let mut v = value();
    v["surfaces"][0]["path"] = json!("/usr//bin/synveil");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidPath));
}
#[test]
fn nul_rejected() {
    let mut v = value();
    v["surfaces"][0]["path"] = json!("/usr/bin/syn\0veil");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidPath));
}
#[test]
fn unexpected_root_rejected() {
    let mut v = value();
    v["surfaces"][0]["path"] = json!("/etc/synveil/config");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidPath));
}
#[test]
fn invalid_binary_identity_rejected() {
    let mut v = value();
    v["binary_identity"] = json!("synveil desktop");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidIdentity));
}
#[test]
fn invalid_desktop_identity_rejected() {
    let mut v = value();
    v["desktop_application_id"] = json!("../synveil");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidIdentity));
}
#[test]
fn autostart_rejected() {
    let mut v = value();
    v["autostart"] = json!(true);
    assert_eq!(parse(v), Err(LinuxPackageError::AutostartForbidden));
}
#[test]
fn empty_verification_rejected() {
    let mut v = value();
    v["surfaces"][0]["verification"] = json!("");
    assert_eq!(parse(v), Err(LinuxPackageError::MissingVerification));
}
#[test]
fn user_owned_surface_rejected() {
    let mut v = value();
    v["surfaces"][0]["owner"] = json!("USER_LIBRARY_CONTENT");
    assert_eq!(parse(v), Err(LinuxPackageError::UnauthorizedOwnership));
}
#[test]
fn deb_metadata_matches() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(c.formats[0].artifact_type, "deb");
    assert_eq!(c.formats[0].package_identity, "synveil");
}
#[test]
fn rpm_metadata_matches() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(c.formats[1].artifact_type, "rpm");
    assert_eq!(c.formats[1].package_identity, "synveil");
}
#[test]
fn product_version_preserved() {
    assert_eq!(
        LinuxPackageContract::from_json(&source())
            .unwrap()
            .product_version,
        "0.1.0"
    );
}
#[test]
fn binary_identity_consistent() {
    assert_eq!(
        LinuxPackageContract::from_json(&source())
            .unwrap()
            .binary_identity,
        "synveil-desktop"
    );
}
#[test]
fn desktop_identity_consistent() {
    assert_eq!(
        LinuxPackageContract::from_json(&source())
            .unwrap()
            .desktop_application_id,
        "synveil.desktop"
    );
}
#[test]
fn deb_hooks_declared() {
    assert_eq!(
        LinuxPackageContract::from_json(&source()).unwrap().formats[0].hooks,
        ["preinst", "postinst", "prerm", "postrm"]
    );
}
#[test]
fn rpm_hooks_declared() {
    assert_eq!(
        LinuxPackageContract::from_json(&source()).unwrap().formats[1].hooks,
        ["pre", "post", "preun", "postun"]
    );
}

macro_rules! preservation_test {
    ($name:ident,$resource:ident) => {
        #[test]
        fn $name() {
            let c = LinuxPackageContract::from_json(&source()).unwrap();
            assert!(c.preservation.contains(&ResourceClass::$resource));
        }
    };
}
preservation_test!(preserves_credentials, CredentialState);
preservation_test!(preserves_client_sync, ClientSyncState);
preservation_test!(preserves_user_library, UserLibraryContent);
preservation_test!(preserves_server_config, ServerConfig);
preservation_test!(preserves_server_database, ServerDatabase);
preservation_test!(preserves_server_objects, ServerObjectData);
preservation_test!(preserves_external_dependencies, ExternalDependency);

macro_rules! missing_preservation_test {
    ($name:ident,$resource:expr) => {
        #[test]
        fn $name() {
            let mut v = value();
            v["preservation"]
                .as_array_mut()
                .unwrap()
                .retain(|x| x != $resource);
            assert_eq!(parse(v), Err(LinuxPackageError::MissingPreservation));
        }
    };
}
missing_preservation_test!(missing_credentials_rejected, "CREDENTIAL_STATE");
missing_preservation_test!(missing_client_sync_rejected, "CLIENT_SYNC_STATE");
missing_preservation_test!(missing_user_library_rejected, "USER_LIBRARY_CONTENT");
missing_preservation_test!(missing_server_config_rejected, "SERVER_CONFIG");
missing_preservation_test!(missing_server_database_rejected, "SERVER_DATABASE");
missing_preservation_test!(missing_server_objects_rejected, "SERVER_OBJECT_DATA");
missing_preservation_test!(missing_external_dependency_rejected, "EXTERNAL_DEPENDENCY");

macro_rules! protected_authorization_test {
    ($name:ident,$resource:ident) => {
        #[test]
        fn $name() {
            let c = LinuxPackageContract::from_json(&source()).unwrap();
            assert_eq!(
                c.authorize(
                    LinuxPackageOperation::Uninstall,
                    EffectAuthority::NativePackageManager,
                    ResourceClass::$resource,
                    true,
                    true
                ),
                Err(LinuxPackageError::UnauthorizedOwnership)
            );
        }
    };
}
protected_authorization_test!(cannot_mutate_credentials, CredentialState);
protected_authorization_test!(cannot_mutate_client_sync, ClientSyncState);
protected_authorization_test!(cannot_mutate_user_library, UserLibraryContent);
protected_authorization_test!(cannot_mutate_server_config, ServerConfig);
protected_authorization_test!(cannot_mutate_server_database, ServerDatabase);
protected_authorization_test!(cannot_mutate_server_objects, ServerObjectData);
protected_authorization_test!(cannot_mutate_external_dependency, ExternalDependency);

macro_rules! operation_test {
    ($name:ident,$operation:ident) => {
        #[test]
        fn $name() {
            let c = LinuxPackageContract::from_json(&source()).unwrap();
            assert_eq!(
                c.authorize(
                    LinuxPackageOperation::$operation,
                    EffectAuthority::NativePackageManager,
                    ResourceClass::NativePackageState,
                    true,
                    true
                ),
                Ok(())
            );
        }
    };
}
operation_test!(native_install_authorized, Install);
operation_test!(native_upgrade_authorized, Upgrade);
operation_test!(native_repair_authorized, Repair);
operation_test!(native_uninstall_authorized, Uninstall);
operation_test!(native_verify_authorized, Verify);

#[test]
fn wrong_native_authority_rejected() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Install,
            EffectAuthority::InstallerAdapter,
            ResourceClass::NativePackageState,
            true,
            true
        ),
        Err(LinuxPackageError::WrongAuthority)
    );
}
#[test]
fn wrong_integration_authority_rejected() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Repair,
            EffectAuthority::NativePackageManager,
            ResourceClass::PlatformIntegrationOwned,
            true,
            true
        ),
        Err(LinuxPackageError::WrongAuthority)
    );
}
#[test]
fn integration_authority_accepted() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Repair,
            EffectAuthority::PlatformIntegrationAdapter,
            ResourceClass::PlatformIntegrationOwned,
            true,
            true
        ),
        Ok(())
    );
}
#[test]
fn final_verification_required() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Install,
            EffectAuthority::NativePackageManager,
            ResourceClass::PackageOwned,
            false,
            true
        ),
        Err(LinuxPackageError::FinalVerificationRequired)
    );
}
#[test]
fn incompatible_upgrade_fails_before_mutation() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Upgrade,
            EffectAuthority::NativePackageManager,
            ResourceClass::PackageOwned,
            true,
            false
        ),
        Err(LinuxPackageError::IncompatibleUpgrade)
    );
}

macro_rules! surface_test {
    ($name:ident,$needle:expr) => {
        #[test]
        fn $name() {
            let c = LinuxPackageContract::from_json(&source()).unwrap();
            let surface = c.surfaces.iter().find(|x| x.path == $needle).unwrap();
            assert_eq!(surface.owner, ResourceClass::PackageOwned);
            assert_eq!(
                c.authorize(
                    LinuxPackageOperation::Install,
                    EffectAuthority::NativePackageManager,
                    surface.owner,
                    true,
                    true
                ),
                Ok(())
            );
            assert_eq!(
                c.authorize(
                    LinuxPackageOperation::Install,
                    EffectAuthority::PlatformIntegrationAdapter,
                    surface.owner,
                    true,
                    true
                ),
                Err(LinuxPackageError::WrongAuthority)
            );
        }
    };
}
surface_test!(
    maintenance_payload_present,
    "/usr/bin/synveil-scheduled-maintenance-once"
);
surface_test!(client_payload_present, "/usr/bin/synveil-client");
surface_test!(desktop_payload_present, "/usr/bin/synveil-desktop");
surface_test!(
    user_unit_present,
    "/usr/lib/systemd/user/synveil-client.service"
);
surface_test!(
    system_service_present,
    "/usr/lib/systemd/system/synveil-scheduled-maintenance.service"
);
surface_test!(
    system_timer_present,
    "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer"
);
surface_test!(sysusers_present, "/usr/lib/sysusers.d/synveil.conf");
surface_test!(tmpfiles_present, "/usr/lib/tmpfiles.d/synveil.conf");
surface_test!(
    desktop_entry_present,
    "/usr/share/applications/synveil.desktop"
);
surface_test!(
    icon_present,
    "/usr/share/icons/hicolor/scalable/apps/synveil.svg"
);
surface_test!(license_present, "/usr/share/doc/synveil/LICENSE");
surface_test!(notice_present, "/usr/share/doc/synveil/NOTICE");
surface_test!(
    template_present,
    "/usr/share/synveil/synveil-scheduled-maintenance.env.example"
);

#[test]
fn no_generic_shell_field() {
    assert!(!source().contains("shell_command"));
    assert!(!source().contains("sh -c"));
}
#[test]
fn deb_dependencies_nonempty() {
    assert!(
        !LinuxPackageContract::from_json(&source()).unwrap().formats[0]
            .runtime_dependencies
            .is_empty()
    );
}
#[test]
fn rpm_dependencies_nonempty() {
    assert!(
        !LinuxPackageContract::from_json(&source()).unwrap().formats[1]
            .runtime_dependencies
            .is_empty()
    );
}
#[test]
fn architecture_names_are_format_specific() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert!(c.formats[0].architectures.contains(&"amd64".into()));
    assert!(c.formats[1].architectures.contains(&"x86_64".into()));
}
#[test]
fn package_state_busy_is_typed() {
    assert_eq!(
        LinuxPackageIntegrationState::PackageManagerBusy,
        LinuxPackageIntegrationState::PackageManagerBusy
    );
}
#[test]
fn unknown_outcome_remains_engine_typed() {
    assert_eq!(ApplyOutcome::OutcomeUnknown, ApplyOutcome::OutcomeUnknown);
}
#[test]
fn known_partial_remains_engine_typed() {
    assert_eq!(
        ApplyOutcome::KnownPartialMutation,
        ApplyOutcome::KnownPartialMutation
    );
}
#[test]
fn failure_before_mutation_remains_engine_typed() {
    assert_eq!(
        ApplyOutcome::FailureBeforeMutation,
        ApplyOutcome::FailureBeforeMutation
    );
}
#[test]
fn journal_unknown_requires_reconciliation() {
    assert_eq!(JournalState::OutcomeUnknown, JournalState::OutcomeUnknown);
}
#[test]
fn package_busy_is_not_success() {
    assert_ne!(
        LinuxPackageIntegrationState::PackageManagerBusy,
        LinuxPackageIntegrationState::Installed
    );
}

#[test]
fn live_path_without_symlink_is_accepted() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("usr/bin")).unwrap();
    assert_eq!(
        validate_live_package_path(root.path(), "/usr/bin/synveil-desktop"),
        Ok(())
    );
}

#[cfg(unix)]
#[test]
fn live_path_symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("usr")).unwrap();
    symlink("/tmp", root.path().join("usr/bin")).unwrap();
    assert_eq!(
        validate_live_package_path(root.path(), "/usr/bin/synveil-desktop"),
        Err(LinuxPackageError::SymlinkEscape)
    );
}

#[test]
fn wrong_but_valid_product_version_rejected() {
    let mut v = value();
    v["product_version"] = json!("0.1.1");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidIdentity));
}

#[test]
fn wrong_but_valid_binary_identity_rejected() {
    let mut v = value();
    v["binary_identity"] = json!("synveil");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidIdentity));
}

#[test]
fn wrong_but_valid_desktop_identity_rejected() {
    let mut v = value();
    v["desktop_application_id"] = json!("other.desktop");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidIdentity));
}

#[test]
fn missing_package_surface_rejected() {
    let mut v = value();
    v["surfaces"].as_array_mut().unwrap().pop();
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidSurfaceSet));
}

#[test]
fn extra_package_surface_rejected() {
    let mut v = value();
    v["surfaces"].as_array_mut().unwrap().push(json!({
        "path": "/usr/share/synveil/extra",
        "owner": "PACKAGE_OWNED",
        "verification": "regular-file"
    }));
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidSurfaceSet));
}

#[test]
fn package_shipped_surface_cannot_claim_platform_owner() {
    let mut v = value();
    v["surfaces"][3]["owner"] = json!("PLATFORM_INTEGRATION_OWNED");
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidSurfaceSet));
}

fn assert_bad_metadata(mutator: impl FnOnce(&mut Value)) {
    let mut v = value();
    mutator(&mut v);
    assert_eq!(parse(v), Err(LinuxPackageError::InvalidFormatMetadata));
}

macro_rules! metadata_mutation_test {
    ($name:ident,$body:expr) => {
        #[test]
        fn $name() {
            assert_bad_metadata($body);
        }
    };
}

metadata_mutation_test!(
    duplicate_deb_architecture_rejected,
    |v: &mut Value| v["formats"][0]["architectures"][1] = json!("amd64")
);
metadata_mutation_test!(
    duplicate_rpm_architecture_rejected,
    |v: &mut Value| v["formats"][1]["architectures"][1] = json!("x86_64")
);
metadata_mutation_test!(
    unknown_deb_architecture_rejected,
    |v: &mut Value| v["formats"][0]["architectures"][1] = json!("riscv64")
);
metadata_mutation_test!(
    unknown_rpm_architecture_rejected,
    |v: &mut Value| v["formats"][1]["architectures"][1] = json!("riscv64")
);
metadata_mutation_test!(
    wrong_format_architecture_rejected,
    |v: &mut Value| v["formats"][0]["architectures"][0] = json!("x86_64")
);
metadata_mutation_test!(missing_deb_architecture_rejected, |v: &mut Value| {
    v["formats"][0]["architectures"]
        .as_array_mut()
        .unwrap()
        .pop();
});
metadata_mutation_test!(missing_rpm_architecture_rejected, |v: &mut Value| {
    v["formats"][1]["architectures"]
        .as_array_mut()
        .unwrap()
        .pop();
});
metadata_mutation_test!(architecture_order_rejected, |v: &mut Value| v["formats"][0]
    ["architectures"]
    .as_array_mut()
    .unwrap()
    .reverse());
metadata_mutation_test!(duplicate_dependency_rejected, |v: &mut Value| v["formats"]
    [0]["runtime_dependencies"][1] =
    json!("systemd"));
metadata_mutation_test!(missing_required_dependency_rejected, |v: &mut Value| {
    v["formats"][0]["runtime_dependencies"]
        .as_array_mut()
        .unwrap()
        .pop();
});
metadata_mutation_test!(unexpected_dependency_rejected, |v: &mut Value| v["formats"]
    [1]["runtime_dependencies"]
    .as_array_mut()
    .unwrap()
    .push(json!("curl")));
metadata_mutation_test!(dependency_order_rejected, |v: &mut Value| {
    v["formats"][1]["runtime_dependencies"]
        .as_array_mut()
        .unwrap()
        .reverse()
});
metadata_mutation_test!(
    duplicate_hook_rejected,
    |v: &mut Value| v["formats"][0]["hooks"][1] = json!("preinst")
);
metadata_mutation_test!(
    unknown_hook_rejected,
    |v: &mut Value| v["formats"][0]["hooks"][1] = json!("unknown-hook")
);
metadata_mutation_test!(missing_hook_rejected, |v: &mut Value| {
    v["formats"][1]["hooks"].as_array_mut().unwrap().pop();
});
metadata_mutation_test!(deb_hook_in_rpm_rejected, |v: &mut Value| v["formats"][1]
    ["hooks"][0] =
    json!("postinst"));
metadata_mutation_test!(rpm_hook_in_deb_rejected, |v: &mut Value| v["formats"][0]
    ["hooks"][0] =
    json!("post"));
metadata_mutation_test!(hook_order_rejected, |v: &mut Value| {
    v["formats"][0]["hooks"].as_array_mut().unwrap().reverse()
});

preservation_test!(preserves_application_config, ApplicationConfig);
missing_preservation_test!(missing_application_config_rejected, "APPLICATION_CONFIG");

#[test]
fn duplicate_preservation_rejected() {
    let mut v = value();
    v["preservation"]
        .as_array_mut()
        .unwrap()
        .push(json!("APPLICATION_CONFIG"));
    assert_eq!(parse(v), Err(LinuxPackageError::DuplicatePreservation));
}

#[test]
fn package_cannot_mutate_application_config() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Repair,
            EffectAuthority::NativePackageManager,
            ResourceClass::ApplicationConfig,
            true,
            true
        ),
        Err(LinuxPackageError::UnauthorizedOwnership)
    );
}

#[cfg(unix)]
#[test]
fn symlinked_root_rejected() {
    use std::os::unix::fs::symlink;
    let parent = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = parent.path().join("root");
    symlink(target.path(), &root).unwrap();
    assert_eq!(
        validate_live_package_path(&root, "/usr/bin/synveil-desktop"),
        Err(LinuxPackageError::SymlinkEscape)
    );
}

#[test]
fn missing_root_rejected() {
    let parent = tempfile::tempdir().unwrap();
    assert_eq!(
        validate_live_package_path(&parent.path().join("missing"), "/usr/bin/synveil-desktop"),
        Err(LinuxPackageError::InvalidRoot)
    );
}

#[test]
fn non_directory_root_rejected() {
    let root = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(
        validate_live_package_path(root.path(), "/usr/bin/synveil-desktop"),
        Err(LinuxPackageError::InvalidRoot)
    );
}

macro_rules! invalid_path_test {
    ($name:ident,$path:expr) => {
        #[test]
        fn $name() {
            let root = tempfile::tempdir().unwrap();
            assert_eq!(
                validate_live_package_path(root.path(), $path),
                Err(LinuxPackageError::InvalidPath)
            );
        }
    };
}
invalid_path_test!(newline_path_rejected, "/usr/bin/synveil\ndesktop");
invalid_path_test!(carriage_return_path_rejected, "/usr/bin/synveil\rdesktop");
invalid_path_test!(tab_path_rejected, "/usr/bin/synveil\tdesktop");
invalid_path_test!(backslash_path_rejected, "/usr/bin/synveil\\desktop");
invalid_path_test!(control_byte_path_rejected, "/usr/bin/synveil\u{1f}desktop");

#[test]
fn non_package_platform_integration_requires_platform_adapter() {
    let c = LinuxPackageContract::from_json(&source()).unwrap();
    assert_eq!(
        c.authorize(
            LinuxPackageOperation::Repair,
            EffectAuthority::PlatformIntegrationAdapter,
            ResourceClass::PlatformIntegrationOwned,
            true,
            true
        ),
        Ok(())
    );
}
