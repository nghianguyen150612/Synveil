//! Static contract tests for the Prompt 99 Windows desktop ZIP packager.
//!
//! These tests are intentionally host-neutral. A native Windows runner owns
//! windeployqt execution and Task Scheduler runtime tests; Linux can still
//! prove that the checked-in packager has a bounded, explicit payload policy
//! and is syntactically valid.

use std::{fs, path::PathBuf, process::Command};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn script() -> PathBuf {
    repo_root().join("deploy/packages/build-windows.sh")
}

#[test]
fn windows_packager_is_shell_safe_and_syntax_clean() {
    let path = script();
    let content = fs::read_to_string(&path).expect("read Windows packager");
    assert!(content.starts_with("#!/usr/bin/env bash"));
    assert!(content.contains("set -euo pipefail"));
    assert!(!content.contains("eval "), "packager must not use eval");
    assert!(
        !content.contains("curl | sh"),
        "packager must not bootstrap code"
    );
    assert!(
        !content.contains("rm -rf /") && !content.contains("rm -rf \"/\""),
        "packager must not recursively remove a broad root"
    );

    let output = Command::new("bash")
        .arg("-n")
        .arg(&path)
        .output()
        .expect("run bash -n");
    assert!(
        output.status.success(),
        "Windows packager must be syntax-clean: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn windows_packager_has_complete_explicit_runtime_policy() {
    let content = fs::read_to_string(script()).expect("read Windows packager");
    for required in [
        "synveil-desktop.exe",
        "synveil-client.exe",
        "windeployqt",
        "--compiler-runtime",
        "--qmldir",
        "Qt6Core.dll",
        "Qt6Network.dll",
        "Qt6QuickControls2.dll",
        "platforms/qwindows.dll",
        "QtQuick/Controls",
        "libc++.dll",
        "libunwind.dll",
        "qt.conf",
        "llvm-readobj",
        "dumpbin",
        "zip -X",
        "LICENSE",
        "NOTICE",
    ] {
        assert!(
            content.contains(required),
            "Windows packager missing {required}"
        );
    }
    for forbidden in [
        "/usr/lib",
        "/usr/include",
        "windeployqt --all",
        "--qmldir \"/\"",
    ] {
        assert!(
            !content.contains(forbidden),
            "Windows packager contains forbidden broad/development input {forbidden}"
        );
    }
}

#[test]
fn platform_qt_plugin_discovery_matches_the_windows_manifest_layout() {
    let content = fs::read_to_string(script()).expect("read Windows packager");
    let configuration = content
        .split("cat > \"${STAGE_ROOT}/qt.conf\" <<'EOF'\n")
        .nth(1)
        .expect("qt.conf owner")
        .split("\nEOF")
        .next()
        .unwrap();
    let value = |key: &str| {
        configuration
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap()
    };
    assert_eq!(value("Prefix="), ".");
    assert_eq!(value("Plugins="), ".");
    assert_eq!(value("Qml2Imports="), "qml");
    assert!(content.contains("${STAGE_ROOT}/platforms/qwindows.dll"));
    // The native Windows job exercises the shipped qwindows plugin, after
    // extracting the ZIP outside the source/build tree and SDK plugin paths.
    let ci = fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();
    assert!(ci.contains("Expand-Archive"));
    assert!(ci.contains("$env:QT_QPA_PLATFORM = \"windows\""));
}
