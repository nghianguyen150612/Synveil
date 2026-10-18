# PROMPT059 — Full iOS v0.1 Android Parity Audit and Regression Hardening

## Baseline and branch

- Repository: `nghianguyen150612/Synveil`.
- Integration branch: `ios-app`.
- Verified P058 merge baseline and P059 starting SHA: `c0c4c42ed41f589f5c7782e697824d89c4f8e907`.
- PR #142 is merged into `ios-app` at that exact merge commit. Its four iOS checks passed; the simulator run recorded 2,019 passed, 2 skipped, and 0 failed.
- P059 branch: `ios/p059-full-parity-audit`, created directly from `origin/ios-app` at the verified baseline.
- No P059 branch or PR existed when the audit began.
- P055 PR #139 remains open and unmerged. No P055 code was cherry-picked or included in this work.
- GitHub open issue searches for iOS/Android returned no matching issues.
- P060 is the next milestone and is not started by this work.

## Scope and source review

This is a source-audited verification and correction milestone. It reviewed Android and iOS source,
UI entry points, security and persistence boundaries, lifecycle behavior, tests, manifests P022–P058,
and the current OpenAPI. The primary documents are the iOS product contract, platform mapping,
roadmap, architecture, CI architecture, Rust/Swift FFI contract, shared-core reuse audit, Android
Definition of Done/progress/roadmap/release/README, iOS README, Photos and Backup documentation,
ADR-007, and `api/openapi.yaml`.

### Implemented and verified functionality

The following host-app behaviors are implemented in native Swift/SwiftUI and covered by source,
tests, and the merged milestone evidence recorded through P058:

| Behavior | Source/evidence | Qualification |
| --- | --- | --- |
| Server reachability, one-time enrollment, DeviceBearer session restoration, and local logout | P022–P029; `SessionController`, restoration services, Keychain store, and enrollment services | Server authorization is checked before the session becomes authenticated. Local logout removes the credential locally and does not claim server revocation. |
| Paginated library and logical-node browsing, breadcrumbs, active/trash views, and saved metadata | P031–P047; library/node services, SQLite projection, offline browser | Offline state is labeled; eligible cached reads remain fenced by current authorization evidence. |
| Create/rename/move/trash/restore and durable mutation replay | P034–P047; mutation service and SQLite queue | Conflict resolution remains in the owner/web workflow; no DeviceBearer conflict winner is fabricated. |
| Foreground upload, verified download, preview, and export | P038–P051; P045 upload coordinator, P043 download, SwiftUI document/share flows | Uploads use a single immutable operation at `POST /api/v1/upload-sessions`; content transfer requires HTTPS. Background file transfer is capability-off under P051. |
| Inbound feed, transactional application, signed ACK, and rebaseline recovery | P039–P047; device-scoped sync services and SQLite transaction boundaries | These operations use the current `/devices/{device_id}/libraries/{library_id}/changes` route family. |
| Default-off opportunistic metadata refresh | P048 merged through PR #131; `BGAppRefreshTask` coordinator/scheduler | Bounded to eligible work. iOS controls delivery; completion of a batch does not mean all libraries are current. |
| Notification and sync preferences | P049–P050 | Local notification and settings behavior is implemented; APNs wakeups are not claimed. |
| P056–P058 PhotoKit host-app flow | P056–P058 merged through P058 PR #142 | Opt-in discovery, private resource preparation, and foreground P045 upload orchestration are implemented. This is post-baseline work, not proof of a backed-up PhotoAsset. |

### Valid iOS-native equivalents

- SwiftUI replaces Android Compose for setup, library, node, mutation, transfer, and settings flows.
- Keychain Services with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` stores generic-password
  credentials. This uses device-only Keychain data protection; the implementation does not create a
  Secure Enclave key. Physical-device protection behavior remains unqualified.
- App-private SQLite replaces Room for durable metadata, sync, and mutation projections.
- Security-scoped document picker URLs and iOS share/export flows replace SAF `content://` URIs and
  Android intents.
- `NWPathMonitor` and persisted policy replace Android connectivity observation where supported.
- `BGAppRefreshTask` is the iOS-native metadata-refresh mechanism. It cannot promise a 15-minute
  cadence, battery threshold, or delivery time.

### Android-only mechanisms

WorkManager, Android Doze, Android Room, Preferences DataStore, Android Keystore APIs, SAF
`content://` URIs, and Android notification channels have no direct iOS implementation. Native
equivalents preserve the supported behavior where available. Android's user-selectable
battery-not-low and minimum-period settings have no exact iOS counterpart; iOS uses system
scheduling, Low Power Mode gating, and Wi-Fi-only or Wi-Fi/cellular eligibility.

### Features intentionally deferred or capability-off

- Production Files app integration remains outside v0.1. The `NSFileProviderReplicatedExtension`
  source is isolated and fail-closed; app startup does not register a production domain. P055 PR
  #139 remains unmerged and excluded.
- Authenticated background upload/download remains disabled under the P051 redirect security gate.
  Foreground P045 uploads and P043 downloads remain available.
- PhotoKit P056–P058 is a separately authorized, post-baseline host-app feature. The current server
  has no verified PhotoAsset, PhotoResource, or Live Photo import binding and no proof connecting
  these uploads to a BackupSnapshot or retention record. `FileVersion` upload completion alone is
  not backup completion under ADR-007.
- APNs remote wakeups and guaranteed periodic background execution are not implemented.
- Multi-profile switching remains a product decision; P059 does not expand that scope.

### Missing, corrected, or not fully qualified

1. **Endpoint security/parity defect corrected in P059.** Before this change, iOS accepted arbitrary
   HTTP hosts and API subpaths, while its HTTP transport only permitted numeric loopback. Setup could
   accept an origin that later failed transport policy. iOS now aligns with Android canonical-origin
   rules: HTTPS by default, optional debug numeric loopback HTTP, origin-only paths, no userinfo,
   query or fragment (including empty delimiters), whitespace or backslashes, malformed ports, or
   origins over 2,048 UTF-8 bytes. Numeric-looking malformed IPv4 hosts are rejected. The transport
   default follows the same `#if DEBUG` policy. Regression coverage was added for these cases.
2. **Debug-only transfer difference remains explicit.** iOS foreground raw upload and content
   download transports require HTTPS even for debug loopback. This is stricter than Android's debug
   loopback transport exception and does not affect production HTTPS parity; the existing iOS
   transfer security tests preserve that policy.
3. **Profile labels and switching are absent.** iOS setup stores one endpoint/session and has no
   human-readable server label, profile list, or profile switcher. Android supports labeled
   multi-profile storage. Product/access-group behavior remains unresolved under CAP-13.
4. **Photos server proof is absent.** PhotoKit observations or uploaded FileVersions cannot be
   described as PhotoAsset backup, Live Photo backup, or retained snapshot content.
5. **Device/signing evidence is unavailable.** P059 does not claim physical iPhone Keychain,
   BGTaskScheduler, real PhotoKit, live File Provider, or signed-archive validation. Those belong to
   the separate P060 release decision only where in-scope.

## Documentation corrections

- Replaced obsolete upload examples with current `/api/v1/upload-sessions` routes and device-scoped
  feed/ACK examples in the iOS contract and architecture documents.
- Marked the multipart `/api/v1/uploads` blueprint in English and Vietnamese upload references as
  historical; linked the current OpenAPI contract.
- Corrected current P048 and P058 status after their merges, the stale P058-deferred roadmap entry,
  the old Prompt015 iOS README status, and Photos documentation that implied no client implementation
  existed.
- Corrected the legacy File Provider API name and removed Secure Enclave claims unsupported by the
  current generic-password Keychain implementation.
- Separated P059 parity verification from P060 release validation and removed File Provider/PhotoKit
  post-baseline qualification from automatic P060 release requirements.

## P059 verification

### Portable checks completed

- `python3 clients/ios/Support/validate_ios_sources.py` — passed.
- `python3 -m unittest discover -s clients/ios/Support/tests -p 'test_*.py'` — 120 tests passed.
- `bash scripts/validate-docs.sh` — passed.
- `git diff --check` — passed.

### macOS CI and device evidence

P059 exact-head iOS Static Validation, iOS Build, iOS Rust Apple Build, and iOS Simulator Tests
must be recorded after branch push. This Linux workspace has no Swift/Xcode toolchain and cannot
claim native compilation. Physical-device validation remains `NOT_AVAILABLE`; no signed archive or
release-readiness claim is made here.

PR #142's four iOS workflows passed on the P058 source head. Its unrelated repository checks also
included Linux packaging, broad Rust workspace/desktop, and PostgreSQL scheduled-maintenance
failures; those are not iOS checks and are not evidence against the P058 iOS source.

## Release boundary

P059 records parity categories and correction evidence only. It does not declare v0.1 release
readiness, sign or package an application, change P055's open state, or begin P060.
