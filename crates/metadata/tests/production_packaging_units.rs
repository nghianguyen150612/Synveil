#![cfg(target_os = "linux")]

//! Prompt 108 production packaging contract units.
//!
//! These are repository-local, host-neutral checks for the release package
//! boundary. They do not install a package, mutate a real host, or require a
//! Windows runner. When locally built artifacts are present, the artifact
//! checks inspect their metadata as an additional layer of evidence.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const SECRET_SCAN_NEEDLES: &[&str] = &[
    "svd1_",
    "sve1_",
    "bearer ",
    "authorization:",
    "api_key=",
    "private_key",
    "postgresql://user:password@",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn has_command(name: &str) -> bool {
    Command::new("bash")
        .args(["-c", "command -v \"$1\" >/dev/null 2>&1", "bash", name])
        .status()
        .is_ok_and(|status| status.success())
}

fn package_files(extension: &str) -> Vec<PathBuf> {
    package_files_in(&repo_root().join("target/packages"), extension)
}

fn windows_package_files(extension: &str) -> Vec<PathBuf> {
    package_files_in(&repo_root().join("target/windows-packages"), extension)
}

fn package_files_in(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn manifest_rows() -> Vec<(String, String, String, String, String, String)> {
    read(repo_root().join("deploy/install/MANIFEST"))
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "malformed MANIFEST line: {line}");
            (
                fields[0].to_owned(),
                fields[1].to_owned(),
                fields[2].to_owned(),
                fields[3].to_owned(),
                fields[4].to_owned(),
                fields[5].to_owned(),
            )
        })
        .collect()
}

fn cargo_version() -> String {
    let mut in_workspace_package = false;
    for line in read(repo_root().join("Cargo.toml")).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_workspace_package = trimmed == "[workspace.package]";
            continue;
        }
        if in_workspace_package && trimmed.starts_with("version") {
            return trimmed
                .split_once('=')
                .expect("workspace version assignment")
                .1
                .trim()
                .trim_matches(['"', '\''])
                .to_owned();
        }
    }
    panic!("workspace.package.version not found");
}

#[test]
fn package_unit_1_metadata_correctness() {
    let root = repo_root();
    let control = read(root.join("deploy/packages/debian/control.tmpl"));
    let spec = read(root.join("deploy/packages/rpm/synveil.spec.tmpl"));
    let build = read(root.join("deploy/packages/build.sh"));
    assert!(control.contains("Package: synveil\n"));
    assert!(spec.contains("Name:           synveil\n"));
    assert!(control.contains("Maintainer: Synveil Project"));
    assert!(spec.contains("License:        MIT"));
    assert!(control.contains("@SYNVEIL_VERSION@"));
    assert!(spec.contains("@SYNVEIL_VERSION@"));
    assert!(build.contains("DEB_DEPENDS="));
    assert!(build.contains("libdbus-1-3"));
    assert!(build.contains("libsystemd0"));
    assert!(read(root.join("LICENSE")).contains("MIT License"));

    if let Some(deb) = package_files("deb").first() {
        if has_command("dpkg-deb") {
            let output = Command::new("dpkg-deb")
                .args(["--field"])
                .arg(deb)
                .output()
                .expect("inspect DEB metadata");
            assert!(output.status.success(), "dpkg-deb --field failed");
            let metadata = String::from_utf8_lossy(&output.stdout);
            assert!(metadata.contains("Package: synveil"));
            assert!(metadata.contains(&format!("Version: {}", cargo_version())));
            assert!(metadata.contains("libdbus-1-3"));
            assert!(metadata.contains("libsystemd0"));
        } else {
            eprintln!("SKIP PACKAGE-UNIT-1 artifact metadata: dpkg-deb unavailable");
        }
    }
    if let Some(rpm) = package_files("rpm")
        .into_iter()
        .find(|path| !path.to_string_lossy().contains(".src."))
    {
        if has_command("rpm") {
            let metadata = Command::new("rpm")
                .args(["-qp", "--qf", "%{NAME}\n%{VERSION}\n%{RELEASE}\n"])
                .arg(&rpm)
                .output()
                .expect("inspect RPM metadata");
            assert!(metadata.status.success(), "rpm -qp metadata failed");
            let fields = String::from_utf8_lossy(&metadata.stdout);
            let version = cargo_version();
            assert!(fields.lines().any(|line| line == "synveil"));
            assert!(fields.lines().any(|line| line == version));
            let requires = Command::new("rpm")
                .args(["-qp", "--requires"])
                .arg(&rpm)
                .output()
                .expect("inspect RPM dependencies");
            assert!(requires.status.success(), "rpm -qp --requires failed");
            let requires = String::from_utf8_lossy(&requires.stdout);
            assert!(requires.lines().any(|line| line == "dbus-libs"));
            assert!(requires.lines().any(|line| line == "systemd"));
            assert!(requires.lines().any(|line| line == "systemd-libs"));
        } else {
            eprintln!("SKIP PACKAGE-UNIT-1 artifact metadata: rpm unavailable");
        }
    }
}

#[test]
fn package_unit_2_executable_layout() {
    let root = repo_root();
    let rows = manifest_rows();
    let destinations = rows
        .iter()
        .filter(|row| row.5 == "PACKAGE")
        .map(|row| row.1.as_str())
        .collect::<BTreeSet<_>>();
    for destination in [
        "/usr/bin/synveil-scheduled-maintenance-once",
        "/usr/bin/synveil-client",
        "/usr/bin/synveil-desktop",
        "/usr/lib/systemd/user/synveil-client.service",
        "/usr/share/applications/synveil.desktop",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
    ] {
        assert!(
            destinations.contains(destination),
            "MANIFEST lacks {destination}"
        );
    }
    let desktop_entry = read(root.join("deploy/applications/synveil.desktop"));
    assert!(desktop_entry.contains("Exec=/usr/bin/synveil-desktop"));
    assert!(desktop_entry.contains("Icon=synveil"));
    assert!(desktop_entry.contains("Terminal=false"));
    assert!(!desktop_entry.contains("systemctl --user enable"));
}

#[test]
fn package_unit_3_desktop_entry_and_icons() {
    let root = repo_root();
    let desktop = read(root.join("deploy/applications/synveil.desktop"));
    let icon = read(root.join("deploy/icons/hicolor/scalable/apps/synveil.svg"));
    assert!(desktop.contains("[Desktop Entry]"));
    assert!(desktop.contains("Type=Application"));
    assert!(desktop.contains("Categories=Utility;Security;FileTransfer;"));
    assert!(desktop.contains("StartupNotify=true"));
    assert!(desktop.contains("Version=1.0"));
    assert!(icon.contains("<svg"));
    assert!(icon.contains("viewBox="));
}

#[test]
fn package_unit_4_version_consistency() {
    let root = repo_root();
    let version = cargo_version();
    let version_script = read(root.join("deploy/packages/common/version.sh"));
    let linux_builder = read(root.join("deploy/packages/build.sh"));
    let windows_builder = read(root.join("deploy/packages/build-windows.sh"));
    let desktop_main = read(root.join("crates/desktop/src/main.rs"));
    assert!(version_script.contains("workspace.package.version"));
    assert!(linux_builder.contains("synveil_cargo_version"));
    assert!(windows_builder.contains("synveil_cargo_version"));
    assert!(desktop_main.contains("env!(\"CARGO_PKG_VERSION\")"));
    assert!(!linux_builder.contains(&format!("DEB_VERSION=\"{version}\"")));
    assert!(!windows_builder.contains(&format!("PACKAGE_VERSION=\"{version}\"")));
    // Version=1.0 is the desktop-entry specification version, not a second
    // Synveil application version; the application version remains Cargo-owned.
    assert_eq!(
        read(root.join("deploy/applications/synveil.desktop"))
            .lines()
            .find(|line| line.starts_with("Version=")),
        Some("Version=1.0")
    );
}

#[test]
fn package_unit_5_install_path_safety() {
    let root = repo_root();
    let common = read(root.join("deploy/install/common.sh"));
    let install = read(root.join("deploy/install/install.sh"));
    let uninstall = read(root.join("deploy/install/uninstall.sh"));
    assert!(common.contains("realpath -m -s"));
    assert!(common.contains("destination parent escapes staged root"));
    assert!(install.contains("SYNVEIL_ALLOW_HOST_ROOT"));
    assert!(install.contains("refusing to follow symlink at $class destination"));
    assert!(install.contains("synveil_atomic_install"));
    assert!(uninstall.contains("synveil_dest_under_root \"$STAGED_ROOT\" \"$dest\""));
    assert!(!install.contains("curl | sh"));
    assert!(!uninstall.contains("rm -rf /"));
}

#[test]
fn package_unit_6_upgrade_preserves_state() {
    let root = repo_root();
    let install_readme = read(root.join("deploy/install/README.md"));
    let installer = read(root.join("deploy/install/install.sh"));
    assert!(install_readme.contains("atomic rename"));
    assert!(install_readme.contains("preserving `/etc/synveil`"));
    assert!(install_readme.contains("/var/lib/synveil"));
    assert!(installer.contains("SYNVEIL_INSTALL_FAIL_AFTER"));
    assert!(installer.contains("never overwrites"));
    assert!(installer.contains("CREDENTIAL_DIRECTORY"));
    let client_sync_lib = read(root.join("crates/client-sync/src/lib.rs"));
    assert!(client_sync_lib.contains("LOCAL_SCHEMA_VERSION"));
    assert_eq!(
        client_sync_lib
            .lines()
            .find(|line| line.contains("LOCAL_SCHEMA_VERSION"))
            .map(str::trim),
        Some("pub const LOCAL_SCHEMA_VERSION: i64 = 7;")
    );
}

#[test]
fn package_unit_7_uninstall_data_preservation() {
    let root = repo_root();
    let uninstall = read(root.join("deploy/install/uninstall.sh"));
    assert!(uninstall.contains("--purge"));
    assert!(uninstall.contains("ordinary uninstall"));
    assert!(uninstall.contains("/var/lib/synveil"));
    assert!(uninstall.contains("external storage pools"));
    assert!(uninstall.contains("synveil account"));
    assert!(uninstall.contains("unlink symlink PACKAGE"));
    assert!(uninstall.contains("never"));
    assert!(uninstall.contains("handle_purge_target"));
}

#[test]
fn package_unit_8_runtime_dependency_closure() {
    let root = repo_root();
    let linux = read(root.join("deploy/packages/build.sh"));
    let windows = read(root.join("deploy/packages/build-windows.sh"));
    let rpm = read(root.join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(linux.contains("ldd_output"));
    assert!(linux.contains("unresolved dynamic dependency"));
    assert!(linux.contains("libdbus-1-3"));
    assert!(linux.contains("libsystemd0"));
    assert!(rpm.contains("dbus-libs"));
    assert!(rpm.contains("systemd-libs"));
    assert!(windows.contains("pe_import_names"));
    assert!(windows.contains("find_stage_dll"));
    assert!(windows.contains("missing non-system import"));
    assert!(windows.contains("platforms/qwindows.dll"));
}

#[test]
fn package_unit_9_windows_package_manifest() {
    let root = repo_root();
    let builder = read(root.join("deploy/packages/build-windows.sh"));
    for required in [
        "SYNVEIL-MANIFEST.txt",
        "write_package_manifest",
        "validate_package_manifest",
        "sha256_file",
        "platform=windows-x86_64",
        "grep -Fxq \"$MANIFEST_NAME\"",
    ] {
        assert!(
            builder.contains(required),
            "Windows manifest contract lacks {required}"
        );
    }
    let zip_files = windows_package_files("zip");
    let Some(zip) = zip_files.first() else {
        eprintln!(
            "SKIP PACKAGE-UNIT-9 artifact manifest: no Windows ZIP under target/windows-packages"
        );
        return;
    };
    if !has_command("unzip") {
        eprintln!("SKIP PACKAGE-UNIT-9 artifact manifest: unzip unavailable");
        return;
    }
    let output = Command::new("unzip")
        .args(["-p"])
        .arg(zip)
        .arg("SYNVEIL-MANIFEST.txt")
        .output()
        .expect("read Windows package manifest");
    assert!(output.status.success(), "ZIP lacks SYNVEIL-MANIFEST.txt");
    let manifest = String::from_utf8_lossy(&output.stdout);
    assert!(manifest.contains("format=1"));
    assert!(manifest.contains("platform=windows-x86_64"));
    assert!(manifest.contains("synveil-desktop.exe"));
    assert!(manifest.contains("synveil-client.exe"));
}

#[test]
fn package_unit_10_linux_ownership_and_modes() {
    let rows = manifest_rows();
    let package_rows = rows
        .iter()
        .filter(|row| row.5 == "PACKAGE")
        .collect::<Vec<_>>();
    assert_eq!(package_rows.len(), 13, "unexpected PACKAGE manifest count");
    for row in package_rows {
        assert_eq!(row.3, "root", "PACKAGE owner must be root: {}", row.1);
        assert_eq!(row.4, "root", "PACKAGE group must be root: {}", row.1);
        assert!(
            matches!(row.2.as_str(), "0644" | "0755"),
            "invalid mode {} for {}",
            row.2,
            row.1
        );
    }
    let builder = read(repo_root().join("deploy/packages/build.sh"));
    assert!(builder.contains("--owner=0 --group=0 --numeric-owner"));
    assert!(builder.contains("chown root:root"));
    let spec = read(repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(spec.contains("%attr(0755,root,root) /usr/bin/synveil-desktop"));
    assert!(spec.contains("%dir %attr(0750,root,synveil) /etc/synveil"));
}

#[test]
fn security_unit_7_package_artifact_secret_scan() {
    let root = repo_root();
    for path in [
        root.join("deploy/install/MANIFEST"),
        root.join("deploy/packages/debian/control.tmpl"),
        root.join("deploy/packages/rpm/synveil.spec.tmpl"),
        root.join("deploy/config/synveil-scheduled-maintenance.env.example"),
        root.join("deploy/NOTICE"),
    ] {
        let content = read(&path).to_ascii_lowercase();
        for needle in SECRET_SCAN_NEEDLES {
            assert!(
                !content.contains(needle),
                "{} contains secret-like marker {needle}",
                path.display()
            );
        }
    }

    for artifact in package_files("deb")
        .into_iter()
        .chain(package_files("rpm"))
        .chain(windows_package_files("zip"))
    {
        let bytes = fs::read(&artifact).expect("read package artifact");
        let lowercase = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        for needle in SECRET_SCAN_NEEDLES {
            assert!(
                !lowercase.contains(needle),
                "{} contains secret-like marker {needle}",
                artifact.display()
            );
        }
    }
}
