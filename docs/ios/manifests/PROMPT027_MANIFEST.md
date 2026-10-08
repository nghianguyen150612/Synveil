# Prompt027 Manifest — Logout & Credential Cleanup

## Overview

- **Prompt Number**: Prompt027
- **Title**: Logout & Credential Cleanup
- **Goal**: Provide explicit, offline-capable local logout with verified Keychain deletion and lifecycle fencing against stale restoration.
- **Starting `origin/ios-app` SHA**: `7dc025c18ddaa39c51ec3018c775a1533ef953f9` (verified Prompt026 integration SHA).
- **Actual checkout SHA before branch creation**: `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.
- **Starting worktree state**: Clean on the supplied `work` branch before switching. The checkout was reused at `/workspace/Synveil`; the initially fetched branch was resolved explicitly to `origin/ios-app` because the checkout had no remote-tracking ref.
- **Working branch**: `ios/p027-logout-credential-cleanup`, created directly from `origin/ios-app` at the verified Prompt026 SHA.

## Authoritative References Inspected

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/manifests/PROMPT025_MANIFEST.md`
- `docs/ios/manifests/PROMPT026_MANIFEST.md`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Application/Session/SessionRestorationService.swift`
- `clients/ios/Application/Session/AppStartupState.swift`
- `clients/ios/Application/Services/SecureCredentialSinkProtocol.swift`
- `clients/ios/Infrastructure/Security/KeychainCredentialStore.swift`
- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentManager.kt`
- `clients/android/app/src/main/java/com/synveil/android/data/enrollment/CredentialVault.kt`
- `clients/android/app/src/main/java/com/synveil/android/data/session/DeviceSessionManager.kt`
- `api/openapi.yaml`

## Logout Architecture

- `AppDependencyContainer` creates `SessionLogoutService` from the same `KeychainCredentialStore` used by enrollment and restoration.
- `SessionLogoutServiceProtocol` is the testable application boundary. `SessionLogoutService` calls `SecureCredentialSinkProtocol.delete()` and then verifies absence through `isActiveCredentialAbsent()`.
- The service has no HTTP transport, enrollment token, bearer parameter, or Security.framework dependency.
- `SessionController.requestLogout()` transitions to `.logoutInProgress` before cleanup, clears its pending restoration session, cancels and invalidates the current restoration operation, and owns one logout task shared by duplicate requests.
- Only a verified `.credentialAbsent` result returns to an unauthenticated route. A cleanup failure is kept as typed, non-secret `logoutFailure` context with state `.logoutCleanupRequired` for an explicit retry.
- Restoration task IDs plus the existing monotonic transition revision prevent old startup or retry results from changing state after logout. Retry and startup are state-gated while cleanup runs or needs recovery.
- The logout task is unstructured and owned by `SessionController`; cancelling or dismissing the SwiftUI caller after confirmation does not cancel the bounded local cleanup. Before confirmation, Cancel starts no operation.

## Local Logout and Server Revocation

- Local logout removes the device's locally stored `svd1_` credential and blocks local authentication through it. It does not revoke that bearer on the server.
- `api/openapi.yaml` documents `/api/v1/auth/logout` as browser-session logout. Device-credential revoke operations require an owner browser session and CSRF authorization. Prompt027 calls neither route.
- UI and product documentation state that the server may continue accepting the credential until an owner separately revokes it.
- Logout makes no network request and works offline. Timeouts, HTTP 503, DNS errors, backgrounding, and ordinary restoration cancellation do not initiate cleanup.

## Keychain Deletion and Verification

- Reuses P025's Keychain identity unchanged: service `com.synveil.ios.device-session.v1`, account `active-device-session.v1`.
- `KeychainCredentialStore.delete()` continues to use native `SecItemDelete`, serialized by the existing actor lifecycle lock. `errSecSuccess` and `errSecItemNotFound` proceed to verification; other statuses map through the existing typed storage error model.
- The store checks absence with an attribute-only `SecItemCopyMatching` query while holding the lifecycle lock. The verification query does not request `kSecReturnData`; an item that remains causes `.verificationFailure`.
- The application service performs a second protocol-level absence check. A missing item counts as successful cleanup. A present item or an unreadable Keychain never produces success.
- Read-back and storage errors remain typed and contain no bearer or serialized envelope.

## Failure, Retry, Concurrency, and Cancellation

- Authentication is invalidated before any Keychain operation. If deletion fails or absence cannot be verified, authenticated UI and operations stay blocked; the app shows a secure-storage recovery surface with Retry Cleanup.
- Cleanup failures are not routed to Welcome or ordinary server validation as if logout succeeded. The configured or restored server endpoint stays in memory for the recovery attempt.
- Rapid requests await one controller-owned operation. Keychain deletion remains idempotent and serialized against existing store/load/update operations.
- Restoration retry cannot start during or after cleanup. A late successful authorization response is discarded by task cancellation, operation-ID fencing, and the transition revision.
- Successful cleanup transitions to `.readyForServerValidation` when an endpoint remains in memory, otherwise `.needsServerProfile`. Enrollment still requires the normal validation transition to `.needsEnrollment`.
- If a restored endpoint existed only inside the deleted Keychain envelope and no bootstrap endpoint was configured, the endpoint is retained for the current process but is not persisted separately. A later cold launch therefore returns to server setup.
- The app has no long-lived authenticated transport instance yet. Prompt027 clears `pendingRestorationSession` and any active restoration task handle. Swift does not guarantee cryptographic zeroization of copies already held by an in-flight framework call.

## UI Integration

- The authenticated shell offers a destructive “Log Out / Forget Session” action with a native SwiftUI confirmation dialog.
- The restoration-verification-pending screen also offers an explicit “Forget saved session” action so a user can cancel a saved session while a retry is in flight.
- Cleanup has a progress surface; errors have a retry surface. Stable accessibility identifiers are provided for the action, confirmation, progress, and retry controls. Labels use scalable system text and VoiceOver hints.
- No Prompt028 authentication-error redesign or automatic logout behavior was added.

## Tests and Validation

- **New deterministic logout tests**: 15 `SessionLogoutTests`, covering local/offline cleanup, missing item, deletion and verification failures, retry state, endpoint routing, duplicate requests, cancellation, stale authorization results including a delayed HTTP 200, UI routing/accessibility identifiers, state-transition gating, and cold-start restoration after deletion.
- **New Keychain tests**: 2 additions verify attribute-only absence checking and reject a successful delete status when the item remains retrievable.
- **Existing regression coverage**: Prompt024 enrollment, Prompt025 Keychain, and Prompt026 restoration test suites remain included in the configured iOS test target. Existing timeout/503 restoration tests assert credentials are not deleted.
- **iOS XCTest source method count**: 219 at the current implementation revision; final Simulator result pending macOS CI.
- **Linux iOS source validator**: passed (`python3 clients/ios/Support/validate_ios_sources.py`).
- **Static validator unit tests**: passed (17 tests; `python3 -m unittest discover -s clients/ios/Support/tests`).
- **Documentation validation**: passed (`bash scripts/validate-docs.sh`).
- **PBX project structure**: passed local structural check (162 unique objects, no unresolved 24-character references, balanced delimiters). Native Xcode project loading remains a macOS CI check.
- **Diff whitespace validation**: passed (`git diff --check`).
- **Swift formatting and syntax**: native Swift tooling is unavailable in the Linux environment. A `swift:6.2` Docker image pull stalled after partial downloads and was stopped; no Swift formatter, parser, typecheck, or concurrency result is claimed locally. Final iOS Static Validation and Build remain required.
- **Rust regression tests**: no Rust/FFI source changed and `cargo` is unavailable locally; iOS Rust Apple Build applicability will be checked against its workflow path filters.
- **macOS CI on final P027 head**: pending GitHub Actions. Required gates: iOS Static Validation, iOS Build, iOS Simulator Tests, and iOS Rust Apple Build if applicable.
- **Simulator Keychain evidence**: pending final Simulator run. Do not infer real Keychain execution if the existing unsigned-Simulator entitlement test skips.
- **Physical-device validation**: `NOT_AVAILABLE` unless a physical iPhone run is obtained.

## Files Modified

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/Application/Services/SecureCredentialSinkProtocol.swift`
- `clients/ios/Application/Session/AppStartupState.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Application/Session/SessionLogoutService.swift`
- `clients/ios/Infrastructure/Security/KeychainCredentialStore.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/KeychainCredentialStoreTests.swift`
- `clients/ios/Tests/SynveilTests/RootViewTests.swift`
- `clients/ios/Tests/SynveilTests/SessionLogoutTests.swift`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/manifests/PROMPT027_MANIFEST.md`

## Delivery

- **Feature commit SHA**: pending.
- **PR targeting `ios-app`**: pending.
- **Merge status**: pending GitHub verification.
- **Resulting `ios-app` SHA**: pending GitHub verification.
- **Unrelated failures**: pending final workflow inspection.
