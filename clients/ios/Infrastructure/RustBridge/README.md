# Synveil iOS v0.1 — Rust↔Swift Infrastructure Adapter Documentation

## 1. Overview & Architecture Isolation

The `clients/ios/Infrastructure/RustBridge/` directory contains the sole Swift infrastructure interface wrapping the compiled Rust static library (`libsynveil_ios_ffi.a`) for Synveil iOS (`v0.1`).

```text
┌─────────────────────────────────────────────────────────┐
│                    Swift Application                    │
│             (Domain / Application / SwiftUI)            │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│       Infrastructure / RustBridgeAsyncAdapter           │
│           (parseSHA256, formatSHA256 async)             │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│           Infrastructure / RustBridgeExecutor           │
│        (Task.detached worker, cancellation checks)      │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│           Infrastructure / RustBridgeAdapter            │
│  (consumeRustBuffer, parseSHA256, formatSHA256, etc.)   │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│                   SynveilRustFFI                        │
│       (Raw C ABI Clang Module + Generated C Header)     │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│                 libsynveil_ios_ffi.a                    │
│            (Dedicated Thin C ABI Bridge Crate)          │
└─────────────────────────────────────────────────────────┘
```

### Key Rules
1. **Raw C Module Containment**: Raw C FFI module symbols (`SynveilRustFFI`) are restricted strictly to `Infrastructure/RustBridge/`.
2. **Pointer Confinement**: Raw pointers (`UnsafeMutablePointer`, `SynveilFfiBuffer`) never escape `RustBridgeAdapter`. All upper layers interact purely with native Swift value types (`Data`, `String`).
3. **Copy-Before-Release**: `RustBridgeAdapter` copies Rust output bytes into Swift `Data` before calling `synveil_ffi_buffer_release` to guarantee memory safety.
4. **Swift-Owned Concurrency**: Concurrency is managed entirely by Swift (`SWIFT_OWNS_CONCURRENCY = TRUE`). Rust contains no async runtime (`RUST_ASYNC_RUNTIME = NONE`) and no callback C ABI (`CALLBACK_ABI = NONE`).
5. **MainActor Isolation Safety**: Application and UI callers invoke `RustBridgeAsyncAdapter` asynchronously. Bounded synchronous Rust calls are scheduled off inherited `@MainActor` context via encapsulated `RustBridgeExecutor` (`Task.detached`).

---

## 2. Memory Ownership & Lifecycle Contract

### 2.1 Input Memory (Swift → Rust)
- Swift owns the input memory (e.g., `String.utf8` or `Data`).
- Swift passes borrowed pointers and lengths (`const uint8_t *`, `size_t`) for the synchronous duration of the FFI call.
- Rust never retains, stores, or frees borrowed Swift pointers.

### 2.2 Output Memory (Rust → Swift)
- Rust allocates output buffers using the standard Rust allocator, returning a caller-visible `SynveilFfiBuffer` struct.
- Swift adapter checks status and validates output shape (`data != nil`, `capacity >= len > 0` or canonical zero `data == nil && len == 0 && capacity == 0`).
- Swift adapter copies output bytes into a native Swift `Data` object.
- Swift adapter calls `synveil_ffi_buffer_release(&buffer)` to drop the Rust allocation and zero the local struct.
- Swift MUST NEVER call libc `free()` or Swift deallocators on `buffer.data`.

---

## 3. Approved C ABI Surface & Operations

### Exports (`synveil_ffi_*`)
- `synveil_ffi_abi_version() -> uint32_t`
- `synveil_ffi_validate_abi_version(expected: uint32_t) -> uint32_t`
- `synveil_ffi_buffer_release(buffer: *mut SynveilFfiBuffer) -> uint32_t`
- `synveil_ffi_sha256_parse(input_ptr, input_len, out_digest) -> uint32_t`
- `synveil_ffi_sha256_format(digest_ptr, digest_len, out_utf8) -> uint32_t`

---

## 4. Swift Concurrency & Cancellation Model

### 4.1 Synchronous vs Async Roles
- **`RustBridgeAdapter`**: Low-level synchronous Infrastructure primitive performing C ABI calls and managing memory cleanup.
- **`RustBridgeExecutor`**: Encapsulated enum running synchronous work on a `Task.detached` worker to escape inherited actor context (`@MainActor`). Private worker handles are never exposed.
- **`RustBridgeAsyncAdapter`**: Swift-facing `Sendable` async boundary providing `parseSHA256` and `formatSHA256`.

### 4.2 Cooperative Cancellation Policy
- Cancellation is checked before scheduling worker, inside detached worker before calling FFI, and after worker completion before publishing results.
- **No Mid-FFI Interrupt**: In-flight synchronous Rust calls run to completion, ensuring all Rust-owned buffers are freed normally before `CancellationError` is thrown to the caller.

---

## 5. Testing Evidence

Memory safety and lifecycle contracts are verified via:
1. **Rust Unit Tests** (`crates/ios-ffi/src/lib.rs`):
   - Canonical zero / layout / shape validation
   - Release idempotence on zeroed struct
   - Failure-path buffer zeroing invariant
   - 10,000-cycle allocation/release stress loop
   - Panic firewall cleanup test
2. **Swift XCTests** (`clients/ios/Tests/SynveilTests/RustBridgeMemoryTests.swift`):
   - SHA-256 parse / format / round-trip
   - Domain and invalid UTF-8 error mapping
   - Post-release Swift data access safety
   - 1,000-iteration Simulator repeated lifecycle execution
