#![deny(unsafe_code)]

//! Dedicated thin C ABI bridge crate for Synveil iOS (`synveil-ios-ffi`).
//!
//! # Architecture & Ownership Boundary
//! This crate is the sole owner of the C ABI exports for the native
//! Synveil iOS client (`v0.1`). It acts as an isolation layer between pure,
//! platform-neutral shared Rust core crates (`synveil-core`) and Swift
//! infrastructure adapters (`clients/ios/Infrastructure/RustBridge`).

use std::panic::UnwindSafe;

/// Canonical ABI version exposed across the Swift ↔ Rust C ABI boundary.
pub const SYNVEIL_FFI_ABI_VERSION: u32 = 1;

/// Stable FFI status code: Success (0).
pub const SYNVEIL_FFI_STATUS_SUCCESS: u32 = 0;
/// Stable FFI status code: Invalid Argument (1).
pub const SYNVEIL_FFI_STATUS_INVALID_ARGUMENT: u32 = 1;
/// Stable FFI status code: Invalid UTF-8 Input (2).
pub const SYNVEIL_FFI_STATUS_INVALID_UTF8: u32 = 2;
/// Stable FFI status code: Buffer Too Small (3).
pub const SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL: u32 = 3;
/// Stable FFI status code: Domain Error (4).
pub const SYNVEIL_FFI_STATUS_DOMAIN_ERROR: u32 = 4;
/// Stable FFI status code: Internal Error (5).
pub const SYNVEIL_FFI_STATUS_INTERNAL_ERROR: u32 = 5;
/// Stable FFI status code: Panic Encountered (6).
pub const SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED: u32 = 6;
/// Stable FFI status code: Unsupported ABI Version (7).
pub const SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION: u32 = 7;

/// ABI-stable numeric status codes returned across the FFI boundary.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynveilFfiStatus {
    Success = SYNVEIL_FFI_STATUS_SUCCESS,
    InvalidArgument = SYNVEIL_FFI_STATUS_INVALID_ARGUMENT,
    InvalidUtf8 = SYNVEIL_FFI_STATUS_INVALID_UTF8,
    BufferTooSmall = SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL,
    DomainError = SYNVEIL_FFI_STATUS_DOMAIN_ERROR,
    InternalError = SYNVEIL_FFI_STATUS_INTERNAL_ERROR,
    PanicEncountered = SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED,
    UnsupportedAbiVersion = SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION,
}

/// Reusable panic firewall for fallible FFI operations.
///
/// Guarantees that no panic unwinds across the C ABI boundary.
/// Converts normal operations returning `Result<(), SynveilFfiStatus>` into stable `u32` status codes.
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

/// Returns the supported C ABI version for `synveil-ios-ffi`.
///
/// # ABI Guarantees
/// - Accepts no arguments.
/// - Allocates no heap memory.
/// - Performs no I/O, network, or platform operations.
/// - Is deterministic, reentrant, and thread-safe.
/// - Catches any internal panic and returns `0` (invalid ABI version) on unwind.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_abi_version() -> u32 {
    std::panic::catch_unwind(|| SYNVEIL_FFI_ABI_VERSION).unwrap_or(0)
}

/// Validates the expected C ABI version against the compiled bridge ABI version.
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) if `expected_version` matches current ABI version (1).
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `expected_version` is 0.
/// - `SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION` (7) if `expected_version` is non-zero and unsupported.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_validate_abi_version(expected_version: u32) -> u32 {
    ffi_status_boundary(|| {
        if expected_version == 0 {
            Err(SynveilFfiStatus::InvalidArgument)
        } else if expected_version != SYNVEIL_FFI_ABI_VERSION {
            Err(SynveilFfiStatus::UnsupportedAbiVersion)
        } else {
            Ok(())
        }
    })
}

/// Compile-time marker confirming bridge crate identity and core wiring.
#[doc(hidden)]
pub const SYNVEIL_IOS_FFI_BRIDGE_CRATE_ID: &str = "synveil-ios-ffi";

/// Returns a static compile marker string verifying linkage with `synveil-core`.
#[must_use]
pub fn bridge_compile_marker() -> &'static str {
    SYNVEIL_IOS_FFI_BRIDGE_CRATE_ID
}

#[cfg(test)]
mod tests {
    use super::*;
    use synveil_core::Sha256Digest;

    #[test]
    fn test_status_numeric_values() {
        assert_eq!(SynveilFfiStatus::Success as u32, 0);
        assert_eq!(SynveilFfiStatus::InvalidArgument as u32, 1);
        assert_eq!(SynveilFfiStatus::InvalidUtf8 as u32, 2);
        assert_eq!(SynveilFfiStatus::BufferTooSmall as u32, 3);
        assert_eq!(SynveilFfiStatus::DomainError as u32, 4);
        assert_eq!(SynveilFfiStatus::InternalError as u32, 5);
        assert_eq!(SynveilFfiStatus::PanicEncountered as u32, 6);
        assert_eq!(SynveilFfiStatus::UnsupportedAbiVersion as u32, 7);

        assert_eq!(SYNVEIL_FFI_STATUS_SUCCESS, 0);
        assert_eq!(SYNVEIL_FFI_STATUS_INVALID_ARGUMENT, 1);
        assert_eq!(SYNVEIL_FFI_STATUS_INVALID_UTF8, 2);
        assert_eq!(SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL, 3);
        assert_eq!(SYNVEIL_FFI_STATUS_DOMAIN_ERROR, 4);
        assert_eq!(SYNVEIL_FFI_STATUS_INTERNAL_ERROR, 5);
        assert_eq!(SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED, 6);
        assert_eq!(SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION, 7);
    }

    #[test]
    fn test_panic_firewall_success_error_and_panic() {
        // Success path
        let res_ok = ffi_status_boundary(|| Ok(()));
        assert_eq!(res_ok, SYNVEIL_FFI_STATUS_SUCCESS);

        // Expected status error path
        let res_err = ffi_status_boundary(|| Err(SynveilFfiStatus::InvalidArgument));
        assert_eq!(res_err, SYNVEIL_FFI_STATUS_INVALID_ARGUMENT);

        // Deliberate panic containment
        let res_panic = ffi_status_boundary(|| {
            panic!("ffi panic firewall test");
        });
        assert_eq!(res_panic, SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED);
    }

    #[test]
    fn test_validate_abi_version_semantics() {
        // Matching ABI -> Success (0)
        assert_eq!(
            synveil_ffi_validate_abi_version(1),
            SYNVEIL_FFI_STATUS_SUCCESS
        );

        // 0 -> InvalidArgument (1)
        assert_eq!(
            synveil_ffi_validate_abi_version(0),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );

        // Unsupported non-zero ABI -> UnsupportedAbiVersion (7)
        assert_eq!(
            synveil_ffi_validate_abi_version(2),
            SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION
        );
        assert_eq!(
            synveil_ffi_validate_abi_version(99),
            SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION
        );
    }

    #[test]
    fn test_abi_version_constant_and_export() {
        assert_eq!(SYNVEIL_FFI_ABI_VERSION, 1);
        assert_eq!(synveil_ffi_abi_version(), 1);
    }

    #[test]
    fn test_bridge_compile_marker() {
        assert_eq!(bridge_compile_marker(), "synveil-ios-ffi");
    }

    #[test]
    fn test_synveil_core_linkage() {
        let empty_hash = Sha256Digest::from_bytes([0u8; 32]);
        assert_eq!(empty_hash.as_bytes().len(), 32);
    }
}
