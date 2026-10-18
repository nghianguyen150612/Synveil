# Prompt033 — Live Node Metadata and File Details Verification

## Goal and Baseline

Extend P032's native file browser with authenticated, live, read-only file metadata. Directory snapshots remain navigation context; only a successfully verified single-Node response becomes current File Details metadata. No mutation, content transfer, preview, persistence, authentication replacement, or server authorization change is included.

- Repository: `nghianguyen150612/Synveil`.
- Actual starting `origin/ios-app` SHA: `2139b972a309e407bcfccf21a5ddd8e37187a6e0`.
- Working branch: `ios/p033-live-node-metadata`, targeting `ios-app`.
- The initial clean cloud checkout was branch `work` at `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. It was not used for implementation. `git fetch origin +refs/heads/ios-app:refs/remotes/origin/ios-app` fetched the authoritative integration branch; `git merge-base --is-ancestor 2139b972a309e407bcfccf21a5ddd8e37187a6e0 origin/ios-app` passed. The feature branch was created directly from that verified integration tip. No hard reset, `main` implementation, or `codex/` branch was used.

## Authoritative References

Reviewed the iOS product contract, roadmap, platform mapping, P029–P032 manifests, and `api/openapi.yaml`, together with the existing Node models/decoder/repository, authenticated Library request provider, Node Browser routes/ViewModel/view, Library Catalog navigation, application composition/root, and SessionController. Android's `NodeModels.kt`, `SynveilHttpTransport.kt`, and `NodeBrowserScreen.kt` informed parity: logical metadata, canonical identity, exact revision strings, and the single-node route. The current explicit read-only instruction and OpenAPI authorization contract constrain this prompt even where the broader roadmap describes future mutations.

## API and Authentication Architecture

`GET /api/v1/nodes/{node_id}` (`getNode`, `IMPLEMENTED`, `DEVICE_CREDENTIAL`) returns `NodeResponse` with safe logical metadata and no content or storage-provider paths.

`NodeRepositoryProtocol.getNode(libraryId:nodeId:expectedParent:)` returns `NodeDetailsRepositoryResult`: `loaded(Node)`, `unavailable`, `inconsistent`, or `failed(NodeFailure)`. Cancellation, network, protocol, resource-limit, and authentication failures remain typed through P031's established `NodeFailure` alias.

The production path is `AuthenticatedNodeRepository` → existing `AuthenticatedLibraryRequestProvider` → validated Keychain session → existing bounded `URLSessionHTTPTransport`. The Node provider protocol adds `requestNode`; the shared provider factors its existing GET construction into one helper for both paginated and single-resource reads. `URLComponents` preserves the configured base path, omits collection query parameters for metadata, and binds the URL to the validated HTTPS endpoint's host and port. Only the provider constructs Authorization. The request is GET with no body, cookie, CSRF header, enrollment token, mutation, automatic retry, or content request. Scope and HTTP request debug descriptions stay redacted.

The unchanged production composition uses one Keychain store, one Node repository boundary, explicit 10-second request/15-second resource timeouts, and a 1 MiB streaming response limit for both success and error bodies. The repository/decoder independently bound responses. Existing transport cookie/cache suppression, TLS verification, and redirect rejection remain in force.

Direct create/rename/move/trash/restore routes are BrowserSession-only. No DeviceBearer mutation, browser-cookie workaround, or server authorization extension was added. Future mutation work must inspect the device-scoped synchronization contract.

## Strict Decoder Reuse

`NodeResponseDTO` models the single `data` resource and required `meta`. Single and collection envelopes both call the same `validateResourceShape` and `mapResource` functions. Collection pagination, duplicate-ID rejection, and collection limits remain intact.

The decoder rejects missing/unknown keys at envelope, resource, attributes, and metadata levels; malformed JSON; wrong JSON types or resource discriminator; invalid request IDs, canonical IDs, revisions, logical names, enums, timestamps, and boolean hints. Optional properties may be omitted but explicit null is rejected. Rust-backed Node/Library/version syntax and logical-name validation are reused through the existing bridge. Revision text retains exact decimal semantics, including values beyond machine-integer precision, with no numeric conversion. Timestamp parsing retains strict RFC3339 and calendar validation.

## Identity, Scope, and Changed Files

After decoding and a final Keychain/lifecycle validation, the repository requires the requested Node ID and Library ID, the exact expected parent ID, and a non-self parent relationship. Root scope uses `.libraryRoot(rootNodeId:)` with the validated real root ID, rather than expecting a null parent. A mismatch returns `inconsistent` without publishing the returned resource.

File Details requires `FILE`, `ACTIVE`, no trash/restore timestamps, and no purge eligibility. Unexpected kinds or lifecycle metadata return `unavailable`. The ViewModel repeats identity, Library, parent, kind, and lifecycle checks as defense against invalid injected repositories.

A valid rename, newer exact revision, changed update time, or current-version identity is accepted as authoritative. No equality with the old directory snapshot is required. The parent listing is neither refetched nor reordered. Moved, trashed, purging, replaced, and unavailable resources clear verified metadata. A valid HTTP 404 error becomes unavailable without revoking the device; the UI says, “This file is no longer available at its previous location,” without claiming a cause the protocol does not prove.

Documented `authentication_failed` and `device_revoked` classifications route through SessionController's existing recovery handler. Offline, DNS, timeout, and 503 failures retain credentials. No credential deletion occurs in metadata code.

## Native Details, Refresh, and Lifecycle

`NodeFileDetailsViewModel` is `@Observable` and `@MainActor`, injected with the existing repository and SessionController and holding immutable selected-route context. Its typed states are idle, loading, loaded, refreshing, refresh failed, unavailable, failed, cancelled, and invalidated. No raw response, credential, catalog copy, or disk cache is stored.

Initial loading displays progress without snapshot metadata. Success displays the server's current logical name, kind, created/updated dates, exact revision, Node and Library IDs, Library and folder context, canonical parent ID, and optional version ID. Content transfer remains explicitly deferred. Refresh uses only the same single-node GET. Duplicate simultaneous loads/refreshes are suppressed; progress clears on completion. Transient refresh failure can retain a previously successful same-session Node, explicitly labeled as previously loaded and unverified by that refresh. Security/protocol/scope/lifecycle failures and cancellation clear verified data.

The ViewModel captures the session lifecycle revision, owns and cancellation-forwards its repository task, and fences completions with a request generation. Session changes and navigation disappearance cancel/invalidate the destination. Rendering uses `presentationState`, which hides protected data immediately when the session revision/state changes, before observer delivery. The view observes both session state and lifecycle revision. Separate state-owned destinations keyed by the existing immutable route prevent File A from overwriting File B, even with identical filenames.

Library Catalog injects the existing Node repository and controller into the existing `NodeFileDetailsRoute` destination; no new NavigationStack/provider/store is created. Native Back navigation, existing root/directory scopes, sorting, duplicate-name distinction, pull-to-refresh, accessibility, and bounded collection browsing remain unchanged.

Dynamic Type-compatible text, flexible Unicode names, readable progress, semantic labels, status messages, stable `synveil.node.details.*` accessibility identifiers, and selectable safe identifiers are provided. No fabricated size, MIME type, checksum, Download, Rename, Delete, or Share action is shown.

## Files

Created:

- `clients/ios/Features/Node/NodeFileDetailsViewModel.swift`
- `clients/ios/Features/Node/NodeFileDetailsView.swift`
- `clients/ios/Tests/SynveilTests/NodeFileDetailsViewModelTests.swift`
- `clients/ios/Support/tests/test_node_file_details_registration.py`
- `docs/ios/manifests/PROMPT033_MANIFEST.md`

Modified:

- `clients/ios/Domain/Node/NodeModels.swift`
- `clients/ios/Domain/Node/NodeResponseDTO.swift`
- `clients/ios/Application/Node/AuthenticatedNodeRepository.swift`
- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`
- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Features/Node/NodeBrowserView.swift` (snapshot-only details view moved/replaced)
- `clients/ios/Tests/SynveilTests/NodeBrowserTests.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserViewModelTests.swift` (explicit fail-closed single-read test-double conformance)
- `clients/ios/Tests/SynveilTests/NodeBrowserViewTests.swift` (injected details composition)
- `clients/ios/Support/tests/test_node_browser_registration.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

All three new Swift production/test files are registered in their proper Xcode source targets and file groups. No Rust, Android, server, desktop, or workflow source is changed.

## Tests and Local Validation

- Added **52** single-node decoder/repository XCTest methods covering valid real Rust mapping, required/unknown fields, JSON types/nulls, identifiers, revisions, logical names, timestamps, enums, metadata, bounds, GET/auth/base-path construction, Keychain/origin/TLS/redirect fail-closed behavior, unavailable/scope/lifecycle cases, newer name/revision/version, credential retention/recovery, logout during HTTP/Rust validation, credential replacement, and cancellation.
- Added **26** File Details ViewModel XCTest methods covering initial/loading/success/unavailable/failure states, authoritative updates, duplicate suppression, refresh, transient retention versus security clearing, missing dependency, logout/recovery/replacement/navigation races, injected wrong scope/kind/state, Unicode, and native SwiftUI surface construction. **78 new XCTest methods** total.
- Added **5** Python composition/registration regressions. Full Python suite: **38 passed**.
- iOS source validator: **PASS**. Documentation validation: **PASS**. `git diff --check`: **PASS**.
- Downloaded official Swift 6.2.3 tooling into ignored `work/`. Changed/new Swift files pass strict formatting; all production/test Swift files pass syntax parsing. Domain/Application/Rust bridge and both Node ViewModels pass Swift 6 strict-concurrency type checking; Node decoder/repository and ViewModel test code also passes Linux type checking.
- Rust FFI regression suite (`cargo +1.88.0 test -p synveil-ios-ffi --lib`): **16 passed, 0 failed**; FFI static library build: **PASS**. No Rust source changed.
- Temporary Linux SwiftPM XCTest harness uses copied production sources and the real Rust FFI. Only Linux XCTest async-discovery compatibility and the unavailable SwiftUI import/view-construction fragment are adapted in temporary test copies; no such adaptation is committed. Result: **218 tests passed, 0 failed** (161 Node decoder/repository tests, 31 existing Browser ViewModel tests, and 26 File Details ViewModel tests). This is not Apple Simulator execution.
- Hosted Simulator Keychain round-trip: **SKIPPED**, with the log explicitly reporting that the unsigned process has no Keychain access entitlement. Deterministic injected Keychain tests passed. This limitation is not claimed resolved. Physical-device validation: **NOT_AVAILABLE**.

## Hosted Delivery Evidence

- Final feature SHA: **`f2ca24cad9c54de5126de5e74a18831279f3c356`** (`feat(ios): add live node metadata details`). No source fix commits were needed after publication.
- Feature PR: [#94](https://github.com/nghianguyen150612/Synveil/pull/94), targeting **`ios-app`**. The PR was opened in draft, its description updated with verified results, marked ready, and squash-merged. GitHub REST confirms **`merged=true`** and **`state=closed`**. Merge commit/resulting feature integration SHA: **`d7d84ebb0a33e5c933df93ac4ec14126be989842`**. A fresh fetch and the hosted branch API both matched that SHA; the merged tree contains the live details ViewModel/view, test file, and manifest.
- iOS Static Validation: push [37928319938](https://github.com/nghianguyen150612/Synveil/actions/runs/37928319938) and PR [37928327910](https://github.com/nghianguyen150612/Synveil/actions/runs/37928327910): **PASS**, each with the exact final feature SHA. Hosted recursive strict formatting and all source/registration checks passed.
- iOS Build: push [37928319985](https://github.com/nghianguyen150612/Synveil/actions/runs/37928319985) and PR [37928327772](https://github.com/nghianguyen150612/Synveil/actions/runs/37928327772): **PASS**, each with the exact final feature SHA.
- iOS Simulator Tests: push [37928320001](https://github.com/nghianguyen150612/Synveil/actions/runs/37928320001): **PASS**, with the exact final feature SHA. Native result-bundle summary: **596 total, 595 passed, 1 skipped, 0 failed**. Hosted logs independently confirm all **52** new single-node decoder/repository methods and all **26** File Details ViewModel methods passed. Existing P024–P032 regression suites remain green. The separate duplicate PR Simulator run [37928327802](https://github.com/nghianguyen150612/Synveil/actions/runs/37928327802) was still in progress at feature-merge inspection; no result for that duplicate is claimed here. The required exact-head Simulator evidence is the completed push run.
- iOS Rust Apple Build: explicitly dispatched [37928329647](https://github.com/nghianguyen150612/Synveil/actions/runs/37928329647): **PASS**, with the exact final feature SHA. Apple target checks, C header/export alignment, device/Simulator release static-library builds, staged bundle validation, and artifact upload completed.
- Skipped test: `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`. The log states: “The unsigned Simulator test process has no Keychain access entitlement.” No real Keychain success is claimed. Deterministic Keychain tests passed. Physical-device validation: **NOT_AVAILABLE**.
- Manifest finalization: immutable final-head and merge evidence is committed in documentation-only [PR #95](https://github.com/nghianguyen150612/Synveil/pull/95), branch `ios/p033-manifest-finalization`, from the verified feature integration tip, following P032's delivery pattern. The feature SHA and integration SHA above identify the fully verified source change; the follow-up's own commit/merge SHA cannot be embedded in itself. The final user report records the hosted branch tip after that follow-up.
- Prompt034 readiness: **READY_FOR_PROMPT034**, based on genuine hosted confirmation that P033 is merged into `ios-app` and all four required final-head iOS workflows passed. Future metadata mutations still require the appropriate device-scoped or owner-authorized contract.

## Unrelated CI Evidence

- Linux AppImage push [37928320012](https://github.com/nghianguyen150612/Synveil/actions/runs/37928320012) and PR [37928328028](https://github.com/nghianguyen150612/Synveil/actions/runs/37928328028): **FAIL**. Both logs report “private or temporary build path found in synveil-desktop,” with matched marker `/home/`. P033 changes no desktop/AppImage source; this existing class of failure remains outside scope.
- PostgreSQL 17 scheduled-maintenance push [37928319945](https://github.com/nghianguyen150612/Synveil/actions/runs/37928319945): **FAIL** in unchanged `crates/metadata/tests/backup_scheduler_tick_postgres.rs:405`. The schema-current assertion returned **36** versus expected **34**; that suite reports 24 passed and 1 failed. P033 changes no database, migration, server, or scheduling code.
- At finalization inspection, Rust CI push/PR runs were queued, Linux native-package push/PR runs were in progress, and the duplicate PR PostgreSQL workflow was in progress. No pass/fail result is claimed for unfinished unrelated workflows. No unrelated fixes were added.
