# Prompt041 Manifest — Offline Browser Integration & Cache Freshness UX

## Bootstrap and delivery state

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Target integration branch: `ios-app`.
- Required P040 ancestry SHA: `2516e5342caff69249be80184ed4d4f03d88cfc5`.
- Fetched `origin/ios-app` SHA at implementation start: `2516e5342caff69249be80184ed4d4f03d88cfc5`.
- The required P040 SHA is an ancestor of the fetched integration tip. The checkout was clean before branch creation. The unrelated initial `work` checkout at `3851ea11927e24255614cfc38adbaccbd345ca03` was not used.
- Feature branch: `ios/p041-offline-browser-freshness`, created from `origin/ios-app`.
- Implementation and hosted-delivery evidence: **pending**. This manifest will be finalized with a documentation-only follow-up after exact-head validation and source PR merge; no CI, PR, or merge result is claimed in this source change.

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
- `clients/ios/Tests/SynveilTests/NodeProjectionSQLiteTests.swift`
- `clients/ios/Support/tests/test_node_browser_registration.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

Twenty-four XCTest methods were added: 22 focused browser/service cases, one native saved-view construction case, and one real file-backed SQLite scope/reopen case. Coverage includes live/nonempty and empty precedence; each eligible transient failure and grouped forbidden credential, origin, TLS, redirect, protocol, cancellation and permission failures; exact root ID and query bound; local-only saved browsing; server-only refresh; cache-read errors; completeness downgrade, rebaseline and truncation; invalid parent identity; stale-route gating; original-error preservation; successful/transient/security refresh; cached folder navigation; canonical Unicode file details; incomplete/wrong-kind/parent-mismatched records; inactive-ancestor rejection; logout during a gated cache read; late cache-result fencing; and P039 reopen through the actual authenticated P040 scope capability.

The existing P024–P040 XCTest suites remain registered. No local Swift compiler or Apple toolchain is installed in this Linux execution environment; the new XCTest and SQLite integration sources have not been executed locally.

## Local validation and hosted evidence

At source PR preparation:

- `python3 clients/ios/Support/validate_ios_sources.py`: passed.
- `python3 -m unittest discover -s clients/ios/Support/tests`: 70 passed.
- `git diff --check`: passed.
- `bash scripts/validate-docs.sh`: passed after source manifest creation; rerun before publication.
- Swift formatting, Swift 6 type checking, portable XCTest, Xcode Build, and Simulator tests: pending hosted macOS CI. No Simulator or physical-device result is claimed here.
- Real SQLite browser-service XCTest: source added; execution pending hosted Simulator CI.
- Keychain/Data Protection native-test skips and physical-device status: pending hosted evidence; prior P040 skip information is not reused as P041 evidence.
- Exact-feature-head iOS Static Validation, iOS Build, iOS Simulator Tests, and applicable iOS Rust Apple Build: pending.

## Final hosted delivery record

The following fields will be filled only from GitHub evidence after the source PR and manifest-finalization PR complete:

- Frozen P041 source feature SHA: **pending**.
- Source PR number and URL, base `ios-app`: **pending**.
- Exact-head iOS CI run IDs and results: **pending**.
- Source PR ready/merged/closed status and merge time: **pending**.
- Documentation-only manifest PR number and URL: **pending**.
- Resulting hosted `ios-app` SHA with source and final manifest: **pending**.
- Unrelated CI failures, if any: **pending**.
- Physical-device status: **pending**.
- Remaining limitations: no offline sign-in/session restoration, no full snapshot/rebaseline, no content caching, no background work, and no automatic sync/ACK/mutation drain.

Prompt042 readiness: **not yet established**.
