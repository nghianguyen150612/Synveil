# Prompt025 Manifest — Keychain Session Storage

## Overview

- **Prompt Number**: Prompt025
- **Title**: Keychain Session Storage
- **Goal**: Persist validated device credentials in native Keychain Services with explicit update, load, and delete behavior.
- **Starting `ios-app` SHA**: `78d3aa9325c2af2811111168be0bcc287c904809`
- **Actual starting SHA**: `78d3aa9325c2af2811111168be0bcc287c904809`
- **Working Branch**: `ios/p025-keychain-session-storage`
- **Starting worktree**: Clean after aligning the cloud-provided unrelated `work` checkout to the verified hosted `origin/ios-app` tip.

## Authoritative References Inspected

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/manifests/PROMPT024_MANIFEST.md`
- `clients/ios/Application/Services/SecureCredentialSinkProtocol.swift`
- `clients/ios/Features/Onboarding/EnrollmentViewModel.swift`
- `clients/ios/Domain/Services/Enrollment/DeviceCredential.swift`
- `clients/ios/Domain/Services/Enrollment/DeviceCredentialRecord.swift`
- `clients/ios/Domain/Services/Enrollment/EnrollmentExchangeService.swift`
- `clients/ios/Application/Services/RustBridgeProtocol.swift`
- `clients/ios/Infrastructure/RustBridge/RustBridgeAsyncAdapter.swift`
- `clients/android/app/src/main/java/com/synveil/android/data/enrollment/CredentialVault.kt`
- `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentManager.kt`

## Keychain Design

- **Architecture**: SwiftUI/ViewModel → `SecureCredentialSinkProtocol` → actor-isolated `KeychainCredentialStore` → `Security.framework` (`SecItemAdd`, `SecItemUpdate`, `SecItemCopyMatching`, `SecItemDelete`).
- **Keychain item class**: `kSecClassGenericPassword`.
- **Service**: `com.synveil.ios.device-session.v1`.
- **Account**: `active-device-session.v1` (one active session per app installation; neither identifier contains the bearer).
- **Accessibility**: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`.
- **Synchronization**: Explicitly non-synchronizable (`kSecAttrSynchronizable = false`).
- **Access group**: App-only default Keychain access; custom `kSecAttrAccessGroup` omitted and no Keychain Sharing entitlement added.
- **Schema**: Bounded Codable JSON session envelope, `formatVersion = 1`, maximum 4 KiB. Unknown versions are typed as unsupported; malformed payloads fail closed.
- **Persisted fields**: Canonical server endpoint, owner user ID, device ID, credential ID, `svd1_` credential, and server creation timestamp. Request IDs and diagnostics are excluded.
- **Origin binding**: The canonical endpoint is validated and compared on load/update; a credential cannot be replaced into a different server scope.
- **Preflight**: Unique temporary account; bounded non-secret probe is added, read back, compared, and deleted on every path. It never uses the production account.
- **Add/update**: Initial item creation uses `SecItemAdd`; replacement uses `SecItemUpdate` and is verified by read-back. No delete-first update.
- **Load**: Validates payload size/version, endpoint canonicalization, IDs, timestamp, and `svd1_` syntax plus Rust shared-core validation. No startup restoration is invoked.
- **Delete**: Deletes only the local active session; missing item is already deleted. No server revocation claim.
- **OSStatus mapping**: Central infrastructure mapper distinguishes unavailable, missing, duplicate, operation failures, and unexpected statuses without secret data.
- **Concurrency**: A Swift actor serializes session lifecycle operations.
- **Single-session boundary**: v0.1 supports one active device session; multi-profile access-group sharing is deferred.

## Enrollment Integration

- Production composition initializes `RustBridgeAsyncAdapter` and injects it into both enrollment exchange and Keychain load/store validation.
- If Rust validation does not initialize, enrollment has no exchange service and fails closed before network use.
- Enrollment order remains local Rust `sve1_` validation → Keychain preflight → single-shot exchange → validated Rust `svd1_` response → Keychain store → read-back verification.
- Only the store's non-secret verification receipt can advance `.needsEnrollment` to `.authenticated`, and the receipt endpoint must match the configured endpoint.
- Storage or verification failure after exchange moves the session to recovery-required; ambiguous exchanges are not replayed.
- P026 startup restoration and P027 logout/revocation workflows remain deferred.

## Tests and Validation Evidence

- **Unit tests**: Added injectable Security client coverage for item attributes, preflight/cleanup, add/update semantics, verification, schema and scope validation, deletion, error mapping, metadata redaction, and transient request ID exclusion.
- **Enrollment tests**: Cover fail-closed validation, zero calls after failed preflight, single-shot ambiguity, post-exchange store/verification failures, and authentication transition gating.
- **Simulator Keychain test**: The isolated unique-service round-trip test ran in Apple CI and was skipped because the unsigned Simulator test process has no Keychain access entitlement. No real Simulator Keychain round-trip was verified.
- **Linux source/static validation**: `validate_ios_sources.py` passed; its 17 static-validator unit tests passed; `scripts/validate-docs.sh` passed; `git diff --check` passed; changed Swift source line-length validation passed.
- **PBX project validation**: OpenStep syntax and object-reference parser passed (152 objects, 217 references); the fresh Xcode Build passed on the final P025 code head.
- **Swift formatting**: Not available locally; fresh iOS Static Validation, including strict Swift formatting, passed on the final P025 code head.
- **Rust regression tests**: Not run locally because `cargo` is unavailable. No Rust/FFI source changed, so the dedicated iOS Rust Apple Build workflow was not triggered; the iOS Build and Simulator workflows both successfully prepared Rust static libraries for Xcode.
- **Simulator test evidence**: Run `37776897010` passed on `f9b820a003f0c04c167a03cec0c749dcfcc8b789`: 158 tests, 0 failures, 1 skipped. The 33 `KeychainCredentialStoreTests` had 32 passes and 1 skip; the skipped test was the real Simulator Keychain round-trip, skipped for the missing Keychain entitlement.
- **Final-head iOS CI**: All required gates passed on `f9b820a003f0c04c167a03cec0c749dcfcc8b789`: iOS Static Validation run `37776896998`, iOS Build run `37776897271`, and iOS Simulator Tests run `37776897010`.
- **Physical-device Keychain validation**: `NOT_AVAILABLE` (no physical iPhone was available).
- **Unrelated CI at merge time**: Linux AppImage run `37776897076` failed its artifact path check after finding `/home/` in `synveil-desktop`; the unrelated Rust CI run had failures in the Windows workspace/UI and macOS/Windows test jobs. Linux native packages and PostgreSQL checks were still running when Prompt025 merged. No unrelated workflows or components were changed.

## Delivery

- **Final P025 code commit**: `f9b820a003f0c04c167a03cec0c749dcfcc8b789` (`test(ios): handle unsigned simulator Keychain access`).
- **PR targeting `ios-app`**: [#79](https://github.com/nghianguyen150612/Synveil/pull/79).
- **Merge state**: GitHub verified `merged = true`, `state = closed`, merged at `2026-10-08T12:52:49Z`.
- **Merge commit / resulting `ios-app` SHA at P025 merge**: `cb00a182316705f292ae4cd5966feebd41972f0a`.
- **Unrelated CI failures**: Recorded above; not addressed by Prompt025.
