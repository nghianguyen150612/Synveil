# Prompt044 — Verified File Export, Native Preview & Secure Sharing

## Baseline, recovery and publication lineage

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Required P043 integration baseline and original Prompt044 start SHA: `020893864238dac9750eb4a9747c5d051fbc31a6`.
- The P043 baseline is an ancestor of `ios-app`.
- This resume inspected a clean checkout on `ios/p044-native-presenter-tests` at
  `7f22131e03ff1898b82647ce0897432cd612bdb2` before editing. The existing feature work and PRs
  were retained.
- The original feature branch was `ios/p044-verified-file-export`. Its recovered implementation history is `911e30a` (feature), `df21045` and `8a2ae92` (formatting), and `fda8b6d` (session lifecycle revision). These commits were preserved rather than regenerated.
- Original feature PR [#121](https://github.com/nghianguyen150612/Synveil/pull/121) targeted `ios-app` from `fda8b6d3afcd06732f5642ac92ac6fc4cbe33f12`; GitHub reports merged at `2026-10-11T09:17:23Z`, integration commit `1bf89dc1438601ba3ebac89061c5a43c300bff71`.
- Resume review found that concurrent callers could both inspect a lease before asynchronous file revalidation. Follow-up PR [#122](https://github.com/nghianguyen150612/Synveil/pull/122), source `feb2b8689ca26fa442c0c3c0cd632eab5d49e42b`, consumes the lease before suspension and fails closed on revalidation/session invalidation. GitHub reports merged at `2026-10-11T09:39:24Z`, integration commit `d94f43dbc5ff253c6e2a436339fc50e0199e0e92`.
- The resume checkout recovered the open native-presenter test PR [#123](https://github.com/nghianguyen150612/Synveil/pull/123), initially at `7f22131e03ff1898b82647ce0897432cd612bdb2`. Final source head `ea8fff05fd84d95ad0931dd29164812711f41fbf` adds actual presenter construction, cancellation callback, active-session-invalidation and window-hosted simulator coverage. PR #123 merged at `2026-10-11T10:43:43Z` as `6958d6408bd14ede70983a49ac0e272f0dc8d85d`.
- Work completed during this resume: corrected the Quick Look test to use `QLPreviewItem.previewItemURL`, added cancellation/dismissal callback cleanup coverage, and added invalidation coverage after a presentation lease is active.
- The P043 ancestor check passed. No `main` base, hard reset, destructive cleanup or `codex/` branch was used.

## Verified capability and trust boundary

`FileDownloadCoordinator.verifiedFile(library:node:version:)` is local-only. It requires the exact durable `VERIFIED` download record, authenticated scope and credential identity, immutable Library/Node/FileVersion match, and a fresh length plus full SHA-256 verification of the actual private file. It performs no content or metadata HTTP request. SQLite state alone is not sufficient authority.

`VerifiedFileExportCoordinator` issues opaque, purpose-bound one-use leases for Quick Look, Share and Save to Files. A lease binds the source operation, Library/Node/FileVersion identity, digest, length and session lifecycle revision. It contains no bearer credential and no source URL. Lease membership and authority remain inside the coordinator. The final handoff revalidates the temporary copy and current session. A lease is consumed before the first asynchronous suspension so concurrent calls cannot authorize two handoffs.

## Temporary copies, filesystem safety and quota

Only a separate presentation copy under `Library/Caches/staging/exports/` reaches native frameworks. The original `.verified` cache URL is never passed to Quick Look, the activity controller or the document picker, and the original file remains unchanged.

Exports use opaque UUID directories, a fixed incomplete filename and a sanitized display basename. Descriptor-relative `openat` operations use `O_NOFOLLOW`; the implementation checks regular files and single-link ownership, uses restrictive permissions, requests the existing Data Protection policy, excludes files from backup, streams through a 64 KiB buffer, and publishes with same-directory atomic rename and directory/file synchronization. The source is hashed while copying; byte count and digest are checked, then the sealed destination is reopened and hashed before presentation.

Downloads, reservations and exports share the 512 MiB staging ceiling and 64 MiB free-space reserve. Reservations are serialized. Failed deletion remains charged by inventory; startup cleanup removes only recognized opaque orphan export directories. No SQLite schema change was needed.

`SafeExportFilename` normalizes Unicode, replaces path separators and controls, handles empty/hidden/extension-only names, bounds UTF-8 length and does not use Content-Disposition as a path.

## Native presentation, UI and lifecycle

- **Quick Look:** `QLPreviewController` receives the verified temporary copy. Uniform Type Identifiers and conservative PDF/JPEG/PNG/plain-text signature checks determine preview eligibility. Unsupported, HTML, script, executable and unknown content does not use a custom renderer.
- **Share:** an explicit tap presents `UIActivityViewController` with the temporary URL. Completion and cancellation callbacks release local staging. Synveil does not claim it can revoke bytes already transferred to another app or that a recipient imported them.
- **Save to Files:** an explicit tap presents `UIDocumentPickerViewController(forExporting:asCopy: true)`. Cancellation/completion callbacks release staging. Picker success is not represented as cloud-provider synchronization.
- File Details and snapshot information expose Preview, Share and Save to Files only for a current verified capability. Existing download, cancel, resume, restart, progress, verification and error behavior remains available. Names are multiline; controls have accessibility labels, hints and identifiers and use native SwiftUI layout/Dynamic Type.
- Logout, credential replacement, Library/server scope change or lifecycle invalidation revokes leases and stops any new handoff. Preparation cancellation, preview dismissal/backgrounding and view dismissal trigger cleanup. Cleanup is best effort and failed cleanup stays charged.
- Preview/export are local after verified capability acquisition. They do not trigger content GETs, uploads, background transfer, synchronization, mutation submission, ACK, rebaseline or public sharing links.

## Source inventory

Recovered production changes are in `AppDependencyContainer.swift`, `RootView.swift`, `SynveilApp.swift`, `AuthenticatedLibraryRequestProvider.swift`, `FileDownloadCoordinator.swift`, `VerifiedFileExportCoordinator.swift`, `DownloadModels.swift`, `LibraryCatalogView.swift`, `NodeBrowserView.swift`, `NodeFileDetailsView.swift`, `SnapshotNodeInformationView.swift`, `FileDownloadControls.swift`, `FileExportPresentation.swift`, `DownloadStagingStore.swift` and `Synveil.xcodeproj/project.pbxproj`. `VerifiedFileExportTests.swift` is registered in the existing XCTest target. The follow-up test PR makes `NativeFilePresenter` module-internal for `@testable` hosting; it changes no public API or Xcode registration.

The implementation adds no new persistence schema, dependency, credential store or network endpoint. The following features remain excluded: upload, background transfer, File Provider, PhotoKit, automatic sync/mutation/ACK/rebaseline, public links, conversion/editing and an untrusted HTML/executable renderer.

## Test coverage

The final `VerifiedFileExportTests` suite contains 15 methods:

- Local verified-capability acquisition without content requests; PARTIAL rejection; wrong Library, Node and FileVersion rejection.
- Bounded 16 MiB streaming copy, safe filename, exact bytes and preservation of the original; same-length destination tampering rejection.
- Shared quota, orphan recovery, path/name rules, conservative preview type/signature policy and forged/reused lease rejection.
- Concurrent one-use handoff; invalidation before handoff and while a presentation lease is active.
- Actual Quick Look, Share Sheet and document-picker controller construction from the verified temporary URL; native dismissal/cancellation callback wiring and temporary-copy cleanup.

The native callback test invokes cancellation/dismissal callbacks directly. It does not pretend to exercise recipient import, a third-party app, or Files-provider synchronization. P024–P043 regression tests remain in the full Simulator suite.

## Local validation

Observed at the final source tree (`ea8fff0`):

- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**.
- `python3 -m unittest discover -s clients/ios/Support/tests`: **77 passed**.
- `bash scripts/validate-docs.sh`: **PASS**.
- `git diff --check`: **PASS**.
- Hosted builds use the Swift 6.3.3 compiler with the existing Xcode target setting
  `SWIFT_STRICT_CONCURRENCY=complete`; the project's Swift language mode remains 5.0.
- This Linux workspace has no Xcode toolchain; native Swift formatting, build and XCTest evidence is from the hosted macOS workflows below.

## Exact-head hosted validation

Required workflows run on final source head `ea8fff05fd84d95ad0931dd29164812711f41fbf`:

| Gate | Run | Result |
|---|---:|---|
| iOS Static Validation and strict Swift formatting | [38131994374](https://github.com/nghianguyen150612/Synveil/actions/runs/38131994374) | **PASS** |
| iOS Build | [38131994411](https://github.com/nghianguyen150612/Synveil/actions/runs/38131994411) | **PASS** |
| iOS Simulator Tests | [38131994528](https://github.com/nghianguyen150612/Synveil/actions/runs/38131994528) | **PASS** |
| iOS Rust Apple Build | [38131994428](https://github.com/nghianguyen150612/Synveil/actions/runs/38131994428) | **PASS** |

The Simulator run used Xcode 26.6 / Swift 6.3.3 on macOS 26.6.2 and an iPhone 17 Pro with iOS 26.5. It executed 1,638 tests: **1,636 passed, 2 skipped, 0 failed**. All 15 `VerifiedFileExportTests` passed. The uploaded `ios-simulator-test-results` artifact is ID `11700285268`, 56,868,238 bytes, SHA-256 `79bb554832e28b38173b0692e18f311d7e78a0aed5e5c2009d853d403791282b`. Physical-device validation: **NOT_AVAILABLE**.

The first native-presenter attempt at `7f22131e03ff1898b82647ce0897432cd612bdb2` failed Simulator compilation because the test treated `QLPreviewItem` as an `NSURL`; that assertion was corrected to check `previewItemURL`. Run [38130835096](https://github.com/nghianguyen150612/Synveil/actions/runs/38130835096) on `538e6e3` then completed with 1,634 passed, 2 skipped and 2 failures: both native presenter tests had a headless `UIHostingController` and could not find the Quick Look controller. Mounting the host in a simulator `UIWindow` fixed both failures: exact-head run `38131994528` passed all 15 P044 tests and the full suite. No failing test was removed or disabled.

The two existing Simulator skips remain explicit:

- `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`: unsigned Simulator test process lacks the Keychain access entitlement.
- `MutationQueueSQLiteTests.testNativeDataProtectionAttributes`: Simulator filesystem does not expose Data Protection attributes; physical-device round-trip remains required.

No hardware Keychain, real-device Data Protection, real external sharing, provider synchronization, production-server transfer or physical power-loss guarantee is claimed.

## Earlier exact-head gates and other workflows

PR #121 source `fda8b6d3afcd06732f5642ac92ac6fc4cbe33f12` passed all four iOS gates: Static `38118705983`, Build `38118705967`, Simulator `38118705995`, Rust Apple `38118705972`. PR #122 source `feb2b8689ca26fa442c0c3c0cd632eab5d49e42b` passed Static `38128210126`, Build `38128210125`, Simulator `38128210128`, Rust Apple `38128210162`.

On PR #121's exact source head, non-required repository workflows showed failures for Rust CI (`38118705970`), PostgreSQL maintenance (`38118705986`), Linux AppImage (`38118705974`) and Linux packages (`38118705968`). Those statuses are not represented as iOS passes; the four required iOS gates passed. Subsequent non-iOS workflow statuses are outside the required P044 iOS gate and are not used as evidence here.

On prior source head `538e6e3`, Rust CI (`38130835058`) failed in existing cross-platform Rust/desktop checks (Windows-only Unix API assumptions, macOS IPC tests and a Linux `systemd-tmpfiles --dry-run` option unsupported by that runner). Linux AppImage (`38130835047`) failed its artifact audit because the desktop binary contains a host `/home/` build-path marker. These runs do not touch the P044 Swift changes; the PR #123 diff from its `ios-app` base contains only `FileExportPresentation.swift` and `VerifiedFileExportTests.swift`. PostgreSQL maintenance (`38130835070`) and Linux packages (`38130835042`) were still in progress on that head. Final-head non-iOS statuses are separate and are not used as P044 iOS gate evidence.

## Hosted integration and readiness

- PR #121 is merged into `ios-app` at `1bf89dc1438601ba3ebac89061c5a43c300bff71`.
- PR #122 is merged into `ios-app` at `d94f43dbc5ff253c6e2a436339fc50e0199e0e92`.
- PR #123 is closed with `merged=true`; exact-head gates passed and integration commit is `6958d6408bd14ede70983a49ac0e272f0dc8d85d`.
- The documentation-only finalization PR and resulting latest `ios-app` SHA are recorded in the final task report to avoid a self-referential commit SHA in this manifest.

Prompt044 is **not** marked ready until GitHub confirms the manifest finalization is merged and the integrated `ios-app` contents are verified.
