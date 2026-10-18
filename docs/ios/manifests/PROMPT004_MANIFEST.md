# Prompt004 Manifest — Synveil iOS Architecture Specification

## Execution Metadata
- **Prompt Number**: `004`
- **Goal**: Define the production architecture for the native Synveil iOS v0.1 client and produce ADR-058.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `de4f6c922d38e9b799ccf759027a8b8b81d35990`
- **Work Branch**: `ios/p004-ios-architecture`

## Authoritative Inputs Inspected
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`
- `docs/ios/PROMPT001_MANIFEST.md`
- `docs/ios/manifests/PROMPT002_MANIFEST.md`
- `docs/ios/manifests/PROMPT003_MANIFEST.md`
- `docs/adr/README.md`
- Existing ADRs (ADR-001 through ADR-057)

## Codebase Sources & Reference Inspected
- `crates/core/src/`
- `crates/client-sync/src/`
- `clients/android/app/src/main/java/com/synveil/android/`

## Files Created / Modified
- `docs/ios/IOS_ARCHITECTURE.md` (Created)
- `docs/adr/ADR-058-ios-v0.1-client-architecture.md` (Created)
- `docs/adr/README.md` (Modified)
- `docs/ios/manifests/PROMPT004_MANIFEST.md` (Created)

## Commands Run
- `git status`
- `git fetch origin`
- `git rev-parse origin/ios-app`
- `git checkout -b ios/p004-ios-architecture origin/ios-app`
- `cargo test -p synveil-core -p synveil-object-store`

## Major Architecture Decisions
1. **Native SwiftUI Application**: Native Swift application with SwiftUI, Swift Structured Concurrency (`async`/`await`, `Actor`, `@MainActor`), `URLSession`, Keychain Services, and `BackgroundTasks`.
2. **Apple System API Ownership**: Apple platform APIs remain native Swift components. Generic Rust crates MUST NOT import or depend on Apple frameworks (`Security`, `UIKit`, `CFNetwork`).
3. **Narrow Shared Rust Bridge**: Exposes pure domain correctness rules, identifier generation (UUIDv7), token parsing, cryptographic SHA-256 hashing, change feed evaluation, signed ACK computation, rebaseline snapshot manifest tree hash verification, and conflict rules via a minimal C-FFI / UniFFI bridge.
4. **UI Isolation**: SwiftUI views are pure functions of state. Views MUST NOT directly invoke raw network transport, FFI, Keychain APIs, or SQLite database operations.
5. **URLSession Transport**: Networking uses Apple `URLSession` (supporting both foreground and background `URLSessionConfiguration.background` transfers) rather than forcing Rust `reqwest`/`rustls` onto iOS.
6. **Keychain Secret Storage**: Authentication credentials (`svd1_` device tokens) reside strictly in Apple Keychain Services (`Security.framework`) with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` protection. Secrets MUST NEVER appear in `UserDefaults`, SQLite database tables, application logs, or SwiftUI view state.
7. **Decoupled Transfer Engine**: File transfers (streaming downloads and 4 MiB resumable uploads) are managed by an independent Transfer Engine decoupled from SwiftUI view lifetimes.
8. **Deferred Extensions Boundary**: File Provider Extension (`NSFileProviderExtension`, P053–P055) and PhotoKit Auto-Upload (`PHPhotoLibrary`, P056–P058) are defined as isolated extension subsystems sharing App Group storage.
9. **No Desktop Porting**: Desktop process launcher, UNIX domain socket IPC, systemd services, and Qt/QML UI shell are explicitly excluded from iOS.

## Unresolved Product Decisions Preserved
- **Keychain Access Group Sharing (`DECISION-01`)**: Kept unresolved as an architectural parameter; v0.1 foundation uses default app bundle Keychain isolation (`kSecAttrAccessGroup` omitted), with the `KeychainVault` protocol designed to accept an optional access group identifier parameter when File Provider (P053) introduces shared App Groups later.

## Validation Performed
- Verified `docs/ios/IOS_ARCHITECTURE.md` covers all 29 required architecture sections.
- Verified ADR-058 exists and follows the repository bilingual format.
- Verified `docs/adr/README.md` correctly indexes ADR-058.
- Ran `cargo test -p synveil-core -p synveil-object-store` (All unit tests passed cleanly).
- Confirmed zero application code files, Xcode project files, Swift source files, Objective-C/C headers, or Cargo target changes were created.

## Limitations & Scope Enforcement
- Documentation and architecture specification only.
- No Swift code, Xcode project files, FFI exports, XCFrameworks, or Rust code changes were introduced.
