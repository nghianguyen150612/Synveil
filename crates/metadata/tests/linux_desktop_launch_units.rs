#![cfg(target_os = "linux")]

//! Static Linux Prompt 99 launch/autostart contract tests.
//!
//! These tests inspect repository-owned definitions only. The real disposable
//! user-manager lifecycle is exercised by the Linux package workflow and by
//! the bounded manual gate; no real Synveil unit is enabled by this test.

use std::{fs, path::PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn active_lines(content: &str) -> impl Iterator<Item = &str> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with(';'))
}

#[test]
fn auto1_client_unit_is_user_level_and_not_a_root_service() {
    let unit = read("deploy/systemd-user/synveil-client.service");
    assert!(unit.contains("[Service]\n"));
    assert!(unit.contains("Type=simple\n"));
    assert!(unit.contains("ExecStart=/usr/bin/synveil-client\n"));
    assert!(unit.contains("[Install]\n"));
    assert!(unit.contains("WantedBy=default.target\n"));
    assert!(!unit.contains("User=root"));
    assert!(!unit.contains("Group=root"));
    assert!(!unit.contains("network-online.target"));
    assert!(
        !repo_root()
            .join("deploy/systemd/synveil-client.service")
            .exists()
    );
    assert!(
        !read("deploy/install/MANIFEST")
            .lines()
            .any(|line| line.contains("/usr/lib/systemd/system/synveil-client.service"))
    );
}

#[test]
fn auto2_auto3_restart_policy_is_bounded_and_source_derived() {
    let unit = read("deploy/systemd-user/synveil-client.service");
    assert!(unit.contains("Restart=on-failure\n"));
    assert!(unit.contains("RestartSec=30s\n"));
    assert!(unit.contains("StartLimitIntervalSec=5min\n"));
    assert!(unit.contains("StartLimitBurst=5\n"));
    assert!(unit.contains("RestartPreventExitStatus=78\n"));

    let client = read("crates/client/src/lib.rs");
    assert!(client.contains("DESKTOP_CLIENT_CONFIG_EXIT_CODE: u8 = 78"));
    assert!(client.contains("DESKTOP_CLIENT_RUNTIME_EXIT_CODE: u8 = 70"));
}

#[test]
fn auto4_auto5_user_actions_are_explicit_and_shell_free() {
    let launch = read("crates/client/src/launch.rs");
    for action in ["\"start\"", "\"enable\"", "\"disable\"", "\"stop\""] {
        assert!(
            launch.contains(action),
            "fixed Linux action missing: {action}"
        );
    }
    assert!(launch.contains("\"--user\","));
    assert!(launch.contains("\"show\","));
    assert!(!launch.contains("sh -c"));
    assert!(!launch.contains("cmd.exe /C"));
    assert!(!launch.contains("PowerShell"));
    assert!(!launch.contains("Command::new(\"sh\")"));
    assert!(!launch.contains("Command::new(\"cmd\")"));
}

#[test]
fn auto6_desktop_entry_is_application_metadata_not_login_autostart() {
    let entry = read("deploy/applications/synveil.desktop");
    let lines: Vec<&str> = active_lines(&entry).collect();
    for required in [
        "Name=Synveil",
        "Exec=/usr/bin/synveil-desktop",
        "Type=Application",
        "Icon=synveil",
        "Terminal=false",
        "Categories=Utility;Security;FileTransfer;",
    ] {
        assert!(
            lines.contains(&required),
            "desktop entry missing {required}"
        );
    }
    for forbidden in [
        "X-GNOME-Autostart",
        "X-KDE-autostart",
        "Autostart=",
        "OnlyShowIn=",
    ] {
        assert!(
            !entry.contains(forbidden),
            "desktop entry must not contain {forbidden}"
        );
    }
}

#[test]
fn auto7_manifest_has_user_unit_and_no_system_client_unit() {
    let manifest = read("deploy/install/MANIFEST");
    assert!(manifest.contains(
        "deploy/systemd-user/synveil-client.service  /usr/lib/systemd/user/synveil-client.service"
    ));
    assert!(!manifest.contains("synveil-client.service  /usr/lib/systemd/system/"));
}
