# Prompt030 Manifest — Native Library Catalog Screen

## Prompt Goal and Baseline

- **Goal:** Replace the authenticated placeholder with a native SwiftUI Library Catalog backed by Prompt029's authenticated `LibraryCatalogRepositoryProtocol`.
- **Phase:** E — File Browser & File Operations. Prompt030 implements the catalog presentation only; it does not browse folders or mutate metadata.
- **Starting `origin/ios-app` SHA:** `69d468f25c1ec56cece3c53c3d906b397bec1072`.
- **Working branch:** `ios/p030-library-catalog-ui`.
- **Cloud checkout evidence:** Reused `/workspace/Synveil`; origin was `https://github.com/nghianguyen150612/Synveil.git`. The initial clean checkout was temporary branch `work` at `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. Fetching `origin/ios-app` returned the stated P029 SHA, and `git merge-base --is-ancestor 69d468f25c1ec56cece3c53c3d906b397bec1072 origin/ios-app` succeeded. The feature branch was created directly from `origin/ios-app`; no source was edited before recording the baseline.

## Authoritative References

Inspected the current `IOS_V0_1_PRODUCT_CONTRACT.md`, `IOS_V0_1_ROADMAP.md`, `IOS_PLATFORM_MAPPING.md`, P026–P029 manifests, and the Library collection/resource/status schemas in `api/openapi.yaml`. Reviewed the P029 models, strict response DTO/decoder, authenticated request provider/repository, `SessionController`, startup states, dependency container, root routing, app entry point, recovery presentation, logout tests, and Android `LibraryScreen.kt` as a behavioral reference.

The screen reuses the existing complete `listLibraries()` result. P029 retains its bounded page retrieval, DeviceBearer origin/session validation, lifecycle revision protection, typed `LibraryFailure`, and recovery routing for `authentication_failed` and `device_revoked`.

## UI and ViewModel Architecture

- `SynveilApp` passes `AppDependencyContainer.libraryCatalog` into `RootView`; only the `.authenticated` branch creates `LibraryCatalogView`.
- `LibraryCatalogView` owns its `@Observable @MainActor` `LibraryCatalogViewModel` in `@State`. The authenticated surface identity includes the session lifecycle revision, so a later authenticated session starts with a new model and navigation stack.
- The ViewModel accepts only the P029 repository boundary and `SessionController`. It stores no bearer value, raw HTTP response, or credential record.
- The typed state covers idle, loading, loaded, empty, refreshing, refresh failure with explicitly identified prior in-memory results, refresh cancellation, initial failure, cancellation, and invalidation.
- A deterministic POSIX presentation sort orders names and uses the canonical Library ID as a tie-breaker. Domain IDs, revisions, names, and timestamps are not mutated.

## Loading, Refresh, and Failure Behavior

- SwiftUI `.task` invokes the one-shot `loadIfNeeded()` gate. The ViewModel verifies the authenticated state and captured lifecycle revision before the request and again before publishing its result. Reappearance and body updates cannot start duplicate initial loads.
- The model tracks and cancels its active repository task on invalidation. Duplicate refresh calls are suppressed while a request is active. Native `.refreshable`, the toolbar action, and explicit Retry actions all use the same `listLibraries()` repository method.
- Empty catalog results render **No libraries available**, explain that the authenticated server has no visible libraries, and offer an explicit refresh.
- Typed `LibraryFailure` values map to concise safe copy. Raw Swift errors, response bodies, headers, and credentials are not presented. Offline, DNS, timeout, and server-unavailable errors can be retried without logout or credential deletion.
- A refresh that fails for a temporary network/server availability reason may retain the same session's nonempty catalog in memory. The UI labels it as previously loaded and not verified by the failed refresh. It is never persisted. Credential, origin, stale-session, TLS, protocol, authorization, and other non-transient failures clear prior results.
- P029 remains responsible for permanent authentication handling. Its root transition invalidates this model, and `RootView` displays the existing P028 recovery experience.

## Library Rows and Selection

- Native `NavigationStack` and inset `List` rows show the Library name, folder symbol, textual status, status symbol, and authoritative `updatedAt` value.
- `ACTIVE`, `READ_ONLY`, and `QUARANTINED` map to **Active**, **Read Only**, and **Quarantined** with distinct accessible symbols. Status is not conveyed by color alone; no row action changes status.
- Selection stores only `LibraryId` in the navigation value and opens a read-only detail screen with name, status, updated time, and secondary Library ID. The screen explicitly states that folder browsing is not available yet; it presents no fictitious contents or file actions.

## Logout, Session Fencing, and Accessibility

- The native toolbar retains the Prompt027 **Log Out / Forget Session** action and confirmation wording. Confirming delegates to `SessionController.requestLogout()`, preserving verified Keychain deletion, coalescing, progress, and cleanup recovery. The confirmation and VoiceOver hint state that local deletion does not revoke server access.
- Logout, recovery, session revision change, and ViewModel invalidation clear transient catalog state and cancel the active repository task. Late success or failure cannot publish. P029's credential identity/origin validation remains unchanged.
- Stable identifiers cover the authenticated root, loading, empty/error feedback, refresh, rows/status, read-only details, and logout actions. VoiceOver labels combine name, status, and update time; progress and error feedback are announced. System typography, multiline names, flexible row heights, and a native List support Dynamic Type and compact/wide layouts.

## Files Added and Modified

Added:

- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Features/Library/LibraryCatalogViewModel.swift`
- `clients/ios/Tests/SynveilTests/LibraryCatalogViewModelTests.swift`
- `clients/ios/Tests/SynveilTests/LibraryCatalogViewTests.swift`
- `clients/ios/Support/tests/test_library_catalog_view_registration.py`
- `docs/ios/manifests/PROMPT030_MANIFEST.md`

Modified:

- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/README.md`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/SessionLogoutTests.swift` (routes the existing logout test through the real catalog screen)

No P024–P029 regression test files or production security/network services were removed or weakened. Both new production files are registered in the app target and the two XCTest files are registered in `SynveilTests`.

## Tests and Local Validation

- **New XCTest coverage:** 29 deterministic ViewModel tests and 5 native SwiftUI construction/status/logout contract tests (34 additions). Coverage includes idle/loading/loaded/empty/error, one-shot load, duplicate load/refresh suppression, refresh completion/failure, transient prior-data labeling, cancellation, logout and recovery invalidation, late success/failure, stale-session failure, safe error mapping/retry policy, permanent-auth retry suppression, stable IDs, long UTF-8 names, and missing repository fail-closed behavior.
- **iOS source validator:** PASS — `python3 clients/ios/Support/validate_ios_sources.py`.
- **Support unit tests:** PASS — 29 tests via `python3 -m unittest discover -s clients/ios/Support/tests`.
- **Documentation validation:** PASS — `bash scripts/validate-docs.sh`.
- **Official Swift formatting:** PASS — Swift 6.2.3 `swift-format lint --recursive --strict clients/ios`.
- **Swift syntax:** PASS — Swift 6.2.3 parser over all `clients/ios/**/*.swift` files.
- **Swift 6 strict concurrency/typecheck:** PASS for the new ViewModel and its tests against temporary minimal session/security type stubs. This check is limited to the ViewModel/test subset; Linux has no SwiftUI or Xcode SDK.
- **Whitespace:** PASS — `git diff --check`.
- **Native Xcode build:** PASS on the exact final feature head `c8e7e630b2265b8c75da4a66c4f1e752961c01c3` in hosted push run [37895120326](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120326) and PR run [37895130564](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130564).
- **Native Simulator tests:** PASS on the same exact head in hosted push run [37895120290](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120290) and PR run [37895130780](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130780). Each run reported **368 total, 367 passed, 1 skipped, 0 failed**. All 34 new Prompt030 tests passed.
- **Rust Apple build:** Not triggered by the changed paths; Prompt030 changes no Rust, core, generated FFI, or Rust build files.
- **Real Keychain round-trip:** `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService` was **SKIPPED** in both hosted Simulator runs. The hosted log states: “The unsigned Simulator test process has no Keychain access entitlement.” Deterministic injected Keychain tests passed; the real round-trip is not claimed passing.
- **Physical-device validation:** `NOT_AVAILABLE`.

## Hosted Delivery Evidence

- **Final feature SHA:** `c8e7e630b2265b8c75da4a66c4f1e752961c01c3` (`feat(ios): add native library catalog screen`).
- **Feature PR:** [#88](https://github.com/nghianguyen150612/Synveil/pull/88), target `ios-app`. Opened as a draft, marked ready after the required iOS checks passed, and squash-merged.
- **iOS Static Validation:** PASS on the exact feature head in push run [37895120267](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120267) and PR run [37895130776](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130776). Both ran Swift formatting, support validator self-tests, and the iOS static source validator.
- **iOS Build:** PASS on the exact feature head in push run [37895120326](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120326) and PR run [37895130564](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130564); both Xcode Simulator builds succeeded.
- **iOS Simulator Tests:** PASS on the exact feature head in push run [37895120290](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120290) and PR run [37895130780](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130780): **368 total, 367 passed, 1 skipped, 0 failed** in each run. The skipped real Keychain test is documented above.
- **iOS Rust Apple Build:** Not triggered and not applicable to the changed paths.
- **Hosted feature merge:** GitHub REST confirmed `merged=true`, `state=closed`, merged at **2026-10-09 06:59:33 UTC**. The resulting hosted `ios-app` feature merge SHA is **`a3cdcae5388231ed53371e5dc8d44cc86ff7b9af`**. A fresh fetch verified this SHA on `origin/ios-app`; the Library Catalog view, ViewModel, tests, and this manifest exist in that tree.
- **Manifest finalization:** This documentation-only follow-up uses the repository's established P029 evidence workflow because the resulting integration SHA is only available after feature merge. The follow-up PR's own merge metadata is recorded by GitHub and is not self-embedded in this manifest.
- **Prompt031 readiness:** `READY_FOR_PROMPT031`, based on the genuine hosted feature merge and passing exact-head iOS checks.

## Unrelated CI Results

- **Rust CI:** Push run [37895120338](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120338) and PR run [37895130765](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130765) failed in unchanged non-iOS work: Clippy, desktop workspace tests/checks on Linux/macOS/Windows, and Windows compilation. No Rust, desktop, or installer source was changed by Prompt030; these failures were not expanded into this feature.
- **Linux AppImage:** Push run [37895120335](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120335) and PR run [37895130634](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130634) failed the existing desktop artifact inspection because a private or temporary build path was found in `synveil-desktop`. No desktop packaging source changed.
- **Web quality:** Push run [37895120338](https://github.com/nghianguyen150612/Synveil/actions/runs/37895120338) failed the web test step; the duplicate PR event passed in Rust CI run [37895130765](https://github.com/nghianguyen150612/Synveil/actions/runs/37895130765). No web files changed.
- **PostgreSQL 17 and DEB/RPM package workflows:** These unrelated runs were still in progress at feature merge and are not claimed as passing or failing by Prompt030.
