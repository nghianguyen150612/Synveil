# Prompt028 Manifest — Authentication Error UX

## Goal and Baseline

- Goal: actionable native authentication/recovery presentation with safe existing actions; preserve P024–P027 security and lifecycle guarantees.
- Actual starting `origin/ios-app` SHA: `3126e998e667947f1c4ce8703d38035f027199f6`.
- Required P027 merge ancestor verified: `ece9cf998b5087d675802fbd14c3c79100fa4687`.
- Supplied clean checkout reused at `/workspace/Synveil`; initial `work` HEAD was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. The remote fetched only `main` by default; `ios-app` was fetched explicitly into its remote-tracking ref before branching. No edits were made on `main`.
- Branch: `ios/p028-authentication-error-ux`, created from the verified integration baseline.

## References Inspected

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`, `docs/ios/IOS_PLATFORM_MAPPING.md`.
- `docs/ios/manifests/PROMPT024_MANIFEST.md` through `PROMPT027_MANIFEST.md`.
- Application: `AppStartupState.swift`, `SessionController.swift`, `SessionRestorationService.swift`, `SessionLogoutService.swift`, `SecureCredentialSinkProtocol.swift`.
- UI: `EnrollmentViewModel.swift`, `EnrollmentView.swift`, `ServerValidationViewModel.swift`, `ServerValidationView.swift`, `RootView.swift`.
- `api/openapi.yaml`: generic `invalid_enrollment`, documented `authentication_failed`/`device_revoked`, single-shot exchange and owner revoke-all recovery; existing authenticated read-only library probe.
- Android: `data/session/DeviceSessionManager.kt`, `data/enrollment/EnrollmentManager.kt`, `feature/enrollment/EnrollmentScreen.kt`, `feature/startup/StartupState.kt`.
- Existing enrollment, Keychain, restoration, logout and readiness tests; iOS workflow path filters and Xcode target membership.

## Presentation Architecture and Matrix

`AuthenticationRecoveryPresentation` carries a stable category, title, explanation,
recommended step, protection/access statement, optional implemented primary action,
severity/symbol, readable accessibility description and optional safe request ID.
`AuthenticationRecoveryPresenter` maps trusted typed classifications; it does not
classify server strings. `SessionSecureStorageFailure` and retained restoration
failure context carry no payload/credential and do not create a new state machine.

| Condition | Presentation and protection | Action / next step |
| --- | --- | --- |
| `authentication_failed` | Saved authorization rejected; retained credential, blocked access | Trusted owner; a new authorized grant may be needed; no retry/deletion/enrollment button |
| `device_revoked` | Distinct revoked surface; same credential retry cannot repair access | Authorized owner enrollment workflow; no bearer retry |
| Keychain unavailable | Device cannot access protected storage; fail closed | Unlock and reopen app at the existing startup boundary; no inoperative in-app retry |
| Keychain read/other failure | Device security/storage operation unverified | Unlock/reopen; device support and trusted owner |
| Corrupt record | Damaged/unverified session, no silent replacement | Trusted owner recovery; no overwrite |
| Unsupported record | Cannot safely read this session format | Check app version and trusted owner recovery |
| Invalid stored credential | Local validation failed, access blocked | Trusted owner recovery |
| Origin mismatch | Saved session belongs to another server; no bearer sent to new origin | Original trusted configuration guidance; no migration |
| TLS | Secure connection cannot be trusted | Owner checks HTTPS certificate/configuration; no bypass |
| Protocol | Response cannot establish authorization | Owner checks compatible Synveil/configuration |
| Offline / DNS / timeout | Distinct temporary connection guidance; credential retained | Retry Verification through the existing read-only restoration service |
| HTTP 503 restoration | Temporary server unavailability, never revocation | Retry Verification; no deletion |
| Empty local token | Token required, no network request | Enter token |
| Malformed local token | Canonical `sve1_` plus 64 lowercase hex required, no network request | Edit and submit corrected input |
| `invalid_enrollment` | Generic one-time grant rejection, not a typo diagnosis | Request a new authorized grant; rejected input is cleared |
| Ambiguous POST | Result unconfirmed, grant may be consumed, access blocked | Do not resend; trusted owner may revoke affected credentials and issue a new grant |
| Logout cleanup failure | Deletion unverified, logout incomplete, authenticated work blocked | Retry Cleanup through `SessionController.requestLogout()` and `SessionLogoutService` |
| Pre-auth readiness failure | Enrollment remains blocked; no claim of stored authorization | Existing read-only connection check with owner/configuration guidance |

No secondary operation is invented. The existing explicit Forget Session action and
confirmation remain on pending verification. Cleanup never claims server revocation.

## Action, Navigation, Concurrency and Cancellation Safety

- Root recovery is informational; no enrollment POST, credential delete, direct root-state assignment or force-authentication action exists there.
- Retry Verification invokes only `retrySessionRestoration()` from the existing pending state. Duplicate button invocations are disabled immediately; controller operation coalescing remains authoritative.
- Retry Cleanup invokes only `requestLogout()`; its controller-owned cleanup survives caller disappearance/cancellation and only verified absence opens an unauthenticated route.
- Pre-auth retry invokes the existing health validation operation and is gated to `.readyForServerValidation`; no new unauthenticated operation is added.
- Enrollment retains Rust validation, secure-storage preflight, one-shot exchange, verified persistence receipt and exact endpoint gates. Rejected server text is never displayed. Ambiguous outcomes leave enrollment through controlled recovery and cannot be replayed by reconstruction/resubmission.
- Root transition revisions and the start of a newer verification retry clear presentation context. Existing P026/P027 task IDs, identity revalidation and cancellation safeguards remain intact.
- Phase view models capture controller lifecycle revision and endpoint; callbacks after newer transitions are discarded. Server validation uses an operation revision so an old cancelled callback cannot overwrite a newer check or its progress.
- The shared retry button owns its caller task and cancels it on disappearance; the controller preserves P027 cleanup ownership. No decorative progress or success animation is added.

## Accessibility and Diagnostic Privacy

- Shared native `Label`, `Text`, `DisclosureGroup`, `ProgressView` and system button styles; status heading, explanatory/protection/next-step reading order and action hints.
- Stable `synveil.recovery.*` status/diagnostic IDs, category IDs and existing root/retry/cleanup IDs. Action progress exposes an explicit readable state.
- Scrollable recovery/enrollment/readiness content, semantic system fonts and vertically expanding text avoid fixed-height clipping. Status meaning comes from text and symbols, not color.
- Only optional request IDs are displayed/copyable. UI allowlist requires 8–128 ASCII bytes from `[A-Za-z0-9._~-]`; rejects grant/bearer markers, authorization/bearer text, 64-character hex secret shapes, controls, URLs and markup.
- No raw errors, OSStatus, request bodies, server messages/codes, full responses, hosts, authorization headers or credential envelopes are displayed or copied. Restoration does not expose a request ID because its existing typed result has none.
- Manual VoiceOver/physical-device assessment: `NOT_AVAILABLE`; automated label/identifier and Dynamic Type rendering coverage is provided.

## Tests and Local Validation

- Added 37 `AuthenticationRecoveryTests`: deterministic presentation, storage context, successful restoration context clearing, cleanup-service retry/readiness routing, owner-only recovery, no UI replay, local correction and generic rejection, stale/cancelled phase results, duplicate retries, safe diagnostics, status/action accessibility and native Dynamic Type sizing.
- Added five Python presentation-boundary regressions: no UI authentication/enrollment/deletion, existing controller action allowlist, no error dumps, scalable accessible content, correct PBX targets.
- Existing P024 enrollment, P025 Keychain, P026 restoration and P027 logout suites remain in the native test target, including wrong-origin rejection, transient credential retention, duplicate probes, stale callbacks and verified cleanup.
- Linux source validator: passed.
- Static validator tests: passed, 22 tests.
- Documentation validator: passed.
- PBX integrity: passed, 168 unique objects, no unresolved IDs, balanced delimiters; new production/test membership validated.
- `git diff --check`: passed.
- Official Swift 6.2 formatting and strict recursive lint: passed in `swift:6.2`.
- Swift parser: passed for all iOS Swift source files using `swiftc -frontend -parse`.
- Swift 6 language-mode compilation/typechecking and Linux XCTest harness: passed, 79 tests (36 platform-independent recovery tests and 43 restoration regressions), zero failures. Harness copies production Foundation/Observation code, substitutes only the native transport constructor with an offline stub, and excludes SwiftUI/UIKit rendering assertions. This is not native iOS, Simulator or real Keychain execution.
- Native Xcode and SwiftLint are unavailable locally; native build, Apple-framework rendering and Simulator XCTest remain hosted gates.
- iOS Rust Apple Build: not triggered/applicable; no Rust/core/FFI/generated-header or matching build-script/workflow paths changed. Native iOS Build and Simulator workflows still prepare their Rust artifacts.

## Hosted Validation and Delivery

- Final tested feature commit SHA: `a7504b00a07ddccfdb46acff6b6390f1ae05106c` (`fix(ios): fence retry presentation and validate recovery sources`).
- PR targeting `ios-app`: [PR #84](https://github.com/nghianguyen150612/Synveil/pull/84). Opened as draft, marked ready after the required final-head iOS checks passed, then merged by squash.
- Final-head iOS Static Validation: passed on `a7504b00a07ddccfdb46acff6b6390f1ae05106c`, [run 37861519755](https://github.com/nghianguyen150612/Synveil/actions/runs/37861519755). Hosted Apple Swift 6.3.3 strict formatting, all 22 static tests and source validation passed. The duplicate pull-request static run `37861523696` also passed.
- Final-head iOS Build: passed on the same head, [run 37861519693](https://github.com/nghianguyen150612/Synveil/actions/runs/37861519693); Xcode reported `BUILD SUCCEEDED`.
- Final-head iOS Simulator Tests: passed on the same head, [run 37861519725](https://github.com/nghianguyen150612/Synveil/actions/runs/37861519725); Xcode reported `TEST SUCCEEDED` and the result bundle confirmed **256 total, 255 passed, 1 skipped, 0 failed**. All 37 new recovery cases passed, including `testRecoveryViewsRenderAtAccessibilityDynamicTypeSizes`. Existing P024–P027 deterministic suites remained green.
- Real Keychain Simulator test: `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService` was **skipped**. Actual final-head log: “The unsigned Simulator test process has no Keychain access entitlement.” This is not a successful real Keychain round trip; deterministic injected Keychain tests passed.
- Physical-device validation: `NOT_AVAILABLE`.
- Hosted merge state: GitHub confirmed `merged=true` at **2026-10-09 00:19:24 UTC**.
- P028 feature merge / resulting `ios-app` SHA: `fcd039c15d991321f31f2ff76a8cd289ec073518`, verified from the hosted PR record and a fresh `origin/ios-app` fetch.
- Manifest finalization: this post-merge documentation-only update records already-observed evidence. The same `ios/p028-authentication-error-ux` branch incorporates the verified integration merge before this update; no tested iOS source changes are included. Its own finalization commit/merge cannot be embedded self-referentially; GitHub's follow-up PR record provides that separate documentation integration metadata.
- Unrelated failure: [Linux AppImage run 37861519718](https://github.com/nghianguyen150612/Synveil/actions/runs/37861519718) failed “Build and independently reproduce AppImage”: `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop`, with marker `/home/`. No desktop packaging source was changed.
- Other workflows at the feature merge inspection: duplicate AppImage and PostgreSQL scheduled-maintenance runs were in progress; Linux native packages and Rust CI were queued/in progress. They are not claimed as passing and were not modified. Duplicate pull-request iOS Build/Simulator runs were still in progress/queued, separate from the completed passing final-head runs cited above.

## Files Changed

- `clients/ios/App/RootView.swift`
- `clients/ios/Application/Session/AppStartupState.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Features/Authentication/AuthenticationRecoveryPresentation.swift`
- `clients/ios/Features/Authentication/AuthenticationRecoveryView.swift`
- `clients/ios/Features/Onboarding/EnrollmentView.swift`
- `clients/ios/Features/Onboarding/EnrollmentViewModel.swift`
- `clients/ios/Features/Onboarding/ServerValidationView.swift`
- `clients/ios/Features/Onboarding/ServerValidationViewModel.swift`
- `clients/ios/Support/tests/test_authentication_recovery_safety.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/AuthenticationRecoveryTests.swift`
- `clients/ios/Tests/SynveilTests/SessionRestorationTests.swift`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/manifests/PROMPT028_MANIFEST.md`
