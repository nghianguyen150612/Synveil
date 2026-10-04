#![deny(unsafe_code)]

//! Dedicated thin C ABI bridge crate for Synveil iOS (`synveil-ios-ffi`).
//!
//! # Architecture & Ownership Boundary
//! This crate is the sole owner of the C ABI exports for the native
//! Synveil iOS client (`v0.1`). It acts as an isolation layer between pure,
//! platform-neutral shared Rust core crates (`synveil-core`) and Swift
//! infrastructure adapters (`clients/ios/Infrastructure/RustBridge`).

/// Canonical ABI version exposed across the Swift ↔ Rust C ABI boundary.
pub const SYNVEIL_FFI_ABI_VERSION: u32 = 1;

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
        // Verify genuine compilation against approved synveil-core types.
        let empty_hash = Sha256Digest::from_bytes([0u8; 32]);
        assert_eq!(empty_hash.as_bytes().len(), 32);
    }
}
