//! Prompt 78 — Linux systemd Sandbox & Runtime Privilege Hardening.
//!
//! Static deployment tests that lock the evidence-validated sandbox contract
//! without mutating the host. These run in `cargo test --workspace` and do
//! not require PostgreSQL. Live compatibility (idle / due-work /
//! existing-work gates against PostgreSQL 17 plus negative probes) is
//! documented in ADR-026 and `docs/en/DEPLOYMENT.md`, not re-executed here.

use std::{fs, path::PathBuf, process::Command};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn service_path() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.service")
}

fn read_service() -> String {
    fs::read_to_string(service_path()).expect("read service unit")
}

fn active_lines(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with(';') && !t.is_empty()
        })
        .map(|s| s.to_string())
        .collect()
}

fn active_value(lines: &[String], key: &str) -> Option<String> {
    lines
        .iter()
        .rev()
        .find(|l| l.trim_start().starts_with(key))
        .map(|l| {
            l.split_once('=')
                .map(|x| x.1)
                .unwrap_or("")
                .trim()
                .to_string()
        })
}

fn require_exact(lines: &[String], key: &str, expected: &str) {
    let got = active_value(lines, key)
        .unwrap_or_else(|| panic!("service must contain active {key}, got none"));
    assert_eq!(
        got, expected,
        "service {key} must be exactly {expected:?}, got {got:?}"
    );
}

// ---------------------------------------------------------------------------
// Privilege / capabilities
// ---------------------------------------------------------------------------

#[test]
fn hardening_denies_privilege_escalation_and_capabilities() {
    let svc = read_service();
    let lines = active_lines(&svc);
    require_exact(&lines, "NoNewPrivileges=", "yes");
    require_exact(&lines, "RestrictSUIDSGID=", "yes");
    // Empty bounding + ambient sets: zero Linux capabilities granted.
    require_exact(&lines, "CapabilityBoundingSet=", "");
    require_exact(&lines, "AmbientCapabilities=", "");
    for l in &lines {
        assert!(
            !l.contains("CAP_SYS_ADMIN")
                && !l.contains("CAP_SYS_PTRACE")
                && !l.contains("CAP_SYS_RAWIO")
                && !l.contains("CAP_NET_ADMIN")
                && !l.contains("CAP_SYS_MODULE")
                && !l.contains("CAP_DAC_OVERRIDE"),
            "service must not grant Linux capabilities, got {l}"
        );
    }
    // Identity contract preserved under hardening.
    require_exact(&lines, "User=", "synveil");
    require_exact(&lines, "Group=", "synveil");
}

// ---------------------------------------------------------------------------
// Filesystem
// ---------------------------------------------------------------------------

#[test]
fn hardening_locks_filesystem_to_read_only_strict() {
    let svc = read_service();
    let lines = active_lines(&svc);
    require_exact(&lines, "ProtectSystem=", "strict");
    // Zero writable exceptions: the database-backed runtime writes nothing.
    for l in &lines {
        assert!(
            !l.trim_start().starts_with("ReadWritePaths="),
            "service must not grant writable paths (ProtectSystem=strict, zero writes demonstrated), got {l}"
        );
    }
    require_exact(&lines, "ProtectHome=", "yes");
    require_exact(&lines, "PrivateTmp=", "yes");
    require_exact(&lines, "PrivateDevices=", "yes");
    require_exact(&lines, "DevicePolicy=", "closed");
    // Credential source explicitly denied to the runtime identity.
    let inacc: Vec<_> = lines
        .iter()
        .filter(|l| l.trim_start().starts_with("InaccessiblePaths="))
        .collect();
    assert!(
        inacc.iter().any(|l| l.contains("/etc/synveil/credentials")),
        "InaccessiblePaths must deny /etc/synveil/credentials, got {inacc:?}"
    );
    require_exact(&lines, "UMask=", "0077");
    require_exact(&lines, "WorkingDirectory=", "/");
    // RuntimeDirectory reserved; no StateDirectory/LogsDirectory managers.
    assert!(
        lines.iter().any(|l| l.trim() == "RuntimeDirectory=synveil"),
        "RuntimeDirectory=synveil must be reserved"
    );
    for l in &lines {
        assert!(
            !l.trim_start().starts_with("StateDirectory="),
            "StateDirectory must not compete with tmpfiles.d, got {l}"
        );
        assert!(
            !l.trim_start().starts_with("LogsDirectory="),
            "no filesystem log path (journald owns logging), got {l}"
        );
    }
}

// ---------------------------------------------------------------------------
// Kernel / process / namespace
// ---------------------------------------------------------------------------

#[test]
fn hardening_restricts_kernel_process_and_namespaces() {
    let svc = read_service();
    let lines = active_lines(&svc);
    require_exact(&lines, "ProtectKernelTunables=", "yes");
    require_exact(&lines, "ProtectKernelModules=", "yes");
    require_exact(&lines, "ProtectKernelLogs=", "yes");
    require_exact(&lines, "ProtectControlGroups=", "yes");
    require_exact(&lines, "ProtectProc=", "invisible");
    require_exact(&lines, "ProcSubset=", "pid");
    require_exact(&lines, "RestrictRealtime=", "yes");
    require_exact(&lines, "LockPersonality=", "yes");
    assert!(
        lines
            .iter()
            .any(|l| l.trim_start().starts_with("RestrictNamespaces=")),
        "RestrictNamespaces must bound namespace creation"
    );
    // PrivateUsers is a documented deferral (persistent synveil ownership vs
    // user namespaces); it must not be enabled silently.
    for l in &lines {
        assert!(
            l.trim() != "PrivateUsers=yes",
            "PrivateUsers=yes is deferred by ADR-026; enabling it requires documented evidence, got {l}"
        );
    }
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

#[test]
fn hardening_bounds_address_families_to_postgres_only() {
    let svc = read_service();
    let lines = active_lines(&svc);
    let raf = active_value(&lines, "RestrictAddressFamilies=")
        .expect("RestrictAddressFamilies must be declared");
    for fam in ["AF_UNIX", "AF_INET", "AF_INET6"] {
        assert!(
            raf.split_whitespace().any(|x| x == fam),
            "RestrictAddressFamilies must allow {fam} for PostgreSQL, got {raf}"
        );
    }
    for fam in [
        "AF_PACKET",
        "AF_NETLINK",
        "AF_BLUETOOTH",
        "AF_VSOCK",
        "AF_CAN",
    ] {
        assert!(
            !raf.split_whitespace().any(|x| x == fam),
            "RestrictAddressFamilies must not allow {fam}, got {raf}"
        );
    }
    assert!(
        !lines.iter().any(|l| l.trim() == "PrivateNetwork=yes"),
        "PrivateNetwork=yes would break PostgreSQL TCP"
    );
    // IP allow/deny deferred: no baked-in localhost assumption.
    for l in &lines {
        assert!(
            !l.trim_start().starts_with("IPAddressAllow="),
            "IP allowlist is deferred (DB host is a packaging choice), got {l}"
        );
    }
}

// ---------------------------------------------------------------------------
// Syscall / execution policy
// ---------------------------------------------------------------------------

#[test]
fn hardening_validates_syscall_and_execution_policy() {
    let svc = read_service();
    let lines = active_lines(&svc);
    require_exact(&lines, "SystemCallArchitectures=", "native");
    require_exact(&lines, "MemoryDenyWriteExecute=", "yes");
    let filters: Vec<_> = lines
        .iter()
        .filter(|l| l.trim_start().starts_with("SystemCallFilter="))
        .map(|l| {
            l.split_once('=')
                .map(|x| x.1)
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .collect();
    assert!(
        filters.iter().any(|f| f == "@system-service"),
        "SystemCallFilter must start from @system-service, got {filters:?}"
    );
    for denied in [
        "@mount",
        "@raw-io",
        "@reboot",
        "@swap",
        "@module",
        "@debug",
        "@privileged",
        "@obsolete",
    ] {
        assert!(
            filters.iter().any(|f| f == &format!("~{denied}")),
            "SystemCallFilter must deny {denied}, got {filters:?}"
        );
    }
    require_exact(&lines, "SystemCallErrorNumber=", "EPERM");
}

// ---------------------------------------------------------------------------
// Credential delivery preserved under hardening
// ---------------------------------------------------------------------------

#[test]
fn hardening_preserves_prompt77_credential_delivery() {
    let svc = read_service();
    let lines = active_lines(&svc);
    assert!(
        lines.iter().any(
            |l| l.trim() == "LoadCredential=database-url:/etc/synveil/credentials/database-url"
        ),
        "LoadCredential delivery must be preserved, got {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.trim() == "Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url"),
        "non-secret credential-path variable must be preserved"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.trim_start().starts_with("EnvironmentFile=")),
        "non-secret EnvironmentFile tuning must be preserved"
    );
    for l in &lines {
        assert!(
            !l.contains("DATABASE_URL="),
            "unit must not carry secrets in Environment (Prompt 77 boundary), got {l}"
        );
    }
}

// ---------------------------------------------------------------------------
// systemd syntax + security posture smoke
// ---------------------------------------------------------------------------

#[test]
fn hardened_unit_verifies_cleanly() {
    let svc_path = service_path();
    let has_analyze = Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .is_ok();
    if !has_analyze {
        eprintln!("systemd-analyze not available; static assertions above still hold");
        return;
    }
    let svc_content = fs::read_to_string(&svc_path).expect("read service");
    let patched = svc_content.replace(
        "ExecStart=/usr/bin/synveil-scheduled-maintenance-once",
        "ExecStart=/usr/bin/true",
    );
    let tmp = std::env::temp_dir().join(format!(
        "synveil-p78-verify-{}.service",
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

// ---------------------------------------------------------------------------
// Boundaries: portable core, migrations, background runtime
// ---------------------------------------------------------------------------

#[test]
fn portable_core_has_no_systemd_sandbox_contamination() {
    let core_dir = repo_root().join("crates/core/src");
    let out = Command::new("grep")
        .arg("-R")
        .arg("-n")
        .arg("CapabilityBoundingSet\\|ProtectSystem\\|SystemCallFilter\\|NoNewPrivileges\\|RestrictNamespaces\\|LoadCredential")
        .arg(&core_dir)
        .output()
        .expect("grep");
    if out.status.success() {
        panic!(
            "crates/core must not contain systemd sandbox concepts:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

#[test]
fn historical_hardening_scope_remains_unchanged_with_current_migration_count() {
    let dir = repo_root().join("migrations");
    let mut sql_count = 0;
    for entry in fs::read_dir(&dir).expect("read migrations") {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".sql") {
            sql_count += 1;
        }
    }
    assert_eq!(
        sql_count, 36,
        "Prompt 86 adds one migration; the current workspace has 36, got {sql_count}"
    );
}
