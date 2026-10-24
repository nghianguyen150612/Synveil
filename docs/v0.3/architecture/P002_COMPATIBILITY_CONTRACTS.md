# Synveil v0.3 — P002 compatibility and migration contracts

**Status:** proposed policy for maintainer review. This document records compatibility goals, not qualified client/server combinations.
**Source baseline:** develop/v0.3 at d69e167913feec5ebed548b60eb07cbdf40ba6e9.
**Authoritative baseline:** [P001 compatibility baseline](../audits/P001_COMPATIBILITY_BASELINE.md) and [P001 audit](../audits/P001_V02_BASELINE_AUDIT.md).

## 1. Compatibility scope and evidence

Synveil product version, HTTP wire version, OpenAPI description version, client build version and local database schema version are separate identifiers. Product source is currently 0.2.0. The Axum router declares API prefix /api/v1. OpenAPI info.version currently reads 0.1.0-blueprint. The repository does not establish a supported cross-version client/server matrix, and P001 did not qualify v0.2 release interoperability.

For this policy:

- **Existing contract** means behavior present in the baseline source and OpenAPI document.
- **Compatibility goal** means behavior P003–P060 should preserve or test.
- **Qualified** means an exact client, server, platform and relevant recovery path passed a recorded test. No broad v0.2 cross-version claim is made here.

## 2. HTTP API v1 contract

### 2.1 Versioning rules

1. Keep existing product resources under /api/v1. Do not create an API v2 solely because the product version advances to 0.3.
2. Preserve existing path, method, authentication class, request meaning, response meaning, status/error behavior, ownership checks and retry behavior. Any incompatible semantic change requires a separately approved migration and client-support plan.
3. Prefer new additive endpoints for genuinely new operations. Add optional request fields only when omission preserves the exact existing behavior. Do not make an existing request field newly required.
4. Add new response fields as optional. Clients must ignore unknown response fields where their decoder permits it; new clients must tolerate older servers omitting them. Before relying on that rule, add wire-contract tests for each maintained client because decoder behavior differs by platform.
5. Do not silently change error codes or HTTP status meaning. Add a stable error code only for a new condition; old clients should show a safe generic error when they do not recognize it.
6. State-changing operations must have a documented idempotency/replay contract. Existing operations keep their current operation IDs, body fingerprints, preconditions, and replay behavior. A new operation should persist a stable operation identity and request fingerprint before effects so retry cannot duplicate the effect.
7. Pagination cursors must be opaque, bounded and stable for their documented ordering. Adding sort fields or cursor components must not reinterpret existing cursors. Reject malformed or expired cursors safely.
8. Keep health/readiness paths separate from product API versioning: /live, /ready, /health/live and /health/ready remain deployment probes.

### 2.2 Authentication and authorization compatibility

The existing browser session cookie plus CSRF/provenance behavior remains the browser contract. Device enrollment grants and device bearer credentials remain a distinct device contract. New endpoints must declare whether they accept a browser session, a device credential, or both; they must not infer one identity from the other.

API authentication establishes a principal. API authorization continues to check current owner, library and device scopes server-side. Opaque resource identifiers, profile labels, network location, TLS termination or Tailscale membership do not grant application permission. Requests without required authentication, CSRF proof, owner scope, device binding or approval fail closed.

An old client keeps access to existing v1 operations it is already allowed to use. v0.3-only features are unavailable unless the client and server negotiate the corresponding capability and the authenticated principal is authorized. Compatibility does not mean retaining insecure behavior after an explicitly reviewed security deprecation.

### 2.3 Capability negotiation and minimum-client policy proposal

Proposed capability discovery is additive, for example a read-only endpoint under /api/v1/system or /api/v1/capabilities. It should return stable capability identifiers, contract revisions and optional server policy such as a minimum supported client version. It must not return secrets, internal host paths or topology details. Existing clients that do not call it continue on existing routes.

Proposed rollout rule:

- A server must continue existing v1 behavior by default while v0.3 capabilities are introduced.
- A client uses a new capability only when the server advertises it and the caller is authorized.
- An older supported client receives existing behavior or a clear unsupported-capability state. It must not retry through a weaker authorization path.
- A hard minimum-client block is reserved for a separately approved security or data-integrity requirement. It must use a stable error and actionable upgrade guidance, with server-side audit evidence.
- Minimum versions are per client family and tied to a tested support window, not inferred from Cargo version or OpenAPI metadata.

This is a proposal. The capability endpoint, capability names, minimum-version policy and legacy support window require an ADR decision and compatibility tests before implementation.

### 2.4 OpenAPI metadata policy

The current discrepancy is real: api/openapi.yaml reports info.version 0.1.0-blueprint while workspace package metadata is 0.2.0. Do not fix it by treating info.version as the wire API prefix or blindly copying the product release number.

Recommended correction policy: define info.version as the revision of the published API description, manage it independently from product release version and the /api/v1 wire namespace, and expose product build/release metadata separately only if clients need it. Correct the current stale value in a reviewed OpenAPI-only change after selecting the document-version format. Do not change route version or client compatibility as a side effect.

## 3. Client compatibility requirements

### 3.1 Client-specific state and behavior

| Client | Existing compatibility-sensitive state | v0.3 requirement |
|---|---|---|
| Web | Browser session cookie, CSRF cookie/provenance checks and single-flight session recovery; session data is server-side. Web has no owned SQL database. | Preserve login/logout/session and CSRF behavior. Session recovery may reconstruct safe GET state but must not replay a mutation POST. Hide unsupported optional capabilities without redirecting to weaker routes. Browser/version support remains unknown until a browser matrix is defined. |
| Desktop | Qt/QML UI is separate from synveil-client. Client process owns SQLite profiles, sync state and queues at schema version 8; secrets use the platform credential store. Local IPC is versioned and bounded. | Preserve UI/client process isolation and local IPC capabilities. An older client retains v1 sync and existing mutation/conflict behavior. Unknown local schema or incompatible IPC fails safely without deleting queue state. |
| Android | Kotlin/Compose, Room cache schema 4, separate DataStore profile/enrollment/settings state and Keystore-protected credential envelope. | Preserve profile-to-server/device binding, enrollment recovery, HTTPS endpoint rules, offline queues and durable sync/mutation recovery. Do not reset Room or rebind credentials when a server omits a new capability. |
| iOS | SwiftUI, Keychain credentials, mutation/sync/transfer SQLite schema 9 and separate photo-observation schema 3. File Provider data is another versioned contract. | Preserve Keychain binding, SQLite queue/checkpoints/transfer state, explicit session recovery and existing background-transfer limitations. A capability change must not discard pending operations or change File Provider identity. |

### 3.2 Capability behavior across version skew

| Server | Client | Intended behavior | Current evidence |
|---|---|---|---|
| v0.2 source baseline | Same-baseline web/desktop/Android/iOS clients | Existing contract behavior only; exact all-client interoperability is not claimed. | Source tests and some workflow results exist, but no complete cross-platform matrix. |
| v0.3 server | Last supported v0.2 client for each family | Preserve selected existing /api/v1 flows; new features remain hidden. This is the primary backward-compatibility goal. | Unknown until exact artifacts and server are tested. v0.2 release compatibility is not qualified. |
| v0.2 server | v0.3 client for each family | Detect missing optional capabilities and continue only with legacy flows; do not send v0.3-only mutations. | Unknown; must be tested before a client claims support for old servers. |
| v0.3 server | v0.3 client for each family | Use negotiated capabilities, preserve legacy behavior and test each new flow end-to-end. | Future qualification requirement. |
| Server with unsupported future schema/API contract | Any client | Stop unsafe operation, retain local durable state, show recoverable upgrade/configuration guidance. | Proposed; exact user experience and error envelope remain to be tested. |

The matrix is a qualification plan, not a supported-version promise. A release matrix should name exact server source/release, client artifact/version, OS version, authentication type and tested scenario. At minimum each claimed supported pair must exercise session or device authentication; metadata browsing; one upload and download path where implemented; sync/rebaseline; offline queue/restart; conflict recovery; backup operation support where that client exposes it; and unsupported-capability behavior.

### 3.3 Minimum client and unsupported behavior

No current minimum-client policy is published. Do not declare all v0.2 clients supported merely because they use /api/v1. Before release, maintainers should select a support window for Web, Desktop, Android and iOS independently and record which exact artifacts are in scope.

When a client is too old for an optional capability, the server returns its existing v1 behavior and the client disables that capability. When a security or integrity change truly requires a block, the server returns a stable machine-readable error and upgrade instructions; clients must keep local queued state and must not retry through a less-protected operation. The block threshold and rollback strategy require maintainer approval.

## 4. Persistent-state and migration contract

### 4.1 Reverified schema inventory

The P001 baseline counts match source at d69e167:

| Store | Current schema evidence | Current state |
|---|---|---|
| PostgreSQL | 36 ordered SQLx migrations in migrations/, from 20260822000000_initial_domain.sql through 20260910000000_sync_retention_handoff_proofs.sql. | Canonical server identity and metadata. Migration ledger/checksums are managed by SQLx through synveil-metadata. |
| Desktop SQLite | 8 SQLx migrations in crates/client-sync/migrations; LOCAL_SCHEMA_VERSION is 8. | Profiles, replica/projection, checkpoints, pending operations and conflict/recovery evidence. |
| Android Room | CacheDatabase declares version 4 and explicit migrations 1→2→3→4. | Local logical cache and profile-scoped sync/operation state. DataStore has separate versioned profile/settings records. |
| iOS mutation/sync SQLite | MutationQueueSQLiteStore schema version 9 with forward version transitions and transaction/integrity checks. | Mutation queue, sync evidence, projections and transfer state. |
| iOS photo observation SQLite | PhotoObservationSQLiteSchema version 3. | Separate opt-in photo-observation and backup queue state. |

Evidence paths: [migrations](../../../migrations), [MigrationRunner](../../../crates/metadata/src/migrations.rs), [desktop schema version](../../../crates/client-sync/src/lib.rs), [Android cache database](../../../clients/android/app/src/main/java/com/synveil/android/data/cache/CacheDatabase.kt), [iOS mutation store](../../../clients/ios/Infrastructure/Persistence/MutationQueueSQLiteStore.swift), and [iOS photo schema](../../../clients/ios/Infrastructure/Persistence/PhotoObservationSQLiteSchema.swift).

P002 creates no migrations or schema changes.

### 4.2 Forward-only expand/migrate/contract

All persistent stores follow forward-only evolution:

1. **Before upgrade:** verify a supported source version, stop or quiesce writers as required, make a recoverable PostgreSQL backup, and record ObjectStore backup/integrity evidence separately. Client queues and secret bindings remain in their own stores. A backup claim is not valid until restore has been rehearsed for the declared failure boundary.
2. **Expand:** add nullable or defaulted structures that the old and new application versions can both tolerate. Preserve old columns/records and API response meaning. Do not rewrite old migration files or rename/renumber existing versions.
3. **Migrate:** run bounded, observable backfills with durable progress if work cannot fit in a single transaction. Make retries deterministic and idempotent; validate row counts, foreign keys, ownership, checksums and content references.
4. **Dual-compatible window:** if code must read/write old and new representations, define one canonical write owner, consistency checks and removal criteria. Do not allow two independent writers to make divergent authority.
5. **Contract:** remove an old column or behavior only in a later approved release after the minimum supported server/client window excludes all readers and the recovery window is closed.
6. **Failure:** preserve the SQLx ledger, local schema version, journal and durable queues. Surface a stable failure and stop affected operations. Never drop/reset a schema, erase a queue, or infer missing identity from a library name.

The existing PostgreSQL migration set is immutable after release and SQLx checks checksums. New SQLx migrations must preserve the current forward-only history and migration-lock behavior. Each migration must be transactional or include explicit durable resume/reconciliation semantics for any effect that cannot be transactional.

### 4.3 Local client state and credentials

Desktop, Android and iOS migration code must reject unknown future schema versions and preserve profile/server/owner/device/library binding. Keep pending operation IDs, original request payload or equivalent replay evidence, base epoch/sequence, acknowledgement state, conflicts, rebaseline stage and transfer recovery records. Migrations must never silently drop or regenerate that state.

Credential bytes remain in the platform secure store: OS credential service, Android Keystore-protected storage or iOS Keychain. SQLite/DataStore may hold references, key identifiers or encrypted envelopes already in use, but new migrations must not copy plaintext bearer tokens into general profile files. A missing, revoked or mismatched credential pauses queued network work and requires explicit recovery; it must not be rebound to a new endpoint or owner.

### 4.4 Rollback limits

No general database downgrade is promised. If an application release must be rolled back after a forward migration, it is safe only while the old application can read the expanded schema. After contract changes, recovery may require restoring a verified PostgreSQL backup and its matching ObjectStore state. That restore is operator-driven unless a future concrete recovery protocol is approved and rehearsed.

PostgreSQL and ObjectStore do not participate in one atomic transaction. A database backup without the referenced object bytes, or object bytes without canonical metadata, is not a complete application restore. The application logical backup manifest and content pins protect captured references but are not a full host/database recovery system.

## 5. Storage and synchronization invariants

| Invariant | Contract to preserve |
|---|---|
| Canonical metadata | PostgreSQL remains authority for users, devices, owner/resource scopes, libraries, nodes, file versions, sync journals/checkpoints, conflicts, backup manifests and durable operation outcomes. |
| Canonical content bytes | ObjectStore remains byte authority behind server-generated opaque object keys. Never expose physical paths, keys or backend credentials to clients. |
| Version identity | FileVersion identity is immutable. Historical restore appends a new version referencing verified content; it does not rewind or mutate previous history. |
| Authorization | Preserve owner/device/library scope checks in API and repository access. A UUID, cached profile, Tailscale peer or capability token alone is not authority. |
| Sync ordering | Preserve per-library journal epoch and sequence, device/library checkpoints, bounded feed ordering, durable acknowledgements and snapshot/rebaseline handoff. |
| Mutation identity | Preserve stable mutation IDs/fingerprints and original scope. Retries reuse the same identity. A replay with a different body is rejected or reconciled as a conflict. |
| Conflict behavior | Preserve explicit stale-write/conflict detection and explicit resolution. No silent last-write-wins, implicit merge or automatic rebasing is introduced. |
| Backup references | Preserve immutable snapshot manifests and ObjectStore content pins until authorized plan execution releases them. Prune remains explicit and auditable. |
| Client recovery | Preserve durable queues/checkpoints/transfer state across restart, migration and expired authentication. Local state survives unsupported capability and network loss. |

Current source evidence includes migrations/README.md; synveil-metadata repositories and migration runner; synveil-object-store key types; synveil-storage upload/download services; API sync, mutation, conflict and version handlers; and each client persistence layer. The P001 compatibility report provides the contract-level inventory and evidence limits.

## 6. Recovery-surface contract

| Operation | What it restores | What it does not promise |
|---|---|---|
| File-version restore | One logical file's verified content as a new immutable version. | Database rollback, snapshot of all files, OS or application installation rollback. |
| Application logical-state snapshot restore | The application-level snapshot manifest and selected logical resources/content references covered by its plan. | PostgreSQL physical recovery or arbitrary system files. |
| PostgreSQL backup/WAL recovery | Database state to a documented recovery point. | Presence or correctness of ObjectStore bytes unless coordinated and verified separately. |
| ObjectStore backup recovery | Stored object bytes and opaque identities. | Matching PostgreSQL references, logical metadata or policy state. |
| Host filesystem snapshot | Selected filesystem and host configuration as captured by the OS/provider. | Cross-system consistency unless the capture protocol quiesces all writers and includes all required volumes. |
| Full host/system rollback | A provider-defined machine image/state boundary. | Synveil application authorization or data consistency without a specific tested restore procedure. |

Before a release advertises a recovery surface, it must state its consistency point, included stores, omitted state, restore ordering, validation, maximum data loss and failure handling. P002 makes no atomic PostgreSQL/ObjectStore, host-wide or full-system rollback claim.

## 7. Compatibility acceptance matrix

Each release candidate should turn the intended version-skew table in section 3 into exact artifact rows. The minimum evidence set is:

| Test group | Required cases | Pass condition |
|---|---|---|
| API v1 contract | Route/method/status/error/auth fixtures for existing browser and device operations; request-size and pagination boundaries. | Existing semantics remain; additive fields and endpoints pass old and new fixtures. |
| Browser | Session restoration, CSRF mutation, logout/revocation, unsupported capability. | No mutation replay during auth recovery; old web flow remains usable. |
| Desktop | UI/client IPC, local schema 8 migration, durable queue/restart, secure credential lookup, server skew. | No queue or identity loss; unsupported capability is bounded and visible. |
| Android | Room 4 migration, DataStore profile, Keystore credential binding, offline sync, transfer recovery, server skew. | No profile/device rebind or durable work loss; HTTPS policy remains enforced. |
| iOS | SQLite 9 and photo schema 3 migrations, Keychain binding, rebaseline/ack/transfer recovery, File Provider identity, server skew. | No credential or queued state loss; background limitations remain explicit. |
| Storage and sync | PostgreSQL migration, opaque object key, upload retry, version restore, feed ordering, mutation replay/conflict, snapshot/pin behavior. | All canonical owners and immutable identities remain stable; ambiguous outcomes reconcile safely. |
| Upgrade/recovery | Supported source DB/client schema, failed migration, process interruption, DB restore, ObjectStore restore and declared combined procedure. | Every claimed recovery boundary is rehearsed; unsupported downgrade or mixed backup is rejected with actionable guidance. |

The v0.2 release itself is not a qualified compatibility floor. P055–P060 must record exact source/artifact hashes and platform/OS versions before a combination is described as supported.
