# Synveil iOS v0.1 — Prompt 014 Manifest

## 1. Prompt Overview & Metadata
* **Prompt Number**: `014`
* **Prompt Title**: `Minimal Rust iOS FFI Crate & Apple Target Compilation`
* **Authoritative Integration Branch**: `ios-app`
* **Starting Integration Baseline SHA**: `e8d3e24515d0525d5a1f82380f4ae7533ec1ed01`
* **Work Branch**: `ios/p014-rust-apple-target`
* **Target PR Base**: `ios-app`

---

## 2. Rust Workspace & Toolchain Diagnostics
* **Rust Toolchain Channel**: `stable` (`rustc 1.94.0`)
* **Cargo Resolver**: `3`
* **Workspace Edition**: `2024`
* **Workspace Version**: `0.1.0`

---

## 3. Bridge Crate Specifications
* **Bridge Crate Directory**: `crates/ios-ffi/`
* **Cargo Package Name**: `synveil-ios-ffi`
* **Artifact Crate Types**: `["staticlib", "rlib"]` (`cdylib` strictly prohibited and omitted)
* **Workspace Registration**: Added `"crates/ios-ffi"` to `[workspace.members]` in root `Cargo.toml`.

---

## 4. Dependency Closure & Prohibited Dependency Audit
* **Direct Dependencies**: `synveil-core` (platform-neutral domain primitives)
* **Dependency Tree Summary**:
  ```text
  synveil-ios-ffi v0.1.0 (/app/crates/ios-ffi)
  └── synveil-core v0.1.0 (/app/crates/core)
      ├── chrono v0.4.45
      ├── chrono-tz v0.10.4
      ├── sha2 v0.10.9
      ├── subtle v2.6.1
      ├── time v0.3.55
      ├── uuid v1.24.1
      └── zeroize v1.9.0
  ```
* **Prohibited Dependency Audit Result**:
  * `synveil-client`: ABSENT
  * `synveil-desktop`: ABSENT
  * `synveil-install-engine`: ABSENT
  * `synveil-api`: ABSENT
  * `synveil-metadata`: ABSENT
  * `synveil-platform`: ABSENT
  * `sqlx`: ABSENT
  * `reqwest`: ABSENT
  * `tokio`: ABSENT
  * `keyring`: ABSENT
  * `cxx` / `cxx-qt`: ABSENT
  * `axum`: ABSENT
  * **Status**: `PASS` — Zero desktop, server, async runtime, database, or keyring dependencies present.

---

## 5. Apple Target Compilation Verification
* **Target Discovery Output**:
  * `aarch64-apple-ios` (Physical iPhone device) — SUPPORTED
  * `aarch64-apple-ios-sim` (Apple Silicon Simulator) — SUPPORTED
  * `x86_64-apple-ios` (Intel Simulator) — SUPPORTED
* **Required Minimum Apple Targets**:
  * `aarch64-apple-ios`
  * `aarch64-apple-ios-sim`
* **Deployment Target Policy**: `IPHONEOS_DEPLOYMENT_TARGET=17.0`
* **Compilation & Build Commands**:
  * `cargo check -p synveil-ios-ffi --locked --target <target>`
  * `cargo build -p synveil-ios-ffi --locked --target <target>`
* **Produced Static Libraries**:
  * `target/aarch64-apple-ios/debug/libsynveil_ios_ffi.a`
  * `target/aarch64-apple-ios-sim/debug/libsynveil_ios_ffi.a`
  * `target/x86_64-apple-ios/debug/libsynveil_ios_ffi.a`
* **Binary Commit Policy**: `NO` — Output stays under ignored `target/`; verified zero `.a` files tracked in Git.

---

## 6. Bridge Implementation & Deferral Boundaries
* **C ABI Exports Status**: `NONE_IN_P014` (No `extern "C"` functions or `#[no_mangle]` symbols; deferred to P016).
* **`cbindgen` Decision**: `CBINDGEN_DEFERRED_TO_P015_OR_P016` (No C ABI functions exist yet; header generation deferred to avoid empty artifact).
* **C Header Status**: `NONE_IN_P014` (`synveil_ios_ffi.h` deferred to P016).
* **Swift / Xcode Linking Status**: `NONE_IN_P014` (Xcode project `project.pbxproj` unmodified; deferred to P015/P016).

---

## 7. CI Workflow Integration
* **CI Workflow File**: `.github/workflows/ios-rust-apple-build.yml`
* **CI Job Name**: `Rust Apple Target Build`
* **CI Runner Environment**: `macos-latest`
* **Target Build Script**: `scripts/check-ios-rust.sh`

---

## 8. Summary of Files Touched
* **Created**:
  * `crates/ios-ffi/Cargo.toml`
  * `crates/ios-ffi/src/lib.rs`
  * `crates/ios-ffi/README.md`
  * `scripts/check-ios-rust.sh`
  * `.github/workflows/ios-rust-apple-build.yml`
  * `docs/ios/manifests/PROMPT014_MANIFEST.md`
* **Modified**:
  * `Cargo.toml`
  * `Cargo.lock`
  * `clients/ios/Infrastructure/RustBridge/README.md`

---

## 9. Gate Verification
* **Host Cargo Format**: `cargo fmt --all -- --check` -> PASS
* **Host Cargo Check**: `cargo check -p synveil-ios-ffi --locked` -> PASS
* **Host Cargo Test**: `cargo test -p synveil-ios-ffi --locked` -> PASS
* **Host Cargo Clippy**: `cargo clippy -p synveil-ios-ffi --all-targets --locked -- -D warnings` -> PASS
* **iOS Source Validation**: `python3 clients/ios/Support/validate_ios_sources.py` -> PASS
* **Apple Rust Target Verification Script**: `./scripts/check-ios-rust.sh` -> PASS

---

## 10. Unrelated Subsystem Failures
* **Linux AppImage Job (`Build, reproduce, inspect, and smoke AppImage`)**:
  * **Status**: Failed with `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop`.
  * **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` — Desktop AppImage build failure is an existing desktop packaging issue on Linux runners and unrelated to native iOS Rust FFI crate (`synveil-ios-ffi`) compilation.
