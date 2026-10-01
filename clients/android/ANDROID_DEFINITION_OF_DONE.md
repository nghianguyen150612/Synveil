# Synveil Android Definition of Done

The Android client is complete only when every roadmap milestone P1 through
P27, including P18A, is `COMPLETE` in `ANDROID_PROGRESS.md` and the evidence
below is current for the exact final branch state. No skipped milestone,
unrecorded waiver, or unresolved P0/P1 defect is acceptable.

## ROADMAP

- Every Android milestone in `ANDROID_ROADMAP.md` is `COMPLETE`.
- No milestone is skipped, merged into another milestone, or marked complete
  without its readiness token and validation evidence.
- `ANDROID_PROGRESS.md` contains durable starting SHA, completion SHA, commit
  subject, readiness token, validation evidence, and blocker state for every
  milestone.
- Every milestone commit is pushed to `origin/android-app`.
- The accepted P1-P18A DeviceBearer, Keystore, TLS/origin, Room, sync,
  idempotency, conflict, credential-fencing, and BrowserSession-only
  invariants remain intact.

## BUILD

- A clean debug build passes from a clean checkout with the documented JDK,
  Android SDK Platform 36, and Build Tools 36.0.0.
- A clean production/release build path passes from a clean checkout.
- Release output has no accidental debug-only behavior, loopback HTTP policy,
  development labels, test credentials, or debug cleartext allowance.
- Release package/application ID, version code, version name, manifest, and
  artifact metadata are intentional and documented.
- Signing credentials and server secrets remain external; no signing material,
  generated APK/AAB, mapping artifact, or local secret is committed.
- Gradle wrapper, dependency versions, R8/ProGuard configuration, and resource
  packaging are reproducible and validated.

## TESTS

- The complete JVM suite passes with no test failure or unexpected warning.
- Android lint and configured static checks pass.
- Connected Android instrumentation passes on the dedicated API 36 test AVD or
  a documented API 36 device.
- AndroidKeyStore credential creation, encryption, recreation, malformed data,
  missing-key, fencing, and deletion regressions pass.
- Every Room migration step and representative legacy-database upgrade passes
  on device, preserving cache, sync, ACK, rebaseline, mutation, and content
  operation state.
- WorkManager unique-work, constraints, backoff, cancellation, result mapping,
  process recreation, profile deletion, and no-secret-input regressions pass.
- Critical sync, mutation, transfer, process-death, crash-safety, replay,
  rebaseline, conflict-summary, and recovery-fault suites pass.
- Emulator unavailability never substitutes for host-executable tests; it is
  recorded separately as an environmental blocker.

## RUNTIME

- The release candidate installs on API 36 without an unsafe downgrade or
  package mismatch.
- The app launches from a clean install and after upgrade from the accepted
  baseline.
- Primary flows pass: onboarding, profile creation/edit/delete/switch,
  enrollment, re-enrollment/recovery, libraries, browser, metadata operations,
  transfers, offline/cache, synchronization, settings, and error recovery.
- Force-stop during enrollment, mutation, transfer, ACK, rebaseline, cleanup,
  background work, and navigation is recoverable after relaunch.
- Navigation, configuration change, process recreation, back behavior, and
  lifecycle transitions do not lose scope, duplicate mutation, or crash.
- API 36 runtime smoke produces no crash, ANR, unsafe URI grant, or secret in
  visible UI/logcat output.

## SECURITY

- DeviceBearer and enrollment credentials are encrypted or held only in the
  intended Android Keystore/runtime boundary; plaintext is absent from Room,
  DataStore, navigation, saved state, logs, diagnostics, and artifacts.
- TLS, canonical origin, redirect, retry, cleartext, and release-vs-debug
  behavior are audited and match the server contract.
- Android permissions are minimal, justified, and tested. `INTERNET` is the
  only required baseline permission unless a later reviewed requirement proves
  another one necessary.
- SAF and any FileProvider/URI grants are audited for authority, lifetime,
  path confinement, MIME behavior, revocation, and no arbitrary filesystem
  exposure.
- Backup/privacy behavior is explicit; credentials, cache secrets, staging
  bytes, and private metadata are not accidentally backed up or logged.
- Browser-session cookies and CSRF are never mixed with DeviceBearer requests.
- Conflict inspection and manual resolution remain BrowserSession-only unless
  the server contract is explicitly changed and reviewed; Android never fakes
  a resolution request or silently chooses a winner.
- No known P0/P1 security defect, authorization bypass, credential leak,
  replay violation, unsafe redirect, or cross-profile access defect exists.

## DATA

- The full Room migration chain is tested from representative legacy schemas
  through the final schema.
- Cache, mutation queue, pending ACK, rebaseline, content-operation, terminal
  cleanup, and staging retention behavior is bounded and crash-safe.
- Multi-profile and multi-device composite-key isolation is proven for reads,
  writes, cleanup, scheduled work, and visible UI state.
- Owner/device identity rebind and re-enrollment cannot attach old credentials
  or cached rows to a different owner, device, origin, or profile.
- Ambiguous requests replay the same immutable mutation/upload/ACK identity and
  never silently advance an unverified checkpoint or offset.

## PERFORMANCE/RESOURCES

- Transfer paths use bounded memory and streaming behavior; file bytes are not
  loaded into Room or unbounded in-memory buffers.
- Representative large metadata workload (including 5,000 nodes per library,
  1,000 queued mutations, 500 change events, and 100 conflict summaries)
  completes with stable ordering and bounded concurrency.
- Cursor/page/retry/database loops are bounded and instrumented where needed.
- Startup, scrolling, refresh, and transfer flows have no obvious ANR, runaway
  recomposition, unbounded retry, or resource leak on API 36 phone/tablet
  profiles.

## UX

- Onboarding and profile management explain configuration, active profile,
  connection, enrollment, re-enrollment, and local-forget semantics.
- Enrollment, authentication failure, device revocation, ambiguous outcomes,
  offline state, stale cache, pending work, conflict review, and recovery
  states provide actionable and truthful messages.
- Libraries and browser support the accepted logical operations with bounded
  pagination and safe destructive confirmations.
- Transfers expose source/destination selection, progress, cancel, retry,
  resume, completion, failure, low-storage, and URI-permission states.
- Metadata operations and synchronization expose pending, applied, conflict,
  paused, retrying, and owner/web-review states without inventing server
  capabilities.
- Settings expose network, battery, periodic scheduling, and cleanup behavior
  without placing secrets in preferences or WorkManager input.
- Accessibility passes for semantics, labels, contrast, touch targets, text
  scaling, TalkBack, keyboard/switch navigation, focus, and announcements.
- Light, dark, dynamic/system theme, phone, tablet, and large-window layouts
  are usable and do not alter authorization or data boundaries.

## DOCUMENTATION

- `clients/android/README.md` accurately describes implemented behavior,
  unsupported features, BrowserSession-only limitations, build requirements,
  runtime gates, and release behavior.
- `clients/android/docs/SYNC_ARCHITECTURE.md` matches the accepted sync,
  mutation, transfer, Room, and recovery implementation.
- `ANDROID_ROADMAP.md`, `ANDROID_PROGRESS.md`, and this file agree on milestone
  names, dependencies, statuses, readiness tokens, and mandatory gates.
- Build, test, API 36 emulator, release artifact, signing, rollout, rollback,
  and troubleshooting instructions are current.
- English documentation remains consistent with the relevant `docs/en/`
  architecture, API, security, sync, deployment, and release contracts.

## RELEASE/MERGE

- Android CI runs appropriate debug/release, JVM, lint/static, artifact, and
  API 36 instrumentation gates, with emulator limitations reported honestly.
- No unresolved implementation blocker remains for the requested Android
  release scope.
- No known P0/P1 correctness, data-loss, authorization, privacy, or upgrade
  defect remains.
- Every milestone commit is present on `origin/android-app` and the final
  branch contains only reviewed source/documentation changes.
- `HEAD == origin/android-app` and `git status --short` is empty.
- `android-app` is ready for review and merge into `main`; no direct merge or
  store publication is performed by the milestone workflow.

## Final Completion Token

Only after every section above is freshly verified and every roadmap milestone
is `COMPLETE`, emit:

`SYNVEIL_ANDROID_APP_COMPLETE`
