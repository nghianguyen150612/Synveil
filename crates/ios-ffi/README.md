# Dedicated C ABI Bridge Crate (`synveil-ios-ffi`)

This crate provides the C ABI bridge layer between `synveil-core` and the native Synveil iOS Swift client (`clients/ios/Infrastructure/RustBridge`).

## C ABI Exports (P017)
1. `synveil_ffi_abi_version() -> u32`
2. `synveil_ffi_validate_abi_version(expected_version: u32) -> u32`

## FFI Status Table
All fallible FFI functions return a `u32` status code:
- `0` = `SYNVEIL_FFI_STATUS_SUCCESS`
- `1` = `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT`
- `2` = `SYNVEIL_FFI_STATUS_INVALID_UTF8`
- `3` = `SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL`
- `4` = `SYNVEIL_FFI_STATUS_DOMAIN_ERROR`
- `5` = `SYNVEIL_FFI_STATUS_INTERNAL_ERROR`
- `6` = `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED`
- `7` = `SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION`

## Panic Firewall Policy
All exported fallible C ABI functions wrap execution in `ffi_status_boundary` (`std::panic::catch_unwind`).
No Rust panic unwinds across the C ABI boundary into foreign Swift frames. Panic payloads are caught and converted cleanly to `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) without exposing panic message strings.
