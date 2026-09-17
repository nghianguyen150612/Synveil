#![cfg(target_os = "linux")]

//! Prompt 79 — Native Linux distribution packaging (DEB + RPM) units.
//!
//! Static packaging tests that lock the Gen-1 native-packaging contract
//! without mutating the host (no root, no package-manager install, no
//! PostgreSQL, no network). Source-definition tests always run. Artifact
//! tests (hash/mode/parity) run when `deploy/packages/build.sh` outputs exist
//! under `target/packages/`; otherwise they log a SKIP notice and return so
//! `cargo test --workspace` stays green for developers who have not built
//! packages yet. CI (`linux-packages.yml`) always builds first, so parity is
//! enforced there.
//!
//! Covers: version source, arch mapping, metadata, payload parity, binary
//! hash, unit/sysusers/tmpfiles parity, modes, no secret payload, no numeric
//! UID/GID, no auto-start/enable, upgrade-vs-remove, data preservation,
//! credential preservation, external-storage preservation, shell safety,
//! install-time network rule, CI gate integrity.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Version source of truth (§5)
// ---------------------------------------------------------------------------

fn cargo_version() -> String {
    let content = read(&repo_root().join("Cargo.toml"));
    let mut in_section = false;
    for line in content.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_section = t.starts_with("[workspace.package]");
            continue;
        }
        if in_section && t.starts_with("version") {
            // version = "0.1.0"
            let v: String = t
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
            assert!(!v.is_empty(), "workspace.package.version must not be empty");
            return v;
        }
    }
    panic!("workspace.package.version not found in Cargo.toml");
}

fn sh_helper(script: &str, snippet: &str) -> String {
    let out = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail; source \"{script}\"; {snippet}"))
        .output()
        .expect("run bash helper");
    assert!(
        out.status.success(),
        "helper failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf8")
        .trim()
        .to_string()
}

#[test]
fn cargo_version_is_gen1_expected_shape() {
    let v = cargo_version();
    assert!(
        v.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "cargo version must be SemVer-shaped, got {v:?}"
    );
    assert!(
        v.contains('.'),
        "cargo version must contain dots, got {v:?}"
    );
}

#[test]
fn deb_version_derives_from_cargo_without_hardcode() {
    let cargo = cargo_version();
    let script = repo_root()
        .join("deploy/packages/common/version.sh")
        .to_string_lossy()
        .to_string();
    let deb = sh_helper(&script, &format!("synveil_deb_version \"{cargo}\""));
    assert_eq!(deb, cargo, "plain SemVer must map identically to DEB");
    // Pre-release normalization is deterministic ('-' -> '~').
    let pre = sh_helper(&script, "synveil_deb_version \"1.2.3-beta.1\"");
    assert_eq!(pre, "1.2.3~beta.1");
    // build.sh must derive (source version.sh), never hardcode 0.1.0 as package version.
    let build = read(&repo_root().join("deploy/packages/build.sh"));
    assert!(
        build.contains("common/version.sh"),
        "build.sh must source version.sh"
    );
    assert!(
        build.contains("synveil_cargo_version"),
        "build.sh must call synveil_cargo_version"
    );
    let tmpl = read(&repo_root().join("deploy/packages/debian/control.tmpl"));
    assert!(
        tmpl.contains("@SYNVEIL_VERSION@"),
        "control template must use placeholder, not hardcoded version"
    );
    assert!(
        !tmpl.contains("Version: 0.1.0"),
        "control template must not hardcode cargo version"
    );
}

#[test]
fn rpm_version_derives_from_cargo_without_hardcode() {
    let cargo = cargo_version();
    let script = repo_root()
        .join("deploy/packages/common/version.sh")
        .to_string_lossy()
        .to_string();
    let rpm = sh_helper(&script, &format!("synveil_rpm_version \"{cargo}\""));
    assert_eq!(
        rpm, cargo,
        "plain SemVer must map identically to RPM Version"
    );
    let pre = sh_helper(&script, "synveil_rpm_version \"1.2.3-beta.1\"");
    assert_eq!(pre, "1.2.3~beta.1", "RPM Version must not contain '-'");
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(
        spec.contains("@SYNVEIL_VERSION@"),
        "spec must use version placeholder"
    );
    assert!(
        !spec.contains("\nVersion:        0.1.0\n"),
        "spec must not hardcode cargo version"
    );
}

// ---------------------------------------------------------------------------
// Architecture mapping (§3)
// ---------------------------------------------------------------------------

#[test]
fn architecture_mapping_is_deterministic() {
    let script = repo_root()
        .join("deploy/packages/common/arch.sh")
        .to_string_lossy()
        .to_string();
    assert_eq!(sh_helper(&script, "synveil_deb_arch \"x86_64\""), "amd64");
    assert_eq!(sh_helper(&script, "synveil_rpm_arch \"x86_64\""), "x86_64");
    // Declarative aarch64 mapping only (not claimed as built).
    assert_eq!(sh_helper(&script, "synveil_deb_arch \"aarch64\""), "arm64");
    assert_eq!(
        sh_helper(&script, "synveil_rpm_arch \"aarch64\""),
        "aarch64"
    );
    // Host mapping must succeed on this machine.
    let machine = sh_helper("/dev/null", "uname -m");
    let _ = machine;
    let host_deb = sh_helper(&script, "synveil_deb_arch");
    let host_rpm = sh_helper(&script, "synveil_rpm_arch");
    assert!(!host_deb.is_empty() && !host_rpm.is_empty());
}

// ---------------------------------------------------------------------------
// Package metadata: name, license, maintainer, dependencies (§4, §6, §7, §30)
// ---------------------------------------------------------------------------

#[test]
fn package_name_is_synveil_everywhere() {
    let control = read(&repo_root().join("deploy/packages/debian/control.tmpl"));
    assert!(
        control.contains("Package: synveil\n"),
        "DEB package name must be synveil"
    );
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(
        spec.contains("Name:           synveil\n"),
        "RPM name must be synveil"
    );
    for banned in [
        "synveil-prod",
        "synveil-server",
        "synveil-backup",
        "synveil2",
    ] {
        assert!(
            !control.contains(banned),
            "must not invent alternate name {banned} in DEB"
        );
        assert!(
            !spec.contains(banned),
            "must not invent alternate name {banned} in RPM"
        );
    }
}

#[test]
fn license_is_mit_matching_repository() {
    let repo_license = read(&repo_root().join("LICENSE"));
    assert!(
        repo_license.contains("MIT License"),
        "repository license must be MIT"
    );
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(
        spec.contains("License:        MIT\n"),
        "RPM License must be MIT"
    );
    let workspace = read(&repo_root().join("Cargo.toml"));
    assert!(
        workspace.contains("license = \"MIT\""),
        "workspace license must be MIT"
    );
}

#[test]
fn maintainer_is_project_scoped_without_personal_email() {
    let control = read(&repo_root().join("deploy/packages/debian/control.tmpl"));
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    for content in [&control, &spec] {
        assert!(
            !content.contains("gmail.com")
                && !content.contains("nghianguyen")
                && !content.contains("Nguyen Nghia"),
            "package metadata must not embed personal user information"
        );
    }
    assert!(
        control.contains("Synveil Project"),
        "DEB maintainer must be project-scoped"
    );
    assert!(
        spec.contains("Synveil Project"),
        "RPM changelog/maintainer must be project-scoped"
    );
    assert!(
        control.contains("example.invalid"),
        "placeholder email domain must be clearly non-routable (signing deferred)"
    );
}

#[test]
fn dependencies_are_minimal_and_exclude_server_packages() {
    // control.tmpl carries a placeholder; the concrete value lives in build.sh
    // (single substitution point) and in the built artifact's control file.
    let control = read(&repo_root().join("deploy/packages/debian/control.tmpl"));
    assert!(
        control.contains("Depends: @SYNVEIL_DEPENDS@"),
        "DEB control must substitute Depends from build.sh"
    );
    let build = read(&repo_root().join("deploy/packages/build.sh"));
    let depends_line = build
        .lines()
        .find(|l| l.trim_start().starts_with("DEB_DEPENDS="))
        .expect("build.sh must declare DEB_DEPENDS");
    assert!(
        depends_line.contains("systemd"),
        "must depend on systemd runtime"
    );
    assert!(
        depends_line.contains("libc6"),
        "must depend on libc6 (dynamic loader)"
    );
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(spec.contains("Requires:"), "RPM must declare Requires");
    assert!(spec.contains("systemd"), "RPM must require systemd");
    // Scan code lines only: header comments document the exclusion policy.
    let spec_code: String = spec
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for banned in [
        "postgresql-server",
        "postgresql15-server",
        "postgresql-client",
        "docker",
        "podman",
        "nginx",
        "redis",
    ] {
        assert!(
            !depends_line.to_lowercase().contains(banned),
            "DEB must not require {banned}"
        );
        assert!(!spec_code.contains(banned), "RPM must not require {banned}");
    }
    // Built DEB control (when artifacts exist) must carry the substituted value.
    if let Some(deb) = find_deb() {
        let dd = tempdir("deb-ctrl");
        let out = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "set -euo pipefail; ar p {} control.tar.gz | tar -xzf - -C {}",
                shell_quote(&deb.to_string_lossy()),
                shell_quote(&dd.to_string_lossy())
            ))
            .output()
            .expect("extract control");
        assert!(out.status.success());
        let built = read(&dd.join("control"));
        let built_depends = built
            .lines()
            .find(|l| l.starts_with("Depends:"))
            .expect("built control Depends");
        assert!(
            built_depends.contains("systemd") && built_depends.contains("libc6"),
            "built DEB Depends must be minimal+correct: {built_depends:?}"
        );
        let _ = fs::remove_dir_all(&dd);
    }
}

// ---------------------------------------------------------------------------
// Payload authority: MANIFEST is the single source of truth (§8)
// ---------------------------------------------------------------------------

fn parse_manifest() -> Vec<(String, String, String, String, String, String)> {
    let content = read(&repo_root().join("deploy/install/MANIFEST"));
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut p = t.split_whitespace();
        out.push((
            p.next().unwrap_or("").to_string(),
            p.next().unwrap_or("").to_string(),
            p.next().unwrap_or("").to_string(),
            p.next().unwrap_or("").to_string(),
            p.next().unwrap_or("").to_string(),
            p.next().unwrap_or("").to_string(),
        ));
    }
    out
}

#[test]
fn payload_derives_from_manifest_not_duplicate_list() {
    let build = read(&repo_root().join("deploy/packages/build.sh"));
    assert!(
        build.contains("deploy/install/install.sh"),
        "build.sh must stage via install.sh"
    );
    assert!(
        build.contains("MANIFEST"),
        "build.sh must reference MANIFEST authority"
    );
    let payload = read(&repo_root().join("deploy/packages/common/payload.sh"));
    assert!(
        payload.contains("deploy/install/install.sh"),
        "payload helper must delegate to install.sh"
    );
    // Spec %install must copy the staged payload, not enumerate its own sources.
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(
        spec.contains("SYNVEIL_STAGED_PAYLOAD"),
        "spec must consume staged payload"
    );
}

#[test]
fn manifest_package_set_matches_expected_gen1_payload() {
    let entries = parse_manifest();
    let package: Vec<_> = entries.iter().filter(|e| e.5 == "PACKAGE").collect();
    let dests: BTreeSet<&str> = package.iter().map(|e| e.1.as_str()).collect();
    for expected in [
        "/usr/bin/synveil-client",
        "/usr/bin/synveil-desktop",
        "/usr/bin/synveil-scheduled-maintenance-once",
        "/usr/lib/systemd/user/synveil-client.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "/usr/lib/sysusers.d/synveil.conf",
        "/usr/lib/tmpfiles.d/synveil.conf",
        "/usr/share/applications/synveil.desktop",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "/usr/share/doc/synveil/LICENSE",
        "/usr/share/doc/synveil/NOTICE",
        "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
    ] {
        assert!(
            dests.contains(expected),
            "MANIFEST PACKAGE set must contain {expected}"
        );
    }
    assert_eq!(
        package.len(),
        13,
        "desktop PACKAGE set must be exactly 13 entries"
    );
    // Modes per Prompt 75 contract.
    let modes: BTreeMap<&str, &str> = package
        .iter()
        .map(|e| (e.1.as_str(), e.2.as_str()))
        .collect();
    for p in [
        "/usr/bin/synveil-client",
        "/usr/bin/synveil-desktop",
        "/usr/bin/synveil-scheduled-maintenance-once",
    ] {
        assert_eq!(modes[p], "0755", "mode for {p}");
    }
    for p in [
        "/usr/lib/systemd/user/synveil-client.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "/usr/lib/sysusers.d/synveil.conf",
        "/usr/lib/tmpfiles.d/synveil.conf",
        "/usr/share/applications/synveil.desktop",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "/usr/share/doc/synveil/LICENSE",
        "/usr/share/doc/synveil/NOTICE",
        "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
    ] {
        assert_eq!(modes[p], "0644", "mode for {p}");
    }
    // No duplicates.
    assert_eq!(
        dests.len(),
        package.len(),
        "no duplicate destinations allowed"
    );
    // No secret payload in manifest.
    for e in &entries {
        assert!(
            e.1 != "/etc/synveil/credentials/database-url",
            "MANIFEST must never list the credential file as payload"
        );
    }
}

// ---------------------------------------------------------------------------
// Artifact helpers (extract once per test into a fresh tempdir)
// ---------------------------------------------------------------------------

fn find_deb() -> Option<PathBuf> {
    let dir = repo_root().join("target/packages");
    let mut v: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "deb"))
        .collect();
    v.sort();
    v.pop()
}

fn find_rpm() -> Option<PathBuf> {
    let dir = repo_root().join("target/packages");
    let mut v: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rpm"))
        .collect();
    // Exclude src.rpm if ever present.
    v.retain(|p| !p.to_string_lossy().contains(".src."));
    v.sort();
    v.pop()
}

fn tempdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "synveil-pkgtest-{}-{}-{}",
        tag,
        std::process::id(),
        uuid_simple()
    ));
    fs::create_dir_all(&d).expect("create tempdir");
    d
}

fn uuid_simple() -> String {
    // No uuid crate in metadata dev-deps; use nanos + pid for uniqueness.
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn extract_deb(deb: &Path, dest: &Path) {
    // Real .deb layout: ar archive with data.tar.gz (+ control.tar.gz).
    let data = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "set -euo pipefail; ar p {} data.tar.gz | tar -xzf - -C {}",
            shell_quote(&deb.to_string_lossy()),
            shell_quote(&dest.to_string_lossy())
        ))
        .output()
        .expect("extract deb");
    assert!(
        data.status.success(),
        "deb extract failed: {}",
        String::from_utf8_lossy(&data.stderr)
    );
}

fn extract_rpm(rpm: &Path, dest: &Path) {
    let out = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "set -euo pipefail; rpm2cpio {} | cpio -idm -D {} >/dev/null 2>&1",
            shell_quote(&rpm.to_string_lossy()),
            shell_quote(&dest.to_string_lossy())
        ))
        .output()
        .expect("extract rpm");
    assert!(
        out.status.success(),
        "rpm extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn sha256_file(p: &Path) -> String {
    let out = Command::new("sha256sum")
        .arg(p)
        .output()
        .expect("sha256sum");
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .expect("utf8")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

fn require_artifacts() -> Option<(PathBuf, PathBuf)> {
    match (find_deb(), find_rpm()) {
        (Some(d), Some(r)) => Some((d, r)),
        _ => {
            eprintln!(
                "SKIP artifact test: run deploy/packages/build.sh first (target/packages/*.deb + *.rpm missing)"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Binary + unit + fragment hash parity (§26, §59, §60)
// ---------------------------------------------------------------------------

#[test]
fn packaged_binary_matches_release_sha() {
    let Some((deb, rpm)) = require_artifacts() else {
        return;
    };
    let dd = tempdir("deb-bin");
    let rd = tempdir("rpm-bin");
    extract_deb(&deb, &dd);
    extract_rpm(&rpm, &rd);
    for (name, rel) in [
        (
            "synveil-scheduled-maintenance-once",
            "usr/bin/synveil-scheduled-maintenance-once",
        ),
        ("synveil-client", "usr/bin/synveil-client"),
        ("synveil-desktop", "usr/bin/synveil-desktop"),
    ] {
        let src = repo_root().join(format!("target/release/{name}"));
        assert!(
            src.is_file(),
            "release binary must exist: {}",
            src.display()
        );
        let src_sha = sha256_file(&src);
        assert_eq!(
            sha256_file(&dd.join(rel)),
            src_sha,
            "DEB {rel} must be byte-identical to release binary"
        );
        assert_eq!(
            sha256_file(&rd.join(rel)),
            src_sha,
            "RPM {rel} must be byte-identical to release binary"
        );
    }
    let _ = fs::remove_dir_all(&dd);
    let _ = fs::remove_dir_all(&rd);
}

#[test]
fn packaged_units_match_authoritative_sources() {
    let Some((deb, rpm)) = require_artifacts() else {
        return;
    };
    let dd = tempdir("deb-unit");
    let rd = tempdir("rpm-unit");
    extract_deb(&deb, &dd);
    extract_rpm(&rpm, &rd);
    for (src_rel, pkg_rel) in [
        (
            "deploy/systemd/synveil-scheduled-maintenance.service",
            "usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        ),
        (
            "deploy/systemd-user/synveil-client.service",
            "usr/lib/systemd/user/synveil-client.service",
        ),
        (
            "deploy/applications/synveil.desktop",
            "usr/share/applications/synveil.desktop",
        ),
        (
            "deploy/icons/hicolor/scalable/apps/synveil.svg",
            "usr/share/icons/hicolor/scalable/apps/synveil.svg",
        ),
        ("LICENSE", "usr/share/doc/synveil/LICENSE"),
        ("deploy/NOTICE", "usr/share/doc/synveil/NOTICE"),
        (
            "deploy/systemd/synveil-scheduled-maintenance.timer",
            "usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        ),
        (
            "deploy/sysusers.d/synveil.conf",
            "usr/lib/sysusers.d/synveil.conf",
        ),
        (
            "deploy/tmpfiles.d/synveil.conf",
            "usr/lib/tmpfiles.d/synveil.conf",
        ),
        (
            "deploy/config/synveil-scheduled-maintenance.env.example",
            "usr/share/synveil/synveil-scheduled-maintenance.env.example",
        ),
    ] {
        let src_sha = sha256_file(&repo_root().join(src_rel));
        assert_eq!(
            sha256_file(&dd.join(pkg_rel)),
            src_sha,
            "DEB {pkg_rel} must equal {src_rel}"
        );
        assert_eq!(
            sha256_file(&rd.join(pkg_rel)),
            src_sha,
            "RPM {pkg_rel} must equal {src_rel}"
        );
    }
    let _ = fs::remove_dir_all(&dd);
    let _ = fs::remove_dir_all(&rd);
}

#[test]
fn deb_rpm_normalized_payload_parity() {
    let Some((deb, rpm)) = require_artifacts() else {
        return;
    };
    let dd = tempdir("deb-par");
    let rd = tempdir("rpm-par");
    extract_deb(&deb, &dd);
    extract_rpm(&rpm, &rd);
    let rels = [
        "usr/bin/synveil-client",
        "usr/bin/synveil-desktop",
        "usr/bin/synveil-scheduled-maintenance-once",
        "usr/lib/systemd/user/synveil-client.service",
        "usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "usr/lib/sysusers.d/synveil.conf",
        "usr/lib/tmpfiles.d/synveil.conf",
        "usr/share/applications/synveil.desktop",
        "usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "usr/share/doc/synveil/LICENSE",
        "usr/share/doc/synveil/NOTICE",
        "usr/share/synveil/synveil-scheduled-maintenance.env.example",
    ];
    for rel in rels {
        let d = dd.join(rel);
        let r = rd.join(rel);
        assert!(d.is_file(), "DEB must contain {rel}");
        assert!(r.is_file(), "RPM must contain {rel}");
        assert_eq!(
            sha256_file(&d),
            sha256_file(&r),
            "DEB vs RPM content must match for {rel}"
        );
        let dm = fs::metadata(&d).expect("meta").permissions().mode() & 0o777;
        let rm = fs::metadata(&r).expect("meta").permissions().mode() & 0o777;
        assert_eq!(dm, rm, "DEB vs RPM mode must match for {rel}");
    }
    // No unexpected payload files (excluding directory skeleton etc/).
    let mut deb_files = BTreeSet::new();
    collect_files(&dd, &dd, &mut deb_files);
    let mut rpm_files = BTreeSet::new();
    collect_files(&rd, &rd, &mut rpm_files);
    let expected: BTreeSet<String> = rels.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        deb_files, expected,
        "DEB must contain exactly the expected payload files"
    );
    assert_eq!(
        rpm_files, expected,
        "RPM must contain exactly the expected payload files"
    );
    let _ = fs::remove_dir_all(&dd);
    let _ = fs::remove_dir_all(&rd);
}

#[test]
fn native_archives_have_no_source_or_build_paths() {
    let Some((deb, rpm)) = require_artifacts() else {
        return;
    };

    let members = Command::new("ar")
        .args(["t", deb.to_string_lossy().as_ref()])
        .output()
        .expect("list DEB ar members");
    assert!(
        members.status.success(),
        "ar member listing failed: {}",
        String::from_utf8_lossy(&members.stderr)
    );
    let member_text = String::from_utf8(members.stdout).expect("DEB ar member list is UTF-8");
    let members: Vec<&str> = member_text.lines().collect();
    assert_eq!(
        members,
        ["debian-binary", "control.tar.gz", "data.tar.gz"],
        "DEB must retain the real deterministic three-member layout"
    );

    let deb_paths = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "set -euo pipefail; ar p {} data.tar.gz | tar -tzf -",
            shell_quote(&deb.to_string_lossy())
        ))
        .output()
        .expect("list DEB data paths");
    assert!(
        deb_paths.status.success(),
        "DEB data path listing failed: {}",
        String::from_utf8_lossy(&deb_paths.stderr)
    );
    let rpm_paths = Command::new("rpm")
        .args(["-qpl", rpm.to_string_lossy().as_ref()])
        .output()
        .expect("list RPM payload paths");
    assert!(
        rpm_paths.status.success(),
        "RPM payload path listing failed: {}",
        String::from_utf8_lossy(&rpm_paths.stderr)
    );

    for (format, output) in [("DEB", deb_paths.stdout), ("RPM", rpm_paths.stdout)] {
        let paths = String::from_utf8(output).expect("package path list is UTF-8");
        for forbidden in ["/mnt/Projects/", "/tmp/", "target/", "target\\", "build/"] {
            assert!(
                !paths.contains(forbidden),
                "{format} payload must not contain development path {forbidden}: {paths}"
            );
        }
    }
}

fn collect_files(root: &Path, cur: &Path, out: &mut BTreeSet<String>) {
    let entries = fs::read_dir(cur).expect("readdir");
    for e in entries.filter_map(|x| x.ok()) {
        let p = e.path();
        if p.is_dir() {
            collect_files(root, &p, out);
        } else if p.is_file() {
            out.insert(
                p.strip_prefix(root)
                    .expect("prefix")
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
}

#[test]
fn payload_file_modes_follow_contract() {
    let Some((deb, _)) = require_artifacts() else {
        return;
    };
    let dd = tempdir("deb-mode");
    extract_deb(&deb, &dd);
    let mode = |rel: &str| {
        fs::metadata(dd.join(rel))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777
    };
    for rel in ["usr/bin/synveil-client", "usr/bin/synveil-desktop"] {
        assert_eq!(mode(rel), 0o755, "mode for {rel}");
    }
    assert_eq!(mode("usr/bin/synveil-scheduled-maintenance-once"), 0o755);
    for rel in [
        "usr/lib/systemd/user/synveil-client.service",
        "usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "usr/lib/sysusers.d/synveil.conf",
        "usr/lib/tmpfiles.d/synveil.conf",
        "usr/share/applications/synveil.desktop",
        "usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "usr/share/doc/synveil/LICENSE",
        "usr/share/doc/synveil/NOTICE",
        "usr/share/synveil/synveil-scheduled-maintenance.env.example",
    ] {
        assert_eq!(mode(rel), 0o644, "mode for {rel}");
    }
    assert_eq!(mode("etc/synveil"), 0o750);
    assert_eq!(mode("etc/synveil/credentials"), 0o700);
    let _ = fs::remove_dir_all(&dd);
}

// ---------------------------------------------------------------------------
// Secrets, identity, sandbox preservation (§11, §12, §13, §15)
// ---------------------------------------------------------------------------

#[test]
fn no_secret_payload_in_either_package() {
    let Some((deb, rpm)) = require_artifacts() else {
        return;
    };
    let dd = tempdir("deb-sec");
    let rd = tempdir("rpm-sec");
    extract_deb(&deb, &dd);
    extract_rpm(&rpm, &rd);
    for (tag, root) in [("DEB", &dd), ("RPM", &rd)] {
        assert!(
            !root.join("etc/synveil/credentials/database-url").exists(),
            "{tag} must never contain the credential file"
        );
        assert!(
            !root
                .join("etc/synveil/synveil-scheduled-maintenance.env")
                .exists(),
            "{tag} must not seed the admin env file (template only)"
        );
        // Text payload scan: allow only the documented template placeholder line.
        for rel in [
            "usr/lib/systemd/user/synveil-client.service",
            "usr/lib/systemd/system/synveil-scheduled-maintenance.service",
            "usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
            "usr/lib/sysusers.d/synveil.conf",
            "usr/lib/tmpfiles.d/synveil.conf",
            "usr/share/applications/synveil.desktop",
            "usr/share/icons/hicolor/scalable/apps/synveil.svg",
            "usr/share/doc/synveil/LICENSE",
            "usr/share/doc/synveil/NOTICE",
            "usr/share/synveil/synveil-scheduled-maintenance.env.example",
        ] {
            let content = read(&root.join(rel));
            for line in content.lines() {
                let low = line.to_lowercase();
                let is_template_placeholder =
                    rel.ends_with(".env.example") && low.contains("database_url=postgresql://...");
                if is_template_placeholder {
                    continue;
                }
                assert!(
                    !low.contains("postgresql://") || !low.contains('@'),
                    "{tag} {rel} must not contain a real credential URL: {line:?}"
                );
                assert!(
                    !low.contains("begin private key"),
                    "{tag} {rel} must not contain private keys"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(&dd);
    let _ = fs::remove_dir_all(&rd);
}

#[test]
fn no_numeric_synveil_uid_gid_baked_in() {
    let sysusers = read(&repo_root().join("deploy/sysusers.d/synveil.conf"));
    // sysusers must use '-' auto-allocation, never a hardcoded number.
    for line in sysusers.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = t.split_whitespace().collect();
        // Format: <type> <name> <id> ...
        assert!(
            fields.len() >= 3,
            "sysusers line must have 3+ fields: {t:?}"
        );
        assert_eq!(
            fields[2], "-",
            "sysusers ID must be '-' (auto-allocated), got {:?}",
            fields[2]
        );
    }
    for script_rel in [
        "deploy/packages/debian/postinst",
        "deploy/packages/debian/prerm",
        "deploy/packages/debian/postrm",
        "deploy/packages/rpm/synveil.spec.tmpl",
        "deploy/packages/build.sh",
    ] {
        let content = read(&repo_root().join(script_rel));
        // Scan code lines only: documentation comments legitimately mention
        // useradd (to forbid it) and numeric-UID policy (to forbid it).
        let code: String = content
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("useradd"),
            "{script_rel} must use systemd-sysusers, not useradd"
        );
        assert!(
            !code.contains("groupadd"),
            "{script_rel} must not use groupadd"
        );
        // No chown with numeric owner (e.g. chown 1001 / chown 0:1001 for synveil paths).
        for line in code.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                continue;
            }
            if t.contains("chown") {
                assert!(
                    !regex_numeric_chown(t),
                    "{script_rel} must use symbolic identity, not numeric UID/GID: {t:?}"
                );
            }
        }
    }
}

fn regex_numeric_chown(line: &str) -> bool {
    // Detect `chown <digits>` / `chown <digits>:` / `chown :<digits>` patterns.
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if line[i..].starts_with("chown") {
            let rest = &line[i + 5..];
            // Look for a digit adjacent to chown's owner/group argument.
            let mut chars = rest.chars().peekable();
            // Skip spaces and '-' flags (e.g. chown -R is banned elsewhere anyway).
            while let Some(&c) = chars.peek() {
                if c == ' ' || c == '\t' {
                    chars.next();
                } else {
                    break;
                }
            }
            if let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    return true;
                }
                // root:1234 or 0:synveil forms
                let arg: String = chars.take_while(|&c| c != ' ' && c != '\t').collect();
                if arg
                    .split(':')
                    .any(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
                {
                    // Allow root:root (non-numeric) — only flag numeric parts that are not 0?
                    // 0 is root and acceptable only as `root`; numeric 0 alone is still numeric style.
                    // Build scripts legitimately use --owner=0 for tar archives (not chown), so only
                    // flag chown lines with non-zero numerics or UID-style numerics.
                    let numeric_parts: Vec<&str> = arg
                        .split(':')
                        .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                        .collect();
                    if numeric_parts.iter().any(|p| *p != "0") {
                        return true;
                    }
                }
            }
            return false;
        }
        i += 1;
    }
    false
}

#[test]
fn packaged_service_preserves_credential_boundary_and_sandbox() {
    let svc = read(&repo_root().join("deploy/systemd/synveil-scheduled-maintenance.service"));
    // Prompt 77 delivery (conceptual LoadCredential flow).
    assert!(
        svc.contains("LoadCredential=database-url:/etc/synveil/credentials/database-url"),
        "service must use LoadCredential for the credential source"
    );
    assert!(
        svc.contains("SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url"),
        "service must expose the non-secret credential path"
    );
    assert!(
        !svc.lines().any(|l| {
            let t = l.trim_start();
            !t.starts_with('#')
                && (t.starts_with("Environment=DATABASE_URL")
                    || t.starts_with("EnvironmentFile=") && t.contains("credentials"))
        }),
        "service must not deliver secrets via Environment=/EnvironmentFile credentials"
    );
    // Prompt 75 identity.
    assert!(
        svc.contains("\nUser=synveil\n"),
        "service must run as User=synveil"
    );
    assert!(
        svc.contains("\nGroup=synveil\n"),
        "service must run as Group=synveil"
    );
    // Prompt 78 sandbox spot-checks (full matrix lives in linux_sandbox_hardening_units).
    for directive in [
        "NoNewPrivileges=yes",
        "RestrictSUIDSGID=yes",
        "CapabilityBoundingSet=",
        "AmbientCapabilities=",
        "ProtectSystem=strict",
        "ProtectHome=yes",
        "PrivateTmp=yes",
        "PrivateDevices=yes",
        "DevicePolicy=closed",
        "InaccessiblePaths=/etc/synveil/credentials",
        "ProtectKernelTunables=yes",
        "ProtectKernelModules=yes",
        "ProtectKernelLogs=yes",
        "ProtectControlGroups=yes",
        "ProtectProc=invisible",
        "ProcSubset=pid",
        "RestrictNamespaces=yes",
        "RestrictRealtime=yes",
        "LockPersonality=yes",
        "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6",
        "SystemCallArchitectures=native",
        "MemoryDenyWriteExecute=yes",
        "UMask=0077",
    ] {
        assert!(
            svc.contains(directive),
            "packaged service must preserve sandbox directive {directive}"
        );
    }
    assert!(
        !svc.contains("Restart=always"),
        "one-shot must not use Restart=always"
    );
}

// ---------------------------------------------------------------------------
// Lifecycle: no auto-start/enable, upgrade-vs-remove, preservation (§32-§44)
// ---------------------------------------------------------------------------

fn lifecycle_script_texts() -> Vec<(String, String)> {
    vec![
        (
            "debian/postinst".to_string(),
            read(&repo_root().join("deploy/packages/debian/postinst")),
        ),
        (
            "debian/prerm".to_string(),
            read(&repo_root().join("deploy/packages/debian/prerm")),
        ),
        (
            "debian/postrm".to_string(),
            read(&repo_root().join("deploy/packages/debian/postrm")),
        ),
        (
            "rpm/spec".to_string(),
            read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl")),
        ),
    ]
}

#[test]
fn no_automatic_timer_enable_or_service_start() {
    for (name, content) in lifecycle_script_texts() {
        let mut forbidden_found = None;
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with('#')
                || t.starts_with("printf")
                || t.starts_with("\"")
                || t.starts_with("log ")
                || t.starts_with("echo")
            {
                // Comments/log lines may mention the words; only active invocations count.
                // Conservative: skip lines that merely document NOT doing it.
                if t.contains("NOT auto")
                    || t.contains("never")
                    || t.contains("NEVER")
                    || t.contains("explicit")
                {
                    continue;
                }
                // printf/log strings describing policy are fine.
                if t.starts_with("printf") || t.starts_with("log") {
                    continue;
                }
            }
            if t.starts_with('#') {
                continue;
            }
            if t.contains("systemctl enable") || t.contains("systemctl start") {
                forbidden_found = Some(t.to_string());
            }
        }
        assert!(
            forbidden_found.is_none(),
            "{name} must never auto-enable/start (found: {forbidden_found:?})"
        );
    }
    let svc = read(&repo_root().join("deploy/systemd/synveil-scheduled-maintenance.service"));
    // Only active (non-comment) directives count; the unit documents that it is
    // intentionally NOT WantedBy=multi-user.target.
    let active: Vec<&str> = svc
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with(';') && !t.is_empty()
        })
        .collect();
    assert!(
        !active
            .iter()
            .any(|l| l.contains("WantedBy=multi-user.target")),
        "service must not self-enable via WantedBy"
    );
}

#[test]
fn no_maintenance_execution_from_package_hooks() {
    for (name, content) in lifecycle_script_texts() {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                continue;
            }
            // %files payload listings name the binary path without executing it.
            if t.starts_with("%attr") || t.starts_with('/') || t.starts_with("\"/") {
                continue;
            }
            // Policy log/printf lines describe NOT running it.
            if t.starts_with("log")
                || t.starts_with("printf")
                || t.contains("NOT ")
                || t.contains("NEVER")
                || t.contains("never run")
            {
                continue;
            }
            assert!(
                !t.contains("synveil-scheduled-maintenance-once"),
                "{name} must not execute the maintenance binary (found: {t:?})"
            );
        }
    }
}

#[test]
fn upgrade_vs_remove_distinction_is_explicit() {
    let prerm = read(&repo_root().join("deploy/packages/debian/prerm"));
    assert!(
        prerm.contains("upgrade"),
        "prerm must handle upgrade distinctly"
    );
    assert!(
        prerm.contains("remove"),
        "prerm must handle final removal distinctly"
    );
    let postrm = read(&repo_root().join("deploy/packages/debian/postrm"));
    assert!(
        postrm.contains("upgrade"),
        "postrm must handle upgrade distinctly"
    );
    assert!(
        postrm.contains("purge"),
        "postrm must handle purge distinctly"
    );
    let spec = read(&repo_root().join("deploy/packages/rpm/synveil.spec.tmpl"));
    assert!(
        spec.contains("\"$1\" = \"0\""),
        "%preun/%postun must test $1 == 0 (erase) vs upgrade"
    );
}

#[test]
fn removal_is_data_preserving_in_all_hooks() {
    for (name, content) in lifecycle_script_texts() {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                continue;
            }
            // No hook may recursively delete state/config/credential paths.
            for target in ["/var/lib/synveil", "/etc/synveil", "database-url"] {
                if (t.contains("rm ") || t.contains("rm\t"))
                    && t.contains(target)
                    && !t.contains("rm -f \"$full\"")
                    && !t.contains("rm -f \"$tmp\"")
                {
                    // Allow only the documented purge-via-admin-layer references in comments/logs.
                    panic!("{name} must not delete {target} from package hooks (found: {t:?})");
                }
            }
        }
    }
    let postrm = read(&repo_root().join("deploy/packages/debian/postrm"));
    assert!(
        postrm.contains("data-preserving"),
        "postrm purge must document data-preserving parity"
    );
}

#[test]
fn external_storage_is_never_touched() {
    for (name, content) in lifecycle_script_texts() {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with('#') || t.starts_with("log") || t.starts_with("printf") {
                continue;
            }
            for banned in [
                "chown -R",
                "chmod -R",
                "/srv/",
                "/mnt/",
                "/home/",
                "object-store",
            ] {
                if t.contains(banned) && !t.contains("never") && !t.contains("NEVER") {
                    panic!("{name} must not touch external storage (found {banned:?} in: {t:?})");
                }
            }
        }
    }
}

#[test]
fn install_time_hooks_use_no_network() {
    for (name, content) in lifecycle_script_texts() {
        // Strip comments before scanning so documentation mentioning curl does not trip the gate.
        let code: String = content
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in [
            "curl",
            "wget",
            "git clone",
            "cargo install",
            "apt-get",
            "dnf ",
            "yum ",
            "pip install",
        ] {
            assert!(
                !code.contains(banned),
                "{name} hooks must perform 0 network downloads (found {banned:?})"
            );
        }
    }
    let build = read(&repo_root().join("deploy/packages/build.sh"));
    let build_code: String = build
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !build_code.contains("curl | sh") && !build_code.contains("curl|sh"),
        "build.sh must not curl|sh"
    );
    assert!(
        !build_code
            .lines()
            .any(|l| l.trim_start().starts_with("eval")),
        "build.sh must not use eval"
    );
}

#[test]
fn shell_scripts_are_syntax_clean_and_safe() {
    let scripts = [
        "deploy/packages/build.sh",
        "deploy/packages/common/version.sh",
        "deploy/packages/common/arch.sh",
        "deploy/packages/common/payload.sh",
        "deploy/packages/debian/postinst",
        "deploy/packages/debian/prerm",
        "deploy/packages/debian/postrm",
    ];
    for rel in scripts {
        let p = repo_root().join(rel);
        let interpreter =
            if rel.ends_with("postinst") || rel.ends_with("prerm") || rel.ends_with("postrm") {
                "sh"
            } else {
                "bash"
            };
        let out = Command::new(interpreter)
            .arg("-n")
            .arg(&p)
            .output()
            .expect("syntax check");
        assert!(
            out.status.success(),
            "{rel} must be syntax-clean: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let content = read(&p);
        let code: String = content
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("rm -rf /") && !code.contains("rm -rf \"/\""),
            "{rel} must not contain rm -rf /"
        );
        assert!(
            !code.lines().any(|l| {
                let t = l.trim_start();
                t.starts_with("eval ") || t == "eval"
            }),
            "{rel} must not use eval"
        );
    }
}

// ---------------------------------------------------------------------------
// CI gate integrity (§77, §83)
// ---------------------------------------------------------------------------

#[test]
fn package_ci_workflow_exists_and_is_failure_honest() {
    let ci = repo_root().join(".github/workflows/linux-packages.yml");
    assert!(
        ci.is_file(),
        "CI workflow .github/workflows/linux-packages.yml must exist"
    );
    let content = read(&ci);
    assert!(
        content.contains(".deb") || content.contains("deb"),
        "CI must build DEB"
    );
    assert!(
        content.contains(".rpm") || content.contains("rpm"),
        "CI must build RPM"
    );
    assert!(
        !content.contains("continue-on-error: true"),
        "CI must not mask failures with continue-on-error"
    );
    // No blanket `|| true` in run steps (diagnostic-only allowances must be explicit).
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        if t.contains("|| true") {
            panic!("CI must not mask correctness gates with `|| true` (found: {t:?})");
        }
    }
    assert!(
        content.contains("signing deferred") || content.contains("Unsigned"),
        "CI must document unsigned-artifact status"
    );
}
