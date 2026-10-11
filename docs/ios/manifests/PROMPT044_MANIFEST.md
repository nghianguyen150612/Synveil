# Prompt044 — Verified File Export, Native Preview & Secure Sharing

## Repository bootstrap

- Repository remote: `https://github.com/nghianguyen150612/Synveil.git`.
- Required P043 integration baseline: `020893864238dac9750eb4a9747c5d051fbc31a6`.
- Actual starting `HEAD`: `020893864238dac9750eb4a9747c5d051fbc31a6`.
- Fetched `origin/ios-app`: `020893864238dac9750eb4a9747c5d051fbc31a6`.
- The required P043 ancestor check passed. The working tree was clean before implementation.
- Feature branch: `ios/p044-verified-file-export`.
- No implementation was taken from `main`; no reset or `codex/` branch was used.

## References and design

Inspected the iOS product contract, roadmap, platform mapping, Prompt033/041/042/043 manifests, P043 download models/coordinator/staging store, SQLite download lookup, File Details and snapshot UI, session lifecycle, app composition, and Xcode source registration. P043 remains the source of authenticated immutable FileVersion access and verified cache authority.

The local-only `verifiedFile(library:node:version:)` method requires the exact durable VERIFIED record, matching immutable Node/Library/version and current scope/credential, and an on-disk file whose length and SHA-256 match. It performs no HTTP request. A reopened VERIFIED record can mint a new short-lived capability for the current authenticated lifecycle.

`VerifiedFileExportCoordinator` creates opaque one-use leases for `preview`, `share`, and `saveToFiles`. Leases bind source operation, immutable version, SHA-256, byte length, and lifecycle revision; they carry no URL or credential. Lease membership remains in the coordinator and a forged, reused, or released lease cannot authorize a handoff.

## Temporary copy, filesystem, and quota

The P043 staging actor owns `Library/Caches/staging/exports`. Every copy uses a locally generated opaque UUID directory, a fixed incomplete filename, and a sanitized display basename. Descriptor operations use `openat`/`O_NOFOLLOW`, regular-file/single-link checks, private permissions, requested Data Protection, backup exclusion, bounded 64 KiB buffers, and same-directory atomic rename. The logical filename never determines the directory.

The staging actor serializes source copying against publication and discard. It hashes source bytes while copying, checks exact byte count and digest, fsyncs the destination, reopens and hashes the destination, then seals it. The original `.verified` file is never handed to a presenter or modified. A final destination verification and session/record check run immediately before native handoff.

Temporary copies and outstanding reservations share P043's 512 MiB ceiling and 64 MiB free-space reserve. Failed cleanup remains charged through on-disk inventory. Startup cleanup removes only recognized opaque orphan export directories; cleanup failures remain on disk and charged. No SQLite schema change was required.

`SafeExportFilename` replaces filesystem separators, Unicode separator lookalikes and control characters, normalizes Unicode, removes hidden leading dots, handles extension-only/empty names, and bounds names while retaining a safe extension when possible. Per-lease directories avoid basename collisions. Content-Disposition is not used as a path.

## Native presentation and session policy

- Quick Look uses `QLPreviewController` and receives only the verified temporary copy. Preview classification uses Uniform Type Identifiers plus conservative PDF/JPEG/PNG/plain-text signature checks. HTML, scripts, executables, and unknown types do not get a custom renderer. Unsupported files remain eligible for explicit Share or Save to Files.
- Share uses `UIActivityViewController` with an explicit user tap and the temporary file URL. Completion/cancellation callbacks trigger cleanup. Synveil does not record the selected recipient.
- Save to Files uses `UIDocumentPickerViewController(forExporting:asCopy: true)`. The picker reports its own result; Synveil does not claim provider cloud synchronization or durability.
- App backgrounding dismisses an active Quick Look preview. Logout, credential replacement, lifecycle change, or view dismissal fences a new handoff and revokes local lease state. Cleanup is best effort; copies already received by another application cannot be revoked.
- Preview/share/export are local after capability acquisition. They do not issue content, metadata, change-feed, ACK, rebaseline, upload, or mutation requests.
- File Details and snapshot information label these actions as the “Verified downloaded version”; they do not claim it is the latest server version.

## Changed files

Production changes are in `FileDownloadCoordinator.swift`, `DownloadModels.swift`, `AuthenticatedLibraryRequestProvider.swift`, `DownloadStagingStore.swift`, `VerifiedFileExportCoordinator.swift`, `FileExportPresentation.swift`, `FileDownloadControls.swift`, `NodeFileDetailsView.swift`, `SnapshotNodeInformationView.swift`, `NodeBrowserView.swift`, `LibraryCatalogView.swift`, `AppDependencyContainer.swift`, `RootView.swift`, and `SynveilApp.swift`. `Synveil.xcodeproj/project.pbxproj` registers both production files and the test file and links QuickLook. `VerifiedFileExportTests.swift` adds nine native XCTest methods covering local-only capability acquisition, partial-record rejection, bounded 16 MiB copy and safe naming, destination tampering, shared quota, orphan cleanup, forged/reused leases, filename normalization, and conservative preview typing.

## Validation evidence

Observed local Linux results:

- `python3 clients/ios/Support/validate_ios_sources.py`: PASS.
- `python3 -m unittest discover -s clients/ios/Support/tests`: 77 passed.
- `bash scripts/validate-docs.sh`: PASS.
- `git diff --check`: PASS.
- `xcodebuild` and the Swift compiler are unavailable in this Linux workspace. No Apple framework build, Simulator result, or Swift XCTest result is claimed here.

Native Simulator rendering/presentation tests, exact-feature-head hosted iOS Static Validation, iOS Build, iOS Simulator Tests, and iOS Rust Apple Build: **PENDING**. Physical-device validation: **NOT_AVAILABLE**. The two Keychain/Data Protection Simulator skips documented by P043 are historical P043 evidence and are not P044 test results.

## Publication and integration status

- Final feature SHA: **PENDING**.
- Feature PR targeting `ios-app`: **PENDING**.
- Exact-head hosted CI: **PENDING**.
- Hosted merge (`merged=true`, `state=closed`): **PENDING**.
- Resulting `origin/ios-app` SHA: **PENDING**.
- Unrelated CI failures: not evaluated for this feature head.
- Deferred: uploads, background downloads, File Provider, public links, PhotoKit, automatic sync, file conversion/editing, and physical-device validation.

Prompt044 is not yet reported ready for Prompt045. The readiness marker will be used only after hosted evidence confirms integration into `ios-app`.
