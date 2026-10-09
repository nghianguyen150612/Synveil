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
- **Native Xcode build and Simulator tests:** Pending hosted macOS CI; no native iOS execution is claimed from Linux.
- **Rust Apple build:** Not triggered by the changed paths; Prompt030 changes no Rust, core, generated FFI, or Rust build files.
- **Real Keychain round-trip:** The existing Simulator case is expected to remain skipped because the unsigned test process lacks a Keychain entitlement. Its result will be reported from this Prompt030 run; no pass is presumed.
- **Physical-device validation:** `NOT_AVAILABLE`.

## Hosted Delivery Evidence

The feature branch is prepared for a pull request targeting `ios-app`. Hosted PR, exact-head workflow results, final feature commit, merge result, and resulting integration SHA will be recorded from GitHub after delivery. The repository's P029 evidence workflow uses a manifest finalization follow-up after the feature merge because the resulting hosted integration SHA does not exist before merge; this manifest will be completed through that established workflow if required.

- **Final feature SHA:** Pending.
- **Feature PR URL/number and target:** Pending; target must be `ios-app`.
- **Exact-head iOS Static Validation:** Pending.
- **Exact-head iOS Build:** Pending.
- **Exact-head iOS Simulator Tests and pass/skip/fail counts:** Pending.
- **iOS Rust Apple Build:** Not applicable to changed paths.
- **Hosted merge state and feature merge integration SHA:** Pending.
- **Unrelated CI failures:** Pending hosted workflow review.
- **Prompt031 readiness:** Pending genuine hosted merge confirmation.
