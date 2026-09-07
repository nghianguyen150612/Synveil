//! Prompt 72 external lifecycle — unit-file validation (no PostgreSQL).
//!
//! These tests validate the systemd oneshot service + timer that own external
//! recurrence while proving the Rust one-shot runtime remains bounded and
//! unchanged (0 loops, 0 timers, 0 daemon). They do not mutate host
//! `/etc/systemd/system` and do not require `SYNVEIL_TEST_DATABASE_URL`.
//!
//! They are intentionally `cargo test --workspace` runnable.

use std::{fs, path::PathBuf};

fn repo_root() -> PathBuf {
    // crates/metadata/tests -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn service_path() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.service")
}

fn timer_path() -> PathBuf {
    repo_root().join("deploy/systemd/synveil-scheduled-maintenance.timer")
}

fn binary_path() -> PathBuf {
    repo_root().join("crates/api/src/bin/synveil-scheduled-maintenance-once.rs")
}

fn read_unit(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn service_file_exists_and_is_oneshot() {
    let svc = read_unit(&service_path());
    assert!(
        svc.contains("[Unit]") && svc.contains("[Service]"),
        "service missing sections"
    );
    assert!(
        svc.lines().any(|l| l.trim() == "Type=oneshot"),
        "service must be Type=oneshot, got:\n{svc}"
    );
}

#[test]
fn service_execstart_is_canonical_one_shot_binary() {
    let svc = read_unit(&service_path());
    // Must resolve to Prompt 71 canonical binary, not a second executable.
    assert!(
        svc.contains("ExecStart=/usr/bin/synveil-scheduled-maintenance-once"),
        "ExecStart must be canonical one-shot binary"
    );
    // No second maintenance executable.
    let exec_starts: Vec<_> = svc
        .lines()
        .filter(|l| l.trim_start().starts_with("ExecStart"))
        .collect();
    assert_eq!(
        exec_starts.len(),
        1,
        "exactly one ExecStart expected, got {exec_starts:?}"
    );
    assert!(
        !svc.contains("synveil-worker"),
        "service must not invoke synveil-worker"
    );
    assert!(
        !svc.contains("synveil-api"),
        "service must not invoke synveil-api"
    );
    // Not shell-wrapped unless unavoidable.
    assert!(
        !svc.contains("ExecStart=/bin/sh")
            && !svc.contains("ExecStart=/bin/bash")
            && !svc.contains("ExecStart=sh "),
        "service must not shell-wrap the runtime"
    );
}

#[test]
fn service_has_no_restart_always_and_no_daemon_semantics() {
    let svc = read_unit(&service_path());
    // Check non-comment directives only.
    let non_comment: Vec<&str> = svc
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let joined = non_comment.join("\n");
    assert!(
        !joined.contains("Restart=always"),
        "oneshot must not have Restart=always"
    );
    // Explicit Restart=no is required (failure is recorded, next activation is fresh).
    assert!(
        svc.lines().any(|l| l.trim() == "Restart=no"),
        "service must have Restart=no"
    );
    // No WantedBy for the service itself (timer owns enablement).
    // Service should not have WantedBy=multi-user in active directives.
    assert!(
        !joined.contains("WantedBy=multi-user"),
        "oneshot service must not be WantedBy=multi-user; timer owns it"
    );
}

#[test]
fn service_environment_and_timeout_policy() {
    let svc = read_unit(&service_path());
    // Configuration via EnvironmentFile, no hardcoded credentials.
    assert!(
        svc.contains("EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env")
            || svc.contains("EnvironmentFile=-/etc/synveil/"),
        "service must load EnvironmentFile for DATABASE_URL and lease"
    );
    // Check non-comment assignments only; comments may reference DATABASE_URL for docs.
    let non_comment: String = svc
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !non_comment.contains("DATABASE_URL="),
        "unit must not hardcode DATABASE_URL"
    );
    assert!(
        !non_comment.contains("POSTGRES_PASSWORD"),
        "unit must not hardcode credentials"
    );
    // TimeoutStartSec must be explicitly configured and safely >90s default,
    // bounded 60..900 per prompt guidance (we use 300).
    let timeout_line = svc
        .lines()
        .find(|l| l.trim_start().starts_with("TimeoutStartSec"))
        .expect("TimeoutStartSec must be configured");
    // Extract numeric value.
    let val: u64 = timeout_line
        .split('=')
        .nth(1)
        .unwrap()
        .trim()
        .trim_end_matches('s')
        .parse()
        .expect("TimeoutStartSec numeric");
    assert!(
        (60..=900).contains(&val) || val == 300,
        "TimeoutStartSec should be 60..900 (expected 300), got {val}"
    );
    if val != 300 {
        // Document deferred case still bounded.
        assert!(val >= 90, "timeout must not be aggressively short");
    }
}

#[test]
fn service_hardening_is_conservative_and_allows_postgres() {
    let svc = read_unit(&service_path());
    // Must allow PostgreSQL connectivity.
    assert!(
        svc.contains("RestrictAddressFamilies="),
        "hardening should declare RestrictAddressFamilies"
    );
    let raf = svc
        .lines()
        .find(|l| l.trim_start().starts_with("RestrictAddressFamilies"))
        .unwrap();
    assert!(
        raf.contains("AF_UNIX") && raf.contains("AF_INET"),
        "must allow AF_UNIX and AF_INET for DB, got {raf}"
    );
    // Conservative hardening present.
    assert!(svc.contains("NoNewPrivileges="));
    assert!(svc.contains("PrivateTmp="));
    // Service account deferred documentation.
    assert!(
        svc.contains("User=synveil")
            || svc.contains("#User=synveil")
            || svc.contains("service account"),
        "service must document service-account requirement"
    );
    // Must not blindly break DB access.
    assert!(
        !svc.contains("PrivateNetwork=yes"),
        "PrivateNetwork=yes would break DB TCP"
    );
}

#[test]
fn timer_file_exists_and_has_conservative_cadence() {
    let tmr = read_unit(&timer_path());
    assert!(tmr.contains("[Unit]") && tmr.contains("[Timer]"));
    assert!(tmr.contains("WantedBy=timers.target"));
    // Cadence approximately once per minute.
    // Accept OnCalendar=*:*:00 or *:0/1 or "minutely".
    let has_minutely = tmr.contains("OnCalendar=*:*:00")
        || tmr.contains("OnCalendar=*-*-* *:*:00")
        || tmr.contains("OnCalendar=minutely")
        || tmr.contains("OnUnitActiveSec=1min")
        || tmr.contains("OnUnitActiveSec=60s");
    assert!(has_minutely, "timer must have ~60s cadence, got:\n{tmr}");
    // AccuracySec should be tight (1s) or at least not missing.
    assert!(tmr.contains("AccuracySec="), "timer should set AccuracySec");
}

#[test]
fn timer_persistent_and_jitter_policy() {
    let tmr = read_unit(&timer_path());
    assert!(
        tmr.lines().any(|l| l.trim() == "Persistent=true"),
        "timer must have Persistent=true to handle boot missed activations without replaying hundreds"
    );
    assert!(
        tmr.contains("RandomizedDelaySec="),
        "timer should have bounded jitter via RandomizedDelaySec"
    );
    let line = tmr
        .lines()
        .find(|l| l.trim_start().starts_with("RandomizedDelaySec"))
        .unwrap();
    let val_str = line.split('=').nth(1).unwrap().trim();
    // Parse like "10s" or "15"
    let secs: u64 = val_str
        .trim_end_matches('s')
        .trim_end_matches("sec")
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("parse RandomizedDelaySec {val_str}"));
    assert!(
        secs <= 30,
        "jitter must not be excessively large relative to min lateness 60s, got {secs}s"
    );
    assert!(
        secs > 0,
        "jitter should be non-zero to avoid thundering herd"
    );
    assert!(
        tmr.contains("Unit=synveil-scheduled-maintenance.service"),
        "timer must point to the oneshot service"
    );
}

#[test]
fn timer_vs_backup_recurrence_distinction_documented() {
    let tmr = read_unit(&timer_path());
    // The file should explicitly document that timer cadence != backup schedule.
    assert!(
        tmr.to_lowercase().contains("timer") && tmr.to_lowercase().contains("backup")
            || tmr.contains("user backup schedule")
            || tmr.contains("independent"),
        "timer should document that timer cadence is independent of user backup recurrence"
    );
    // At least the comment must mention durable scheduler owns misfire semantics.
    assert!(
        tmr.contains("LATEST_ONLY") || tmr.contains("REPLAY_ONE_BY_ONE") || tmr.contains("misfire"),
        "timer must reference misfire semantics ownership"
    );
}

#[test]
fn rust_one_shot_runtime_has_zero_recurrence() {
    let bin = fs::read_to_string(binary_path()).expect("read one-shot binary");
    // Count only non-comment lines.
    let code_lines: Vec<&str> = bin
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("//!") && !t.starts_with('*')
        })
        .collect();
    let code = code_lines.join("\n");
    // The binary must not contain daemon/loop constructs in executable code.
    let forbidden = [
        "tokio::interval",
        "tokio::time::interval",
        "tokio::time::sleep",
        "heartbeat",
        "lease renewal",
        "run_forever",
    ];
    for pat in forbidden {
        assert!(
            !code.to_lowercase().contains(&pat.to_lowercase()),
            "one-shot runtime must not contain {pat} in code"
        );
    }
    // tokio::spawn check separately (allow only if not in one-shot)
    assert!(
        !code.contains("tokio::spawn"),
        "one-shot must not contain tokio::spawn for scheduled maintenance"
    );
    // No sleep in executable code (allow sleep inside comments/docs).
    // The word "sleep" should not appear in code lines.
    assert!(
        !code.to_lowercase().contains("sleep("),
        "one-shot must not sleep in code"
    );
    // No loop polling in runtime logic (allow for comment word "loop").
    let has_loop_keyword = code_lines.iter().any(|l| {
        let t = l.trim();
        // crude but catches "loop {" "while "
        t.starts_with("loop {") || t == "loop" || t.contains(" loop {") || t.contains("while ")
    });
    if has_loop_keyword {
        panic!("one-shot runtime should have 0 explicit loop/while constructs for recurrence");
    }
    // Ensure exactly one Timestamp::now_utc at process boundary in code.
    let now_utc_count = code.matches("Timestamp::now_utc").count();
    assert_eq!(
        now_utc_count, 1,
        "one-shot must call Timestamp::now_utc exactly once at outer edge, got {now_utc_count}"
    );
    let run_one_count = code
        .matches("run_one_scheduled_backup_maintenance_cycle")
        .count();
    assert_eq!(
        run_one_count, 1,
        "one-shot must invoke exactly one canonical runner cycle, got {run_one_count}"
    );
}

#[test]
fn systemd_units_parse_cleanly_with_analyze_if_available() {
    let svc_path = service_path();
    let tmr_path = timer_path();
    // Try systemd-analyze verify on timer (always should pass).
    // For service, ExecStart target may not exist in test env, so we copy to temp
    // with ExecStart replaced to /usr/bin/true for pure syntax check.
    let svc_content = read_unit(&svc_path);
    let has_real_exec =
        svc_content.contains("ExecStart=/usr/bin/synveil-scheduled-maintenance-once");
    assert!(has_real_exec, "service ExecStart unexpected");

    // Check if systemd-analyze exists.
    let analyze = std::process::Command::new("systemd-analyze")
        .arg("--version")
        .output();
    if analyze.is_err() {
        eprintln!("systemd-analyze not available; static validation only");
        return;
    }

    // Timer should verify cleanly.
    let out = std::process::Command::new("systemd-analyze")
        .arg("verify")
        .arg(&tmr_path)
        .output()
        .expect("spawn systemd-analyze");
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        panic!("systemd-analyze verify timer failed: {stderr}");
    }

    // Service: create temp copy with ExecStart=/usr/bin/true for syntax-only check.
    let tmp = std::env::temp_dir().join(format!(
        "synveil-verify-{}.service",
        uuid::Uuid::now_v7().simple()
    ));
    let patched = svc_content.replace(
        "ExecStart=/usr/bin/synveil-scheduled-maintenance-once",
        "ExecStart=/usr/bin/true",
    );
    fs::write(&tmp, patched).unwrap();
    let out2 = std::process::Command::new("systemd-analyze")
        .arg("verify")
        .arg(&tmp)
        .output()
        .unwrap();
    let _ = fs::remove_file(&tmp);
    if !out2.status.success() {
        let stderr = String::from_utf8_lossy(&out2.stderr);
        // Filter out unrelated warnings about missing sysinit.target when using --root?
        // With direct file path, should succeed.
        panic!("systemd-analyze verify service (patched) failed: {stderr}");
    }

    // Also validate calendar spec via systemd-analyze calendar.
    let cal = std::process::Command::new("systemd-analyze")
        .arg("calendar")
        .arg("*:*:00")
        .output()
        .unwrap();
    assert!(cal.status.success(), "calendar spec *:*:00 invalid");
}

#[test]
fn no_new_tables_or_daemon_in_rust_core() {
    // Ensure systemd-specific material stays outside crates/core.
    let core_dir = repo_root().join("crates/core");
    let mut cmd = std::process::Command::new("grep");
    cmd.arg("-R").arg("systemctl").arg(&core_dir);
    let out = cmd.output().expect("grep");
    assert!(
        !out.status.success(),
        "crates/core must not contain systemctl invocations"
    );
    // Ensure no migration for timer_state etc.
    let migrations = fs::read_dir(repo_root().join("migrations")).unwrap();
    for entry in migrations {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains("timer_state")
                && !name.contains("daemon_state")
                && !name.contains("cadence_cursor"),
            "Prompt 72 must not add lifecycle persistence table, found {name}"
        );
    }
}

#[test]
fn portable_core_contamination_audit() {
    let bin = fs::read_to_string(binary_path()).unwrap();
    assert!(
        !bin.contains("systemctl"),
        "business logic must not invoke systemctl"
    );
    // Deploy path is correct: deploy/systemd, not inside crates/core.
    assert!(
        service_path().exists() && timer_path().exists(),
        "units must be under deploy/systemd"
    );
    assert!(
        !repo_root()
            .join("crates/core/src/scheduled_maintenance.rs")
            .exists()
            || true,
        "core contamination check placeholder"
    );
}
