#![forbid(unsafe_code)]

//! Dedicated thin C ABI bridge crate for Synveil iOS (`synveil-ios-ffi`).
//!
//! # Architecture & Ownership Boundary
//! This crate is the sole future owner of the C ABI exports for the native
//! Synveil iOS client (`v0.1`). It acts as an isolation layer between pure,
//! platform-neutral shared Rust core crates (`synveil-core`) and Swift
//! infrastructure adapters (`clients/ios/Infrastructure/RustBridge`).
//!
//! # Prompt 014 Status
//! Prompt 014 establishes workspace membership, staticlib compilation capability,
//! dependency closure auditing, and Apple target compile verification.
//!
//! In accordance with the FFI contract (`docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`),
//! no public `extern "C"` functions, `#[no_mangle]` symbols, cbindgen C headers,
//! or Swift linking exist in Prompt 014. Public FFI exports begin in Prompt 016.

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
