# Prompt043 — Secure File Download Foundation & Verified Local Staging

## Baseline and repository workflow

The clean Cloud checkout started on `work` at `3851ea11927e24255614cfc38adbaccbd345ca03`. The existing repository remote was verified as `https://github.com/nghianguyen150612/Synveil.git`. Fetching `origin/ios-app` resolved to `1d3e5e0a2433ce37edce4f8eac7cdc1f44c6ba6b`; the required P042 ancestor check passed and there were no intervening commits. Feature branch: `ios/p043-secure-file-download`. No implementation used `main`, no destructive reset was performed, and no `codex/` branch was created.

## Contract and architecture

References inspected: iOS product contract, roadmap, platform mapping, P031/P033/P038/P039/P040/P041/P042 manifests; `api/openapi.yaml`; `crates/api/src/versions.rs` and `downloads.rs`; `crates/storage/src/downloads.rs` and local range implementation; canonical core digest/identity types; Android transfer models/operations; native Node, credential, session, SQLite, projection, and details components.

An explicit foreground operation validates a live ACTIVE FILE Node in the selected Library, then fetches `GET /api/v1/versions/{version_id}`. Cached/snapshot selection still requires these online checks. The Node and version are distinct Rust-validated canonical UUIDv7 types. Strict envelopes reject unknown/missing keys, null, wrong types, invalid timestamps/request IDs, wrong resource/Node/version identity, noncanonical decimal lengths and noncanonical digests. Length remains an exact UInt64 parsed from the server's decimal string, never floating point. A present request-ID header must match the body.

`AuthenticatedLibraryRequestProvider` alone constructs the canonical HTTPS immutable version content URL, loads the current Keychain credential, and supplies DeviceBearer, octet-stream Accept and identity encoding. Scope binds endpoint, owner, Device, Library, Node, version, expected length/digest, non-secret credential identifier, original session UUID and opaque operation UUID. Live operations additionally capture the existing session lifecycle/revision and exact credential record. No bearer is persisted or presented to UI.

`URLSessionFileContentTransport` uses a foreground ephemeral session, finite 30-second request/300-second resource timeouts, no cookie storage, no shared cache, redirect rejection and AsyncBytes backpressure. Application content buffers are bounded to 64 KiB. It checks headers before invoking the staging sink, counts all body bytes, rejects overflow/EOF and explicitly cancels its task on sink/header failure. TLS verification remains unchanged. Protocol fixtures establish bounded application buffers, not a physical-device memory guarantee.

| Response | Required behavior |
|---|---|
| 200 | Exact immutable length, quoted canonical expected ETag, octet-stream, identity encoding, private/no-store; no Content-Range |
| 206 | Exact requested inclusive start/end/total and selected Content-Length; same expected ETag; multipart rejected |
| Range returns 200 | Reject before any append; retain durable prefix and require explicit recovery |
| 416 | Bounded strict error JSON `invalid_range` and exact `bytes */total`; no offset advance |
| 304 | Only dedicated conditional revalidation after local VERIFIED record, actual file length/hash and live scope checks; no missing-byte inference |
| Other errors | Bounded strict JSON never reaches a staging sink; authentication/device-revocation codes, source absence and server unavailability remain distinct |

No fresh/resume request sends If-None-Match or If-Range. Current Node changes cannot replace the pinned version. Resume re-fetches the original version metadata and rejects changed length/hash. Historical content requires current server authorization.

## Private staging, accounting and durable state

`DownloadStagingStore` owns `Library/Caches/staging/downloads`, with separate opaque `.part` and `.verified` identities. Neither logical names nor Content-Disposition become paths. Root ancestors reject symbolic links; descriptor operations use openat/O_NOFOLLOW, regular-file and single-link checks. Creation uses exclusive ownership, 0700 directories/0600 files, backup exclusion and `completeUntilFirstUserAuthentication`, matching P035. Physical iOS protection attributes are checked on device; Simulator protection support is not assumed. The system cache can be evicted and does not provide document permanence.

A separate kernel-owned foreground lease prevents duplicate writers across independent coordinators and SQLite connections, without holding a SQLite transaction during networking, and releases on process death. The actor serializes shared reservations and charges all durable downloads, private verified cache, partial bytes, unknown transfer files within the managed root (including future upload staging placed there) and uncommitted files. Policy: 512 MiB total and 64 MiB free-space reserve. Arithmetic overflow, reservation overlap and low space fail before content GET. Available space is checked during writing as well. No file-sized memory allocation or fixed preallocation occurs. Conservative retained records continue to charge quota; cache eviction/retention management is deferred.

The existing actor-owned SQLite connection migrates transactionally to schema v6. Exact v5 schema verification precedes new download objects. Existing v1–v4 upgrade paths are preserved; future v7 and altered schemas fail closed. Original mutation requests/UUIDs, attempt history, UNKNOWN/conflict evidence, feeds, ACKs, active/prepared snapshots, confirmed cuts and quarantine remain in their original tables. Download bytes and bearer credentials never enter SQLite.

Each checkpoint orders bounded write, fsync, SQLite FULL/WAL COMMIT, exact readback, scope validation, then progress publication. Persisted offsets include an actual incremental prefix SHA-256. Recovery truncates uncommitted tails, rejects missing/truncated/corrupt committed prefixes, rehashes the full prefix and resumes with the original ID and exact immutable Range. Reopen/status does not start network work. Lost COMMIT acknowledgment is resolved by exact readback.

States include PREPARED, DOWNLOADING, PARTIAL, VERIFYING, VERIFIED, CANCELLED, recoverable/permanent failure, integrity/storage/session/source failure. 100% transferred remains VERIFYING until a new full-file CryptoKit hash and exact length match, same-filesystem rename/directory fsync, and final SQLite COMMIT/readback. Empty files follow the same SHA-256 rule. A rename with lost acknowledgment or failed final COMMIT leaves recoverable private evidence; explicit resume validates and reconciles it without downloading again.

The narrow verified-file capability contains immutable identity and captured session, no generic URL. Reads require current session/credential, current durable VERIFIED record and actual length/full hash, return at most 64 KiB, and recheck authority before returning bytes. Missing/evicted/corrupt verified content becomes unavailable. Explicit restart discards private files and resets the same operation only after immutable metadata revalidation; there is no automatic digest retry.

Logout immediately fences capabilities and cancels the writer. After it stops, scopes are quarantined before best-effort physical deletion. Cleanup failure cannot grant access. Credential replacement and late callbacks fail session validation. The UI cancels work when its selected Library/file view disappears. Download storage initialization failure disables downloads independently of browsing, sync and mutation recovery.

## Native UI and exclusions

File Details and eligible saved snapshot information provide Download File, durable byte progress, a distinct Verifying phase, Downloaded and Verified, Cancel, Resume and explicit Restart Download. Eligibility is only a presentation hint; authoritative preflight remains required. Text supports Dynamic Type, multiline layout, VoiceOver labels/hints and updates. Status load is local-only. No speed/ETA, MIME type, export, preview, Quick Look, Share Sheet, upload, background scheduling, automatic retry, metadata mutation, ACK, queue drain or rebaseline is introduced by downloading.

## Validation and publication evidence

The following results are observed P043 evidence. Linux tests are separate from native Simulator execution.

Final source feature SHA: `fee2e4b8e852d2d04fcf168f2027e7265ec9c160`. Feature branch: `ios/p043-secure-file-download`. The Rust Apple workflow now also triggers on `clients/ios/**` so integration changes receive all four native gates automatically.

| New native XCTest suite | Cases |
|---|---:|
| FileDownloadMetadataTests | 26 |
| FileDownloadHTTPPolicyTests | 32 |
| FileDownloadRecoveryTests | 38 |
| FileDownloadStorageTests | 19 |
| FileDownloadTransportTests | 13 |
| FileDownloadViewTests | 3 |
| Total new native cases | 131 |

Native fixtures exercise real URLSession AsyncBytes through URLProtocol responses, a generated 16 MiB response with a sink bounded to 64 KiB, incremental CryptoKit hashing, the same response through the production coordinator/private filesystem/file-backed SQLite, valid 200/206/304/416 handling, redirect/TLS errors, reopen/resume, corrupted/truncated prefixes, unknown tails, injected write/fsync/rename/commit/read failures, publication reconciliation, quota, scope/logout fencing and SwiftUI hosting/rendering at the largest accessibility text size. Fixtures do not contact a production server. FileDownloadTestSupport contains helpers, not additional test cases.

The successful run includes the fixes identified by genuine native CI: CRLF validation checks Unicode scalars because CRLF can be one Swift grapheme, existing-directory normalization compares canonical paths rather than URL directory-slash metadata, and the rejected-redirect URLProtocol fixture completes its original response. No failing test was removed or disabled.

| Local validation at final source head | Observed result |
|---|---|
| Official Swift 6.2 portable suite | 1,216 executed; 1,198 passed; 18 platform skips; 0 failures |
| Download-only portable rerun | 115 passed; 0 skips; 0 failures |
| iOS static source/architecture validator | PASS |
| Python Support suite | 77 passed |
| Official strict Swift formatting and Swift syntax parsing | PASS |
| Documentation validation | PASS |
| git diff --check | PASS |

The portable harness uses production domain/application/persistence/staging logic with real temporary files and file-backed SQLite; harness copies replace CryptoKit imports with a local Swift Crypto counterpart. Apple URLSession, SwiftUI and Rust adapter seams are excluded or explicitly skipped. It adds no repository package dependency. The 18 Linux skips are not counted as passes or as native evidence.

| Exact-feature-head hosted gate | Run | Result |
|---|---|---|
| iOS Static Validation | [38113731710](https://github.com/nghianguyen150612/Synveil/actions/runs/38113731710) | PASS |
| iOS Build | [38113731694](https://github.com/nghianguyen150612/Synveil/actions/runs/38113731694) | PASS |
| iOS Simulator Tests | [38113731700](https://github.com/nghianguyen150612/Synveil/actions/runs/38113731700) | PASS |
| iOS Rust Apple Build | [38113731711](https://github.com/nghianguyen150612/Synveil/actions/runs/38113731711) | PASS |

The Simulator xcresult summary reports **1623 total: 1621 passed, 2 skipped, 0 failed**. Environment: Synveil · Built with macOS 26.6.2; iPhone 17 Pro, iOS 26.5 Simulator, arm64. Artifact: `ios-simulator-test-results`, ID `11692393491`, digest `sha256:bdbd838fc8bb4363300313db3e141cf1b146e3eafb9873dcd4f1b65b0c44f399`. The artifact's workflow head matches the final source SHA above.

The two existing native skips remain explicit:

- `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`: “The unsigned Simulator test process has no Keychain access entitlement.”
- `MutationQueueSQLiteTests.testNativeDataProtectionAttributes`: “The Simulator filesystem does not expose Data Protection attributes; requested policy is verified separately. Physical-device round-trip remains required.”

Physical-device validation: **NOT_AVAILABLE**. No real-device Data Protection, hardware Keychain, cellular behavior, production-server download or physical power-loss durability is claimed. Simulator fsync/SQLite/rename/fault fixtures prove the observed software behavior, not hardware crash guarantees.

## Hosted integration

[Feature PR #119](https://github.com/nghianguyen150612/Synveil/pull/119) targets `ios-app`; GitHub independently reports `merged=true`, `state=closed`, with squash integration commit `345419eea168bab7c5294eba46f1c0fb403e082f`. `origin/ios-app` was fetched and the integrated tree's source/tests/project registration/initial manifest were verified against the successful feature tree. The required baseline remains an ancestor.

This evidence-only manifest finalization is a separate PR targeting `ios-app`, following P042's pattern. It changes no app, test, schema or workflow source. The finalization PR's hosted merge result and resulting `ios-app` SHA are recorded in the task's final report, avoiding a self-referential commit SHA in this file. Readiness is reported only after that hosted merge and integrated-file verification.

## Unrelated CI and remaining limitations

The completed final-head [Rust CI push run 38113727812](https://github.com/nghianguyen150612/Synveil/actions/runs/38113727812) reports seven failing jobs and four passing jobs. Failures are Windows Check/Test/Qt desktop (Unix-only APIs in unchanged `crates/install-engine/src/appimage.rs`); Linux Qt desktop and Quality/Clippy (double-must-use errors in unchanged `crates/object-store/src/types.rs`, lines 95 and 359); macOS Test (three `UnsafeEndpoint` IPC failures in unchanged `crates/client/src/control.rs` and `launch.rs`, also observed in PR run 38113731722); and Ubuntu Test (`systemd-tmpfiles --dry-run` unsupported in the runner, `crates/metadata/tests/linux_install_lifecycle.rs:1493`). Ubuntu Check, macOS Check, Dependency policy and Web quality passed. The [Linux AppImage PR run 38113731679](https://github.com/nghianguyen150612/Synveil/actions/runs/38113731679) also failed its artifact audit because `synveil-desktop` contained a `/home/` build-path marker. PostgreSQL and Linux package workflows were still running at the finalization snapshot. These statuses are separate from the four successful exact-head iOS gates.

No desktop, server, installer or package implementation was changed for P043. The failing unrelated jobs are not claimed as passing. No blanket CI success is claimed.

Private staging is a revocable cache. Conservative retained records still charge shared quota; automatic cache eviction/retention management, background execution, uploads, export, preview/Quick Look, sharing, automatic retries and production-server/device end-to-end validation remain deferred. Full SHA-256 is rechecked on capability reads, favoring integrity over read latency. Application-owned chunk bounds do not measure URLSession/OS buffers or physical-device peak RSS. Native rendering tests cover construction/layout/rendering, not a manual VoiceOver audit.

## Source inventory

38 files relative to the required P042 baseline: 16 added, 22 modified. All eight production and six native test/support Swift files are registered in the appropriate Xcode targets; the new Python inventory guard verifies the full download membership.

| Status | Repository path |
|---|---|
| Modified | `.github/workflows/ios-rust-apple-build.yml` |
| Modified | `clients/ios/App/AppDependencyContainer.swift` |
| Modified | `clients/ios/App/RootView.swift` |
| Modified | `clients/ios/App/SynveilApp.swift` |
| Modified | `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift` |
| Added | `clients/ios/Application/Transfer/FileDownloadCoordinator.swift` |
| Added | `clients/ios/Domain/Transfer/DownloadModels.swift` |
| Added | `clients/ios/Domain/Transfer/FileVersionResponseDecoder.swift` |
| Modified | `clients/ios/Features/Library/LibraryCatalogView.swift` |
| Modified | `clients/ios/Features/Node/NodeBrowserView.swift` |
| Modified | `clients/ios/Features/Node/NodeFileDetailsView.swift` |
| Modified | `clients/ios/Features/Node/SnapshotNodeInformationView.swift` |
| Added | `clients/ios/Features/Transfer/FileDownloadControls.swift` |
| Added | `clients/ios/Features/Transfer/FileDownloadViewModel.swift` |
| Added | `clients/ios/Infrastructure/Network/URLSessionFileContentTransport.swift` |
| Added | `clients/ios/Infrastructure/Persistence/FileDownloadSQLiteSchema.swift` |
| Modified | `clients/ios/Infrastructure/Persistence/MutationQueueSQLiteStore.swift` |
| Added | `clients/ios/Infrastructure/Transfer/DownloadStagingStore.swift` |
| Added | `clients/ios/Support/tests/test_file_download_registration.py` |
| Modified | `clients/ios/Support/tests/test_inbound_sync_coordinator.py` |
| Modified | `clients/ios/Support/tests/test_library_catalog_view_registration.py` |
| Modified | `clients/ios/Support/tests/test_node_browser_registration.py` |
| Modified | `clients/ios/Support/tests/test_rebaseline_snapshot.py` |
| Modified | `clients/ios/Support/tests/test_sync_feed_foundation.py` |
| Modified | `clients/ios/Synveil.xcodeproj/project.pbxproj` |
| Added | `clients/ios/Tests/SynveilTests/FileDownloadMetadataTests.swift` |
| Added | `clients/ios/Tests/SynveilTests/FileDownloadRecoveryTests.swift` |
| Added | `clients/ios/Tests/SynveilTests/FileDownloadStorageTests.swift` |
| Added | `clients/ios/Tests/SynveilTests/FileDownloadTestSupport.swift` |
| Added | `clients/ios/Tests/SynveilTests/FileDownloadTransportTests.swift` |
| Added | `clients/ios/Tests/SynveilTests/FileDownloadViewTests.swift` |
| Modified | `clients/ios/Tests/SynveilTests/InboundSyncSQLiteTests.swift` |
| Modified | `clients/ios/Tests/SynveilTests/MutationQueueSQLiteTests.swift` |
| Modified | `clients/ios/Tests/SynveilTests/MutationRecoverySQLiteTests.swift` |
| Modified | `clients/ios/Tests/SynveilTests/NodeProjectionSQLiteTests.swift` |
| Modified | `clients/ios/Tests/SynveilTests/RebaselineSQLiteTests.swift` |
| Modified | `clients/ios/Tests/SynveilTests/RebaselineTestSupport.swift` |
| Added | `docs/ios/manifests/PROMPT043_MANIFEST.md` |

Prior safety assertions remain. Existing schema-version/future-schema expectations advance to v6/v7, old-schema fixture downgrades remove only new download objects, and the old unavailable-content presentation expectation reflects the now-supported download action.
