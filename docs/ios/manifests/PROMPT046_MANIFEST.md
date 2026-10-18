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
request URL/method/offset/length. A task identifier alone cannot authorize adoption. Native Resume and scoped cancellation recheck
its opaque intent immediately before acting, protecting against OS task-identifier reuse.

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

HTTPS origin construction stays inside the existing credential provider. The foreground upload
transport rejects redirects. The original P046 background transport also implemented
`willPerformHTTPRedirection`, but Apple's documented background URLSession behavior does not invoke
that decision callback; it therefore does not reject redirects for OS-managed PATCH tasks. P051A
corrects this claim and disables new background submissions and resumption in the production iOS
composition pending a deployment-bound security proof.
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

### P051A retrospective security correction

The original P046 redirect-delegate description above was not a valid background-session control.
Apple does not call it for background URLSession transfers. A cross-origin redirect can expose the
DeviceBearer if the OS forwards that header, and a 307/308 can forward the PATCH body even if the
header is removed. These are separate risks. P051A adds an API-process 3xx response guard and
negative tests, but the API cannot control an external reverse proxy. Production P046 scheduling
and Resume are therefore disabled. Restoration attempts to persist withdrawal of continuation
permission before best-effort cancellation. If SQLite rejects that write, the current process still
keeps the capability closed and cancels the OS task; the unchanged unresolved record is retried on
later restoration. Unknown or duplicate task IDs are retained as bounded orphan observations when
SQLite is available. Existing intent, chunk, task identity, response evidence and authoritative
offset are preserved; no PATCH replay or completion occurs. A later foreground Resume must reconcile
the original session with the existing authoritative status GET. See
`docs/en/AUTHENTICATED_TRANSFER_SECURITY.md` for the threat assessment, deployment checklist and
remaining limitations.

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

Observed local checks pass: the iOS source validator, all 84 portable support tests,
`scripts/validate-docs.sh`, `git diff --check`, and strict recursive formatting/lint with the
official `swift:6.2.3` toolchain. Linux has no Xcode or iOS Simulator. Native evidence below is
observed from macOS GitHub Actions on the final feature head; no earlier P045 run is counted for P046.

The source contains 52 new deterministic background XCTest methods. Coverage includes typed transitions/terminal protections, explicit opt-in,
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
Pause, missing-callback recovery and metadata validation. The final-head results below supersede
these intermediate outcomes.

## Verified final-head native validation

All four required PR workflows completed successfully on exact feature head
`ca5df5ac4490b1a67a112ff0291bc8f603c6af7c`:

| Gate | Observed run | Result |
|---|---|---|
| iOS Static Validation | [38387443777](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443777) | PASS |
| iOS Build | [38387443834](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443834) | PASS |
| iOS Simulator Tests | [38387443724](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443724) | PASS |
| iOS Rust Apple Build | [38387443801](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443801) | PASS |

The Simulator run used Xcode 26.6 / Swift 6.3.3 and iPhone 17 Pro on iOS 26.5. The observed
xcresult summary contains **1,703 total tests: 1,701 passed, 0 failed, 2 skipped**.
All **52 BackgroundUploadTests** passed, including the task-identifier reuse test. The retained
P045 upload/replacement/pause/abort and existing regression suites remain enabled.

The two existing skips are explicit:

- `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`: unsigned
  Simulator test execution has no Keychain access entitlement.
- `MutationQueueSQLiteTests.testNativeDataProtectionAttributes`: Simulator storage does not expose
  physical Data Protection attributes; real-device validation remains required.

Result artifact: `ios-simulator-test-results`, ID `11807509914`, 63,952,645 bytes,
SHA-256 `443c18f0db921a89bbfc6e4e24e203a719e5fd594a75c1a5e67512237010fd0c`,
[associated run](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443724).
No unresolved P046 test failure was observed on this final head. Simulator evidence does not
qualify real OS suspension, device-lock behavior or production-server execution.

Other repository workflows are not iOS gates and did report failures on the same feature head:

| Workflow | Run | Observed issue |
|---|---|---|
| Rust CI | [38387443868](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443868) | Clippy double `must_use`, Linux lifecycle artifact assertion, and macOS host IPC tests |
| Linux AppImage | [38387443705](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443705) | Private or temporary build path in desktop artifact |
| PostgreSQL 17 scheduled-maintenance | [38387443706](https://github.com/nghianguyen150612/Synveil/actions/runs/38387443706) | Scheduler schema/controller-generation assertion |

These failures concern unchanged server/desktop/installer sources. P045 also records the Clippy,
AppImage and scheduler issues. No unrelated implementation or workflow suppression was introduced;
these results are disclosed separately from the four passing P046 gates.

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

| Record | Verified value |
|---|---|
| Starting `ios-app` SHA | `a06e17f481b9b3f1a1c4e47366dcaa88eea399d7` |
| Feature branch | `ios/p046-background-transfer-recovery` |
| Final feature SHA | `ca5df5ac4490b1a67a112ff0291bc8f603c6af7c` |
| Feature PR | [#128](https://github.com/nghianguyen150612/Synveil/pull/128), targeting `ios-app`, MERGED |
| Review acceptance | User accepted implementation in the task conversation on 2026-10-13 |
| Merge method and time | Ordinary two-parent merge commit, 2026-10-13 11:18:17 UTC |
| Verified feature integration SHA | `dc849942bc20eae35b273e57b807b609079d6aa1` |

After all four final-head gates passed and review acceptance was recorded, PR #128 was marked
ready and merged. A fresh remote fetch confirmed the feature SHA as an ancestor and the merge
parents as the starting integration and final feature heads. No force push, squash, rebase or
unrelated source cleanup was used. This post-merge evidence update is documentation only and is
published separately; its resulting integration SHA is verified in the task's final report rather
than attempting a self-referential commit hash in this manifest.

Decision: **COMPLETE_READY_FOR_P047** for the reviewed foundation and its required automated gates.
Physical-device qualification remains **NOT_AVAILABLE**, with the procedure above; this is not a
production qualification claim. P047 has not been started.
