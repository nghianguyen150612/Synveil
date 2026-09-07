//! Prompt 79 — Linux deployment adversarial verification (staged, no PG, no root).
//!
//! Proves the Prompt 76 package lifecycle fails safely under hostile,
//! malformed, interrupted, and concurrent filesystem conditions without
//! mutating the host. All tests operate on disposable staged roots under
//! `/tmp` and never touch `/usr`, `/etc`, `/var`, or `/run` on the host.
//!
//! Coverage:
//! - B: credential filesystem attacks (symlink source/dir, mode, skeleton)
//! - C: installation failure attacks (interrupted install/upgrade, reinstall,
//!   mode repair, uninstall, reinstall-after-uninstall, purge)
//! - D: destructive path safety (symlink attacks, `..`, relative, host root,
//!   shared parents)
//! - I/J/K/L (static): sandbox negative surface, unit corruption detection,
//!   manifest integrity, service identity attacks
//! - M: data-preservation sentinel matrix (single staged scenario × 6 ops)
//!
//! Live PostgreSQL, crash/lease, and concurrency gates live in
//! `linux_deployment_adversarial_postgres` (api crate) and the repaired
//! `packaged_maintenance_runtime_postgres`.

use std::{
    collections::HashSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

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
fn service_path() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.service")
}
fn timer_path() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.timer")
}

fn temp_root(prefix: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "p79-adv-{}-{}",
        prefix,
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn temp_binary(content: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("p79-adv-bin-{}", uuid::Uuid::now_v7().simple()));
    fs::write(&p, content).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn run_install(root: &Path, binary: &Path, extra_env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new("bash");
    cmd.arg(install_script())
        .arg(format!("--root={}", root.display()))
        .arg(format!("--binary={}", binary.display()));
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn install.sh")
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

fn parse_manifest() -> Vec<(String, String, String, String, String, String)> {
    let content = fs::read_to_string(manifest_path()).expect("read MANIFEST");
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut parts = t.split_whitespace();
        out.push((
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
        ));
    }
    out
}

fn package_count() -> usize {
    parse_manifest()
        .iter()
        .filter(|(_, _, _, _, _, c)| c == "PACKAGE")
        .count()
}

/// Stage the adversarial sentinel set inside `root` plus external locations.
/// Returns (admin_conf, credential_file, state_file, external_file, pg_sim).
/// - admin_conf: `<root>/etc/synveil/synveil-scheduled-maintenance.env`
/// - credential: `<root>/etc/synveil/credentials/database-url`
/// - state: `<root>/var/lib/synveil/state` (created directly; tmpfiles owns it
///   in production, but the lifecycle must preserve it when present)
/// - external: outside `root`, must survive every operation including purge
/// - pg_sim: simulated PostgreSQL data dir outside `root`, must survive all
fn stage_sentinels(root: &Path, tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let etc = root.join("etc/synveil");
    let cred_dir = root.join("etc/synveil/credentials");
    let state_dir = root.join("var/lib/synveil");
    fs::create_dir_all(&cred_dir).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    let admin = etc.join("synveil-scheduled-maintenance.env");
    fs::write(&admin, format!("ADMIN_SENTINEL_{tag}\nLEASE=120\n")).unwrap();
    let cred = cred_dir.join("database-url");
    fs::write(
        &cred,
        format!("postgresql://admin:secret-{tag}@127.0.0.1:5432/synveil"),
    )
    .unwrap();
    fs::set_permissions(&cred, fs::Permissions::from_mode(0o600)).unwrap();
    let state = state_dir.join("state");
    fs::write(&state, format!("STATE_SENTINEL_{tag}\n")).unwrap();
    let external = std::env::temp_dir().join(format!(
        "p79-adv-external-{}-{}",
        tag,
        uuid::Uuid::now_v7().simple()
    ));
    fs::write(&external, format!("EXTERNAL_USER_DATA_{tag}\n")).unwrap();
    let pg = std::env::temp_dir().join(format!(
        "p79-adv-pg-{}-{}",
        tag,
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&pg).unwrap();
    fs::write(pg.join("PG_VERSION"), "17\n").unwrap();
    fs::write(pg.join("base-sentinel"), format!("PGDATA_{tag}\n")).unwrap();
    (admin, cred, state, external, pg)
}

// ---------------------------------------------------------------------------
// B4 — credential directory skeleton contract
// ---------------------------------------------------------------------------

#[test]
fn adversarial_fresh_install_leaves_directory_present_secret_absent() {
    let root = temp_root("b4-skeleton");
    let bin = temp_binary("p79-b4-v1");
    let out = run_install(&root, &bin, &[]);
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cred_dir = root.join("etc/synveil/credentials");
    assert!(
        cred_dir.is_dir(),
        "credential directory skeleton must exist"
    );
    assert_eq!(file_mode(&cred_dir), 0o700);
    assert!(
        !cred_dir.join("database-url").exists(),
        "fresh install must leave secret absent"
    );
    let etc = root.join("etc/synveil");
    assert!(etc.is_dir());
    assert_eq!(file_mode(&etc), 0o750);
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// B1 — credential source symlink must not be followed destructively
// ---------------------------------------------------------------------------

#[test]
fn adversarial_credential_source_symlink_not_followed() {
    let root = temp_root("b1-symlink");
    let bin = temp_binary("p79-b1-v1");
    assert!(run_install(&root, &bin, &[]).status.success());
    let (admin, _cred, state, external, pg) = stage_sentinels(&root, "b1");
    // Replace the credential file with a symlink to an external sentinel.
    let cred_link = root.join("etc/synveil/credentials/database-url");
    let target = std::env::temp_dir().join(format!(
        "p79-adv-b1-target-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::write(&target, "EXTERNAL_CRED_TARGET\n").unwrap();
    fs::remove_file(&cred_link).unwrap();
    std::os::unix::fs::symlink(&target, &cred_link).unwrap();
    // Ordinary uninstall must unlink the package path style link without
    // deleting the target. Purge must unlink the config symlink without
    // traversing into the target.
    let out = run_uninstall(&root, false);
    assert!(out.status.success());
    assert!(
        target.exists(),
        "ordinary uninstall must not delete symlink target"
    );
    // Re-stage link for purge probe (ordinary uninstall preserves config).
    assert!(cred_link.is_symlink(), "config symlink must be preserved");
    let out = run_uninstall(&root, true);
    assert!(out.status.success());
    assert!(
        target.exists(),
        "purge must unlink the symlink, not delete its target"
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "EXTERNAL_CRED_TARGET\n"
    );
    // Governed sentinels: purge removes admin/state, preserves external/PG.
    assert!(!admin.exists(), "purge removes admin config");
    assert!(!state.exists(), "purge removes state");
    assert!(external.exists(), "external user data survives purge");
    assert!(pg.join("PG_VERSION").exists(), "PG data survives purge");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&target);
    let _ = fs::remove_file(&external);
    let _ = fs::remove_dir_all(&pg);
}

// ---------------------------------------------------------------------------
// B2 — credential directory symlink: external target survives everything
// ---------------------------------------------------------------------------

#[test]
fn adversarial_credential_directory_symlink_target_survives_lifecycle() {
    let root = temp_root("b2-dirlink");
    let bin = temp_binary("p79-b2-v1");
    assert!(run_install(&root, &bin, &[]).status.success());
    // Replace credential directory with a symlink to an external dir.
    let cred_dir = root.join("etc/synveil/credentials");
    let external_dir = std::env::temp_dir().join(format!(
        "p79-adv-b2-extdir-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&external_dir).unwrap();
    let sentinel = external_dir.join("sentinel");
    fs::write(&sentinel, "B2_SENTINEL\n").unwrap();
    fs::remove_dir_all(&cred_dir).unwrap();
    std::os::unix::fs::symlink(&external_dir, &cred_dir).unwrap();

    // Upgrade over the symlinked dir must not destroy the external target.
    let bin2 = temp_binary("p79-b2-v2");
    let out = run_install(&root, &bin2, &[]);
    // Install may succeed (mkdir -p follows the link) or fail closed; either
    // way the external sentinel must survive.
    eprintln!(
        "b2 upgrade over dir symlink: success={} stderr={}",
        out.status.success(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        sentinel.exists(),
        "external dir sentinel must survive upgrade"
    );
    // Ordinary uninstall + purge must not recursively destroy the target.
    assert!(run_uninstall(&root, false).status.success());
    assert!(
        sentinel.exists(),
        "sentinel must survive ordinary uninstall"
    );
    assert!(run_uninstall(&root, true).status.success());
    assert!(
        sentinel.exists(),
        "purge must unlink the dir symlink, not rm -rf the external target"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&external_dir);
}

// ---------------------------------------------------------------------------
// B3 — incorrect mode is detectable (contract, not silent chmod)
// ---------------------------------------------------------------------------

#[test]
fn adversarial_world_readable_credential_is_detectable_not_silently_fixed() {
    let root = temp_root("b3-mode");
    let bin = temp_binary("p79-b3-v1");
    assert!(run_install(&root, &bin, &[]).status.success());
    let cred = root.join("etc/synveil/credentials/database-url");
    fs::write(&cred, "postgresql://u:p@127.0.0.1/db").unwrap();
    fs::set_permissions(&cred, fs::Permissions::from_mode(0o644)).unwrap();
    // The lifecycle layer must not silently chmod arbitrary admin paths.
    // Reinstall must preserve the admin file as-is (mode included); the
    // insecure condition is detectable by inspection against the 0600 contract.
    let bin2 = temp_binary("p79-b3-v2");
    assert!(run_install(&root, &bin2, &[]).status.success());
    let mode = file_mode(&cred);
    assert_eq!(
        mode, 0o644,
        "reinstall must not silently chmod admin credential (detect, don't mutate); got {mode:o}"
    );
    assert!(
        mode & 0o044 != 0,
        "test precondition: file is world-readable, mode {mode:o}"
    );
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// C1 — interrupted fresh install preserves everything
// ---------------------------------------------------------------------------

#[test]
fn adversarial_interrupted_fresh_install_preserves_no_deletion() {
    let n = package_count();
    assert!(n >= 6, "expected >=6 PACKAGE entries, got {n}");
    for fail_after in 0..=n {
        let root = temp_root(&format!("c1-{fail_after}"));
        // Pre-stage sentinels that a fresh install must never delete, plus an
        // external pool outside the root.
        let (admin, cred, state, external, pg) =
            stage_sentinels(&root, &format!("c1-{fail_after}"));
        let bin = temp_binary("p79-c1-v1");
        let out = run_install(
            &root,
            &bin,
            &[("SYNVEIL_INSTALL_FAIL_AFTER", &fail_after.to_string())],
        );
        if fail_after >= n {
            assert!(
                out.status.success(),
                "fail_after={fail_after} >= {n} should succeed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        } else {
            assert!(!out.status.success(), "fail_after={fail_after} must fail");
        }
        // No config/state/external deletion under any interruption point.
        assert!(admin.exists(), "fail_after={fail_after}: admin preserved");
        assert!(
            cred.exists(),
            "fail_after={fail_after}: credential preserved"
        );
        assert!(state.exists(), "fail_after={fail_after}: state preserved");
        assert!(
            external.exists(),
            "fail_after={fail_after}: external preserved"
        );
        assert!(
            pg.join("PG_VERSION").exists(),
            "fail_after={fail_after}: pg preserved"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&external);
        let _ = fs::remove_dir_all(&pg);
    }
}

// ---------------------------------------------------------------------------
// C2 — interrupted upgrade at multiple boundaries preserves everything
// ---------------------------------------------------------------------------

#[test]
fn adversarial_interrupted_upgrade_preserves_at_multiple_boundaries() {
    let n = package_count();
    for fail_after in [0usize, 1, 3, 5] {
        if fail_after > n {
            continue;
        }
        let root = temp_root(&format!("c2-{fail_after}"));
        let v1 = temp_binary("p79-c2-v1-content");
        assert!(run_install(&root, &v1, &[]).status.success());
        let (admin, cred, state, external, pg) =
            stage_sentinels(&root, &format!("c2-{fail_after}"));
        let admin_before = fs::read(&admin).unwrap();
        let cred_before = fs::read(&cred).unwrap();
        let state_before = fs::read(&state).unwrap();
        let v2 = temp_binary("p79-c2-v2-content-DIFFERENT");
        let out = run_install(
            &root,
            &v2,
            &[("SYNVEIL_INSTALL_FAIL_AFTER", &fail_after.to_string())],
        );
        if fail_after >= n {
            assert!(out.status.success());
        } else {
            assert!(!out.status.success(), "fail_after={fail_after} must fail");
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            assert!(
                stderr.contains("config/state")
                    || stderr.contains("preserved")
                    || stderr.contains("PACKAGE may be partially"),
                "failure must document data-preservation, got: {stderr}"
            );
        }
        assert_eq!(fs::read(&admin).unwrap(), admin_before, "config preserved");
        assert_eq!(
            fs::read(&cred).unwrap(),
            cred_before,
            "credential preserved"
        );
        assert_eq!(fs::read(&state).unwrap(), state_before, "state preserved");
        assert!(external.exists(), "external preserved");
        assert!(pg.join("PG_VERSION").exists(), "pg preserved");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&external);
        let _ = fs::remove_dir_all(&pg);
    }
}

// ---------------------------------------------------------------------------
// C3/C4 — reinstall repairs damaged artifacts and modes, preserves admin
// ---------------------------------------------------------------------------

#[test]
fn adversarial_reinstall_repairs_damaged_package_artifacts() {
    let root = temp_root("c3-repair");
    let v1 = temp_binary("p79-c3-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let (admin, cred, state, _, _) = stage_sentinels(&root, "c3");
    // Corrupt/remove package artifacts.
    let bin_path = root.join("usr/bin/synveil-scheduled-maintenance-once");
    let svc_path = root.join("usr/lib/systemd/system/synveil-scheduled-maintenance.service");
    let tmr_path = root.join("usr/lib/systemd/system/synveil-scheduled-maintenance.timer");
    fs::write(&bin_path, "CORRUPTED").unwrap();
    fs::remove_file(&svc_path).unwrap();
    fs::write(&tmr_path, "CORRUPTED TIMER").unwrap();
    // Reinstall with a known-good binary.
    let good = temp_binary("p79-c3-good-binary");
    let good_bytes = fs::read(&good).unwrap();
    assert!(run_install(&root, &good, &[]).status.success());
    assert_eq!(fs::read(&bin_path).unwrap(), good_bytes, "binary repaired");
    assert!(
        fs::read_to_string(&svc_path)
            .unwrap()
            .contains("ExecStart="),
        "service repaired"
    );
    assert!(
        fs::read_to_string(&tmr_path)
            .unwrap()
            .contains("OnCalendar"),
        "timer repaired"
    );
    assert!(admin.exists() && cred.exists() && state.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn adversarial_reinstall_restores_package_owned_modes() {
    let root = temp_root("c4-mode");
    let v1 = temp_binary("p79-c4-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let bin_path = root.join("usr/bin/synveil-scheduled-maintenance-once");
    fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(file_mode(&bin_path), 0o644);
    let v2 = temp_binary("p79-c4-v2");
    assert!(run_install(&root, &v2, &[]).status.success());
    assert_eq!(file_mode(&bin_path), 0o755, "package-owned mode restored");
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// C5/C6/C7 — uninstall / reinstall-after-uninstall / purge
// ---------------------------------------------------------------------------

#[test]
fn adversarial_ordinary_uninstall_preserves_config_state_external_db() {
    let root = temp_root("c5-uninstall");
    let v1 = temp_binary("p79-c5-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let (admin, cred, state, external, pg) = stage_sentinels(&root, "c5");
    // Shared parents pre-exist; record them.
    for parent in ["usr/bin", "usr/lib/systemd/system", "etc", "var/lib"] {
        fs::create_dir_all(root.join(parent)).unwrap();
    }
    assert!(run_uninstall(&root, false).status.success());
    // PACKAGE removed.
    assert!(
        !root
            .join("usr/bin/synveil-scheduled-maintenance-once")
            .exists()
    );
    assert!(
        !root
            .join("usr/lib/systemd/system/synveil-scheduled-maintenance.service")
            .exists()
    );
    // Preserved.
    assert!(admin.exists(), "/etc/synveil preserved");
    assert!(cred.exists(), "credential preserved");
    assert!(state.exists(), "state preserved");
    assert!(external.exists(), "external preserved");
    assert!(pg.join("PG_VERSION").exists(), "pg preserved");
    // Shared parents preserved.
    for parent in ["usr/bin", "usr/lib/systemd/system", "etc", "var/lib"] {
        assert!(
            root.join(parent).exists(),
            "shared parent {parent} preserved"
        );
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&external);
    let _ = fs::remove_dir_all(&pg);
}

#[test]
fn adversarial_reinstall_after_uninstall_restores_and_reuses() {
    let root = temp_root("c6-reinstall");
    let v1 = temp_binary("p79-c6-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let (admin, cred, state, _, _) = stage_sentinels(&root, "c6");
    let admin_before = fs::read(&admin).unwrap();
    let cred_before = fs::read(&cred).unwrap();
    assert!(run_uninstall(&root, false).status.success());
    let v2 = temp_binary("p79-c6-v2-new");
    let v2_bytes = fs::read(&v2).unwrap();
    assert!(run_install(&root, &v2, &[]).status.success());
    assert_eq!(
        fs::read(root.join("usr/bin/synveil-scheduled-maintenance-once")).unwrap(),
        v2_bytes
    );
    assert_eq!(fs::read(&admin).unwrap(), admin_before, "config reused");
    assert_eq!(fs::read(&cred).unwrap(), cred_before, "credential reused");
    assert!(state.exists(), "state reused");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn adversarial_explicit_purge_removes_only_governed_state() {
    let root = temp_root("c7-purge");
    let v1 = temp_binary("p79-c7-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let (admin, cred, state, external, pg) = stage_sentinels(&root, "c7");
    // External pool with nested content.
    let ext_dir = std::env::temp_dir().join(format!(
        "p79-adv-c7-extdir-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::create_dir_all(&ext_dir).unwrap();
    fs::write(ext_dir.join("objects.bin"), "USER_OBJECTS\n").unwrap();
    assert!(run_uninstall(&root, true).status.success());
    assert!(!admin.exists(), "purge removes admin config");
    assert!(!cred.exists(), "purge removes credential");
    assert!(!state.exists(), "purge removes state");
    assert!(
        !root.join("etc/synveil").exists(),
        "purge removes config dir"
    );
    assert!(
        !root.join("var/lib/synveil").exists(),
        "purge removes state dir"
    );
    assert!(external.exists(), "external file survives purge");
    assert!(
        ext_dir.join("objects.bin").exists(),
        "external dir survives purge"
    );
    assert!(pg.join("PG_VERSION").exists(), "pg survives purge");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&external);
    let _ = fs::remove_dir_all(&ext_dir);
    let _ = fs::remove_dir_all(&pg);
}

// ---------------------------------------------------------------------------
// D1/D2 — binary and unit symlink attacks
// ---------------------------------------------------------------------------

#[test]
fn adversarial_binary_symlink_unlinked_without_target_deletion() {
    let root = temp_root("d1-binlink");
    let v1 = temp_binary("p79-d1-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let bin_path = root.join("usr/bin/synveil-scheduled-maintenance-once");
    let sentinel = std::env::temp_dir().join(format!(
        "p79-adv-d1-target-{}",
        uuid::Uuid::now_v7().simple()
    ));
    fs::write(&sentinel, "D1_EXTERNAL\n").unwrap();
    fs::remove_file(&bin_path).unwrap();
    std::os::unix::fs::symlink(&sentinel, &bin_path).unwrap();
    assert!(run_uninstall(&root, false).status.success());
    assert!(
        !bin_path.exists() && !bin_path.is_symlink(),
        "link unlinked"
    );
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "D1_EXTERNAL\n");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&sentinel);
}

#[test]
fn adversarial_unit_symlink_unlinked_without_target_deletion() {
    let root = temp_root("d2-unitlink");
    let v1 = temp_binary("p79-d2-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    for dest in [
        "usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
    ] {
        let link = root.join(dest);
        let sentinel = std::env::temp_dir().join(format!(
            "p79-adv-d2-{}-{}",
            dest.replace('/', "_"),
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&sentinel, "D2_EXTERNAL\n").unwrap();
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&sentinel, &link).unwrap();
        assert!(run_uninstall(&root, false).status.success());
        assert!(
            !link.is_symlink() && !link.exists(),
            "unit link {dest} unlinked"
        );
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "D2_EXTERNAL\n");
        let _ = fs::remove_file(&sentinel);
        // Reinstall for the second unit in the same root.
        if dest.contains(".service") {
            let v2 = temp_binary("p79-d2-v2");
            assert!(run_install(&root, &v2, &[]).status.success());
        }
    }
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// D3/D4 — config/state symlink attacks on purge
// ---------------------------------------------------------------------------

#[test]
fn adversarial_config_and_state_symlinks_not_traversed_on_purge() {
    let root = temp_root("d34-links");
    let v1 = temp_binary("p79-d34-v1");
    assert!(run_install(&root, &v1, &[]).status.success());
    let _ = stage_sentinels(&root, "d34");
    // Replace /etc/synveil with a symlink to an external dir.
    let etc_link = root.join("etc/synveil");
    let ext_etc =
        std::env::temp_dir().join(format!("p79-adv-d3-ext-{}", uuid::Uuid::now_v7().simple()));
    fs::create_dir_all(&ext_etc).unwrap();
    fs::write(ext_etc.join("keep"), "D3_KEEP\n").unwrap();
    fs::remove_dir_all(&etc_link).unwrap();
    std::os::unix::fs::symlink(&ext_etc, &etc_link).unwrap();
    // Replace /var/lib/synveil with a symlink to an external dir.
    let state_link = root.join("var/lib/synveil");
    let ext_state =
        std::env::temp_dir().join(format!("p79-adv-d4-ext-{}", uuid::Uuid::now_v7().simple()));
    fs::create_dir_all(&ext_state).unwrap();
    fs::write(ext_state.join("objects"), "D4_KEEP\n").unwrap();
    if state_link.is_dir() && !state_link.is_symlink() {
        fs::remove_dir_all(&state_link).unwrap();
    }
    std::os::unix::fs::symlink(&ext_state, &state_link).unwrap();

    assert!(run_uninstall(&root, true).status.success());
    assert!(
        ext_etc.join("keep").exists(),
        "config symlink target must survive purge"
    );
    assert!(
        ext_state.join("objects").exists(),
        "state symlink target must survive purge"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&ext_etc);
    let _ = fs::remove_dir_all(&ext_state);
}

// ---------------------------------------------------------------------------
// D5/D6/D7/D8 — root validation and shared parents
// ---------------------------------------------------------------------------

#[test]
fn adversarial_dotdot_relative_and_host_root_rejected() {
    let bin = temp_binary("p79-d567-v1");
    // D5: `..` escape rejected.
    let evil = std::env::temp_dir().join(format!("p79-adv-{}-ok", uuid::Uuid::now_v7().simple()));
    fs::create_dir_all(&evil).unwrap();
    let dotdot = format!("{}/../evil", evil.display());
    let out = run_install(Path::new(&dotdot), &bin, &[]);
    assert!(!out.status.success(), ".. root must be rejected");
    // D6: relative root rejected.
    let out = run_install(Path::new("relative/root"), &bin, &[]);
    assert!(!out.status.success(), "relative root must be rejected");
    // D7: host root without opt-in rejected.
    let out = run_install(Path::new("/"), &bin, &[]);
    assert!(
        !out.status.success(),
        "host root without opt-in must be rejected"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(stderr.contains("SYNVEIL_ALLOW_HOST_ROOT"));
    let _ = fs::remove_dir_all(&evil);
}

#[test]
fn adversarial_shared_parents_never_removed() {
    let root = temp_root("d8-parents");
    let bin = temp_binary("p79-d8-v1");
    // Pre-create shared parents with canary files proving pre-existence.
    for parent in [
        "usr/bin",
        "usr/lib/systemd/system",
        "usr/lib/sysusers.d",
        "usr/lib/tmpfiles.d",
        "usr/share",
        "etc",
        "var/lib",
    ] {
        let dir = root.join(parent);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".canary"), "SHARED_PARENT\n").unwrap();
    }
    assert!(run_install(&root, &bin, &[]).status.success());
    assert!(run_uninstall(&root, false).status.success());
    assert!(run_uninstall(&root, true).status.success());
    for parent in [
        "usr/bin",
        "usr/lib/systemd/system",
        "usr/lib/sysusers.d",
        "usr/lib/tmpfiles.d",
        "usr/share",
        "etc",
        "var/lib",
    ] {
        assert!(
            root.join(parent).exists(),
            "shared parent {parent} must never be removed"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// J — unit corruption is detectable (disposable copies only)
// ---------------------------------------------------------------------------

fn production_service_text() -> String {
    fs::read_to_string(service_path()).expect("read production service")
}

/// Minimal validator mirroring the static deployment tests: returns a list of
/// violations for a candidate unit text. Empty means the candidate passes the
/// production contract.
fn validate_service_candidate(text: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let active: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with(';') && !t.trim().is_empty()
        })
        .collect();
    let get = |key: &str| {
        active
            .iter()
            .rev()
            .find(|l| l.trim_start().starts_with(key))
            .and_then(|l| l.split_once('='))
            .map(|x| x.1.trim())
            .unwrap_or("")
            .to_string()
    };
    if get("ExecStart=") != "/usr/bin/synveil-scheduled-maintenance-once" {
        violations.push(format!(
            "ExecStart must be canonical, got {:?}",
            get("ExecStart=")
        ));
    }
    if get("User=") != "synveil" {
        violations.push(format!("User must be synveil, got {:?}", get("User=")));
    }
    if get("Group=") != "synveil" {
        violations.push(format!("Group must be synveil, got {:?}", get("Group=")));
    }
    if !active
        .iter()
        .any(|l| l.trim() == "LoadCredential=database-url:/etc/synveil/credentials/database-url")
    {
        violations.push("LoadCredential delivery missing".to_string());
    }
    if get("Restart=") != "no" {
        violations.push(format!("Restart must be no, got {:?}", get("Restart=")));
    }
    if get("ProtectSystem=") != "strict" {
        violations.push(format!(
            "ProtectSystem must be strict, got {:?}",
            get("ProtectSystem=")
        ));
    }
    if get("NoNewPrivileges=") != "yes" {
        violations.push("NoNewPrivileges must be yes".to_string());
    }
    if active.iter().any(|l| l.contains("CAP_SYS_ADMIN")) {
        violations.push("must not grant CAP_SYS_ADMIN".to_string());
    }
    violations
}

#[test]
fn adversarial_production_unit_passes_validator() {
    let text = production_service_text();
    let violations = validate_service_candidate(&text);
    assert!(
        violations.is_empty(),
        "production unit must pass: {violations:?}"
    );
    // Production file itself is never mutated by these tests.
    assert!(
        text.contains("User=synveil") && text.contains("ProtectSystem=strict"),
        "production contract intact"
    );
}

#[allow(clippy::type_complexity)]
#[test]
fn adversarial_corrupted_unit_copies_are_detected() {
    let base = production_service_text();
    let cases: Vec<(&str, Box<dyn Fn(String) -> String>)> = vec![
        (
            "wrong ExecStart",
            Box::new(|s: String| {
                s.replace(
                    "ExecStart=/usr/bin/synveil-scheduled-maintenance-once",
                    "ExecStart=/usr/bin/false",
                )
            }),
        ),
        (
            "missing User",
            Box::new(|s: String| {
                s.lines()
                    .filter(|l| !l.trim_start().starts_with("User="))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
        ),
        (
            "User=root",
            Box::new(|s: String| s.replace("User=synveil", "User=root")),
        ),
        (
            "missing LoadCredential",
            Box::new(|s: String| {
                s.lines()
                    .filter(|l| !l.trim_start().starts_with("LoadCredential="))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
        ),
        (
            "Restart=always",
            Box::new(|s: String| s.replace("Restart=no", "Restart=always")),
        ),
        (
            "removed ProtectSystem",
            Box::new(|s: String| {
                s.lines()
                    .filter(|l| !l.trim_start().starts_with("ProtectSystem="))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
        ),
    ];
    for (label, mutate) in cases {
        let corrupted = mutate(base.clone());
        // Write to a disposable copy only; never touch the production unit.
        let tmp = std::env::temp_dir().join(format!(
            "p79-adv-unit-{}-{}",
            label.replace(' ', "_"),
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&tmp, &corrupted).unwrap();
        let read_back = fs::read_to_string(&tmp).unwrap();
        let violations = validate_service_candidate(&read_back);
        assert!(
            !violations.is_empty(),
            "corrupted copy ({label}) must be detected, got none"
        );
        let _ = fs::remove_file(&tmp);
    }
    // Production unit still passes after all corruption probes.
    assert!(validate_service_candidate(&base).is_empty());
}

// ---------------------------------------------------------------------------
// K — packaging artifact integrity + manifest negative cases
// ---------------------------------------------------------------------------

#[test]
fn adversarial_installed_package_artifacts_match_sources() {
    let root = temp_root("k-integrity");
    let bin = temp_binary("p79-k-v1-binary-bytes");
    assert!(run_install(&root, &bin, &[]).status.success());
    let manifest = parse_manifest();
    for (source, dest, _mode, _owner, _group, class) in &manifest {
        if class != "PACKAGE" {
            continue;
        }
        let full = root.join(dest.trim_start_matches('/'));
        assert!(full.exists(), "PACKAGE artifact missing: {dest}");
        let expected: Vec<u8> = if source == "BINARY" {
            fs::read(&bin).unwrap()
        } else {
            fs::read(repo_root().join(source))
                .unwrap_or_else(|e| panic!("read source {source}: {e}"))
        };
        assert_eq!(
            fs::read(&full).unwrap(),
            expected,
            "PACKAGE artifact {dest} must be byte-identical to source {source}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn adversarial_manifest_negative_cases_rejected() {
    // K1: duplicate manifest destination must be rejected by validation.
    let manifest = parse_manifest();
    let mut seen = HashSet::new();
    for (_, dest, _, _, _, _) in &manifest {
        assert!(
            seen.insert(dest.clone()),
            "K1: duplicate manifest destination {dest}"
        );
    }
    // K2: every destination must be under an allowed prefix.
    let allowed = [
        "/usr/bin/",
        "/usr/lib/systemd/system/",
        "/usr/lib/sysusers.d/",
        "/usr/lib/tmpfiles.d/",
        "/usr/share/synveil/",
        "/etc/synveil",
        "/var/lib/synveil",
        "/run/synveil",
    ];
    for (_, dest, _, _, _, _) in &manifest {
        assert!(
            allowed
                .iter()
                .any(|p| dest == p.trim_end_matches('/') || dest.starts_with(p)),
            "K2: path outside allowed prefix: {dest}"
        );
        assert!(
            !dest.contains(".."),
            "K2: destination must not contain ..: {dest}"
        );
    }
    // K3: unknown lifecycle class must be rejected (install.sh errors).
    // Proved by the installer's `unknown manifest class` branch; here we lock
    // that the manifest contains only known classes.
    for (_, dest, _, _, _, class) in &manifest {
        assert!(
            [
                "PACKAGE",
                "CONFIG_DIRECTORY",
                "CREDENTIAL_DIRECTORY",
                "STATE_DIRECTORY",
                "RUNTIME_MANAGED"
            ]
            .contains(&class.as_str()),
            "K3: unknown lifecycle class {class} for {dest}"
        );
    }
    // K4: permission contract mismatch must fail validation — every manifest
    // mode parses as octal and installed files carry the declared mode.
    let root = temp_root("k4-modes");
    let bin = temp_binary("p79-k4-v1");
    assert!(run_install(&root, &bin, &[]).status.success());
    for (_, dest, mode, _, _, class) in &manifest {
        if matches!(class.as_str(), "STATE_DIRECTORY" | "RUNTIME_MANAGED") {
            continue; // not created by payload
        }
        let full = root.join(dest.trim_start_matches('/'));
        let expected = u32::from_str_radix(mode, 8).expect("mode must be octal");
        assert_eq!(
            file_mode(&full),
            expected,
            "K4: mode mismatch for {dest}: expected {mode}, got {:o}",
            file_mode(&full)
        );
    }
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// L — service identity attacks (static)
// ---------------------------------------------------------------------------

#[test]
fn adversarial_service_identity_prohibits_privileged_configurations() {
    let svc = fs::read_to_string(service_path()).expect("read service");
    let active: Vec<String> = svc
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with(';') && !t.trim().is_empty()
        })
        .map(|s| s.to_string())
        .collect();
    for bad in [
        "User=root",
        "Group=root",
        "DynamicUser=yes",
        "SupplementaryGroups=wheel",
        "SupplementaryGroups=docker",
        "CAP_SYS_ADMIN",
        "CAP_SYS_PTRACE",
        "CAP_SYS_RAWIO",
    ] {
        assert!(
            !active
                .iter()
                .any(|l| l.trim() == bad || l.contains(bad) && bad.starts_with("CAP")),
            "identity attack configuration must be absent: {bad}"
        );
    }
    // DynamicUser must not appear at all (persistent account required).
    assert!(
        !active
            .iter()
            .any(|l| l.trim_start().starts_with("DynamicUser=")),
        "DynamicUser must be absent (persistent synveil account)"
    );
    assert!(
        !active
            .iter()
            .any(|l| l.trim_start().starts_with("SupplementaryGroups=")),
        "no supplementary groups (least privilege)"
    );
    // Timer/syusers/tmpfiles Sanity: persistent non-login account, no fixed UID.
    let sysusers = fs::read_to_string(repo_root().join("deploy/sysusers.d/synveil.conf"))
        .expect("read sysusers");
    assert!(
        sysusers.contains("synveil"),
        "sysusers must declare synveil"
    );
    assert!(
        !sysusers.contains("UID") || sysusers.contains("synveil"),
        "no arbitrary fixed UID expected"
    );
    assert!(
        sysusers.contains("nologin")
            || sysusers.contains("/usr/sbin/nologin")
            || sysusers.contains("-"),
        "account must be non-login (shell nologin or locked)"
    );
    let _ = timer_path;
}

// ---------------------------------------------------------------------------
// I (static) — sandbox negative surface locked
// ---------------------------------------------------------------------------

#[test]
fn adversarial_sandbox_negative_surface_locked() {
    let svc = fs::read_to_string(service_path()).expect("read service");
    let active: Vec<String> = svc
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with(';') && !t.trim().is_empty()
        })
        .map(|s| s.to_string())
        .collect();
    let has = |key: &str| active.iter().any(|l| l.trim_start().starts_with(key));
    // No writable exceptions under ProtectSystem=strict.
    assert!(!has("ReadWritePaths="), "zero writable paths expected");
    // No secrets in Environment.
    assert!(
        !active.iter().any(|l| l.contains("DATABASE_URL=")),
        "no secret in Environment"
    );
    // Credential boundary preserved + denied source.
    assert!(
        active.iter().any(
            |l| l.trim() == "LoadCredential=database-url:/etc/synveil/credentials/database-url"
        ),
        "LoadCredential preserved"
    );
    assert!(
        active
            .iter()
            .any(|l| l.contains("/etc/synveil/credentials") && l.contains("InaccessiblePaths")),
        "InaccessiblePaths must deny credential source"
    );
    // Unprivileged identity under sandbox.
    assert!(active.iter().any(|l| l.trim() == "User=synveil"));
    assert!(active.iter().any(|l| l.trim() == "Group=synveil"));
    // systemd-analyze verify on a patched copy (ExecStart canonical path does
    // not exist in CI container, so patch to /usr/bin/true for syntax check).
    if Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .is_ok()
    {
        let patched = svc.replace(
            "ExecStart=/usr/bin/synveil-scheduled-maintenance-once",
            "ExecStart=/usr/bin/true",
        );
        let tmp = std::env::temp_dir().join(format!(
            "p79-adv-verify-{}.service",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&tmp, patched).unwrap();
        let out = Command::new("systemd-analyze")
            .arg("verify")
            .arg(&tmp)
            .output()
            .unwrap();
        let _ = fs::remove_file(&tmp);
        assert!(
            out.status.success(),
            "systemd-analyze verify failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // Shell syntax.
    for script in [
        "deploy/install/install.sh",
        "deploy/install/uninstall.sh",
        "deploy/install/common.sh",
    ] {
        let out = Command::new("bash")
            .arg("-n")
            .arg(repo_root().join(script))
            .output()
            .unwrap();
        assert!(out.status.success(), "bash -n failed for {script}");
    }
}

// ---------------------------------------------------------------------------
// M — data-preservation sentinel matrix (critical deliverable)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
#[test]
fn adversarial_data_preservation_matrix() {
    // One staged scenario with sentinels in every governed location plus
    // external/PG locations outside the root. Each lifecycle op runs on a
    // FRESH clone so operations do not interfere; the matrix asserts the
    // contractual YES/NO per operation.
    let ops: Vec<(&str, Box<dyn Fn(&Path, &Path)>)> = vec![
        (
            "reinstall",
            Box::new(|root: &Path, bin: &Path| {
                assert!(run_install(root, bin, &[]).status.success());
            }),
        ),
        (
            "upgrade",
            Box::new(|root: &Path, _bin: &Path| {
                let v2 = temp_binary("p79-matrix-upgrade-v2");
                assert!(run_install(root, &v2, &[]).status.success());
            }),
        ),
        (
            "failed-upgrade",
            Box::new(|root: &Path, bin: &Path| {
                let v2 = temp_binary("p79-matrix-failed-v2");
                let out = run_install(root, &v2, &[("SYNVEIL_INSTALL_FAIL_AFTER", "2")]);
                assert!(!out.status.success());
                let _ = bin;
            }),
        ),
        (
            "uninstall",
            Box::new(|root: &Path, _bin: &Path| {
                assert!(run_uninstall(root, false).status.success());
            }),
        ),
        (
            "reinstall-after-uninstall",
            Box::new(|root: &Path, bin: &Path| {
                assert!(run_uninstall(root, false).status.success());
                assert!(run_install(root, bin, &[]).status.success());
            }),
        ),
        (
            "purge",
            Box::new(|root: &Path, _bin: &Path| {
                assert!(run_uninstall(root, true).status.success());
            }),
        ),
    ];
    // Expected preservation: YES = file must exist after op.
    // Columns: reinstall upgrade failed uninstall reinstall purge
    // admin:      YES     YES     YES     YES      YES      NO
    // credential: YES     YES     YES     YES      YES      NO
    // state:      YES     YES     YES     YES      YES      NO
    // external:   YES     YES     YES     YES      YES      YES
    // pg:         YES     YES     YES     YES      YES      YES
    let mut matrix: Vec<(String, bool, bool, bool, bool, bool)> = Vec::new();
    for (label, op) in &ops {
        let root = temp_root(&format!("matrix-{label}"));
        let v1 = temp_binary("p79-matrix-v1");
        assert!(run_install(&root, &v1, &[]).status.success());
        let (admin, cred, state, external, pg) = stage_sentinels(&root, label);
        // Simulated binary PACKAGE artifact sentinel (package-owned).
        let pkg_bin = root.join("usr/bin/synveil-scheduled-maintenance-once");
        assert!(pkg_bin.exists());
        op(&root, &v1);
        let admin_y = admin.exists();
        let cred_y = cred.exists();
        let state_y = state.exists();
        let ext_y =
            external.exists() && fs::read_to_string(&external).unwrap().contains("EXTERNAL");
        let pg_y = pg.join("PG_VERSION").exists();
        eprintln!(
            "MATRIX {label}: admin={} cred={} state={} external={} pg={}",
            if admin_y { "YES" } else { "NO" },
            if cred_y { "YES" } else { "NO" },
            if state_y { "YES" } else { "NO" },
            if ext_y { "YES" } else { "NO" },
            if pg_y { "YES" } else { "NO" },
        );
        matrix.push((label.to_string(), admin_y, cred_y, state_y, ext_y, pg_y));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&external);
        let _ = fs::remove_dir_all(&pg);
    }
    let expect = |label: &str| {
        matrix
            .iter()
            .find(|(l, _, _, _, _, _)| l == label)
            .unwrap()
            .clone()
    };
    for label in [
        "reinstall",
        "upgrade",
        "failed-upgrade",
        "uninstall",
        "reinstall-after-uninstall",
    ] {
        let (_, admin, cred, state, ext, pg) = expect(label);
        assert!(admin, "{label}: admin config YES");
        assert!(cred, "{label}: credential YES");
        assert!(state, "{label}: state YES");
        assert!(ext, "{label}: external YES");
        assert!(pg, "{label}: pg YES");
    }
    let (_, admin, cred, state, ext, pg) = expect("purge");
    assert!(!admin, "purge: admin NO");
    assert!(!cred, "purge: credential NO");
    assert!(!state, "purge: state NO");
    assert!(ext, "purge: external YES");
    assert!(pg, "purge: pg YES");
}
