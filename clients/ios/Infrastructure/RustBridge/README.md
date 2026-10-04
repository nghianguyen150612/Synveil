# Infrastructure/RustBridge Adapter

This directory contains the native Swift Infrastructure adapter layer for interacting with `synveil-ios-ffi`.

## Architecture & Boundary
- `Generated/synveil_ios_ffi.h`: Auto-generated C header created by `cbindgen`. Do not hand-edit.
- `Raw/module.modulemap`: Clang module map exposing `SynveilRustFFI`.
- `RustBridgeStatus.swift`: Swift status mapping enum (`RustBridgeStatus`) decoding raw `uint32_t` status codes from Rust.
- `RustBridgeError.swift`: Swift error enum (`RustBridgeError`) representing bridge failure statuses.
- `RustBridgeAdapter.swift`: Sendable struct wrapping C FFI operations, handling version validation, status checks, and error throwing.

## FFI Status Table
| Code | Constant | Meaning |
|---|---|---|
| `0` | `SYNVEIL_FFI_STATUS_SUCCESS` | Operation succeeded |
| `1` | `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` | Input argument validation failed |
| `2` | `SYNVEIL_FFI_STATUS_INVALID_UTF8` | Invalid UTF-8 bytes supplied |
| `3` | `SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL` | Provided output buffer is undersized |
| `4` | `SYNVEIL_FFI_STATUS_DOMAIN_ERROR` | Domain business logic or format validation error |
| `5` | `SYNVEIL_FFI_STATUS_INTERNAL_ERROR` | Allocation or internal Rust error |
| `6` | `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` | Uncaught panic contained by Rust firewall |
| `7` | `SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION` | FFI ABI version mismatch |

## C ABI Exports (P017)
- `synveil_ffi_abi_version()`: Returns canonical u32 ABI version (`1`).
- `synveil_ffi_validate_abi_version(expected_version)`: Validates expected ABI version and returns u32 status.

## Deferred Boundaries
- Memory ownership / buffers (`SynveilFfiBuffer`) -> Deferred to P018
- Async concurrency boundary -> Deferred to P019
- Swift domain model protocols -> Deferred to P020
