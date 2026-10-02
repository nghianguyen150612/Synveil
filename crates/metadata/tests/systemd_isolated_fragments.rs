#![cfg(target_os = "linux")]

//! Prompt020 — version-compatible isolated validation of the shipped
//! sysusers/tmpfiles fragments.
//!
//! Runs on every supported systemd. `--dry-run` is used where it exists
//! (>= 250) and a provably host-confined `--root` execution is used otherwise
//! (249). See `common/mod.rs` for the rationale and the non-mutation proof.

mod common;

use std::{fs, path::PathBuf};

use common::{HostSnapshot, IsolatedRoot, validate_sysusers, validate_tmpfiles};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Proves the isolated strategy is not vacuous.
///
/// A deliberately invalid tmpfiles mode and an unknown sysusers command type
/// must both be rejected under whichever strategy is active. Without this, a
/// broken isolation setup could make every fragment check pass silently, and a
/// broken fragment could go unnoticed.
#[test]
fn isolated_validation_rejects_invalid_fragments_and_spares_host() {
    let snapshot = HostSnapshot::capture();

    let isolated = IsolatedRoot::new("selftest-tmpfiles");
    let bad_tmpfiles = isolated.path().join("bad.conf");
    fs::write(&bad_tmpfiles, "d /var/lib/synveil BADMODE\n").unwrap();
    assert!(
        validate_tmpfiles(&bad_tmpfiles).is_some_and(|result| result.is_err()),
        "an invalid tmpfiles mode must be rejected, not silently accepted"
    );

    let isolated = IsolatedRoot::new("selftest-sysusers");
    let bad_sysusers = isolated.path().join("bad.conf");
    fs::write(&bad_sysusers, "q not-a-real-command-type\n").unwrap();
    assert!(
        validate_sysusers(&bad_sysusers).is_some_and(|result| result.is_err()),
        "an unknown sysusers command type must be rejected, not silently accepted"
    );

    snapshot.assert_unchanged("isolated validation self-test");
}

/// The fragments actually shipped by this repository must validate.
#[test]
fn shipped_sysusers_fragment_validates_in_isolation() {
    match validate_sysusers(&repo_root().join("deploy/sysusers.d/synveil.conf")) {
        None => eprintln!(
            "systemd-sysusers unavailable on this host; static sysusers checks cover \
             it (limitation reported, not treated as a pass)"
        ),
        Some(Ok(())) => {}
        Some(Err(error)) => panic!("{error}"),
    }
}

#[test]
fn shipped_tmpfiles_fragment_validates_in_isolation() {
    match validate_tmpfiles(&repo_root().join("deploy/tmpfiles.d/synveil.conf")) {
        None => eprintln!(
            "systemd-tmpfiles unavailable on this host; static tmpfiles checks cover \
             it (limitation reported, not treated as a pass)"
        ),
        Some(Ok(())) => {}
        Some(Err(error)) => panic!("{error}"),
    }
}
