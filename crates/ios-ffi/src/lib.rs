#![deny(unsafe_code)]

//! Dedicated thin C ABI bridge crate for Synveil iOS (`synveil-ios-ffi`).
//!
//! # Architecture & Ownership Boundary
//! This crate is the sole owner of the C ABI exports for the native
//! Synveil iOS client (`v0.1`). It acts as an isolation layer between pure,
//! platform-neutral shared Rust core crates (`synveil-core`) and Swift
//! infrastructure adapters (`clients/ios/Infrastructure/RustBridge`).

use std::panic::UnwindSafe;
use synveil_core::{
    DeviceCredentialSecret, EnrollmentSecret, LibraryId, LogicalName, NodeId, Sha256Digest,
};

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

/// Generic C-compatible heap buffer representation owned by Rust.
///
/// # ABI Layout
/// Across C/Swift boundaries, this struct maps to:
/// ```c
/// typedef struct SynveilFfiBuffer {
///     uint8_t *data;
///     size_t len;
///     size_t capacity;
/// } SynveilFfiBuffer;
/// ```
///
/// # Memory Invariants
/// - Canonical empty state: `data == NULL`, `len == 0`, `capacity == 0`.
/// - Valid allocated state: `data != NULL`, `len > 0`, `capacity >= len`.
/// - Releasing a buffer via `synveil_ffi_buffer_release` zeroes all fields back to canonical empty state.
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub struct SynveilFfiBuffer {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl SynveilFfiBuffer {
    /// Canonical empty buffer state representation (`NULL` pointer, 0 length, 0 capacity).
    pub const CANONICAL_EMPTY: Self = Self {
        data: std::ptr::null_mut(),
        len: 0,
        capacity: 0,
    };

    /// Returns `true` if this buffer matches the canonical empty state.
    #[must_use]
    pub fn is_canonical_empty(&self) -> bool {
        self.data.is_null() && self.len == 0 && self.capacity == 0
    }

    /// Converts a Rust `Vec<u8>` into a `SynveilFfiBuffer` without unnecessary copying.
    /// Empty vectors are directly canonicalized to `{ data: NULL, len: 0, capacity: 0 }`.
    #[must_use]
    pub fn from_vec(vec: Vec<u8>) -> Self {
        if vec.is_empty() {
            Self::CANONICAL_EMPTY
        } else {
            let mut md = std::mem::ManuallyDrop::new(vec);
            Self {
                data: md.as_mut_ptr(),
                len: md.len(),
                capacity: md.capacity(),
            }
        }
    }
}

/// Safely initializes an out-parameter buffer pointer to canonical zero (`NULL`/0/0).
///
/// Returns `Err(SynveilFfiStatus::InvalidArgument)` if `out_buffer` is null.
#[allow(unsafe_code)]
unsafe fn initialize_out_buffer(out_buffer: *mut SynveilFfiBuffer) -> Result<(), SynveilFfiStatus> {
    if out_buffer.is_null() {
        return Err(SynveilFfiStatus::InvalidArgument);
    }
    // SAFETY: out_buffer was verified non-null above.
    unsafe {
        out_buffer.write(SynveilFfiBuffer::CANONICAL_EMPTY);
    }
    Ok(())
}

/// Converts a raw input pointer and length into a borrowed Rust slice for synchronous execution.
///
/// # Validation Rules
/// - `ptr == NULL` and `len == 0` -> Ok(`&[]`)
/// - `ptr == NULL` and `len > 0` -> Err(`InvalidArgument`)
/// - `ptr != NULL` and `len >= 0` -> Ok(`&[u8]`)
#[allow(unsafe_code)]
unsafe fn borrowed_slice<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], SynveilFfiStatus> {
    if ptr.is_null() {
        if len == 0 {
            Ok(&[])
        } else {
            Err(SynveilFfiStatus::InvalidArgument)
        }
    } else if len == 0 {
        Ok(&[])
    } else {
        // SAFETY: ptr was verified non-null, len > 0, and memory is borrowed strictly for function duration.
        Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
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

/// Helper that wraps a boolean validation operation across the C ABI boundary.
///
/// # Invariants & Safety
/// - Verifies `out_valid` is non-null before dereferencing. Returns `INVALID_ARGUMENT` if null.
/// - Initializes `*out_valid = 0` prior to evaluating `operation`.
/// - Executes `operation` inside `std::panic::catch_unwind`.
/// - On success (`Ok(valid)`): writes `1` if `valid` is true, `0` if `valid` is false, and returns `SUCCESS`.
/// - On status error (`Err(status)`): `*out_valid` remains `0` and returns status code.
/// - On panic: `*out_valid` remains `0` and returns `PANIC_ENCOUNTERED`.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn ffi_bool_validation_boundary<F>(out_valid: *mut u8, operation: F) -> u32
where
    F: FnOnce() -> Result<bool, SynveilFfiStatus> + UnwindSafe,
{
    if out_valid.is_null() {
        return SYNVEIL_FFI_STATUS_INVALID_ARGUMENT;
    }

    // SAFETY: out_valid was checked non-null above.
    unsafe {
        out_valid.write(0);
    }

    match std::panic::catch_unwind(operation) {
        Ok(Ok(is_valid)) => {
            // SAFETY: out_valid was checked non-null above.
            unsafe {
                out_valid.write(if is_valid { 1 } else { 0 });
            }
            SYNVEIL_FFI_STATUS_SUCCESS
        }
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

/// Releases memory owned by a `SynveilFfiBuffer` previously produced by `synveil-ios-ffi`.
///
/// # Safety & Preconditions
/// - `buffer` must be a non-null pointer to a `SynveilFfiBuffer`.
/// - `buffer` must be either canonical empty (`NULL`/0/0) or an unmodified buffer created by `synveil-ios-ffi`.
/// - Releasing a canonical empty buffer succeeds and does nothing.
/// - Releasing a buffer immediately zeroes the pointed struct to canonical empty state before dropping memory.
/// - Copying a live `SynveilFfiBuffer` and releasing both copies is prohibited.
///
/// # Generic Zeroization Policy
/// `synveil_ffi_buffer_release` frees buffer memory via the default Rust allocator.
/// It does not guarantee cryptographic zeroization of release memory contents (`GENERIC_BUFFER_RELEASE_DOES_NOT_GUARANTEE_SECRET_ZEROIZATION`).
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_buffer_release(buffer: *mut SynveilFfiBuffer) -> u32 {
    ffi_status_boundary(|| {
        if buffer.is_null() {
            return Err(SynveilFfiStatus::InvalidArgument);
        }

        // SAFETY: buffer is checked non-null. Read fields individually so the ownership-bearing
        // struct is not made Copy/Clone in Rust.
        let (data, len, capacity) = unsafe { ((*buffer).data, (*buffer).len, (*buffer).capacity) };

        if data.is_null() && len == 0 && capacity == 0 {
            return Ok(());
        }

        // Validate the only supported live-buffer shape: non-null data, len > 0,
        // and capacity >= len. Non-null zero-length shapes are not produced by this crate.
        if data.is_null() || len == 0 || capacity == 0 || len > capacity {
            return Err(SynveilFfiStatus::InvalidArgument);
        }

        // Zero out caller-visible struct BEFORE dropping allocation to invalidate pointer on caller side.
        // SAFETY: buffer pointer was checked non-null and valid.
        unsafe {
            buffer.write(SynveilFfiBuffer::CANONICAL_EMPTY);
        }

        // Reconstruct Vec and drop it using default Rust allocator.
        // SAFETY: data was verified non-null, len > 0, capacity >= len, and this function's ABI
        // precondition requires the buffer to be an unmodified value produced by this crate.
        let _vec = unsafe { Vec::from_raw_parts(data, len, capacity) };
        // _vec is dropped at end of scope.

        Ok(())
    })
}

/// Validates whether a borrowed string token matches the canonical enrollment secret format (`sve1_<64 lowercase hex>`).
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) with `*out_valid = 1` if valid, or `*out_valid = 0` if domain-invalid.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_valid` is null or `input_ptr` is null with `input_len > 0`.
/// - `SYNVEIL_FFI_STATUS_INVALID_UTF8` (2) if `input_ptr` contains invalid UTF-8 bytes.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_enrollment_secret_validate(
    input_ptr: *const u8,
    input_len: usize,
    out_valid: *mut u8,
) -> u32 {
    ffi_bool_validation_boundary(out_valid, || {
        // SAFETY: input_ptr and input_len are validated by borrowed_slice.
        let input_bytes = unsafe { borrowed_slice(input_ptr, input_len)? };
        let input_str =
            std::str::from_utf8(input_bytes).map_err(|_| SynveilFfiStatus::InvalidUtf8)?;
        Ok(EnrollmentSecret::parse(input_str).is_ok())
    })
}

/// Validates whether a borrowed string token matches the canonical device credential format (`svd1_<64 lowercase hex>`).
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) with `*out_valid = 1` if valid, or `*out_valid = 0` if domain-invalid.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_valid` is null or `input_ptr` is null with `input_len > 0`.
/// - `SYNVEIL_FFI_STATUS_INVALID_UTF8` (2) if `input_ptr` contains invalid UTF-8 bytes.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_device_credential_validate(
    input_ptr: *const u8,
    input_len: usize,
    out_valid: *mut u8,
) -> u32 {
    ffi_bool_validation_boundary(out_valid, || {
        // SAFETY: input_ptr and input_len are validated by borrowed_slice.
        let input_bytes = unsafe { borrowed_slice(input_ptr, input_len)? };
        let input_str =
            std::str::from_utf8(input_bytes).map_err(|_| SynveilFfiStatus::InvalidUtf8)?;
        Ok(DeviceCredentialSecret::parse(input_str).is_ok())
    })
}

/// Validates whether a borrowed string representation matches a canonical lowercase hyphenated UUIDv7 LibraryId.
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) with `*out_valid = 1` if valid, or `*out_valid = 0` if domain-invalid.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_valid` is null or `input_ptr` is null with `input_len > 0`.
/// - `SYNVEIL_FFI_STATUS_INVALID_UTF8` (2) if `input_ptr` contains invalid UTF-8 bytes.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_library_id_validate(
    input_ptr: *const u8,
    input_len: usize,
    out_valid: *mut u8,
) -> u32 {
    ffi_bool_validation_boundary(out_valid, || {
        // SAFETY: input_ptr and input_len are validated by borrowed_slice.
        let input_bytes = unsafe { borrowed_slice(input_ptr, input_len)? };
        let input_str =
            std::str::from_utf8(input_bytes).map_err(|_| SynveilFfiStatus::InvalidUtf8)?;
        Ok(LibraryId::parse_str(input_str).is_ok())
    })
}

/// Validates whether a borrowed string representation matches a canonical lowercase hyphenated UUIDv7 NodeId.
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) with `*out_valid = 1` if valid, or `*out_valid = 0` if domain-invalid.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_valid` is null or `input_ptr` is null with `input_len > 0`.
/// - `SYNVEIL_FFI_STATUS_INVALID_UTF8` (2) if `input_ptr` contains invalid UTF-8 bytes.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_node_id_validate(
    input_ptr: *const u8,
    input_len: usize,
    out_valid: *mut u8,
) -> u32 {
    ffi_bool_validation_boundary(out_valid, || {
        // SAFETY: input_ptr and input_len are validated by borrowed_slice.
        let input_bytes = unsafe { borrowed_slice(input_ptr, input_len)? };
        let input_str =
            std::str::from_utf8(input_bytes).map_err(|_| SynveilFfiStatus::InvalidUtf8)?;
        Ok(NodeId::parse_str(input_str).is_ok())
    })
}

/// Validates whether a borrowed UTF-8 string is a valid non-empty LogicalName (<= 1024 UTF-8 bytes).
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) with `*out_valid = 1` if valid, or `*out_valid = 0` if domain-invalid.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_valid` is null or `input_ptr` is null with `input_len > 0`.
/// - `SYNVEIL_FFI_STATUS_INVALID_UTF8` (2) if `input_ptr` contains invalid UTF-8 bytes.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_logical_name_validate(
    input_ptr: *const u8,
    input_len: usize,
    out_valid: *mut u8,
) -> u32 {
    ffi_bool_validation_boundary(out_valid, || {
        // SAFETY: input_ptr and input_len are validated by borrowed_slice.
        let input_bytes = unsafe { borrowed_slice(input_ptr, input_len)? };
        let input_str =
            std::str::from_utf8(input_bytes).map_err(|_| SynveilFfiStatus::InvalidUtf8)?;
        Ok(LogicalName::new(input_str).is_ok())
    })
}

/// Parses a canonical SHA-256 string (e.g. `sha256:<64 hex chars>`) from borrowed UTF-8 input bytes
/// and outputs a Rust-owned 32-byte raw digest buffer in `out_digest`.
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) on valid parse, writing a 32-byte digest to `out_digest`.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_digest` is null or `input_ptr` is null with `input_len > 0`.
/// - `SYNVEIL_FFI_STATUS_INVALID_UTF8` (2) if `input_ptr` contains invalid UTF-8 bytes.
/// - `SYNVEIL_FFI_STATUS_DOMAIN_ERROR` (4) if input is valid UTF-8 but invalid Synveil SHA-256 representation.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_sha256_parse(
    input_ptr: *const u8,
    input_len: usize,
    out_digest: *mut SynveilFfiBuffer,
) -> u32 {
    ffi_status_boundary(|| {
        // SAFETY: out_digest is checked for null inside initialize_out_buffer.
        unsafe { initialize_out_buffer(out_digest)? };

        // SAFETY: input_ptr and input_len are validated by borrowed_slice.
        let input_bytes = unsafe { borrowed_slice(input_ptr, input_len)? };

        let input_str =
            std::str::from_utf8(input_bytes).map_err(|_| SynveilFfiStatus::InvalidUtf8)?;

        let digest = Sha256Digest::parse(input_str).map_err(|_| SynveilFfiStatus::DomainError)?;

        let buffer = SynveilFfiBuffer::from_vec(digest.as_bytes().to_vec());

        // SAFETY: out_digest was verified non-null and initialized.
        unsafe {
            out_digest.write(buffer);
        }

        Ok(())
    })
}

/// Formats a 32-byte raw SHA-256 digest into a canonical `sha256:<64 hex chars>` UTF-8 string buffer in `out_utf8`.
///
/// # Returns
/// - `SYNVEIL_FFI_STATUS_SUCCESS` (0) on valid 32-byte digest input, writing a 71-byte UTF-8 buffer to `out_utf8`.
/// - `SYNVEIL_FFI_STATUS_INVALID_ARGUMENT` (1) if `out_utf8` is null or `digest_ptr` is null with `digest_len > 0`.
/// - `SYNVEIL_FFI_STATUS_DOMAIN_ERROR` (4) if `digest_len != 32`.
/// - `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED` (6) if an internal panic occurs.
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn synveil_ffi_sha256_format(
    digest_ptr: *const u8,
    digest_len: usize,
    out_utf8: *mut SynveilFfiBuffer,
) -> u32 {
    ffi_status_boundary(|| {
        // SAFETY: out_utf8 is checked for null inside initialize_out_buffer.
        unsafe { initialize_out_buffer(out_utf8)? };

        // SAFETY: digest_ptr and digest_len are validated by borrowed_slice.
        let digest_bytes = unsafe { borrowed_slice(digest_ptr, digest_len)? };

        if digest_bytes.len() != 32 {
            return Err(SynveilFfiStatus::DomainError);
        }

        let digest =
            Sha256Digest::try_from(digest_bytes).map_err(|_| SynveilFfiStatus::DomainError)?;

        let formatted_str = digest.to_string();
        let buffer = SynveilFfiBuffer::from_vec(formatted_str.into_bytes());

        // SAFETY: out_utf8 was verified non-null and initialized.
        unsafe {
            out_utf8.write(buffer);
        }

        Ok(())
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
        let res_ok = ffi_status_boundary(|| Ok(()));
        assert_eq!(res_ok, SYNVEIL_FFI_STATUS_SUCCESS);

        let res_err = ffi_status_boundary(|| Err(SynveilFfiStatus::InvalidArgument));
        assert_eq!(res_err, SYNVEIL_FFI_STATUS_INVALID_ARGUMENT);

        let res_panic = ffi_status_boundary(|| {
            panic!("ffi panic firewall test");
        });
        assert_eq!(res_panic, SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED);
    }

    #[test]
    fn test_validate_abi_version_semantics() {
        assert_eq!(
            synveil_ffi_validate_abi_version(1),
            SYNVEIL_FFI_STATUS_SUCCESS
        );
        assert_eq!(
            synveil_ffi_validate_abi_version(0),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            synveil_ffi_validate_abi_version(2),
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

    #[test]
    #[allow(unsafe_code)]
    fn test_ffi_bool_validation_boundary_semantics() {
        let mut valid_out = 0xFFu8;

        // 1. Null out pointer returns INVALID_ARGUMENT
        let status = ffi_bool_validation_boundary(std::ptr::null_mut(), || Ok(true));
        assert_eq!(status, SYNVEIL_FFI_STATUS_INVALID_ARGUMENT);

        // 2. Success true writes 1
        let status = ffi_bool_validation_boundary(&mut valid_out, || Ok(true));
        assert_eq!(status, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid_out, 1);

        // 3. Success false writes 0
        let status = ffi_bool_validation_boundary(&mut valid_out, || Ok(false));
        assert_eq!(status, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid_out, 0);

        // 4. Status error leaves valid_out as 0
        valid_out = 0xFF;
        let status =
            ffi_bool_validation_boundary(&mut valid_out, || Err(SynveilFfiStatus::InvalidUtf8));
        assert_eq!(status, SYNVEIL_FFI_STATUS_INVALID_UTF8);
        assert_eq!(valid_out, 0);

        // 5. Panic leaves valid_out as 0 and returns PANIC_ENCOUNTERED
        valid_out = 0xFF;
        let status =
            ffi_bool_validation_boundary(&mut valid_out, || panic!("bool boundary test panic"));
        assert_eq!(status, SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED);
        assert_eq!(valid_out, 0);
    }

    #[test]
    fn test_enrollment_and_device_credential_validators() {
        let valid_sve = format!("sve1_{}", "a1".repeat(32));
        let valid_svd = format!("svd1_{}", "b2".repeat(32));
        let mut valid = 0xFFu8;

        // Valid enrollment token
        let res =
            synveil_ffi_enrollment_secret_validate(valid_sve.as_ptr(), valid_sve.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Valid device credential token passed to enrollment validator -> valid = 0
        let res =
            synveil_ffi_enrollment_secret_validate(valid_svd.as_ptr(), valid_svd.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Valid device credential token
        let res =
            synveil_ffi_device_credential_validate(valid_svd.as_ptr(), valid_svd.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Valid enrollment token passed to device credential validator -> valid = 0
        let res =
            synveil_ffi_device_credential_validate(valid_sve.as_ptr(), valid_sve.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Invalid forms: uppercase, wrong length, non-hex
        let uppercase = valid_sve.to_ascii_uppercase();
        let res =
            synveil_ffi_enrollment_secret_validate(uppercase.as_ptr(), uppercase.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        let wrong_len = format!("{valid_sve}a");
        let res =
            synveil_ffi_enrollment_secret_validate(wrong_len.as_ptr(), wrong_len.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Invalid UTF-8
        let bad_utf8 = [0xFF, 0xFE, 0xFD];
        let res =
            synveil_ffi_enrollment_secret_validate(bad_utf8.as_ptr(), bad_utf8.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_INVALID_UTF8);
        assert_eq!(valid, 0);

        // Null out pointer
        let res = synveil_ffi_enrollment_secret_validate(
            valid_sve.as_ptr(),
            valid_sve.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(res, SYNVEIL_FFI_STATUS_INVALID_ARGUMENT);
    }

    #[test]
    fn test_library_and_node_id_validators() {
        let valid_v7 = LibraryId::new().to_string();
        let mut valid = 0xFFu8;

        // Valid LibraryId
        let res = synveil_ffi_library_id_validate(valid_v7.as_ptr(), valid_v7.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Valid NodeId
        let res = synveil_ffi_node_id_validate(valid_v7.as_ptr(), valid_v7.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Uppercase UUID -> valid = 0
        let uppercase = valid_v7.to_ascii_uppercase();
        let res = synveil_ffi_library_id_validate(uppercase.as_ptr(), uppercase.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Unhyphenated UUID -> valid = 0
        let unhyphenated = valid_v7.replace('-', "");
        let res =
            synveil_ffi_library_id_validate(unhyphenated.as_ptr(), unhyphenated.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // UUIDv4 (not v7) -> valid = 0
        let v4_str = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
        let res = synveil_ffi_library_id_validate(v4_str.as_ptr(), v4_str.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Malformed text -> valid = 0
        let malformed = "not-a-uuid";
        let res = synveil_ffi_library_id_validate(malformed.as_ptr(), malformed.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Null out pointer -> INVALID_ARGUMENT
        let res = synveil_ffi_library_id_validate(
            valid_v7.as_ptr(),
            valid_v7.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(res, SYNVEIL_FFI_STATUS_INVALID_ARGUMENT);
    }

    #[test]
    fn test_logical_name_validator() {
        let mut valid = 0xFFu8;

        // Valid name
        let name = "hello.txt";
        let res = synveil_ffi_logical_name_validate(name.as_ptr(), name.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Slash preserving ("A/B")
        let slash_name = "A/B";
        let res =
            synveil_ffi_logical_name_validate(slash_name.as_ptr(), slash_name.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Multibyte Unicode name
        let unicode_name = "synveil_🚀_document.pdf";
        let res = synveil_ffi_logical_name_validate(
            unicode_name.as_ptr(),
            unicode_name.len(),
            &mut valid,
        );
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // Empty string -> valid = 0
        let empty = "";
        let res = synveil_ffi_logical_name_validate(empty.as_ptr(), empty.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Exactly 1024 UTF-8 bytes -> valid = 1
        let max_bytes = "a".repeat(1024);
        let res =
            synveil_ffi_logical_name_validate(max_bytes.as_ptr(), max_bytes.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 1);

        // 1025 UTF-8 bytes -> valid = 0
        let max_plus_one = "a".repeat(1025);
        let res = synveil_ffi_logical_name_validate(
            max_plus_one.as_ptr(),
            max_plus_one.len(),
            &mut valid,
        );
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Multibyte boundary test (1023 'a's + 4-byte emoji = 1027 bytes -> valid = 0)
        let mut unicode_over = "a".repeat(1023);
        unicode_over.push('🚀'); // '🚀' is 4 UTF-8 bytes (total 1027 bytes)
        assert_eq!(unicode_over.len(), 1027);
        let res = synveil_ffi_logical_name_validate(
            unicode_over.as_ptr(),
            unicode_over.len(),
            &mut valid,
        );
        assert_eq!(res, SYNVEIL_FFI_STATUS_SUCCESS);
        assert_eq!(valid, 0);

        // Invalid UTF-8
        let bad_utf8 = [0xFF, 0xFE];
        let res = synveil_ffi_logical_name_validate(bad_utf8.as_ptr(), bad_utf8.len(), &mut valid);
        assert_eq!(res, SYNVEIL_FFI_STATUS_INVALID_UTF8);
        assert_eq!(valid, 0);
    }

    #[test]
    #[allow(unsafe_code)]
    fn test_buffer_layout_and_lifecycle() {
        // 1. Canonical empty buffer is null/0/0
        let empty = SynveilFfiBuffer::CANONICAL_EMPTY;
        assert!(empty.is_canonical_empty());
        assert!(empty.data.is_null());
        assert_eq!(empty.len, 0);
        assert_eq!(empty.capacity, 0);

        // Empty vec converts directly to canonical empty
        let vec_empty = Vec::<u8>::new();
        let buf_empty = SynveilFfiBuffer::from_vec(vec_empty);
        assert!(buf_empty.is_canonical_empty());

        // 2. Allocated non-empty buffer has non-null data, expected len, capacity >= len
        let vec = vec![1, 2, 3, 4, 5];
        let mut buf = SynveilFfiBuffer::from_vec(vec);
        assert!(!buf.is_canonical_empty());
        assert!(!buf.data.is_null());
        assert_eq!(buf.len, 5);
        assert!(buf.capacity >= 5);

        // 3. Successful release zeroes all fields
        let release_status = synveil_ffi_buffer_release(&mut buf);
        assert_eq!(release_status, SYNVEIL_FFI_STATUS_SUCCESS);
        assert!(buf.is_canonical_empty());

        // 4. Releasing canonical zero buffer succeeds
        let mut empty_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
        assert_eq!(
            synveil_ffi_buffer_release(&mut empty_buf),
            SYNVEIL_FFI_STATUS_SUCCESS
        );
        assert!(empty_buf.is_canonical_empty());

        // 5. Releasing same struct twice succeeds safely
        let vec2 = vec![10, 20, 30];
        let mut buf2 = SynveilFfiBuffer::from_vec(vec2);
        assert_eq!(
            synveil_ffi_buffer_release(&mut buf2),
            SYNVEIL_FFI_STATUS_SUCCESS
        );
        assert_eq!(
            synveil_ffi_buffer_release(&mut buf2),
            SYNVEIL_FFI_STATUS_SUCCESS
        );

        // 6. Null SynveilFfiBuffer* -> INVALID_ARGUMENT
        assert_eq!(
            synveil_ffi_buffer_release(std::ptr::null_mut()),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );

        // 7. Obvious invalid shapes are rejected
        let mut malformed_len = SynveilFfiBuffer {
            data: std::ptr::NonNull::dangling().as_ptr(),
            len: 10,
            capacity: 5, // len > capacity
        };
        assert_eq!(
            synveil_ffi_buffer_release(&mut malformed_len),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );

        let mut malformed_null_data = SynveilFfiBuffer {
            data: std::ptr::null_mut(),
            len: 5,
            capacity: 5, // data == NULL with len > 0
        };
        assert_eq!(
            synveil_ffi_buffer_release(&mut malformed_null_data),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );

        let mut malformed_zero_len = SynveilFfiBuffer {
            data: std::ptr::NonNull::dangling().as_ptr(),
            len: 0,
            capacity: 8, // live buffers must never be non-null with zero length
        };
        assert_eq!(
            synveil_ffi_buffer_release(&mut malformed_zero_len),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );
    }

    #[test]
    #[allow(unsafe_code)]
    fn test_producer_success_and_round_trip() {
        let hex_64 = "ab".repeat(32);
        let canonical_str = format!("sha256:{hex_64}");

        // Parse canonical SHA-256 string
        let mut digest_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
        let parse_status =
            synveil_ffi_sha256_parse(canonical_str.as_ptr(), canonical_str.len(), &mut digest_buf);
        assert_eq!(parse_status, SYNVEIL_FFI_STATUS_SUCCESS);
        assert!(!digest_buf.data.is_null());
        assert_eq!(digest_buf.len, 32);

        // Copy digest bytes safely before release
        // SAFETY: digest_buf was verified non-null and valid.
        let digest_slice = unsafe { std::slice::from_raw_parts(digest_buf.data, digest_buf.len) };
        assert_eq!(digest_slice, &[0xab; 32]);

        // Format raw digest bytes
        let mut formatted_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
        let format_status =
            synveil_ffi_sha256_format(digest_buf.data, digest_buf.len, &mut formatted_buf);
        assert_eq!(format_status, SYNVEIL_FFI_STATUS_SUCCESS);
        assert!(!formatted_buf.data.is_null());
        assert_eq!(formatted_buf.len, 71); // "sha256:" (7) + 64 hex = 71

        // SAFETY: formatted_buf was verified non-null and valid.
        let formatted_slice =
            unsafe { std::slice::from_raw_parts(formatted_buf.data, formatted_buf.len) };
        let formatted_str = std::str::from_utf8(formatted_slice).expect("valid utf-8");
        assert_eq!(formatted_str, canonical_str);

        // Release both buffers
        assert_eq!(
            synveil_ffi_buffer_release(&mut digest_buf),
            SYNVEIL_FFI_STATUS_SUCCESS
        );
        assert_eq!(
            synveil_ffi_buffer_release(&mut formatted_buf),
            SYNVEIL_FFI_STATUS_SUCCESS
        );
    }

    #[test]
    fn test_input_errors_and_failure_cleanup() {
        let mut out_buf = SynveilFfiBuffer {
            data: 0x1234 as *mut u8,
            len: 99,
            capacity: 99,
        };

        // 1. Null output pointer -> INVALID_ARGUMENT
        assert_eq!(
            synveil_ffi_sha256_parse(b"sha256:00".as_ptr(), 9, std::ptr::null_mut()),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );

        // 2. Null input + nonzero length -> INVALID_ARGUMENT & out_buf zeroed
        assert_eq!(
            synveil_ffi_sha256_parse(std::ptr::null(), 10, &mut out_buf),
            SYNVEIL_FFI_STATUS_INVALID_ARGUMENT
        );
        assert!(out_buf.is_canonical_empty());

        // 3. Invalid UTF-8 -> INVALID_UTF8 & out_buf zeroed
        let invalid_utf8 = [0xFF, 0xFE, 0xFD];
        assert_eq!(
            synveil_ffi_sha256_parse(invalid_utf8.as_ptr(), invalid_utf8.len(), &mut out_buf),
            SYNVEIL_FFI_STATUS_INVALID_UTF8
        );
        assert!(out_buf.is_canonical_empty());

        // 4. Valid UTF-8 but malformed hash string -> DOMAIN_ERROR & out_buf zeroed
        let invalid_sha = "sha256:invalid_hex_string!";
        assert_eq!(
            synveil_ffi_sha256_parse(invalid_sha.as_ptr(), invalid_sha.len(), &mut out_buf),
            SYNVEIL_FFI_STATUS_DOMAIN_ERROR
        );
        assert!(out_buf.is_canonical_empty());

        // 5. Format with wrong digest length (e.g. 31 or 33 bytes) -> DOMAIN_ERROR & out_buf zeroed
        let wrong_digest = [0xab; 31];
        assert_eq!(
            synveil_ffi_sha256_format(wrong_digest.as_ptr(), wrong_digest.len(), &mut out_buf),
            SYNVEIL_FFI_STATUS_DOMAIN_ERROR
        );
        assert!(out_buf.is_canonical_empty());
    }

    #[test]
    fn test_repeated_allocation_and_release_lifecycle() {
        let canonical_str = format!("sha256:{}", "12".repeat(32));

        for _ in 0..10_000 {
            let mut digest_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
            let p_res = synveil_ffi_sha256_parse(
                canonical_str.as_ptr(),
                canonical_str.len(),
                &mut digest_buf,
            );
            assert_eq!(p_res, SYNVEIL_FFI_STATUS_SUCCESS);
            assert_eq!(digest_buf.len, 32);

            let mut format_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
            let f_res = synveil_ffi_sha256_format(digest_buf.data, digest_buf.len, &mut format_buf);
            assert_eq!(f_res, SYNVEIL_FFI_STATUS_SUCCESS);
            assert_eq!(format_buf.len, 71);

            assert_eq!(
                synveil_ffi_buffer_release(&mut digest_buf),
                SYNVEIL_FFI_STATUS_SUCCESS
            );
            assert!(digest_buf.is_canonical_empty());

            assert_eq!(
                synveil_ffi_buffer_release(&mut format_buf),
                SYNVEIL_FFI_STATUS_SUCCESS
            );
            assert!(format_buf.is_canonical_empty());
        }
    }

    #[test]
    #[allow(unsafe_code)]
    fn test_panic_cleanup_before_ownership_transfer() {
        let mut out_buf = SynveilFfiBuffer {
            data: 0x5678 as *mut u8,
            len: 123,
            capacity: 123,
        };

        let out_ptr: *mut SynveilFfiBuffer = &mut out_buf;

        // Simulate internal operation panicking before transfer
        let status = ffi_status_boundary(|| {
            // SAFETY: out_ptr points to valid out_buf on stack.
            unsafe { initialize_out_buffer(out_ptr)? };
            let _temp_alloc = Box::new([1, 2, 3, 4, 5]);
            panic!("deliberate test panic during FFI execution");
        });

        assert_eq!(status, SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED);
        assert!(out_buf.is_canonical_empty());
    }

    #[test]
    #[allow(unsafe_code)]
    fn test_concurrent_multi_thread_stateless_reentrancy() {
        let threads: Vec<_> = (0..8)
            .map(|thread_idx| {
                std::thread::spawn(move || {
                    let hex_byte = format!("{:02x}", (thread_idx + 1) * 16);
                    let hex_64 = hex_byte.repeat(32);
                    let canonical_str = format!("sha256:{hex_64}");

                    for _ in 0..250 {
                        let mut digest_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
                        let parse_status = synveil_ffi_sha256_parse(
                            canonical_str.as_ptr(),
                            canonical_str.len(),
                            &mut digest_buf,
                        );
                        assert_eq!(parse_status, SYNVEIL_FFI_STATUS_SUCCESS);
                        assert!(!digest_buf.data.is_null());
                        assert_eq!(digest_buf.len, 32);

                        let mut format_buf = SynveilFfiBuffer::CANONICAL_EMPTY;
                        let format_status = synveil_ffi_sha256_format(
                            digest_buf.data,
                            digest_buf.len,
                            &mut format_buf,
                        );
                        assert_eq!(format_status, SYNVEIL_FFI_STATUS_SUCCESS);
                        assert!(!format_buf.data.is_null());
                        assert_eq!(format_buf.len, 71);

                        // SAFETY: format_buf was verified non-null and valid.
                        let formatted_slice =
                            unsafe { std::slice::from_raw_parts(format_buf.data, format_buf.len) };
                        let formatted_str =
                            std::str::from_utf8(formatted_slice).expect("valid utf-8");
                        assert_eq!(formatted_str, canonical_str);

                        assert_eq!(
                            synveil_ffi_buffer_release(&mut digest_buf),
                            SYNVEIL_FFI_STATUS_SUCCESS
                        );
                        assert!(digest_buf.is_canonical_empty());

                        assert_eq!(
                            synveil_ffi_buffer_release(&mut format_buf),
                            SYNVEIL_FFI_STATUS_SUCCESS
                        );
                        assert!(format_buf.is_canonical_empty());
                    }
                })
            })
            .collect();

        for handle in threads {
            handle.join().expect("thread completed successfully");
        }
    }
}
