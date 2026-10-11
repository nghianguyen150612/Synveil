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

The actor serializes shared reservations and charges all durable downloads, private verified cache, partial bytes, unknown future upload staging and uncommitted files. Policy: 512 MiB total and 64 MiB free-space reserve. Arithmetic overflow, reservation overlap and low space fail before content GET. Available space is checked during writing as well. No file-sized memory allocation or fixed preallocation occurs. Conservative retained records continue to charge quota; cache eviction/retention management is deferred.

The existing actor-owned SQLite connection migrates transactionally to schema v6. Exact v5 schema verification precedes new download objects. Existing v1–v4 upgrade paths are preserved; future v7 and altered schemas fail closed. Original mutation requests/UUIDs, attempt history, UNKNOWN/conflict evidence, feeds, ACKs, active/prepared snapshots, confirmed cuts and quarantine remain in their original tables. Download bytes and bearer credentials never enter SQLite.

Each checkpoint orders bounded write, fsync, SQLite FULL/WAL COMMIT, exact readback, scope validation, then progress publication. Persisted offsets include an actual incremental prefix SHA-256. Recovery truncates uncommitted tails, rejects missing/truncated/corrupt committed prefixes, rehashes the full prefix and resumes with the original ID and exact immutable Range. Reopen/status does not start network work. Lost COMMIT acknowledgment is resolved by exact readback.

States include PREPARED, DOWNLOADING, PARTIAL, VERIFYING, VERIFIED, CANCELLED, recoverable/permanent failure, integrity/storage/session/source failure. 100% transferred remains VERIFYING until a new full-file CryptoKit hash and exact length match, same-filesystem rename/directory fsync, and final SQLite COMMIT/readback. Empty files follow the same SHA-256 rule. A rename with lost acknowledgment or failed final COMMIT leaves recoverable private evidence; explicit resume validates and reconciles it without downloading again.

The narrow verified-file capability contains immutable identity and captured session, no generic URL. Reads require current session/credential, current durable VERIFIED record and actual length/full hash, return at most 64 KiB, and recheck authority before returning bytes. Missing/evicted/corrupt verified content becomes unavailable. Explicit restart discards private files and resets the same operation only after immutable metadata revalidation; there is no automatic digest retry.

Logout immediately fences capabilities and cancels the writer. After it stops, scopes are quarantined before best-effort physical deletion. Cleanup failure cannot grant access. Credential replacement and late callbacks fail session validation. The UI cancels work when its selected Library/file view disappears. Download storage initialization failure disables downloads independently of browsing, sync and mutation recovery.

## Native UI and exclusions

File Details and eligible saved snapshot information provide Download File, durable byte progress, a distinct Verifying phase, Downloaded and Verified, Cancel, Resume and explicit Restart Download. Eligibility is only a presentation hint; authoritative preflight remains required. Text supports Dynamic Type, multiline layout, VoiceOver labels/hints and updates. Status load is local-only. No speed/ETA, MIME type, export, preview, Quick Look, Share Sheet, upload, background scheduling, automatic retry, metadata mutation, ACK, queue drain or rebaseline is introduced by downloading.

## Validation and publication evidence

Local and hosted validation is being completed. Results below must be replaced with observed evidence before this manifest is finalized. No previous prompt's CI is evidence for P043.

- New native tests: 127 across strict metadata/HTTP policy, recovery, storage and actual URLSession fixture suites.
- First portable download test run: 113 passed, zero failures, prior to the final explicit restart addition. Linux harness uses official Swift 6.2, production domain/application/persistence/staging code, real filesystem/SQLite and a local Swift Crypto counterpart for CryptoKit. Native URLSession/SwiftUI/Rust platform seams are excluded or explicitly skipped in this harness; Linux results are not Simulator execution.
- Static source validator / Python Support / docs / strict formatting: final results pending.
- Exact final feature SHA / hosted PR / exact-head iOS Static, Build, Simulator and Rust Apple runs: pending.
- Simulator pass/skip/fail totals: pending. Existing Keychain entitlement and Data Protection limitations retain their actual native skip status when observed.
- Physical-device validation: NOT_AVAILABLE. No real-device Data Protection, hardware Keychain, cellular behavior, production-server download or physical power-loss durability is claimed.
- Merge and resulting hosted `ios-app` SHA: pending. READY_FOR_PROMPT044 is not claimed until GitHub independently confirms the merge.

## Source inventory

Added: `DownloadModels.swift`, `FileVersionResponseDecoder.swift`, `URLSessionFileContentTransport.swift`, `DownloadStagingStore.swift`, `FileDownloadSQLiteSchema.swift`, `FileDownloadCoordinator.swift`, `FileDownloadViewModel.swift`, `FileDownloadControls.swift` and five FileDownload test/support files. All are registered in the Xcode project.

Modified: authenticated provider, existing SQLite store/migration, dependency container, app/root/catalog/browser/details/snapshot composition, schema-version regression expectations and fixture downgrades. Prior safety tests remain, with supported schema/future-schema assertions advanced to v6/v7 and the old unavailable-content presentation expectation updated for the now-implemented action.
