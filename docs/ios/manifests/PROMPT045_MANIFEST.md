# Prompt045 — Secure Resumable Upload & Content Replacement

## Baseline and publication record

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Verified `origin/ios-app` start SHA: `3b145172b625893894732a404c306b6f0cfdb736` (P044 integration).
- The baseline passed the required P044 ancestry check before implementation. Work is on
  `ios/p045-resumable-file-upload`; no `main` base, `codex/` branch, hard reset, or unrelated
  working-tree change was used.
- Prompt045 feature commit, PR, exact-head CI runs, merge state, and resulting integration SHA are
  pending genuine GitHub publication and verification. They will be recorded after those events.

## API contract and implementation

The implementation follows `api/openapi.yaml` and the current server in
`crates/api/src/uploads.rs` / `crates/storage/src/uploads.rs`:

- `POST /api/v1/upload-sessions` creates `CREATE_FILE` or `REPLACE_CONTENT`; the UUIDv7
  idempotency key is also the server UploadSession ID. The server returns relative `Location`,
  authoritative `Upload-Offset`, and the strict session resource. A lost create response is
  recovered with the same canonical request bytes and key.
- `GET /api/v1/upload-sessions/{id}` supplies the authoritative status and offset. Both the
  response header and `received_bytes` must agree.
- `PATCH /api/v1/upload-sessions/{id}` sends raw `application/octet-stream` data at an exact
  canonical offset. A successful 204 must contain the exact accepted offset and a valid request
  ID. After any ambiguous result, the client performs status reconciliation before another PATCH.
- `POST .../{id}/complete` has no request body. The client publishes Uploaded only after strict
  completion evidence is persisted and read back. A lost response is resolved through status GET
  on the original session. No new session or completion retry is sent automatically.
- `POST .../{id}/abort` is explicit and status is reconciled if its response is ambiguous. Device
  Bearer is used without BrowserSession cookies or CSRF. Raw transfer uses an ephemeral foreground
  URLSession transport, finite timeouts, no shared cache/cookies, HTTPS origin construction, and
  redirect rejection. No background URLSession or retry loop was added.

The wire models accept only the two supported operations and reject unknown fields, malformed
identities, noncanonical decimal values, mismatched target/hash/length, header/body offset
disagreement, untrusted Location values, invalid request IDs, and noncanonical completion data.
The target revision for replacement is frozen in the persisted canonical request. A stale target
revision remains a conflict; the client does not rewrite it.

## Source staging, storage, and recovery

SwiftUI uses `.fileImporter` for one ordinary file. The selected external URL is used only during
the import operation; security-scoped access is released with `defer`, and `NSFileCoordinator`
coordinates the read. The source descriptor is opened without following symlinks and must be a
regular single-link file. Source metadata is compared before and after copying to detect changes.

The source is copied to an opaque `.upload` file in the existing private transfer staging root.
Import uses a 64 KiB buffer, calculates the complete SHA-256 and exact length, synchronizes the
file and directory, applies existing file-protection/backup-exclusion policy, and atomically
publishes the copy. External paths and bearer credentials are not persisted. A resumed transfer
rehashes its staged source before network work, and every chunk is read only from that staged
copy.

Uploads use the existing `DownloadStagingStore` accounting boundary. Downloads, verified files,
P044 export copies, uploads, reservations and unrecognized files in the managed staging root share
the 512 MiB budget and 64 MiB free-space reserve. Checked arithmetic and serialized reservations
fail closed. No verified download or unresolved evidence is evicted to make room. Failed cleanup
remains charged.

The existing SQLite database moves transactionally from schema v6 to v7 with a dedicated
`file_uploads` table and immutable identity/no-rewind triggers. The v1–v6 upgrade paths are kept;
unknown future versions and altered schemas still fail closed. Existing mutation requests/history,
feed and ACK evidence, projections, rebaseline proof, downloads, verified versions, export state,
and quarantine are preserved. The table stores the exact canonical request bytes, typed target,
scope and credential identifiers, original key, staged identity, server session, authoritative
offset, uncertainty, completion evidence, safe failure category, and cleanup status.

An existing filesystem transfer lease is shared with downloads, and SQLite compare-and-set updates
protect session identity, offset, attempts and operation ownership across connections. Server
offsets are committed before UI progress changes. Restart and saved-record loading perform local
work only; Resume, Check Server Status, and Abort are explicit user actions. Logout/session
invalidation cancels active work, fences late responses, and quarantines persisted state.

## Native workflows

- **Upload File:** available for a live active directory, selects one file, confirms the destination
  name, and uploads only after explicit confirmation. A committed Node appears only after a live
  refresh.
- **Replace File Content:** available for a live active file, verifies the current Node and exact
  revision before session creation, requires explicit confirmation, and explains that the server
  creates a new immutable FileVersion. The original revision is retained after a conflict.
- **Pause** cancels foreground work while preserving staged and server identity. **Abort Upload**
  requires a separate destructive confirmation. Recent records show server-confirmed byte offsets,
  local phase, and safe error categories with Resume/Check/Abort actions where allowed.
- UI actions are session-fenced and expose accessibility labels, hints and identifiers; native
  SwiftUI layouts support Dynamic Type. No automatic upload, inbound ACK, mutation-queue bypass,
  or optimistic canonical Node insertion was added.

## Source inventory

New production files:

- `clients/ios/Application/Transfer/AuthenticatedUploadRepository.swift`
- `clients/ios/Application/Transfer/FileUploadCoordinator.swift`
- `clients/ios/Domain/Transfer/UploadModels.swift`
- `clients/ios/Domain/Transfer/UploadSessionDTO.swift`
- `clients/ios/Features/Transfer/FileUploadControls.swift`
- `clients/ios/Features/Transfer/FileUploadViewModel.swift`
- `clients/ios/Infrastructure/Network/URLSessionRawUploadTransport.swift`
- `clients/ios/Infrastructure/Persistence/FileUploadSQLiteSchema.swift`
- `clients/ios/Infrastructure/Transfer/UploadStagingStore.swift`

New tests:

- `clients/ios/Tests/SynveilTests/FileUploadTests.swift`
- `clients/ios/Tests/SynveilTests/FileUploadStorageTests.swift`
- `clients/ios/Tests/SynveilTests/FileUploadViewTests.swift`
- `clients/ios/Tests/SynveilTests/FileUploadCoordinatorTests.swift`

The implementation also updates app composition, the authenticated request provider, shared
staging and SQLite, Node Browser/File Details, Xcode target membership, and schema-version
expectations in existing tests. The portable source-registration checks cover every new Swift file
and the intended app/test target membership.

## Validation evidence

Observed in the Linux workspace at the current local tree:

- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**.
- `python3 -m unittest discover -s clients/ios/Support/tests`: **79 passed**.
- `bash scripts/validate-docs.sh`: **PASS**.
- `git diff --check`: **PASS**.
- Native Swift/Xcode compilation and XCTest were **not available locally**; the workspace has no
  `swift`, `xcodebuild`, or `plutil` command. No Simulator or filesystem XCTest result is claimed
  from Linux.
- Thirteen new XCTest methods cover canonical request bytes, strict responses and states, offset
  validation, raw route restrictions, file-backed schema/reopen/migration/completion readback,
  a deterministic 16 MiB + 123 byte staged source, multi-chunk reconstruction, quota failure,
  native rendering, lost CREATE recovery, partial PATCH acceptance, authoritative GET recovery,
  and lost COMPLETE recovery. These native tests are authored and registered; execution is pending
  hosted macOS CI.
- Native Keychain/Data Protection skip status is pending that run. Physical-device validation is
  `NOT_AVAILABLE`. Real Files-provider integration, production-server upload, hardware Keychain,
  physical Data Protection, background transfer, cellular behavior, and physical power-loss
  guarantees are not claimed.

## Exact-head CI, merge, and remaining limitations

Required GitHub Actions evidence for **iOS Static Validation**, **iOS Build**, **iOS Simulator
Tests**, and **iOS Rust Apple Build** is pending. The final table will identify each run and exact
source SHA. Unrelated workflow failures will be reported separately and will not be counted as iOS
evidence.

| Record | Verified value |
|---|---|
| Feature implementation SHA | Pending commit |
| GitHub PR targeting `ios-app` | Pending creation |
| Exact-head required iOS CI | Pending hosted runs |
| PR state / `merged` | Pending GitHub confirmation |
| P045 resulting `ios-app` SHA | Pending GitHub confirmation |
| Feature source/tests/manifest on `ios-app` | Pending post-merge verification |

Remaining limitations include the platform's normal cache-eviction risk for `Library/Caches`, no
background uploads, and any hosted/device limitations reported by the eventual CI run. Prompt045
must not be marked ready until GitHub confirms the PR is merged and the integrated source,
tests, and manifest are verified on `ios-app`.
