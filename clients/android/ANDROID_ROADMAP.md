# Synveil Android Roadmap

This file is the durable execution plan for the native Android client. It is
derived from the Android implementation at checkpoint
`18da4d20fd7cc55107e17c8b297cd9218c679750`, the Android JVM and instrumentation
tests, the Gradle and manifest configuration, `.github/workflows/android.yml`,
`api/openapi.yaml`, the Rust API implementation, and the English architecture,
security, sync, deployment, and release documentation.

The accepted baseline is P1 through P18A. The next implementation milestone is
P19. Each remaining milestone is intentionally scoped so it can be completed
with exactly one focused implementation commit, its mandatory validation, and a
push to `origin/android-app`.

## Execution Rules

- Work only on `android-app`; never commit directly to `main`.
- Before each continuation, fetch `origin`, verify the branch, and reread the
  durable Android files.
- Select exactly the milestone marked `NEXT` in `ANDROID_PROGRESS.md`.
- Do not implement a later milestone early or skip a milestone.
- `COMMIT_UNLOCKED=false` until every mandatory gate for the current milestone
  passes.
- Preserve the accepted DeviceBearer, Keystore, TLS/origin, Room, sync,
  idempotency, conflict, credential-fencing, and BrowserSession-only
  authorization invariants.
- An emulator outage never replaces host-executable validation. Repair or
  recreate the dedicated API 36 test AVD when reasonably possible.
- A milestone is complete only after its implementation, tests, required
  runtime gates, focused commit, push, remote-HEAD verification, clean
  worktree, and progress update are all complete.

## Accepted Baseline: P1-P18A

P1-P18A are accepted and verified at the checkpoint above. Their historical
commit anchors are recorded here so later work does not reopen or weaken their
contracts.

| Milestone | Accepted scope | Evidence anchor |
|---|---|---|
| P1 | Native Android application foundation, Compose shell, Android 36 baseline, CI foundation | `adf75a9` |
| P2 | Durable non-secret server profiles, canonical origin validation, profile lifecycle UI | `a2451a9` |
| P3 | Verified profile-bound HTTP transport, bounded responses, redirect/retry policy, TLS boundary | `a2b1fae` |
| P4 | One-time device enrollment and encrypted Android Keystore credential lifecycle | `ac99f2a` and `0d6d5d7` |
| P5 | Profile-bound authenticated DeviceBearer session management and scope fencing | `260f743` |
| P6 | Authenticated library discovery and strict library wire/domain models | `7302fe3` |
| P7 | Logical directory and node browsing with bounded child pagination | `7302fe3` |
| P8 | SAF-backed download, open/share intents, and resumable file creation/replacement primitives | `7302fe3` |
| P9 | Durable Room cache for libraries, nodes, sync state, and recovery staging | `54d2261` |
| P10 | Inbound journal feed, checkpoint, signed acknowledgement, and rebaseline recovery | `54d2261` |
| P11 | WorkManager unique one-time/periodic synchronization with bounded retry policy | `54d2261` |
| P12 | Durable sync settings, profile/library scope coordination, and lifecycle-safe integration | `54d2261` |
| P13 | Durable outbound metadata mutation intents and optimistic revision bases | `67cc3d8` |
| P14 | Replace-content staging, exact byte/hash verification, resumable upload recovery | `67cc3d8` |
| P15 | Safe local conflict summaries and explicit owner/web review boundary | `67cc3d8` |
| P16 | Bidirectional sync integration with immutable IDs, replay, and per-scope serialization | `67cc3d8` |
| P17 | Room 2-to-3 migration coverage and terminal-state cleanup hardening | `18da4d2` |
| P18 | Deterministic fault matrices, stress workloads, and synchronization scope-lock coverage | `18da4d2` |
| P18A | Cross-cutting recovery hardening and accepted checkpoint verification | `18da4d2` |

No remaining milestone may change the meaning of the accepted baseline. In
particular, Android must not invent browser-session conflict APIs, mix browser
cookies or CSRF with DeviceBearer requests, persist plaintext credentials, or
turn ambiguous mutations/transfers/acknowledgements into silent success.

## Repository-Grounded Gap Audit

The accepted code already contains the core transport, enrollment, cache,
transfer, mutation, sync, and WorkManager primitives. The remaining work is
product and release completion rather than a new protocol. The following gaps
drive P19 onward:

- The Compose surface is functional but still exposes development/foundation
  language and lacks a complete state-driven onboarding, re-enrollment,
  revoked-device, ambiguous-result, and recovery journey.
- Library and node screens provide the primary operations but need a cohesive
  file-management UX for pending, offline, stale, failed, trashed, and
  owner-review states, with stronger accessibility and lifecycle handling.
- Cache and queue primitives exist, but foreground presentation of offline
  data, connectivity transitions, retry decisions, cleanup, storage pressure,
  and process death is incomplete.
- Transfer primitives stage and recover content, but end-to-end lifecycle
  orchestration, cancellation, restart/reconciliation, SAF edge cases, and
  low-storage behavior need product-level coverage.
- WorkManager scheduling exists and has instrumentation coverage, but the
  complete user-controlled background policy, connectivity transitions,
  permanent-auth handling, and post-force-stop behavior need an integrated
  application gate.
- Room currently has a tested 2-to-3 migration and cleanup hooks; the full
  migration chain, multi-profile deletion/switch isolation, rebinding safety,
  and bounded cache/queue maintenance need release-grade verification.
- Theme support has dynamic light/dark behavior, but accessibility semantics,
  keyboard/focus behavior, large-screen layout, and representative performance
  evidence are not yet a release contract.
- The Gradle project is still pre-release: `versionName` is `0.1.0-dev`, the
  release build has shrinking disabled, signing identity is undecided, and CI
  runs debug assembly, JVM tests, and lint only. There is no release artifact
  gate, API 36 connected gate, or Android release documentation.
- The manifest intentionally has only `INTERNET`, `allowBackup=false`, and no
  FileProvider. SAF is the intended file boundary; the final audit must prove
  that no extra storage permission, unsafe URI grant, debug cleartext behavior,
  or secret backup/logging path is introduced.
- The server contract exposes DeviceBearer enrollment, logical library/node
  reads, upload sessions, metadata mutations, and sync/rebaseline routes. The
  server keeps conflict inspection and manual resolution BrowserSession-only;
  Android may show a safe owner/web-review state but must not fabricate a
  DeviceBearer resolution call or select a conflict winner.

## Remaining Milestones

### P19 — Onboarding, session state, and recovery UX

**Goal:** Make first launch and every credential/profile state understandable,
recoverable, and lifecycle-safe without exposing secrets or changing protocol
boundaries.

**Concrete scope:**

- Replace foundation-only startup copy with an explicit state machine for no
  profile, selected profile, untested profile, enrolled, not enrolled,
  enrollment recovery required, authentication failed, and device revoked.
- Provide a guided first-run path from profile creation through one-time
  enrollment and library entry, with safe back navigation and process
  recreation recovery.
- Add explicit re-enrollment/forget-on-device/recovery messaging and safe
  profile switching; make destructive local cleanup confirmable and
  profile-scoped.
- Ensure DeviceBearer, enrollment tokens, cookies, CSRF values, and private
  key material never enter UI state strings, navigation arguments, saved state,
  logs, analytics, or crash messages.
- Surface owner/browser review for ambiguous enrollment and conflict outcomes.

**Intentionally excluded:** New server routes, automatic enrollment retry,
browser-session authentication, conflict resolution, file-management redesign,
background scheduler redesign, and release signing.

**Dependencies:** P18A; existing profile, enrollment, session, cache, and
navigation primitives; the current API/OpenAPI authorization boundary.

**Required tests:** ViewModel/state-machine unit tests for every transition;
profile-switch and cleanup regression tests; redacted-state/logging tests;
navigation argument tests; malformed, revoked, authentication-failed, and
ambiguous-result tests; process recreation tests for each recoverable state.

**Runtime/instrumentation gates:** API 36 install and launch; first-run
profile/enrollment smoke flow using a local deterministic test server; force
stop/relaunch during enrollment recovery; profile switch and forget flow; no
crash and no credential visible in UI hierarchy or logcat.

**Security/regression gates:** `./gradlew test`, `./gradlew lint`, transport
and credential-fencing suites, AndroidKeyStore regression, and a manual audit
that browser cookies/CSRF are never mixed into DeviceBearer flows.

**Completion/readiness token:** `SYNVEIL_ANDROID_P19_READY`.

### P20 — Complete file-management and metadata UX

**Goal:** Turn the existing library/browser and mutation primitives into a
coherent, accessible file-management experience.

**Concrete scope:**

- Complete library list, directory navigation, breadcrumbs/back behavior,
  empty/loading/stale/offline/error states, bounded pagination, and refresh.
- Make create-directory, rename, move, trash, restore, and pending mutation
  states understandable and idempotent from the user’s perspective.
- Integrate download/save/open/share and replace-content SAF flows with
  cancel, failure, completion, retry/recovery, MIME/name, and URI-lifetime
  handling.
- Show local conflict summaries and `owner/web review required` without
  inventing a DeviceBearer conflict endpoint or automatic winner.
- Add semantics, content descriptions, focus order, touch targets, keyboard
  navigation, and clear destructive-action confirmations.

**Intentionally excluded:** Automatic file-byte mirroring, text/binary merge,
browser-session conflict resolution, new server mutation semantics, and
background transfer execution.

**Dependencies:** P19; P13-P16 mutation/transfer/sync contracts; SAF and
`api/openapi.yaml` logical node/upload contracts.

**Required tests:** Compose/UI state tests for every screen state; mutation
queue and optimistic-update regression tests; pagination/cursor tests; SAF
intent and URI-grant tests; transfer error/cancel/retry tests; accessibility
semantics checks; BrowserSession-only conflict boundary tests.

**Runtime/instrumentation gates:** API 36 browse/create/rename/move/trash/
restore/download/open/share/replace smoke flow; rotate/recreate during a
transfer and mutation; verify no crash and no unsafe filesystem or URI access.

**Security/regression gates:** full JVM suite, lint, transfer instrumentation,
DeviceBearer transport tests, authorization-scope tests, and a SAF/FileProvider
audit proving only user-selected `content://` data is accessed.

**Completion/readiness token:** `SYNVEIL_ANDROID_P20_READY`.

### P21 — Offline-first cache, connectivity, and foreground recovery

**Goal:** Make cached metadata and durable pending work predictable across
offline periods and connectivity transitions.

**Concrete scope:**

- Observe network availability without changing the server-origin/TLS policy
  and expose online, offline, stale-cache, retrying, and failed states.
- Render cached libraries/nodes immediately where valid, identify staleness,
  and prevent destructive actions when the required sync base is unavailable.
- Provide explicit retry and recovery actions for pending mutations, pending
  acknowledgements, rebaseline staging, and content operations.
- Reconcile foreground refresh with the existing per-profile/device/library
  coordinator without duplicate mutating critical sections.
- Define bounded stale-cache and error presentation semantics that survive
  process death.

**Intentionally excluded:** WorkManager policy changes, new sync protocol
routes, automatic conflict resolution, and file-byte mirroring.

**Dependencies:** P19-P20; P9-P16 cache, mutation, transfer, and sync
invariants; Android connectivity APIs.

**Required tests:** Connectivity transition tests; cache-first and stale-state
tests; process-death/relaunch tests; duplicate-refresh/scope-lock tests;
pending-ACK/rebaseline/mutation recovery tests; multi-profile cache visibility
tests.

**Runtime/instrumentation gates:** API 36 airplane-mode/offline-to-online
smoke flow; foreground refresh after force-stop; cached browsing without
network; recovery of pending work after reconnect; no unbounded retry loop.

**Security/regression gates:** complete JVM suite, sync fault matrix, lint,
Room/cache instrumentation, profile isolation checks, and a review that no
credential or server secret is included in connectivity diagnostics.

**Completion/readiness token:** `SYNVEIL_ANDROID_P21_READY`.

### P22 — Transfer lifecycle, process death, and storage pressure

**Goal:** Make downloads, replace-content operations, and file creation safe
through cancellation, interruption, restart, and low-storage conditions.

**Concrete scope:**

- Provide one lifecycle-aware transfer controller for foreground operations,
  cancellation, resumption, exact offset reconciliation, and terminal cleanup.
- Persist and recover staged content-operation metadata without persisting file
  bytes in Room or exposing credentials.
- Enforce bounded staging size, free-space checks, stale-staging cleanup,
  deterministic failure messaging, and recovery-required states.
- Verify SAF source re-open behavior, source mutation/permission loss, output
  URI failures, partial downloads, and invalid server offsets/hashes.
- Keep transfer work serialized with the existing profile/device/library
  coordinator where mutations or sync can touch the same node.

**Intentionally excluded:** Always-on background file mirroring, arbitrary
filesystem access, new upload protocol behavior, and automatic conflict
resolution.

**Dependencies:** P20-P21; P8 and P14 transfer contracts; Room content
operation entities; server exact-offset upload and immutable download routes.

**Required tests:** JVM fault-point matrix for every transfer phase; staging
quota and free-space tests; process-death/reopen tests; URI permission-loss
tests; hash/length/offset validation tests; cancellation and terminal cleanup
tests; Android transfer intent instrumentation.

**Runtime/instrumentation gates:** API 36 large representative download and
replace flow; kill process at prepare, stage, session, chunk, completion, and
SAF-save points; relaunch and recover; verify staging cleanup and no crash.

**Security/regression gates:** bounded-memory assertions, no Room file bytes,
no bearer logging, SAF URI audit, full JVM/lint suite, and AndroidKeyStore
credential-fencing regression.

**Completion/readiness token:** `SYNVEIL_ANDROID_P22_READY`.

### P23 — WorkManager background synchronization and connectivity policy

**Goal:** Deliver reliable, user-visible, policy-respecting background metadata
sync without weakening foreground recovery or authentication boundaries.

**Concrete scope:**

- Wire sync settings to unique one-time and periodic work for explicit
  profile/library scopes with connected/unmetered and battery-not-low policy.
- Persist/display scheduler state, last attempt/success/error, paused-auth,
  revoked-device, retry, and rebaseline-required outcomes.
- Handle online/offline transitions, Doze/best-effort scheduling, force-stop
  relaunch, duplicate enqueue, cancellation, and profile deletion.
- Resolve credentials just in time and keep all WorkManager input non-secret.
- Bound retries and classify permanent authentication, revocation, and
  protocol failures as safe pauses rather than hot loops.

**Intentionally excluded:** Background file-byte mirroring, a foreground
service, new server endpoints, and conflict winner selection.

**Dependencies:** P19-P21; accepted P11-P12 worker/coordinator contracts;
Android WorkManager and network constraint behavior.

**Required tests:** WorkManager `TestDriver`/instrumentation tests for unique
work, constraints, backoff, cancellation, worker result mapping, process
recreation, profile isolation, and no-secret input; sync fault and revocation
tests.

**Runtime/instrumentation gates:** API 36 connected/unmetered transitions,
periodic enqueue at the Android minimum, force-stop/relaunch, revoked-device
pause, and reconnect recovery; verify no duplicate mutating scope.

**Security/regression gates:** worker input/log audit, DeviceBearer just-in-time
resolution, full JVM/lint suite, sync scope locks, and Android instrumentation
for scheduler behavior.

**Completion/readiness token:** `SYNVEIL_ANDROID_P23_READY`.

### P24 — Room migrations, profile isolation, and bounded maintenance

**Goal:** Make local durable state upgradeable, deletable, and bounded for
multiple profiles and devices.

**Concrete scope:**

- Define and test the immutable Room migration chain from the current schema
  forward, including preservation of active cache, sync checkpoints, pending
  acknowledgements, rebaseline staging, mutation queue, and content operations.
- Fence and clean every profile’s cache, queue, staging metadata, and scheduled
  work when a profile is removed or reconfigured.
- Prove profile/device/library composite-key isolation and safe owner/device
  identity rebind behavior after re-enrollment.
- Add bounded pruning for terminal queue state, stale cache, expired staging,
  and invalid orphan references without deleting recoverable work.
- Define handling for database corruption, migration failure, locked storage,
  and low disk space as explicit recovery states.

**Intentionally excluded:** Changing server schema/protocol, storing file
  bytes or credentials in Room, and adding a new backup system.

**Dependencies:** P21-P23; accepted Room 2-to-3 migration and cache schema;
profile/enrollment fencing contracts.

**Required tests:** Every migration step from a representative legacy schema;
rollback/failure behavior; multi-profile and device-rebind isolation;
pruning/retention tests; corruption and low-space error tests; scheduled-work
cleanup tests; Room instrumentation on API 36.

**Runtime/instrumentation gates:** Install/upgrade representative databases,
switch/delete/re-enroll profiles, force-stop during cleanup, and verify no
cross-profile rows, credentials, or scheduled work remain.

**Security/regression gates:** Room migration instrumentation, full sync and
credential-fencing suites, no-secret-on-disk audit, and bounded row/query
review.

**Completion/readiness token:** `SYNVEIL_ANDROID_P24_READY`.

### P25 — Accessibility, adaptive UI, and performance/resource hardening

**Goal:** Make the completed client usable across supported Android form
factors while proving bounded resource behavior.

**Concrete scope:**

- Add Compose semantics, content descriptions, focus order, keyboard and
  switch navigation, error announcements, contrast, minimum touch targets,
  and text scaling coverage for onboarding, profiles, libraries, transfers,
  sync, and settings.
- Validate light, dark, dynamic-color, and system-theme behavior without
  embedding secrets or state in visual diagnostics.
- Adapt navigation and content layout for API 36 phones, tablets, and large
  windows without changing authorization or sync scope.
- Measure representative metadata workloads, cursor/page bounds, database
  queries, UI recomposition, transfer memory, and retry concurrency.
- Fix only demonstrated unbounded memory, cursor, retry, or database loops;
  retain declared server and client limits.

**Intentionally excluded:** New product features, protocol changes, arbitrary
native platform forks, and marketing performance promises.

**Dependencies:** P20-P24; existing Compose Material 3 theme and Android 36
compatibility target.

**Required tests:** Compose/UI accessibility tests; text-scale and theme tests;
large-screen layout tests; representative 5,000-node/1,000-mutation workload;
bounded-memory and retry assertions; startup/navigation performance checks.

**Runtime/instrumentation gates:** API 36 phone and tablet/large-window smoke
flows; TalkBack/accessibility-service pass; dark/light/system theme pass;
representative scroll and transfer run with no ANR, crash, or runaway memory.

**Security/regression gates:** lint/static analysis, full JVM and Android tests,
secret/log audit, and confirmation that adaptive layouts do not bypass
profile/device/library authorization.

**Completion/readiness token:** `SYNVEIL_ANDROID_P25_READY`.

### P26 — Production Gradle, release artifact, CI, and documentation readiness

**Goal:** Convert the pre-release Android project into a reproducible,
reviewable production build path without committing secrets or generated
artifacts.

**Concrete scope:**

- Establish intentional version/package metadata and a documented release
  identity; keep signing credentials external to the repository and CI.
- Configure a production release build with appropriate shrinking/obfuscation,
  explicit keep rules, resource optimization decisions, and no debug-only
  cleartext or loopback behavior.
- Add release artifact validation for APK/AAB shape, manifest, package name,
  version, native/debug symbols policy, permissions, network security, and
  absence of plaintext secrets.
- Extend Android CI with wrapper validation, clean debug/release build paths,
  JVM tests, lint/static checks, API 36 instrumentation when an emulator is
  available, and clear host-vs-emulator reporting.
- Update Android README and release/build/test documentation; document
  signing, versioning, artifact retention, rollout, rollback, and known
  BrowserSession-only limitations.

**Intentionally excluded:** Publishing to a store, committing signing
  material, server deployment, changing server authorization, and final
  merge approval.

**Dependencies:** P19-P25; current Gradle/AGP/Kotlin/Room/WorkManager setup;
repository release and deployment documentation.

**Required tests:** Clean `assembleDebug`, clean production/release build,
`test`, `lint`, R8/ProGuard validation, manifest/permission checks, artifact
inspection, and API 36 connected tests when available.

**Runtime/instrumentation gates:** Install the production artifact on API 36,
launch primary flows, verify release cleartext rejection, verify no debug
labels/loopback behavior, and exercise process relaunch and navigation.

**Security/regression gates:** release-vs-debug configuration diff; TLS/origin
and redirect audit; DeviceBearer, backup/privacy, SAF/URI, permissions,
R8/reflection, and no-secret artifact audits; no generated APK/AAB or signing
material committed.

**Completion/readiness token:** `SYNVEIL_ANDROID_P26_READY`.

### P27 — FINAL Android production and release readiness

**Goal:** Prove the whole Android roadmap is complete, integrated, documented,
and ready for review/merge into `main`.

**Concrete scope:**

- Re-read the complete roadmap, progress ledger, and definition of done;
  require every milestone P1-P26 and P18A to be `COMPLETE` with evidence.
- Run the full host gate, lint/static gate, release build gate, API 36
  connected instrumentation gate, AndroidKeyStore gate, Room migration gate,
  WorkManager gate, and critical sync/crash/recovery gate.
- Execute API 36 runtime smoke for install, launch, onboarding, profiles,
  enrollment/recovery, libraries/browser, transfers, metadata operations,
  offline/cache, synchronization, settings, theme, accessibility, force-stop,
  and relaunch.
- Perform the final security, data, performance/resource, privacy, permissions,
  SAF/URI, BrowserSession boundary, and release artifact audits.
- Verify every milestone commit was pushed to `origin/android-app`, the
  branch is clean, and `HEAD == origin/android-app`.
- Produce the review/merge handoff; do not merge or publish without the
  repository owner’s explicit follow-up instruction.

**Intentionally excluded:** Any new feature, protocol change, automatic
conflict resolution, store publication, direct merge into `main`, or waiver of
data-loss/authorization/release defects.

**Dependencies:** P19-P26 and every accepted P1-P18A invariant.

**Required tests:** Every mandatory gate in `ANDROID_DEFINITION_OF_DONE.md`,
with fresh output captured in the progress ledger and no skipped gate.

**Runtime/instrumentation gates:** Dedicated API 36 emulator/device install,
full smoke workflow, force-stop/relaunch, lifecycle/navigation, accessibility,
and no-crash evidence. An emulator failure must be recorded as a blocker and
cannot suppress host gates.

**Security/regression gates:** No unresolved P0/P1 security or correctness
defect; no browser/device authentication mixing; no plaintext credential,
secret, unsafe URI, cleartext-release, or unauthorized conflict behavior.

**Completion/readiness token:** `SYNVEIL_ANDROID_APP_COMPLETE`.
