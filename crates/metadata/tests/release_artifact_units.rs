#![cfg(target_os = "linux")]

//! Prompt 113 release-artifact determinism and validation units.
//!
//! These tests consume the real release artifacts and generated manifests when
//! the packaging gate has produced them. They never create a passing fake
//! release artifact: mutation coverage copies a real artifact and intentionally
//! corrupts it so the hash gate must reject it.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn manifest(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref().to_path_buf();
    path.is_file().then_some(path)
}

fn linux_manifest() -> Option<PathBuf> {
    manifest(repo_root().join("target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt"))
}

fn reproducible_manifest() -> Option<PathBuf> {
    manifest(repo_root().join("target/packages-reproducible/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt"))
}

fn validator() -> PathBuf {
    repo_root().join("scripts/validate-release-artifacts.sh")
}

fn run_validator(manifest_path: &Path) -> std::process::Output {
    Command::new("bash")
        .arg(validator())
        .arg(format!("--manifest={}", manifest_path.display()))
        .current_dir(repo_root())
        .output()
        .expect("run release artifact validator")
}

fn compare_artifacts(expected: &Path, actual: &Path) -> std::process::Output {
    Command::new("bash")
        .arg("-c")
        .arg("source \"$1\"; synveil_require_identical_artifacts \"$2\" \"$3\" test-artifact")
        .arg("synveil-artifact-compare")
        .arg(repo_root().join("deploy/packages/common/reproducible.sh"))
        .arg(expected)
        .arg(actual)
        .current_dir(repo_root())
        .output()
        .expect("compare release artifacts")
}

fn first_artifact_line(contents: &str) -> Option<(usize, String, String)> {
    let marker = "artifacts=sha256 size build_id path";
    let marker_line = contents.lines().position(|line| line == marker)?;
    contents
        .lines()
        .enumerate()
        .skip(marker_line + 1)
        .find_map(|(index, line)| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            (fields.len() == 4).then(|| (index, fields[0].to_owned(), fields[3].to_owned()))
        })
}

#[test]
fn artifact_unit_1_same_source_produces_stable_artifact_metadata() {
    let common = read(repo_root().join("deploy/packages/common/reproducible.sh"));
    let builder = read(repo_root().join("deploy/packages/build.sh"));
    assert!(common.contains("CARGO_INCREMENTAL=0"));
    assert!(common.contains("export QT_HASH_SEED=0"));
    assert!(common.contains("synveil_prepare_reproducible_qt_tools"));
    assert!(common.contains("--remap-path-prefix"));
    assert!(common.contains("CARGO_TARGET_DIR"));
    assert!(common.contains("source_fingerprint"));
    let qt_wrapper = read(repo_root().join("scripts/reproducible-qt-wrapper.rs"));
    assert!(qt_wrapper.contains("normalize_qml_resources"));
    assert!(qt_wrapper.contains("SOURCE_DATE_EPOCH"));
    assert!(qt_wrapper.contains("QT_HOST_LIBEXECS/get"));
    let workspace_manifest = read(repo_root().join("Cargo.toml"));
    assert!(workspace_manifest.contains("[patch.crates-io]"));
    assert!(workspace_manifest.contains("cxx-qt-build = { path = \"vendor/cxx-qt-build\" }"));
    let cxx_qt_build = read(repo_root().join("vendor/cxx-qt-build/src/lib.rs"));
    assert!(cxx_qt_build.contains("qt_modules.sort();"));
    assert!(builder.contains("SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt"));
    let desktop_build = read(repo_root().join("crates/desktop/build.rs"));
    assert!(desktop_build.contains("cargo:rerun-if-env-changed=QT_HASH_SEED"));
    assert!(desktop_build.contains("cargo:rerun-if-env-changed=QMAKE"));
    assert!(desktop_build.contains("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH"));

    match (linux_manifest(), reproducible_manifest()) {
        (Some(first), Some(second)) => {
            assert_eq!(
                fs::read(&first).expect("read primary artifact manifest"),
                fs::read(&second).expect("read reproducible artifact manifest"),
                "same source/toolchain inputs must produce identical artifact metadata"
            );
        }
        _ => eprintln!(
            "SKIP ARTIFACT-UNIT-1: both primary and reproducible release manifests are required"
        ),
    }
}

#[test]
fn artifact_unit_2_hash_comparison_detects_intentional_modification() {
    let Some(manifest_path) = linux_manifest() else {
        eprintln!("SKIP ARTIFACT-UNIT-2: Linux release manifest is absent");
        return;
    };
    let contents = read(&manifest_path);
    let Some((line_index, expected_hash, relative_path)) = first_artifact_line(&contents) else {
        panic!("release manifest has no artifact entry");
    };
    let artifact = repo_root().join(relative_path);
    let tamper_dir = repo_root().join("target/p113-artifact-unit");
    fs::create_dir_all(&tamper_dir).expect("create artifact tamper directory");
    let temp_path = tamper_dir.join(format!("tampered-{}", std::process::id()));
    fs::copy(&artifact, &temp_path).expect("copy real release artifact for mutation test");
    let mut bytes = fs::read(&temp_path).expect("read copied release artifact");
    let last = bytes.last_mut().expect("release artifact is non-empty");
    *last ^= 0x01;
    fs::write(&temp_path, &bytes).expect("write intentional artifact mutation");

    let comparison = compare_artifacts(&artifact, &temp_path);
    assert!(!comparison.status.success());
    let comparison_stderr = String::from_utf8_lossy(&comparison.stderr);
    assert!(
        comparison_stderr.contains("expected:"),
        "diagnostic: {comparison_stderr}"
    );
    assert!(
        comparison_stderr.contains("actual:"),
        "diagnostic: {comparison_stderr}"
    );
    assert!(
        comparison_stderr.contains("first differing byte:"),
        "diagnostic: {comparison_stderr}"
    );

    let digest = Command::new("sha256sum")
        .arg(&temp_path)
        .output()
        .expect("hash tampered release artifact");
    assert!(digest.status.success());
    let actual_hash = String::from_utf8_lossy(&digest.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    assert_ne!(actual_hash, expected_hash);

    let mut lines = contents.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut fields = lines[line_index]
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fields[3] = temp_path
        .strip_prefix(repo_root())
        .expect("tampered artifact is repo-relative")
        .to_string_lossy()
        .replace('\\', "/");
    lines[line_index] = fields.join(" ");
    let tampered_manifest =
        tamper_dir.join(format!("tampered-manifest-{}.txt", std::process::id()));
    fs::write(&tampered_manifest, lines.join("\n") + "\n")
        .expect("write tampered artifact manifest");
    let output = run_validator(&tampered_manifest);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected:"), "diagnostic: {stderr}");
    assert!(stderr.contains("actual:"), "diagnostic: {stderr}");
    let _ = fs::remove_file(temp_path);
    let _ = fs::remove_file(tampered_manifest);
}

#[test]
fn artifact_unit_3_developer_absolute_paths_are_rejected() {
    let common = read(repo_root().join("deploy/packages/common/reproducible.sh"));
    assert!(common.contains("/home/"));
    assert!(common.contains("/mnt/"));
    assert!(common.contains("[A-Za-z]:"));
    assert!(common.contains("private or temporary build path"));

    if let Some(manifest_path) = linux_manifest() {
        let output = run_validator(&manifest_path);
        assert!(
            output.status.success(),
            "release path scan failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        eprintln!("SKIP ARTIFACT-UNIT-3 artifact scan: Linux release manifest is absent");
    }
}

#[test]
fn artifact_unit_4_temporary_build_paths_do_not_leak_into_artifacts() {
    let common = read(repo_root().join("deploy/packages/common/reproducible.sh"));
    assert!(common.contains("/tmp/synveil-"));
    assert!(common.contains("/var/tmp/synveil-"));
    assert!(common.contains("synveil-target"));

    if let Some(manifest_path) = linux_manifest() {
        let output = run_validator(&manifest_path);
        assert!(
            output.status.success(),
            "temporary-path artifact scan failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        eprintln!("SKIP ARTIFACT-UNIT-4 artifact scan: Linux release manifest is absent");
    }
}

#[test]
fn artifact_unit_7_nested_target_roots_use_stable_rust_and_cxx_prefixes() {
    let root = repo_root()
        .canonicalize()
        .expect("canonicalize repository root for compiler prefix probe");
    let probe_dir = root
        .join("target")
        .join(format!("p113-target-root-probe-{}", std::process::id()));
    let source = "const char *build_path = __FILE__;\n";
    let rust_source = "fn main() { println!(\"{}\", file!()); }\n";
    let roots = [probe_dir.join("root-a"), probe_dir.join("root-b")];

    for target_root in &roots {
        let target_dir = target_root.join("target");
        fs::create_dir_all(&target_dir).expect("create temporary target-root probe directory");
        fs::write(target_dir.join("prefix.cpp"), source).expect("write temporary C++ prefix probe");
        fs::write(target_dir.join("prefix.rs"), rust_source)
            .expect("write temporary Rust prefix probe");

        let output = Command::new("bash")
            .arg("-c")
            .arg(
                r#"set -euo pipefail
source "$1"
repo_root="$2"
export CARGO_TARGET_DIR="$3"
synveil_prepare_reproducible_rust_build "$repo_root"
read -r -a cxx_flags <<< "$CXXFLAGS"
read -r -a rust_flags <<< "$RUSTFLAGS"
c++ "${cxx_flags[@]}" -c "$CARGO_TARGET_DIR/prefix.cpp" -o "$CARGO_TARGET_DIR/prefix.o"
rustc "${rust_flags[@]}" --edition=2021 --crate-name p113_path_probe \
    "$CARGO_TARGET_DIR/prefix.rs" -o "$CARGO_TARGET_DIR/rust-prefix"
strings "$CARGO_TARGET_DIR/prefix.o"
"#,
            )
            .arg("synveil-target-root-probe")
            .arg(root.join("deploy/packages/common/reproducible.sh"))
            .arg(&root)
            .arg(&target_dir)
            .current_dir(&root)
            .output()
            .expect("run Rust and C++ prefix-map probes");
        assert!(
            output.status.success(),
            "compiler prefix-map probe failed for {}: {}",
            target_dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let strings = String::from_utf8_lossy(&output.stdout);
        assert!(
            strings.contains("/usr/src/synveil-target/prefix.cpp"),
            "C++ output did not use the stable target prefix: {strings}"
        );
        let rust_output = Command::new(target_dir.join("rust-prefix"))
            .output()
            .expect("run Rust source-path probe");
        assert!(rust_output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&rust_output.stdout).trim(),
            "/usr/src/synveil-target/prefix.rs",
            "Rust file!() must use the stable target prefix"
        );
    }

    let target_a = roots[0].join("target");
    let target_b = roots[1].join("target");
    for artifact in ["prefix.o", "rust-prefix"] {
        assert_eq!(
            fs::read(target_a.join(artifact)).expect("read first-root compiler output"),
            fs::read(target_b.join(artifact)).expect("read second-root compiler output"),
            "compiler output {artifact} must be byte-identical across temporary target roots"
        );
    }
    let _ = fs::remove_dir_all(&probe_dir);
}

#[test]
fn artifact_unit_5_manifest_matches_generated_artifacts() {
    let Some(manifest_path) = linux_manifest() else {
        eprintln!("SKIP ARTIFACT-UNIT-5: Linux release manifest is absent");
        return;
    };
    let output = run_validator(&manifest_path);
    assert!(
        output.status.success(),
        "generated artifact manifest did not validate: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = read(&manifest_path);
    let marker_line = contents
        .lines()
        .position(|line| line == "artifacts=sha256 size build_id path")
        .expect("artifact marker");
    assert_eq!(
        contents
            .lines()
            .skip(marker_line + 1)
            .filter(|line| line.split_whitespace().count() == 4)
            .count(),
        3,
        "Linux manifest must describe maintenance, client, and desktop binaries"
    );
}

#[test]
fn artifact_unit_6_validation_failure_explains_mismatch_reason() {
    let common = read(repo_root().join("deploy/packages/common/reproducible.sh"));
    let validator_source = read(validator());
    assert!(common.contains("expected:"));
    assert!(common.contains("actual:"));
    assert!(validator_source.contains("different source tree"));
    assert!(validator_source.contains("release artifact manifest mismatch"));

    let Some(manifest_path) = linux_manifest() else {
        eprintln!("SKIP ARTIFACT-UNIT-6: Linux release manifest is absent");
        return;
    };
    let mut lines = read(&manifest_path)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let marker_index = lines
        .iter()
        .position(|line| line == "artifacts=sha256 size build_id path")
        .expect("artifact marker");
    let entry = lines
        .iter_mut()
        .skip(marker_index + 1)
        .find(|line| line.split_whitespace().count() == 4)
        .expect("artifact entry");
    let mut fields = entry
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fields[0] = "0".repeat(64);
    *entry = fields.join(" ");

    let tampered_manifest =
        std::env::temp_dir().join(format!("synveil-p113-manifest-{}", std::process::id()));
    fs::write(&tampered_manifest, lines.join("\n") + "\n")
        .expect("write intentionally mismatched manifest");
    let output = run_validator(&tampered_manifest);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected:"), "diagnostic: {stderr}");
    assert!(stderr.contains("actual:"), "diagnostic: {stderr}");
    assert!(stderr.contains("manifest mismatch"), "diagnostic: {stderr}");
    let _ = fs::remove_file(tampered_manifest);
}
