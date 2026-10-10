# Prompt041 Manifest — Offline Browser Integration & Cache Freshness UX

## Bootstrap and delivery state

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Target integration branch: `ios-app`.
- Required P040 ancestry SHA: `2516e5342caff69249be80184ed4d4f03d88cfc5`.
- Fetched `origin/ios-app` SHA at implementation start: `2516e5342caff69249be80184ed4d4f03d88cfc5`.
- The required P040 SHA is an ancestor of the fetched integration tip. The checkout was clean before branch creation. The unrelated initial `work` checkout at `3851ea11927e24255614cfc38adbaccbd345ca03` was not used.
- Feature branch: `ios/p041-offline-browser-freshness`, created from `origin/ios-app`.
- Frozen source feature SHA: `b9b39448397bb35701c7fea2d2022dfefaeb7e61`.
- Source PR: [#114](https://github.com/nghianguyen150612/Synveil/pull/114), base `ios-app`, marked ready before squash merge.
- Independent GitHub REST confirmation: `merged=true`, `state=closed`, exact frozen feature head, merged at `2026-10-10T15:18:30Z`.
- Source squash merge and hosted `ios-app` SHA before manifest finalization: `c89269544389a043f25a93f5c5642a8abbff3dd2`. The source feature and squash-merge trees both equal `27b3abc2c8a61f75f77d060c1bff3f91ba52e9a1`; the required P040 SHA remains an ancestor.
- Final hosted evidence is being recorded on documentation-only branch `ios/p041-manifest-finalization`, based on the verified source squash merge. The finalization PR identity and merged `ios-app` tip will be added after it is created and independently verified; no source or test files change in that PR.

## References inspected

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/manifests/PROMPT031_MANIFEST.md`
- `docs/ios/manifests/PROMPT032_MANIFEST.md`
- `docs/ios/manifests/PROMPT033_MANIFEST.md`
- `docs/ios/manifests/PROMPT037_MANIFEST.md`
- `docs/ios/manifests/PROMPT038_MANIFEST.md`
- `docs/ios/manifests/PROMPT039_MANIFEST.md`
- `docs/ios/manifests/PROMPT040_MANIFEST.md`
- Existing Node Browser, file-details, Library Catalog, authenticated Node repository, session controller, P039 SQLite projection, P040 scope capture, dependency composition, P037 mutation feature, and Android v0.1 cache/browser sources.

Android's Room cache/browser currently loads cached children before its live request and uses connectivity state for presentation. P041 does not adopt that precedence or its completeness assumptions: iOS attempts its existing authenticated live listing first and uses only P039's validated partial projection as saved metadata.

## Browser architecture and source policy

`OfflineNodeBrowserService` is the application boundary composed from the existing `NodeRepositoryProtocol`, `NodeProjectionRepositoryProtocol`, and `InboundSyncCoordinatorProtocol.scope(libraryId:)`. SwiftUI receives the service and existing live repository only; it receives no SQL store, credential, transport, sync, ACK, or mutation capability.

- A successful live listing, including a valid empty listing, is authoritative for that request and is published as `LIVE`.
- Only `.offline`, `.dnsFailure`, `.timeout`, `.serverUnavailable`, and HTTP 5xx permit fallback. Fallback first captures the current local authenticated scope, then reads the exact scoped P039 projection.
- Authentication, credential, Device revocation, origin, session, TLS, redirect, protocol, permission/status, cancellation, and other non-transient failures do not select cache fallback.
- A failed scope capture, scope mismatch, invalid row, or failed projection read cannot publish cached rows. During a transient fallback, projection failure returns the original live failure.
- Explicit **View Saved Items** calls the saved-only service mode. It does not call the live Node repository, Sync Now, ACK, or mutation submission. Cached child-folder routes preserve saved-only mode until the user explicitly refreshes from the server.
- **Refresh from Server** performs a live-only list. Verified success replaces saved content. A transient failure retains the already validated saved presentation with a warning; a security failure clears the current protected presentation.
- Source-aware ViewModel generations serialize source resolution and reject late results after cancellation, route replacement, or session lifecycle changes. Logout/recovery invalidates presentation and cancels owned tasks.

## Scope, root identity, and projection rules

P040 scope capture supplies the canonical origin, owner, registered Device, and selected Library identity from the currently authenticated session. P039's local Keychain/session validation and Rust canonical-ID checks remain the read authority. No cache read authenticates a logged-out session or causes an HTTP request.

The live root request keeps `.libraryRoot(rootNodeId:)`, so P031 omits `parent_id`. The cache read uses `parent.expectedParentId`, which is the actual Library root Node ID. Nested cached navigation keeps canonical Node IDs and the existing `NodeBrowserRoute`; names are never used as identity.

The P039 SQLite projection is reused without schema changes, cache hydration shortcuts, or completeness upgrades. Cached rows must remain complete canonical active Nodes with matching Library and parent identities. Sorting remains folders-first, numeric-aware case-insensitive name order, then Node ID. Reads are bounded at 500 rows; `hasMore` is surfaced as a truncated saved listing.

| P039 result | Browser presentation |
|---|---|
| `partial` | Partial saved listing; other server items may exist. Zero rows do not become an empty-folder state. |
| `missing` | No saved listing for this folder; this does not mean empty. |
| `staleKnown` | Stale/recovery warning; ordinary cached folder navigation is disabled. |
| `complete` | Accepted only when the projection itself proves complete provenance and the bounded result is not truncated. |
| `rebaselineRequired` | Recovery-required source banner; no edits or normal folder navigation. Safe existing canonical metadata may be displayed under the P039 scope policy. |

Incremental applied/confirmed positions are displayed separately with epoch and sequence when present. Neither position, a high watermark, nor `Node.updatedAt` is presented as a last-sync timestamp or whole-Library freshness guarantee. P039 currently keeps incremental projections partial.

## File details and mutation behavior

For cached file details, the browser's cached row is only a navigation identity. `OfflineNodeBrowserService.savedNode` rereads the selected ID through `NodeProjectionRepositoryProtocol.node(scope:nodeId:)` and requires exact scope, ID, active canonical complete metadata, FILE kind, expected parent, and a fresh check of every known route ancestor before publishing details. Present inactive, incomplete, purged, trashed, or mismatched ancestors block publication; a missing ancestor remains unknown under P039's existing ancestry policy. Missing or invalid selected records remain unavailable. Cached details show a saved/not-verified source indicator and offer explicit Refresh from Server; no content action was added.

Cached projection rows are read-only in the browser. New Folder, Rename, Move, and Trash controls remain unavailable for saved-only rows. P037's guarded operation-preparation flow remains available for previously loaded same-session P031 results after a transient refresh failure or cancellation; this in-memory path does not apply to SQLite-only rows. Pending Changes remains available when a stable live or previously loaded result is visible. P041 does not rewrite canonical projection rows from pending mutations or a live listing, and does not initiate Sync Now, ACK, mutation POST, or background work.

## Accessibility and privacy

The native SwiftUI List/NavigationStack hierarchy is reused. Live results have a persistent current-server source label; cached results have partial, missing, truncated, stale/recovery, local-applied and server-confirmed details; retained results have a previously-loaded label. File Details distinguish live, saved, and previously loaded metadata. These states have stable accessibility identifiers and explicit text. Source changes post a VoiceOver announcement. Cached text wraps for Dynamic Type and Unicode names. Bearer credentials, ACK material, SQL, and private records are not emitted to UI or logs.

## Source and test changes

Added:

- `clients/ios/Application/Node/OfflineNodeBrowserService.swift`
- `clients/ios/Tests/SynveilTests/OfflineNodeBrowserServiceTests.swift`
- This manifest.

Modified:

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Features/Node/NodeBrowserView.swift`
- `clients/ios/Features/Node/NodeBrowserViewModel.swift`
- `clients/ios/Features/Node/NodeFileDetailsView.swift`
- `clients/ios/Features/Node/NodeFileDetailsViewModel.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserViewTests.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserViewModelTests.swift`
- `clients/ios/Tests/SynveilTests/NodeProjectionSQLiteTests.swift`
- `clients/ios/Support/tests/test_node_browser_registration.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

Twenty-five XCTest methods were added: 22 focused browser/service cases, two native route/view cases (saved-view construction and unauthenticated-route rejection), and one real file-backed SQLite scope/reopen case. Coverage includes live/nonempty and empty precedence; each eligible transient failure and grouped forbidden credential, origin, TLS, redirect, protocol, cancellation and permission failures; exact root ID and query bound; local-only saved browsing; server-only refresh; cache-read errors; completeness downgrade, rebaseline and truncation; invalid parent identity; stale-route gating; original-error preservation; successful/transient/security refresh; cached folder navigation; canonical Unicode file details; incomplete/wrong-kind/parent-mismatched records; inactive-ancestor rejection; logout during a gated cache read; late cache-result fencing; and P039 reopen through the actual authenticated P040 scope capability.

The existing P024–P040 XCTest suites remain registered. This Linux execution environment has no local Swift compiler or Apple toolchain; all P041 XCTest and SQLite integration execution evidence below comes from hosted macOS Simulator CI.

## Local validation and hosted evidence

Final local validation on the frozen feature source:

- `python3 clients/ios/Support/validate_ios_sources.py`: passed.
- `python3 -m unittest discover -s clients/ios/Support/tests`: 70 passed.
- `git diff --check`: passed.
- `bash scripts/validate-docs.sh`: passed.
- `git diff --check`: passed.
- Local Swift/Xcode/Simulator execution is unavailable in this Linux environment. Strict Swift format, Xcode Build and native XCTest execution are reported from exact-head hosted macOS CI below.

## Final hosted delivery record

The frozen P041 source head is `b9b39448397bb35701c7fea2d2022dfefaeb7e61`. GitHub independently reports source PR [#114](https://github.com/nghianguyen150612/Synveil/pull/114) as `merged=true`, `state=closed`, and `draft=false`, merged at `2026-10-10T15:18:30Z`. The source squash merge was `c89269544389a043f25a93f5c5642a8abbff3dd2`; its tree exactly matches the tested source feature tree. `origin/ios-app` was fetched and verified at that SHA before manifest finalization, and the required P040 ancestry remains intact.

All required exact-head iOS workflows passed for the frozen feature SHA:

| Workflow | Exact-head GitHub run | Result |
|---|---|---|
| iOS Static Validation | [38062285715](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285715) | SUCCESS; strict Swift formatting and static validator passed |
| iOS Build | [38062285734](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285734) | SUCCESS |
| iOS Simulator Tests | [38062285728](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285728) | SUCCESS; 1,431 total, 1,429 passed, 2 skipped, 0 failed |
| iOS Rust Apple Build (explicitly dispatched) | [38062292811](https://github.com/nghianguyen150612/Synveil/actions/runs/38062292811) | SUCCESS; Apple targets, C header alignment and staged artifact bundle validated |

The Simulator result bundle ran on iPhone 17 Pro with iOS 26.5. `OfflineNodeBrowserServiceTests`, `NodeBrowserViewTests`, and `NodeProjectionSQLiteTests` passed. The real file-backed SQLite test `NodeProjectionSQLiteTests.testOfflineBrowserReadsReopenedFileBackedProjectionWithoutHTTP` passed, proving the reopened P039 projection was read through authenticated P040 scope without an HTTP request. The full registered P024–P040 regression suite ran in the same target and remained green. The test-result artifact is [ios-simulator-test-results, ID 11673463399](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285728), digest `sha256:d5b197774d9a80cbb99b5ecfea0cae51f3783855940ddd88ef89c20329b41694`.

Exactly two Simulator tests skipped, neither counted as passed:

- `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`: the unsigned Simulator test process has no Keychain access entitlement.
- `MutationQueueSQLiteTests.testNativeDataProtectionAttributes`: the Simulator filesystem does not expose Data Protection attributes; the test's requested policy is checked separately.

Physical-device validation: **NOT_AVAILABLE**. No hardware-backed Keychain, physical Data Protection round-trip, or physical power-loss durability result is claimed.

## Unrelated hosted CI snapshot

The source PR was merged after the required exact-head iOS gates above passed. Shared non-iOS workflows are outside P041's source scope; no unrelated server, desktop or packaging code was changed.

- Linux AppImage [38062285740](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285740): FAILURE because artifact inspection found a private/temporary `/home/` build path in `synveil-desktop`.
- Rust CI [38062285758](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285758): FAILURE. Clippy reports `clippy::double_must_use` at unchanged `crates/object-store/src/types.rs:95` and `:359`; Windows check/test and native desktop jobs also fail compiling Unix-only APIs in unchanged `crates/install-engine/src/appimage.rs`.
- PostgreSQL 17 scheduled-maintenance PR run [38062286315](https://github.com/nghianguyen150612/Synveil/actions/runs/38062286315): **pending at source merge**; its duplicate push run [38062282575](https://github.com/nghianguyen150612/Synveil/actions/runs/38062282575) failed. The PR run will be updated here if it completes before manifest publication.
- Linux native packages PR run [38062285730](https://github.com/nghianguyen150612/Synveil/actions/runs/38062285730) and duplicate push run [38062282564](https://github.com/nghianguyen150612/Synveil/actions/runs/38062282564): **pending at source merge**. No pass or failure is inferred from pending jobs.

Documentation-only manifest-finalization PR: [#115](https://github.com/nghianguyen150612/Synveil/pull/115), based on source squash merge `c89269544389a043f25a93f5c5642a8abbff3dd2`. Its final `closed`/`merged=true` state and resulting hosted `ios-app` tip are independently verified after merge and reported in the delivery response; a manifest cannot contain its own eventual squash-merge OID without changing that OID.

Remaining limitations: cold-start offline sign-in/session restoration is not added; no full snapshot/rebaseline, content cache, background work, automatic sync/ACK, or mutation drain is added. Physical-device checks are unavailable.

Prompt042 readiness after this documentation-only finalization is merged: **READY_FOR_PROMPT042**.
