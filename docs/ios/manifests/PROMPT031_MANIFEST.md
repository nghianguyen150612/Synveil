# Prompt031 Manifest — Node Browser API & Domain Models

## Goal, Baseline, and Scope

- Phase E: deliver the authenticated, read-only logical Node data foundation for later native folder browsing.
- Starting hosted `origin/ios-app`: **`8797ccd175499cbc32b3df4c35318b4683cdc2ba`**.
- Working branch: **`ios/p031-node-browser-api`**.
- Cloud checkout verification: reused the existing clean repository. Initial temporary `work` HEAD was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. `git remote -v` confirmed the requested GitHub repository; fetching `+refs/heads/ios-app:refs/remotes/origin/ios-app` succeeded. Both the fetched integration SHA and the expected ancestor check matched Prompt030. Created the feature branch directly from `origin/ios-app`; no implementation used `main` or the temporary checkout.
- No folder UI, breadcrumbs, mutation, enrollment exchange, transfer, preview, persistent cache, or automatic retry was added. P030 presentation is preserved. Single-node retrieval was unnecessary and remains deferred.

## Authoritative References

Inspected the current product contract, roadmap, platform mapping, P029/P030 manifests, and `api/openapi.yaml` (Node collection/resource/attributes, ParentId, Page, ResponseMeta, OpaqueId, U64Decimal, and error contracts). Reviewed P029 models/DTOs/provider/repository, P030 view and ViewModel, session controller, credential sink, Keychain store, Rust protocol/adapter, URLSession transport, and dependency composition. Android references: `NodeModels.kt`, `SynveilHttpTransport.kt`, and existing `NodeBrowserScreen.kt`. Server references: `crates/api/src/files.rs`, `crates/metadata/src/repository.rs`, and `crates/core/src/ids.rs`.

## Node Domain and Rust Validation

- `Node` contains existing `NodeId` and `LibraryId`, optional parent and typed file-version identity, exact `NodeRevision`, unchanged logical name, typed kind/state, required creation/update dates, optional trash/restore dates, and required Boolean purge eligibility. No credential, HTTP header, or Keychain payload enters the model.
- `NodeKind`: FILE / DIRECTORY. `NodeState`: ACTIVE / TRASHED / PURGING. Server fields are authoritative; unknown values fail. Domain decoding supports all documented lifecycle states; child-list publication accepts only active Nodes without trash timestamps.
- `NodeRevision` delegates to P029's exact canonical decimal validator and retains decimal text without fixed-width or floating-point conversion.
- Existing Rust-backed `LibraryId` / `NodeId` factories enforce canonical lowercase UUIDv7. Library and parent input IDs are revalidated at the repository boundary. Rust validates logical names using the existing UTF-8 bound; names remain exact metadata and never become filesystem paths.
- `FileVersionId` is a distinct type. Rust's `domain_id!` macro uses identical UUIDv7 syntax for NodeId and FileVersionId, so its existing bridge syntax validator is reused without casting a file-version identity to NodeId or adding FFI exports.
- `NodeFailure` aliases the existing typed authenticated-read `LibraryFailure` vocabulary. Both repositories use the same transport/error classifier and controller recovery handler, avoiding competing authentication semantics.

## Strict Wire Decoding

- Separate Node collection/resource/attribute DTOs reuse the common Page and response metadata wire shapes.
- Shape validation rejects unknown keys at every object level, missing fields, wrong types, malformed JSON, invalid identifiers/names/revisions/timestamps, invalid request IDs, wrong discriminator, duplicate IDs, and unsupported enums. A successful HTTP status alone cannot produce success.
- Optional parent/version/trash/deadline fields may be omitted. Every present value must be a valid string; explicit null is rejected because the OpenAPI schemas do not permit it. Required `purge_eligible` must be a JSON Boolean.
- P029's strict RFC3339/Foundation policy validates dates and offsets, including impossible calendar-date rejection. No client timestamps are invented.
- Page completion requires cursor absence. Continuing pages require a nonempty cursor within the bound. No partially decoded Nodes escape.

## Root and Directory Scope

- Public `NodeRepositoryProtocol.listChildren(libraryId:parent:)` takes `NodeParentScope.libraryRoot(rootNodeId:)` or `.directory(NodeId)`.
- Root requests use `GET /api/v1/libraries/{library_id}/nodes?limit=100` and **omit parent_id**. The selected Library's validated root ID is used only for checking response parentage.
- Explicit directories include exactly their validated `parent_id` on every page.
- The actual server resolves omitted parent to `Library.root_node_id` and selects active direct children whose `parent_node_id` equals that ID. Consequently root children need not have null parents. Both modes enforce exact returned parentage and Library ownership, reject the parent as its own child, and fail the entire request on scope violations.
- Valid empty directories return `loaded([])`. Duplicate display names are permitted; Node IDs remain authoritative.

## Pagination and Transport Bounds

- **100 Nodes/page; 64 pages; 4096 Nodes/directory; 512 Unicode scalars/cursor**, following OpenAPI maxLength and P029's policy.
- **1 MiB per response**, including error responses. Production shares the existing bounded streaming URLSession transport; the repository also checks injected response bodies.
- Cursors remain opaque. URLComponents query items safely encode values, including literal plus as `%2B` for form-style server parsing.
- Operation-local cursor sets compare exact UTF-8 bytes; duplicate IDs within/across pages and repeated cursors fail. Resource-limit, protocol, cancellation, or later-page errors never return incomplete success. No cursor state persists or crosses Library/directory/session operations.

## Authentication, Origin, and Lifecycle

- Extended P029's `AuthenticatedLibraryRequestProvider` with an internal Node protocol and typed child-list entry point. It remains the sole bearer composition boundary; repositories supply no raw URL or bearer arguments.
- Production injects the same provider, Rust bridge, Keychain store, session controller, and bounded trusted transport into both repositories. `AppDependencyContainer.nodeRepository` is exposed through its protocol and fails closed if security initialization fails.
- Every request uses the stored canonical HTTPS endpoint, preserving its base path, host and port. Headers are DeviceBearer Authorization, JSON Accept, identity Accept-Encoding, and the existing iOS User-Agent. No cookie, CSRF, enrollment token, body, or mutation is sent.
- Captured lifecycle revision and secure session identity are checked before HTTP, after HTTP, and after asynchronous Rust mapping before publication. Logout, credential removal/replacement, endpoint mismatch, recovery, and cancellation discard late data. Swift 6 actor isolation and deterministic continuations replace timing sleeps.
- Only validated documented 401 `authentication_failed` / `device_revoked` errors enter existing controlled SessionController recovery. Arbitrary 403/404 errors do not imply revocation. No repository deletes credentials.
- Typed outcomes distinguish unauthenticated, credential unavailable/invalid, origin mismatch, stale session, authentication rejection, device revocation, offline, DNS, timeout, TLS, redirect, 503, other HTTP statuses, content type, protocol, resource limits, repeated cursor, and cancellation. Failures contain no raw server text, credential, URL or response body; requests and scopes retain redacted debug descriptions.

## Implementation Files

Created:

- `clients/ios/Domain/Node/NodeModels.swift`
- `clients/ios/Domain/Node/NodeResponseDTO.swift`
- `clients/ios/Application/Node/AuthenticatedNodeRepository.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserTests.swift`
- `clients/ios/Support/tests/test_node_browser_registration.py`
- `docs/ios/manifests/PROMPT031_MANIFEST.md`

Modified:

- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`
- `clients/ios/Application/Library/AuthenticatedLibraryCatalogRepository.swift`
- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

All four new Swift files have app/test target membership and group registration. Existing tests and source checks were preserved.

## Local Validation

- **109 new deterministic Node XCTest cases** cover model/wire strictness, Rust validation, optionals, timestamp/calendar/name rules, root/directory scopes, complete/empty/multiple-page results, limits, opaque query encoding, duplicate IDs/cursors, security headers/origins, recovery classification, credential retention, and controlled HTTP/Keychain/Rust suspension races.
- Linux source/architecture validator: PASS.
- Python support tests, including PBX structure and production composition: **31 passed**.
- Documentation validation and `git diff --check`: PASS.
- Official Swift **6.2.3** recursive strict formatting and parsing of every iOS Swift file: PASS.
- Linux Swift 6.2.3 strict-concurrency compilation and real-Rust-linked XCTest harness: **341 passed, 0 failed**, including all **109 Node tests**, all 78 P029 catalog tests, all 29 P030 ViewModel tests, and session/logout/restoration/transport/Rust regressions. Temporary harness copies adapt synchronous actor-isolated XCTest declarations to async for Linux discovery. Apple SwiftUI construction, Security.framework, and native URLSession-dependent suites are excluded from Linux execution; repository source and tests are unchanged. The existing RustBridgeAdapter Int-bound warning remains pre-existing.
- Rust `cargo test -p synveil-ios-ffi`: **16 passed, 0 failed**.
- Linux execution is not native iOS or Simulator evidence.

## Hosted Delivery and Apple Validation

All four required iOS workflows passed on the exact final feature head **`441cf426435e9ada80dae905710b316d65b4dd25`**. GitHub REST verified each run's head SHA, completed status, and success conclusion before marking ready and merging.

- Final feature SHA: **`441cf426435e9ada80dae905710b316d65b4dd25`** (`feat(ios): add authenticated node browser API`).
- Feature PR: [#90](https://github.com/nghianguyen150612/Synveil/pull/90), target **`ios-app`**. Opened as draft, marked ready after exact-head validation, and squash-merged.
- iOS Static Validation: [push run 37901158225](https://github.com/nghianguyen150612/Synveil/actions/runs/37901158225), PASS. Apple Swift 6.3.3 strict formatting, all 31 support checks, and source/architecture validation passed. Duplicate [PR run 37901192340](https://github.com/nghianguyen150612/Synveil/actions/runs/37901192340) also passed on the same head.
- iOS Build: [push run 37901158202](https://github.com/nghianguyen150612/Synveil/actions/runs/37901158202), PASS, `BUILD SUCCEEDED`. Duplicate [PR run 37901192403](https://github.com/nghianguyen150612/Synveil/actions/runs/37901192403) also passed on the same head.
- iOS Simulator Tests: [push run 37901158283](https://github.com/nghianguyen150612/Synveil/actions/runs/37901158283), PASS, `TEST SUCCEEDED`. Result bundle on iPhone 17 Pro / iOS 26.5 / ARM64 reports **477 total, 476 passed, 1 skipped, 0 failed**. All **109 new Node tests passed**; existing P024 enrollment, P025 deterministic Keychain, P026 restoration, P027 logout, P028 recovery, P029 catalog, and P030 UI/ViewModel regressions remain green. Duplicate [PR run 37901192428](https://github.com/nghianguyen150612/Synveil/actions/runs/37901192428) was still running at feature merge and is not claimed passing in this evidence snapshot.
- iOS Rust Apple Build: [dispatch run 37901191023](https://github.com/nghianguyen150612/Synveil/actions/runs/37901191023), PASS on the same feature head, including Apple target compilation, C-header/export alignment, staged device/Simulator static artifacts, and artifact integrity verification. Explicitly dispatched even though no Rust source changed.
- Real Keychain round-trip: **SKIPPED**, `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`. Actual hosted log: “The unsigned Simulator test process has no Keychain access entitlement.” Deterministic injected Keychain tests passed; real Keychain integration is not claimed passing.
- Physical-device validation: **NOT_AVAILABLE**.
- Hosted feature merge: GitHub REST confirmed **`merged=true`, `state=closed`**, merged at **2026-10-09 07:59:59 UTC**.
- Resulting hosted `ios-app` feature integration SHA: **`f9fb31f089b4a578a96f721e1868a89128ffdaa2`**. Fresh fetch matched the hosted PR's merge SHA. Node production files and this manifest were verified in that integration tree; its entire `clients/ios` tree matched the tested feature head.
- Manifest finalization: documentation-only follow-up branch **`ios/p031-manifest-finalization`** starts directly from that verified integration SHA. It records already-observed evidence without changing tested iOS sources. Its own commit and resulting integration SHA cannot be embedded self-referentially; the genuine hosted follow-up PR provides that separate documentation integration record.
- Readiness: **READY_FOR_PROMPT032**, based on the confirmed hosted feature merge and all required exact-head iOS checks. Future UI can inject `NodeRepositoryProtocol`, pass the selected Library's root metadata for root validation, and use typed directory scope for subsequent browsing.

## Unrelated CI Evidence

- [Rust CI push run 37901158255](https://github.com/nghianguyen150612/Synveil/actions/runs/37901158255) failed in unchanged non-iOS code. The macOS client control/launch suite reported three `UnsafeEndpoint` failures (`crates/client/src/control.rs` and `launch.rs`). The Linux metadata install lifecycle suite failed `sysusers_tmpfiles_artifacts_match_authoritative_sources` because the runner's `systemd-tmpfiles` does not recognize `--dry-run`. The duplicate [PR Rust run 37901192470](https://github.com/nghianguyen150612/Synveil/actions/runs/37901192470) also failed. No Rust, client, metadata or packaging source changed in P031.
- [Linux AppImage push run 37901158278](https://github.com/nghianguyen150612/Synveil/actions/runs/37901158278) failed desktop artifact inspection: `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop`. Duplicate [PR run 37901192363](https://github.com/nghianguyen150612/Synveil/actions/runs/37901192363) also failed. No desktop or AppImage source changed.
- PostgreSQL 17 scheduled-maintenance and Linux native package runs were still running at feature-merge inspection and are not claimed passing. No unrelated CI work was added to P031.
