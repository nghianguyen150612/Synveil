# Prompt018 Manifest — Synveil iOS v0.1 FFI Memory Ownership & Buffer Safety

## 1. Metadata & Work Context
- **Prompt Number**: Prompt018
- **Goal**: Implement and prove the Rust↔Swift memory ownership contract for borrowed Swift inputs, Rust-owned output buffers (`SynveilFfiBuffer`), explicit release (`synveil_ffi_buffer_release`), failure-path zeroing, repeated lifecycle safety, and SHA-256 parse/format integration.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `4e7cb65269a6950aea1238ff779e2665c669de19`
- **Work Branch**: `ios/p018-ffi-memory-safety`
- **Authoritative Files Inspected**:
  - `docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`
  - `docs/ios/manifests/PROMPT017_MANIFEST.md`
  - `crates/ios-ffi/src/lib.rs`
  - `crates/ios-ffi/cbindgen.toml`
  - `crates/core/src/hashes.rs`
  - `clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift`
  - `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
  - `scripts/build-ios-rust-artifacts.sh`
  - `scripts/validate_ios_rust_artifact.py`
  - `scripts/generate-ios-rust-header.sh`

---

## 2. FFI Contract & Memory Ownership Details
- **ABI Version**: `1`
- **Status Model**: Inherited from P017 (`u32` stable codes: SUCCESS=0, INVALID_ARGUMENT=1, INVALID_UTF8=2, BUFFER_TOO_SMALL=3, DOMAIN_ERROR=4, INTERNAL_ERROR=5, PANIC_ENCOUNTERED=6, UNSUPPORTED_ABI_VERSION=7).
- **`SynveilFfiBuffer` Rust Layout**:
  ```rust
  #[repr(C)]
  pub struct SynveilFfiBuffer {
      pub data: *mut u8,
      pub len: usize,
      pub capacity: usize,
  }
  ```
- **Generated C Layout**:
  ```c
  typedef struct SynveilFfiBuffer {
      uint8_t *data;
      size_t len;
      size_t capacity;
  } SynveilFfiBuffer;
  ```
- **Empty-Buffer Representation**: Canonical zero (`data == NULL`, `len == 0`, `capacity == 0`).
- **Input Pointer Rules**: Borrowed strictly for call duration; `NULL` + `len > 0` returns `INVALID_ARGUMENT`; Rust never retains or frees foreign inputs.
- **Output Ownership Rules**: Transferred to caller only on `SUCCESS`. Out-parameters initialized to canonical zero upon entry and kept zero on non-success/panic.
- **Release Function Signature**:
  ```c
  uint32_t synveil_ffi_buffer_release(struct SynveilFfiBuffer *buffer);
  ```
- **Release Semantics**: Reconstructs `Vec<u8>` and drops allocation; zeroes caller-visible struct immediately before drop. Releasing canonical empty succeeds safely.
- **Repeated Release Semantics**: Releasing the same struct instance twice succeeds safely because the first release zeroes it.
- **Copied-Live-Buffer Prohibition**: Copying a live buffer struct and releasing both copies is strictly prohibited by ABI contract.
- **Unsafe Code Policy & Audit**: `crates/ios-ffi/src/lib.rs` preserves `#![deny(unsafe_code)]` at crate scope, using narrowly scoped `#[allow(unsafe_code)]` functions with explicit `// SAFETY:` audit comments for raw pointer dereferencing and slice/vector reconstruction.
- **Generic Zeroization Policy**: `GENERIC_BUFFER_RELEASE_DOES_NOT_GUARANTEE_SECRET_ZEROIZATION`. Memory is freed via standard Rust allocator.

---

## 3. Approved C Exports
Exact export set at P018 completion:
```text
synveil_ffi_abi_version
synveil_ffi_buffer_release
synveil_ffi_sha256_format
synveil_ffi_sha256_parse
synveil_ffi_validate_abi_version
```

---

## 4. Capability Selection & Error Mapping
- **Parse Export**: `synveil_ffi_sha256_parse(input_ptr, input_len, out_digest) -> uint32_t`
  - Valid canonical SHA-256 string -> `SUCCESS` (32-byte digest).
  - Null input + len > 0 or null out_digest -> `INVALID_ARGUMENT`.
  - Invalid UTF-8 bytes -> `INVALID_UTF8`.
  - Valid UTF-8 malformed string -> `DOMAIN_ERROR`.
- **Format Export**: `synveil_ffi_sha256_format(digest_ptr, digest_len, out_utf8) -> uint32_t`
  - 32-byte digest -> `SUCCESS` (71-byte canonical string `sha256:<64 hex>`).
  - Wrong digest length != 32 -> `DOMAIN_ERROR`.

---

## 5. Infrastructure & Testing Evidence
- **Rust Unit Tests**: 11 unit tests in `crates/ios-ffi/src/lib.rs` passing (covering canonical empty state, shape validation, round trips, domain errors, failure cleanup zeroing, 10,000-cycle repeated lifecycle, and panic firewall cleanup).
- **Swift Helper**: `consumeRustBuffer` in `RustBridgeAdapter.swift` copies buffer bytes to native Swift `Data` before releasing Rust memory. Raw pointers do not escape `Infrastructure/RustBridge/`.
- **Swift Memory XCTest**: `RustBridgeMemoryTests.swift` registered in `SynveilTests` target. Includes parse/format success, round trips, domain/UTF-8 errors, post-release Swift data ownership verification, and 1,000 Simulator repeated iterations.
- **Header Drift & Validation**: Passed `./scripts/generate-ios-rust-header.sh --check`.
- **Artifact Manifest Memory Status**: `c_abi_export_status = "MEMORY_MODEL_P018"`, `ffi_memory_model = "P018_RUST_OWNED_BUFFER"`.
- **Artifact Validator Tests**: `python3 -m unittest discover -s scripts/tests` passed.
- **Static Validation**: `python3 clients/ios/Support/validate_ios_sources.py` passed.
- **RustBridgeProtocol Status**: `RUST_BRIDGE_PROTOCOL_DEFERRED_TO_P020`.
- **Async Status**: `DEFERRED_P019`.

---

## 6. Files Created / Modified
### Created
- `clients/ios/Tests/SynveilTests/RustBridgeMemoryTests.swift`
- `docs/ios/manifests/PROMPT018_MANIFEST.md`

### Modified
- `crates/ios-ffi/cbindgen.toml`
- `crates/ios-ffi/src/lib.rs`
- `crates/ios-ffi/README.md`
- `clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift`
- `clients/ios/Infrastructure/RustBridge/README.md`
- `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `scripts/build-ios-rust-artifacts.sh`
- `scripts/validate_ios_rust_artifact.py`
- `scripts/tests/test_validate_ios_rust_artifact.py`
- `docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`
