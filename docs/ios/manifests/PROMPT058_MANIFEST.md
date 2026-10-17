# PROMPT058 — PhotoKit foreground backup orchestration

## Integration status

- Verified starting `ios-app` baseline: `82e3e53eb690942089401c005c0c28c1f0bd6921` (merged P057 PR #141).
- P055 PR #139 remains separate and unmerged; this branch does not include or change it.
- Feature branch: `ios/p058-photokit-backup-orchestration`.
- P058 changes are not merged. P059 has not started.
- Completion decision: `IMPLEMENTED_PENDING_NATIVE_VALIDATION` while the required exact-HEAD Apple workflows and independent review are pending.

## Server capability audit

The audit checked `api/openapi.yaml`, `crates/api/src/router.rs`, upload/version/backup route implementations, and the current iOS P045 integration. No server contract was added by P058.

| Capability | Classification | Evidence and client behavior |
| --- | --- | --- |
| Canonical P045 file upload | `IMPLEMENTED_VERIFIED` | Existing upload-session create/status/complete routes and `FileUploadCoordinator`; P058 calls this coordinator with `backgroundEnabled: false`. |
| FileVersion commit confirmation | `IMPLEMENTED_VERIFIED` | Upload completion returns the canonical `file_version_id`, expected byte length, and SHA-256 evidence. P058 records it only after matching the verified local content. |
| PhotoAsset import binding | `PLANNED_NOT_IMPLEMENTED` | No PhotoAsset import route or Rust service exists. The adapter reports unavailable and the queue remains `FILE_UPLOADED_AWAITING_PHOTO_BINDING`. |
| PhotoResource binding | `PLANNED_NOT_IMPLEMENTED` | No resource-association route exists. |
| Live Photo group binding | `PLANNED_NOT_IMPLEMENTED` | No authoritative group contract exists. Still and motion parts remain independent and unbound. |
| Idempotent binding recovery | `PLANNED_NOT_IMPLEMENTED` | There is no binding endpoint or binding-status lookup to reconcile. |
| BackupSet association for a P045 FileVersion | `PLANNED_NOT_IMPLEMENTED` | BackupSet metadata and operations exist, but no verified operation associates this uploaded photo FileVersion with a BackupSet. |
| BackupSnapshot membership and retention proof for the photo | `PLANNED_NOT_IMPLEMENTED` | Generic snapshot metadata and manifest reads exist; they do not establish membership/retention for this P058 upload. |
| Remote restoration of original PhotoKit resources | `PLANNED_NOT_IMPLEMENTED` | Generic FileVersion/snapshot restore capabilities exist. PhotoKit resource identity and group bindings do not, so P058 cannot claim restoration as a photo asset. |

`IMPLEMENTED_UNVERIFIED_DEPLOYMENT` is not claimed for any P058 PhotoAsset or retention operation. No capability is marked `DEFERRED_SECURITY_REVIEW` as implemented; any future server endpoint requires a separate contract and security review.

## Policy and consent

`PhotoBackupConfiguration` keeps discovery enrollment separate from automatic foreground upload consent. Automatic upload defaults off. Activating it requires current discovery enrollment, valid PhotoKit access, an active Library and destination directory, and the explicit confirmation shown in settings. The confirmation names the selected destination and states that uploaded files may remain unbound.

The persisted policy includes media categories, Live Photo selection, rendition policy, Library and parent, Wi-Fi/cellular rules, Low Data Mode handling, maximum resource size, one-operation concurrency, pause state, and monotonically revised routing policy. Target or eligibility changes invalidate prior transfer confirmation; committed P045 intents keep their existing IDs and destination.

## Queue, discovery, and execution

- The isolated PhotoObservation SQLite store moves from schema v2 to v3 transactionally. Central application SQLite remains unchanged.
- Candidate uniqueness combines profile/owner/device, client-library scope, PhotoKit local identity, source revision, resource identity/role/rendition, authorization generation, and policy revision. SHA-256 is integrity evidence, not asset identity.
- Queue capacity is bounded to 1,000 pending items and 50,000 retained candidate rows per owner/device/profile. Terminal rows without P045/FileVersion references may be pruned after 90 days.
- Discovery reuses P056/P057 complete-scan generations and reads ordered SQLite pages of at most 500 observations. The durable cursor is keyed to the scan generation and keeps its end position, so an unchanged scan does not loop over page one. Candidate batches commit atomically at queue capacity; the cursor advances only after the batch is durable.
- Queue leases expire after one hour. On a new PhotoKit authorization generation, same-profile, same-owner, same-device candidates are adopted into the current session. Uncommitted extraction work is requeued; any row with a P045 ID is preserved as read-only reconciliation work, including rows from an older policy revision.
- Expired extraction work returns to the queue; an expired upload claim becomes reconciliation work and retains its exact P045 ID. Existing P045 status is checked against the saved Library, and a transfer can resume only when the current policy and exact saved destination still match.
- New extraction failures stop after five attempts. Recoverable retries use bounded exponential delay from 30 seconds to six hours. An unknown P045 outcome continues to reconcile its existing operation and never creates a second session.
- Each foreground pass scans one bounded page and processes at most two resources, serially. A user-owned active P045 operation takes priority.
- The worker requires an authenticated current identity, active app lifecycle, current permission, active destination, current network policy, and current policy revision. Logout/session invalidation cancels the worker before the existing PhotoKit authorization barrier.
- PhotoKit resource requests keep iCloud network retrieval disabled. P057 private staging, streaming hashes, exact byte counts, shared quota reservation, and P045 handoff are reused; no second HTTP client or background URLSession job is introduced.
- Missing PhotoKit resources are reported as source unavailable. PhotoKit absence never causes remote delete/trash/prune actions.

## Upload authority and Live Photos

The worker persists the P045 operation ID before handoff, reconciles that exact ID after interruption, and accepts completion only when server state, FileVersion ID, byte length, and SHA-256 match P057 evidence. It records `FILE_UPLOADED_AWAITING_PHOTO_BINDING` and never labels a normal Library upload as a retained backup.

Live Photo still and motion resources have independent candidate, extraction, P045, and FileVersion evidence. Local group states are `PENDING`, `PARTIAL`, `RESOURCES_UPLOADED_UNBOUND`, `BINDING_PENDING`, `BINDING_CONFIRMED`, `BACKUP_CONFIRMED`, and `FAILED_RECOVERABLE`. The unavailable adapter cannot produce a binding or retention proof; successful component files are preserved when another component fails.

## Settings and limitations

Settings expose discovery permission/status, a separate automatic-upload opt-in, destination pickers, still/video/Live Photo categories, rendition choice, network/Low Data Mode policy, maximum resource size, pause/resume, queue counters, and Live Photo component status. It states that processing runs only while the authenticated app is active.

PhotoAsset import, Live Photo server grouping, BackupSet association for P045 uploads, and authoritative BackupSnapshot retention proof remain unavailable. Background media transfer, silent iCloud retrieval, and remote deletion remain disabled.

## Tests and validation

Seven new XCTest cases cover automatic-off defaults and explicit consent, durable duplicate suppression versus changed source revision, durable end-of-scan cursors, all-or-nothing queue capacity, interrupted extraction lease recovery, preservation of the original P045 ID after unknown upload recovery, and queue adoption across authorization generations. The schema migration test now expects v3 and checks new queue/configuration tables. Existing P001–P057 test targets remain registered.

Portable validation and four native iOS workflow results will be recorded here against the final pushed commit. Native XCTest totals must come from the exact P058 simulator run; P057 CI results are not reused. Physical-device, controlled live-upload, PhotoAsset-binding, and backup-retention deployment checks are not claimed.

## Review, CI, and deployment gates

- Pull request: pending creation.
- Independent reviewer: pending request.
- iOS Static Validation: pending exact-HEAD result.
- iOS Build: pending exact-HEAD result.
- iOS Simulator Tests: pending exact-HEAD result and XCTest totals.
- iOS Rust Apple Build: pending exact-HEAD result.
- `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`
- `DEFERRED_LIVE_UPLOAD_DEPLOYMENT_NOT_AVAILABLE`
- `DEFERRED_PHOTO_BINDING_SERVER_API_NOT_IMPLEMENTED`
- `DEFERRED_BACKUP_RETENTION_BINDING_NOT_IMPLEMENTED`

## Remaining product work after P058

Define and separately review authenticated, idempotent PhotoAsset/PhotoResource/Live Photo binding; define how canonical FileVersions enter a BackupSet/Snapshot and how retention proof is reconciled; add contract tests and deployment evidence; qualify large libraries, permission changes, storage pressure, and real P045 uploads on devices. Background transfer remains a separate security gate. P059 is not authorized by this work.
