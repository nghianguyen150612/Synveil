# Prompt026 Manifest — Session Restoration

## Overview

- **Prompt Number**: Prompt026
- **Title**: Session Restoration
- **Goal**: Restore a locally validated Keychain device session at startup, recover its canonical server endpoint, and enter the authenticated state only after a successful read-only DeviceBearer authorization check.
- **Verified starting `ios-app` SHA**: `db7f807dc4bc84de7d0502f120b6ee372355f415`
- **Required P025 commits verified**: Keychain implementation `cb00a182316705f292ae4cd5966feebd41972f0a` and manifest finalization `db7f807dc4bc84de7d0502f120b6ee372355f415`.
- **Cloud checkout/bootstrap evidence**: Reused the existing `/workspace/Synveil` checkout; `origin` points to `https://github.com/nghianguyen150612/Synveil.git`; `git fetch origin ios-app` resolved to the verified SHA above; the worktree was clean before branch creation.
- **Actual starting `HEAD` before editing**: `db7f807dc4bc84de7d0502f120b6ee372355f415`.
- **Working branch**: `ios/p026-session-restoration`.

## Authoritative References Inspected

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/manifests/PROMPT024_MANIFEST.md`
- `docs/ios/manifests/PROMPT025_MANIFEST.md`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Application/Session/AppStartupState.swift`
- `clients/ios/Application/Configuration/AppConfiguration.swift`
- `clients/ios/Application/Services/SecureCredentialSinkProtocol.swift`
- `clients/ios/Infrastructure/Security/KeychainCredentialStore.swift`
- `clients/ios/Domain/Services/Enrollment/DeviceCredentialSession.swift`
- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/Infrastructure/Network/URLSessionHTTPTransport.swift`
- `api/openapi.yaml`
- `clients/android/app/src/main/java/com/synveil/android/data/session/DeviceSessionManager.kt`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- Existing iOS session, enrollment, transport, Keychain, and root-view tests.

## Restoration Architecture

`SynveilApp` calls `AppDependencyContainer.prepareEnrollmentSecurity()` before `SessionController.start()`. The container coalesces overlapping preparation tasks, initializes `RustBridgeAsyncAdapter`, creates one `KeychainCredentialStore`, and injects a `SessionRestorationService` using that store and `AuthenticatedSessionValidationService`.

The service calls `SecureCredentialSinkProtocol.load(expectedServerEndpoint:)`. A missing item remains a normal onboarding result. A loaded session is checked against its canonical endpoint with `GET /api/v1/libraries?limit=1`; a second Keychain load after the response discards a stale result if the stored session identity changed during the probe.

`SessionController` remains `@MainActor`-isolated. It owns the only startup transition to `.authenticated` from a `SessionRestorationResult.remotelyVerifiedAuthorizedSession`. The first-enrollment gate `markAuthenticated(after: SecureCredentialPersistenceReceipt)` remains scoped to the `.needsEnrollment` path.

## Keychain and Origin Policy

- Reuses P025's `SecureCredentialSinkProtocol.load(expectedServerEndpoint:)`; no raw `SecItemCopyMatching` call was added.
- P025 storage identity and format remain unchanged: service `com.synveil.ios.device-session.v1`, account `active-device-session.v1`, schema version 1, 4 KiB maximum, and `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`.
- The existing store remains non-synchronizable, uses the app's default access group, and Rust-validates the `svd1_` bearer before returning a session.
- When configuration has an endpoint, the store receives that endpoint as `expectedServerEndpoint`; a mismatch fails before any network request and does not rewrite or delete the item.
- When configuration has no endpoint, the canonical endpoint from the validated Keychain session is used. No endpoint is derived from unvalidated stored text.
- Missing Keychain item plus no endpoint routes to `.needsServerProfile`; missing item plus a configured endpoint routes to `.readyForServerValidation`. Existing health validation remains required before `.needsEnrollment`.

## Authenticated Authorization Probe

- **Operation**: `GET /api/v1/libraries?limit=1`.
- **Rationale**: `api/openapi.yaml` documents `listLibraries` as implemented and permits `DEVICE_CREDENTIAL` authentication. `limit` is bounded to 1–100, so 1 is the smallest valid read-only page. Health endpoints do not establish DeviceBearer authorization.
- **Request policy**: HTTP GET, no body, exact session origin and canonical base path, `Accept: application/json`, and `Authorization: Bearer <validated credential>` only in the header. The bearer is excluded from URL, query, request IDs, UI state, navigation, and logs.
- **Response policy**: Existing `URLSessionHTTPTransport` with finite 10-second request and 15-second resource timeouts, a 16 KiB body bound, ephemeral no-cookie configuration, disabled redirects, default trusted TLS validation, and no retry logic. Only a bounded, structurally valid JSON 200 response proves authorization.
- **Rejection policy**: A valid documented 401 error envelope maps `authentication_failed` and `device_revoked` separately. Neither result deletes the Keychain credential. Revocation is never inferred from arbitrary 403 responses, malformed errors, or transport failure.

## Result Classification and State Transitions

- Typed results distinguish no stored item, locally valid but remotely unverified session, remotely verified session, Keychain unavailable, read failure, corrupt/unsupported payload, scope mismatch, invalid credential, generic Keychain failure, authentication rejection, revocation, server unavailable, DNS failure, network failure, timeout, TLS failure, wrong content type, malformed protocol response, redirect rejection, session identity change, and cancellation.
- Only `.remotelyVerifiedAuthorizedSession` may transition startup to `.authenticated`.
- Transient offline, DNS, timeout, and HTTP 503 outcomes enter `.restorationVerificationPending`. TLS, protocol, authentication, revocation, scope, credential, and secure-store failures enter typed recovery states.
- Authentication rejection and device revocation preserve the existing Keychain item for Prompt027 cleanup and recovery policy.
- A missing item never jumps to `.needsEnrollment`; configured server validation remains an explicit prerequisite.
- `createdAt` is not treated as an expiry timestamp.

## Retry, Cancellation, and Race Handling

- Retry is explicit and offered only from `.restorationVerificationPending`.
- When a locally validated session is available, retry reloads it with the expected endpoint and requires its identity to match before performing a new read-only authorization request. If startup was cancelled before a session was loaded, retry repeats the validated Keychain lookup.
- Retry does not invoke enrollment exchange, submit `sve1_`, rotate credentials, update Keychain, or delete the active item.
- Concurrent startup and retry calls are coalesced by `SessionController`; the dependency container also coalesces concurrent Rust/Keychain initialization.
- Controller transition revisions discard results after endpoint or root-state changes. The service reloads the Keychain session after an authorization response and returns a typed session-changed result if its identity no longer matches.
- Cancellation is checked after Keychain/Rust validation, transport, the final identity read, and before the restoration authenticated transition. Cancellation never authenticates or deletes credentials.

## Tests and Validation

- Added **43 deterministic `SessionRestorationTests`** with injected Keychain and HTTP boundaries, including endpoint recovery, origin mismatch, storage failures, Rust-validation fail-closed behavior, DeviceBearer header and URL privacy, exact API origin and path, bounded read-only transport, strict response shape, 401 classifications, transient failures, retry, duplicate startup/retry, cancellation, stale state and session identity, and no artificial expiry.
- Extended root-view coverage for the verification-pending surface and recovery categories.
- Existing Prompt024 enrollment tests retain first-enrollment receipt gating; existing Prompt025 Keychain tests remain unchanged and are included in final Simulator CI.
- **Linux source validator**: passed (`python3 clients/ios/Support/validate_ios_sources.py`).
- **Static validator unit tests**: passed (17 tests; `python3 -m unittest discover -s clients/ios/Support/tests`).
- **Linux restoration XCTest**: passed (43 tests, 0 failures) in a temporary Swift 6.2 package built from the production restoration files and injected test doubles. This does not include Apple frameworks or the real Keychain.
- **Native iOS test target size**: 202 XCTest methods across `clients/ios/Tests/SynveilTests` at this revision; final-head macOS Simulator results are pending.
- **Documentation validation**: passed (`scripts/validate-docs.sh`).
- **Swift syntax and type validation**: passed with Swift 6.2 Linux for changed Swift syntax, the production restoration dependency set under strict concurrency, and the new test source. This does not replace an iOS build.
- **PBX project verification**: structural check passed (158 unique objects, no duplicate or unresolved IDs, balanced delimiters); macOS build remains a hosted CI gate.
- **Swift formatter**: official Swift 6.2 `swift-format lint --recursive --strict clients/ios` passed in the `swift:6.2` container.
- **Rust FFI regression tests**: no Rust/FFI source changed; local `cargo` is unavailable. The Rust Apple Build workflow is applicable only if its configured paths change.
- **Final-head iOS Static Validation**: pending hosted PR workflow.
- **Final-head iOS Build**: pending hosted PR workflow.
- **Final-head iOS Simulator Tests**: pending hosted PR workflow.
- **Simulator Keychain integration test**: P025's real Keychain round-trip was skipped because the unsigned Simulator test process lacked a Keychain entitlement. P026 does not fabricate a real Keychain result; deterministic injected storage tests provide CI coverage.
- **Physical-device validation**: `NOT_AVAILABLE`.

## Files Changed

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/README.md`
- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/Application/README.md`
- `clients/ios/Application/Session/AppStartupState.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Application/Session/SessionRestorationService.swift`
- `clients/ios/Domain/Services/Transport/AuthenticatedSessionValidationService.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/RootViewTests.swift`
- `clients/ios/Tests/SynveilTests/SessionRestorationTests.swift`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/manifests/PROMPT026_MANIFEST.md`

## Delivery

- **Feature commit SHA**: pending.
- **PR targeting `ios-app`**: pending.
- **Merge result**: pending GitHub verification. The final merge state and resulting integration SHA will be recorded in the PR's final verification comment to avoid an evidence-only follow-up commit.
- **Resulting `ios-app` SHA**: pending GitHub verification; see the PR's final verification comment.
- **Unrelated CI failures**: pending final PR workflow inspection.
