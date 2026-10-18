# Synveil iOS v0.1 — P059 Full Parity Audit Report

**Audit baseline:** `c0c4c42ed41f589f5c7782e697824d89c4f8e907` (P058 merge into `ios-app`)  
**Audited feature branch:** `ios/p059-full-parity-audit`  
**Source-fix SHA:** `373ae476d7045d8cbfb709b256085e6442a69163`  
**Initial audit closeout:** `3c67e190bbab002c2609f748a1a52ef192be3190`  
**Source of truth:** [Parity matrix](IOS_V0_1_PARITY_MATRIX.md), [iOS contract](IOS_V0_1_PRODUCT_CONTRACT.md), [Android Definition of Done](../../clients/android/ANDROID_DEFINITION_OF_DONE.md), [Android progress](../../clients/android/ANDROID_PROGRESS.md), and [P059 manifest](manifests/PROMPT059_MANIFEST.md).

## Executive conclusion

P059's substantive iOS parity and endpoint-hardening source changes passed all four required iOS workflows on the source-fix commit. However, **the audit does not establish full Android v0.1 parity or production release readiness**: labeled server profiles and multi-profile switching remain absent on iOS, and physical-device, signed archive, live authenticated owner-server and background-scheduling acceptance were not performed.

The P059 audit can be integrated as a *documented audit milestone* at the owner's direction. This is **not equivalent to closing all P060 release gates**, accepting a missing feature as PASS, or authorizing distribution. P060 is allowed to reach `BLOCKED_RELEASE` when the remaining required evidence is unavailable.

## Complete capability classification

This report uses the itemized 42-row source/test-backed [parity matrix](IOS_V0_1_PARITY_MATRIX.md).

| Classification | Count | Interpretation |
| --- | ---: | --- |
| PASS | 18 | Supported core parity behavior with source and simulator evidence |
| IOS_EQUIVALENT | 12 | Apple-native behavior with explicit platform constraints |
| NOT_APPLICABLE | 6 | Android-only framework mechanisms mapped to other rows |
| INTENTIONALLY_DEFERRED | 4 | Optional/blocked post-baseline capabilities, kept OFF |
| BLOCKED | 2 | Required Android product behavior missing or unresolved (CAP-13) |
| **Total** | **42** | Device and live-deployment acceptance separate from source-backed classifications |

`PASS`/ `IOS_EQUIVALENT` above are *functional implementation audit classifications*, **not** claims of device certification. All identified gaps are documented below; no unverified behavior has been promoted to a release claim.

## Android reference and method

The Android baseline is `clients/android/`, especially `ANDROID_DEFINITION_OF_DONE.md`, `ANDROID_PROGRESS.md`, `ANDROID_ROADMAP.md`, and checked-in Kotlin source and tests. The Android completion record documents gradle builds/lint, Room, Keystore, WorkManager, transfer recovery, accessibility and API 36 instrumentation. P059 did **not** rerun those on a connected Android device; this is prior recorded evidence and is not an iOS-equivalent native runtime observation.

For iOS, source and XCTest were checked against original iOS product contract, Rust/Swift FFI contract, verified P022–P058 implementation history, and the current OpenAPI. Prior manifests and CI statuses were distinguished from actual P059 source-head evidence. A modern Android internal API is **not** required verbatim on Apple platforms; product-level authorization, data integrity and user-visible recovery must match.

## Defects found and resolved in P059

### SEC-P059-01 — Canonical origin policy mismatch (corrected)

Previously the iOS server endpoint model accepted arbitrary HTTP hosts and API subpaths while the actual transport rejected them. Setup could accept unusable or insecure endpoints. P059 now rejects:

- Non-HTTPS production endpoint origins (except conditional debug numeric loopback policy).
- Embedded credentials, query or fragment, and additional URL path.
- Empty query/fragment delimiters, whitespace, backslashes and oversized inputs.
- Invalid ports, malformed numeric-looking IPv4 and incorrectly scoped loopback rules.
- Bracketed IPv6 loopback inconsistencies.

**Evidence:** `clients/ios/Domain/Configuration/ServerEndpoint.swift`, `EndpointValidationError.swift`, `ServerSetupViewModel.swift`, `Tests/SynveilTests/ServerEndpointTests.swift`, and `ServerSetupViewModelTests.swift`.

**Regression history:** The initial P059 simulator CI exposed four regressions: IPv6 bracket handling, two old route expectations and a duplicate-slash fixture. Tests were corrected as supported by the current OpenAPI; the corrected source commit passed the full simulator suite. This is not a waiver to permit insecure HTTP.

### DOC-P059-02 — Outdated API and implementation documentation (corrected)

The audit updated obsolete upload/feed/ACK examples, stale iOS README/roadmap statuses, Keychain/Secure Enclave wording, PhotoKit scope, File Provider status and the Rust/Swift FFI contract. Current upload sessions follow `/api/v1/upload-sessions`; the documented client sync route family is device-scoped. Legacy endpoint examples remain historical, not newly implemented APIs.

## Open parity blockers

### CAP-13A — Labeled server profiles

Android persists multiple labeled profiles. Current iOS setup stores one endpoint/session, without a matching profile label/list model. **Status: BLOCKED (PAR-03).**

P060 disposition: implement an authenticated-safe, persistent labeled profile model with migration and tests **or** obtain an explicit product-owner decision narrowing iOS v0.1's required scope and update the authoritative contract. A silent deferral or assuming that a URL is a label is unacceptable.

### CAP-13B — Multi-profile switching

Android supports switching saved server profiles with credential/cache isolation; iOS currently has no equivalent selector. **Status: BLOCKED (PAR-04).**

P060 disposition: implement profile-aware endpoint selection, Keychain scoping, queued-operation separation, logout/switch barriers, tests for stale callbacks and migration **or** make a separately approved product-scope revision. Do not loosen the existing single-session security boundary merely to satisfy a parity checkbox.

These two requirements are separate but related; a single targeted P060 remediation plan may cover both. They must not be counted as PASS merely because P059 is merged.

## Explicitly deferred or capability-OFF

- P055 PR #139 File Provider content/production Files operations: not integrated; no production domain registration. This was a separate post-baseline extension, **not** a missing Android v0.1 parity requirement.
- P046 background upload and P051 background download: OFF due to redirect/credential/content security restrictions. Foreground P045/P043 remains the supported path.
- PhotoKit P056–P058: opt-in, foreground-only PhotoKit observation/staging/upload exists, but no authoritative PhotoAsset/Live Photo binding, BackupSnapshot association or retention proof. A FileVersion upload is not a completed retained photo backup.
- APNs wakeups and guaranteed Android-style scheduling cadence: unsupported / not promised.

Do not activate any of these to force a parity score.

## Security findings and continuing risks

| Area | P059 finding | Status for P060 |
| --- | --- | --- |
| HTTPS / endpoint validation | Canonical origin restrictions tightened | Source/test FIXED; live deployment check pending |
| Keychain credentials | Generic-password ThisDeviceOnly, not a claimed Secure Enclave key | Device protection and upgrade/logout test pending |
| Transfer integrity | P043/P045 hash, offset and durable recovery paths in scope | Mocked native PASS; live server/integrity drill pending |
| SQLite scope and crash safety | Owner, device, generations and transactions reviewed | Native fault tests PASS; on-device storage failure gate pending |
| FFI | Rust Apple and Swift tests PASS | Signed-device ARM64 runtime ABI gate pending |
| Foreground PhotoKit backup | Background disabled, files not backup-proof | Product claims must remain limited |
| File Provider P055 | Open PR excluded, production OFF | Review separately, do not promote to release |
| Background transfer redirects | P051 security gate unresolved, capabilities OFF | Any activation BLOCKED pending separate security acceptance |

No blanket independent security-review approval is asserted by this source audit.

## CI and execution evidence

### Verified native P059 source commit

| iOS gate | Evidence | Result |
| --- | --- | --- |
| Static Validation | [39093259832](https://github.com/nghianguyen150612/Synveil/actions/runs/39093259832) | PASS |
| iOS Build | [39093259817](https://github.com/nghianguyen150612/Synveil/actions/runs/39093259817) | PASS |
| Rust Apple Build | [39093259786](https://github.com/nghianguyen150612/Synveil/actions/runs/39093259786) | PASS |
| Simulator Tests | [39093259808](https://github.com/nghianguyen150612/Synveil/actions/runs/39093259808) | 2,024 PASS, 2 skipped, 0 failed |

**Portable checks (reported by coding agent):** iOS source validator PASS; Python support tests 120 PASS; docs validation PASS; `git diff --check` PASS. The native simulator report is independently retrievable from GitHub Actions. Docs-only closeout on `3c67e190` did not modify executable source; any later doc updates require fresh exact-head CI evidence to be recorded, not inferred.

### Unrelated repository-wide issues

The P059 source head reported Rust CI failures in desktop/shared-workspace jobs including unsupported `systemd-tmpfiles --dry-run`, macOS client-control and rustls-policy advisories, plus Linux AppImage embedded build-path scan failure. PostgreSQL and DEB/RPM were still in progress at initial closeout. **These are not PASS and should be handled as separate repository-wide work.** Evaluate the actual current CI state before promoting a global release.

## P060 readiness and release gate checklist

P060 owns the decision; P059 does not certify a production IPA.

1. **Parity:** CAP-13 profile labels and multi-profile switching must be resolved or owner-approved as an explicit v0.1 scope revision.
2. **Source and reproducibility:** clean merged `ios-app` HEAD, immutable source release tag, reproducible iOS build.
3. **Native CI:** four iOS gates PASS on final source and release candidate commit; count failed/skipped XCTest truthfully.
4. **Signing:** correct Apple team, bundle identity, provisioning, release entitlements and distribution certificate; no signing secrets in Git.
5. **Physical iPhone:** Keychain ThisDeviceOnly, local file protection, enrollment/logout, iOS permissions, scene transitions, BGAppRefresh best-effort, accessibility and performance.
6. **Authenticated server fixture:** complete end-to-end enrollment, browsing, mutation, sync feed/ACK, upload/download hashes, recovery and revocation against a controlled backend.
7. **Security:** review redirects, FFI memory, signed archive, persisted SQLite migration/recovery, push/privacy/PhotoKit consent.
8. **Release artifact:** inspect archive/IPA for debug credentials/paths, minimum OS, required permissions, build/version identity and app privacy declaration.
9. **Rollout:** TestFlight candidate, install/upgrade/rollback and regression testing, known-limitations and support documentation.
10. **Scoped exclusions:** preserve P046/P051 OFF, P055 unmerged, PhotoAsset binding/retention claims OFF; no P059/P060 operation may quietly flip them.

**Decision:** `AUDIT_COMPLETE_WITH_BLOCKED_PARITY_AND_RELEASE_GATES`. This allows a documented P059 audit integration upon owner approval but **does not mean** `COMPLETE_READY_FOR_PRODUCTION_RELEASE`.

**Next:** P060 release gate. If physical-device or required parity proof is missing, P060 must end `BLOCKED_RELEASE` or a similarly explicit status—not fabricate acceptance or automatically merge `ios-app` to `main`.
