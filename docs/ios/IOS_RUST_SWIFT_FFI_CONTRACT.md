# Synveil iOS v0.1 — Rust↔Swift FFI Strategy Contract

## 1. Document Overview & Purpose

This document defines the **authoritative Rust↔Swift FFI Strategy Contract** for the native Synveil iOS client (`v0.1`). It establishes the bridge technology, memory ownership rules, type representations, error and panic handling policies, concurrency boundaries, and prompt responsibilities prior to building any Apple Rust binary artifacts in P014–P020.

---

## 2. Part A — Bridge Technology Selection

### 2.1 Selected Strategy
For Synveil iOS v0.1, the authoritative bridge strategy is:

**Explicit Stable C ABI Implemented by a Dedicated Thin Rust Bridge Crate**

```text
┌─────────────────────────────────────────────────────────┐
│                    Shared Rust Core                     │
│    (synveil-core, synveil-object-store, client-sync)    │
└────────────────────────────┬────────────────────────────┘
                             │ Pure Rust types & logic
                             ▼
┌─────────────────────────────────────────────────────────┐
│        Dedicated Thin Rust iOS FFI Crate                │
│    (crates/ios-ffi / package: synveil-ios-ffi)          │
└────────────────────────────┬────────────────────────────┘
                             │ Stable C ABI (`#[repr(C)]`, `#[no_mangle]`)
                             ▼
┌─────────────────────────────────────────────────────────┐
│                  Stable C ABI Boundary                  │
│       (Pointer + Length, C primitives, status codes)    │
└────────────────────────────┬────────────────────────────┘
                             │ C headers (`CBINDGEN`)
                             ▼
┌─────────────────────────────────────────────────────────┐
│           Swift Infrastructure / RustBridge             │
│    (clients/ios/Infrastructure/RustBridge/Adapter)      │
└────────────────────────────┬────────────────────────────┘
                             │ Swift-facing protocols / values
                             ▼
┌─────────────────────────────────────────────────────────┐
│           Swift Domain / Application Layer              │
└────────────────────────────┬────────────────────────────┘
```

### 2.2 Evaluated Alternative: UniFFI
UniFFI was evaluated for Synveil iOS v0.1 and classified as:

`EVALUATED_NOT_SELECTED_FOR_IOS_V0_1`

* **Evaluation Summary**:
  * *Advantages*: Auto-generates Swift bindings from UDL/proc-macros; reduces manual mapping boilerplate.
  * *Costs & Risks*: Introduces a generator toolchain runtime dependency; complex generated code lifecycle; less direct control over ABI ownership, buffer allocations, and panic firewalls; unnecessary overhead for an intentionally narrow v0.1 FFI surface.
  * *Decision*: Re-evaluatable in post-v0.1 if bridge surface expands significantly, but rejected for v0.1 in favor of exact C ABI control.

### 2.3 Prohibited Interop: Swift C++ Interop / CXX-Qt
The desktop CXX-Qt C++ bridge is desktop-specific. For iOS v0.1:
* Swift C++ Interop, CXX-Qt, Qt runtime, and C++ ABI are **strictly prohibited**.
* C++ interop layers MUST NOT be introduced into `clients/ios/`.

---

## 3. Part B — Dedicated Rust Bridge Crate

### 3.1 Crate Identity & Location
* **Repository Path**: `crates/ios-ffi/`
* **Cargo Package Name**: `synveil-ios-ffi`
* **Target Artifact Type**: `staticlib` (`libsynveil_ios_ffi.a`)

### 3.2 Ownership & Isolation Principles
* `synveil-core` MUST remain platform-neutral and completely unaware of C ABI, Swift, or iOS-specific bridging concerns.
* `synveil-ios-ffi` acts as the single thin adapter crate exposing C ABI exports for iOS.

### 3.3 Allowed vs Prohibited Rust Dependencies

| Allowed Dependencies | Prohibited Dependencies |
|---|---|
| `synveil-core` (domain types, tokens, hashes) | `synveil-client` (desktop daemon orchestration) |
| `synveil-object-store` (pure data types & hashes) | `synveil-desktop` / CXX-Qt / Qt bindings |
| `synveil-client-sync` (pure change evaluation) | `synveil-install-engine` |
| `serde` / `serde_json` (if JSON payload passing) | `synveil-platform` (desktop keyring / pathing) |
| Standard Library (`std::panic::catch_unwind`) | Axum / Server / PostgreSQL crates |
| | Tokio async runtime / Reqwest |

### 3.4 No Apple API Ownership in Rust
The Rust FFI layer MUST NOT attempt to own, manage, or invoke Apple system APIs. The following remain 100% native Swift/iOS responsibilities:
* Keychain Services (`Security.framework`)
* Network Transport (`URLSession`)
* User Interface (`SwiftUI` / `UIKit`)
* File Provider Extension (`NSFileProviderExtension`)
* Camera Roll Access (`PhotoKit`)
* Background Scheduling (`BackgroundTasks`)
* Connectivity Monitoring (`NWPathMonitor`)

---

## 4. Part C — ABI Design Rules

### 4.1 Fixed-Width C Primitives
All primitives crossing the C ABI boundary MUST use explicit fixed-width C types:
* `uint8_t`, `uint32_t`, `uint64_t`
* `int32_t`, `int64_t`
* `size_t` (for byte buffer lengths)

Relying on C `int` or Rust `isize`/`usize` (without C fixed-width equivalence) is forbidden across the ABI.

### 4.2 Prohibited Cross-ABI Layouts
The following Rust-native types MUST NEVER cross the C ABI directly:
* `String`, `&str`, `Vec<T>`, slices (`&[T]`)
* Rust references (`&T`, `&mut T`)
* `Option<T>`, `Result<T, E>`
* Rust enums without explicit `#[repr(C)]` / `#[repr(u32)]`
* Rust trait objects (`dyn Trait`), `Box<T>`
* Rust `Future`, Tokio tasks, Rust `async` functions
* Unannotated Rust `struct` layouts without `#[repr(C)]`
* Uncaught Rust panic payloads

### 4.3 Boolean Representation
Booleans crossing the C ABI use explicit fixed-width integers:
```text
uint8_t
0 = false
1 = true
```
Inputs containing values `> 1` MUST be rejected by Rust bridge validation with an invalid argument error.

### 4.4 ABI-Visible Enums & Status Table
All ABI-visible status returns MUST use explicit `uint32_t` integer representations matching `SynveilFfiStatus`:
```rust
#[repr(u32)]
pub enum SynveilFfiStatus {
    Success = 0,
    InvalidArgument = 1,
    InvalidUtf8 = 2,
    BufferTooSmall = 3,
    DomainError = 4,
    InternalError = 5,
    PanicEncountered = 6,
    UnsupportedAbiVersion = 7,
}
```
Swift code MUST map unknown discriminants to an `.unknown(uint32_t)` error case rather than trapping.

### 4.5 Symbol Namespace
All exported C symbols MUST use the mandatory prefix:

`synveil_ffi_`

Examples:
* `synveil_ffi_abi_version`
* `synveil_ffi_validate_abi_version`
* `synveil_ffi_buffer_release`
* `synveil_ffi_sha256_parse`
* `synveil_ffi_sha256_format`

Generic symbol names (e.g., `free`, `parse`, `hash`, `version`) are strictly prohibited to prevent link-time collisions in Xcode binaries.

### 4.6 ABI Versioning
* **Initial Version**: `1`
* **Version Query Signature**: `synveil_ffi_abi_version() -> uint32_t`
* **Version Validation Signature**: `synveil_ffi_validate_abi_version(expected: uint32_t) -> uint32_t`
* **Policy**:
  * Compatible additive function additions keep ABI version `1`.
  * Breaking parameter, layout, or ownership changes require incrementing the ABI version.
  * Swift adapter MUST query and verify `synveil_ffi_validate_abi_version(1) == 0` upon initialization.

---

## 5. Part D — UTF-8 String Contract

### 5.1 Representation
Strings cross the boundary as **length-delimited UTF-8**:

`pointer (const uint8_t*) + byte length (size_t)`

Null-terminated C strings (`const char*` with trailing `\0`) are NOT used as primary string representations due to truncation risks with embedded NUL bytes.

### 5.2 Input Strings (Swift → Rust)
1. Swift owns the underlying string memory buffer.
2. Swift passes `const uint8_t*` pointer and explicit `size_t` byte length.
3. Rust borrows the buffer strictly for the synchronous duration of the function call.
4. Rust MUST NOT retain the input pointer past function return.
5. Rust validates UTF-8 validity. Invalid UTF-8 returns status `SYNVEIL_FFI_STATUS_INVALID_UTF8` (no lossy conversion or truncation).
6. Embedded NUL bytes are allowed in UTF-8 validation unless prohibited by domain rules.

### 5.3 Output Strings (Rust → Swift)
1. Rust allocates the output string buffer using Rust allocator.
2. Rust returns a raw buffer handle containing pointer + length + capacity (`SynveilFfiBuffer`) to Swift.
3. Swift copies/consumes the string into a native Swift `String`.
4. Swift MUST explicitly call `synveil_ffi_buffer_release()` to free the Rust-allocated output buffer.
5. Swift MUST NEVER call libc `free()` or `deallocate()` on Rust-allocated memory.

---

## 6. Part E — Binary Byte-Buffer Contract

### 6.1 Representation
Binary data (hashes, tokens, serialized model payloads) cross the boundary as raw byte pointers:

`const uint8_t* pointer + size_t length`

### 6.2 Null Pointer & Empty Buffer Rules
* **Valid Empty Input**: `pointer == NULL` AND `length == 0`.
* **Valid Non-Empty Input**: `pointer != NULL` AND `length > 0`.
* **Invalid Input**: `pointer == NULL` AND `length > 0` (returns `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT`).

### 6.3 Output Byte Buffers
Rust-allocated byte output structures MUST use a uniform C struct layout:

```rust
#[repr(C)]
pub struct SynveilFfiBuffer {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}
```

Every `SynveilFfiBuffer` allocated by Rust MUST be freed by calling `synveil_ffi_buffer_release(&mut buffer)`.

---

## 7. Part F — Error Transport Strategy

### 7.1 Status Envelope Pattern
All fallible FFI functions MUST return an explicit status code (`uint32_t`) as the function return value, returning data payloads via out-parameters:

```c
uint32_t synveil_ffi_sha256_parse(
    const uint8_t *input_ptr,
    size_t input_len,
    SynveilFfiBuffer *out_digest
);
```

### 7.2 Error Rules
1. Status `0` (`SYNVEIL_FFI_STATUS_SUCCESS`) indicates success; out-parameters are valid ONLY when status is `0`.
2. Non-zero status indicates failure; out-parameter buffers remain unallocated or canonical zeroed (`NULL`/0/0).
3. No Rust `Result` or Swift exception crosses the ABI directly.
4. Status codes are stable machine-readable integers (`uint32_t`).
5. Diagnostic error strings MUST NEVER contain user credentials, tokens, or raw file content.

---

## 8. Part G — Panic Handling & Firewall Policy

### 8.1 Zero Panic Unwinding Rule
> **INVARIANT**: Absolutely no Rust panic may unwind across the C FFI boundary into foreign Swift stack frames. Unwinding across FFI causes undefined behavior / process termination.

### 8.2 Mandatory Panic Firewall
Every exported FFI entrypoint MUST wrap its body inside `ffi_status_boundary` (`std::panic::catch_unwind`):

```rust
pub fn ffi_status_boundary<F>(operation: F) -> u32
where
    F: FnOnce() -> Result<(), SynveilFfiStatus> + UnwindSafe,
{
    match std::panic::catch_unwind(operation) {
        Ok(Ok(())) => SYNVEIL_FFI_STATUS_SUCCESS,
        Ok(Err(status)) => status as u32,
        Err(_) => SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED,
    }
}
```

Uncaught panics convert cleanly into `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6). Panic message strings are not returned to foreign frames.

---

## 9. Part H — Null Pointer Semantics

| Condition | Semantics / Action |
|---|---|
| `ptr == NULL` AND `len == 0` | Valid empty input where empty buffers are supported. |
| `ptr == NULL` AND `len > 0` | Invalid argument error (`SYNVEIL_FFI_STATUS_INVALID_ARGUMENT`). |
| Out-parameter `ptr == NULL` | Invalid argument error (`SYNVEIL_FFI_STATUS_INVALID_ARGUMENT`). |
| Optional pointer parameter | Permitted ONLY where explicitly documented in function contract. |

---

## 10. Part I — Memory Ownership & Allocation Rules

### 10.1 Memory Rules
1. **Allocator Responsibility**: The allocator that creates memory is solely responsible for exposing its release API.
2. **No Cross-Allocator Free**: Swift MUST NEVER call `free()`, `realloc()`, or Swift deallocators on Rust pointers. Swift calls `synveil_ffi_buffer_release()`.
3. **No Retained Swift Borrow**: Rust MUST NEVER retain or free pointers borrowed from Swift input calls past function return.
4. **Explicit Release Lifecycle**: Output wrappers in Swift MUST invoke release upon completion or deinitialization.
5. **No Double Free**: Swift adapters MUST zero or invalidate local pointer references immediately after calling release functions. `synveil_ffi_buffer_release` zeroes the struct upon release so repeated calls on the zeroed struct are idempotent.
6. **Copied-Live-Buffer Prohibition**: Physically copying a live `SynveilFfiBuffer` struct and releasing both copies is strictly prohibited.
7. **Generic Zeroization Policy**: Memory is freed using the standard Rust allocator (`GENERIC_BUFFER_RELEASE_DOES_NOT_GUARANTEE_SECRET_ZEROIZATION`).

---

## 11. Part J — Opaque Handle Policy

### 11.1 v0.1 Preference: Stateless Calls
Synveil iOS v0.1 strongly prefers **stateless, value-in / value-out synchronous FFI functions** (e.g., UUID generation, SHA-256 hashing, token parsing, feed evaluation).

### 11.2 Handle Contract (If Required Later)
If stateful Rust handles are introduced in future phases:
* Handles MUST be opaque pointers: `typedef struct SynveilOpaqueHandle SynveilOpaqueHandle;`
* Lifecycle MUST be managed via explicit `synveil_ffi_handle_destroy(SynveilOpaqueHandle* handle)`.
* Handle thread-safety MUST be explicitly declared (`Thread-confined` vs `Send-safe`). Handles are NOT assumed thread-safe by default.

---

## 12. Part K — Async & Concurrency Boundary Policy

### 12.1 Synchronous Core Boundary
The v0.1 shared core logic exposed over FFI is synchronous, deterministic domain calculation.

### 12.2 Prohibited Async Exports & Architecture Decision (P019)
For Synveil iOS v0.1:
```text
RUST_ASYNC_RUNTIME = NONE
CALLBACK_ABI = NONE
CALLBACK_ABI_NOT_SELECTED_FOR_IOS_V0_1
SWIFT_OWNS_CONCURRENCY = TRUE
```

The FFI boundary MUST NEVER export:
* Rust `Future` traits, Tokio tasks, async-std, smol, or Reqwest
* Tokio runtime handles or global executor state
* Callback registries, C function-pointer callback APIs, or Swift continuations stored in Rust memory
* Polling APIs, task handles, or cancellation tokens across the FFI boundary

### 12.3 Authoritative Concurrency Model (P019)
```text
Swift UI / Application
        │
        │ async/await
        ▼
Swift async Rust bridge (RustBridgeAsyncAdapter)
        │
        │ Swift-managed worker task (RustBridgeExecutor)
        ▼
non-MainActor cooperative executor (Task.detached worker)
        │
        │ bounded synchronous C call
        ▼
synveil-ios-ffi
        │
        ▼
shared Rust core
```

* **MainActor Isolation Safety**: Application and UI callers invoke `RustBridgeAsyncAdapter` asynchronously. Raw synchronous FFI calls never execute inline on `@MainActor` and are scheduled off inherited actor context via encapsulated `RustBridgeExecutor` (`Task.detached`).
* **Cooperative Cancellation Policy**:
  * `SWIFT_COOPERATIVE_CANCELLATION` with `NO_MID_FFI_INTERRUPT`.
  * Cancellation is checked before worker scheduling, inside the detached task before invoking synchronous FFI, and after worker completion.
  * In-flight synchronous Rust calls run to completion, ensuring all Rust-owned buffers are freed normally before `CancellationError` is published to the caller.

---

## 13. Part L — Threading Contract

1. **Stateless Reentrancy**: All stateless pure FFI functions (`synveil_ffi_generate_uuidv7`, `synveil_ffi_sha256_parse`, `synveil_ffi_sha256_format`) MUST be reentrant and safe to call concurrently from any thread.
2. **No Swift Assumptions**: Rust FFI functions MUST NOT assume Swift thread-local state or GCD dispatch queue identity.
3. **No Global Mutable State**: `synveil-ios-ffi` MUST NOT introduce global mutable static variables or hidden process-wide singletons.

---

## 14. Part M — Swift Adapter Ownership & Boundary

### 14.1 Boundary Isolation
Raw C ABI bindings and generated headers reside **exclusively** under:

`clients/ios/Infrastructure/RustBridge/`

Upper layers (`Features`, `Application`, `Domain`) MUST NEVER import raw FFI module symbols directly.

### 14.2 Protocol Deferral Decision (P013)
* `RustBridgeProtocol` remains **deferred** in P013–P018 (`RUST_BRIDGE_PROTOCOL_DEFERRED_TO_P020`).
* A Swift-facing `RustBridgeProtocol` will be introduced in P020 once callable FFI signatures are compiled and verified. Fake/empty marker protocols are prohibited.

---

## 15. Part N — Bridge API Surface Policy

The FFI API surface MUST remain deliberately narrow.

### 15.1 Candidate Capabilities for P016–P020
* `synveil_ffi_abi_version() -> uint32_t` (P016)
* `synveil_ffi_validate_abi_version(u32) -> uint32_t` (P017)
* `synveil_ffi_buffer_release(buffer) -> uint32_t` (P018)
* `synveil_ffi_sha256_parse(input_ptr, input_len, out_digest) -> uint32_t` (P018)
* `synveil_ffi_sha256_format(digest_ptr, digest_len, out_utf8) -> uint32_t` (P018)
* `generate_uuidv7() -> String` (UUIDv7 string generation)
* `validate_enrollment_token(token) -> Bool` (`sve1_` token format verification)
* `validate_bearer_token(token) -> Bool` (`svd1_` token format verification)
* `evaluate_sync_feed(json) -> ParsedFeed` (Journal change feed parsing & verification)

### 15.2 Explicitly Excluded Capabilities
The following MUST NOT cross FFI:
* Network socket I/O, TLS negotiation, HTTP transports
* SQLite connection handles or SQL queries
* Keychain credential storage
* File system directory scanning or File Provider APIs
* Desktop IPC, process lifecycle, or daemon controllers

---

## 16. Part O — Secrets & Redaction Rules

1. **No Secret Logging**: Bearer tokens (`svd1_`) and private key material MUST NEVER be logged, formatted via `Debug` traits, or exposed in error messages across FFI.
2. **Panic Redaction**: Panic firewalls MUST sanitize panic messages to generic strings before returning across FFI.
3. **Memory Cleanup**: Swift and Rust adapters MUST clear in-memory sensitive byte buffers after operation completion.

---

## 17. Part P — ABI Compatibility & Stability Rules

1. **Explicit C ABI Stability**: Only the explicit `#[repr(C)]` exports provide ABI stability. Native Rust ABI (`extern "Rust"`) is explicitly unstable.
2. **Breaking Changes**: Parameter ordering changes, enum discriminant renumbering, or ownership lifecycle changes constitute breaking ABI changes requiring an ABI version increment.

---

## 18. Part Q — Header & Generation Strategy

* **Header Tooling Recommendation**:

`CBINDGEN_RECOMMENDED_FOR_P014_P015`

* **Policy**:
  * P014/P015/P018 configure `cbindgen` to generate standard C headers (`synveil_ios_ffi.h`) directly from Rust `#[repr(C)]` exports.
  * Generated headers live in `clients/ios/Infrastructure/RustBridge/Generated/`.
  * CI verifies header alignment against Rust source to prevent drift via `./scripts/generate-ios-rust-header.sh --check`.

---

## 19. Part R — Apple Artifact Strategy

* **Artifact Choice**: `Rust static library` (`staticlib` -> `libsynveil_ios_ffi.a`). `cdylib` is prohibited for iOS app embedding.
* **Build Verification Status**:

`APPLE_BUILD_VERIFIED_P014_P015`

* **Target Apple Architecture Triples**:
  * `aarch64-apple-ios` (Physical iPhone device)
  * `aarch64-apple-ios-sim` (Apple Silicon Simulator)
  * `x86_64-apple-ios` (Intel Simulator, if supported by toolchain)

---

## 20. Part S — Contract Tables

### 20.1 Type Mapping Table

| Semantic Type | Rust Internal Type | C ABI Representation | Swift Raw Representation | Ownership | Validation Rules | Implementation Prompt |
|---|---|---|---|---|---|---|
| **Boolean** | `bool` | `uint8_t` | `UInt8` | Value | Must be `0` or `1` | P016 |
| **Integer (32-bit)** | `i32` / `u32` | `int32_t` / `uint32_t` | `Int32` / `UInt32` | Value | Exact bit width | P016 |
| **Integer (64-bit)** | `i64` / `u64` | `int64_t` / `uint64_t` | `Int64` / `UInt64` | Value | Exact bit width | P016 |
| **UTF-8 String Input** | `&str` | `const uint8_t*, size_t` | `UnsafePointer<UInt8>?, Int` | Swift Borrowed | Valid UTF-8, no retain | P016 / P018 |
| **UTF-8 String Output**| `String` | `SynveilFfiBuffer` | `SynveilFfiBuffer` struct | Rust Owned | Released via Rust API | P018 |
| **Byte Buffer Input** | `&[u8]` | `const uint8_t*, size_t` | `UnsafePointer<UInt8>?, Int` | Swift Borrowed | Non-null if len > 0 | P018 |
| **Byte Buffer Output**| `Vec<u8>` | `SynveilFfiBuffer` | `SynveilFfiBuffer` struct | Rust Owned | Released via Rust API | P018 |
| **Status / Error Code**| `SynveilFfiStatus`| `uint32_t` | `UInt32` | Value | Mapped to Swift enum | P017 |
| **Opaque Handle** | `Box<T>` | `SynveilHandle*` | `OpaquePointer?` | Bridge Owned | Non-null pointer | Deferred |

### 20.2 Memory Ownership Table

| Direction | Value Category | Allocator / Owner | Lifetime | Release Responsibility | Null Behavior |
|---|---|---|---|---|---|
| **Swift → Rust** | Input String / Bytes | Swift Runtime | Call Duration | Swift automatic cleanup | NULL + len 0 valid; NULL + len > 0 error |
| **Rust → Swift** | Output Buffer / String | Rust Allocator | Until Swift Consumption | Swift calls `synveil_ffi_buffer_release` | Returned as empty buffer if zero-length |
| **Rust → Swift** | Opaque Handle | Rust Heap | Persistent | Swift calls `synveil_ffi_handle_destroy` | NULL pointer returned on creation failure |

### 20.3 Failure Taxonomy Table

| Failure Source | May Cross ABI? | ABI Representation | Swift Handling | Implementation Prompt |
|---|---|---|---|---|
| **FFI Argument Error** | Yes | `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` | Throws `RustBridgeError.invalidArgument` | P017 / P018 |
| **Invalid UTF-8 Input**| Yes | `SYNVEIL_FFI_STATUS_INVALID_UTF8` | Throws `RustBridgeError.invalidUtf8` | P017 / P018 |
| **Domain Validation** | Yes | `SYNVEIL_FFI_STATUS_DOMAIN_ERROR` | Throws `RustBridgeError.domainError` | P017 / P018 / P020 |
| **Uncaught Rust Panic**| Yes (Firewalled)| `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` | Throws `RustBridgeError.panicEncountered` | P017 / P018 |
| **Memory Allocation** | Yes | `SYNVEIL_FFI_STATUS_INTERNAL_ERROR` | Throws `RustBridgeError.internalError` | P017 / P018 |
| **Unsupported ABI** | Yes | `SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION` | Throws `RustBridgeCompatibilityError.unsupportedABIVersion` | P017 |

### 20.4 Threading Matrix Table

| Operation Category | Execution Model | MainActor Allowed? | Thread-Safe Expectation | Verifying Prompt |
|---|---|---|---|---|
| **ABI Version Query** | Synchronous | Yes (Trivial) | Fully Reentrant | P016 |
| **ABI Validation Query**| Synchronous | Yes (Trivial) | Fully Reentrant | P017 |
| **SHA-256 Parse / Format**| Synchronous | No (Dispatch Off-Main) | Fully Reentrant | P018 |
| **UUIDv7 Generation** | Synchronous | No (Dispatch Off-Main) | Fully Reentrant | P016 / P018 |
| **Token Format Parsing**| Synchronous | No (Dispatch Off-Main) | Fully Reentrant | P020 |
| **Sync Feed Evaluation**| Synchronous | No (Dispatch Off-Main) | Fully Reentrant | P020 |

---

## 21. Part T — Prompt Responsibility Matrix (P014–P020)

```text
P013 (Contract) ──► P014 (Crate/Target) ──► P015 (CI Gate) ──► P016 (First Call)
                                                                       │
P020 (Model Map) ◄── P019 (Async Bound) ◄── P018 (Mem Safety) ◄── P017 (Errors/Panic)
```

| Prompt | Title | Scope & Gate Criteria |
|---|---|---|
| **P013** | **FFI Strategy Contract** | Authoritative contract specification (`IOS_RUST_SWIFT_FFI_CONTRACT.md`). Zero code implementation. |
| **P014** | **Minimal Rust Bridge Crate** | Create `crates/ios-ffi/` (`synveil-ios-ffi`). Verify `cargo check` for Apple targets. No FFI calls required yet. |
| **P015** | **Rust Apple Artifact CI** | GitHub Actions workflow for Apple target artifact compilation (`libsynveil_ios_ffi.a`). |
| **P016** | **First Trivial FFI Call** | Implement `synveil_ffi_abi_version()`. Link static library in Xcode. Execute trivial Swift→Rust call in unit test. |
| **P017** | **FFI Error Model & Panic Firewall**| Implement status envelope, error codes, and `catch_unwind` panic firewall. |
| **P018** | **Memory Ownership & Safety** | Implement `SynveilFfiBuffer` allocation/release functions, SHA-256 parse/format capabilities, Swift `consumeRustBuffer` copy-before-release lifecycle, failure path zeroing, and repeated lifecycle stress tests. |
| **P019** | **Async & Concurrency Boundary** | Establish Swift-owned async concurrency boundary (`RustBridgeExecutor` using encapsulated `Task.detached`, `RustBridgeAsyncAdapter`), MainActor isolation safety, cooperative cancellation without mid-FFI interrupt, multi-thread Rust stress proof, and Simulator concurrency/cancellation test suite. |
| **P020** | **Shared Model Mapping** | Implement token parsing, hash calculation, and sync feed evaluation FFI signatures. Introduce Swift `RustBridgeProtocol`. |
