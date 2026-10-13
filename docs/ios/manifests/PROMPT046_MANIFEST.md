# Prompt046 — Background Transfer Scheduling & Recovery Foundation

## Scope and verified baseline

- Starting `origin/ios-app`: `a06e17f481b9b3f1a1c4e47366dcaa88eea399d7`.
- Remote references fetched and inspected on 2026-10-13. P045 implementation merge
  `c32d37e20f41f0064d23de0816a6c15478df150f` and final P045 evidence are ancestors.
- Feature branch: `ios/p046-background-transfer-recovery`, based on that integration HEAD.
- Initial workspace branch `work` was clean and pointed to desktop/main history. It was preserved;
  source work switched to the verified iOS baseline before creating the feature branch.
- No AGENTS.md or Swift package manifest exists in the selected baseline. Native sources use the
  Xcode app/test targets, SwiftUI lifecycle, the Rust bridge, SQLite, and the four macOS iOS workflows.
- Reviewed P045 manifest, native project registration, credential/session controllers, staging and
  transfer leases, `api/openapi.yaml`, `crates/api/src/uploads.rs`, and the server's authoritative
  exact-offset/frame append implementation in `crates/storage/src/uploads.rs`.
- No server, Android, desktop, installer, ACK, rebaseline, or mutation protocol changes. P047 is not started.

## Dependency and risk inventory

P045 supplies immutable UUIDv7 session/intent identity, frozen replacement revision, strict wire
responses, SQLite offset CAS, shared filesystem transfer lease, credential scope fencing, private
source staging, and common quota accounting. P046 depends on those boundaries and on the server
rejecting stale/gap exact-offset writes. The server can durably accept part of a streaming request
before a timeout. Neither local bytes nor OS success establishes accepted bytes.

Critical risks are submission/binding crashes, asynchronous logout and delegate delivery,
foreground/background overlap after process termination, device-lock file access, and OS persistence
of authorization headers. Recovery fails closed when binding or evidence is inconsistent. Native
behavior and hardware qualification are separate gates.

## Architecture and source inventory

New production sources:

- `clients/ios/Domain/Transfer/BackgroundUploadModels.swift`: typed state transitions, immutable
  chunk intent, safe callback evidence and system task snapshots.
- `clients/ios/Infrastructure/Persistence/BackgroundUploadSQLiteSchema.swift`: v8 durable jobs
  and bounded orphan task observations.
- `clients/ios/Application/Transfer/BackgroundUploadCoordinator.swift`: authorization, file-backed
  PATCH dispatch, task restoration, strict GET reconciliation, pause and quarantine.
- `clients/ios/Infrastructure/Network/URLSessionBackgroundUploadTransport.swift`: background
  upload adapter, serial delegates, allowlisted evidence and lifecycle persistence barrier.
- `clients/ios/App/BackgroundUploadAppDelegate.swift`: UIKit background URLSession event delivery.

New tests: `BackgroundUploadTests.swift` and portable background source registration checks.
Modified production sources: `FileUploadCoordinator`, `AuthenticatedLibraryRequestProvider`,
`UploadStagingStore`, `DownloadStagingStore`, `MutationQueueSQLiteStore`, `FileUploadControls`,
`FileUploadViewModel`, `SynveilApp`, `AppDependencyContainer`, and Xcode target membership.
Existing SQLite migration fixtures and portable schema expectations advance to v8; prior test
coverage is retained. Shared scripted upload test provider gains background request/status seams.

The existing coordinator still owns user operations, CREATE, COMPLETE, abort, immutable upload
identity and publication. The adapter owns only background PATCH orchestration. SwiftUI selects
explicit policy and displays durable state; it does not mutate server offsets or own OS tasks.

## Session identifier and scheduling policy

The stable session identifier is `com.synveil.ios.upload.background.v1`. It contains no account,
credential, token, file path, upload ID or target. Task descriptions contain only a random local
chunk-intent UUID. `uploadTask(with:fromFile:)` is used, never Data or stream upload tasks.
Tasks are created suspended, bound durably, then resumed. System inventory uses `getAllTasks` via
Foundation's async `allTasks` API and validates intent, generation, task ID and original authenticated
request URL/method/offset/length. A task identifier alone cannot authorize adoption.

BackgroundTasks was evaluated and omitted: background URLSession already handles OS execution and
relaunch. There is no independent bounded maintenance operation that justifies another wakeup path.
No BG identifiers, Info.plist background modes, processing declarations or entitlements are added.
There is no app polling, timer, automatic zero-progress retry, or wakeup resubmission loop. A finite
24-hour resource timeout bounds OS waiting; scheduling and execution are never guaranteed.

## State machine and invariants

The typed scheduling state distinguishes foregroundOnly, preparing, queued, scheduled, transferring,
awaitingEvidence, reconciling, paused, suspended, completedChunk, awaitingFinalization,
failedRecoverable, failedTerminal, aborted and quarantined. P045's local upload/publication state
remains authoritative for file operations. Only a strict server response can advance the stored
accepted offset. Final chunk acceptance leaves `READY_TO_COMPLETE`, never `COMMITTED`.

- One unresolved chunk intent per upload is enforced by the durable job primary key and exact blob
  CAS, with a globally unique intent index. Unresolved intent blocks foreground state/offset mutation
  even across reopened connections.
- The shared transfer lease serializes preparation, foreground actions, recovery and continuation.
- Offset acceptance and durable release of PATCH exclusion occur in one SQLite transaction.
- Canonical, sorted-key metadata permits exact CAS independently of JSON dictionary ordering.
- A new generation cannot replace an unresolved chunk. Duplicate callbacks are idempotent; conflicting
  duplicate evidence, unknown state, mismatched scope/route or duplicate system mapping fails closed.
- Pause durably withdraws authorization before best-effort OS cancellation. A callback cannot Resume.
- Every automatically continued chunk belongs to the same explicitly approved upload. No file
  selection, session creation, COMPLETE, replacement retargeting, ACK or rebaseline occurs.
- Recovery only continues after server GET agrees with the original chunk range and immutable target.
  Zero accepted progress stops; no speculative PATCH replay is allowed. If task completion evidence
  is missing after relaunch, GET may confirm progress but continuation requires an explicit user action.
  A delivered cancellation withdraws background continuation even when the original Pause write failed.

## Protocol, security and account isolation

The existing five UploadSession endpoints and P045 decoders are reused unchanged. A valid PATCH 204
must have an empty body, valid request ID and exact canonical accepted offset. GET validates header
and body offset agreement, session ID, operation, expected digest/length, frozen target and state.
A valid 204 inconsistent with GET cannot advance progress. Malformed or lost responses can recover
only through authenticated GET. Non-open, conflicting, expired and invalid states cannot schedule.

HTTPS origin construction stays inside the existing credential provider. All redirects are rejected.
Cookies, shared caches and CSRF authority are disabled; no request secrets are serialized in SQLite,
task descriptions, filenames or logs. Evidence persists only bounded allowlisted response headers,
status, body-emptiness and a safe failure category. Account/device/endpoint/library and credential
identities are validated against the durable P045 record and current session, with lifecycle fencing.
Logout invalidates the adapter, cancels known OS tasks best effort, quarantines durable jobs/scopes
and retains uncertain payloads. Old account evidence cannot publish under a new login.

Apple may persist HTTP request headers, including the Device Bearer header, in OS-managed background
session storage. Application Keychain storage does not imply submitted authorization remains solely
in Keychain. The app controls its own serialization and delegates, not the OS's internal request
storage or how long that material is retained. Cancel/logout cannot guarantee revocation of bytes
already sent or a request already reaching the server. Real-device inspection remains required.

## Persistence and filesystem policy

SQLite schema v7 migrates transactionally to v8 with `background_upload_jobs` and
`background_upload_orphans`. Earlier v1–v7 migration paths and all prior data remain. Exact schema
verification, quick_check, foreign_key_check, FULL synchronization, future-version rejection,
capacity limits and existing scope quarantine remain enabled. Job metadata has a 4 KiB bound;
operation identity references `file_uploads`. Metadata encoding v1 requires explicit version and
allowlisted fields; future versions or fields are rejected. No bearer values are persisted. Unknown decoding and
corrupt state halt inventory and cancel system work, without schema downgrade or speculative repair.

Each chunk is at most P045's 4 MiB. It is copied from the rehashed private source at the planned
range, length/digest checked, fsynced, marked read-only and directory-fsynced before intent dispatch.
Opaque `.bgchunk` files remain in the shared staging root, are opened with O_NOFOLLOW, must be
regular single-link files, and count toward the existing 512 MiB ceiling and 64 MiB reserve. Failed
cleanup and unrecognized files remain charged. No unresolved source or payload is evicted.

Existing `completeUntilFirstUserAuthentication` and backup exclusion are reused; unrelated files
are not weakened. After first unlock this class permits background reads while locked, with the
corresponding privacy tradeoff. Before first unlock the payload/credential may be unavailable and
work must defer. Simulator checks cannot prove real device-lock behavior. The inherited
`Library/Caches` eviction risk remains: missing data fails closed and requires explicit recovery.

## Lifecycle and crash recovery

`@UIApplicationDelegateAdaptor` routes the stable session event callback to a persistence barrier.
Callback evidence can be persisted before credentials finish restoring; it cannot start network
recovery or be classified as invalid authorization merely because bootstrap is unfinished.
Application-delegate launch bootstraps storage/session/recovery independently of scene creation;
SwiftUI awaits the same one-time task. A launch request arriving before composition is retained.
The serial delegate queue submits ordered main-queue events. Callback evidence is committed before
acknowledging the OS. Finish-events acknowledgement waits for queued durable work, occurs once, and
is withheld if local storage fails. If startup storage/schema or staging is unavailable, the stable
session is reconnected solely for best-effort cancellation, with no task adoption or offset mutation.
Network reconciliation/next-chunk execution is not required for
acknowledging already persisted callback evidence. Cold launch initializes/migrates storage,
reconnects the session, restores inventory and binds system tasks before continuation. Application
reopen never starts a new upload session. Paused jobs stay paused; unknown/orphan tasks are cancelled.

| Boundary | Known evidence | Uncertainty / next safe action |
|---|---|---|
| Source/chunk creation or fsync fails | Original source/identity retained; no submitted task | Fail preparation; leftover files stay quota charged |
| Preparation metadata commit fails | No OS task created | Read durable inventory after reopen; no blind submit |
| Intent commit acknowledgement lost | Intent may exist, task not created by this invocation | Enumerate tasks and reconcile before continuation |
| OS task created, binding commit fails | Durable unique intent, suspended task may exist | Enumerate/match full request; never submit another PATCH because mapping is absent |
| Binding committed, process ends before resume | Exact task and intent persisted | Validate scope/payload and resume existing task only |
| Cancellation/timeout/lost callback | Intent and planned range retained | Wait for task disappearance, GET once; missing callback or cancellation cannot automatically continue |
| Callback write fails | Offset unchanged; original intent retained | Retain queued event; withhold system acknowledgement and retry on a later lifecycle pass |
| GET interrupted or invalid | Chunk range and exclusion retained | No continuation; explicit Check Server Status / Resume recovery |
| Offset commit fails or acknowledgement lost | Offset/job transaction is all-or-nothing | Reopen/read inventory, never infer commit from memory |
| Cleanup fails after resolution | Accepted offset durable; old payload remains charged | Later safe cleanup; no eviction or offset rewind |
| Logout invalidation fails | In-process invalidation fences networking; evidence retained | Cancel best effort, existing database quarantine fails closed |

The server's exact-offset append and concurrency checks remain an explicit protocol dependency,
including when the OS itself retries a request. Application code never retries an ambiguous PATCH
without authoritative reconciliation and never treats OS byte counters as evidence.

## Automated and native validation

Portable checks and strict formatting are being run on Linux with the official `swift:6.2.3`
toolchain. Native build and XCTest must be observed on macOS GitHub Actions. Run URLs, XCTest counts,
PR state and merge evidence will be recorded only after genuine publication/observation; they are
not yet available at this implementation stage. No earlier P045 run is counted for P046.

New deterministic XCTest coverage includes typed transitions/terminal protections, explicit opt-in,
file-backed exact range/digest, foreground exclusion across reopen, 204/GET agreement, partial and
lost-response recovery, zero-progress stop, stale CAS/generation, pause/logout fencing, duplicate and
orphan callbacks, wrong-origin/duplicate/missing task mapping, interrupted/unauthenticated GET,
v7 migration/data retention, intent/binding/callback commit failure, lifecycle persistence barriers,
payload symlink/mutation/quota rejection and native small-screen accessibility rendering. Existing
P045 and prior regression suites remain enabled. BackgroundTasks scheduler tests are inapplicable
because no BG scheduler or BG expiration lifecycle is added.

## Observed intermediate native results

At `3c783a6fa8a764fcc8911f0110f712250fae900b`, the following observed PR gates ran:

| Gate | Run | Observed outcome |
|---|---|---|
| iOS Static Validation | [38381495466](https://github.com/nghianguyen150612/Synveil/actions/runs/38381495466) | PASS |
| iOS Build | [38381495440](https://github.com/nghianguyen150612/Synveil/actions/runs/38381495440) | PASS |
| iOS Rust Apple Build | [38381495467](https://github.com/nghianguyen150612/Synveil/actions/runs/38381495467) | PASS |
| iOS Simulator Tests | [38381489483](https://github.com/nghianguyen150612/Synveil/actions/runs/38381489483) | FAIL: 1,695 passed, 2 failed, 2 skipped; 1,699 total |

All 48 then-present `BackgroundUploadTests` methods passed. The two failures were retained
`MutationQueueSQLiteTests` assertions: the exact table inventory still expected 20 instead of the
new 22 tables, and the unknown-future-schema test injected v9 but still expected v8 afterward.
The assertions are corrected to the actual v8 inventory and preserved v9 marker; no test is removed
or suppressed. The prior first build at `5a8b7b4dbb6862ede28a730e55fb3257bf0ce460`
([38379945349](https://github.com/nghianguyen150612/Synveil/actions/runs/38379945349)) exposed an
invalid CryptoKit initializer; that was corrected using `SHA256.hash(data:)`.

These are intermediate results, not final-head qualification. Further source changes strengthen
Pause, missing-callback recovery and metadata validation. Final-head native results remain pending.

## Hardware evidence and procedure

All physical-device evidence is **NOT_AVAILABLE**. Before production qualification, use a signed
build on a real device and a controlled Synveil server. Record device/OS/build SHA, server trace with
request IDs (redacted), accepted offsets, job inventory and final immutable FileVersion evidence.

1. Opt in to a multi-chunk file; lock and suspend the app. Verify actual server offsets and event
   delivery, including launch after OS termination. Distinguish system termination from user force
   quit; test ordinary reopen and document force-quit suppression of background work.
2. Switch Wi-Fi/cellular, enable airplane mode, inject DNS/TLS failures and disconnect after partial
   server frame acceptance. Confirm one original identity and strict GET before the next PATCH.
3. Test first-unlock, lock, restart and Keychain/file protection. Verify payload readability and
   retained request/header behavior; no guarantee about internal OS header storage is inferred.
4. Enable Low Power Mode and constrained networking; record system delay and 24-hour timeout.
   No guaranteed scheduling or power behavior is claimed.
5. Pause/cancel/logout, then log in as another account while a callback is delayed. Verify durable
   quarantine and no publication or continuation under the new account.
6. Use real local/iCloud/third-party Files providers, low physical storage and interrupted writes.
   Exercise physical power-loss boundaries around intent, binding, offset commit and cleanup.
7. Explicitly finalize against the production-compatible server; verify immutable content/version
   integrity and frozen replacement revision conflict. Do not repeat COMPLETE speculatively.

## GitHub and P047 readiness

PR [#128](https://github.com/nghianguyen150612/Synveil/pull/128) is open and draft, targeting `ios-app`.
Review acceptance, exact final feature CI, merge and resulting `ios-app` integration SHA are pending. This manifest does not invent those values or attempt to contain its own commit SHA.
The feature must remain draft/open if mandatory gates or evidence are unavailable. An ordinary
merge commit is permitted only after required gates and review acceptance. P047 is **NOT READY**
at this stage and has not been started.
