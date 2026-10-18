# Prompt003 Manifest: Shared-Core Reuse Audit & Dependency Map

## Execution Summary

* **Prompt Number**: 003
* **Goal**: Perform a comprehensive audit of the existing Synveil Rust shared core for iOS reuse and produce `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`.
* **Starting Integration Branch**: `ios-app`
* **Starting Commit SHA**: `453969cf057af3343e9c0639b24fdc427322fe4d`
* **Work Branch**: `ios/p003-shared-core-reuse-audit`
* **Implementation Scope**: Shared core architecture audit, capability mapping, FFI boundaries identification, and dependency direction specification. No Rust FFI, C headers, Swift code, or Xcode projects were created.

---

## Files Inspected

### Documentation & Architectural Contracts
* `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
* `docs/ios/IOS_V0_1_ROADMAP.md`
* `docs/ios/IOS_PLATFORM_MAPPING.md`
* `docs/ios/PROMPT001_MANIFEST.md`
* `docs/ios/manifests/PROMPT002_MANIFEST.md`
* `docs/adr/ADR-001-rust-modular-monolith.md`
* `docs/adr/ADR-005-resumable-uploads.md`
* `docs/adr/ADR-006-change-journal-sync.md`
* `docs/adr/ADR-015-credential-model.md`
* `docs/adr/ADR-028-durable-rebaseline-snapshot-materialization.md`
* `docs/adr/ADR-032-deterministic-sync-conflict-preservation.md`
* `docs/adr/ADR-033-bounded-bidirectional-sync-cycle.md`

### Workspace Manifests & Shared Rust Crates
* `Cargo.toml` (Workspace root)
* `crates/core/` (`synveil-core`)
* `crates/object-store/` (`synveil-object-store`)
* `crates/client-sync/` (`synveil-client-sync`)
* `crates/platform/` (`synveil-platform`)
* `crates/client/` (`synveil-client`)
* `crates/desktop/` (`synveil-desktop`)
* `crates/install-engine/` (`synveil-install-engine`)
* `crates/auth/` (`synveil-auth`)
* `crates/api/` (`synveil-api`)
* `crates/metadata/` (`synveil-metadata`)
* `crates/storage/` (`synveil-storage`)

---

## Crates Inspected

1. `synveil-core`: Primary classification `DIRECTLY_REUSABLE` (`SOURCE_AUDIT_PORTABLE`).
2. `synveil-object-store`: Primary classification `DIRECTLY_REUSABLE` (`SOURCE_AUDIT_PORTABLE`).
3. `synveil-client-sync`: Primary classification `REUSABLE_WITH_ADAPTER` (requires decoupling SQLite / Reqwest IO).
4. `synveil-platform`: Primary classification `PLATFORM_COUPLED` (desktop keyring and OS path assumptions).
5. `synveil-client`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (desktop IPC sockets/process control).
6. `synveil-desktop`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (Qt/QML GUI shell).
7. `synveil-install-engine`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (Linux AppImage packaging).
8. `synveil-auth`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (Server authentication engine).
9. `synveil-api`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (Server Axum REST API).
10. `synveil-metadata`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (Server PostgreSQL database engine).
11. `synveil-storage`: Primary classification `UNSUITABLE_FOR_IOS_REUSE` (Server physical storage engine).

---

## Files Created

* `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`
* `docs/ios/manifests/PROMPT003_MANIFEST.md`

---

## Files Modified

* None (Zero modifications to existing codebase source files).

---

## Commands Run

* `git fetch origin` / `git checkout ios-app` (Verified starting SHA `453969cf057af3343e9c0639b24fdc427322fe4d`).
* `git checkout -b ios/p003-shared-core-reuse-audit` (Created work branch).
* `python3 /home/jules/self_created_tools/crate_inspector.py` (Inspected crate dependencies and features).
* `python3 /home/jules/self_created_tools/deep_crate_audit.py` (Inspected modules and target `cfg(...)` directives).
* `python3 /home/jules/self_created_tools/crate_details.py` (Gathered crate responsibilities and target dependencies).
* `cargo test -p synveil-core -p synveil-object-store` (Shared Rust unit test suite execution).
* `cd clients/android && ./gradlew test` (Android baseline reference unit test execution).

---

## Linux Validation Performed

1. **Document Completeness Check**: Verified that `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md` contains all 20 required sections.
2. **Taxonomy Adherence Check**: Confirmed that every crate and capability is assigned exactly one primary classification (`DIRECTLY_REUSABLE`, `REUSABLE_WITH_ADAPTER`, `PLATFORM_COUPLED`, `UNSUITABLE_FOR_IOS_REUSE`).
3. **Capability Mapping Coverage**: Confirmed coverage for all 34 capabilities (`CAP-01` through `CAP-34`).
4. **Rust Crate Tests**: Ran `cargo test -p synveil-core -p synveil-object-store` -> 100% PASS (10/10 tests passed).
5. **Android Parity Baseline Unit Tests**: Ran `./gradlew test` in `clients/android` -> 100% PASS (61/61 tasks passed).

---

## macOS CI Validation Plan (Post-Merge / Phase C)

* Xcode compilation, Apple target static library linking (`aarch64-apple-ios`, `aarch64-apple-ios-sim`), Swift SPM FFI bridge unit tests, and iOS Simulator test execution will take place on macOS CI runners when FFI implementation commences in Prompt013/Prompt014.

---

## Tests Added & Run

* **Tests Added**: 0 (Prompt003 is documentation/audit only).
* **Tests Run**:
  * `cargo test -p synveil-core -p synveil-object-store` (Passed)
  * `./gradlew test` in `clients/android` (61/61 tasks passed)

---

## Unrelated CI Check Run Failures

* None caused by Prompt003. Any pre-existing AppImage packaging or desktop CXX-Qt CI failures are classified as `PRE_EXISTING_OR_UNRELATED`.

---

## Limitations

* **Apple Compilation Verification**: Portability of `synveil-core` and `synveil-object-store` was verified via source-level audit (`SOURCE_AUDIT_PORTABLE`); official compilation against Apple iOS target triples (`APPLE_BUILD_NOT_YET_VERIFIED`) will be validated in Prompt014 on macOS CI.

---

## Unresolved Issues / Blockers

* None.

---

## Commit & PR Status

* **Commit Message**: `ios: audit shared core reuse`
* **Branch**: `ios/p003-shared-core-reuse-audit`
* **PR Target**: `ios-app`
* **Merge Status**: Pending review and PR submission to `ios-app`.
