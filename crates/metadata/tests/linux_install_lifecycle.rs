#![cfg(target_os = "linux")]

//! Prompt 76 — Linux Installation, Upgrade & Data-Preserving Uninstall Foundation.
//!
//! Package-neutral lifecycle tests that prove deterministic mechanics for:
//! fresh install, idempotent reinstall, upgrade, failed-upgrade recovery,
//! uninstall, purge separation, ownership, systemd/sysusers/tmpfiles integration,
//! and staged-root safety without mutating the developer host.
//!
//! All tests operate on disposable temporary roots (e.g. /tmp/synveil-install-*)
//! and never write to /usr, /etc, /var, /run on the host. They are runnable via
//! `cargo test --workspace --locked` without PostgreSQL and without root.

use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use synveil_metadata::DatabaseConfig;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn install_script() -> PathBuf {
    repo_root().join("deploy/install/install.sh")
}
fn uninstall_script() -> PathBuf {
    repo_root().join("deploy/install/uninstall.sh")
}
fn manifest_path() -> PathBuf {
    repo_root().join("deploy/install/MANIFEST")
}
fn service_src() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.service")
}
fn timer_src() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.timer")
}
fn sysusers_src() -> PathBuf {
    repo_root().join("deploy/sysusers.d/synveil.conf")
}
fn tmpfiles_src() -> PathBuf {
    repo_root().join("deploy/tmpfiles.d/synveil.conf")
}
fn template_src() -> PathBuf {
    repo_root().join("deploy/config/synveil-scheduled-maintenance.env.example")
}

fn read_manifest() -> String {
    fs::read_to_string(manifest_path()).expect("read MANIFEST")
}
#[allow(dead_code)]
fn credential_dir_src() -> PathBuf {
    // No source file; manifest entry is "-" for directory
    PathBuf::from("/etc/synveil/credentials")
}

fn parse_manifest() -> Vec<(String, String, String, String, String, String)> {
    let content = read_manifest();
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut parts = t.split_whitespace();
        let source = parts.next().unwrap_or("").to_string();
        let dest = parts.next().unwrap_or("").to_string();
        let mode = parts.next().unwrap_or("").to_string();
        let owner = parts.next().unwrap_or("").to_string();
        let group = parts.next().unwrap_or("").to_string();
        let class = parts.next().unwrap_or("").to_string();
        assert!(
            !source.is_empty()
                && !dest.is_empty()
                && !mode.is_empty()
                && !owner.is_empty()
                && !group.is_empty()
                && !class.is_empty(),
            "malformed manifest line: {line}"
        );
        out.push((source, dest, mode, owner, group, class));
    }
    out
}

fn temp_root(prefix: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "synveil-install-{}-{}",
        prefix,
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn temp_binary(content: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "synveil-bin-{}-{}",
        uuid::Uuid::now_v7().simple(),
        "bin"
    ));
    fs::write(&p, content).unwrap();
    // chmod 0755 so install can copy
    let mut perm = fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&p, perm).unwrap();
    p
}

fn run_install(root: &Path, binary: &Path, extra_env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new("bash");
    cmd.arg(install_script())
        .arg(format!("--root={}", root.display()))
        .arg(format!("--binary={}", binary.display()))
        // The lifecycle tests use one disposable executable as the byte
        // source for all three package-owned binaries.  Native package tests
        // exercise distinct release artifacts; these tests only need a valid
        // staged payload for install/upgrade/uninstall mechanics.
        .arg(format!("--client-binary={}", binary.display()))
        .arg(format!("--desktop-binary={}", binary.display()));
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn install.sh")
}

fn run_install_with_fail(root: &Path, binary: &Path, fail_after: usize) -> std::process::Output {
    run_install(
        root,
        binary,
        &[("SYNVEIL_INSTALL_FAIL_AFTER", &fail_after.to_string())],
    )
}

fn run_uninstall(root: &Path, purge: bool) -> std::process::Output {
    let mut cmd = Command::new("bash");
    cmd.arg(uninstall_script())
        .arg(format!("--root={}", root.display()));
    if purge {
        cmd.arg("--purge");
    }
    cmd.output().expect("spawn uninstall.sh")
}

fn file_mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn assert_mode(path: &Path, expected: u32) {
    let m = file_mode(path);
    assert_eq!(
        m,
        expected,
        "mode for {} expected {:o} got {:o}",
        path.display(),
        expected,
        m
    );
}

fn hash_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

// ---------------------------------------------------------------------------
// 1. Fresh staged install
// ---------------------------------------------------------------------------
#[test]
fn fresh_staged_install_produces_expected_tree() {
    let root = temp_root("fresh");
    let bin = temp_binary("fresh-v1-binary-content-12345");
    let out = run_install(&root, &bin, &[]);
    assert!(
        out.status.success(),
        "fresh install failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Required PACKAGE artifacts must exist at exact paths
    let expected_files = [
        ("/usr/bin/synveil-client", 0o755),
        ("/usr/bin/synveil-desktop", 0o755),
        ("/usr/bin/synveil-scheduled-maintenance-once", 0o755),
        ("/usr/lib/systemd/user/synveil-client.service", 0o644),
        ("/usr/share/applications/synveil.desktop", 0o644),
        ("/usr/share/icons/hicolor/scalable/apps/synveil.svg", 0o644),
        ("/usr/share/doc/synveil/LICENSE", 0o644),
        ("/usr/share/doc/synveil/NOTICE", 0o644),
        (
            "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
            0o644,
        ),
        (
            "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
            0o644,
        ),
        ("/usr/lib/sysusers.d/synveil.conf", 0o644),
        ("/usr/lib/tmpfiles.d/synveil.conf", 0o644),
        (
            "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
            0o644,
        ),
    ];
    for (dest, mode) in expected_files {
        let full = root.join(dest.trim_start_matches('/'));
        assert!(full.exists(), "PACKAGE artifact missing: {dest}");
        assert_mode(&full, mode);
    }

    // CONFIG_DIRECTORY must exist with 0750
    let etc_synveil = root.join("etc/synveil");
    assert!(etc_synveil.is_dir(), "/etc/synveil directory must exist");
    assert_mode(&etc_synveil, 0o750);

    // CREDENTIAL_DIRECTORY must exist with 0700 (Prompt 77)
    let cred_dir = root.join("etc/synveil/credentials");
    assert!(
        cred_dir.is_dir(),
        "/etc/synveil/credentials directory must exist (0700 root:root)"
    );
    assert_mode(&cred_dir, 0o700);

    // Secret env file must NOT be created on fresh install (template policy A)
    let env_file = root.join("etc/synveil/synveil-scheduled-maintenance.env");
    assert!(
        !env_file.exists(),
        "fresh install must not create working env file; template is at /usr/share"
    );
    // Credential secret file must NOT be created on fresh install
    let cred_file = root.join("etc/synveil/credentials/database-url");
    assert!(
        !cred_file.exists(),
        "fresh install must not invent database secret; admin provisions it"
    );

    // STATE_DIRECTORY must NOT be seeded (tmpfiles manages it)
    let state = root.join("var/lib/synveil");
    assert!(
        !state.exists(),
        "/var/lib/synveil must not be created by package payload (tmpfiles owns it)"
    );
    // RUNTIME_MANAGED must not exist
    let run = root.join("run/synveil");
    assert!(
        !run.exists(),
        "/run/synveil must not be created by package payload (RuntimeDirectory owns it)"
    );

    // No unexpected PACKAGE files beyond manifest + allowed parent dirs
    // Enumerate all files under root and ensure each is in manifest destinations
    let manifest = parse_manifest();
    let package_dests: HashSet<String> = manifest
        .iter()
        .filter(|(_, _, _, _, _, c)| c == "PACKAGE")
        .map(|(_, d, _, _, _, _)| d.clone())
        .collect();
    let mut found_files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let p = entry.path();
            if p.is_dir() && !p.is_symlink() {
                stack.push(p);
            } else if p.is_file() || p.is_symlink() {
                // compute destination-like path
                let rel = p.strip_prefix(&root).unwrap();
                let dest = format!("/{}", rel.display());
                found_files.push(dest);
            }
        }
    }
    for f in &found_files {
        // Files under /etc/synveil are admin config (not in manifest as PACKAGE) — but fresh install has none
        // State sentinel also not present; so any file must be in package set
        // Allow nothing else
        assert!(
            package_dests.contains(f),
            "unexpected file in staged root not in PACKAGE manifest: {f}"
        );
    }

    // Cleanup
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 2. Package artifact manifest
// ---------------------------------------------------------------------------
#[test]
fn package_artifact_manifest_is_authoritative_and_collision_free() {
    let manifest = parse_manifest();
    // Every PACKAGE-owned file appears exactly once
    let mut dest_counts: HashMap<String, usize> = HashMap::new();
    for (_, dest, _, _, _, class) in &manifest {
        if class == "PACKAGE" {
            *dest_counts.entry(dest.clone()).or_insert(0) += 1;
        }
    }
    for (dest, cnt) in &dest_counts {
        assert_eq!(
            *cnt, 1,
            "PACKAGE destination appears {cnt} times, must be exactly once: {dest}"
        );
    }
    // No destination collisions across all classes
    let mut all_dests: HashSet<String> = HashSet::new();
    for (_, dest, _, _, _, _) in &manifest {
        assert!(
            all_dests.insert(dest.clone()),
            "destination collision in manifest (two entries for same dest): {dest}"
        );
    }
    // Must contain exactly the expected PACKAGE set.
    let expected_package = [
        "/usr/bin/synveil-client",
        "/usr/bin/synveil-desktop",
        "/usr/bin/synveil-scheduled-maintenance-once",
        "/usr/lib/systemd/user/synveil-client.service",
        "/usr/share/applications/synveil.desktop",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "/usr/share/doc/synveil/LICENSE",
        "/usr/share/doc/synveil/NOTICE",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "/usr/lib/sysusers.d/synveil.conf",
        "/usr/lib/tmpfiles.d/synveil.conf",
        "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
    ];
    assert_eq!(
        dest_counts.len(),
        expected_package.len(),
        "expected {} PACKAGE entries, got {}: {:?}",
        expected_package.len(),
        dest_counts.len(),
        dest_counts.keys()
    );
    for e in expected_package {
        assert!(dest_counts.contains_key(e), "PACKAGE manifest missing: {e}");
    }
    // Must contain CONFIG_DIRECTORY /etc/synveil
    let has_config = manifest
        .iter()
        .any(|(_, d, _, _, _, c)| d == "/etc/synveil" && c == "CONFIG_DIRECTORY");
    assert!(
        has_config,
        "manifest must contain CONFIG_DIRECTORY /etc/synveil"
    );
    // Must contain STATE_DIRECTORY and RUNTIME_MANAGED for contract completeness
    assert!(
        manifest
            .iter()
            .any(|(_, d, _, _, _, c)| d == "/var/lib/synveil" && c == "STATE_DIRECTORY"),
        "manifest must contain STATE_DIRECTORY /var/lib/synveil"
    );
    assert!(
        manifest
            .iter()
            .any(|(_, d, _, _, _, c)| d == "/run/synveil" && c == "RUNTIME_MANAGED"),
        "manifest must contain RUNTIME_MANAGED /run/synveil"
    );
    assert!(
        manifest.iter().any(
            |(_, d, _, _, _, c)| d == "/etc/synveil/credentials" && c == "CREDENTIAL_DIRECTORY"
        ),
        "manifest must contain CREDENTIAL_DIRECTORY /etc/synveil/credentials"
    );
    // Single authoritative file: common.sh and install/uninstall must not duplicate destinations
    let install_content = fs::read_to_string(install_script()).unwrap();
    let uninstall_content = fs::read_to_string(uninstall_script()).unwrap();
    // They should source common.sh and read MANIFEST, not hardcode destinations independently
    assert!(
        install_content.contains("MANIFEST") || install_content.contains("manifest"),
        "install.sh must reference MANIFEST (authoritative mapping)"
    );
    assert!(
        uninstall_content.contains("MANIFEST") || uninstall_content.contains("manifest"),
        "uninstall.sh must reference MANIFEST"
    );
    // Ensure install.sh does not hardcode all destinations separately (count of dest strings)
    // It's okay to have some, but not duplicate entire manifest.
    let hardcoded_in_install = expected_package
        .iter()
        .filter(|d| install_content.contains(*d))
        .count();
    // Should be 0 hardcoded destinations in install.sh (it reads manifest)
    assert_eq!(
        hardcoded_in_install, 0,
        "install.sh should not hardcode PACKAGE destinations; it must read MANIFEST (found {hardcoded_in_install} hardcodes)"
    );
}

// ---------------------------------------------------------------------------
// 3. Idempotent reinstall
// ---------------------------------------------------------------------------
#[test]
fn idempotent_reinstall_keeps_identical_package_state() {
    let root = temp_root("idempotent");
    let bin = temp_binary("idempotent-v1-same-content");
    let out1 = run_install(&root, &bin, &[]);
    assert!(out1.status.success(), "first install failed");
    // Snapshot hashes of PACKAGE files
    let snap1: HashMap<String, Vec<u8>> = {
        let mut m = HashMap::new();
        for (_, dest, _, _, _, class) in parse_manifest() {
            if class != "PACKAGE" {
                continue;
            }
            let full = root.join(dest.trim_start_matches('/'));
            m.insert(dest, hash_file(&full));
        }
        m
    };
    let modes1: HashMap<String, u32> = {
        let mut m = HashMap::new();
        for (_, dest, _, _, _, class) in parse_manifest() {
            if class != "PACKAGE" {
                continue;
            }
            let full = root.join(dest.trim_start_matches('/'));
            m.insert(dest, file_mode(&full));
        }
        m
    };
    let out2 = run_install(&root, &bin, &[]);
    assert!(
        out2.status.success(),
        "second install (idempotent) failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    for (_, dest, _, _, _, class) in parse_manifest() {
        if class != "PACKAGE" {
            continue;
        }
        let full = root.join(dest.trim_start_matches('/'));
        assert!(
            full.exists(),
            "PACKAGE file missing after second install: {dest}"
        );
        let h = hash_file(&full);
        assert_eq!(
            h, snap1[&dest],
            "PACKAGE file checksum changed after idempotent reinstall: {dest}"
        );
        let mode = file_mode(&full);
        assert_eq!(
            mode, modes1[&dest],
            "mode changed after idempotent reinstall: {dest}"
        );
    }
    // Config directory still exists, no config file yet
    assert!(root.join("etc/synveil").exists());
    assert!(
        !root
            .join("etc/synveil/synveil-scheduled-maintenance.env")
            .exists()
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 4. Configuration preservation
// ---------------------------------------------------------------------------
#[test]
fn config_preservation_across_reinstall_and_upgrade() {
    let root = temp_root("config-preserve");
    let bin_v1 = temp_binary("config-test-v1");
    let out = run_install(&root, &bin_v1, &[]);
    assert!(out.status.success());
    // Create admin config
    let env_path = root.join("etc/synveil/synveil-scheduled-maintenance.env");
    let original = "DATABASE_URL=postgresql://admin:secret@127.0.0.1:5432/synveil\nSYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS=120\n";
    fs::write(&env_path, original).unwrap();
    // chmod 0640 like production
    let mut perm = fs::metadata(&env_path).unwrap().permissions();
    perm.set_mode(0o640);
    fs::set_permissions(&env_path, perm).unwrap();
    let orig_bytes = fs::read(&env_path).unwrap();

    // Reinstall same version — must be byte-identical
    let out2 = run_install(&root, &bin_v1, &[]);
    assert!(out2.status.success());
    assert_eq!(
        fs::read(&env_path).unwrap(),
        orig_bytes,
        "config changed after reinstall"
    );

    // Upgrade to v2 — must still be byte-identical
    let bin_v2 = temp_binary("config-test-v2-upgraded-binary-with-different-content-longer");
    let out3 = run_install(&root, &bin_v2, &[]);
    assert!(out3.status.success());
    assert_eq!(
        fs::read(&env_path).unwrap(),
        orig_bytes,
        "config changed after upgrade"
    );
    // Verify binary actually upgraded
    let bin_dest = root.join("usr/bin/synveil-scheduled-maintenance-once");
    assert_eq!(fs::read(&bin_dest).unwrap(), fs::read(&bin_v2).unwrap());

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

// ---------------------------------------------------------------------------
// 5. Persistent-state preservation
// ---------------------------------------------------------------------------
#[test]
fn persistent_state_survives_reinstall_and_upgrade() {
    let root = temp_root("state-preserve");
    let bin_v1 = temp_binary("state-v1");
    let out = run_install(&root, &bin_v1, &[]);
    assert!(out.status.success());
    // Create sentinel under /var/lib/synveil (simulating runtime state)
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&state_dir).unwrap();
    let sentinel = state_dir.join("sentinel.dat");
    let state_content = "state-v1-persistent-data-should-survive";
    fs::write(&sentinel, state_content).unwrap();
    // also test nested file
    let nested = state_dir.join("nested/bookkeeping.json");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, r#"{"epoch": 1}"#).unwrap();

    // Reinstall
    let out2 = run_install(&root, &bin_v1, &[]);
    assert!(out2.status.success());
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), state_content);
    assert_eq!(fs::read_to_string(&nested).unwrap(), r#"{"epoch": 1}"#);

    // Upgrade
    let bin_v2 = temp_binary("state-v2-upgraded");
    let out3 = run_install(&root, &bin_v2, &[]);
    assert!(out3.status.success());
    assert_eq!(
        fs::read_to_string(&sentinel).unwrap(),
        state_content,
        "state lost after upgrade"
    );
    assert_eq!(fs::read_to_string(&nested).unwrap(), r#"{"epoch": 1}"#);

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

// ---------------------------------------------------------------------------
// 6. External user-data preservation
// ---------------------------------------------------------------------------
#[test]
fn external_user_data_never_modified_by_install_upgrade_uninstall() {
    let root = temp_root("external-preserve");
    let bin_v1 = temp_binary("external-v1");
    let bin_v2 = temp_binary("external-v2");
    // Create external storage pool outside staged root (simulate /srv/synveil-data)
    let external = std::env::temp_dir().join(format!(
        "synveil-external-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(external.join("pool/subdir")).unwrap();
    fs::write(external.join("pool/data.txt"), "user-data-keep-123").unwrap();
    fs::write(
        external.join("pool/subdir/nested.bin"),
        vec![0u8, 1, 2, 3, 255],
    )
    .unwrap();
    let data_before = fs::read(external.join("pool/data.txt")).unwrap();
    let nested_before = fs::read(external.join("pool/subdir/nested.bin")).unwrap();

    // Install
    let out = run_install(&root, &bin_v1, &[]);
    assert!(out.status.success());
    assert_eq!(
        fs::read(external.join("pool/data.txt")).unwrap(),
        data_before
    );
    assert_eq!(
        fs::read(external.join("pool/subdir/nested.bin")).unwrap(),
        nested_before
    );

    // Upgrade
    let out2 = run_install(&root, &bin_v2, &[]);
    assert!(out2.status.success());
    assert_eq!(
        fs::read(external.join("pool/data.txt")).unwrap(),
        data_before
    );

    // Uninstall
    let out3 = run_uninstall(&root, false);
    assert!(out3.status.success());
    assert_eq!(
        fs::read(external.join("pool/data.txt")).unwrap(),
        data_before
    );

    // Purge (still must not touch external)
    // Reinstall first to have something to purge
    let out4 = run_install(&root, &bin_v2, &[]);
    assert!(out4.status.success());
    let out5 = run_uninstall(&root, true);
    assert!(out5.status.success());
    assert_eq!(
        fs::read(external.join("pool/data.txt")).unwrap(),
        data_before,
        "external data deleted by purge (forbidden)"
    );
    assert_eq!(
        fs::read(external.join("pool/subdir/nested.bin")).unwrap(),
        nested_before
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

// ---------------------------------------------------------------------------
// 7. Upgrade package-owned artifact
// ---------------------------------------------------------------------------
#[test]
fn upgrade_replaces_package_artifacts_while_preserving_config_and_state() {
    let root = temp_root("upgrade");
    let bin_v1 = temp_binary("UPGRADE-V1-CONTENT-abc");
    let out1 = run_install(&root, &bin_v1, &[]);
    assert!(out1.status.success());
    // Snapshot original service content
    let svc_path = root.join("usr/lib/systemd/system/synveil-scheduled-maintenance.service");
    let svc_before = fs::read(&svc_path).unwrap();

    // Create config and state
    fs::write(
        root.join("etc/synveil/synveil-scheduled-maintenance.env"),
        "DATABASE_URL=postgresql://user:pass@localhost/db\n",
    )
    .unwrap();
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("keep"), "keep-state").unwrap();
    let config_before =
        fs::read(root.join("etc/synveil/synveil-scheduled-maintenance.env")).unwrap();
    let state_before = fs::read(state_dir.join("keep")).unwrap();

    // Simulate N+1: change binary content and also simulate unit definition change
    // For this test, we change binary only; but we also verify that upgrading
    // overwrites the service file if source changed. To simulate source change,
    // we create a temporary sysusers fragment with different content and pass via
    // a modified manifest? Simpler: verify binary replacement is sufficient, and
    // that config/state remain.
    let bin_v2 = temp_binary("UPGRADE-V2-CONTENT-def-DIFFERENT-LONGER-1234567890");
    let out2 = run_install(&root, &bin_v2, &[]);
    assert!(
        out2.status.success(),
        "upgrade failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    // New binary visible
    let new_bin = fs::read(root.join("usr/bin/synveil-scheduled-maintenance-once")).unwrap();
    assert_eq!(new_bin, fs::read(&bin_v2).unwrap());
    assert_ne!(new_bin, fs::read(&bin_v1).unwrap());
    // Service file still matches source (unchanged in this test, but we check it still exists and is not truncated)
    let svc_after = fs::read(&svc_path).unwrap();
    assert_eq!(svc_after, svc_before);
    // Config preserved byte-identical
    assert_eq!(
        fs::read(root.join("etc/synveil/synveil-scheduled-maintenance.env")).unwrap(),
        config_before
    );
    // State preserved
    assert_eq!(fs::read(state_dir.join("keep")).unwrap(), state_before);

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

// ---------------------------------------------------------------------------
// 8. Failed upgrade
// ---------------------------------------------------------------------------
#[test]
fn failed_upgrade_preserves_config_state_and_user_data_reports_partial() {
    let root = temp_root("failed-upgrade");
    let bin_v1 = temp_binary("FAILED-UPGRADE-V1");
    let out1 = run_install(&root, &bin_v1, &[]);
    assert!(out1.status.success());
    // Create admin config, state, external
    let env_path = root.join("etc/synveil/synveil-scheduled-maintenance.env");
    fs::write(
        &env_path,
        "DATABASE_URL=postgresql://keep:keep@localhost/db\n",
    )
    .unwrap();
    let env_before = fs::read(&env_path).unwrap();
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("sentinel"), "sentinel-keep").unwrap();
    let state_before = fs::read(state_dir.join("sentinel")).unwrap();
    let external = std::env::temp_dir().join(format!(
        "synveil-failed-ext-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("data.txt"), "external-keep").unwrap();
    let ext_before = fs::read(external.join("data.txt")).unwrap();

    let bin_v2 = temp_binary("FAILED-UPGRADE-V2-NEW-CONTENT");
    // Inject failure after 2 PACKAGE artifacts (binary + service)
    let out2 = run_install_with_fail(&root, &bin_v2, 2);
    assert!(
        !out2.status.success(),
        "failed upgrade should exit non-zero with injected failure"
    );
    let stderr = String::from_utf8_lossy(&out2.stderr);
    assert!(
        stderr.contains("injected failure") || stderr.contains("PACKAGE_DONE"),
        "stderr should report injected failure, got: {stderr}"
    );
    // Admin config, state, user data must remain intact
    assert_eq!(
        fs::read(&env_path).unwrap(),
        env_before,
        "config deleted during failed upgrade"
    );
    assert_eq!(
        fs::read(state_dir.join("sentinel")).unwrap(),
        state_before,
        "state deleted during failed upgrade"
    );
    assert_eq!(
        fs::read(external.join("data.txt")).unwrap(),
        ext_before,
        "external deleted during failed upgrade"
    );

    // Package-owned version may be partially updated: binary was first, so it should be new; later artifacts may still be old.
    // In our injection, binary (1) and service (2) succeed, then failure before timer. So binary should be v2, timer should still be old (which is same as v1 since source unchanged).
    // At least verify that one package file is new (binary)
    let installed_bin = fs::read(root.join("usr/bin/synveil-scheduled-maintenance-once")).unwrap();
    assert_eq!(
        installed_bin,
        fs::read(&bin_v2).unwrap(),
        "first PACKAGE artifact should be upgraded even on partial failure"
    );
    // Document that full transactional rollback is deferred: we honestly report partial.
    assert!(
        stderr.contains("partially updated") || stderr.contains("DEFERRED"),
        "should document that rollback is deferred and partial update may occur, got: {stderr}"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

// ---------------------------------------------------------------------------
// 9. Ordinary uninstall
// ---------------------------------------------------------------------------
#[test]
fn ordinary_uninstall_removes_package_preserves_config_state_external() {
    let root = temp_root("ordinary-uninstall");
    let bin = temp_binary("uninstall-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    // Create config/state/external
    fs::write(
        root.join("etc/synveil/synveil-scheduled-maintenance.env"),
        "DATABASE_URL=postgresql://keep@localhost/db\n",
    )
    .unwrap();
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("sentinel"), "keep-state").unwrap();
    let external = std::env::temp_dir().join(format!(
        "synveil-uninstall-ext-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("data.txt"), "external-keep").unwrap();

    let out2 = run_uninstall(&root, false);
    assert!(
        out2.status.success(),
        "ordinary uninstall failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );

    // Required removed: PACKAGE artifacts
    for dest in [
        "/usr/bin/synveil-client",
        "/usr/bin/synveil-desktop",
        "/usr/bin/synveil-scheduled-maintenance-once",
        "/usr/lib/systemd/user/synveil-client.service",
        "/usr/share/applications/synveil.desktop",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "/usr/share/doc/synveil/LICENSE",
        "/usr/share/doc/synveil/NOTICE",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "/usr/lib/sysusers.d/synveil.conf",
        "/usr/lib/tmpfiles.d/synveil.conf",
        "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
    ] {
        let full = root.join(dest.trim_start_matches('/'));
        assert!(
            !full.exists(),
            "PACKAGE artifact should be removed on ordinary uninstall: {dest}"
        );
    }
    // Required preserved
    assert!(
        root.join("etc/synveil").exists(),
        "/etc/synveil must be preserved on ordinary uninstall"
    );
    assert!(
        root.join("etc/synveil/synveil-scheduled-maintenance.env")
            .exists(),
        "admin env must be preserved"
    );
    assert!(
        root.join("var/lib/synveil").exists(),
        "/var/lib/synveil must be preserved"
    );
    assert!(
        root.join("var/lib/synveil/sentinel").exists(),
        "state sentinel must be preserved"
    );
    assert!(
        external.join("data.txt").exists(),
        "external must be preserved"
    );
    // Parent dirs preserved
    for parent in [
        "/usr/bin",
        "/usr/lib/systemd/user",
        "/usr/lib/systemd/system",
        "/usr/lib/sysusers.d",
        "/usr/lib/tmpfiles.d",
        "/usr/share/applications",
        "/usr/share/icons/hicolor/scalable/apps",
        "/usr/share/doc/synveil",
        "/etc",
        "/var/lib",
    ] {
        let full = root.join(parent.trim_start_matches('/'));
        assert!(
            full.exists(),
            "parent directory must not be removed: {parent}"
        );
    }

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 10. Reinstall after uninstall
// ---------------------------------------------------------------------------
#[test]
fn reinstall_after_ordinary_uninstall_restores_package_and_preserves() {
    let root = temp_root("reinstall-after-uninstall");
    let bin_v1 = temp_binary("reinstall-v1");
    let out = run_install(&root, &bin_v1, &[]);
    assert!(out.status.success());
    let env_path = root.join("etc/synveil/synveil-scheduled-maintenance.env");
    fs::write(&env_path, "DATABASE_URL=postgresql://keep2@localhost/db\n").unwrap();
    let env_before = fs::read(&env_path).unwrap();
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("sentinel"), "sentinel-keep2").unwrap();
    let state_before = fs::read(state_dir.join("sentinel")).unwrap();

    // Ordinary uninstall
    let out2 = run_uninstall(&root, false);
    assert!(out2.status.success());
    // Package gone, config/state remain
    assert!(
        !root
            .join("usr/bin/synveil-scheduled-maintenance-once")
            .exists()
    );
    assert!(env_path.exists());
    assert!(state_dir.join("sentinel").exists());

    // Reinstall
    let bin_v2 = temp_binary("reinstall-v2-new-binary-content");
    let out3 = run_install(&root, &bin_v2, &[]);
    assert!(
        out3.status.success(),
        "reinstall failed: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
    // Package restored
    assert!(
        root.join("usr/bin/synveil-scheduled-maintenance-once")
            .exists()
    );
    assert_eq!(
        fs::read(root.join("usr/bin/synveil-scheduled-maintenance-once")).unwrap(),
        fs::read(&bin_v2).unwrap()
    );
    // Config/state still preserved (reused)
    assert_eq!(
        fs::read(&env_path).unwrap(),
        env_before,
        "config not preserved across reinstall"
    );
    assert_eq!(fs::read(state_dir.join("sentinel")).unwrap(), state_before);

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

// ---------------------------------------------------------------------------
// 11. Purge separation
// ---------------------------------------------------------------------------
#[test]
fn purge_separation_ordinary_uninstall_is_not_purge() {
    let root = temp_root("purge-separation");
    let bin = temp_binary("purge-sep-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    fs::write(
        root.join("etc/synveil/synveil-scheduled-maintenance.env"),
        "DATABASE_URL=postgresql://a@b/c\n",
    )
    .unwrap();
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("sentinel"), "keep").unwrap();

    // Ordinary uninstall must NOT purge
    let out2 = run_uninstall(&root, false);
    assert!(out2.status.success());
    assert!(
        root.join("etc/synveil").exists(),
        "ordinary uninstall must not purge /etc/synveil"
    );
    assert!(
        root.join("var/lib/synveil").exists(),
        "ordinary uninstall must not purge /var/lib/synveil"
    );

    // Now explicit purge must remove them
    // Reinstall to have state again (package files already gone, but we need to test purge after uninstall)
    let out3 = run_install(&root, &bin, &[]);
    assert!(out3.status.success());
    // Ensure still there
    assert!(root.join("etc/synveil").exists());
    let out4 = run_uninstall(&root, true);
    assert!(
        out4.status.success(),
        "purge failed: {}",
        String::from_utf8_lossy(&out4.stderr)
    );
    assert!(
        !root.join("etc/synveil").exists(),
        "purge must remove /etc/synveil"
    );
    assert!(
        !root.join("var/lib/synveil").exists(),
        "purge must remove /var/lib/synveil"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 12. Purge user-data exclusion
// ---------------------------------------------------------------------------
#[test]
fn purge_excludes_external_user_data() {
    let root = temp_root("purge-exclude");
    let bin = temp_binary("purge-exclude-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    let external = std::env::temp_dir().join(format!(
        "synveil-purge-external-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(external.join("pool")).unwrap();
    fs::write(external.join("pool/data.txt"), "user-data-keep-purge-test").unwrap();
    let before = fs::read(external.join("pool/data.txt")).unwrap();

    // Even explicit purge must not delete external
    let out2 = run_uninstall(&root, true);
    assert!(out2.status.success());
    assert_eq!(
        fs::read(external.join("pool/data.txt")).unwrap(),
        before,
        "purge deleted external user data (forbidden)"
    );
    assert!(external.exists(), "external pool must survive purge");

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 13. Symlink safety
// ---------------------------------------------------------------------------
#[test]
fn symlink_safety_package_unlink_does_not_follow_target() {
    let root = temp_root("symlink-package");
    let bin = temp_binary("symlink-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());

    let external = std::env::temp_dir().join(format!(
        "synveil-symlink-ext-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&external).unwrap();
    let secret = external.join("secret.txt");
    fs::write(&secret, "do-not-delete-symlink-target").unwrap();

    // Replace PACKAGE file with symlink to external
    let pkg_path = root.join("usr/bin/synveil-scheduled-maintenance-once");
    fs::remove_file(&pkg_path).unwrap();
    std::os::unix::fs::symlink(&secret, &pkg_path).unwrap();
    assert!(pkg_path.is_symlink());

    let out2 = run_uninstall(&root, false);
    assert!(
        out2.status.success(),
        "uninstall with symlink should succeed without following: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        !pkg_path.exists() && !pkg_path.is_symlink(),
        "symlink itself should be removed"
    );
    assert!(
        secret.exists(),
        "symlink target outside root must not be deleted"
    );
    assert_eq!(
        fs::read_to_string(&secret).unwrap(),
        "do-not-delete-symlink-target"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin);
}

#[test]
fn symlink_safety_purge_does_not_follow_config_symlink() {
    let root = temp_root("symlink-purge");
    let bin = temp_binary("symlink-purge-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());

    let external = std::env::temp_dir().join(format!(
        "synveil-symlink-purge-ext-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(external.join("target")).unwrap();
    fs::write(external.join("target/keep.txt"), "keep-external").unwrap();

    // Make /etc/synveil a symlink to external target
    let etc_synveil = root.join("etc/synveil");
    fs::remove_dir_all(&etc_synveil).unwrap();
    std::os::unix::fs::symlink(external.join("target"), &etc_synveil).unwrap();
    assert!(etc_synveil.is_symlink());

    let out2 = run_uninstall(&root, true);
    assert!(
        out2.status.success(),
        "purge with symlink config should succeed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        !etc_synveil.exists() && !etc_synveil.is_symlink(),
        "symlink should be unlinked"
    );
    assert!(
        external.join("target/keep.txt").exists(),
        "purge must not follow symlink to delete target"
    );
    assert_eq!(
        fs::read_to_string(external.join("target/keep.txt")).unwrap(),
        "keep-external"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 14. Parent-directory safety
// ---------------------------------------------------------------------------
#[test]
fn parent_directory_safety_uninstall_does_not_remove_shared_parents() {
    let root = temp_root("parent-safety");
    let bin = temp_binary("parent-v1");
    // Pre-create parent directories to simulate they existed before install
    for parent in [
        "usr/bin",
        "usr/lib/systemd/system",
        "usr/lib/sysusers.d",
        "usr/lib/tmpfiles.d",
        "etc",
        "var/lib",
    ] {
        fs::create_dir_all(root.join(parent)).unwrap();
    }
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    let out2 = run_uninstall(&root, false);
    assert!(out2.status.success());
    for parent in [
        "usr/bin",
        "usr/lib/systemd/system",
        "usr/lib/sysusers.d",
        "usr/lib/tmpfiles.d",
        "etc",
        "var/lib",
        "usr/share/synveil",
    ] {
        let full = root.join(parent);
        // Parent may or may not have been created depending on install, but it must NOT have been removed if it existed
        // For those we pre-created, ensure still exists
        if parent == "usr/bin" || parent == "etc" || parent == "var/lib" {
            assert!(full.exists(), "shared parent must not be removed: {parent}");
        }
        // Even if not pre-created, we ensure we didn't rm -rf parent itself incorrectly
        // The test is that parent directories still exist if they had content or were pre-existing
    }
    // Specifically ensure /usr etc still exist
    assert!(root.join("usr").exists(), "/usr should survive uninstall");
    assert!(root.join("etc").exists(), "/etc should survive uninstall");
    assert!(root.join("var").exists(), "/var should survive uninstall");

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 15. Wrong-root/path safety
// ---------------------------------------------------------------------------
#[test]
fn wrong_root_path_safety_is_rejected() {
    let bin = temp_binary("wrong-root-v1");
    // Empty root
    let out = Command::new("bash")
        .arg(install_script())
        .arg("--root=")
        .arg(format!("--binary={}", bin.display()))
        .output()
        .unwrap();
    assert!(!out.status.success(), "empty root should be rejected");

    // Relative root
    let out = Command::new("bash")
        .arg(install_script())
        .arg("--root=relative/path")
        .arg(format!("--binary={}", bin.display()))
        .output()
        .unwrap();
    assert!(!out.status.success(), "relative root should be rejected");

    // .. escape
    let out = Command::new("bash")
        .arg(install_script())
        .arg("--root=/tmp/root/../escape")
        .arg(format!("--binary={}", bin.display()))
        .output()
        .unwrap();
    assert!(!out.status.success(), "root with .. should be rejected");

    // Host root without allow
    let out = Command::new("bash")
        .arg(install_script())
        .arg("--root=/")
        .arg(format!("--binary={}", bin.display()))
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "host root / without allow should be rejected"
    );

    // Uninstall with bad root
    let out = Command::new("bash")
        .arg(uninstall_script())
        .arg("--root=../relative")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "uninstall relative root should be rejected"
    );

    // Destination escape is prevented via manifest destinations themselves being safe,
    // but we test that install rejects if DESTDIR contains .. that would cause escape
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 16. Permission contract
// ---------------------------------------------------------------------------
#[test]
fn permission_contract_matches_prompt75() {
    let manifest = parse_manifest();
    let expectations: HashMap<&str, (&str, u32, &str, &str)> = [
        ("/usr/bin/synveil-client", ("0755", 0o755, "root", "root")),
        ("/usr/bin/synveil-desktop", ("0755", 0o755, "root", "root")),
        (
            "/usr/bin/synveil-scheduled-maintenance-once",
            ("0755", 0o755, "root", "root"),
        ),
        (
            "/usr/lib/systemd/user/synveil-client.service",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/share/applications/synveil.desktop",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/share/doc/synveil/LICENSE",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/share/doc/synveil/NOTICE",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/lib/sysusers.d/synveil.conf",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/lib/tmpfiles.d/synveil.conf",
            ("0644", 0o644, "root", "root"),
        ),
        (
            "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
            ("0644", 0o644, "root", "root"),
        ),
        ("/etc/synveil", ("0750", 0o750, "root", "synveil")),
        ("/etc/synveil/credentials", ("0700", 0o700, "root", "root")),
        ("/var/lib/synveil", ("0750", 0o750, "synveil", "synveil")),
        ("/run/synveil", ("0750", 0o750, "synveil", "synveil")),
    ]
    .into_iter()
    .collect();

    for (source, dest, mode, owner, group, class) in &manifest {
        if let Some((exp_mode_str, exp_mode, exp_owner, exp_group)) =
            expectations.get(dest.as_str())
        {
            assert_eq!(
                mode, exp_mode_str,
                "mode string for {dest} must be {exp_mode_str}, got {mode} (class {class} source {source})"
            );
            assert_eq!(owner, exp_owner, "owner for {dest} must be {exp_owner}");
            assert_eq!(group, exp_group, "group for {dest} must be {exp_group}");
            // Numeric check
            let parsed = u32::from_str_radix(mode, 8).unwrap();
            assert_eq!(parsed, *exp_mode, "mode octal mismatch for {dest}");
        } else {
            panic!("unexpected manifest destination not in contract: {dest} (class {class})");
        }
    }

    // Also verify that installed files have correct modes (rootless: ownership via manifest, mode via fs)
    let root = temp_root("perm-contract");
    let bin = temp_binary("perm-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    for (_, dest, mode_str, _, _, class) in &manifest {
        if class == "PACKAGE" || class == "CONFIG_DIRECTORY" || class == "CREDENTIAL_DIRECTORY" {
            let full = root.join(dest.trim_start_matches('/'));
            if full.exists() {
                let expected = u32::from_str_radix(mode_str, 8).unwrap();
                assert_eq!(
                    file_mode(&full),
                    expected,
                    "installed mode for {dest} must be {mode_str}"
                );
            }
        }
    }
    // Check that binary is non-writable by synveil runtime: package-owned 0755 root:root, not synveil writable implied
    // Runtime is synveil:synveil; package files are 0644/0755 root:root -> synveil cannot write.
    // We assert that package files are NOT owned by synveil in manifest (they are root:root)
    for (_, dest, _, owner, group, class) in &manifest {
        if class == "PACKAGE" {
            assert_eq!(owner, "root", "PACKAGE {dest} must be root-owned");
            assert_eq!(group, "root", "PACKAGE {dest} must be root group");
        }
    }
    assert!(
        !root.join("var/log/synveil").exists(),
        "/var/log/synveil must not be created (journald preferred)"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 17. systemd artifacts
// ---------------------------------------------------------------------------
#[test]
fn systemd_artifacts_match_validated_definitions() {
    let root = temp_root("systemd-artifacts");
    let bin = temp_binary("systemd-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());

    let installed_svc = fs::read_to_string(
        root.join("usr/lib/systemd/system/synveil-scheduled-maintenance.service"),
    )
    .unwrap();
    let src_svc = fs::read_to_string(service_src()).unwrap();
    assert_eq!(
        installed_svc, src_svc,
        "installed service must be byte-identical to source"
    );

    let installed_tmr =
        fs::read_to_string(root.join("usr/lib/systemd/system/synveil-scheduled-maintenance.timer"))
            .unwrap();
    let src_tmr = fs::read_to_string(timer_src()).unwrap();
    assert_eq!(
        installed_tmr, src_tmr,
        "installed timer must be byte-identical to source"
    );

    // Prompt 75 identity regression
    assert!(
        installed_svc.contains("User=synveil"),
        "service must contain User=synveil"
    );
    assert!(
        installed_svc.contains("Group=synveil"),
        "service must contain Group=synveil"
    );
    assert!(
        !installed_svc.contains("User=root"),
        "service must not contain User=root"
    );
    // Check systemd-analyze verify if available
    if Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .is_ok()
    {
        let tmr_path = root.join("usr/lib/systemd/system/synveil-scheduled-maintenance.timer");
        let out = Command::new("systemd-analyze")
            .arg("verify")
            .arg(&tmr_path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "systemd-analyze verify timer failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Service: patch ExecStart to true for verify
        let patched = installed_svc.replace(
            "ExecStart=/usr/bin/synveil-scheduled-maintenance-once",
            "ExecStart=/usr/bin/true",
        );
        let tmp = std::env::temp_dir().join(format!(
            "synveil-verify-{}.service",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&tmp, patched).unwrap();
        let out2 = Command::new("systemd-analyze")
            .arg("verify")
            .arg(&tmp)
            .output()
            .unwrap();
        let _ = fs::remove_file(&tmp);
        assert!(
            out2.status.success(),
            "systemd-analyze verify service failed: {}",
            String::from_utf8_lossy(&out2.stderr)
        );
    }

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// 18. sysusers/tmpfiles artifacts
// ---------------------------------------------------------------------------
#[test]
fn sysusers_tmpfiles_artifacts_match_authoritative_sources() {
    let root = temp_root("sysusers-tmpfiles");
    let bin = temp_binary("sysusers-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());

    let installed_sysusers =
        fs::read_to_string(root.join("usr/lib/sysusers.d/synveil.conf")).unwrap();
    let src_sysusers = fs::read_to_string(sysusers_src()).unwrap();
    assert_eq!(
        installed_sysusers, src_sysusers,
        "sysusers must be byte-identical"
    );

    let installed_tmpfiles =
        fs::read_to_string(root.join("usr/lib/tmpfiles.d/synveil.conf")).unwrap();
    let src_tmpfiles = fs::read_to_string(tmpfiles_src()).unwrap();
    assert_eq!(
        installed_tmpfiles, src_tmpfiles,
        "tmpfiles must be byte-identical"
    );

    // Validate sysusers dry-run if available
    if Command::new("systemd-sysusers")
        .arg("--help")
        .output()
        .is_ok()
    {
        let tmp_root = std::env::temp_dir().join(format!(
            "synveil-sysusers-{}",
            uuid::Uuid::now_v7().simple()
        ));
        fs::create_dir_all(&tmp_root).unwrap();
        let src_path = root.join("usr/lib/sysusers.d/synveil.conf");
        let out = Command::new("systemd-sysusers")
            .arg("--dry-run")
            .arg(format!("--root={}", tmp_root.display()))
            .arg(src_path.to_string_lossy().to_string())
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&tmp_root);
        assert!(
            out.status.success(),
            "systemd-sysusers dry-run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    if Command::new("systemd-tmpfiles")
        .arg("--help")
        .output()
        .is_ok()
    {
        let tmp_root = std::env::temp_dir().join(format!(
            "synveil-tmpfiles-{}",
            uuid::Uuid::now_v7().simple()
        ));
        fs::create_dir_all(&tmp_root).unwrap();
        let src_path = root.join("usr/lib/tmpfiles.d/synveil.conf");
        let out = Command::new("systemd-tmpfiles")
            .arg("--dry-run")
            .arg("--create")
            .arg("--graceful")
            .arg(format!("--root={}", tmp_root.display()))
            .arg(src_path.to_string_lossy().to_string())
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&tmp_root);
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // Unknown user in empty root is expected without sysusers applied; treat as pass if that's the only error
            if !(stderr.contains("Unknown user") || stderr.contains("Failed to resolve user")) {
                panic!("systemd-tmpfiles dry-run failed: {stderr}");
            }
        }
    }

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

// ---------------------------------------------------------------------------
// Additional: service account uninstall policy
// ---------------------------------------------------------------------------
#[test]
fn service_account_not_removed_on_ordinary_uninstall() {
    // Policy: ordinary uninstall retains synveil account; purge may consider removal but does not auto-delete.
    // Since tests are rootless and cannot create users, we verify documentation and script behavior.
    let uninstall_content = fs::read_to_string(uninstall_script()).unwrap();
    // Must not contain userdel/groupdel for ordinary uninstall
    assert!(
        !uninstall_content.contains("userdel synveil")
            || uninstall_content.contains("# only after purge"),
        "uninstall script must not automatically delete synveil account on ordinary uninstall"
    );
    assert!(
        uninstall_content.contains("RETAINED") || uninstall_content.contains("retained"),
        "uninstall should document account retained"
    );
    // Install script docs should mention sysusers
    let install_content = fs::read_to_string(install_script()).unwrap();
    assert!(
        install_content.contains("systemd-sysusers") || install_content.contains("sysusers"),
        "install should document sysusers step (deferred to host)"
    );
}

// ---------------------------------------------------------------------------
// Additional: database not touched
// ---------------------------------------------------------------------------
#[test]
fn database_not_touched_by_package_lifecycle() {
    // Verify install/uninstall scripts never perform destructive database operations.
    // Documentation mentions of PostgreSQL for preservation are allowed.
    let install_content = fs::read_to_string(install_script()).unwrap().to_lowercase();
    let uninstall_content = fs::read_to_string(uninstall_script())
        .unwrap()
        .to_lowercase();
    for bad in [
        "drop database",
        "dropdb",
        "rm -rf /var/lib/postgresql",
        "psql -c \"drop",
    ] {
        assert!(
            !install_content.contains(&bad.to_lowercase()),
            "install must not contain destructive DB command {bad}"
        );
        assert!(
            !uninstall_content.contains(&bad.to_lowercase()),
            "uninstall must not contain destructive DB command {bad}"
        );
    }
    // Ensure scripts document that database is preserved (at least via comments)
    // and never claim to delete or recreate it.
    assert!(
        !install_content.contains("rm -rf")
            || install_content.contains("purge")
            || install_content.contains("tmp"),
        "install rm -rf must be scoped to staging, not DB"
    );
    // Manifest must not contain database paths as package-owned
    let manifest = read_manifest();
    // Allow mentions in comments, but not as destinations
    for line in manifest.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let lower = t.to_lowercase();
        assert!(
            !lower.contains("/var/lib/postgresql") && !lower.contains("/var/lib/pgsql"),
            "manifest must not own postgresql data path: {t}"
        );
        // Ensure no manifest destination is a database path
        let mut parts = t.split_whitespace();
        let _source = parts.next().unwrap_or("");
        let dest = parts.next().unwrap_or("");
        assert!(
            !dest.contains("postgresql") && !dest.contains("postgres"),
            "manifest destination must not be database-owned: {dest}"
        );
    }
}

// ---------------------------------------------------------------------------
// Additional: shell safety
// ---------------------------------------------------------------------------
#[test]
fn shell_safety_set_euo_pipefail_and_no_eval() {
    for script in [
        install_script(),
        uninstall_script(),
        repo_root().join("deploy/install/common.sh"),
    ] {
        let content = fs::read_to_string(&script).unwrap();
        assert!(
            content.contains("set -euo pipefail"),
            "{} must contain set -euo pipefail",
            script.display()
        );
        assert!(
            !content.contains("eval "),
            "{} must not use eval",
            script.display()
        );
        // Ensure no unquoted $ROOT/$USER_INPUT in rm -rf
        assert!(
            !content.contains("rm -rf $ROOT") && !content.contains("rm -rf $USER_INPUT"),
            "{} must not have unquoted rm -rf",
            script.display()
        );
        // Check quoting of paths (basic heuristic: rm -f "$full" not rm -f $full)
        // We do this via grep -n in bash, but here we just ensure presence of quoted "$"
        assert!(
            content.contains("\"$"),
            "{} should quote paths",
            script.display()
        );
    }
    // Validate syntax via bash -n
    for script in [install_script(), uninstall_script()] {
        let out = Command::new("bash")
            .arg("-n")
            .arg(&script)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "bash -n failed for {}: {}",
            script.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

// ---------------------------------------------------------------------------
// Additional: manifest not duplicated
// ---------------------------------------------------------------------------
#[test]
fn manifest_authoritative_no_duplicate_logic() {
    // The manifest file must be the single source; install and uninstall must source common.sh
    let install = fs::read_to_string(install_script()).unwrap();
    let uninstall = fs::read_to_string(uninstall_script()).unwrap();
    assert!(
        install.contains("common.sh"),
        "install must source common.sh"
    );
    assert!(
        uninstall.contains("common.sh"),
        "uninstall must source common.sh"
    );
    // Ensure MANIFEST file exists and is singular (no second manifest)
    assert!(
        manifest_path().exists(),
        "MANIFEST must exist at deploy/install/MANIFEST"
    );
    let entries: Vec<_> = fs::read_dir(repo_root().join("deploy/install"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .to_uppercase()
                .contains("MANIFEST")
        })
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "there must be exactly one MANIFEST file in deploy/install, found {}",
        entries.len()
    );
}

// ---------------------------------------------------------------------------
// Prompt 77 — credential lifecycle extensions (fresh/reinstall/upgrade/uninstall/purge/symlink/rotation)
// ---------------------------------------------------------------------------

#[test]
fn credential_fresh_install_declares_directory_without_secret() {
    let root = temp_root("cred-fresh");
    let bin = temp_binary("cred-fresh-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(
        out.status.success(),
        "fresh install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cred_dir = root.join("etc/synveil/credentials");
    assert!(
        cred_dir.is_dir(),
        "/etc/synveil/credentials must exist after fresh install"
    );
    assert_mode(&cred_dir, 0o700);
    assert!(
        !cred_dir.join("database-url").exists(),
        "fresh install must not invent database secret"
    );
    // Also ensure parent /etc/synveil exists 0750
    assert_mode(&root.join("etc/synveil"), 0o750);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

#[test]
fn credential_reinstall_preserves_byte_identical() {
    let root = temp_root("cred-reinstall");
    let bin = temp_binary("cred-reinstall-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    let cred_dir = root.join("etc/synveil/credentials");
    fs::create_dir_all(&cred_dir).unwrap();
    let cred_path = cred_dir.join("database-url");
    let secret = "postgresql://user:secret1@127.0.0.1:5432/synveil";
    fs::write(&cred_path, secret).unwrap();
    // chmod 0600 as contract
    let mut perm = fs::metadata(&cred_path).unwrap().permissions();
    perm.set_mode(0o600);
    fs::set_permissions(&cred_path, perm).unwrap();
    let before = fs::read(&cred_path).unwrap();
    // Reinstall
    let out2 = run_install(&root, &bin, &[]);
    assert!(out2.status.success());
    assert_eq!(
        fs::read(&cred_path).unwrap(),
        before,
        "credential byte-identical after reinstall"
    );
    assert_mode(&cred_path, 0o600);
    assert_mode(&cred_dir, 0o700);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

#[test]
fn credential_upgrade_preserves_byte_identical() {
    let root = temp_root("cred-upgrade");
    let bin_v1 = temp_binary("cred-upgrade-v1");
    let out = run_install(&root, &bin_v1, &[]);
    assert!(out.status.success());
    let cred_path = root.join("etc/synveil/credentials/database-url");
    fs::create_dir_all(cred_path.parent().unwrap()).unwrap();
    let secret = "postgresql://user:secret-upgrade@127.0.0.1/db";
    fs::write(&cred_path, secret).unwrap();
    let mut perm = fs::metadata(&cred_path).unwrap().permissions();
    perm.set_mode(0o600);
    fs::set_permissions(&cred_path, perm).unwrap();
    let before = fs::read(&cred_path).unwrap();
    // Upgrade to v2 (different PACKAGE content)
    let bin_v2 = temp_binary("cred-upgrade-v2-different-content-longer-xyz");
    let out2 = run_install(&root, &bin_v2, &[]);
    assert!(
        out2.status.success(),
        "upgrade failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(
        fs::read(&cred_path).unwrap(),
        before,
        "credential must survive upgrade"
    );
    assert_eq!(fs::read(&cred_path).unwrap(), secret.as_bytes());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin_v1);
    let _ = fs::remove_file(&bin_v2);
}

#[test]
fn credential_ordinary_uninstall_preserves() {
    let root = temp_root("cred-uninstall");
    let bin = temp_binary("cred-uninstall-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    let cred_path = root.join("etc/synveil/credentials/database-url");
    fs::create_dir_all(cred_path.parent().unwrap()).unwrap();
    fs::write(&cred_path, "postgresql://keep:keep@127.0.0.1/db").unwrap();
    let before = fs::read(&cred_path).unwrap();
    let out2 = run_uninstall(&root, false);
    assert!(out2.status.success());
    assert!(
        cred_path.exists(),
        "ordinary uninstall must preserve credential file"
    );
    assert_eq!(fs::read(&cred_path).unwrap(), before);
    assert!(root.join("etc/synveil/credentials").exists());
    assert!(root.join("etc/synveil").exists());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

#[test]
fn credential_purge_removes_with_explicit_flag() {
    let root = temp_root("cred-purge");
    let bin = temp_binary("cred-purge-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    let cred_path = root.join("etc/synveil/credentials/database-url");
    fs::create_dir_all(cred_path.parent().unwrap()).unwrap();
    fs::write(&cred_path, "postgresql://purge:secret@127.0.0.1/db").unwrap();
    // Ordinary uninstall must NOT purge
    let out2 = run_uninstall(&root, false);
    assert!(out2.status.success());
    assert!(
        cred_path.exists(),
        "ordinary uninstall must not purge credential"
    );
    // Explicit purge must remove
    let out3 = run_uninstall(&root, true);
    assert!(
        out3.status.success(),
        "purge failed: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
    assert!(
        !root.join("etc/synveil").exists(),
        "purge must remove /etc/synveil including credentials"
    );
    assert!(!cred_path.exists());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&bin);
}

#[test]
fn credential_symlink_safety_does_not_follow_external() {
    let root = temp_root("cred-symlink");
    let bin = temp_binary("cred-symlink-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(out.status.success());
    let external = std::env::temp_dir().join(format!(
        "synveil-cred-ext-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(external.join("target")).unwrap();
    fs::write(external.join("target/keep.txt"), "external-keep").unwrap();
    // Replace credentials dir with symlink to external target
    let cred_dir = root.join("etc/synveil/credentials");
    fs::remove_dir_all(&cred_dir).unwrap();
    std::os::unix::fs::symlink(external.join("target"), &cred_dir).unwrap();
    assert!(cred_dir.is_symlink());
    // Ordinary uninstall should preserve symlink (not purge), but purge should unlink without following
    let out2 = run_uninstall(&root, true);
    assert!(
        out2.status.success(),
        "purge with symlink cred dir should succeed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        !cred_dir.exists() && !cred_dir.is_symlink(),
        "symlink should be unlinked"
    );
    assert!(
        external.join("target/keep.txt").exists(),
        "purge must not follow symlink to delete external"
    );
    // Also test file symlink
    let root2 = temp_root("cred-symlink-file");
    let out3 = run_install(&root2, &bin, &[]);
    assert!(out3.status.success());
    let cred_path = root2.join("etc/synveil/credentials/database-url");
    fs::create_dir_all(cred_path.parent().unwrap()).unwrap();
    fs::write(external.join("secret.txt"), "secret-outside").unwrap();
    fs::remove_file(&cred_path).unwrap_or(());
    // If file doesn't exist yet, create symlink file
    if cred_path.exists() {
        fs::remove_file(&cred_path).unwrap();
    }
    std::os::unix::fs::symlink(external.join("secret.txt"), &cred_path).unwrap();
    // Purge should unlink file symlink
    let out4 = run_uninstall(&root2, true);
    assert!(out4.status.success());
    assert!(!cred_path.exists());
    assert!(external.join("secret.txt").exists());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&root2);
    let _ = fs::remove_dir_all(&external);
    let _ = fs::remove_file(&bin);
}

#[test]
fn credential_rotation_atomic_new_invocation_reads_new_secret() {
    // Rotation is an atomic replace of the secret file; next one-shot reads new value.
    // We simulate by writing a new file to temp then rename, and prove a fresh
    // read sees the new value (no daemon restart needed).
    let dir = std::env::temp_dir().join(format!(
        "synveil-rotation-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&dir).unwrap();
    let cred_path = dir.join("database-url");
    // Initial secret
    fs::write(&cred_path, "postgresql://user:old-secret@127.0.0.1/db").unwrap();
    // Simulate atomic rotation: write to adjacent temp file, chmod, rename
    let tmp = dir.join("database-url.tmp");
    fs::write(&tmp, "postgresql://user:new-secret@127.0.0.1/db").unwrap();
    let mut perm = fs::metadata(&tmp).unwrap().permissions();
    perm.set_mode(0o600);
    fs::set_permissions(&tmp, perm).unwrap();
    fs::rename(&tmp, &cred_path).unwrap();
    // Fresh read should see new secret (direct file read, no env)
    let content = fs::read_to_string(&cred_path).unwrap();
    assert_eq!(content, "postgresql://user:new-secret@127.0.0.1/db");
    // Validate that the new content is a valid DATABASE_URL via canonical parser
    // (without logging secret)
    let cfg = DatabaseConfig::from_url(content.clone()).unwrap();
    assert_eq!(cfg.pool_config().max_connections(), 10);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn systemd_service_contains_credential_delivery_and_no_secret_env() {
    let svc = fs::read_to_string(service_src()).unwrap();
    // Must contain LoadCredential
    assert!(
        svc.contains("LoadCredential="),
        "service must contain LoadCredential directive"
    );
    assert!(
        svc.contains("LoadCredential=database-url:"),
        "LoadCredential must load database-url"
    );
    // Must contain non-secret env pointer
    assert!(
        svc.contains("SYNVEIL_DATABASE_CREDENTIAL_FILE="),
        "service must expose credential path via non-secret env"
    );
    // Must use %d specifier (systemd credentials directory)
    assert!(
        svc.contains("%d/database-url"),
        "Environment must use %d/database-url specifier"
    );
    // Must NOT require DATABASE_URL via Environment= (hardcoded)
    let non_comment: String = svc
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !non_comment.contains("Environment=DATABASE_URL="),
        "service must not hardcode DATABASE_URL via Environment="
    );
    assert!(
        !non_comment.contains("DATABASE_URL=") || non_comment.contains("EnvironmentFile=-"),
        "service must not contain DATABASE_URL assignment outside EnvironmentFile"
    );
    // Exactly one LoadCredential for database-url
    let load_count = non_comment.matches("LoadCredential=database-url").count();
    assert_eq!(
        load_count, 1,
        "exactly one LoadCredential=database-url expected, got {load_count}"
    );
}

#[test]
fn production_env_example_contains_no_database_url_secret() {
    let tmpl = fs::read_to_string(template_src()).unwrap();
    // Must not contain a functional DATABASE_URL=postgresql:// line
    let has_db_url = tmpl
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .any(|l| l.contains("DATABASE_URL=") && l.contains("postgresql://"));
    assert!(
        !has_db_url,
        "production template must not contain a functional DATABASE_URL with real scheme"
    );
    // Must not contain password-like placeholder that could be mistaken for real secret?
    // At minimum, it should mention LoadCredential and not contain postgres:// with credentials
    assert!(
        tmpl.contains("LoadCredential") || tmpl.contains("credentials"),
        "template should document credential delivery, not DATABASE_URL"
    );
    // Ensure no real secret
    assert!(
        !tmpl.contains("postgres://user:password") && !tmpl.contains("postgresql://user:password"),
        "template must not contain example credentials that look real"
    );
}

#[test]
fn credential_directory_ownership_contract() {
    let manifest = parse_manifest();
    let cred_entry = manifest
        .iter()
        .find(|(_, d, _, _, _, c)| d == "/etc/synveil/credentials" && c == "CREDENTIAL_DIRECTORY")
        .expect("manifest must contain CREDENTIAL_DIRECTORY");
    let (_, _, mode, owner, group, _) = cred_entry;
    assert_eq!(mode, "0700", "credential directory must be 0700");
    assert_eq!(owner, "root", "credential directory owner must be root");
    assert_eq!(group, "root", "credential directory group must be root");
}
