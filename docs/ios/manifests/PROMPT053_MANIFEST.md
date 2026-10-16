# P053 — File Provider Foundation, App Group Isolation & Secure Integration Contracts

## Checkpoint and product status

- Verified integration baseline: `ios-app` at `b51b4ddd10aac4097c285e6bba4ec165496466c1`.
  `origin/ios-app` was fetched and P052 was verified as an ancestor before implementation.
- Feature branch: `ios/p053-file-provider-foundation`.
- Validated P053 implementation source SHA: `bc8761ab8553dd32cc8ca07a5173b3c65d1b3faa`.
- Pull request: [#136](https://github.com/nghianguyen150612/Synveil/pull/136), open against
  `ios-app`.
- Review: requested from `ducminh231011`; no review has been submitted yet.
- Merge: not merged. No integrated SHA is claimed; `ios-app` remains at the P052 baseline until
  review and merge gates are complete.
- The product contract continues to classify Apple Files support as
  `IOS_DEFERRED_AFTER_V0_1`. P053–P055 are an owner-authorized post-baseline workstream. This
  foundation does not change v0.1 release eligibility, Android parity claims, or production feature
  claims.
- No File Provider domain is registered by app startup. P054 remains unauthorized and has not
  started.

## Apple API selection

P053 selects the replicated File Provider architecture: `NSFileProviderReplicatedExtension` and
`NSFileProviderEnumerating`, with the framework's `NSFileProviderEnumerator` callbacks. Apple
documents the replicated protocol for iOS 16 and later; the extension target's iOS 17 minimum is
compatible. The replicated model is intended for a remote hierarchy mirrored by the system and
provides explicit enumeration, item lookup, content, mutation, version, and invalidation contracts.
P053 conforms to the real protocol and uses its current callback signatures, but fails closed before
claiming that remote synchronization is operational.

The legacy `NSFileProviderExtension` subclass model is not used. `NSFileProviderItem` and
`NSFileProviderItemVersion` adaptation, `NSFileProviderManager` registration, and
`NSFileProviderDomain` creation are not implemented as live production operations. The typed seams
and version mapping are groundwork only.

Official references reviewed:

- [File Provider](https://developer.apple.com/documentation/fileprovider)
- [Replicated file provider extension](https://developer.apple.com/documentation/fileprovider/replicated-file-provider-extension)
- [`NSFileProviderReplicatedExtension`](https://developer.apple.com/documentation/fileprovider/nsfileproviderreplicatedextension)
- [`NSFileProviderEnumerating`](https://developer.apple.com/documentation/fileprovider/nsfileproviderenumerating)
- [`NSFileProviderEnumerator`](https://developer.apple.com/documentation/fileprovider/nsfileproviderenumerator)
- [`NSFileProviderItem`](https://developer.apple.com/documentation/fileprovider/nsfileprovideritem)
- [`NSFileProviderItemVersion`](https://developer.apple.com/documentation/fileprovider/nsfileprovideritemversion)
- [`NSFileProviderManager`](https://developer.apple.com/documentation/fileprovider/nsfileprovidermanager)
- [`NSFileProviderDomain`](https://developer.apple.com/documentation/fileprovider/nsfileproviderdomain)

## Xcode target and build structure

- Existing `Synveil` and `SynveilTests` targets are preserved.
- New native app extension target: `SynveilFileProviderExtension`.
- Product: `SynveilFileProviderExtension.appex`; bundle identifier:
  `com.synveil.ios.fileprovider`.
- Extension principal class: `SynveilFileProviderExtension.SynveilFileProviderExtension` after
  build-setting substitution.
- Extension point: `com.apple.fileprovider-nonui`.
- Minimum iOS version: 17.0; `APPLICATION_EXTENSION_API_ONLY = YES`.
- The host app has an explicit target dependency and embeds the `.appex` under `PlugIns` with
  `CodeSignOnCopy` and `RemoveHeadersOnCopy`.
- File Provider sources are in the extension target. Only the non-UI domain, identity, capability,
  authorization, lifecycle, shared-container, and projection contracts needed by the host are
  shared with the app target. The extension principal, callbacks, and framework-only adapter helpers
  are not compiled into the host app. No Rust FFI libraries or app startup files are linked into the
  extension.
- The existing `Synveil` scheme remains the build entry point; its host dependency builds the
  extension. The iOS Build workflow checks for the embedded `.appex` and verifies its bundle ID,
  extension point, principal class, and document group.
- Project signing uses automatic signing configuration without a committed developer team or
  identity. Unsigned Simulator compilation does not prove entitlement activation, installation, or
  Files recognition.

## App Group and shared storage boundary

The configured group is `group.com.synveil.ios`. It matches the reverse-DNS stem of the host app
bundle identifier `com.synveil.ios` and is used by both targets' entitlement files and
`NSExtensionFileProviderDocumentGroup`. Apple Developer portal group registration and compatible
provisioning profiles are still required to activate it on a signed device build.

`FileProviderSharedContainer` resolves only that group with
`FileManager.containerURL(forSecurityApplicationGroupIdentifier:)`. Missing entitlement/container
is a typed failure; it never falls back to the app sandbox. Storage is limited to
`Synveil/FileProvider/v1`, with component validation and symlink rejection. Directories and staged
files use owner-only POSIX permissions, `completeUntilFirstUserAuthentication`, and backup
exclusion. Snapshot files are capped at 8 MiB each, provider storage at 64 MiB, and directory scans
at 8,192 entries. Publication validates a complete typed snapshot, encodes sorted JSON, writes and
synchronizes a private same-directory staging file, and replaces the current generation only after
validation. Publication is disabled by default and is not wired into app startup.

The existing SQLite v9 database stays app-private. The extension never opens it and is not a second
SQLite writer. If a later milestone requires shared metadata, the host app must publish a bounded,
validated, immutable projection through this boundary. Any future shared database proposal needs a
separate ownership, migration, locking, crash-recovery, and rollback review; P053 does not relocate
or migrate SQLite.

## Keychain decision and authorization interface

P053 resolves `DECISION-01` by preserving current app-only Keychain isolation. The host continues
using its default Keychain access group; neither target adds a Keychain sharing entitlement, and no
existing DeviceBearer credential is migrated or made extension-readable. No raw bearer, enrollment
token, request header, or secret is stored in the App Group.

`FileProviderAuthorizationEvidence` is a versioned, non-secret interface for a future
app-mediated authorization protocol. P053 does not publish or consume it at runtime. It binds domain,
owner scope, credential-generation fingerprint, authorization generation, logout, revocation,
account-switch, and protected-data evidence. Production metadata access remains disabled, so even
matching evidence cannot authorize an operation. Credential replacement creates a different domain
identity. Any future extension credential protocol needs a separate security/product decision and a
proven migration and rollback plan.

## Domain and item identities

One opaque domain represents one canonical server endpoint, account, device, and non-secret
credential generation. A domain contains multiple Library roots. It is stable across ordinary
relaunches while those values remain stable. Replacing credentials or accounts creates a different
domain; an old domain cannot silently attach to a new owner. The visible domain identifier is a
versioned SHA-256 fingerprint and contains no endpoint, account identifier, or bearer secret.

Item identifiers use `sfpi1` and bind the domain fingerprint, Library fingerprint, kind, and
canonical Synveil UUIDv7 identifier. The domain root has a fixed scoped representation; a Library
root and Node identifiers are reversible base64url encodings of validated Synveil IDs. Display
names, absolute paths, and local SQLite row numbers are not identifiers. Renaming or moving a Node
does not change its item ID. Library, account, endpoint, device, and credential-generation scope
prevent cross-domain collisions. Immutable FileVersion IDs use a separate `sfpv1` identity bound
to domain, Library, Node, and FileVersion. Corrupt, malformed, wrong-scope, and unsupported
identifiers fail validation.

`FileProviderItemVersionContract` maps immutable FileVersion evidence and server metadata revision
to 32-byte SHA-256 components. Missing authoritative metadata remains nil; local timestamps are
never substituted. Metadata revisions are bounded opaque strings.

## Versioned metadata projection and storage

`FileProviderMetadataSnapshot` schema v1 includes domain identity, non-secret owner fingerprint,
Library identity, projection generation, completeness, stable item and parent identity, item kind,
display name, optional authoritative byte length and timestamps, immutable FileVersion and SHA-256
evidence, metadata revision, tombstone state, and source provenance.

Validation rejects unknown future schemas, mismatched scope/Library, partial or empty projections,
duplicate or invalid identities, invalid parentage or cycles, excessive record counts/payloads,
invalid display names/revisions, and unresolved local mutation or conflict rows. A queued local
rename is not server-confirmed metadata. P050 conflict-review evidence remains app-private and is
not projected as canonical state. Snapshot publication and consumption have deterministic typed
adapters and fixtures; production publication remains disabled.

## Extension lifecycle and operation behavior

The principal class has an independent, lightweight process lifecycle. Construction does not load
SwiftUI, a scene, the app dependency container, Keychain, SQLite, URLSession, P052 recovery, a
background transfer scheduler, shared storage, or a domain manager. It performs no automatic sign-in,
server request, or domain registration. Repeated initialization is idempotent; invalidation prevents
further request work. A cold relaunch begins with no authorization evidence.

Enumeration and item lookup return Apple's `notAuthenticated` error while an authorized complete
Library is unavailable. They do not report an empty Library. `fetchContents` returns no URL and an
unavailable error. Create, modify, and delete return `cannotSynchronize`; no content is discarded and
no mutation reports success. The extension has no production File Provider domain and does not
return a fake successful empty enumerator. P046 background upload and P051 background download
remain disabled; P052 recovery is not called.

## Security threat model and schema decision

Threats considered include unauthorized process launch, missing/mismatched group entitlement,
account or credential replacement, stale authorization after logout/revocation, protected data
unavailability, symlink/path traversal, overlarge snapshots, future/corrupt schemas, partial
publication, stale prior-account snapshots, malicious or unresolved local mutation evidence,
unverified file bytes, sensitive logs, and accidental cross-process SQLite or Keychain sharing.

Mitigations in P053 are closed-by-default capabilities, app-only credentials, no network path in
extension initialization, explicit group resolution, fixed storage root, bounded and typed snapshots,
scope fingerprints, atomic staging/replacement, owner-only permissions and Data Protection, no file
content cache, and fail-closed File Provider callbacks. SQLite stays schema v9; no migration is
needed. Device entitlement activation, App Store acceptance, OS process behavior, and actual Files
integration are not established by Simulator compilation.

## Deterministic test inventory

New XCTest methods in `FileProviderFoundationTests.swift`:

- `testDomainIdentityIsStableScopedAndCredentialGenerationBound`
- `testDomainIdentityRejectsNonCanonicalOrSecretScopeValues`
- `testItemIdentitySurvivesRenameAndMoveAndSeparatesLibraryAndDomain`
- `testFileVersionIdentityIsImmutableAndDistinctFromNodeIdentity`
- `testProductionRegistrationAndAllRemoteCapabilitiesAreDisabled`
- `testInjectedDomainCoordinatorSupportsFutureRegisterDisconnectAndRemove`
- `testAuthorizationEvidenceIsNonSecretAndRequiresExactScope`
- `testAuthorizationDeniesWrongCredentialGenerationLogoutRevocationAndAccountSwitch`
- `testCredentialReplacementAndExtensionRelaunchStartWithoutAuthorization`
- `testLifecycleInitializationCancellationAndInvalidationAreLocalAndIdempotent`
- `testSnapshotProjectionIsVersionedCanonicalAndDeterministicallyOrdered`
- `testSnapshotRejectsFutureSchemaCorruptionWrongOwnerAndUnresolvedConflict`
- `testPartialProjectionAndInvalidParentageAreRejected`
- `testAppGroupResolutionAndUnsafeOrSymlinkPathsFailClosed`
- `testSnapshotPublicationFailurePreservesPreviousGeneration`
- `testUnsupportedOperationsNeverReportSuccess`

The portable support suite adds target/product/entitlement/source-membership and fail-closed
contract assertions. Existing P001–P052 XCTest sources remain in the scheme and are not removed,
skipped, or weakened. The host `SynveilTests` target exercises the pure cross-process contracts;
the extension principal and callbacks are compiled by the embedded target. These mocks and
Simulator results do not claim Files app integration.

## Portable validation and exact-head CI

Portable validation on implementation source SHA `bc8761ab8553dd32cc8ca07a5173b3c65d1b3faa`:

- `python3 clients/ios/Support/validate_ios_sources.py`: passed.
- `python3 -m unittest discover -s clients/ios/Support/tests`: 112 passed.
- `bash scripts/validate-docs.sh`: passed.
- `git diff --check`: passed.
- Strict Swift formatting and native syntax/build checks were run on macOS CI. The local Linux
  workspace has no Xcode, Swift compiler, or `swift-format`.

All four required native workflows passed on the exact P053 implementation source SHA above. The
pull-request runs below are all attached to `bc8761ab8553dd32cc8ca07a5173b3c65d1b3faa`:

| Gate | Result | Exact-head run |
| --- | --- | --- |
| iOS Static Validation | PASS | [Run 38854750751](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750751) |
| iOS Build, including embedded `.appex` metadata inspection | PASS | [Run 38854750740](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750740) |
| iOS Simulator Tests | PASS — 1,947 passed, 0 failed, 2 skipped | [Run 38854751065](https://github.com/nghianguyen150612/Synveil/actions/runs/38854751065) |
| iOS Rust Apple Build | PASS | [Run 38854750752](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750752) |

The Simulator workflow built the host app, its File Provider extension dependency, and the
`SynveilTests` scheme on iPhone 17 Pro / iOS 26.5 Simulator. The build workflow separately checked
that the embedded product is at `Synveil.app/PlugIns/SynveilFileProviderExtension.appex` and that
its bundle ID, extension point, principal class, and document group match the manifest. This proves
compilation and product embedding only; it does not prove signing, entitlement activation, Files
recognition, or production synchronization.

The first Simulator attempt on `a3ca692fb15ea4e638c09d169f61d75f2dea07ca` stopped during test
source compilation because two malformed authorization fixtures used a non-existent initializer
and one throwing assertion omitted `try`. The fixtures were changed to decode deliberately corrupted
encoded evidence and the assertion was corrected; the full suite then passed on `f7e6a09`.

A later Simulator run on `157912cf780686e26b252d53f3e4fd83114d550d` exposed a race in the existing
catalog loading test: the immediate mock response could publish `.loaded` before the test observed
`.loading`. The fixture now suspends the request until after the loading assertion, then completes
it explicitly. On `bc8761a`, the full suite passed with 1,947 passed, 0 failed, and 2 skipped. No
existing test was removed, skipped, or weakened.

Unrelated repository workflows on the same implementation source SHA also reported failures; they
are not iOS File Provider gates and were not changed as part of P053:

- [Rust CI run 38854750763](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750763):
  three `synveil-client` IPC/control tests failed on macOS (80 passed), Linux Qt clippy reported two
  `synveil-object-store` `must_use` errors, and Windows builds hit Unix-only APIs in
  `synveil-install-engine`.
- [Linux AppImage run 38854750879](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750879):
  artifact validation found `/home/` in the packaged desktop binary.
- [PostgreSQL 17 run 38854750991](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750991):
  was still in progress when this manifest revision was prepared.
- [PostgreSQL 17 push run 38854746603](https://github.com/nghianguyen150612/Synveil/actions/runs/38854746603):
  `postgres_scheduler_tick_schema_is_current_on_postgresql_17` expected 34 and observed 36.
- [Linux native DEB/RPM run 38854750746](https://github.com/nghianguyen150612/Synveil/actions/runs/38854750746)
  was still in progress when this manifest revision was prepared; it is unrelated to native iOS
  gates.

The PR-triggered iOS runs are the exact-head evidence for this manifest. No P052 result is reused as
P053 validation. After this manifest revision, the source validator passed, all 112 portable support
tests passed, documentation validation passed, and `git diff --check` passed.

## Physical-device qualification

Status: `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE` until a signed iPhone build, Apple Developer Team,
provisioning profile, and physical device are available. The following procedure is prepared; no
device result is claimed:

1. Register `group.com.synveil.ios` with the selected Apple Developer Team. Sign the app and `.appex`
   with the same team, matching App Group entitlement, bundle IDs, and provisioning profiles.
2. Install on an actual iPhone. Inspect the installed app's embedded `PlugIns` directory and verify
   iOS recognizes the File Provider extension. Confirm the extension does not appear as an active
   Synveil provider while registration is disabled.
3. Verify the Files app does not show a successfully synchronized empty Synveil Library and that
   no domain is present before explicit future authorization.
4. Install a build with the App Group entitlement intentionally absent in a disposable test profile.
   Confirm container resolution fails closed and no app-sandbox fallback is used.
5. With a test account and sanitized server request ledger, cold-launch the extension without
   starting the app. Confirm no Keychain access, automatic sign-in, authenticated request, transfer,
   or domain registration.
6. Sign into account A in the host app, then logout. Confirm old authorization evidence is
   invalidated and the extension cannot expose account A metadata.
7. Replace account A's credential generation and verify the old domain identity does not bind to the
   replacement credential.
8. Switch to account B and verify no account A snapshot or domain metadata is visible.
9. Lock the device before first unlock and while locked. Confirm protected data is unavailable and
   extension operations return unavailable/authentication errors without reading protected files.
10. Cold-launch, terminate the extension process, and trigger another Files request. Confirm a new
    process starts without inherited authorization and performs no automatic recovery.
11. Corrupt or truncate a test snapshot and verify no partial or empty enumeration is reported.
12. Exercise unsupported fetch, create, modify, and delete requests. Confirm no URL, bytes, successful
    item state, or server mutation is reported.
13. Compare the server-side request ledger before and after cold launch, cancellation, invalidation,
    logout, and relaunch. Confirm no unexpected request or mutation submission.
14. Inspect saved legacy P046 background upload tasks and verify extension launch does not resume
    them. Confirm P051 background downloads remain disabled.
15. Record device model, OS build, signing/team identifiers (without secrets), timestamps, extension
    recognition, sanitized request counts, and typed operation results.

The lack of physical-device evidence prevents production Files integration claims but does not
prevent completion of the isolated, buildable foundation.

## Deferred scope and prerequisites

| Requirement | P053 status | Prerequisites |
| --- | --- | --- |
| Full remote File Provider enumeration | Deferred to P054 | Authorized non-secret metadata boundary, complete snapshot semantics, stable anchors, change signaling, cancellation and account fencing. |
| Metadata change propagation into Files | Deferred to P054 | Server-confirmed revisions and validated enumeration/update adapter. |
| Production domain registration | Disabled | Operational enumeration and lifecycle readiness, explicit user authorization, and signing/device qualification. |
| Production item materialization | Deferred to P055 or separately authorized | Immutable FileVersion pinning, exact length and SHA-256 verification, owned staging, session fencing, verified-only publication. |
| Files-originated metadata mutations | Deferred to P055 or separately authorized | Durable intent, server revision preconditions, idempotency, conflict preservation and unknown-outcome handling. |
| Files-originated file uploads | Deferred; reviewed transfer design required | Explicit authorization, foreground/background policy, safe staging and transfer idempotency. |
| Cross-process bearer credential access | Security/product decision pending | P053 keeps credentials app-only; any change needs a separate reviewed protocol and migration/rollback plan. |
| App Store signing and entitlement acceptance | Device/provisioning validation | Apple Team, registered App Group and compatible profiles. |
| Physical Files app integration | Device validation | Signed device build and actual iOS Files execution. |
| P051 background transfer reactivation | Separate security remediation | Explicit authorization after P051 security containment and review; P053 does not enable it. |
| PhotoKit | Deferred to P056–P058 | Separate authorized roadmap work. |

## Review, merge, and closeout

- Pull request URL/state: [#136](https://github.com/nghianguyen150612/Synveil/pull/136), open.
- Required reviewer: `ducminh231011`, requested; review result pending.
- Merge SHA and verified post-merge `ios-app` SHA: none; PR is not merged.
- Validated P053 implementation source SHA: `bc8761ab8553dd32cc8ca07a5173b3c65d1b3faa`; the
  four exact-head native CI URLs are recorded above. A documentation-only closeout commit may
  advance the branch HEAD without changing the validated P053 implementation source.
- Manifest path: `docs/ios/manifests/PROMPT053_MANIFEST.md`.
- Next step after P053: satisfy review and exact-head validation gates. Do not start P054 without
  separate authorization.
