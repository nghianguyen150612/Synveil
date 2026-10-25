# Synveil v0.3 — P003 trust boundaries

**Status:** proposed boundary specification; future components are marked explicitly and are not implemented by this document.
**Evidence snapshot:** P002 head f63374f19a81bbd4b9ef6881445a165c07ba0700.
**Related:** [threat model](P003_THREAT_MODEL.md), [security requirements](P003_SECURITY_REQUIREMENTS.md), [P002 architecture](../architecture/P002_SYSTEM_ARCHITECTURE.md).

## 1. Trust zones

| Zone | Contents | Default trust |
|---|---|---|
| Z-01 User-controlled clients | Browser, desktop UI, desktop client process, Android/iOS apps, future CLI/mount. | Untrusted input and local state; authenticated identity is established only by server or OS boundary. |
| Z-02 Network and endpoint discovery | Internet/LAN, DNS, discovery responses, interface metadata, future tailnet and QR transport. | Untrusted transport and identity claims. |
| Z-03 Application API | HTTPS edge, Axum API v1, auth middleware and application services. | Trusted only after principal authentication and server-side action/resource authorization. |
| Z-04 Canonical metadata | PostgreSQL and SQLx migrations. | Canonical for identity, metadata, permissions, sync/backup plans; separate DB roles and network controls remain deployment concerns. |
| Z-05 Canonical content | ObjectStore bytes, staging, opaque keys and integrity metadata. | Canonical for bytes only after staged verification and metadata commit. Not in one transaction with PostgreSQL. |
| Z-06 Host and operating system | Installer engine, package manager, Polkit helper, systemd, local paths, secret stores and TLS keys. | Privileged only through explicit native OS authorization and typed effects. User UI and remote login are not OS authority. |
| Z-07 Client durable state | Desktop SQLite, Android Room/DataStore, iOS SQLite/File Provider/photo state and secure credential stores. | User/device-local state; preserve identity, queues and recovery evidence on failure. |
| Z-08 CI and artifact delivery | Source repository, GitHub Actions, dependency registries, artifacts, signing keys and update metadata. | Untrusted until source, dependency, workflow and publisher verification passes. |
| Z-09 Optional DeviceVeil | Future cluster control plane and participating hosts. | Separate optional trust domain. Membership grants no Synveil account, storage or administrator authority. |

## 2. Current and proposed data flows

Solid arrows show existing flows; dashed arrows show proposals.

~~~mermaid
flowchart LR
  subgraph Z1["Z-01: user clients"]
    Browser["Browser"]
    Mobile["Android / iOS"]
    UI["Desktop UI"]
    Client["Desktop client process"]
    CLI["CLI / mount — PROPOSED"]
    UI <-->|"bounded local IPC"| Client
  end
  Net["Z-02: Internet / LAN / discovery / optional tailnet"]
  Edge["HTTPS edge or operator proxy"]
  API["Z-03: Axum API v1"]
  Auth["Auth: browser session or device credential"]
  Meta[("Z-04: PostgreSQL metadata")]
  Store["Storage service + ObjectStore adapter"]
  Bytes[("Z-05: object bytes and integrity records")]
  Local["Z-07: SQLite / Room / iOS state"]
  Vault["OS Secret Service / Credential Manager / Keystore / Keychain"]
  Host["Z-06: host helper / package manager / OS"]
  Tailscale["Tailscale adapter — PROPOSED"]
  DeviceVeil["Z-09: DeviceVeil — PROPOSED, optional"]
  CI["Z-08: CI / dependencies / artifacts"]
  Browser --> Net
  Mobile --> Net
  Client --> Net
  Net --> Edge --> API
  API --> Auth
  Auth --> Meta
  API --> Meta
  API --> Store --> Bytes
  Client --> Local
  Mobile --> Local
  Client --> Vault
  Mobile --> Vault
  UI -. "typed plan / explicit confirmation" .-> Host
  CLI -. "typed HTTPS API calls" .-> API
  Net -. "transport only; no authorization" .-> Tailscale
  Tailscale -. "verified TLS to same API" .-> API
  DeviceVeil -. "separate authenticated host contract" .-> Host
  CI -. "verified publisher + artifact chain" .-> Host
~~~

## 3. Identity flows

### Browser session

1. The browser submits login credentials over the configured server endpoint. Password verification and session creation belong to synveil-auth; the API emits a fresh session cookie and a session-bound CSRF cookie.
2. PostgreSQL stores session identity, token digest, creation/expiry and revocation state. The raw session bearer is not the durable session identifier.
3. Browser cookie authentication creates a browser principal. Authenticated mutations require the CSRF header/cookie pair and provenance checks. Logout revokes the current record.
4. The current session read endpoint is not a token renewal endpoint. Web recovery may refresh current session state and CSRF evidence; it must not replay a mutation. Server authorization remains authoritative.

### Device credential and future QR

1. Today, an authenticated browser owner creates a short-lived one-time enrollment grant. The grant secret is returned once; the server stores its digest and owner/device binding.
2. A client exchanges the grant once for a device credential; the raw credential is handed to the client once, while the server stores a digest. Device authentication produces a distinct owner/device/credential principal, not a browser session.
3. The owner may revoke one credential, all credentials for a device, or the device. Current device credential lifetime/rotation is not established.
4. QR pairing is proposed only. A QR may carry a short-lived invitation reference/secret needed for exchange; it is not proof of user consent or server identity. The client must verify the endpoint, display owner/device target, and require explicit confirmation before redemption.

~~~mermaid
sequenceDiagram
  participant Owner as Owner browser
  participant API as Synveil API
  participant DB as PostgreSQL
  participant Client as Device client
  participant OS as Secure OS store
  Owner->>API: Create grant with browser session + CSRF
  API->>DB: Persist grant digest, owner/device, expiry
  API-->>Owner: One-time invitation material
  Owner-->>Client: Show/scan QR invitation — PROPOSED
  Client->>Client: Verify canonical HTTPS server identity + confirm target
  Client->>API: Redeem one-use invitation over verified TLS
  API->>DB: Atomically consume invitation; persist credential digest
  API-->>Client: Device bearer once
  Client->>OS: Store scoped credential in secure OS store
  Client->>API: Later device request with bearer
  API->>DB: Authenticate current owner/device/revocation state
  API-->>Client: Allow only route-authorized resource scope
~~~

## 4. Authorization and approval flow

Authentication establishes a principal; each route must then authorize the requested action against the exact current resource, owner, device and host scope. Current owner/device checks remain the compatibility baseline. P002 roles, approvals, tokens and policy revisions remain proposed.

Future approval is a durable decision record, never a reusable bearer credential. It binds requester and approver, exact operation and parameters, exact resource and target host, requested privilege, policy revision, expiry and replay-resistant identity. Execution checks current authorization and exact scope again, then records success, failure or ambiguous result. Approval does not grant OS privilege.

~~~mermaid
flowchart LR
  Principal["Authenticated browser/device principal"]
  Policy["Server-side owner/device/resource policy"]
  Request["Exact operation request + operation fingerprint"]
  Approval["Approval decision — PROPOSED"]
  Recheck["Execution-time authorization + policy revision check"]
  Native["Native OS authorization — separate"]
  Effect["Typed privileged effect"]
  Audit["Integrity-protected outcome record — PROPOSED"]
  Principal --> Policy --> Request --> Approval --> Recheck
  Recheck --> Native --> Effect
  Principal -.-> Audit
  Policy -.-> Audit
  Approval -.-> Audit
  Effect -.-> Audit
~~~

## 5. Privilege flow and closed operation inventory

### Current observed privileged boundary

The current Linux Host helper is a purpose-specific root process guarded by Polkit. The helper verifies its effective root identity, derives the non-root actor from PKEXEC_UID, accepts a bounded closed request enum, applies reviewed plan fingerprints and ordered effect checks, and records an owned host journal. The observed privileged effect enum is:

| Effect | Required authority and executor | Scope / confirmation |
|---|---|---|
| Prepare configuration | Native Polkit authorization; Linux Host helper | Initialize reviewed Host configuration and preserve existing external database credentials. |
| Prepare storage | Native Polkit authorization; Linux Host helper and storage adapter | Apply reviewed storage selection and ownership handoff. |
| Install verified dependencies | Native Polkit authorization; Linux Host helper/package integration | Install only verified runtime-manifest inputs represented in the reviewed plan. |
| Reconcile owned services | Native Polkit authorization; Linux Host helper/systemd adapter | Reconcile Synveil-owned service units and exact inventory. |
| Configure owned reachability | Native Polkit authorization; Linux Host helper/network adapter | Apply reviewed listener, TLS and firewall plan. |
| Stop owned services | Native Polkit authorization; Linux Host helper/systemd adapter | Stop only reviewed Synveil-owned services. |
| Remove owned software while preserving data | Native Polkit authorization; Linux Host helper | Remove only files still matching owned hashes; preserve user/server data. |

The helper also has read-only inspection and setup/account/device-enrollment protocol variants. Its inspection socket accepts only an inspect request; account creation and desktop enrollment travel through the native helper session and are not generic root commands.

Existing Linux package installation/removal delegates elevation to native package management. AppImage is user-level. Windows installer scope is current-user, and the product does not run the whole wizard as administrator. These facts do not establish an all-platform privileged-effect inventory for future v0.3 features; the exact supported OS effect set must be reviewed before implementation.

### Required future privilege sequence

1. An unprivileged UI builds a typed request and displays exact target, effect, preservation set and artifact identity.
2. The engine validates preconditions and computes a plan/fingerprint.
3. The user explicitly confirms the reviewed plan. Expiring confirmation is bound to that exact plan and cannot transfer to another effect.
4. The adapter obtains native OS authorization for the precise operation. A Synveil password/session, approval record, tailnet peer or DeviceVeil membership is insufficient.
5. The privileged process revalidates actor, plan, sequence, target, arguments, ownership and freshness immediately before effect.
6. The helper records intent before mutation, verifies the observed result after mutation, and records a typed outcome. Cancellation or interruption does not trigger blind retry.
7. The application rechecks user/resource authorization at execution; approval and OS authorization remain separate decisions.

Platform-native authentication, passkeys/FIDO2 and hardware-backed user presence are candidate step-up options only. Their feasibility and user-presence semantics are unverified for each supported OS; no product requirement should claim they exist until native acceptance evidence proves the exact mechanism.

## 6. Sensitive data flows

| Flow | Data and trust crossing | Required verification | Failure behavior |
|---|---|---|---|
| Login/session | Password over TLS; session bearer to host-only cookie; digest to PostgreSQL. | TLS endpoint identity, bounded body, Argon2id verification, new random session, expiry/revocation check, safe logs. | Reject invalid identity; do not issue session or expose account existence. |
| Browser mutation | Cookie plus CSRF cookie/header to API. | Current session, matching signed CSRF proof, Origin/Sec-Fetch-Site policy and server-side resource authorization. | Return denial; never rely on client-only authorization. |
| Device enrollment | One-use grant to exchange, device bearer once to client. | Grant digest, expiry, owner/device binding, atomic consume, TLS server identity, explicit target confirmation. | Expired/replayed/malformed grant fails; preserve local queue; require explicit re-enrollment after lost response. |
| File upload | Client bytes to API staging, then ObjectStore; expected size/hash to metadata. | Body limits, owner scope, lease generation, length/hash verification, create-only promotion and metadata reconciliation. | Keep staged/ambiguous data for safe reconciliation; do not publish unverified bytes or discard prior version. |
| File download | Logical node/version ID to metadata lookup, then opaque object key to ObjectStore stream. | Authenticate and authorize owner/resource before storage access; validate content metadata; prevent unsafe redirects. | Return generic scoped error; do not reveal whether foreign IDs or keys exist. |
| Desktop IPC | Typed local UI commands to client process; possible enrollment material to client. | Unix peer UID/path ownership or Windows owner-only pipe; protocol version, frame size, timeout and command allowlist. | Reject wrong peer, malformed frame or stale protocol; isolate failure to connection. |
| Host effects | Reviewed plan and user choices to native helper; helper invokes package/service/network/filesystem adapters. | Polkit actor, closed effect type, exact plan binding, target validation, ownership/hash check, verified artifact. | Clear authorization and stop on changed input, mismatch or unknown result; require inspection/recovery. |
| Backup/recovery | PostgreSQL manifest/plans reference ObjectStore bytes; host snapshots cover separate OS state. | Verify each store and pin/reference; confirm target owner/host and restore scope. | Reject mismatched/incomplete backup; no atomic combined rollback claim. |
| CI and artifact | Source/lockfiles/workflow to build runner and artifact channel. | Least-privilege CI token, dependency policy, trusted build provenance, signed publisher metadata. | Stop official artifact promotion; do not lower the security gate or silently use stale artifact. |
| DeviceVeil — proposed | One-time bootstrap, host identity, scoped host messages and resource contribution. | Authenticated issuance, host identity verification, explicit operator approval, per-host token scope and quotas. | Keep single-host Synveil usable when cluster is absent, partitioned or revoked. |

## 7. Boundary-by-boundary trust requirements

| Boundary | Identity and authorization evidence | Sensitive data | Required checks and failure behavior |
|---|---|---|---|
| A. Browser → API | Browser session and CSRF; API authorizes every operation. | Session/CSRF, passwords, account and file data. | Host-only Secure cookie in production; explicit origin/proxy config; deny invalid session/CSRF; no mutation replay during recovery; bounded abuse controls. |
| B. Device → API / QR | Distinct device principal, owner/device binding; future QR is one-use enrollment transport only. | Enrollment grant, device bearer, profile and cached files. | TLS server verification, explicit confirmation, expiry/replay checks, rotation/revocation; stale credentials fail closed. |
| C. Identity/permissions/approval | Typed principal plus current policy/resource scope; approval is separately attributed. | Roles, grants, approval parameters, audit outcomes. | Exact legacy owner mapping; no implicit inheritance; execution-time reauthorization; stale/expired/replayed decisions fail. |
| D. UI/engine → privileged OS | Unprivileged user confirmation plus native OS authentication and typed adapter operation. | Host configuration, credentials, system packages/services, user data. | Closed operation list; exact argument and target checks; journal intent/result; no remote-to-root bridge. |
| E. Client → network → API | Verified HTTPS server identity plus app-level user/device auth. | Credentials, files, API requests, endpoint identity. | No public listener by default; no TLS bypass, HTTP downgrade or automatic weaker fallback; Tailscale only transports packets. |
| F. Desktop UI → local IPC / future CLI | OS peer/ACL for local IPC; remote CLI uses typed HTTPS API and device credential. | Enrollment input, local profile and token. | Reject substituted endpoints and malformed frames; same-user code is not isolated; no arbitrary remote shell. |
| G. Application → Protected Areas | Server policy and exact resource identity for app-managed writes; future OS enforcement is separate. | Protected content, integrity baseline, approval and mutation evidence. | Guard every supported write path; state actual platform coverage; fail closed on stale baseline; do not claim arbitrary privileged writes are blocked. |
| H. API/worker → PostgreSQL/ObjectStore/backup | Service identity and data-owner scope; DB and byte store are separate. | Canonical metadata, object bytes, backup/recovery evidence. | Least DB/storage credentials; opaque key boundary; checksum/length; durable GC/restore planning; mismatch quarantined and reconciled. |
| I. Synveil ↔ DeviceVeil | Future mutually authenticated host identity and explicit contract. | Host credentials, cluster membership, resource quotas and topology. | Per-host scope and revocation; membership never implies Synveil admin/user rights; optional failure stays local. |
| J. Source/CI → release | Repository identity, pinned dependencies/actions, build provenance and publisher identity. | CI secrets, signing keys, source and artifacts. | Minimal workflow permissions; reproducible inputs where feasible; signed metadata and downgrade rules before official release claim. |

## 8. Expected fail-closed behavior

| Failure | Required result |
|---|---|
| Unknown identity, malformed token, expired session, revoked device or mismatched owner/device scope | Deny protected operation; expose no foreign resource existence. |
| Missing CSRF proof, untrusted Origin, invalid TLS certificate or unverified endpoint | Reject mutation/connection; do not retry over HTTP or a weaker endpoint. |
| Missing/expired/replayed invitation or changed QR server identity | Do not enroll; preserve existing client profile and queue. |
| Missing role mapping, invalid permission inheritance, stale policy revision or expired approval | Deny; do not substitute owner, admin, network or default role. |
| Native authorization denied or privileged plan changed | Do not execute; clear pending confirmation and retain inspection evidence. |
| Unknown object state, hash/length mismatch, missing object bytes or database/object mismatch | Do not publish or delete; retain evidence for bounded reconciliation and operator recovery. |
| Interrupted client queue, migration or restore | Preserve operation identity and source state; report recovery required; never silently reset queue or claim combined atomic rollback. |
| DeviceVeil outage, partition or revocation | Disable only optional cluster operations; continue supported single-host behavior. |
| Dependency or publisher verification fails | Stop official artifact promotion; do not lower the security gate or silently use a stale artifact. |

## 9. Native enforcement limits

Application-managed protection can reject writes made through Synveil API and supported client workflows. It cannot prevent arbitrary writes by a local process that already has sufficient filesystem or administrator privilege.

- **Linux:** file permissions, ownership, mount flags, SELinux/AppArmor or verified filesystem features may support stronger controls, but Synveil has not selected or qualified one for Protected Areas. Root can bypass ordinary DAC.
- **Windows:** NTFS ACLs and integrity controls can constrain processes, but administrators can change ACLs; a minifilter or comparable interception design needs separate review and signed native acceptance.
- **Android:** app sandboxing and Keystore scope protect app-private state under Android's security model; a rooted or compromised OS can bypass application assumptions. Shared/user-visible files need platform-specific document-provider checks.
- **iOS:** app sandbox and Keychain protect application state under iOS security model; they do not create general arbitrary filesystem write protection. File Provider coordination is not a kernel-wide guarantee.

No kernel module, filesystem driver, passkey flow or cross-platform native enforcement is introduced or claimed by P003.

## 10. Evidence references

See the [threat model evidence ledger](P003_THREAT_MODEL.md#10-evidence-ledger) for source and test paths. Principal boundary sources are [API auth](../../../crates/api/src/auth.rs), [CSRF](../../../crates/api/src/csrf.rs), [device auth](../../../crates/api/src/device_auth.rs), [server network plan](../../../crates/server-network/src/lib.rs), [host helper protocol](../../../crates/server-host/src/protocol.rs), [desktop IPC](../../../crates/client/src/control.rs), [ObjectStore key type](../../../crates/object-store/src/keys.rs), [Android transport](../../../clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt), and [iOS Keychain store](../../../clients/ios/Infrastructure/Security/KeychainCredentialStore.swift).

These boundaries constrain later implementation. Documentation is not evidence that a control has been implemented.
