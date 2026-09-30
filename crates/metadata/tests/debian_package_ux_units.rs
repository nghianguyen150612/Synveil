#![cfg(target_os = "linux")]

//! Prompt013 focused source/workflow regressions (DEB-UX-1 through 10, 17, 18).

use std::{path::PathBuf, process::Command};

#[test]
fn prompt013_source_and_ci_contract_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .arg(root.join("scripts/validate-debian-package-ux.py"))
        .current_dir(&root)
        .output()
        .expect("run Prompt013 validator");
    assert!(
        output.status.success(),
        "Prompt013 validator failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
