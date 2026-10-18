# Dedicated Thin iOS FFI Crate (`synveil-ios-ffi`)

## 1. Role & Isolation Boundary

`synveil-ios-ffi` is the dedicated thin C ABI bridge crate for Synveil iOS (`v0.1`).
It resides at `crates/ios-ffi/` and produces the release static library `libsynveil_ios_ffi.a`.

It bridges pure shared domain logic in `synveil-core` to the native iOS client over a strict C ABI.

---

## 2. Exported C ABI Surface

At P020, the exported C ABI functions are exactly 10 sorted exports:

```c
uint32_t synveil_ffi_abi_version(void);
uint32_t synveil_ffi_buffer_release(struct SynveilFfiBuffer *buffer);
uint32_t synveil_ffi_device_credential_validate(const uint8_t *input_ptr, size_t input_len, uint8_t *out_valid);
uint32_t synveil_ffi_enrollment_secret_validate(const uint8_t *input_ptr, size_t input_len, uint8_t *out_valid);
uint32_t synveil_ffi_library_id_validate(const uint8_t *input_ptr, size_t input_len, uint8_t *out_valid);
uint32_t synveil_ffi_logical_name_validate(const uint8_t *input_ptr, size_t input_len, uint8_t *out_valid);
uint32_t synveil_ffi_node_id_validate(const uint8_t *input_ptr, size_t input_len, uint8_t *out_valid);
uint32_t synveil_ffi_sha256_format(const uint8_t *digest_ptr, size_t digest_len, struct SynveilFfiBuffer *out_utf8);
uint32_t synveil_ffi_sha256_parse(const uint8_t *input_ptr, size_t input_len, struct SynveilFfiBuffer *out_digest);
uint32_t synveil_ffi_validate_abi_version(uint32_t expected_version);
```

### Boolean Validation Conventions & Secrecy
- Reusable helper `ffi_bool_validation_boundary` checks `out_valid != NULL`, initializes `*out_valid = 0`, catches unwinding panics, and writes `1` for valid domain inputs or `0` for invalid domain inputs.
- Domain failures return `SYNVEIL_FFI_STATUS_SUCCESS` (0) with `*out_valid = 0`.
- Memory, encoding, or null-pointer errors return non-zero FFI status codes.
- Secret tokens (`sve1_`, `svd1_`) are never logged, formatted into error messages, or retained by bridge memory.

---

## 3. Memory Ownership & Safety Preconditions

### `SynveilFfiBuffer` Layout
```rust
#[repr(C)]
pub struct SynveilFfiBuffer {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}
```

### Rules
1. **Canonical Empty State**: `{ data: NULL, len: 0, capacity: 0 }`.
2. **Borrowed Inputs**: Input pointers (`const uint8_t *`, `size_t`) are borrowed strictly for the synchronous duration of the FFI function call. Rust never retains or frees foreign input pointers.
3. **Rust-Owned Outputs**: Output allocations are created via standard Rust allocator and transferred to caller-visible `SynveilFfiBuffer` structs on `SUCCESS`.
4. **Failure Zeroing Invariant**: If an FFI function returns non-success or panics, any provided `out_buffer` remains or is set to canonical zero state (`NULL`/0/0).
5. **Release Requirements**: Foreign callers must release Rust-owned buffers by passing them to `synveil_ffi_buffer_release`.
6. **Release Idempotence**: `synveil_ffi_buffer_release` immediately zeroes the caller's struct before dropping memory, making repeated release calls on the same zeroed struct safe.
7. **Copied Live Struct Prohibition**: Physically copying a live `SynveilFfiBuffer` and releasing both copies is strictly prohibited.
8. **Generic Zeroization Policy**: Buffer memory is freed via the default Rust allocator. Generic release does not guarantee cryptographic zeroization (`GENERIC_BUFFER_RELEASE_DOES_NOT_GUARANTEE_SECRET_ZEROIZATION`).

---

## 4. Concurrency & Thread-Safety Contract

1. **RUST_ASYNC_RUNTIME = NONE**: `synveil-ios-ffi` contains no Rust async runtime (no Tokio, async-std, Reqwest, or futures). All FFI exports remain synchronous.
2. **CALLBACK_ABI = NONE**: No function-pointer callbacks, continuations, or task handles cross the FFI boundary (`CALLBACK_ABI_NOT_SELECTED_FOR_IOS_V0_1`).
3. **Stateless Reentrancy**: All FFI functions are reentrant, stateless, and safe for simultaneous calls from multiple caller threads.
4. **No Global State**: The bridge maintains no global mutable state, mutexes, thread-local contexts, or global error strings.
5. **Caller Thread Independence**: Functions operate strictly on arguments passed in and local stack/heap allocations.
