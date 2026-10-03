# Synveil iOS v0.1 — Prompt 015 Manifest

## 1. Prompt Overview & Metadata
* **Prompt Number**: `015`
* **Prompt Title**: `Rust Apple Artifact CI Pipeline`
* **Authoritative Integration Branch**: `ios-app`
* **Starting Integration Baseline SHA**: `859ee3778a34680408b0a5644b48d2790ce6416a`
* **Work Branch**: `ios/p015-rust-apple-artifacts-17711133897826658520`
* **Validated Implementation Head**: `5e450d4d8ca824e4d3fdd82f1af0e856b6568adf`
* **PR**: #50 — https://github.com/nghianguyen150612/Synveil/pull/50
* **Target PR Base**: `ios-app`

---

## 2. Authoritative Files Inspected
* `docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`
* `docs/ios/manifests/PROMPT014_MANIFEST.md`
* `crates/ios-ffi/Cargo.toml`
* `crates/ios-ffi/src/lib.rs`
* `crates/ios-ffi/README.md`
* `scripts/check-ios-rust.sh`
* `.github/workflows/ios-rust-apple-build.yml`
* `Cargo.toml`
* `Cargo.lock`
* `rust-toolchain.toml`
* `.gitignore`
* `clients/ios/Infrastructure/RustBridge/README.md`

---

## 3. Artifact Build & Packaging Specifications
* **Build Script**: `scripts/build-ios-rust-artifacts.sh`
* **Artifact Validator**: `scripts/validate_ios_rust_artifact.py`
* **CI Workflow**: `.github/workflows/ios-rust-apple-build.yml`
* **Workflow Name**: `iOS Rust Apple Build`
* **Job Name**: `Rust Apple Target Build`
* **Release Build Profile**: `release` (`cargo build -p synveil-ios-ffi --locked --release --target <target>`)
* **Minimum Deployment Target**: `IPHONEOS_DEPLOYMENT_TARGET=17.0`
* **Required Target Triples**:
  * `aarch64-apple-ios` (Physical iPhone device)
  * `aarch64-apple-ios-sim` (Apple Silicon Simulator)
* **Optional Target Status**:
  * `x86_64-apple-ios` (Intel Simulator) — SUPPORTED & PACKAGED

---

## 4. Staged Artifact Bundle Layout
* **Staging Root**: `target/ios-rust-artifacts/` (replace-not-overlay)
* **Staged File Set**:
  ```text
  target/ios-rust-artifacts/
  ├── device/
  │   └── arm64/
  │       └── libsynveil_ios_ffi.a
  ├── simulator/
  │   ├── arm64/
  │   │   └── libsynveil_ios_ffi.a
  │   ├── x86_64/
  │   │   └── libsynveil_ios_ffi.a
  │   └── universal/
  │       └── libsynveil_ios_ffi.a
  ├── manifest.json
  └── SHA256SUMS
  ```
* **Universal Simulator Library Decision**:
  * `lipo -create` used to combine `simulator/arm64/libsynveil_ios_ffi.a` and `simulator/x86_64/libsynveil_ios_ffi.a` into `simulator/universal/libsynveil_ios_ffi.a`.
  * Physical device arm64 (`aarch64-apple-ios`) is NEVER combined with simulator binaries.
* **Closed File Set Validation**: Enforced via `scripts/validate_ios_rust_artifact.py`. Zero transient junk, build scripts, object files, or `.DS_Store` present.

---

## 5. Integrity & Metadata Policies
* **Manifest Schema**: `manifest.json` (schema_version: 1)
* **SHA256SUMS Policy**: Relative path format `<sha256>  <rel_path>` with sorted order.
* **Checksum Verification**: Executed during build via `shasum -a 256 -c SHA256SUMS` and validated in Python runner.
* **Path Leakage Scan**: Validated zero private runner paths (`/Users/runner`, `/home/jules`, `/tmp`) in metadata.

---

## 6. Bridge Implementation & Deferral Boundaries
* **Dependency Closure Audit**: `synveil-ios-ffi` -> `synveil-core` (Zero desktop, server, async runtime, database, or keyring dependencies present).
* **C ABI Exports Status**: `NONE_IN_P015` (Verified via `nm` symbol inspection that no `synveil_ffi_*` symbols exist).
* **`cbindgen` Decision**: `DEFERRED_TO_P016` (No C ABI exports to generate headers for).
* **C Header Status**: `NONE_IN_P015` (`synveil_ios_ffi.h` deferred to P016).
* **XCFramework Status**: `NONE_IN_P015` (Verified static library bundle sufficient; XCFramework deferred until header exists).
* **Swift / Xcode Linking Status**: `NONE_IN_P015` (Xcode project `project.pbxproj` unmodified; deferred to P016).

---

## 7. Host Validation
* `python3 clients/ios/Support/validate_ios_sources.py` -> PASS
* `python3 -m unittest discover -s clients/ios/Support/tests` -> PASS
* `bash -n scripts/check-ios-rust.sh` -> PASS
* `bash -n scripts/build-ios-rust-artifacts.sh` -> PASS
* `python3 -m py_compile scripts/validate_ios_rust_artifact.py` -> PASS
* `PYTHONPATH=. python3 -m unittest discover -s scripts/tests` -> PASS
* `cargo check -p synveil-ios-ffi --locked` -> PASS
* `cargo test -p synveil-ios-ffi --locked` -> PASS
* `cargo clippy -p synveil-ios-ffi --all-targets --locked -- -D warnings` -> PASS

---

## 8. Summary of Files Touched
* **Created**:
  * `scripts/build-ios-rust-artifacts.sh`
  * `scripts/validate_ios_rust_artifact.py`
  * `scripts/tests/test_validate_ios_rust_artifact.py`
  * `docs/ios/manifests/PROMPT015_MANIFEST.md`
* **Modified**:
  * `.github/workflows/ios-rust-apple-build.yml`
  * `clients/ios/Infrastructure/RustBridge/README.md`
  * `crates/ios-ffi/README.md`
  * `clients/ios/README.md`

---

## 9. Native iOS & Rust Apple CI Gates Evidence
* **macOS CI Environment**: `macos-latest` (`macos-26-arm64`), macOS `26.6.2`, Xcode `26.6`
* **Workflow Run ID**: `37128317766`
* **Job ID**: `111218104276`
* **Uploaded Artifact Name**: `synveil-ios-rust-staticlibs`
* **Uploaded Artifact ID**: `11275772250`
* **Uploaded Artifact Size**: `29,524,667 bytes` (~29.5 MB)
* **Expiration / Retention State**: 14 days (Expired: False)
* **Device arm64 Result**: SUCCESS (`target/ios-rust-artifacts/device/arm64/libsynveil_ios_ffi.a`)
* **Simulator arm64 Result**: SUCCESS (`target/ios-rust-artifacts/simulator/arm64/libsynveil_ios_ffi.a`)
* **Simulator x86_64 Result**: SUCCESS (`target/ios-rust-artifacts/simulator/x86_64/libsynveil_ios_ffi.a`)
* **Simulator Universal Result**: SUCCESS (`target/ios-rust-artifacts/simulator/universal/libsynveil_ios_ffi.a`)
* **iOS Static Validation Gate**: SUCCESS (Run `37128317749`)
* **iOS Build Gate**: SUCCESS (Run `37128317751`)
* **iOS Simulator Tests Gate**: SUCCESS (Run `37128315752` / `37128317753`)

---

## 10. Unrelated Subsystem Failures
* **Linux AppImage Job (`Build, reproduce, inspect, and smoke AppImage`)**:
  * **Status**: Failed with `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop`.
  * **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` — Desktop AppImage build failure is an existing desktop packaging issue on Linux runners and unrelated to native iOS Rust FFI crate (`synveil-ios-ffi`) static library artifact CI packaging.

---

## 11. Merge Status
* **PR**: #50 — https://github.com/nghianguyen150612/Synveil/pull/50
* **PR Base**: `ios-app`
* **Merge Status**: READY_TO_MERGE
* **Final `ios-app` SHA**: PENDING_MERGE
