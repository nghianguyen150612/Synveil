# Synveil iOS v0.1 — Prompt020 Manifest

## 1. Prompt Overview & Goals
- **Prompt**: Prompt020 — Shared Model Mapping & RustBridgeProtocol
- **Goal**: Establish the Application service boundary via `RustBridgeProtocol`, map shared Rust core primitive models (EnrollmentSecret, DeviceCredentialSecret, LibraryId, NodeId, LogicalName) to C ABI validation exports, preserve SHA256 mapping, maintain strict secrecy/redaction, and complete Phase C Rust↔Swift FFI Foundation closure.
- **Starting Branch**: `ios-app`
- **Starting SHA**: `e6c379f7172da83e469a14af35ada754e0167d12`
- **Work Branch**: `ios/p020-shared-model-mapping-10343842740365488705`
- **Validated Executable Head**: `36d55f00c44ed2a04f0757f508365690bcc7aeb1`
- **PR**: #63 — https://github.com/nghianguyen150612/Synveil/pull/63
- **PR Base**: `ios-app`

## 2. ABI & Export Set Summary
- **ABI Version**: `1` (`SYNVEIL_FFI_ABI_VERSION = 1`)
- **Prior Export Count (P019)**: 5
- **Final Export Count (P020)**: 10
- **Exact Sorted Final C Export Symbols**:
  1. `synveil_ffi_abi_version`
  2. `synveil_ffi_buffer_release`
  3. `synveil_ffi_device_credential_validate`
  4. `synveil_ffi_enrollment_secret_validate`
  5. `synveil_ffi_library_id_validate`
  6. `synveil_ffi_logical_name_validate`
  7. `synveil_ffi_node_id_validate`
  8. `synveil_ffi_sha256_format`
  9. `synveil_ffi_sha256_parse`
  10. `synveil_ffi_validate_abi_version`

## 3. Shared Model Mapping & Validation Semantics
- **EnrollmentSecret**: Validates `sve1_<64 lowercase hex chars>` via `synveil_core::EnrollmentSecret::parse`.
- **DeviceCredentialSecret**: Validates `svd1_<64 lowercase hex chars>` via `synveil_core::DeviceCredentialSecret::parse`.
- **LibraryId**: Validates canonical lowercase hyphenated UUIDv7 string via `synveil_core::LibraryId::parse_str`.
- **NodeId**: Validates canonical lowercase hyphenated UUIDv7 string via `synveil_core::NodeId::parse_str`.
- **LogicalName**: Validates raw UTF-8 logical name (non-empty, <= 1024 UTF-8 bytes, slashes preserved without filesystem normalization) via `synveil_core::LogicalName::new`.
- **Sha256Digest**: Inherited P018 raw 32-byte / canonical `sha256:` string parse and format.
- **Boolean ABI Result**:
  - `0 = false`
  - `1 = true`
  - Normal domain invalid input returns status `SUCCESS` (0) with `*out_valid = 0`.
  - Encoding, null pointer, or panic errors return non-zero FFI status code (`INVALID_ARGUMENT`, `INVALID_UTF8`, `PANIC_ENCOUNTERED`).
  - Output scalar initialized to `0` prior to evaluation.

## 4. Secrecy & Redaction Audit
- No auth tokens or secret credentials are emitted in logs, `Debug` formatting, or test failure messages.
- Swift `String` memory zeroization limitation is acknowledged (Swift strings are heap-allocated and managed by Swift ARC, so guaranteed cryptographic zeroization on drop cannot be guaranteed on the Swift side).

## 5. Rust Validation Helper & Unsafe Audit
- Reusable helper `ffi_bool_validation_boundary` wraps boolean validation calls, checks `out_valid != NULL`, initializes `*out_valid = 0`, and catches unwinding panics.
- Unsafe blocks are narrowly scoped to raw pointer checks and writes, annotated with explicit `// SAFETY:` comments.

## 6. Application Service Protocol Boundary (`RustBridgeProtocol`)
- **Protocol Path**: `clients/ios/Application/Services/RustBridgeProtocol.swift`
- **Protocol Traits**: Pure Swift, `Sendable`, async, uses native Swift types (`Data`, `String`, `Bool`), zero raw FFI imports/pointers.
- **Protocol Conformance**: `RustBridgeAsyncAdapter` conforms to `RustBridgeProtocol`.
- **Dependency Boundary**:
  - `Application`, `Domain`, and `Features` depend on `RustBridgeProtocol` abstraction.
  - Concrete `RustBridgeAsyncAdapter` permitted only in `Infrastructure/RustBridge`, `Tests`, and future `App` composition root.
  - Concrete `RustBridgeAdapter` prohibited outside `Infrastructure/RustBridge` and `Tests`.
- **Static Source Validator**: Enforces these layer boundaries via `clients/ios/Support/validate_ios_sources.py` and its test suite.

## 7. Testing & Verification Proofs
- **Mockability Proof**: `MockRustBridge` in `RustBridgeProtocolTests.swift` proves Application components can be tested without raw FFI linkage.
- **Real Protocol Existential Proof**: `@MainActor` test `testRealProtocolExistentialCallOnSimulatorAndMainActor` in `RustBridgeProtocolTests.swift` invokes real Rust via `let bridge: any RustBridgeProtocol = try await RustBridgeAsyncAdapter()` for all 10 FFI operations.
- **Multibyte UTF-8 Boundary**: Tested 1024 UTF-8 byte boundary on `LogicalName` using multi-byte Unicode/emoji characters.

## 8. Artifact Pipeline & Generated Header
- **C Header**: Regenerated at `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h` using pinned `cbindgen` (0.28.0) and verified via `./scripts/generate-ios-rust-header.sh --check`.
- **Artifact Pipeline**: Updated `scripts/build-ios-rust-artifacts.sh`, `scripts/validate_ios_rust_artifact.py`, and `scripts/tests/test_validate_ios_rust_artifact.py` to enforce the 10-symbol export set and P020 metadata (`ffi_model_mapping = "P020_AUTH_FILE_PRIMITIVES"`, `rust_bridge_protocol = "P020_APPLICATION_SERVICE_BOUNDARY"`).

## 9. Phase C Foundation Closure Status
`P013–P020 FFI FOUNDATION COMPLETE`

Phase C establishes a stable C ABI, Apple static libraries, generated C header, Xcode linking, panic firewall, status model, memory ownership, Swift-owned concurrency, raw FFI isolation, Application protocol boundary, and primitive model mapping.

P021 status: `NOT_STARTED`.


---

## 10. Merge-Review Test Hardening
Before final merge verification, the P020 test matrix was hardened without changing the production ABI or semantics:

- Added explicit non-hex enrollment-token rejection at the FFI boundary.
- Added malformed and nil NodeId rejection coverage.
- Added exact multibyte 1024-byte LogicalName acceptance coverage.
- Added matching Simulator protocol-existential coverage for token wrong-length / uppercase / non-hex cases, LibraryId and NodeId malformed cases, NodeId nil, and exact multibyte 1024-byte LogicalName behavior.
- Corrected XCTest async assertions so every `await` completes before entering `XCTAssert*` autoclosures; this fixes the Xcode test-target compile failure discovered during final merge review.

## 11. Evidence Discipline
This is the final Prompt020 manifest correction. Final-head workflow run IDs, artifact metadata, Simulator totals, and merge evidence will be recorded in the PR discussion after CI completes. No further evidence-only manifest commit should be created.

## 12. PR / Merge State
- **PR**: #63 — https://github.com/nghianguyen150612/Synveil/pull/63
- **PR Base**: `ios-app`
- **Merge State at this manifest commit**: awaiting final-head verification
- **P021 Status**: `NOT_STARTED`
- **Final `ios-app` SHA**: assigned by GitHub at squash merge and recorded in PR merge metadata
