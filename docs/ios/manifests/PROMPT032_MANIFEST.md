# Prompt032 Manifest — Native Folder Navigation & Node Browser UI

## Goal, Baseline, and Scope

- Phase E: implement native, authenticated, read-only browsing of logical Nodes inside a selected Synveil Library.
- Starting hosted `origin/ios-app` SHA: **`76f074655b047522c36a53f6b4cb470c33e43d1f`**.
- Feature branch: **`ios/p032-native-folder-navigation`**.
- Codex Cloud checkout verification: reused `/workspace/Synveil`; remote URL matched `https://github.com/nghianguyen150612/Synveil.git`, and the worktree was clean on the temporary `work` branch at `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. Fetched `+refs/heads/ios-app:refs/remotes/origin/ios-app`, verified the fetched SHA equals the required P031 SHA, and verified P031 is an ancestor. Created the feature branch directly from that integration branch. No implementation used `main` or a `codex/` branch.
- Scope is read-only. No content transfer, mutation, preview, persistent cache, or invented filesystem path is added.

## Authoritative References

Inspected `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`, `docs/ios/IOS_V0_1_ROADMAP.md`, `docs/ios/IOS_PLATFORM_MAPPING.md`, P029–P031 manifests, and the Node collection/resource/attribute and `ParentId` contracts in `api/openapi.yaml`. Reviewed P031 Node models, DTO, authenticated repository and request provider; P030 Library Catalog view and ViewModel; `SessionController`; `AppDependencyContainer`; `RootView`; `SynveilApp`; and the iOS Xcode project and tests. Reviewed the Android `NodeBrowserScreen.kt` and `NodeModels.kt` as behavior references; iOS follows native hierarchical navigation while keeping P032 read-only.

## Navigation and Scope Architecture

- The existing Library Catalog `NavigationStack` owns the full route hierarchy. Selecting a `LibraryId` opens its root Node Browser. The root route retains the selected Library's ID, name, validated `rootNodeId`, typed parent scope, title, and known Node ID ancestry.
- The initial request passes `NodeParentScope.libraryRoot(rootNodeId:)`. P031 omits `parent_id` for this scope and uses the root ID only to validate returned parentage.
- Selecting a valid child directory creates a route using that Node's canonical ID and `NodeParentScope.directory(node.id)`. Nested routes carry the Library context, title, parent scope, and Node ID ancestry. Same-named folders remain separate destinations because identity uses IDs rather than names.
- A known ancestor cycle is rendered without an open-folder destination. Names remain display labels and are never used to build filesystem paths.
- File routes retain Library context, parent scope, directory title, ancestry, and safe file metadata. Native Back navigation returns to the containing directory or Catalog; no manual Back stack is added.

## Node Browser ViewModel and Loading

- `NodeBrowserViewModel` is `@Observable` and `@MainActor`, receives the existing `NodeRepositoryProtocol`, `SessionController`, Library context, and typed current parent scope. A missing repository fails closed.
- Typed states cover idle, loading, loaded, empty, refreshing, refresh failure with same-scope in-memory prior results, refresh cancellation, initial failure, cancellation, and invalidation. Empty success is distinct from all failures. Initial loading does not render an empty-directory message.
- A stable `.task` gate starts one initial listing. Refresh uses `.refreshable` and a native toolbar action. Concurrent load/refresh attempts are suppressed. Both operations call only `listChildren` for the model's immutable Library and parent scope.
- Operation identity includes the authenticated lifecycle revision, Library ID, parent scope, and a local generation. Each destination has a distinct model. Late results are discarded after invalidation or cancellation. Results are additionally checked for matching Library, expected parent, active lifecycle, and duplicate IDs before presentation.
- Nodes sort directories first, then case-insensitive numeric-aware names, then canonical Node ID. Logical names are unchanged. Duplicate names do not merge.
- Only temporary network failures and server 5xx refresh failures retain previously loaded in-memory Nodes for that same scope/session, with explicit stale-data copy. Protocol, scope, credential, origin, and security failures clear prior results. No logout is initiated by a transient failure.
- Logout/recovery changes the authenticated route identity and invalidates the ViewModel. Pending repository tasks are cancelled; late success and failure cannot restore Node data. The existing `RootView` lifecycle revision fence remains in place.

## Presentation, File Details, and Accessibility

- Folder and file rows use distinct SF Symbols, exact logical names, updated timestamps from the validated model, stable Node ID identity, accessible type/name labels and navigation hints, and flexible row heights.
- Native empty, loading, error, cancellation, retry, and refresh states use accessible labels and identifiers. Dynamic Type uses system typography and no fixed-width layout.
- Selecting a file opens a read-only native details page showing the actual name, kind, creation and update times, revision, Node ID, and Library/folder context. The page states that file content is not available. It has no transfer, preview, sharing, rename, delete, or move controls.
- No trashed or purging Nodes are displayed as active children. No persistent cache or offline snapshot was introduced.

## Files Created and Modified

Created:

- `clients/ios/Features/Node/NodeBrowserView.swift`
- `clients/ios/Features/Node/NodeBrowserViewModel.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserViewModelTests.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserViewTests.swift`
- `docs/ios/manifests/PROMPT032_MANIFEST.md`

Modified:

- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Support/tests/test_library_catalog_view_registration.py`
- `clients/ios/Support/tests/test_node_browser_registration.py`

The production `AppDependencyContainer.nodeRepository` and P031 authenticated repository are reused without creating a transport, credential store, or second Node client. All new Swift production and test files are registered in their proper Xcode targets.

## Tests and Local Validation

- Added **31** deterministic `NodeBrowserViewModelTests` for initial/load/empty/error/refresh state, duplicate suppression, scope forwarding, stale data policy, cancellation, session invalidation, late results, missing repository, safe copy, ordering, duplicate names, Unicode names, and Library isolation.
- Added **10** `NodeBrowserViewTests` for root and directory scope identity, nested route ancestry, same-name folder identity, cycle prevention, read-only file metadata, Library mismatch rejection, and SwiftUI surface construction.
- Python source-registration suite: **33 passed**. This includes PBX membership/target checks and source checks for production dependency injection, scopes, accessibility, and absence of fake content operations.
- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**.
- `python3 -m unittest discover -s clients/ios/Support/tests`: **33 passed**.
- `bash scripts/validate-docs.sh`: **PASS**.
- `git diff --check`: **PASS**.
- Local environment is Linux and has no Swift compiler, `swift-format`, Xcode, or iOS Simulator. Swift formatting, parsing, compilation, XCTest, Simulator, and Apple Rust build are therefore not claimed locally; hosted checks remain pending.
- Real Simulator Keychain round-trip status: **PENDING hosted Simulator evidence**. Physical-device validation: **NOT_AVAILABLE**.

## Hosted Delivery Evidence

This section will be completed from GitHub after the feature branch is pushed and its exact-head workflows, pull request, and merge are verified. No hosted evidence is claimed yet.

- Final feature SHA: **PENDING**.
- Pull request targeting `ios-app`: **PENDING**.
- iOS Static Validation: **PENDING exact-head GitHub Actions evidence**.
- iOS Build: **PENDING exact-head GitHub Actions evidence**.
- iOS Simulator Tests and test counts: **PENDING exact-head GitHub Actions evidence**.
- iOS Rust Apple Build: **PENDING exact-head GitHub Actions evidence or confirmed applicability**.
- Real Keychain round-trip: **PENDING hosted Simulator log**.
- Physical-device status: **NOT_AVAILABLE**.
- Hosted merge state and resulting `ios-app` SHA: **PENDING GitHub confirmation**.
- Unrelated CI failures: **PENDING inspection; no unrelated work is included in P032**.
- Prompt033 readiness: **PENDING hosted merge confirmation**.
