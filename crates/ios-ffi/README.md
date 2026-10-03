# `synveil-ios-ffi`

Dedicated thin C ABI bridge crate for the native Synveil iOS client (`v0.1`).

## Scope & Responsibility
- Serves as the sole authorized C ABI export boundary for Synveil iOS.
- Wraps platform-neutral shared Rust core (`synveil-core`).
- Produces a static library archive (`staticlib` -> `libsynveil_ios_ffi.a`).

## Prohibited Dependencies
This crate must remain free of desktop, server, async runtime, database, and platform keyring dependencies:
- `synveil-client` / `synveil-desktop` / CXX-Qt / Qt bindings
- `synveil-install-engine` / `synveil-platform`
- Axum / PostgreSQL / SQLx
- Tokio / Reqwest
- Keyring / Apple Framework Bindings

## Build & Validation
```bash
# Host check and tests
cargo check -p synveil-ios-ffi --locked
cargo test -p synveil-ios-ffi --locked

# Apple target compilation check (macOS)
cargo check -p synveil-ios-ffi --locked --target aarch64-apple-ios
cargo check -p synveil-ios-ffi --locked --target aarch64-apple-ios-sim
```
