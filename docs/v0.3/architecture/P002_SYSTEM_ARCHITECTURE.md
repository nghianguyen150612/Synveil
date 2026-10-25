# Synveil v0.3 — P002 system architecture

**Design status:** recommendations are proposed for maintainer review. Nothing in this document makes a v0.3 feature shipped or an ADR accepted.
**Source baseline:** develop/v0.3 at d69e167913feec5ebed548b60eb07cbdf40ba6e9.
**Roadmap:** Synveil-v0.3-Context-Roadmap-P001-P060.md was not found in the repository or /workspace. This blueprint follows the supplied P002 scope and P001 evidence; phase-specific constraints not present there remain unknown.

## 1. Purpose and evidence labels

P002 defines boundaries for future work while preserving the current v0.2 source contracts. It does not implement an installer wizard, SynveilNet, QR pairing, RBAC, Protected Areas, system snapshots, a CLI, filesystem mounts, or DeviceVeil integration.

Claims below use these evidence labels:

| Label | Meaning |
|---|---|
| **Existing — source verified** | The named source or contract implements the stated narrow behavior at the baseline SHA. This does not qualify every deployment or client combination. |
| **Existing — evidence limited** | Source exists, but P001 records missing release, platform, or cross-version evidence. |
| **Partial** | Some layers exist while other product behavior or supported-platform evidence is absent. |
| **Missing in inspected source** | P001 and the source search did not find the feature. This is a bounded search result, not a statement about external projects. |
| **Proposed** | A recommended v0.3 boundary. It needs approval through the ADR process before implementation. |

The baseline evidence is summarized in [P001_V02_BASELINE_AUDIT.md](../audits/P001_V02_BASELINE_AUDIT.md) and [P001_COMPATIBILITY_BASELINE.md](../audits/P001_COMPATIBILITY_BASELINE.md). The audited source is the current P001 integration branch, not the older P001 pre-merge SHA quoted inside those historical reports.

## 2. Current architecture

### 2.1 Workspace and module ownership

The root [Cargo.toml](../../../Cargo.toml) declares 18 Rust workspace crates, Rust edition 2024 and product source version 0.2.0. These are the current ownership boundaries; proposed responsibilities below should extend them where they fit.

| Existing component | Current responsibility and state | Source evidence |
|---|---|---|
| synveil-core | Shared identifiers, domain values, errors, sync, backup and lifecycle contracts. Existing. | [crates/core/src](../../../crates/core/src) |
| synveil-auth | Password verification, browser sessions and device credentials. Browser and device principals are distinct. Existing; broader RBAC is missing. | [crates/auth/src](../../../crates/auth/src) |
| synveil-metadata | PostgreSQL pool, SQLx migrations, canonical metadata, identity references, sync, backup and operation records. Existing. | [crates/metadata/src](../../../crates/metadata/src) |
| synveil-object-store and synveil-storage | ObjectStore contract and opaque keys; upload, download, integrity and garbage-collection workflows. Bytes are separate from metadata. Existing. | [crates/object-store/src](../../../crates/object-store/src), [crates/storage/src](../../../crates/storage/src) |
| synveil-api | Axum router, /api/v1 handlers, authentication/CSRF middleware, HTTP errors and server runtime composition. Existing. | [crates/api/src/router.rs](../../../crates/api/src/router.rs), [crates/api/src/auth.rs](../../../crates/api/src/auth.rs) |
| synveil-server-config | Validated managed and operator-supplied server configuration, secret references and reachability choices. Existing. | [crates/server-config/src](../../../crates/server-config/src) |
| synveil-server-network | Side-effect-free local/LAN reachability planning, endpoint and firewall intent. Existing; no Tailscale client integration. | [crates/server-network/src/lib.rs](../../../crates/server-network/src/lib.rs) |
| synveil-server-service | Native Linux service and runtime adapters. Existing. | [crates/server-service/src](../../../crates/server-service/src) |
| synveil-server-bootstrap | First-admin and managed first-run bootstrap boundary. Existing; this is account bootstrap, not machine installation. | [crates/server-bootstrap/src/lib.rs](../../../crates/server-bootstrap/src/lib.rs) |
| synveil-server-storage | Managed server storage identity, capacity and configuration helpers. Existing. | [crates/server-storage/src](../../../crates/server-storage/src) |
| synveil-server-host | Managed Linux host inspection, service/network/account operations and a narrow purpose-specific helper protocol. Existing. | [crates/server-host/src](../../../crates/server-host/src) |
| synveil-client-sync | Durable desktop synchronization domain, SQLite profiles, queues, checkpoints, conflicts, observation and recovery. Existing. | [crates/client-sync/src](../../../crates/client-sync/src), [crates/client-sync/migrations](../../../crates/client-sync/migrations) |
| synveil-client | Foreground desktop client process, sync composition, lifecycle, platform networking and local desktop-control IPC. Existing. | [crates/client/src](../../../crates/client/src) |
| synveil-desktop | Qt/QML UI, bridge, tray and user control surface. It is a separate process from synveil-client. Existing. | [crates/desktop/src/main.rs](../../../crates/desktop/src/main.rs), [crates/desktop/qml/Main.qml](../../../crates/desktop/qml/Main.qml) |
| synveil-platform | OS identity, paths, lifecycle, storage capability and secure-secret abstractions. It does not install or start services by itself. Existing. | [crates/platform/src/lib.rs](../../../crates/platform/src/lib.rs) |
| synveil-install-engine | Platform-neutral plans, effects, lifecycle policy, append-only journal and platform adapters. Existing; no three-step wizard UI. | [crates/install-engine/src](../../../crates/install-engine/src) |
| synveil-ios-ffi | Narrow C ABI for the Swift/Rust bridge. Existing. | [crates/ios-ffi/src/lib.rs](../../../crates/ios-ffi/src/lib.rs) |
| Web client | React, TypeScript and Vite setup/login and backup-management surfaces. It uses HTTP; it does not access server storage directly. Existing, partial. | [apps/web/src](../../../apps/web/src) |
| Android client | Kotlin and Jetpack Compose app with profiles, enrollment, device auth, cache, sync, transfers and WorkManager scheduling. Existing, release compatibility unqualified. | [clients/android/app/src/main/java/com/synveil/android](../../../clients/android/app/src/main/java/com/synveil/android) |
| iOS client | SwiftUI app with Keychain credentials, SQLite queues/projections, transfer and sync layers, File Provider foundation and opt-in photo flow. Existing, release compatibility unqualified. | [clients/ios](../../../clients/ios) |

services/ai is outside the Rust workspace and is not part of the core runtime path described here. No production dependency on it is asserted.

### 2.2 Processes, build inputs and network entry points

| Process or entry point | Responsibility, dependencies and boundary |
|---|---|
| synveil-api | Axum server. In managed mode it binds 127.0.0.1:3000 behind the managed HTTPS edge; it validates runtime configuration, connects to PostgreSQL, runs the migration runner, and composes managed local ObjectStore upload/download services before binding. In operator mode the configured bind address remains operator-owned. Source: [synveil-api.rs](../../../crates/api/src/bin/synveil-api.rs). |
| Managed HTTPS edge | Client-facing TLS entry on port 443; proxies to the loopback API backend. Operator-managed HTTPS remains a separate supported configuration. Deployment units and manifests live under [deploy/server](../../../deploy/server) and [deploy/systemd](../../../deploy/systemd). |
| synveil-worker | Internal, non-listening bounded object-GC worker using PostgreSQL and ObjectStore. It records worker health separately in managed mode. Source: [synveil-worker.rs](../../../crates/api/src/bin/synveil-worker.rs). |
| synveil-server-migrate | Explicit managed PostgreSQL migration command. The API also runs the same migration runner during startup. Source: [synveil-server-migrate.rs](../../../crates/api/src/bin/synveil-server-migrate.rs). |
| synveil-scheduled-maintenance-once | Internal one-shot scheduled backup-maintenance executable; it is not a daemon or public API. Source: [synveil-scheduled-maintenance-once.rs](../../../crates/api/src/bin/synveil-scheduled-maintenance-once.rs). |
| synveil-host-helper | Linux root helper with purpose-specific operations and bounded framed IPC; it requires the Polkit actor for its elevated session. It is not a general arbitrary-command helper. Source: [helper.rs](../../../crates/server-host/src/bin/helper.rs), [protocol.rs](../../../crates/server-host/src/protocol.rs). |
| synveil-client and synveil-desktop | Separate user processes. The UI talks to the client over machine-local Unix-socket or Windows named-pipe IPC. The client owns sync composition and secure credential retrieval; the UI does not open its SQLite store. Sources: [control.rs](../../../crates/client/src/control.rs), [lib.rs](../../../crates/client/src/lib.rs). |
| Web, Android and iOS | Separate client processes/apps using HTTP API contracts. Android stores cache in Room and credentials through its Keystore-backed vault; iOS uses independent SQLite state and Keychain. |
| Installer and package executables | Linux package manager operations may use native elevation; AppImage and Windows per-user installation are distinct scopes. Current installation artifacts and configuration boundaries are under [deploy/install](../../../deploy/install), [deploy/packages](../../../deploy/packages), and [deploy/windows/installer](../../../deploy/windows/installer). |

The Rust build uses the workspace dependency set, Tokio, Axum and SQLx at the server boundary, and CXX-Qt/Qt for the desktop target. Web, Android and iOS have independent Vite/npm, Gradle and Xcode build systems. External runtime dependencies include PostgreSQL, the selected object-byte backend, the HTTPS edge or operator proxy, and the platform service/package managers. The P001 baseline documents evidence and release limits for these dependencies; a build alone does not establish a supported runtime combination.

### 2.3 Ownership, trust and recovery boundaries

| State or contract | Canonical owner and boundary |
|---|---|
| User, device, library, node, immutable file-version metadata, sync journal/checkpoints, backup manifests/plans and durable operation state | PostgreSQL through synveil-metadata. API authorization is checked before owner/device-scoped repository operations. |
| File content bytes | ObjectStore through synveil-object-store/synveil-storage; server-generated opaque keys and integrity metadata remain server-side. |
| Desktop profiles, replica, sync queue/checkpoints and conflicts | Desktop SQLite owned by synveil-client-sync; current local schema version is 8. |
| Android cached library/node state, sync queues and profile settings | Room schema 4 plus separate DataStore records; device key material is Keystore-backed. |
| iOS local mutation, sync, projection and transfer state | SQLite schema family 9; separate photo-observation SQLite schema 3. Bearers remain in Keychain. |
| Browser session | Session record and credential digest in PostgreSQL; browser carries a host-only session cookie and CSRF evidence. |
| Installation progress and recovery | Append-only installation journal owned by synveil-install-engine; it is separate from application configuration, credentials, client state, PostgreSQL and object bytes. |

The checked-in contract is [api/openapi.yaml](../../../api/openapi.yaml); the actual route prefix is defined by API_VERSION_PREFIX in [router.rs](../../../crates/api/src/router.rs). The OpenAPI info.version currently says 0.1.0-blueprint while the workspace source version is 0.2.0. That is a documentation-version discrepancy, not evidence of a different wire route version.

## 3. Proposed v0.3 logical architecture

The proposal keeps one application API/server and reuses existing crates. New persistent data should use PostgreSQL only where server-authoritative state is required; client queues stay in each existing client store. A new network service, replacement VPN, generalized privileged daemon or distributed storage layer is not justified by the supplied requirements.

| New responsibility | Recommended owner | Boundary and rationale |
|---|---|---|
| Installer Wizard presentation and progress | Existing synveil-desktop UI or existing platform installer shell; typed plan/event interfaces from synveil-install-engine | Keep presentation separate from effects and journal. No new installer service. Maintainer approval is needed for the final cross-platform presentation split. |
| Installation preflight, plan, durable progress and recovery | Extend synveil-install-engine; keep native mutations in existing platform/package/host adapters | The engine already models intent, ownership, effect authority, privilege, verification, journal and reconciliation. |
| Connectivity settings and reachability | Extend synveil-server-config, synveil-server-network, synveil-server-host and synveil-platform | Preserve current local/LAN/operator modes; add optional Tailscale integration as an adapter only after maintainer approval. |
| Device pairing | Extend synveil-auth, synveil-api and synveil-metadata around existing one-time device grants | QR is a short-lived invitation presentation; existing device-bound credential exchange remains the authority. |
| User roles, permissions, approval and audit | Add policy ports in synveil-auth/synveil-api; persist server-authoritative role, approval and security-audit records via synveil-metadata | Preserve existing owner/device decisions as a compatibility policy. Keep approval and OS privilege checks separate. |
| Protected Areas | Extend API authorization and metadata policy for app-managed resources; let client-sync surface observed changes | Application logic can protect only operations passing through it. OS-level write prevention requires an explicit platform enforcement mechanism. |
| Snapshot and rollback status | Extend metadata/storage and operator recovery documentation only for the chosen recovery surface | File versions, logical snapshots, PostgreSQL backup, object backup and host snapshots remain distinct. |
| CLI | Initially add commands to the existing client code boundary with a dedicated executable surface only if approved; use HTTPS API and device credentials | Do not expose or repurpose desktop UI IPC as a remote API. Keep mount implementations behind platform-specific adapters. |
| DeviceVeil | Optional adapter boundary outside the single-host storage/auth authority | No cluster scheduler or distributed storage enters Synveil by implication. Disabled unless explicitly configured and negotiated. |

### 3.1 Installer Wizard

**Existing:** synveil-install-engine defines installation requests, resource ownership classes, preconditions, privilege requirements, effects, verification, recovery disposition and a durable append-only journal. Linux package/AppImage adapters and Windows per-user Setup have separate platform semantics. The Web SetupPage and /api/v1/bootstrap/admin create the first account; they do not install a machine. Android/iOS enrollment is also separate.

**Proposed boundary:**

1. A wizard gathers intent and non-secret choices, requests preflight from the engine, and displays redacted findings and the resulting plan.
2. Before a plan executes, the UI presents each effect's target, resource class, authority, privilege and reversibility for explicit user confirmation. The UI must not elevate privileges itself.
3. The engine validates the plan fingerprint and preservation snapshot, appends journal records before/after effects, and sends a typed progress stream. The event stream contains IDs, stage, progress, safe error category and recovery action; it does not carry passwords, enrollment tokens or raw secret values.
4. Existing native package managers, Windows per-user Setup, synveil-platform, and synveil-server-host adapters perform only their owned effects. The adapter re-inspects state before a confirmed mutation.
5. On interruption or ambiguous outcome, the engine uses the journal and adapter inspection to classify resume, reconcile, replan or require manual inspection. Unknown ownership or result fails closed; it is not retried as if no effect occurred.
6. Server service lifecycle and first-admin bootstrap occur after machine installation through their own boundaries. A future user account approval must not be treated as approval to modify the operating system.

No new process is required for the wizard proposal. Native package elevation remains with the package manager; the existing Linux helper remains operation-scoped. The Windows target remains per-user unless a later approved requirement changes it.

### 3.2 SynveilNet and Tailscale

**Existing:** synveil-server-config and synveil-server-network model local-only and private-LAN reachability; managed backend is loopback and HTTPS terminates at the edge. The Linux network inspector explicitly classifies interface names beginning with tailscale as Tunnel and excludes tunnels from ordinary private-LAN candidates. No Tailscale client or control-plane integration was found in source.

**Proposed boundary:**

- Persist the selected connectivity mode and endpoint identity in managed server configuration. Network inspection reports typed interface and endpoint evidence; UI policy decides what to show.
- Keep four modes distinct: local-only; private LAN; optional Tailscale-backed remote path; operator-managed HTTPS. Tailscale is a transport path and is not an application identity provider or authorization decision.
- Show an explicit user-visible state and source, such as local-only, LAN endpoint available, remote tunnel connected, tunnel unavailable/expired, or operator endpoint reachable. Do not infer a connected or authorized state only from interface presence.
- A Tailscale adapter may enroll or inspect the local node only through an explicit user-approved flow. It must not start public listeners, open router ports, or silently change the managed network mode.
- Advertised endpoints are bound to server installation identity and verified TLS identity. Clients pin neither an unverified QR URL nor a claimed Tailscale peer name as sufficient application identity.
- A client reaches the same authenticated application API over HTTPS regardless of transport. Authorization stays server-side and resource-scoped.
- If a tunnel expires or is unavailable, report that state and keep the connection unavailable until the user selects another approved endpoint. Never silently downgrade to plaintext or expand from local-only to LAN/public reachability.
- LAN discovery must continue excluding tunnel interfaces. Explicit Tailscale discovery is a separate source with separate trust and endpoint validation.

No replacement VPN protocol or public port exposure is proposed.

### 3.3 Pairing and device enrollment

**Existing:** one-time device enrollment grants and distinct device-bound credentials exist in synveil-auth, API device-auth routes, and migration 20260828000000_device_credentials_enrollment.sql. Browser sessions and device credentials have different identity and CSRF semantics.

**Proposed flow:** the host owner or authorized operator initiates a pairing invitation; the server authenticates that initiator and creates a short-lived, owner/server-bound, single-use invitation. A QR encodes only the invitation exchange reference and high-entropy one-time secret, never a browser session or reusable bearer. The mobile app scans and submits the exchange over the validated HTTPS endpoint. The server rechecks expiry, nonce use, intended owner/server, account policy and device registration transactionally; it consumes the invitation once and issues a separate device credential. The device stores its server, owner, device and credential binding in its OS-protected vault.

Expired or ambiguous exchanges require a fresh invitation; they do not reuse an old token. Device revocation remains server-authoritative. Re-enrollment creates or explicitly rebinds device identity through an approved recovery flow; a profile label or QR scan alone cannot rebind an existing credential. A client that loses local protected credential state must recover by a new enrollment. Pairing should remain usable on older clients through existing device-grant exchange until a capability is negotiated.

### 3.4 Identity, RBAC, approval and privilege

**Existing:** authentication distinguishes browser sessions and device credentials; API handlers apply owner, library and device scopes. There is no general user/role/approval model in the inspected source.

**Proposed layers:**

1. **Authentication:** synveil-auth verifies credentials and produces a principal type without assigning resource rights.
2. **Authorization:** an API policy check evaluates principal, action, owner, device, host and logical resource before repository calls. A compatibility policy must preserve the current owner/device result for existing v0.2 records and clients.
3. **Approval:** a durable approval request records requested operation, scope, expiry, requester, decision and approver. Approval is a distinct resource with replay-safe state transitions; it does not execute the operation by itself.
4. **Privilege elevation:** an OS adapter grants only the specific protected operation under native OS authorization. A valid network tunnel, login or application access token never establishes root/admin authority.
5. **Audit:** immutable or append-only security records correlate principal, authorization result, approval and privileged-operation outcome without storing bearer secrets or content bytes.

New tokens and invitations need explicit expiry, revocation and audience/resource binding. Role assignment semantics, multi-owner resource transitions, expiry rules for existing device credentials and the exact durable approval model are open ADRs. Detailed passkey/FIDO2 and privileged-authentication design belongs to the later security phase.

### 3.5 Protected Areas

Define a protected resource by stable logical identity and owner scope, then bind its current policy and integrity baseline to an immutable policy revision. Application-managed protection can require explicit authorization/approval for mutations that pass through Synveil API or sync mutation contracts. It can record a permitted change, expected prior revision/hash, new version and actor in the same logical operation or durable operation state.

Change detection must distinguish an expected authorized mutation from a remote or local observation. Ambiguous detection is surfaced for review and does not rewrite the baseline. Failure to persist authorization or audit evidence blocks protected mutation.

Application-managed checks cannot block arbitrary filesystem writes that bypass Synveil. Host-OS enforcement requires a separately designed mechanism and platform-specific evidence; it must state which filesystem, privilege, race and bypass cases it covers. P002 adds no kernel module and makes no universal write-blocking claim.

### 3.6 Snapshots and rollback surfaces

Keep these recovery products separate:

| Recovery surface | Current or proposed owner | Boundary |
|---|---|---|
| Historical file version restore | Existing API/version and upload/content services | Creates a new immutable version referencing verified bytes. It does not rewind history. |
| Application logical-state snapshot | Existing backup snapshot manifests and content pins in PostgreSQL plus references to ObjectStore bytes | Captures logical application state. Prune and restore remain explicit plan/execute workflows. |
| PostgreSQL recovery | Operator/database backup and WAL/restore tooling | Database recovery is independent from application file-version restore. No database downgrade is promised. |
| ObjectStore recovery | Operator storage backup/replication and integrity verification | Byte recovery must preserve opaque object identity and referenced content. |
| Host filesystem or full-system rollback | OS/provider snapshot tooling, if selected | Can include OS/configuration/runtime state and has a different blast radius from application data restore. |

P002 makes no atomic cross-store or full-host recovery guarantee. A release must declare its selected boundary and rehearse metadata plus object-byte recovery before claiming restore qualification. Existing content keys, immutable file-version identities, manifests and reference pins remain immutable.

### 3.7 CLI and network mount

Proposed CLI operations use HTTPS API v1 with a device credential: validate/choose host, report health and session state, list logical libraries/files, download content, create an explicit upload, submit protected-operation requests, inspect approval status and revoke a session/device through an authorized flow. Use existing API v1 contracts where their exact semantics suffice. A new approval request, capability discovery, or policy query requires additive endpoints.

The CLI stores credentials through synveil-platform secure storage, binds them to server and device identity, and reports recovery/revocation states. File editing is local editing of a downloaded working copy; an explicit push creates an API mutation with stable mutation identity and expected revision. Existing conflict handling remains authoritative. Ordinary file edits do not create Git commits or imply history rewriting.

A network mount is a logical remote-files interface plus platform-specific filesystem integration. The logical layer owns remote identity, cache, stale state and explicit push policy; FUSE/WinFSP/File Provider or other adapters own OS-specific semantics. A mount must not pretend unsupported OS write-through, locking or offline guarantees exist. Protecting writes in a mount is not a substitute for server-side authorization.

### 3.8 DeviceVeil extension boundary

DeviceVeil remains an optional external host-cluster control plane. Its future owner may manage multiple enrolled hosts, named clusters, per-host roles and resource limits, scheduling priorities, one-time bootstrap invitations, mobile cluster pairing, placement, cluster topology and resource status, and a unified logical storage view. Synveil continues to own each host's canonical metadata, object content and user/resource authorization unless a separately approved contract says otherwise. The unified view is a logical presentation; it does not imply distributed bytes or shared authorization.

The integration boundary should be explicit, authenticated host-to-host or control-plane API contracts with per-host identity, capability negotiation, bounded scope and replay protection. DeviceVeil being connected does not grant resource access. Failure or absence must leave single-host Synveil behavior intact. The feature is disabled by default; no cluster membership, placement, distributed storage or bootstrap behavior is inferred from Synveil v0.2 state.

## 4. Trust-boundary inventory and P003 handoff

The table states intended future responsibilities. Proposed APIs, approvals, audit records and DeviceVeil boundaries are not implemented by the baseline.

| Boundary | Trusted / untrusted parties and authentication | Authorization, data and least privilege | Failure, audit and owner | P003 question |
|---|---|---|---|---|
| Browser → API | Browser and network are untrusted; existing host-only session cookie, CSRF token and provenance checks authenticate requests. | API checks current owner scope; cookies and CSRF evidence are sensitive. Browser cannot select physical object keys. | Reject invalid session/CSRF; sanitize errors. synveil-api/synveil-auth. | Threat model proxy origins, cookie/CSRF assumptions and session fixation/replay. |
| Mobile → API | Mobile/network are untrusted; existing device credential is distinct from browser session and bound to owner/device. | API checks owner, device and library scopes. Enrollment grants and device bearers are sensitive. | Expired/revoked credential fails closed; preserve offline queue for explicit recovery. API/auth and mobile vault. | Model lost device, replayed enrollment QR, cloned backup and credential revocation. |
| Desktop UI → client process | UI and local user session share a host but IPC input is untrusted; existing versioned, bounded local socket/named pipe handshake. | Client exposes bounded commands/status and owns sync store/secret access. IPC must not become a remote or arbitrary filesystem API. | Close malformed/oversized/time-expired request; record sanitized process status. synveil-client control IPC. | Verify OS endpoint ownership, peer validation and local same-user attack surface. |
| API → PostgreSQL | Application process and DB are separate trust zones; DB transport/credentials are deployment-owned. | API/worker get only required DB role and repository methods; identity, metadata and audit state are sensitive. | Timeout/failure blocks affected operation and readiness; preserve migration ledger. synveil-metadata. | Define DB role separation for API, migration, worker and backup operations. |
| API/worker → ObjectStore | API/worker are trusted application processes; storage backend/paths are constrained external state. | Access only through ObjectStore adapter and opaque server keys; content bytes and integrity metadata are sensitive. | Verify length/hash; don't report success on ambiguous storage effects; worker reconciles durable GC state. synveil-storage/object-store. | Threat model path traversal, partial writes, GC races and DB/object restore mismatch. |
| Installer → privileged OS operations | Wizard/UI is untrusted input; adapter and OS package/Polkit boundary perform authority checks. | Confirm a typed, fingerprinted effect with minimal operation/target scope. Config, credential state and user content are protected classes. | Journal before effect, inspect after ambiguity, never retry unknown ownership; synveil-install-engine, platform adapters and existing host helper. | Enumerate privileged effects and actor binding; establish cancellation and rollback safety. |
| Network transport / Tailscale → application | LAN, tunnel peer and endpoint advertisements are untrusted until TLS/server identity is verified. | Network reachability supplies no application authorization. Session/device auth and resource policy remain in API. | Tunnel expiry means unavailable; no silent mode or plaintext fallback. server-network, server-host, client endpoint validation. | Model endpoint substitution, DNS/name claims, tunnel peer identity and fallback. |
| CLI → server | CLI and terminal arguments/local config are untrusted; HTTPS plus device credential. | Server checks every list/read/upload/approval/revoke operation. Credential is host/device bound and never printed. | Stable mutation ID and explicit conflict recovery; synveil-client/API. | Define CLI credential lifecycle, secret exposure and command confirmation. |
| Approval → privileged operation | Requester, approver and execution process are separate authorities. | Approval binds exact operation, resource, expiry and requester; execution rechecks current policy and OS privilege. | Expired, revoked, duplicate or changed request fails closed; append audit outcome. API/auth/metadata and platform adapter. | Separate approver identity, step-up requirements, separation-of-duties and audit retention. |
| DeviceVeil host ↔ host | Each host and control plane is untrusted until mutually authenticated. | Per-host identity, cluster scope and resource permission are distinct; no cross-host object access by default. | Partition isolates optional cluster features; keep local host service usable and audit federation actions. Future adapter only. | Define trust root, compromise containment, replay protection and cluster teardown. |

### P003 security handoff

P003 should turn this inventory into the threat model and make maintainer decisions before any P007–P060 feature uses the proposed privilege, approval, pairing or remote-connectivity boundaries. In particular it should settle:

1. Attacker and asset assumptions for browser, device, local desktop IPC, privileged installer and host-to-host trust.
2. Credential lifetimes, revocation, recovery and server/device binding for existing grants and any QR invitation wrapper.
3. Authorization compatibility mapping from existing owner/device scopes to any proposed roles.
4. Approval object ownership, expiry, requester/approver separation, replay state and audit retention.
5. Exact OS privileged operations and the required native authentication boundary.
6. Whether Tailscale integration is supported, which local control API is trusted, and the no-public-listener/fallback behavior.

No full threat model or privileged-authentication design is claimed complete here.

## 5. Incremental implementation dependency map

This is planning context from the supplied prompt, not a substitute for the missing authoritative roadmap. It identifies prerequisites and evidence categories only.

| Prompt area | Prerequisites and likely modules | Contract, platform, security and migration dependencies | Proposed integration evidence before release |
|---|---|---|---|
| P003–P006 Security foundation | P002 trust inventory; auth, api, metadata, platform, server-host | Approve threat model, owner compatibility, approval/audit and privileged-operation boundaries. No schema migration before ownership is agreed. | Abuse-case tests; principal/policy contract tests; secret redaction; platform privilege-boundary review. |
| P007–P012 Installer Wizard | Existing engine plan/journal/adapters; desktop, install-engine, platform, server-host, deploy scripts | Typed plan/progress/recovery contract; Linux package-manager and Windows per-user behavior; no coupling to first-admin or device enrollment. New persistent journal formats need explicit version checks. | Interrupted-effect/reconcile tests; preflight and preservation tests; Linux native package and Windows user-scope acceptance. |
| P013–P018 SynveilNet | Current local/LAN and operator TLS; network/host/config/platform; P003 threat model | Additive connectivity capabilities/settings, endpoint identity, explicit Tailscale ownership and platform coverage. No auth relaxation or DB migration implied. | Local, LAN, operator HTTPS and optional tunnel matrix; tunnel-expiry and no-fallback tests; no public listener check. |
| P019–P024 Pairing | Existing one-time grant API/auth/migration; P003 trust decisions; mobile vaults | Add short-lived invitation semantics and optional endpoints; preserve device identity and browser/device separation. Persist consumed invitation state if not already covered by grant records. | Expiry/replay/owner/server binding tests; revocation and re-enrollment tests; Android/iOS old-server behavior. |
| P025–P030 Identity/RBAC/tokens | P003 policy model; owner/device compatibility; auth, api, metadata, web/mobile clients | Additive authorization rules and durable token/role records; expand/migrate/contract only after exact mapping. Preserve existing session and enrollment contracts. | Table-driven legacy and new principal/resource matrix; revocation, expiry and owner-isolation DB tests. |
| P031–P036 Protected Areas | Approved policy and approval model; API mutation paths, sync clients and metadata | Distinguish app-managed from OS enforcement; add policy/baseline and operation evidence only after schema ownership and platform limits are approved. | Authorized/unauthorized mutation tests, stale baseline/conflict tests, per-platform bypass statement and audit completeness. |
| P037–P042 Snapshots, health and rollback | Recovery boundary selection; existing backup manifest/pins, health and operator DB/object backup | Keep file-version restore, app snapshot, DB recovery and host rollback separate. Any schema change must preserve references and content pins. | Restore rehearsal for each named surface; DB plus object-byte consistency verification; interruption and health-degradation tests. |
| P043–P048 CLI, mount and protected push | API v1 inventory, device credential policy, P031 approval/policy; client, platform mount adapters | CLI uses HTTPS; new approval endpoints are additive. Mount semantics vary by OS and cannot weaken conflicts or require Git. | CLI auth/redaction and restart tests; explicit upload/conflict/approval tests; OS-specific mount conformance and unmount recovery. |
| P049–P054 Privileged security/hardening | P003 security decisions and inventory of actual OS effects | Bind app approval to operation and native privilege separately; no new root listener or persistent secret without approved design. Schema depends on approved audit model. | Platform-specific privilege tests, replay/expiry denial, process isolation, secret rotation and audit evidence. |
| P055–P060 Compatibility/release qualification | All implemented contracts and migrations; supported client/server policy | Freeze API/persistence versions, client negotiation and rollback limits; qualify each supported platform/server pair. | Explicit client/server matrix, migration from supported data versions, recovery rehearsal, signed artifacts and required CI/review evidence. |

## 6. Architecture acceptance criteria

Future implementation may claim alignment with this blueprint only when:

1. New responsibilities have one owner, a typed module boundary and an explicit state owner.
2. Existing /api/v1, owner/device identity, object-key opacity, immutable version, sync ordering and conflict behavior remain intact.
3. Security-sensitive operations separately authenticate, authorize, obtain any required approval, obtain OS privilege and record an audit result.
4. Every persistence addition has a forward-only migration, unknown-version behavior and recovery plan.
5. Network transport does not grant application rights, and failure does not trigger an unapproved fallback.
6. Cross-store, full-host or cross-platform guarantees are backed by a named protocol and qualifying evidence; otherwise they remain explicitly unsupported or unknown.
7. Tests and release claims state which client, server, platform and recovery combination was actually exercised.

## 7. Architecture diagrams

Legend: solid nodes/edges show current source boundaries; dashed nodes/edges marked PROPOSED show future design only.

### 7.1 Current v0.2 components

~~~mermaid
flowchart LR
  subgraph Clients["Existing clients"]
    Web["React web"]
    Android["Android app"]
    IOS["iOS app"]
    UI["synveil-desktop Qt/QML"]
    Client["synveil-client process"]
    UI <-->|"local socket / named pipe"| Client
  end
  Edge["HTTPS edge / operator proxy"]
  API["synveil-api Axum /api/v1"]
  Worker["synveil-worker internal"]
  Migrate["synveil-server-migrate"]
  Installer["synveil-install-engine"]
  PG[("PostgreSQL canonical metadata")]
  Store[("ObjectStore canonical bytes")]
  Web -->|"HTTPS"| Edge
  Android -->|"HTTPS"| Edge
  IOS -->|"HTTPS"| Edge
  Client -->|"HTTPS"| Edge
  Edge -->|"loopback 127.0.0.1:3000"| API
  API --> PG
  API --> Store
  Worker --> PG
  Worker --> Store
  Migrate --> PG
  Installer --> InstallJournal[("Append-only install journal")]
  Client --> DesktopSQLite[("Desktop SQLite")]
  Android --> Room[("Room + DataStore")]
  IOS --> IOSSQLite[("SQLite 9 + photo DB 3")]
  Client --> Secrets["OS secure credential store"]
  Android --> Keystore["Android Keystore"]
  IOS --> Keychain["iOS Keychain"]
~~~

### 7.2 Proposed v0.3 logical components

~~~mermaid
flowchart LR
  subgraph ExistingClients["Existing client surfaces"]
    Web["Web"]
    Mobile["Android / iOS"]
    Desktop["Desktop UI + client process"]
    CLI["CLI commands — PROPOSED"]
    Mount["OS mount adapters — PROPOSED"]
  end
  Net["server-config / server-network / server-host"]
  TS["Optional Tailscale adapter — PROPOSED"]
  API["synveil-api /api/v1"]
  Auth["synveil-auth: sessions + device credentials"]
  Policy["Authorization policy / RBAC extension — PROPOSED"]
  Approval["Approval + audit records — PROPOSED"]
  PG[("synveil-metadata / PostgreSQL")]
  Storage["synveil-storage + object-store"]
  Bytes[("Opaque object bytes")]
  Wizard["Wizard presentation — PROPOSED"]
  Engine["synveil-install-engine plan / journal"]
  Adapters["platform, package, host adapters"]
  DeviceVeil["Optional DeviceVeil adapter — PROPOSED, disabled"]
  Web --> API
  Mobile --> API
  Desktop --> API
  CLI -. "HTTPS + device credential" .-> API
  Mount -. "logical file interface" .-> CLI
  Net -. "explicit remote mode" .-> TS
  TS -. "transport only" .-> API
  API --> Auth
  API -.-> Policy
  Policy -.-> Approval
  Auth --> PG
  Policy -.-> PG
  Approval -.-> PG
  API --> Storage --> Bytes
  API --> PG
  Wizard -. "typed plan / progress" .-> Engine
  Engine --> Adapters
  DeviceVeil -. "explicit authenticated contract" .-> API
~~~

### 7.3 Trust boundary and identity flow

~~~mermaid
flowchart LR
  Browser["Browser: session cookie + CSRF"]
  Device["Mobile/device: device-bound credential"]
  Tunnel["LAN / Tailscale / operator TLS transport"]
  API["API authentication"]
  Principal["AuthenticatedPrincipal"]
  Owner["Existing owner/device scope"]
  RBAC["RBAC policy — PROPOSED"]
  Approval["Approval decision — PROPOSED"]
  Privileged["OS privilege adapter"]
  Audit["Security audit record — PROPOSED"]
  Browser -->|"untrusted network"| Tunnel
  Device -->|"untrusted network"| Tunnel
  Tunnel -->|"transport does not grant rights"| API
  API --> Principal --> Owner
  Owner -. "compatibility-preserving extension" .-> RBAC
  RBAC -. "only when operation requires" .-> Approval
  Approval -. "recheck exact operation" .-> Privileged
  Principal -.-> Audit
  RBAC -.-> Audit
  Approval -.-> Audit
  Privileged -.-> Audit
~~~

### 7.4 Installer lifecycle and recovery boundaries

~~~mermaid
flowchart LR
  User["User choices / explicit confirmation"]
  Wizard["Wizard UI — PROPOSED"]
  Preflight["Existing engine preflight + plan"]
  Plan["Fingerprint + ownership + privilege contract"]
  Journal[("Existing append-only install journal")]
  Adapter["Native package / per-user / platform adapter"]
  Verify["Re-inspect + verify effect"]
  Done["Complete with typed result"]
  Recovery["Resume / reconcile / replan / inspection"]
  subgraph SeparateFlows["Separate product flows"]
    Bootstrap["First-admin bootstrap through API"]
    Enrollment["Mobile device enrollment through API"]
  end
  User --> Wizard --> Preflight --> Plan
  Plan -->|"confirm"| Journal --> Adapter --> Verify
  Verify -->|"verified"| Journal --> Done
  Adapter -. "process interruption or unknown outcome" .-> Recovery
  Recovery --> Journal
~~~

### 7.5 Connectivity and pairing boundaries

~~~mermaid
flowchart LR
  Mode["Local-only / private LAN / Tailscale / operator HTTPS"]
  Endpoint["Validated HTTPS endpoint + server identity"]
  API["Existing /api/v1"]
  Owner["Host owner authenticates"]
  Invite["Single-use expiring invitation — PROPOSED"]
  QR["QR carries invitation only — PROPOSED"]
  Mobile["Mobile scans and exchanges"]
  Credential["Existing device-bound credential"]
  Authz["Separate server resource authorization"]
  Mode --> Endpoint --> API
  Owner -. "explicit pairing action" .-> Invite
  Invite -.-> QR -.-> Mobile
  Mobile -. "HTTPS exchange" .-> API
  API --> Credential --> Authz
  Mode -. "no application rights" .-> Authz
~~~

### 7.6 Persistent state and storage ownership

~~~mermaid
flowchart LR
  subgraph ClientState["Client-owned durable state"]
    Desk[("Desktop SQLite v8")]
    Room[("Android Room v4 + DataStore")]
    IOS[("iOS SQLite v9")]
    Photo[("iOS photo SQLite v3")]
    Vault["OS key store / Keystore / Keychain"]
  end
  API["API authorization and application services"]
  PG[("PostgreSQL: canonical identity, metadata, journals, plans, manifests")]
  Obj[("ObjectStore: canonical bytes under opaque keys")]
  Install["synveil-install-engine"]
  Journal[("Installer journal: separate lifecycle state")]
  Backup["Operator DB backup / WAL"]
  ObjectBackup["Operator ObjectStore backup"]
  Desk -->|"HTTPS sync"| API
  Room -->|"HTTPS sync"| API
  IOS -->|"HTTPS sync"| API
  Photo -->|"host-app opt-in"| API
  Vault --> Desk
  Vault --> Room
  Vault --> IOS
  API --> PG
  API --> Obj
  PG -. "logical snapshot references / pins" .-> Obj
  PG -. "separate recovery surface" .-> Backup
  Obj -. "separate recovery surface" .-> ObjectBackup
  Install --> Journal
~~~
