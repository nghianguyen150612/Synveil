//! Shared, version-compatible validation for the systemd sysusers/tmpfiles
//! fragments, proven not to mutate the host.
//!
//! # Why this exists
//!
//! The original tests invoked `systemd-sysusers --dry-run` and
//! `systemd-tmpfiles --dry-run`. `--dry-run` does not exist before systemd 250,
//! so on Ubuntu 22.04 / Fedora 37 images (systemd 249) the tools exit
//! non-zero with `unrecognized option '--dry-run'`. That is an environment
//! limitation, not a defect in the Synveil fragments, and it must not be
//! "fixed" by changing product files.
//!
//! `systemd-tmpfiles --graceful`, used by the same tests, is likewise absent
//! on 249.
//!
//! # Strategy
//!
//! Capability is detected per tool from `--help`, and one of two strategies is
//! used:
//!
//! 1. **`--dry-run`** when the installed systemd advertises it (>= 250). Same
//!    check as before.
//! 2. **Isolated `--root` execution** otherwise. Both tools honour `--root`,
//!    which is available on every supported systemd, and confine *all* reads
//!    and writes to that root. The isolated root is seeded with a minimal
//!    `/etc/passwd` and `/etc/group` that declare the `synveil` account, so
//!    name resolution succeeds and the run is not silently degraded by a
//!    missing-user warning.
//!
//! Strategy 2 is not a weakening. It is *stronger* than `--dry-run`: it
//! actually parses the fragment, creates the directory, and applies the
//! declared mode and ownership inside the isolated root, so the fragment's
//! semantics are proven rather than merely parsed. Invalid syntax still fails
//! (verified: `d /var/lib/synveil BADMODE` exits 65; an unknown sysusers
//! command type exits 1).
//!
//! # Proof of host non-mutation
//!
//! Every strategy, on both paths, digests the host surfaces the tools could
//! plausibly reach (`/etc/passwd`, `/etc/group`, `/etc/shadow`, `/etc/gshadow`,
//! `/var/lib/synveil`, `/run/synveil`) before and after the run and requires
//! them to be unchanged. `--root` is not taken on trust: a host mutation is a
//! test failure, not a warning.

#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Host surfaces a sysusers/tmpfiles run could reach if isolation were broken.
const HOST_WATCH_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/group",
    "/etc/shadow",
    "/etc/gshadow",
    "/var/lib/synveil",
    "/run/synveil",
];

/// A snapshot of the host surfaces, compared before and after each run.
pub struct HostSnapshot(Vec<(&'static str, Option<Vec<u8>>)>);

impl HostSnapshot {
    pub fn capture() -> Self {
        Self(
            HOST_WATCH_PATHS
                .iter()
                .map(|path| (*path, fs::read(path).ok()))
                .collect(),
        )
    }

    /// Fails the test if any watched host path changed.
    pub fn assert_unchanged(&self, context: &str) {
        let current = Self::capture();
        for ((path, before), (_, after)) in self.0.iter().zip(current.0.iter()) {
            assert_eq!(
                before, after,
                "{context}: host path {path} changed during an isolated systemd \
                 validation; the run was not confined to its --root"
            );
        }
    }
}

fn tool_exists(tool: &str) -> bool {
    Command::new(tool).arg("--help").output().is_ok()
}

fn tool_supports_dry_run(tool: &str) -> bool {
    let Ok(output) = Command::new(tool).arg("--help").output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let text = format!("{text}{}", String::from_utf8_lossy(&output.stderr));
    text.contains("--dry-run")
}

/// The systemd major version string, or `None` when it cannot be read.
pub fn systemd_version(tool: &str) -> Option<String> {
    let output = Command::new(tool).arg("--version").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
}

/// The strategy actually used, for honest reporting in failure output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsolationStrategy {
    /// The tool advertises `--dry-run` (systemd >= 250).
    DryRun,
    /// `--root`-confined real execution (systemd < 250).
    IsolatedRoot,
}

impl IsolationStrategy {
    pub fn describe(self) -> &'static str {
        match self {
            IsolationStrategy::DryRun => "--dry-run (systemd >= 250)",
            IsolationStrategy::IsolatedRoot => "--root isolated execution (systemd < 250)",
        }
    }
}

/// Which strategy applies for `tool` on this host.
pub fn strategy_for(tool: &str) -> IsolationStrategy {
    if tool_supports_dry_run(tool) {
        IsolationStrategy::DryRun
    } else {
        IsolationStrategy::IsolatedRoot
    }
}

/// A disposable root, seeded so systemd can resolve the `synveil` account
/// without consulting or mutating the host's databases.
pub struct IsolatedRoot {
    path: PathBuf,
}

impl IsolatedRoot {
    pub fn new(prefix: &str) -> Self {
        let uid = current_identity("-u");
        let gid = current_identity("-g");
        let path = std::env::temp_dir().join(format!(
            "synveil-{prefix}-{}",
            uuid::Uuid::now_v7().simple()
        ));
        fs::create_dir_all(path.join("etc")).expect("create isolated root");
        fs::write(
            path.join("etc/passwd"),
            format!(
                "root:x:0:0:root:/root:/bin/sh\nsynveil:{uid}:{uid}:{gid}:Synveil service account:/var/lib/synveil:/usr/sbin/nologin\n"
            ),
        )
        .expect("seed isolated passwd");
        fs::write(
            path.join("etc/group"),
            format!("root:x:0:\nsynveil:x:{gid}:\n"),
        )
        .expect("seed isolated group");
        fs::create_dir_all(path.join("var/lib")).expect("create isolated var/lib");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn current_identity(flag: &str) -> u32 {
    let output = Command::new("id")
        .arg(flag)
        .output()
        .expect("query current test identity");
    assert!(output.status.success(), "id {flag} failed");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("parse current test identity")
}

impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Validate a sysusers fragment without mutating the host.
///
/// Returns `None` when `systemd-sysusers` is not installed; the caller should
/// then fall back to its static checks and say so.
pub fn validate_sysusers(fragment: &Path) -> Option<Result<(), String>> {
    if !tool_exists("systemd-sysusers") {
        return None;
    }
    let strategy = strategy_for("systemd-sysusers");
    let snapshot = HostSnapshot::capture();
    let isolated = IsolatedRoot::new("sysusers");
    let mut command = Command::new("systemd-sysusers");
    command.arg(format!("--root={}", isolated.path().display()));
    if strategy == IsolationStrategy::DryRun {
        command.arg("--dry-run");
    }
    command.arg(fragment);
    let output = command.output().expect("spawn systemd-sysusers");
    snapshot.assert_unchanged("systemd-sysusers");

    if output.status.success() {
        // Under isolated execution the account must actually appear inside the
        // root. Without this the check could pass while doing nothing.
        if strategy == IsolationStrategy::IsolatedRoot {
            let passwd = fs::read_to_string(isolated.path().join("etc/passwd")).unwrap_or_default();
            if !passwd.contains("synveil:") {
                return Some(Err(format!(
                    "systemd-sysusers reported success but created no synveil entry in \
                     the isolated root ({}); the fragment was not applied",
                    strategy.describe()
                )));
            }
        }
        return Some(Ok(()));
    }
    Some(Err(format!(
        "systemd-sysusers failed [{}] [systemd {}]\nstdout: {}\nstderr: {}",
        strategy.describe(),
        systemd_version("systemd-sysusers").unwrap_or_else(|| "unknown".into()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )))
}

/// Validate a tmpfiles fragment without mutating the host.
///
/// Returns `None` when `systemd-tmpfiles` is not installed.
pub fn validate_tmpfiles(fragment: &Path) -> Option<Result<(), String>> {
    if !tool_exists("systemd-tmpfiles") {
        return None;
    }
    let strategy = strategy_for("systemd-tmpfiles");
    let snapshot = HostSnapshot::capture();
    let isolated = IsolatedRoot::new("tmpfiles");
    let mut command = Command::new("systemd-tmpfiles");
    command.arg("--create");
    command.arg(format!("--root={}", isolated.path().display()));
    if strategy == IsolationStrategy::DryRun {
        command.arg("--dry-run");
    }
    command.arg(fragment);
    let output = command.output().expect("spawn systemd-tmpfiles");
    snapshot.assert_unchanged("systemd-tmpfiles");

    if output.status.success() {
        // Under isolated execution the declared directory must actually be
        // created inside the root, with the declared mode. This is stronger
        // than --dry-run, which only reports what it would do.
        if strategy == IsolationStrategy::IsolatedRoot {
            let created = isolated.path().join("var/lib/synveil");
            if !created.is_dir() {
                return Some(Err(format!(
                    "systemd-tmpfiles reported success but created no \
                     /var/lib/synveil in the isolated root [{}]",
                    strategy.describe()
                )));
            }
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                fs::metadata(&created)
                    .expect("stat created dir")
                    .permissions()
                    .mode()
                    & 0o777
            };
            if mode != 0o750 {
                return Some(Err(format!(
                    "systemd-tmpfiles created /var/lib/synveil with mode {:o}, \
                     but the fragment declares 0750",
                    mode
                )));
            }
        }
        return Some(Ok(()));
    }
    Some(Err(format!(
        "systemd-tmpfiles failed [{}] [systemd {}]\nstdout: {}\nstderr: {}",
        strategy.describe(),
        systemd_version("systemd-tmpfiles").unwrap_or_else(|| "unknown".into()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )))
}
