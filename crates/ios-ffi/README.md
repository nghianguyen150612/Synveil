# `synveil-ios-ffi`

Dedicated thin C ABI bridge crate for the native Synveil iOS client (`v0.1`).

## Scope & Responsibility
- Serves as the sole authorized C ABI export boundary for Synveil iOS.
- Wraps platform-neutral shared Rust core (`synveil-core`).
- Produces a release static library archive (`staticlib` -> `libsynveil_ios_ffi.a`).

## First Exported C Symbol (P016)
- `synveil_ffi_abi_version() -> u32`: returns `SYNVEIL_FFI_ABI_VERSION` (`1`).
- Protected by `std::panic::catch_unwind` returning `0` on panic.

## Prohibited Dependencies
This crate must remain free of desktop, server, async runtime, database, and platform keyring dependencies:
- `synveil-client` / `synveil-desktop` / CXX-Qt / Qt bindings
- `synveil-install-engine` / `synveil-platform`
- Axum / PostgreSQL / SQLx
- Tokio / Reqwest
- Keyring / Apple Framework Bindings

## Build, Header Generation & Validation
```bash
# Host check and tests
cargo check -p synveil-ios-ffi --locked
cargo test -p synveil-ios-ffi --locked

# Generate / verify C header
./scripts/generate-ios-rust-header.sh
./scripts/generate-ios-rust-header.sh --check

# Local Apple target compilation check (macOS)
./scripts/check-ios-rust.sh

# Build, stage, and validate release Apple artifacts
./scripts/build-ios-rust-artifacts.sh
python3 scripts/validate_ios_rust_artifact.py
```

## CI Pipeline (P016)
Workflows: `.github/workflows/ios-rust-apple-build.yml`
Artifacts published: `synveil-ios-rust-staticlibs`
Staging directory: `target/ios-rust-artifacts/`
Contains:
- `device/arm64/libsynveil_ios_ffi.a`
- `simulator/arm64/libsynveil_ios_ffi.a`
- `simulator/x86_64/libsynveil_ios_ffi.a`
- `simulator/universal/libsynveil_ios_ffi.a`
- `include/synveil_ios_ffi.h`
- `manifest.json` (Schema v1 metadata with C ABI exports)
- `SHA256SUMS` (Integrity checksums)
