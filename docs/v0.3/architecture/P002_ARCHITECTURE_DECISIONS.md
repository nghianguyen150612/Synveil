# Synveil v0.3 — P002 architecture decision register

**Status:** every entry is PROPOSED. These are recommendations for maintainer approval, not accepted product decisions.
**Baseline:** develop/v0.3 at d69e167913feec5ebed548b60eb07cbdf40ba6e9.
**Related design:** [P002 system architecture](P002_SYSTEM_ARCHITECTURE.md) and [P002 compatibility contracts](P002_COMPATIBILITY_CONTRACTS.md).

## Decision summary

| ID | Topic | Recommended approach | Status | Future owner |
|---|---|---|---|---|
| ADR-P002-001 | Existing crates vs new crates | Extend current owners; add a crate/process only for a demonstrated independent lifecycle or platform boundary. | PROPOSED | P003–P060 by feature |
| ADR-P002-002 | Installer process and privilege | Keep the wizard unprivileged and plan-driven; use the existing engine and narrow native adapters/helper. | PROPOSED | P007–P012, P049–P054 |
| ADR-P002-003 | Tailscale integration | Optional adapter through existing network/host boundaries; HTTPS and application auth remain unchanged. | PROPOSED | P013–P018 |
| ADR-P002-004 | API capability negotiation | Preserve /api/v1 and add optional capability discovery; do not infer API v2 from product version. | PROPOSED | P003–P006, P055–P060 |
| ADR-P002-005 | RBAC compatibility | Add policy checks while preserving current owner/device authorization for existing records. | PROPOSED | P025–P030 |
| ADR-P002-006 | Approval persistence and audit | Persist exact scoped request/decision/outcome in PostgreSQL; make approval separate from execution and privilege. | PROPOSED | P003–P006, P025–P030, P049–P054 |
| ADR-P002-007 | Protected Areas enforcement | Start with application-managed mutation gates; require separate platform design for OS enforcement. | PROPOSED | P031–P036 |
| ADR-P002-008 | Snapshot and rollback boundaries | Treat file version, application snapshot, database recovery, ObjectStore and host rollback separately. | PROPOSED | P037–P042 |
| ADR-P002-009 | CLI transport and mount | Use HTTPS API v1 with device credentials; keep logical file interface separate from OS mount adapters. | PROPOSED | P043–P048 |
| ADR-P002-010 | DeviceVeil integration | Optional authenticated extension boundary; single-host Synveil remains authoritative and independent. | PROPOSED | DeviceVeil owner plus P003 review |
| ADR-P002-011 | Installer presentation ownership | Reuse the desktop/platform installer surface for presentation; keep plan/effects/recovery in install-engine. | PROPOSED | P007–P012 |

All entries need an explicit maintainer decision before implementation changes a public contract, adds persistence, increases privileges, or declares a supported platform guarantee.

## ADR-P002-001 — Prefer existing crate ownership

**Problem and context.** The workspace already has 18 crates with boundaries for auth, API, metadata, storage, network/host, installation, platform support and clients. v0.3 work spans those same responsibilities.

**Existing constraints.** Root Cargo.toml names stable ownership boundaries. P001 found no existing RBAC, approval, Protected Areas, Tailscale, CLI mount or DeviceVeil feature. Synveil is currently one server application with internal workers, not a microservice fleet.

**Alternatives considered.**

1. Add a crate for every feature name.
2. Put all future behavior in synveil-api or synveil-core.
3. Extend the owning crates and add a new crate only when an independent protocol, lifecycle, build target or privilege boundary requires it.

**Recommended approach.** Option 3. Keep domain values and shared errors in synveil-core, authentication and policy ports in synveil-auth, HTTP orchestration in synveil-api, durable authority in synveil-metadata, content operations in storage/object-store, and OS work in platform/host adapters. A CLI may initially reuse the client boundary; a separate package must show independent API or release needs.

**Rationale.** This adds the least new runtime and build complexity while preserving current dependency direction. A crate should mark a real ownership or dependency boundary, not just a product label.

**Security impact.** Shared crate boundaries must not bypass API authorization or give a UI crate direct access to DB, ObjectStore keys or secrets.

**Compatibility impact.** Existing HTTP, local IPC, client database and ObjectStore contracts remain owned by current modules.

**Migration impact.** No data migration follows from this decision alone. New persistent responsibility must still receive a reviewed forward-only migration.

**Future implementation owner.** Each feature prompt; P003 reviews dependency direction.

**Status.** PROPOSED.

## ADR-P002-002 — Keep installer presentation outside the privileged effect boundary

**Problem and context.** The Installer Wizard needs preflight, approvals, progress and recovery while Linux packages and Windows per-user installation have different OS semantics.

**Existing constraints.** synveil-install-engine already models plans, ownership, effect authority, privilege, verification, append-only journaling and recovery. Linux elevation can be delegated to native package management; Windows Setup currently targets the current user. synveil-host-helper is an existing Linux-only purpose-specific root helper.

**Alternatives considered.**

1. Run the entire wizard as root/administrator.
2. Add a persistent privileged service for all platforms.
3. Keep presentation unprivileged and send confirmed typed effects through existing package/platform/host adapters; add a helper only for a concrete missing privileged operation.

**Recommended approach.** Option 3. The engine owns plan and journal; UI owns choices and confirmation; adapters own mutations. Every elevated effect has an operation ID, target scope, privilege class, verification and recovery rule.

**Rationale.** It follows current transaction and privilege boundaries without creating a general-purpose root process or turning account bootstrap into machine installation.

**Security impact.** The UI is untrusted input to adapters. Revalidate the plan and effect at the privileged boundary. Native privilege is not implied by user login or a valid network session.

**Compatibility impact.** Preserve Linux native package operations, AppImage user scope, Windows per-user Setup, server first-admin bootstrap and mobile device enrollment as separate flows.

**Migration impact.** Existing install journal formats remain versioned and immutable. A new journal schema requires explicit compatibility and interrupted-upgrade handling.

**Future implementation owner.** P007–P012; P049–P054 reviews the privileged operations.

**Status.** PROPOSED.

## ADR-P002-003 — Model Tailscale as an optional transport adapter

**Problem and context.** SynveilNet needs local-only, LAN, remote Tailscale and operator-managed HTTPS connectivity without replacing a VPN.

**Existing constraints.** synveil-server-network already separates interface classification and reachability planning. Tailscale interfaces are classified as tunnels and excluded from private-LAN candidates. No Tailscale integration exists.

**Alternatives considered.**

1. Treat every discovered tunnel as a LAN interface.
2. Build a Synveil VPN or public relay service.
3. Add a separate optional Tailscale adapter, retain the current LAN path, and keep the same HTTPS API and application authorization.

**Recommended approach.** Option 3. Network configuration owns selected mode and validated endpoint. A platform adapter performs only explicitly approved Tailscale enrollment/status operations. It does not open public ports or issue Synveil permissions.

**Rationale.** It reuses the current reachability model and keeps transport identity separate from user/resource identity.

**Security impact.** Verify server identity over TLS; treat peer names, QR endpoint claims and local interface presence as untrusted. Fail unavailable on tunnel loss; do not downgrade transport or broaden reachability.

**Compatibility impact.** Preserve local/LAN and operator HTTPS behavior. Existing discovery continues to exclude tunnels unless the new mode is explicitly selected.

**Migration impact.** Persisted connectivity mode or node identity may require a schema migration; add only after exact endpoint ownership and recovery rules are approved. Existing config must default to its current mode.

**Future implementation owner.** P013–P018, with P003 security review.

**Status.** PROPOSED.

## ADR-P002-004 — Preserve API v1 and negotiate optional capabilities

**Problem and context.** New v0.3 operations need safe use by mixed client/server versions.

**Existing constraints.** Product routes are mounted at /api/v1. Existing method semantics, auth classes, status/errors and owner/device checks are contracts. OpenAPI info.version differs from product source version.

**Alternatives considered.**

1. Introduce API v2 for all v0.3 changes.
2. Add optional endpoints/fields and expose stable capability identifiers.
3. Silently change v1 response and authorization behavior based on product release.

**Recommended approach.** Option 2. Keep v1 meanings stable. Add a read-only capabilities surface only after naming the capability version contract. Unknown optional response fields are ignored; old clients can continue without new capabilities. A hard minimum-client block requires a separate security/integrity decision.

**Rationale.** Product version, OpenAPI description version and wire API version identify different things. Additive evolution is compatible with the observed route boundary.

**Security impact.** Capability advertisement does not authorize an operation; every request still authenticates and authorizes. Do not offer insecure fallback when a feature is absent.

**Compatibility impact.** Preserve existing API v1 paths, methods, errors and cookie/device-auth behavior. OpenAPI info.version should identify the API description revision independently, not be copied blindly from Cargo.toml.

**Migration impact.** No migration follows from discovery alone. Capability or client-minimum policy persisted on server requires explicit versioned schema.

**Future implementation owner.** P003–P006 for design; P055–P060 for matrix and release policy.

**Status.** PROPOSED.

## ADR-P002-005 — Extend authorization without changing owner semantics

**Problem and context.** Future multi-user, roles and scoped permissions must coexist with v0.2 owner/device scope checks.

**Existing constraints.** Current identities are browser session or device credential; handlers/repositories scope by owner, library and device. No general role model exists.

**Alternatives considered.**

1. Replace owner scope with a new role system in one migration.
2. Infer a role from client profile, network location or resource identifier.
3. Add an authorization policy layer that maps existing owner/device requests to equivalent legacy permissions, then incrementally adds explicit roles/scopes.

**Recommended approach.** Option 3. Resolve a typed principal first; authorize action and resource before persistence. Preserve existing owner rights exactly during the compatibility window and do not grant a new owner or role implicitly.

**Rationale.** It preserves current resource ownership and makes new policy testable without changing existing data meaning.

**Security impact.** Deny by default when a principal/resource mapping is missing. Recheck authorization after credential revocation and before each protected operation.

**Compatibility impact.** Existing v0.2 browser and device clients keep their current authorized operations. Client labels, UUID possession and Tailscale membership remain non-authoritative.

**Migration impact.** Role and scoped-grant rows require additive migrations and explicit backfill/mapping review. Do not rewrite user, device, owner or library identifiers.

**Future implementation owner.** P025–P030, using P003 policy decisions.

**Status.** PROPOSED.

## ADR-P002-006 — Persist approval separately from execution and OS privilege

**Problem and context.** Protected changes and privileged actions may require approval and later audit.

**Existing constraints.** There is no generic approval queue or persistent security audit model. Existing DB operations may be plan-first and durable; the Linux host helper already uses a bounded operation-specific protocol.

**Alternatives considered.**

1. Treat a logged-in session as approval.
2. Keep approval only in UI memory.
3. Persist an expiring request bound to exact resource, action, requester and policy revision; record approver/decision/outcome; reauthorize and elevate separately when executing.

**Recommended approach.** Option 3. Store server-authoritative approval and audit records through synveil-metadata. Approval never becomes a bearer credential or general privilege token.

**Rationale.** Persistent, replay-safe records survive restart and distinguish intent, decision and effect.

**Security impact.** Validate expiry, revocation, policy version, operation fingerprint and approver authority at execution. Privilege requires native OS authorization in addition to application approval.

**Compatibility impact.** Existing operations without an approval requirement retain their current flow until policy explicitly covers them. New protected operations use additive contracts.

**Migration impact.** Requires reviewed tables, state transitions, retention rules, idempotency and audit access policy. Do not start schema work before P003 resolves actor and retention requirements.

**Future implementation owner.** P003–P006 policy; P025–P030 persistence; P031–P036 and P049–P054 integration.

**Status.** PROPOSED.

## ADR-P002-007 — Begin Protected Areas with application-managed enforcement

**Problem and context.** Users want selected files/directories/resources monitored and protected against unauthorized changes on multiple operating systems.

**Existing constraints.** Synveil does not have a Protected Areas policy or universal filesystem interception mechanism. Sync observation is not equivalent to blocking arbitrary writes.

**Alternatives considered.**

1. Claim arbitrary writes can be blocked through application checks alone.
2. Add a kernel module in the architecture phase.
3. Gate Synveil-managed mutations and record integrity baselines; design OS enforcement separately for each platform if required.

**Recommended approach.** Option 3. Use stable logical resource identity, immutable policy revision, expected prior hash/revision, explicit operation identity and audit evidence. Report observed external writes as detection/review events; do not claim they were prevented.

**Rationale.** It is testable against existing API and mutation paths and states the actual coverage.

**Security impact.** Failure to persist required authorization/audit evidence blocks the protected operation. OS enforcement would need independent privilege and bypass analysis.

**Compatibility impact.** Existing file-version and conflict semantics remain. Protected status adds a decision; it does not silently rewrite a version or auto-resolve a conflict.

**Migration impact.** Logical policies, baselines and audit events require forward-only server and possibly client-state migrations; exact schema is deferred.

**Future implementation owner.** P031–P036; OS-enforcement scope requires P049–P054.

**Status.** PROPOSED.

## ADR-P002-008 — Keep recovery products and rollback domains distinct

**Problem and context.** v0.3 may expose snapshots, health and rollback across files, application state, PostgreSQL, ObjectStore and host systems.

**Existing constraints.** Current file-version restore appends history. Application backup snapshots capture logical manifests and pin object content. PostgreSQL and ObjectStore have separate authorities; no atomic cross-store restore protocol was found.

**Alternatives considered.**

1. Present every restore as a single atomic host rollback.
2. Reuse file-version restore to claim database or system recovery.
3. Name and qualify each recovery surface separately; define a concrete consistency protocol before claiming combined restore.

**Recommended approach.** Option 3.

**Rationale.** Each state owner has distinct consistency and failure behavior. This avoids promising an atomicity guarantee that current storage interfaces do not provide.

**Security impact.** Preserve authorization and audit on logical restore; host rollback tools remain operator/provider privileged and must not be triggered by ordinary file permission.

**Compatibility impact.** Immutable file versions, snapshot manifests, content pins and restore/prune plans remain stable.

**Migration impact.** Any snapshot/rollback metadata must preserve referenced objects and version identities. Physical backup tooling and WAL are operator policy, not application migrations.

**Future implementation owner.** P037–P042; P055–P060 qualifies stated recovery claims.

**Status.** PROPOSED.

## ADR-P002-009 — Use API v1 for CLI and isolate mount-specific behavior

**Problem and context.** A future CLI needs remote file operations, token auth, local editing and explicit push; mount semantics differ by operating system.

**Existing constraints.** Existing API v1 includes logical metadata, resumable upload, content download, device auth, sync and conflict operations. Desktop IPC is local and intended only for UI-to-client control. No CLI or mount adapter exists.

**Alternatives considered.**

1. Expose the desktop control pipe as a remote CLI API.
2. Add a second server protocol or distributed file API.
3. Use HTTPS API v1 with a device credential and isolate a logical remote-files layer from OS mount adapters.

**Recommended approach.** Option 3. Reuse API operations where their semantics match. New approval or capability actions need additive endpoints. A CLI may start as a separate binary surface within the existing client workspace boundary; create a new crate only if transport or release ownership proves independent.

**Rationale.** The server API remains the authorization authority and existing object/version/conflict semantics stay intact.

**Security impact.** Use platform secure storage, never print credentials, validate server identity, and require the server to authorize every remote operation. A mounted path is not a privilege grant.

**Compatibility impact.** Stable mutation IDs, expected revisions and explicit conflict resolution are required. File modifications use explicit upload/push and do not require Git commits.

**Migration impact.** CLI config and any mount cache need versioned local state without changing desktop or mobile database meanings. Keep credential material outside general config.

**Future implementation owner.** P043–P048 with P003 credential policy.

**Status.** PROPOSED.

## ADR-P002-010 — Keep DeviceVeil optional and outside Synveil's single-host authority

**Problem and context.** DeviceVeil may add multiple hosts, named clusters, roles/limits, scheduling, invitations, mobile pairing and a unified logical storage view.

**Existing constraints.** Synveil currently has a single-host metadata and ObjectStore authority with owner/device authorization. DeviceVeil integration is not implemented and its separate scope must remain independent.

**Alternatives considered.**

1. Merge cluster membership and scheduling into Synveil storage/auth immediately.
2. Add distributed storage and placement implicitly whenever DeviceVeil is present.
3. Define an optional authenticated adapter with explicit host identity/capabilities; keep the single-host path complete when the adapter is absent.

**Recommended approach.** Option 3. DeviceVeil owns cluster topology, scheduling and host roles. Synveil owns its existing host-local metadata, bytes and user/resource policy unless a later cross-product contract is explicitly approved.

**Rationale.** It preserves current storage identity and authorization semantics and contains failure to the optional integration.

**Security impact.** Mutual host identity, scoped host credentials, replay protection and explicit user/resource authorization are required. Cluster membership alone grants no access.

**Compatibility impact.** Disabled-by-default behavior preserves current single-host configuration and API. No silent change to the existing storage view or owner scopes.

**Migration impact.** Avoid Synveil cluster tables until ownership, failure and data model are approved. Any host enrollment record must be additive and revocable.

**Future implementation owner.** DeviceVeil owner with P003 review; no P002 implementation.

**Status.** PROPOSED.

## ADR-P002-011 — Keep wizard UI and installation effects separately owned

**Problem and context.** A common wizard experience must support different package and platform installation paths.

**Existing constraints.** The current engine is UI-neutral and supplies plans/journal/recovery. Existing desktop presentation is Qt/QML; package installers and Windows Setup have separate deployment surfaces.

**Alternatives considered.**

1. Move package and OS effect logic into the UI.
2. Force all platform installation through one new daemon.
3. Put presentation in existing desktop or platform installer surfaces and preserve typed plan/effect boundaries in synveil-install-engine.

**Recommended approach.** Option 3. Share plan terminology and lifecycle semantics; allow platform presentation shells to differ where OS policy requires. The exact three-step UI grouping is a product decision, not an engine contract.

**Rationale.** Reuses current UI and installer ownership and avoids duplicating recovery logic in each shell.

**Security impact.** UI choices are untrusted; the engine and adapter validate target, privilege, preservation and effect. Any confirmation token is bound to the plan fingerprint and expires.

**Compatibility impact.** Keep account bootstrap, server installation and mobile enrollment separate. Preserve Windows current-user scope and native Linux package-manager flow.

**Migration impact.** No database migration. Any durable wizard resume state belongs to the existing versioned install journal and must be forward-compatible.

**Future implementation owner.** P007–P012.

**Status.** PROPOSED.
