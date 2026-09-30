#![cfg(target_os = "linux")]
//! Prompt013 focused static and artifact-aware Debian desktop UX contract tests.
use std::{fs, path::PathBuf};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn read(p: &str) -> String {
    fs::read_to_string(root().join(p)).unwrap()
}
fn check(area: &str, name: &str) {
    let json = read("deploy/linux/debian-desktop-ux-v1.json");
    let desk = read("deploy/applications/synveil.desktop");
    let control = read("deploy/packages/debian/control.tmpl");
    let manifest = read("deploy/install/MANIFEST");
    let integ = read("deploy/linux/package-integration-v1.json");
    let hooks = format!(
        "{}\n{}\n{}",
        read("deploy/packages/debian/postinst"),
        read("deploy/packages/debian/prerm"),
        read("deploy/packages/debian/postrm")
    );
    let roadmap = read("docs/v0.2/ROADMAP.md");
    let workflow = read(".github/workflows/linux-packages.yml");
    let rpm_spec = read("deploy/packages/rpm/synveil.spec.tmpl");
    let validator = read("scripts/validate-debian-desktop-package-ux.sh");
    match (area, name) {
        ("contract", "schema") => assert!(json.contains(r#""schema_version": 1"#)),
        ("contract", "format") => assert!(json.contains(r#""package_format": "DEB""#)),
        ("contract", "product") => assert!(json.contains(r#""product": "Synveil""#)),
        ("contract", "product_arch") => {
            assert!(json.contains(r#""supported_product_architecture": "x86_64""#))
        }
        ("contract", "native_arch") => {
            assert!(json.contains(r#""native_package_architecture": "amd64""#))
        }
        ("contract", "package") => assert!(json.contains(r#""package_name": "synveil""#)),
        ("contract", "desktop_path") => assert!(
            json.contains(r#""desktop_entry_path": "/usr/share/applications/synveil.desktop""#)
        ),
        ("contract", "exec") => {
            assert!(json.contains(r#""desktop_exec": "/usr/bin/synveil-desktop""#))
        }
        ("contract", "icon") => assert!(json.contains(r#""desktop_icon": "synveil""#)),
        ("contract", "terminal") => assert!(json.contains(r#""terminal": false"#)),
        ("contract", "dependency_resolution") => {
            assert!(json.contains(r#""native_dependency_resolution": true"#))
        }
        ("contract", "noninteractive") => {
            assert!(json.contains(r#""interactive_maintainer_scripts": false"#))
        }
        ("contract", "no_auto_launch") => {
            assert!(json.contains(r#""auto_launch_after_install": false"#))
        }
        ("contract", "no_auto_start") => {
            assert!(json.contains(r#""auto_start_after_install": false"#))
        }
        ("contract", "closed_keys") => {
            assert!(json.lines().filter(|l| l.contains(':')).count() == 14)
        }
        ("desktop", "header") => assert!(desk.lines().any(|l| l == "[Desktop Entry]")),
        ("desktop", "type") => assert!(desk.lines().any(|l| l == "Type=Application")),
        ("desktop", "name") => assert!(desk.lines().any(|l| l == "Name=Synveil")),
        ("desktop", "comment") => assert!(
            desk.lines()
                .any(|l| l == "Comment=Synveil desktop synchronization")
        ),
        ("desktop", "exec") => assert!(desk.lines().any(|l| l == "Exec=/usr/bin/synveil-desktop")),
        ("desktop", "icon") => assert!(desk.lines().any(|l| l == "Icon=synveil")),
        ("desktop", "terminal") => assert!(desk.lines().any(|l| l == "Terminal=false")),
        ("desktop", "categories") => assert!(
            desk.lines()
                .any(|l| l == "Categories=Utility;Security;FileTransfer;")
        ),
        ("desktop", "startup_notify") => assert!(desk.lines().any(|l| l == "StartupNotify=true")),
        ("desktop", "no_sudo") => assert!(!desk.contains("sudo")),
        ("desktop", "no_pkexec") => assert!(!desk.contains("pkexec")),
        ("desktop", "no_shell") => assert!(!desk.contains("sh -c")),
        ("desktop", "no_hidden") => assert!(!desk.contains("Hidden=true")),
        ("desktop", "no_nodisplay") => assert!(!desk.contains("NoDisplay=true")),
        ("desktop", "no_onlyshowin") => assert!(!desk.contains("OnlyShowIn=")),
        ("metadata", "package") => assert!(control.contains("Package: synveil")),
        ("metadata", "version_token") => assert!(control.contains("Version: @SYNVEIL_VERSION@")),
        ("metadata", "arch_token") => assert!(control.contains("Architecture: @SYNVEIL_ARCH@")),
        ("metadata", "maintainer") => assert!(control.contains("Maintainer: Synveil Project")),
        ("metadata", "depends_token") => assert!(control.contains("Depends: @SYNVEIL_DEPENDS@")),
        ("metadata", "section") => assert!(control.contains("Section: utils")),
        ("metadata", "priority") => assert!(control.contains("Priority: optional")),
        ("metadata", "summary") => {
            assert!(control.contains("Description: Synveil desktop synchronization"))
        }
        ("metadata", "description_body") => {
            assert!(control.contains("Open Synveil from the application menu"))
        }
        ("metadata", "no_internal_ipc") => assert!(!control.contains("IPC")),
        ("payload", "client") => assert!(manifest.contains("/usr/bin/synveil-client")),
        ("payload", "desktop") => assert!(manifest.contains("/usr/bin/synveil-desktop")),
        ("payload", "maintenance") => {
            assert!(manifest.contains("/usr/bin/synveil-scheduled-maintenance-once"))
        }
        ("payload", "user_unit") => {
            assert!(manifest.contains("/usr/lib/systemd/user/synveil-client.service"))
        }
        ("payload", "desktop_entry") => {
            assert!(manifest.contains("/usr/share/applications/synveil.desktop"))
        }
        ("payload", "icon") => {
            assert!(manifest.contains("/usr/share/icons/hicolor/scalable/apps/synveil.svg"))
        }
        ("payload", "license") => assert!(manifest.contains("/usr/share/doc/synveil/LICENSE")),
        ("payload", "notice") => assert!(manifest.contains("/usr/share/doc/synveil/NOTICE")),
        ("payload", "no_system_client") => {
            assert!(!manifest.contains("/usr/lib/systemd/system/synveil-client.service"))
        }
        ("payload", "desktop_mode") => {
            assert!(manifest.lines().any(
                |l| l.contains("/usr/share/applications/synveil.desktop") && l.contains("0644")
            ))
        }
        ("payload", "icon_mode") => assert!(
            manifest
                .lines()
                .any(|l| l.contains("synveil.svg") && l.contains("0644"))
        ),
        ("payload", "unit_mode") => assert!(
            manifest
                .lines()
                .any(|l| l.contains("systemd/user/synveil-client.service") && l.contains("0644"))
        ),
        ("safety", "postinst_no_gui") => assert!(!hooks.contains("synveil-desktop")),
        ("safety", "postinst_no_user_enable") => assert!(!hooks.contains("systemctl --user")),
        ("safety", "postinst_no_network") => assert!(!hooks.contains("curl ")),
        ("safety", "postinst_no_dialog") => assert!(!hooks.contains("zenity")),
        ("safety", "postinst_no_home") => assert!(!hooks.contains("$HOME")),
        ("safety", "hooks_no_credentials") => assert!(!hooks.contains("touch database-url")),
        ("safety", "hooks_no_database_url") => assert!(!hooks.contains("DATABASE_URL")),
        ("safety", "hooks_no_migration") => assert!(!hooks.contains("migrat")),
        ("safety", "hooks_no_server_bootstrap") => assert!(!hooks.contains("synveil-api")),
        ("safety", "prerm_preserves") => {
            assert!(!read("deploy/packages/debian/prerm").contains("rm -rf"))
        }
        ("safety", "postrm_preserves") => {
            assert!(!read("deploy/packages/debian/postrm").contains("rm -rf"))
        }
        ("safety", "integration_no_autostart") => assert!(integ.contains(r#""autostart": false"#)),
        ("cross", "p012_schema") => assert!(integ.contains(r#""schema_version": 1"#)),
        ("cross", "p012_package") => assert!(integ.contains(r#""package_identity": "synveil""#)),
        ("cross", "p012_amd64") => assert!(integ.contains(r#""amd64""#)),
        ("cross", "release_x86_64") => {
            assert!(read("deploy/packages/build.sh").contains(r#"\"architecture\":\"x86_64\""#))
        }
        ("cross", "manifest_deb") => {
            assert!(read("deploy/packages/build.sh").contains(r#"\"artifact_type\":\"deb\""#))
        }
        ("cross", "p019_pending") => assert!(roadmap.contains("### P019")),
        ("cross", "p020_pending") => assert!(roadmap.contains("### P020")),
        ("cross", "version_frozen") => assert!(read("Cargo.toml").contains("version = \"0.1.0\"")),
        ("cross", "client_user_scoped") => {
            assert!(manifest.contains("/usr/lib/systemd/user/synveil-client.service"))
        }
        ("cross", "direct_exec") => assert!(desk.contains("Exec=/usr/bin/synveil-desktop")),
        ("native_ci", "apt_install") => {
            assert!(workflow.contains("sudo apt-get install -y \"$DEB\""))
        }
        ("native_ci", "dpkg_installed") => {
            assert!(workflow.contains("dpkg-query -W -f='${Status}' synveil"))
        }
        ("native_ci", "non_root_launch") => assert!(workflow.contains("test \"$(id -u)\" -ne 0")),
        ("native_ci", "installed_desktop_validate") => assert!(
            workflow.contains("desktop-file-validate /usr/share/applications/synveil.desktop")
        ),
        ("native_ci", "installed_smoke") => {
            assert!(workflow.contains("timeout 15s /usr/bin/synveil-desktop --qml-smoke-test"))
        }
        ("native_ci", "no_autostart") => {
            assert!(workflow.contains("test ! -e \"$HOME/.config/autostart/synveil.desktop\""))
        }
        ("native_ci", "native_remove") => {
            assert!(workflow.contains("sudo apt-get remove -y synveil"))
        }
        ("native_ci", "preservation") => {
            assert!(workflow.contains("sudo test -f /etc/synveil/p013-preserve.conf"));
            assert!(workflow.contains("sudo test -f /var/lib/synveil/p013-preserve.state"));
        }
        ("native_ci", "rpm_install_posix") => {
            let install = rpm_spec
                .split("%install")
                .nth(1)
                .and_then(|s| s.split("%files").next())
                .expect("RPM %install block");
            assert!(install.contains("set -eu"));
            assert!(!install.contains("pipefail"));
        }
        ("artifact", "permission_pattern") => {
            assert!(validator.contains("^-[rwx-]{9}[[:space:]]+root/root"));
            assert!(!validator.contains("^[-d]r[-wx]{8} root/root"));
        }
        _ => panic!("unknown case {area}/{name}"),
    }
}
macro_rules! case {
    ($n:ident,$a:literal,$c:literal) => {
        #[test]
        fn $n() {
            check($a, $c);
        }
    };
}
case!(p013_001_contract_schema, "contract", "schema");
case!(p013_002_contract_format, "contract", "format");
case!(p013_003_contract_product, "contract", "product");
case!(p013_004_contract_product_arch, "contract", "product_arch");
case!(p013_005_contract_native_arch, "contract", "native_arch");
case!(p013_006_contract_package, "contract", "package");
case!(p013_007_contract_desktop_path, "contract", "desktop_path");
case!(p013_008_contract_exec, "contract", "exec");
case!(p013_009_contract_icon, "contract", "icon");
case!(p013_010_contract_terminal, "contract", "terminal");
case!(
    p013_011_contract_dependency_resolution,
    "contract",
    "dependency_resolution"
);
case!(
    p013_012_contract_noninteractive,
    "contract",
    "noninteractive"
);
case!(
    p013_013_contract_no_auto_launch,
    "contract",
    "no_auto_launch"
);
case!(p013_014_contract_no_auto_start, "contract", "no_auto_start");
case!(p013_015_contract_closed_keys, "contract", "closed_keys");
case!(p013_016_desktop_header, "desktop", "header");
case!(p013_017_desktop_type, "desktop", "type");
case!(p013_018_desktop_name, "desktop", "name");
case!(p013_019_desktop_comment, "desktop", "comment");
case!(p013_020_desktop_exec, "desktop", "exec");
case!(p013_021_desktop_icon, "desktop", "icon");
case!(p013_022_desktop_terminal, "desktop", "terminal");
case!(p013_023_desktop_categories, "desktop", "categories");
case!(p013_024_desktop_startup_notify, "desktop", "startup_notify");
case!(p013_025_desktop_no_sudo, "desktop", "no_sudo");
case!(p013_026_desktop_no_pkexec, "desktop", "no_pkexec");
case!(p013_027_desktop_no_shell, "desktop", "no_shell");
case!(p013_028_desktop_no_hidden, "desktop", "no_hidden");
case!(p013_029_desktop_no_nodisplay, "desktop", "no_nodisplay");
case!(p013_030_desktop_no_onlyshowin, "desktop", "no_onlyshowin");
case!(p013_031_metadata_package, "metadata", "package");
case!(p013_032_metadata_version_token, "metadata", "version_token");
case!(p013_033_metadata_arch_token, "metadata", "arch_token");
case!(p013_034_metadata_maintainer, "metadata", "maintainer");
case!(p013_035_metadata_depends_token, "metadata", "depends_token");
case!(p013_036_metadata_section, "metadata", "section");
case!(p013_037_metadata_priority, "metadata", "priority");
case!(p013_038_metadata_summary, "metadata", "summary");
case!(
    p013_039_metadata_description_body,
    "metadata",
    "description_body"
);
case!(
    p013_040_metadata_no_internal_ipc,
    "metadata",
    "no_internal_ipc"
);
case!(p013_041_payload_client, "payload", "client");
case!(p013_042_payload_desktop, "payload", "desktop");
case!(p013_043_payload_maintenance, "payload", "maintenance");
case!(p013_044_payload_user_unit, "payload", "user_unit");
case!(p013_045_payload_desktop_entry, "payload", "desktop_entry");
case!(p013_046_payload_icon, "payload", "icon");
case!(p013_047_payload_license, "payload", "license");
case!(p013_048_payload_notice, "payload", "notice");
case!(
    p013_049_payload_no_system_client,
    "payload",
    "no_system_client"
);
case!(p013_050_payload_desktop_mode, "payload", "desktop_mode");
case!(p013_051_payload_icon_mode, "payload", "icon_mode");
case!(p013_052_payload_unit_mode, "payload", "unit_mode");
case!(p013_053_safety_postinst_no_gui, "safety", "postinst_no_gui");
case!(
    p013_054_safety_postinst_no_user_enable,
    "safety",
    "postinst_no_user_enable"
);
case!(
    p013_055_safety_postinst_no_network,
    "safety",
    "postinst_no_network"
);
case!(
    p013_056_safety_postinst_no_dialog,
    "safety",
    "postinst_no_dialog"
);
case!(
    p013_057_safety_postinst_no_home,
    "safety",
    "postinst_no_home"
);
case!(
    p013_058_safety_hooks_no_credentials,
    "safety",
    "hooks_no_credentials"
);
case!(
    p013_059_safety_hooks_no_database_url,
    "safety",
    "hooks_no_database_url"
);
case!(
    p013_060_safety_hooks_no_migration,
    "safety",
    "hooks_no_migration"
);
case!(
    p013_061_safety_hooks_no_server_bootstrap,
    "safety",
    "hooks_no_server_bootstrap"
);
case!(p013_062_safety_prerm_preserves, "safety", "prerm_preserves");
case!(
    p013_063_safety_postrm_preserves,
    "safety",
    "postrm_preserves"
);
case!(
    p013_064_safety_integration_no_autostart,
    "safety",
    "integration_no_autostart"
);
case!(p013_065_cross_p012_schema, "cross", "p012_schema");
case!(p013_066_cross_p012_package, "cross", "p012_package");
case!(p013_067_cross_p012_amd64, "cross", "p012_amd64");
case!(p013_068_cross_release_x86_64, "cross", "release_x86_64");
case!(p013_069_cross_manifest_deb, "cross", "manifest_deb");
case!(p013_070_cross_p019_pending, "cross", "p019_pending");
case!(p013_071_cross_p020_pending, "cross", "p020_pending");
case!(p013_072_cross_version_frozen, "cross", "version_frozen");
case!(
    p013_073_cross_client_user_scoped,
    "cross",
    "client_user_scoped"
);
case!(p013_074_cross_direct_exec, "cross", "direct_exec");

case!(p013_075_native_ci_apt_install, "native_ci", "apt_install");
case!(
    p013_076_native_ci_dpkg_installed,
    "native_ci",
    "dpkg_installed"
);
case!(
    p013_077_native_ci_non_root_launch,
    "native_ci",
    "non_root_launch"
);
case!(
    p013_078_native_ci_installed_desktop_validate,
    "native_ci",
    "installed_desktop_validate"
);
case!(
    p013_079_native_ci_installed_smoke,
    "native_ci",
    "installed_smoke"
);
case!(p013_080_native_ci_no_autostart, "native_ci", "no_autostart");
case!(
    p013_081_native_ci_native_remove,
    "native_ci",
    "native_remove"
);
case!(p013_082_native_ci_preservation, "native_ci", "preservation");

case!(
    p013_083_native_ci_rpm_install_posix,
    "native_ci",
    "rpm_install_posix"
);

case!(
    p013_084_artifact_permission_pattern,
    "artifact",
    "permission_pattern"
);
