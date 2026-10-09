//! Static contract tests for the Prompt 99 Windows desktop ZIP packager.
//!
//! These tests are intentionally host-neutral. A native Windows runner owns
//! windeployqt execution and Task Scheduler runtime tests; Linux can still
//! prove that the checked-in packager has a bounded, explicit payload policy
//! and is syntactically valid.

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn script() -> PathBuf {
    repo_root().join("deploy/packages/build-windows.sh")
}

fn syntax_check(content: &str) -> Output {
    #[cfg(windows)]
    let bash = {
        // PATH may select the WSL launcher instead of Git's native Bash.
        let output = Command::new("git")
            .arg("--exec-path")
            .output()
            .expect("locate Git installation");
        assert!(output.status.success(), "git --exec-path failed");
        let exec_path = String::from_utf8(output.stdout).expect("Git exec path is UTF-8");
        PathBuf::from(exec_path.trim())
            .ancestors()
            .map(|directory| directory.join("bin/bash.exe"))
            .find(|candidate| candidate.is_file())
            .expect("Git for Windows Bash is required for packaging syntax validation")
    };
    #[cfg(not(windows))]
    let bash = PathBuf::from("bash");

    // A canonical Windows path can use a \\?\ prefix which Bash cannot read.
    // Parse the exact checked-out bytes through stdin instead of translating it.
    let mut child = Command::new(bash)
        .args(["-n", "-s"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run Bash syntax validation");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(content.as_bytes())
        .expect("write exact packager source to Bash");
    child
        .wait_with_output()
        .expect("wait for Bash syntax check")
}

#[test]
fn syntax_validator_rejects_malformed_shell_source() {
    assert!(syntax_check("printf '%s\\n' valid\n").status.success());
    assert!(!syntax_check("if true; then\n").status.success());
}

#[test]
fn windows_packager_is_shell_safe_and_syntax_clean() {
    let path = script();
    let content = fs::read_to_string(&path).expect("read Windows packager");
    assert!(content.starts_with("#!/usr/bin/env bash"));
    assert!(content.contains("set -euo pipefail"));
    assert!(
        !content.contains('\r'),
        "shell source must retain LF line endings"
    );
    assert!(!content.contains("eval "), "packager must not use eval");
    assert!(
        !content.contains("curl | sh"),
        "packager must not bootstrap code"
    );
    assert!(
        !content.contains("rm -rf /") && !content.contains("rm -rf \"/\""),
        "packager must not recursively remove a broad root"
    );

    let output = syntax_check(&content);
    assert!(
        output.status.success(),
        "Windows packager must be syntax-clean ({}): {}",
        output.status,
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
        "--no-compiler-runtime",
        "SYNVEIL_MSVC_CRT_DIR",
        "msvcp140.dll",
        "authenticated app-local MSVC CRT directory is required",
        "x64 app-local MSVC CRT is missing",
        "unexpected elevated CRT bootstrapper",
        "missing non-system import",
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
