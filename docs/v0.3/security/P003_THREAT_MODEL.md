# Synveil v0.3 — P003 security threat model

**Status:** proposed security baseline for maintainer review. This document does not claim that proposed controls exist or are qualified.
**Evidence snapshot:** 2026-10-25; source tree at P002 head f63374f19a81bbd4b9ef6881445a165c07ba0700.
**Integration baseline:** develop/v0.3 at d69e167913feec5ebed548b60eb07cbdf40ba6e9.
**Scope:** security analysis and documentation only. See [P001 audit](../audits/P001_V02_BASELINE_AUDIT.md), [P001 compatibility baseline](../audits/P001_COMPATIBILITY_BASELINE.md), and [P002 architecture](../architecture/P002_SYSTEM_ARCHITECTURE.md).

## 1. Purpose, method and evidence limits

This model turns the P002 trust-boundary inventory into security risks and testable follow-up requirements. It covers the current v0.2 source boundary and proposed v0.3 surfaces: browser/API, device enrollment and QR pairing, RBAC and approval, privileged host operations, SynveilNet/Tailscale, desktop IPC and CLI, Protected Areas, storage and recovery, DeviceVeil, and software delivery.

STRIDE is the primary classification. Abuse cases are included where a feature lifecycle or recovery path creates risk that is not captured by a single request-level threat.

Evidence labels distinguish:

- **Observed:** source, test, configuration or workflow exists at the evidence snapshot.
- **Proposed:** a required future control or feature, not implemented.
- **Unknown:** repository evidence does not establish the behavior or guarantee.

An existing test demonstrates only its tested code path and environment. It does not prove a whole-system, cross-platform, production, or native OS guarantee. P001 and P002 remain the authoritative baseline summaries; this file links to them instead of copying their contents.

The P001–P060 roadmap named in the task is not present in the repository or workspace. P002 records the same limitation. Phase references below use the feature ranges proposed in P002 as planning labels only; they do not infer unseen roadmap requirements.

## 2. Scope, assumptions and exclusions

### In scope

- Browser and device identity, authentication, revocation and current owner/device authorization.
- Server-side API authorization and API v1 compatibility.
- Sensitive data stored or transmitted by Synveil clients, server modules and deployment tooling.
- Existing Linux Host helper, Polkit boundary, installer engine and platform package operations.
- Proposed Tailscale transport, QR pairing, roles, approvals, Protected Areas, CLI and DeviceVeil integration.
- PostgreSQL metadata, ObjectStore bytes, local queues, backups, restore plans, host configuration, CI and release artifacts.

### Assumptions

- HTTP clients, browser code, QR images, peer names, discovered endpoints, local IPC payloads, remote hosts, uploaded files and external package registries are untrusted inputs.
- Production browser and mobile traffic uses TLS with certificate and hostname verification. Operator-managed proxies and public origins are deployment trust inputs that must be configured and reviewed.
- Existing v0.2 owner/device route checks are the compatibility oracle. A new role, token or network identity cannot increase those rights by implication.
- PostgreSQL and ObjectStore are separate durability and recovery domains. A relational snapshot alone does not include object bytes.
- A production build or release is trusted only to the degree established by its source, dependency, workflow, artifact and publisher verification evidence.
- The current implementation is a baseline to preserve, not proof that every deployment uses safe configuration.

### Explicit exclusions

- A fully compromised kernel, root/administrator account, hypervisor or cloud control plane can usually bypass application-layer authorization and local file policy. Synveil can limit, detect, record and recover from some effects, but cannot promise to defeat that authority.
- Physical extraction, side-channel attacks, compromised firmware, malicious browser extensions, compromised upstream certificate authorities, and denial of service outside Synveil control are not fully preventable here. They remain residual risks.
- This is not a penetration test, formal verification, cryptographic protocol proof, legal retention opinion or release qualification.

## 3. Risk ranking

Each risk uses likelihood L and impact I from 1 to 5; score is L × I.

| Value | Likelihood | Impact |
|---|---|---|
| 1 | Unlikely under the stated attacker model and exposure. | Negligible, isolated inconvenience. |
| 2 | Plausible but requires constrained access or multiple failures. | Limited data or one-device disruption with straightforward recovery. |
| 3 | Realistic for an attacker with the stated foothold. | Material account, device, data or service impact. |
| 4 | Likely on an exposed boundary or broad compromise of an account, host or durable state. | Severe compromise, broad data loss or sustained service outage. |
| 5 | Credible catastrophic path once the stated prerequisite is met. | Cross-host/administrator compromise, irreversible broad data loss or large-scale credential/content exposure. |

| Score | Severity | Priority |
|---:|---|---|
| 20–25 | Critical | Blocks the affected feature or release claim until an approved design, mitigation and test exist. |
| 12–19 | High | Must have an owned mitigation and verification plan before the affected feature is enabled. |
| 6–11 | Moderate | Schedule and verify before broad support; document accepted residual risk. |
| 1–5 | Low | Track where useful; do not let it obscure higher risks. |

Status means: **MITIGATED** = evidence demonstrates the risk is addressed within scope; **PARTIAL** = verified controls reduce it but leave material exposure; **OPEN** = no sufficient control was found; **BLOCKED** = the affected future feature must not proceed before a decision/control; **NOT_APPLICABLE** = explicitly excluded. No threat is labeled MITIGATED merely because a mitigation is proposed.

## 4. Threat actors

| Actor | Capabilities and likely objective | Important limit |
|---|---|---|
| ACT-01 Unauthenticated Internet or LAN attacker | Probes exposed routes, exploits parsing/configuration mistakes, guesses credentials, floods login/upload paths, manipulates discovery. | Cannot assume a listener is public; default managed mode is loopback, and LAN exposure is separately configured. |
| ACT-02 Malicious authenticated user | Uses valid account access to read, change, delete, restore, approve or share resources beyond intended scope. | Must still cross server-side resource checks; login is not universal authorization. |
| ACT-03 Compromised browser session | Replays a stolen cookie, uses the browser as the victim or attempts cross-origin mutations. | Cannot forge the session-bound CSRF MAC without the server key; XSS or endpoint compromise can still act as the user. |
| ACT-04 Lost or stolen mobile device | Uses local app state, cached data and an unrevoked credential; may be offline when revoked. | Keychain/Keystore helps protect stored material, but a compromised unlocked OS or app process can use an available credential. |
| ACT-05 Compromised desktop user process | Reads or controls data available to the logged-in user and invokes desktop IPC commands. | Same-user code is not separated from the user by existing per-user IPC. It must not gain root or another user's server scope. |
| ACT-06 Malicious local unprivileged process | Substitutes local endpoints, sends malformed IPC, probes files or abuses shared-user/session resources. | Cannot be assumed to control root-owned files or the kernel. |
| ACT-07 Compromised Tailscale peer | Reaches a tailnet endpoint and attempts endpoint substitution or lateral movement. | Tailnet membership is transport reachability, not Synveil identity or permission. No Tailscale integration exists at baseline. |
| ACT-08 Malicious or compromised DeviceVeil node | Presents false host identity, consumes quota, requests resources or tries to use cluster credentials across hosts. | DeviceVeil is optional and absent from current Synveil source. |
| ACT-09 Attacker holding a leaked API token | Calls token-authorized endpoints from another process or network. | Current device credentials are owner/device-scoped, but the bearer remains usable until revoked; credential expiry/rotation was not found. |
| ACT-10 Supply-chain attacker | Compromises a package, transitive dependency, GitHub Action, build runner, artifact channel, signing key or update metadata. | Current checks are partial; official release signing and publisher claims are not qualified by P001. |
| ACT-11 Operator error | Selects an unsafe listener, trusts an incorrect proxy, approves the wrong action, restores mismatched backups or removes data. | Human confirmation alone is not a security control; plans, scope, revalidation and recoverability still matter. |

## 5. Protected asset inventory

Confidentiality, integrity and availability are rated Critical, High, Moderate or Low. These are security priorities, not product classifications.

| ID | Asset | Custodian / boundary | C | I | A | Why it matters |
|---|---|---|---|---|---|---|
| AS-01 | User accounts, login identities, password verifiers and account status | Auth service / PostgreSQL | H | C | H | Enables account impersonation and ownership changes. |
| AS-02 | Browser sessions and CSRF signing material | Browser, API and PostgreSQL | C | C | H | Session theft grants the matching user's current API authority. |
| AS-03 | Device credentials and credential identifiers | Auth/API, mobile/desktop secure stores | C | C | H | Bearer compromise permits device-authorized API calls until revocation. |
| AS-04 | Enrollment grants, pairing invitations and QR payloads | Browser, API, mobile camera/display | C | C | H | A live enrollment secret can bind a credential to an owner/device. |
| AS-05 | API tokens and future scoped tokens | Client vault, API | C | C | H | Carries remote authorization and can be copied or replayed. |
| AS-06 | Approval requests, decisions and execution receipts | Proposed API/metadata and native adapter | H | C | H | Incorrect scope or replay can authorize destructive or privileged work. |
| AS-07 | Security audit records and request correlation | API, metadata, host journal, deployment logging | H | C | H | Needed to attribute sensitive actions; not equivalent to ordinary tracing. |
| AS-08 | File contents, staged uploads and downloaded content | ObjectStore, API and clients | C | C | H | User content may contain private or regulated information. |
| AS-09 | File metadata, versions, conflict evidence and history | PostgreSQL and client projections | H | C | H | Reveals activity and defines durable file identity and recovery. |
| AS-10 | Backup manifests, content pins, restore/prune plans and outcomes | PostgreSQL, API and ObjectStore | H | C | C | Can expose historic content or cause bulk deletion/restoration. |
| AS-11 | PostgreSQL identity, metadata, grants, journals and plans | API/worker/migration process and PostgreSQL | C | C | C | Canonical identity and metadata; corruption or loss affects all users. |
| AS-12 | ObjectStore bytes, opaque keys, staging records and integrity metadata | API/worker and ObjectStore backend | C | C | C | Canonical bytes and verification evidence are distinct from metadata. |
| AS-13 | Desktop sync queues, profiles, checkpoints, mutations and conflicts | Client SQLite and OS credential store | H | C | C | Durable replay/recovery state; deletion or rebinding can lose user work. |
| AS-14 | Android Room/DataStore and iOS SQLite/File Provider/photo state | Mobile sandbox, Keystore/Keychain | H | C | C | Holds profiles, queues, transfer state and local content metadata. |
| AS-15 | Installer plans, effect fingerprints and lifecycle journals | Install engine, host helper and filesystem | H | C | H | Stale or altered plan state can cause unowned privileged changes or data loss. |
| AS-16 | Host configuration, database URLs, TLS keys, CA keys and deployment inputs | OS-protected config and credential files | C | C | C | Controls service identity, endpoint trust, storage path and database access. |
| AS-17 | OS privilege, Polkit actor identity, package-manager authority and service identities | Native OS / installer and host adapters | H | C | H | A remote token must never imply administrator/root authority. |
| AS-18 | Network identity, canonical endpoints, DNS answers, TLS certificates and trust descriptors | Client, host, proxy, OS trust store | H | C | H | Endpoint substitution can steal credentials or redirect protected traffic. |
| AS-19 | DeviceVeil cluster/host identities, invitations, credentials and resource limits | Future DeviceVeil adapter and participating hosts | H | C | H | A compromised node must not receive account, host or storage authority by membership alone. |
| AS-20 | Dependency lockfiles, build inputs, CI identity, provenance and release artifacts | Git repository, CI, package/update channels | M | C | H | Build compromise can ship malicious code despite application authorization. |

## 6. Existing controls observed at the evidence snapshot

These controls are evidence of implementation, not a statement that the system is secure or release-qualified.

- Browser login issues a fresh 256-bit session bearer, persists its SHA-256 digest, checks account status and expiration, and revokes the current session on logout. The baseline default session TTL is eight hours; configuration is bounded to 30 days. See [session implementation](../../../crates/auth/src/sessions.rs), [session policy](../../../crates/auth/src/config.rs), and [auth service](../../../crates/auth/src/service.rs).
- Production session cookies are host-only, HttpOnly, Secure, SameSite=Lax; browser mutations use a session-bound HMAC double-submit token and provenance checks. Managed edge configuration strips client forwarding headers and forces HTTPS; external proxy behavior remains operator-owned. See [cookie policy](../../../crates/api/src/cookies.rs), [CSRF checks](../../../crates/api/src/csrf.rs), and [managed edge plan](../../../crates/server-network/src/lib.rs).
- Browser and device principals remain distinct. Device enrollment grants are random, digest-only, one-use and ten minutes by default, with a fifteen-minute database ceiling; devices are owner/device bound and revocable. No device-credential expiry or automatic rotation was found. See [device auth](../../../crates/auth/src/device_credentials.rs), [metadata boundary](../../../crates/metadata/src/device_credentials.rs), and [migration](../../../migrations/20260828000000_device_credentials_enrollment.sql).
- API routes authenticate before protected operations; repositories receive owner, library and device scope. Login bodies and other request classes have explicit size limits. A general login/API rate limiter was not found; bounded rate limits exist for selected sync snapshot operations.
- Desktop IPC uses a versioned bounded frame, Unix peer UID check and owner-only Windows named-pipe security descriptor, with connection and time limits. A process running as the same user remains in that OS trust domain. See [desktop IPC](../../../crates/client/src/control.rs) and [IPC tests](../../../crates/client/tests/desktop_control_ipc.rs).
- The Linux host helper uses a closed effect enum, Polkit authorization, actor UID, reviewed plan fingerprints, sequence checks, nonce/replay checks and an append-only setup journal. Effects include configuration/storage preparation, verified dependency installation, owned service reconciliation, reachability configuration, stop and software removal that preserves data. See [helper protocol](../../../crates/server-host/src/protocol.rs), [native host owner](../../../crates/server-host/src/native.rs), and [Polkit policy](../../../deploy/polkit/org.synveil.host.policy). This is not a general command service and does not establish a cross-platform wizard privilege contract.
- Managed host network defaults to loopback/local-only; private LAN is explicit and uses a reviewed listener/firewall plan and HTTPS. Tailscale is heuristically classified as a tunnel from interface type/name; no Tailscale control integration was found.
- Object keys reject absolute/traversal forms; uploads use staging, lease generations, length/hash checks and reconciliation. PostgreSQL and ObjectStore remain separate, and no atomic cross-store restore protocol was found. See [ObjectStore key type](../../../crates/object-store/src/keys.rs), [upload lifecycle](../../../crates/storage/src/uploads.rs), [local ObjectStore](../../../crates/storage/src/local.rs), and [P002 recovery contract](../architecture/P002_COMPATIBILITY_CONTRACTS.md).
- Android credentials use an Android Keystore AES-GCM key and scoped envelope; iOS credentials use a device-only Keychain item; desktop credentials use Secret Service or Windows Credential Manager with unsupported backends failing explicitly. These controls do not defeat a compromised unlocked device or same-user process.
- Request tracing records request/trace ID, method, matched route, status and latency; this is not a durable security audit log. No generic persistent actor-attributed security audit/approval model, RBAC tables, QR pairing, Protected Areas policy, CLI, Tailscale adapter or DeviceVeil integration was found.

## 7. STRIDE coverage map

| STRIDE class | Threat IDs | Main concern |
|---|---|---|
| Spoofing | TM-001, TM-008, TM-009, TM-021, TM-024, TM-026, TM-037, TM-042 | Session, invitation, endpoint, host or build identity impersonation. |
| Tampering | TM-002, TM-013, TM-014, TM-015, TM-018, TM-019, TM-029, TM-030, TM-033, TM-035, TM-039, TM-043 | Requests, policy, plans, content, backups, scripts or update state changed without authorized scope. |
| Repudiation | TM-007, TM-016, TM-020, TM-040, TM-042 | Insufficient attribution or durable evidence for a security-sensitive result. |
| Information disclosure | TM-005, TM-011, TM-012, TM-017, TM-022, TM-025, TM-028, TM-032, TM-036, TM-038 | Secret, content, credential, host or historical data exposure. |
| Denial of service | TM-003, TM-004, TM-010, TM-020, TM-023, TM-027, TM-034, TM-040, TM-041, TM-042 | Resource exhaustion, lockout, unavailable recovery or dependency/build failure. |
| Elevation of privilege | TM-006, TM-013, TM-015, TM-017, TM-018, TM-022, TM-028, TM-030, TM-037, TM-038, TM-039, TM-042 | Scope expansion from login, token, peer, local process, cluster or build authority. |

## 8. Threat register

Likelihood and impact use the 1–5 scale in section 3. Existing controls below are only those verified in the source snapshot. Future implementations must add evidence from the acceptance matrix in [security requirements](P003_SECURITY_REQUIREMENTS.md).

### Browser and API

#### TM-001 — Session theft, replay or fixation

- **STRIDE / assets:** Spoofing; AS-01, AS-02. **Actor:** ACT-03 or ACT-09 with a stolen cookie. **Entry / boundary:** browser cookie to API.
- **Attack path and impact:** Steal or replay a bearer, or cause a victim to retain an attacker-known session. The bearer grants that session principal's authority until expiry or revocation.
- **Risk:** L=3, I=5, score 15 — **High** because one browser session can expose or mutate all resources reachable by that user.
- **Existing controls:** Fresh random token on login; only digest stored; bounded expiry; host-only HttpOnly/Secure production cookie; session checked against revocation, expiry and active user; logout revokes. No separate session-renew endpoint was found. (E-03, E-04)
- **Mitigation / residual:** Require TLS, strict cookie/proxy configuration, session rotation after future authentication-strength or privilege changes, revocation tests and no bearer reuse in recovery. Residual theft remains possible with XSS, endpoint compromise or TLS/proxy compromise.
- **Verification / owner / phase / status:** AT-01, AT-02; auth/API/web; P003 design, P025–P030 implementation; **PARTIAL**.

#### TM-002 — CSRF or reverse-proxy origin confusion

- **STRIDE / assets:** Tampering; AS-02, AS-16, AS-18. **Actor:** ACT-01 or ACT-03. **Entry / boundary:** cross-origin browser request or operator proxy to API.
- **Attack path and impact:** Forge a cookie-authenticated mutation, or make the API trust incorrect origin/forwarded headers. Success changes protected resources as the victim.
- **Risk:** L=3, I=4, score 12 — **High** because a browser carries ambient session authority.
- **Existing controls:** Session-bound HMAC double-submit CSRF proof, same-origin provenance check when present, explicitly configured public origin, host-only cookies, managed edge strips inbound forwarding headers. Operator proxy behavior is not controlled by Synveil. (E-04, E-09)
- **Mitigation / residual:** Keep server checks authoritative; require canonical public origin and trusted-proxy configuration; reject malformed, null or mismatched origins; prohibit wildcard credentialed CORS and header-based origin inference. Residual risk remains in operator proxy misconfiguration and browser compromise.
- **Verification / owner / phase / status:** AT-03; API/auth/deploy/web; P003 design, P025–P030 regression; **PARTIAL**.

#### TM-003 — Login and public-route resource exhaustion

- **STRIDE / assets:** Denial of service; AS-01, AS-11, AS-18. **Actor:** ACT-01. **Entry / boundary:** login, bootstrap status and public API.
- **Attack path and impact:** Repeated Argon2id login attempts or request bursts consume CPU, memory, database connections or log capacity; first-admin contention can deny setup.
- **Risk:** L=4, I=3, score 12 — **High** because public authentication is reachable wherever the API is exposed.
- **Existing controls:** Login has a body limit, unknown users receive dummy-password verification, password inputs are bounded, and selected sync snapshot operations have a specific rate limit. A global or login-specific server rate limiter was not found. (E-03, E-04)
- **Mitigation / residual:** Add bounded per-account and per-source admission controls with lockout-resistant recovery, bounded queues, concurrency caps and observable throttling; use edge protection only as defense in depth. Volumetric DoS remains.
- **Verification / owner / phase / status:** AT-04; API/auth/edge; P003 requirement, P025–P030 implementation; **OPEN**.

#### TM-004 — Unsafe anonymous first-admin bootstrap race

- **STRIDE / assets:** Denial of service, elevation; AS-01, AS-11. **Actor:** ACT-01 or ACT-11. **Entry / boundary:** unauthenticated bootstrap endpoint before first-admin state closes.
- **Attack path and impact:** Reach bootstrap before the intended owner, or exploit unsafe proxy/exposure so an attacker wins the first-admin transaction or blocks setup.
- **Risk:** L=3, I=4, score 12 — **High** because the first successful claimant becomes instance administrator.
- **Existing controls:** Singleton/locked first-admin creation and fail-closed inconsistent state; helper-owned setup hides the route; managed edge returns 404 for the route; browser provenance is checked when headers are present. Anonymous bootstrap still relies on deployment exposure until closed. (E-03, E-04, E-09)
- **Mitigation / residual:** Keep local approval or equivalent authenticated bootstrap trust for every supported deployment; verify public route exposure and proxy behavior before setup; prevent reopening after completion and audit the winning attempt. Residual risk remains if the host or proxy is exposed before the intended operator.
- **Verification / owner / phase / status:** AT-05; API/auth/host/deploy; P003 design, P007–P012 and P025–P030; **PARTIAL**.

#### TM-005 — Secret disclosure in logs, URLs, diagnostics or crash data

- **STRIDE / assets:** Information disclosure; AS-02, AS-03, AS-04, AS-05, AS-08. **Actor:** ACT-01, ACT-05, ACT-09, ACT-10 or an operator with log access. **Entry / boundary:** HTTP headers/body, URI, diagnostics, logs and crash reporting.
- **Attack path and impact:** A bearer, password, QR grant, content body or sensitive URL is copied into telemetry and then replayed or disclosed.
- **Risk:** L=3, I=5, score 15 — **High** because secret leakage can become immediate account/device access.
- **Existing controls:** API traces record method, matched route and status rather than query/body; enrollment DTOs and secret types avoid Debug; diagnostics redact sensitive values and tests exercise redaction. No generic crash-report policy or universal log review evidence was found. (E-03, E-04, E-13)
- **Mitigation / residual:** Redact at ingress and egress, keep secrets out of URLs/process arguments, cap and sanitize diagnostic payloads, document crash-dump policy, and test representative failures. Residual risk remains in host-level memory dumps and third-party telemetry.
- **Verification / owner / phase / status:** AT-06; API/web/desktop/mobile/host; P003 and each client feature phase; **PARTIAL**.

#### TM-006 — Cross-user, cross-device or cross-host authorization bypass

- **STRIDE / assets:** Elevation of privilege; AS-01, AS-03, AS-08, AS-09, AS-11, AS-12. **Actor:** ACT-02 or ACT-09. **Entry / boundary:** authenticated API route to metadata or content service.
- **Attack path and impact:** Guess a logical ID, swap owner/library/device identifiers, or exploit a future handler that authenticates but omits authorization. Result: unauthorized reads, writes, deletion or restore.
- **Risk:** L=3, I=5, score 15 — **High** because a route omission can cross account or device boundaries.
- **Existing controls:** Browser and device principals are distinct; device principals bind owner/device/credential; handlers and repositories pass current owner/library/device scope; physical ObjectStore keys are not client authority. No general RBAC exists. (E-04, E-06)
- **Mitigation / residual:** Require an explicit per-route principal/action/resource matrix, deny-by-default policy, repository-level scope predicates and cross-account/device/host tests for every new route. Residual risk remains from future omissions or compromised server code.
- **Verification / owner / phase / status:** AT-07; API/auth/metadata; P003 baseline, P025–P030; **PARTIAL**.

#### TM-007 — Request tracing mistaken for a security audit

- **STRIDE / assets:** Repudiation; AS-06, AS-07, AS-15. **Actor:** ACT-02 or ACT-11. **Entry / boundary:** approval, revocation, privileged effect or destructive restore.
- **Attack path and impact:** A user or operator disputes an action while request IDs and ordinary logs fail to identify actor, scope, policy, approval, execution result or tampering.
- **Risk:** L=3, I=4, score 12 — **High** because investigations and accountability for destructive actions cannot be established.
- **Existing controls:** Request/trace correlation and domain outcome records exist; no generic actor-attributed persistent security audit/approver model was found. (E-04, E-11)
- **Mitigation / residual:** Define append-only or integrity-protected event records with actor, principal, operation fingerprint, exact resource/host, decision, policy revision, timestamps and execution result; separate audit read permissions and define retention by maintainer policy. Do not claim legal retention.
- **Verification / owner / phase / status:** AT-08; API/metadata/host; P003 decision, P025–P030 and P049–P054; **OPEN**.

### Device identity and QR pairing

#### TM-008 — Stolen, replayed or duplicated enrollment material

- **STRIDE / assets:** Spoofing, elevation; AS-03, AS-04. **Actor:** ACT-03, ACT-04 or ACT-09. **Entry / boundary:** grant response or future QR payload to enrollment exchange.
- **Attack path and impact:** Copy a grant or QR payload before redemption and redeem it first, binding a credential to the intended owner/device without the owner's approval.
- **Risk:** L=3, I=5, score 15 — **High** because possession of the one-time grant is currently exchange authority.
- **Existing controls:** Current grant is high-entropy, digest-only, ten minutes by default, one-use consumed transactionally, owner/device bound and revocable. QR pairing and explicit scan confirmation do not exist. (E-05)
- **Mitigation / residual:** Future invitation must be one-use, short-lived, nonce-bound to server/device/request scope, require explicit owner confirmation and never contain a permanent credential. Consume atomically and expose replay as generic failure. Residual shoulder-surfing/interception remains until confirmed exchange.
- **Verification / owner / phase / status:** AT-09, AT-10; auth/API/web/mobile; P003 design, P019–P024; **PARTIAL**.

#### TM-009 — QR substitution or false server identity

- **STRIDE / assets:** Spoofing, tampering; AS-04, AS-18. **Actor:** ACT-01, ACT-03 or ACT-11. **Entry / boundary:** QR display/camera to client endpoint and enrollment request.
- **Attack path and impact:** Replace the QR or endpoint so a client sends enrollment material to an attacker server, or accepts an attacker server certificate/name.
- **Risk:** L=4, I=5, score 20 — **Critical** because a credential can be issued to the wrong host or stolen during bootstrap.
- **Existing controls:** Mobile profiles require HTTPS in production and reject redirects; no QR bootstrap or cryptographic server-identity confirmation exists. TLS hostname validation does not prove that a user scanned the intended host. (E-05, E-12)
- **Mitigation / residual:** Bind invitation exchange to an already verified canonical HTTPS endpoint and server installation identity; show endpoint/fingerprint for explicit confirmation; reject HTTP, invalid TLS, redirects, endpoint changes and silent fallback. Design recovery for endpoint rotation before pairing.
- **Verification / owner / phase / status:** AT-11; Android/iOS/API; P003 decision, P019–P024; **BLOCKED**.

#### TM-010 — Concurrent redemption and lost-response recovery

- **STRIDE / assets:** Denial of service, tampering; AS-03, AS-04. **Actor:** ACT-04 or ACT-09. **Entry / boundary:** competing requests to consume one grant.
- **Attack path and impact:** Race redemption, or drop the successful response after the grant is consumed. A legitimate client may be unable to recover the just-issued secret and may repeatedly create competing devices.
- **Risk:** L=3, I=4, score 12 — **High** because recovery errors can either widen enrollment or strand a device.
- **Existing controls:** Consumption is one-time; the raw device credential is returned once and not recoverable; documented recovery is owner revoke-all for that device, then create a fresh grant. (E-05)
- **Mitigation / residual:** Preserve one-time secret handling; make exact state/result observable without returning secrets; provide explicit re-enrollment; serialize concurrent transitions. Never make exchange replay return the old bearer.
- **Verification / owner / phase / status:** AT-12; auth/metadata/mobile; P003 design, P019–P024; **PARTIAL**.

#### TM-011 — Lost device, cloned backup or long-lived bearer

- **STRIDE / assets:** Information disclosure, spoofing; AS-03, AS-13, AS-14. **Actor:** ACT-04 or ACT-09. **Entry / boundary:** local secure store to device-authenticated API.
- **Attack path and impact:** A lost or copied device credential remains usable until the owner can revoke it. Offline clients may retain cached content and delayed mutations.
- **Risk:** L=4, I=5, score 20 — **Critical** because current device credential expiry/rotation was not evidenced in the server schema.
- **Existing controls:** Per-credential and per-device revocation; Android Keystore envelope and iOS device-only Keychain; owner/device scope. Credential expiry or automatic rotation was not found. Offline state is durable. (E-05, E-12)
- **Mitigation / residual:** Define bounded credential lifetime, rotation overlap, immediate server-side revocation, last-use evidence and recovery that preserves queued work without rebinding identity. Revocation cannot erase previously copied plaintext or cached files.
- **Verification / owner / phase / status:** AT-13; auth/API/metadata/mobile; P003 decision, P019–P030; **OPEN**.

#### TM-012 — Device profile or credential bound to a substituted endpoint

- **STRIDE / assets:** Information disclosure, elevation; AS-03, AS-14, AS-18. **Actor:** ACT-04 or ACT-09. **Entry / boundary:** local profile, endpoint change or reconnect.
- **Attack path and impact:** Reuse an old credential against a new host or silently rebind its profile/owner/device tuple, sending a bearer to the wrong service or changing authorized identity.
- **Risk:** L=3, I=4, score 12 — **High** because server and client both hold durable identity bindings.
- **Existing controls:** Android credential envelopes bind profile, origin, transport, owner, device and credential; iOS Keychain records validate endpoint; HTTPS is required outside explicit debug loopback. (E-12)
- **Mitigation / residual:** Require deliberate re-enrollment or verified endpoint migration, preserve old queued operations, and fail closed on origin/owner/device mismatch. Do not trust profile labels or matching UUIDs.
- **Verification / owner / phase / status:** AT-14; Android/iOS/desktop/platform; P003 and P019–P030; **PARTIAL**.

### RBAC, permission and approval

#### TM-013 — RBAC mapping broadens legacy owner/device permissions

- **STRIDE / assets:** Tampering, elevation; AS-01, AS-03, AS-06, AS-09. **Actor:** ACT-02 or ACT-11. **Entry / boundary:** role assignment, migration/backfill, permission inheritance.
- **Attack path and impact:** Map the current owner/device to a broader role than existing route scope, infer authority from label/UUID/network, or let a new role inherit delete/restore/admin rights by default.
- **Risk:** L=4, I=5, score 20 — **Critical** because migration could broaden every existing account or device.
- **Existing controls:** Existing principal classes and route/repository owner/device checks are explicit; no general role model or role assignment exists. (E-04, E-06, P002 ADR-P002-005)
- **Mitigation / residual:** Freeze a route/action/resource compatibility matrix; preserve old rights exactly; require explicit grant rows and deny rules; shadow-compare old/new policy before cutover; use additive migrations with reversible read compatibility.
- **Verification / owner / phase / status:** AT-15; auth/API/metadata/web/mobile; P003 decision, P025–P030; **BLOCKED**.

#### TM-014 — Stale authorization after concurrent role or account change

- **STRIDE / assets:** Tampering, elevation; AS-01, AS-06, AS-11. **Actor:** ACT-02 or ACT-11. **Entry / boundary:** check-to-use window between API authorization and mutation.
- **Attack path and impact:** A request is authorized under stale role/device state, then executes after revocation, ownership change or policy update.
- **Risk:** L=3, I=5, score 15 — **High** because a race can complete a destructive action after access is removed.
- **Existing controls:** Browser session auth reloads current user status; device auth reloads owner/device credential state. General roles/policies and concurrency semantics do not exist. (E-03, E-05, E-06)
- **Mitigation / residual:** Bind policy revision and resource version to the operation; reauthorize in the committing transaction or enforce a durable authorization generation; reject stale policy.
- **Verification / owner / phase / status:** AT-16; auth/API/metadata; P003 design, P025–P036; **BLOCKED**.

#### TM-015 — Approval replay, scope substitution or TOCTOU

- **STRIDE / assets:** Tampering, elevation; AS-06, AS-07, AS-15. **Actor:** ACT-02, ACT-09 or ACT-11. **Entry / boundary:** approval decision to later execution.
- **Attack path and impact:** Reuse old approval for changed resource, host, privilege or argument; replay after expiry/revocation; change state between approval and privileged execution.
- **Risk:** L=4, I=5, score 20 — **Critical** because a stale decision can authorize a different destructive or privileged effect.
- **Existing controls:** No generic approval object or API exists. Host helper separately binds plan fingerprint, effects, sequence and changing snapshot; that is not application approval. (E-08, P002 ADR-P002-006)
- **Mitigation / residual:** Bind requester, approver, operation, exact resource/host, requested privilege, parameters/fingerprint, expiry, nonce, policy revision, decision and execution result. Consume once and independently revalidate authorization before effect.
- **Verification / owner / phase / status:** AT-17; API/auth/metadata/native helper; P003 decision, P025–P054; **BLOCKED**.

#### TM-016 — Self-approval, collusion or unaudited approver changes

- **STRIDE / assets:** Repudiation, elevation; AS-06, AS-07. **Actor:** ACT-02 or compromised approver. **Entry / boundary:** requester/approver identities and approval policy.
- **Attack path and impact:** A requester approves their own destructive or privilege-granting request, changes who may approve, or later denies the action.
- **Risk:** L=3, I=5, score 15 — **High** because approval separation is a core control for high-impact operations.
- **Existing controls:** No approval actors, role assignment or durable security audit model exists. (E-07, P002 ADR-P002-006)
- **Mitigation / residual:** Require distinct requester and approver for destructive, cross-user, privilege-granting and host-level actions; forbid self-approval in those classes; audit policy/approver changes. A single-owner exception requires explicit maintainer approval.
- **Verification / owner / phase / status:** AT-18; auth/API/metadata; P003 decision, P025–P054; **BLOCKED**.

### Root and privileged operations

#### TM-017 — Remote Synveil credential becomes arbitrary root authority

- **STRIDE / assets:** Information disclosure, elevation; AS-03, AS-05, AS-15, AS-17. **Actor:** ACT-02 or ACT-09. **Entry / boundary:** remote API/CLI to local privileged process.
- **Attack path and impact:** A valid login, device token, approval or reachable network causes arbitrary command execution as root/administrator. This enables full-host compromise and access to local data.
- **Risk:** L=5, I=5, score 25 — **Critical** because impact is full-host compromise and the architecture must prevent this path.
- **Existing controls:** Current Host helper is a closed Linux effect enum invoked through Polkit; installer engine separates plans from effects; no generic root command endpoint was found. (E-08, E-14)
- **Mitigation / residual:** Maintain a closed reviewed privileged-operation inventory; reject arbitrary shell/path/executable arguments; keep the wizard unprivileged; require native OS authorization independent of Synveil auth, DeviceVeil membership or tailnet access.
- **Verification / owner / phase / status:** AT-19; platform/server-host/install-engine; P003 decision, P007–P012 and P049–P054; **BLOCKED**.

#### TM-018 — Privileged helper actor, plan or replay confusion

- **STRIDE / assets:** Spoofing, tampering, elevation; AS-15, AS-16, AS-17. **Actor:** ACT-06 or ACT-11. **Entry / boundary:** UI/client to Polkit helper and helper to root-owned files/services.
- **Attack path and impact:** Spoof invoking user, substitute a reviewed plan/effect, replay/reorder a privileged effect, or race a changed host input after approval.
- **Risk:** L=3, I=5, score 15 — **High** because a helper bug crosses into root operations.
- **Existing controls:** Linux helper requires root plus non-root Polkit actor UID; frames are bounded and timed; operation enum is closed; execution checks plan binding, effect order, sequence and host snapshot; journal is owner-bound and mode 0600. Native acceptance is platform-specific. (E-08)
- **Mitigation / residual:** Preserve independent actor and plan checks; revalidate each effect at the OS boundary; expire user confirmation; bind it to the displayed plan fingerprint; clear authorization on mismatch; test replay, cancellation and restart on each supported OS.
- **Verification / owner / phase / status:** AT-20; Linux native host and installer; P003 requirement, P007–P012/P049–P054; **PARTIAL**.

#### TM-019 — Installer or runtime artifact substitution

- **STRIDE / assets:** Tampering, elevation; AS-15, AS-16, AS-17, AS-20. **Actor:** ACT-10 or ACT-11. **Entry / boundary:** download, package manager, update metadata, privileged installation.
- **Attack path and impact:** Replace a runtime, package, install script or manifest between review and effect; install malicious code with elevated authority.
- **Risk:** L=4, I=5, score 20 — **Critical** because privileged installation can persist host compromise.
- **Existing controls:** Install plans carry artifact fingerprints and authority/privilege metadata; managed server runtime inputs have signature/keyring verification. P001 says official v0.2 signing/approval is unresolved and candidate hashes are not publisher signatures. (E-08, E-14)
- **Mitigation / residual:** Authenticate publisher identity and signed metadata, bind exact artifact digest to approved effect, verify immediately before use, reject downgrades and locally supplied artifacts for privileged production flows unless explicitly reviewed.
- **Verification / owner / phase / status:** AT-21; installer/package/deploy; P003 design, P007–P012/P055–P060; **PARTIAL**.

#### TM-020 — Interrupted privileged effect or unsafe operator recovery

- **STRIDE / assets:** Repudiation, denial of service, tampering; AS-07, AS-15, AS-16, AS-17. **Actor:** ACT-11 or process/power interruption. **Entry / boundary:** journal, package manager, service manager, configuration/storage mutation.
- **Attack path and impact:** A process dies after an effect but before its result is recorded; an operator retries blindly or removes files modified by another owner.
- **Risk:** L=3, I=5, score 15 — **High** because ambiguous recovery can corrupt configuration or destroy durable data.
- **Existing controls:** Installer journal and Host-owned file intent/hash records support reconciliation and preservation. P001 says tests do not establish power-loss, full native first-run, database rollback or end-to-end preservation. (E-08, E-14)
- **Mitigation / residual:** Persist intent before effect, verify after effect, record typed outcomes, reconcile only from authoritative observations, and stop for human review when ownership/result remains ambiguous. Never automatically restore across unrelated stores.
- **Verification / owner / phase / status:** AT-22; installer/platform/host/storage; P003 design, P007–P012/P037–P054; **PARTIAL**.

### SynveilNet and Tailscale

#### TM-021 — Endpoint, DNS or certificate substitution

- **STRIDE / assets:** Spoofing, tampering; AS-03, AS-04, AS-18. **Actor:** ACT-01, ACT-07 or ACT-11. **Entry / boundary:** discovery or configured endpoint to HTTPS server.
- **Attack path and impact:** Redirect clients to an attacker host or CA, causing credential disclosure, data interception or false host identity.
- **Risk:** L=3, I=5, score 15 — **High** because credentials and file contents transit the endpoint.
- **Existing controls:** Mobile profile policy requires HTTPS outside test-only loopback; clients disable redirects; managed host uses local TLS/CA and canonical origin; operator TLS is external. (E-09, E-12)
- **Mitigation / residual:** Verify TLS chain and hostname on every connection; bind enrollment to server installation identity; do not trust discovery responses, DNS results, endpoint labels or interface names as identity. Define certificate rotation policy after platform review.
- **Verification / owner / phase / status:** AT-23; server-network/host and all clients; P003 design, P013–P024; **PARTIAL**.

#### TM-022 — Compromised tailnet peer or privileged local Tailscale API

- **STRIDE / assets:** Information disclosure, elevation, denial; AS-03, AS-05, AS-16, AS-18, AS-19. **Actor:** ACT-07 or ACT-11. **Entry / boundary:** tailnet reachability, daemon control socket or future adapter.
- **Attack path and impact:** Treat peer membership as Synveil authorization, abuse a local control API to enroll/change routes, or move from a compromised peer to another host/resource.
- **Risk:** L=4, I=5, score 20 — **Critical** because network adjacency could become cross-host data or administrator access.
- **Existing controls:** No Tailscale integration exists; current network code classifies tunnel interfaces and excludes them from ordinary LAN candidates. (E-09)
- **Mitigation / residual:** Keep Tailscale transport-only; require TLS and application authz on every request; require explicit local OS approval for enrollment; use a documented least-privilege control interface; isolate node/host credentials and revoke them independently.
- **Verification / owner / phase / status:** AT-24; server-host/network/client; P003 decision, P013–P018; **BLOCKED**.

#### TM-023 — Public listener, plaintext downgrade or unsafe fallback

- **STRIDE / assets:** Denial of service, information disclosure; AS-08, AS-16, AS-18. **Actor:** ACT-01 or ACT-11. **Entry / boundary:** listener/firewall configuration and client reconnect.
- **Attack path and impact:** A mode switch or tunnel failure opens a public listener or retries over weaker HTTP, exposing sessions/content or bypassing intended network limits.
- **Risk:** L=3, I=5, score 15 — **High** because consequence is credential/content disclosure.
- **Existing controls:** Managed host defaults to local-only loopback; private LAN is explicit, uses port 443/TLS and a reviewed firewall plan; mobile clients reject non-HTTPS outside explicit test policy; clients do not follow redirects. Operator-managed HTTPS is outside managed edge. (E-09, E-12)
- **Mitigation / residual:** Disable public listeners by default, preserve selected mode, fail unavailable when TLS/peer identity fails, never silently downgrade or broaden reachability, and require separate operator action to change mode.
- **Verification / owner / phase / status:** AT-25; network/host/mobile/desktop; P003 design, P013–P018; **PARTIAL**.

#### TM-024 — Interface-name heuristic mistaken for network identity

- **STRIDE / assets:** Spoofing; AS-18. **Actor:** ACT-07 or local privileged actor. **Entry / boundary:** host interface discovery and reachability selection.
- **Attack path and impact:** An interface name/type is treated as proof of trusted peer identity or a secure endpoint.
- **Risk:** L=3, I=4, score 12 — **High** because an OS heuristic is not cryptographic peer or server identity.
- **Existing controls:** Interface name/type is used for candidate classification; tunnel candidates are excluded from LAN planning. No Tailscale identity verification exists. (E-09)
- **Mitigation / residual:** Keep interface names advisory only; verify endpoint certificate and authenticated host identity; obtain Tailscale identity from a supported local API and validate it against explicit policy if integration is approved.
- **Verification / owner / phase / status:** AT-26; server-host/network; P003 decision, P013–P018; **PARTIAL**.

### Local IPC and future CLI

#### TM-025 — Same-user process abuses desktop IPC or credential-bearing commands

- **STRIDE / assets:** Information disclosure, elevation; AS-03, AS-13, AS-14. **Actor:** ACT-05 or ACT-06 running as same user. **Entry / boundary:** Unix socket or Windows named pipe.
- **Attack path and impact:** Connect as the logged-in user and invoke exposed sync, sign-out, profile or enrollment commands, or observe user-controlled client state.
- **Risk:** L=3, I=4, score 12 — **High** because same-user malware can use the client's available authority.
- **Existing controls:** Bounded typed commands, per-user endpoint permissions, redacted errors and no arbitrary shell command. Unix UID or Windows owner ACL identifies the user, not a distinct process. (E-07)
- **Mitigation / residual:** Keep IPC local-only and narrowly typed; do not add host-wide, remote or root operations; minimize credential-bearing commands; use OS process isolation only where available. Treat same-user compromise as residual trust-domain compromise.
- **Verification / owner / phase / status:** AT-27; desktop IPC on Linux/Windows/macOS; P003 requirement, P043–P054; **PARTIAL**.

#### TM-026 — IPC endpoint substitution or filesystem race

- **STRIDE / assets:** Spoofing; AS-13, AS-15. **Actor:** ACT-06. **Entry / boundary:** runtime directory, socket path or named pipe name.
- **Attack path and impact:** Replace a path with a symlink/socket, race stale endpoint cleanup, or impersonate the expected named pipe to capture commands.
- **Risk:** L=2, I=5, score 10 — **Moderate** because present checks reduce exposure, but substitution could disclose commands or credentials.
- **Existing controls:** Unix code validates owner/modes, no-symlink ancestors and peer UID, removes only verified stale sockets, and uses no-follow permission updates; Windows uses owner-only DACL and rejects remote clients. (E-07)
- **Mitigation / residual:** Preserve no-follow operations, first-instance checks, peer checks, protocol handshake and platform-specific adversarial tests; fail closed on ambiguous endpoint ownership.
- **Verification / owner / phase / status:** AT-28; desktop IPC on Unix and Windows; P003 requirement, P043–P048; **PARTIAL**.

#### TM-027 — Malformed IPC frames or connection exhaustion

- **STRIDE / assets:** Denial of service, tampering; AS-13, AS-14. **Actor:** ACT-05 or ACT-06. **Entry / boundary:** local control frame parser and task scheduler.
- **Attack path and impact:** Send oversized, slow or malformed frames, exhaust connections, or crash/restart the client and interrupt sync.
- **Risk:** L=2, I=4, score 8 — **Moderate** because local access is required and frame/concurrency bounds exist.
- **Existing controls:** 64 KiB frame limit, protocol version, handshake/idle/write timeouts and a 32-connection ceiling. (E-07)
- **Mitigation / residual:** Preserve strict parsing, request-size limits, bounded task allocation, cancellation isolation and restart/recovery; test malformed and slow peers under load.
- **Verification / owner / phase / status:** AT-29; desktop IPC; P003 requirement, P043–P048; **PARTIAL**.

#### TM-028 — CLI token leakage or arbitrary command capability

- **STRIDE / assets:** Information disclosure, elevation; AS-03, AS-05, AS-08, AS-13. **Actor:** ACT-05, ACT-06 or ACT-09. **Entry / boundary:** CLI arguments, shell history, profile config, error output or remote command endpoint.
- **Attack path and impact:** Place a token in process arguments/config, leak it in errors, or expose arbitrary shell execution as a remote operation.
- **Risk:** L=4, I=4, score 16 — **High** because a future CLI can create durable token and remote-command exposure.
- **Existing controls:** No Synveil CLI exists. Existing platform secure storage abstractions and typed API v1 operations are available. (E-12, P002 ADR-P002-009)
- **Mitigation / residual:** Store secrets only in OS secure storage; bind credentials to canonical server/device identity; sanitize output; use typed allowlisted commands; require explicit destructive confirmation; never expose arbitrary shell execution.
- **Verification / owner / phase / status:** AT-30; CLI/platform/API on supported OSes; P003 requirement, P043–P048; **BLOCKED**.

### Protected Areas

#### TM-029 — Unauthorized API mutation, path traversal or symlink abuse

- **STRIDE / assets:** Tampering, elevation; AS-08, AS-09, AS-12. **Actor:** ACT-02 or ACT-06. **Entry / boundary:** API file mutation, client sync, upload path, local root path.
- **Attack path and impact:** Modify a protected resource through an unguarded route, alternate path spelling, symlink, replacement race or forged object identifier.
- **Risk:** L=3, I=5, score 15 — **High** because a successful write can overwrite or delete canonical content.
- **Existing controls:** Object keys reject traversal and filesystem-specific separators; storage checks redirected filesystem entries; API mutation routes authenticate and scope current owner. Protected Areas policy does not exist. (E-06, E-10)
- **Mitigation / residual:** Resolve stable resource identity server-side; gate every mutation path; use no-follow/descriptor-relative file operations; bind expected revision/hash; reject traversal and stale policy; audit result.
- **Verification / owner / phase / status:** AT-31; API/storage/client-sync/filesystem; P003 design, P031–P036; **PARTIAL**.

#### TM-030 — OS-privileged process bypasses app-managed protection

- **STRIDE / assets:** Tampering, elevation; AS-08, AS-09, AS-12, AS-17. **Actor:** ACT-02 with host privilege or compromised administrator. **Entry / boundary:** local filesystem/kernel path outside Synveil API.
- **Attack path and impact:** Directly edit, replace, delete or baseline-tamper a file outside app-managed checks, then present it as protected or synchronized.
- **Risk:** L=4, I=5, score 20 — **Critical** for any claim that app-managed protection blocks arbitrary local privileged writes.
- **Existing controls:** No universal OS enforcement or Protected Areas policy was found. P002 limits the initial proposal to app-managed mutation gates. (E-10, P002 ADR-P002-007)
- **Mitigation / residual:** Name the enforcement class in UI/API/docs; do not claim prevention outside Synveil-managed operations. Stronger controls need a separate per-platform native design with administrator bypass and recovery analysis.
- **Verification / owner / phase / status:** AT-32; Linux/Windows/Android/iOS native tests; P003 decision, P031–P036/P049–P054; **BLOCKED**.

#### TM-031 — Protected write race, partial update or approval removal

- **STRIDE / assets:** Tampering; AS-06, AS-08, AS-09. **Actor:** ACT-02 or ACT-11. **Entry / boundary:** policy check, approval, upload staging and commit.
- **Attack path and impact:** Change file or policy between review and commit; truncate content on interruption; remove protection or replay a decision while a write is in progress.
- **Risk:** L=3, I=5, score 15 — **High** because state may be silently corrupted or policy bypassed.
- **Existing controls:** Upload staging, lease generations, expected-version preconditions and reconciliation exist; protected-write workflow does not. (E-10)
- **Mitigation / residual:** Use transactional operation identity, policy revision, expected prior content hash and commit-time authorization; preserve old bytes until verified new content is committed; report ambiguous outcome without destroying evidence.
- **Verification / owner / phase / status:** AT-33; API/storage/metadata/client sync; P003 design, P031–P036; **BLOCKED**.

### Storage, backup and rollback

#### TM-032 — Forged object ID or cross-owner object read

- **STRIDE / assets:** Information disclosure, elevation; AS-08, AS-11, AS-12. **Actor:** ACT-02 or ACT-09. **Entry / boundary:** logical API ID to ObjectStore key.
- **Attack path and impact:** Guess/forge a node/version/object ID or exploit a metadata lookup without owner scope, then read bytes or delete another owner's content.
- **Risk:** L=3, I=5, score 15 — **High** because bytes are highly confidential and backend keys are not user authority.
- **Existing controls:** API reads carry owner scope; ObjectStore uses server-generated opaque keys; clients do not receive physical paths/credentials. (E-06, E-10)
- **Mitigation / residual:** Authorize logical identity before storage access; never accept physical keys from clients; use per-owner/domain authorization at metadata boundary and separate backend credentials by service role.
- **Verification / owner / phase / status:** AT-34; API/metadata/storage; P003 requirement, P031–P042; **PARTIAL**.

#### TM-033 — Incomplete upload or corrupted content accepted

- **STRIDE / assets:** Tampering; AS-08, AS-12. **Actor:** ACT-01, ACT-02 or ACT-09. **Entry / boundary:** upload body/chunk to staged ObjectStore object and metadata commit.
- **Attack path and impact:** Truncate, duplicate, alter or mismatch a chunk/hash/length so incorrect bytes become canonical content.
- **Risk:** L=3, I=4, score 12 — **High** because corrupted content may replace a valid version.
- **Existing controls:** Staging, length and optional SHA-256 checks, opaque handles, leases, create-only promotion and reconciliation are implemented in source. (E-10)
- **Mitigation / residual:** Keep integrity checks mandatory for protected/destructive flows; compare final stored metadata and digest; preserve the prior version until verification; reject ambiguous checksums.
- **Verification / owner / phase / status:** AT-35; storage/object-store/API; P003 regression, P031–P042; **PARTIAL**.

#### TM-034 — Garbage-collection race removes a live object

- **STRIDE / assets:** Denial of service, tampering; AS-08, AS-10, AS-12. **Actor:** ACT-02 or ACT-11 plus a concurrent writer/worker race. **Entry / boundary:** metadata reference, ObjectStore delete and GC worker.
- **Attack path and impact:** A stale GC plan races a new upload, version pin or restore plan, deleting bytes still referenced by canonical metadata.
- **Risk:** L=3, I=5, score 15 — **High** because the object may be irrecoverable after deletion.
- **Existing controls:** Durable metadata planning, leases, conditional object-version delete and reconciliation are present; there is no atomic cross-store backup guarantee. (E-10, E-11)
- **Mitigation / residual:** Retain generation/version fencing, recheck references and pins in final delete transaction, use recoverable quarantine where supported, and stop when state differs from the approved plan.
- **Verification / owner / phase / status:** AT-36; metadata/storage/worker with PostgreSQL; P003 requirement, P037–P042; **PARTIAL**.

#### TM-035 — PostgreSQL/ObjectStore backup mismatch or tampering

- **STRIDE / assets:** Tampering, denial of service; AS-10, AS-11, AS-12. **Actor:** ACT-10, ACT-11 or storage operator. **Entry / boundary:** separate database and object backup/restore flows.
- **Attack path and impact:** Restore DB without matching bytes, restore unreferenced/altered bytes, or claim a database snapshot is a full application backup.
- **Risk:** L=4, I=5, score 20 — **Critical** because recovery can silently lose or corrupt user data at scale.
- **Existing controls:** Logical snapshot manifests and content pins exist, but PostgreSQL and ObjectStore are separate authorities. P002 states no atomic combined restore protocol. (E-11)
- **Mitigation / residual:** Verify object inventory and hashes against metadata references; define coordinated quiesce/capture/restore order and recovery point; test mismatch detection; never claim atomic combined rollback without a protocol.
- **Verification / owner / phase / status:** AT-37; PostgreSQL/ObjectStore/backup operator; P003 decision, P037–P042/P055–P060; **BLOCKED** for combined-recovery claims.

#### TM-036 — Unauthorized restore, rollback or historical secret exposure

- **STRIDE / assets:** Information disclosure, tampering; AS-02, AS-03, AS-08, AS-10, AS-11, AS-12. **Actor:** ACT-02 or ACT-11. **Entry / boundary:** backup manifest, restore target, snapshot access or host rollback.
- **Attack path and impact:** Restore a snapshot to another owner/host, expose old revoked credentials/content, overwrite newer state, or use a host snapshot containing prior secrets.
- **Risk:** L=3, I=5, score 15 — **High** because historical data may retain revoked secrets or superseded private content.
- **Existing controls:** Restore plans are explicit and owner-scoped; file restore appends an immutable version; backup pins protect references. Host-level rollback and secret sanitization are operator domains. (E-06, E-11)
- **Mitigation / residual:** Reauthorize exact target and host at execution; verify manifests/content; preserve revocation tombstones; disclose restore boundary and older-secret exposure; require separate native/operator authority for host rollback.
- **Verification / owner / phase / status:** AT-38; API/metadata/storage/host; P003 requirement, P037–P042; **PARTIAL**.

### DeviceVeil

#### TM-037 — Malicious node spoofs host identity or contributes unauthorized resources

- **STRIDE / assets:** Spoofing, elevation; AS-19, AS-20. **Actor:** ACT-08. **Entry / boundary:** proposed cluster invitation, host registration and resource contribution.
- **Attack path and impact:** Enroll an unapproved host, duplicate another identity or make a node's claimed storage/CPU/network resource appear trusted.
- **Risk:** L=4, I=5, score 20 — **Critical** because false membership can become a trust bridge into multiple hosts.
- **Existing controls:** DeviceVeil integration is absent; no cross-host credential/resource contract exists. (E-02)
- **Mitigation / residual:** Require authenticated one-time issuance, host identity proof, explicit operator approval, revocation, bounded scopes and resource verification appropriate to the guarantee. Membership is not user/admin authority.
- **Verification / owner / phase / status:** AT-39; DeviceVeil adapter and real multi-host environment; P003 decision before any DeviceVeil phase; **BLOCKED**.

#### TM-038 — Cross-host credential leakage and lateral movement

- **STRIDE / assets:** Information disclosure, elevation; AS-03, AS-05, AS-16, AS-19. **Actor:** ACT-08. **Entry / boundary:** node-to-node/control-plane token or shared storage interface.
- **Attack path and impact:** A compromised node obtains a reusable cluster credential and accesses another host, account or ObjectStore outside its explicit scope.
- **Risk:** L=5, I=5, score 25 — **Critical** because compromise could spread across hosts and data authorities.
- **Existing controls:** DeviceVeil is optional and architecturally separate; Synveil remains single-host authority in the P002 proposal. No integration exists. (E-02)
- **Mitigation / residual:** Per-host non-transferable credentials, audience/host/resource scopes, mutual identity, rotation/revocation, no shared user secrets, and per-request resource authorization. A compromised participating host remains a risk.
- **Verification / owner / phase / status:** AT-40; DeviceVeil plus at least two isolated hosts; P003 decision before implementation; **BLOCKED**.

#### TM-039 — Compromised or replayed installation link/script

- **STRIDE / assets:** Tampering, elevation; AS-15, AS-16, AS-17, AS-19, AS-20. **Actor:** ACT-08 or ACT-10. **Entry / boundary:** one-time cluster link or bootstrap script to host installation.
- **Attack path and impact:** Reuse expired invitation, substitute a script/artifact or run unreviewed arbitrary commands with installation authority.
- **Risk:** L=4, I=5, score 20 — **Critical** because a script may execute privileged code on a new host.
- **Existing controls:** No DeviceVeil bootstrap link or script exists. Existing install engine has typed effects and artifact evidence. (E-08, E-14)
- **Mitigation / residual:** Require authenticated issuance, high-entropy one-time redemption, short validity, signed/integrity-verified content, explicit host approval, revocation and least-privilege installation; do not use unreviewed shell as the authority protocol.
- **Verification / owner / phase / status:** AT-41; DeviceVeil and native host installation; P003 decision before implementation; **BLOCKED**.

#### TM-040 — Quota abuse, split brain or cluster outage

- **STRIDE / assets:** Denial of service, repudiation; AS-19. **Actor:** ACT-08 or network partition. **Entry / boundary:** scheduler, membership, host removal and shared resource accounting.
- **Attack path and impact:** A node overcontributes or lies about resource use, partitioned hosts make conflicting decisions, or cluster failure makes the single-host product unavailable.
- **Risk:** L=3, I=4, score 12 — **High** because optional federation must not take down local service.
- **Existing controls:** P002 says DeviceVeil remains optional and single-host behavior remains usable when disabled or unavailable. No runtime code exists. (E-02)
- **Mitigation / residual:** Enforce per-host quotas at the resource owner; define split-brain behavior/revocation; keep local service independent and fail optional features closed during partitions.
- **Verification / owner / phase / status:** AT-42; DeviceVeil multi-host fault environment; P003 decision before implementation; **BLOCKED**.

### Supply chain and update security

#### TM-041 — Vulnerable JavaScript development dependencies compromise build or developer session

- **STRIDE / assets:** Denial of service, elevation; AS-20. **Actor:** ACT-10. **Entry / boundary:** npm lockfile, Vite dev/build, Vitest and transitive tools.
- **Attack path and impact:** Exploit a vulnerable dev server or build/test dependency to access developer/CI data, alter generated web artifacts or make CI unavailable.
- **Risk:** L=4, I=4, score 16 — **High** because build-only dependencies still execute in the developer/build trust boundary.
- **Existing controls:** P002-head web CI runs npm ci, lint, typecheck, tests and build but no npm audit step. Rechecked audit on 2026-10-25 found five advisories in dev-only dependency paths: three high and two moderate; npm reported a fix available for each. This does not prove exploitability in an official build environment, and dev-only status does not make the pipeline safe. Rust Dependency policy passed in the P002-head GitHub run. (E-15)
- **Mitigation / residual:** Review current advisory paths and fixes in a separate dependency change, keep lockfile integrity, add a maintained JS advisory gate or documented exception policy, and run the audit in CI. P003 does not upgrade dependencies.
- **Verification / owner / phase / status:** AT-43; web CI/dependency review; P003 finding, resolve before production web release; **OPEN**.

#### TM-042 — Compromised workflow action, CI identity or build provenance

- **STRIDE / assets:** Spoofing, repudiation, denial, elevation; AS-20. **Actor:** ACT-10. **Entry / boundary:** GitHub Actions workflow, runner, action reference or repository token.
- **Attack path and impact:** A mutable third-party action or overprivileged workflow changes code/artifacts, exfiltrates secrets or publishes under a trusted identity.
- **Risk:** L=3, I=5, score 15 — **High** because CI output can become trusted distribution.
- **Existing controls:** Rust CI explicitly grants contents:read; several workflows declare explicit permissions while others do not. A bounded workflow review found many actions referenced by version tags such as actions/checkout@v4; repository-default token permissions and every workflow's effective permission set were not independently qualified here. (E-15)
- **Mitigation / residual:** Set least privilege per workflow/job, pin third-party actions to reviewed immutable SHAs, restrict secrets/environments, attest build inputs/output and review untrusted pull-request execution. GitHub/runner compromise remains.
- **Verification / owner / phase / status:** AT-44; repository admin/CI; P003 recommendation, before official release signing; **PARTIAL**.

#### TM-043 — Unsigned artifact, publisher substitution or downgrade

- **STRIDE / assets:** Tampering, elevation; AS-15, AS-16, AS-20. **Actor:** ACT-10 or ACT-11. **Entry / boundary:** release manifest, installer, package channel, update metadata.
- **Attack path and impact:** Publish a hash-consistent but unauthorized artifact, substitute publisher identity or downgrade to a vulnerable build.
- **Risk:** L=4, I=5, score 20 — **Critical** because an accepted update can run code with user or host privileges.
- **Existing controls:** Candidate manifests record source/content hashes; managed server runtime inputs have separate signature checks. P001 says official v0.2 signing/approval remains unsatisfied, and P002 does not establish artifact-signing or downgrade guarantees. (E-14)
- **Mitigation / residual:** Define trusted publisher keys and signed metadata, bind signature to product/version/source, enforce monotonic version or explicit rollback authority, and rehearse key rotation/revocation. Keep development CI qualification distinct from official release approval.
- **Verification / owner / phase / status:** AT-45; release/deploy/platform; P003 decision, P049–P060; **BLOCKED** for signed-release claims.

## 9. Risk priorities and implementation blockers

1. **Critical before enabling affected feature:** TM-009 QR/server identity; TM-011 credential lifetime and lost-device policy; TM-013 owner/RBAC compatibility; TM-015 exact approval binding; TM-017 root boundary; TM-019 trusted privileged artifacts; TM-022 Tailscale identity; TM-030 Protected Areas guarantee limits; TM-035 combined backup claims; TM-037–TM-039 DeviceVeil host trust; TM-043 official update identity.
2. **High before broad exposure:** TM-003 login throttling; TM-004 bootstrap exposure; TM-005 secret redaction; TM-006 authorization matrix; TM-007 audit trail; TM-014 concurrent revocation/policy checks; TM-016 actor separation; TM-020 safe recovery; TM-021 endpoint verification; TM-023 downgrade/fallback; TM-025 same-user IPC boundary; TM-028 CLI secrets; TM-031 protected write races; TM-032–TM-036 storage/restore scope; TM-041 npm findings; TM-042 workflow permissions/provenance.
3. **No implementation should silently convert a proposed ADR or requirement into an accepted decision.** Maintainer approval is still needed for the decisions listed in [P003 security decisions](P003_SECURITY_DECISIONS.md).

## 10. Evidence ledger

These source paths were inspected at the snapshot above. Evidence is bounded to those paths and tests.

| Evidence | Source and observed fact |
|---|---|
| E-01 | [P001 audit](../audits/P001_V02_BASELINE_AUDIT.md) and [compatibility baseline](../audits/P001_COMPATIBILITY_BASELINE.md): current authentication, authorization, storage, installer, client state and qualification gaps. |
| E-02 | [P002 system architecture](../architecture/P002_SYSTEM_ARCHITECTURE.md), [compatibility contracts](../architecture/P002_COMPATIBILITY_CONTRACTS.md), and [P002 decision register](../architecture/P002_ARCHITECTURE_DECISIONS.md): current component boundaries and all 11 proposals. |
| E-03 | [Auth sessions](../../../crates/auth/src/sessions.rs), [session config](../../../crates/auth/src/config.rs), [password boundary](../../../crates/auth/src/passwords.rs), [auth service](../../../crates/auth/src/service.rs), and [PostgreSQL auth tests](../../../crates/auth/tests/postgres.rs). |
| E-04 | [API authentication](../../../crates/api/src/auth.rs), [cookies](../../../crates/api/src/cookies.rs), [CSRF](../../../crates/api/src/csrf.rs), [router](../../../crates/api/src/router.rs), [request middleware](../../../crates/api/src/middleware.rs), and API test modules. |
| E-05 | [Device authentication](../../../crates/auth/src/device_credentials.rs), [metadata repository](../../../crates/metadata/src/device_credentials.rs), [enrollment migration](../../../migrations/20260828000000_device_credentials_enrollment.sql), [API device auth](../../../crates/api/src/device_auth.rs), and [PostgreSQL device credential tests](../../../crates/auth/tests/device_credentials_postgres.rs). |
| E-06 | [API files](../../../crates/api/src/files.rs), [downloads](../../../crates/api/src/downloads.rs), [mutations](../../../crates/api/src/mutations.rs), [sync](../../../crates/api/src/sync.rs), and [metadata repository](../../../crates/metadata/src/repository.rs): route and database owner/device scoping. |
| E-07 | [Desktop control IPC](../../../crates/client/src/control.rs), [IPC integration tests](../../../crates/client/tests/desktop_control_ipc.rs), [client controller tests](../../../crates/client/tests/desktop_controller.rs), and [platform secrets](../../../crates/platform/src/native_secrets.rs). |
| E-08 | [Host helper protocol](../../../crates/server-host/src/protocol.rs), [native host helper](../../../crates/server-host/src/native.rs), [helper binary](../../../crates/server-host/src/bin/helper.rs), [Polkit action](../../../deploy/polkit/org.synveil.host.policy), [install engine model](../../../crates/install-engine/src/model.rs), and install-engine contract tests. |
| E-09 | [Server network plan](../../../crates/server-network/src/lib.rs), [Host network adapter](../../../crates/server-host/src/native/network.rs), [deployment edge artifact](../../../deploy/server/EDGE_RUNTIME_ARTIFACT.md), and [PostgreSQL loopback config](../../../deploy/server/postgresql.conf). |
| E-10 | [Object keys](../../../crates/object-store/src/keys.rs), [storage uploads](../../../crates/storage/src/uploads.rs), [local object store](../../../crates/storage/src/local.rs), [ObjectStore tests](../../../crates/object-store/src/tests.rs), and storage integration tests. |
| E-11 | [Backup API](../../../crates/api/src/backups.rs), [metadata backup/snapshot modules](../../../crates/metadata/src/backup.rs), [snapshot module](../../../crates/metadata/src/snapshot.rs), backup migrations, and [P002 recovery contract](../architecture/P002_COMPATIBILITY_CONTRACTS.md). |
| E-12 | [Android transport](../../../clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt), [Android Keystore vault](../../../clients/android/app/src/main/java/com/synveil/android/data/enrollment/CredentialVault.kt), [Keystore instrumentation tests](../../../clients/android/app/src/androidTest/java/com/synveil/android/data/enrollment/AndroidKeystoreCredentialVaultTest.kt), [iOS endpoint](../../../clients/ios/Domain/Configuration/ServerEndpoint.swift), [iOS Keychain store](../../../clients/ios/Infrastructure/Security/KeychainCredentialStore.swift), and [iOS endpoint tests](../../../clients/ios/Tests/SynveilTests/ServerEndpointTests.swift). |
| E-13 | [Web auth recovery](../../../apps/web/src/auth/authAwareApi.ts), [auth provider](../../../apps/web/src/auth/AuthProvider.tsx), [diagnostic redaction](../../../apps/web/src/diagnostics), and related web tests. |
| E-14 | [P001 release audit](../audits/P001_V02_BASELINE_AUDIT.md), [install engine](../../../crates/install-engine/src), [release metadata](../../../deploy/release), [managed server artifact docs](../../../deploy/server), and installer/deployment workflows. |
| E-15 | [Rust/web CI](../../../.github/workflows/ci.yml), [cargo-deny policy](../../../deny.toml), [web package manifest](../../../apps/web/package.json), and [web lockfile](../../../apps/web/package-lock.json); npm audit was rerun on 2026-10-25 against the checked-in lockfile. |
| E-16 | Roadmap search: Synveil-v0.3-Context-Roadmap-P001-P060.md is absent from the repository and workspace; P002 records the same gap. |

## 11. Cross-document traceability

- Normative requirements and threat mappings: [P003 security requirements](P003_SECURITY_REQUIREMENTS.md).
- Trust zones, sensitive data flows, identity and privilege flows: [P003 trust boundaries](P003_TRUST_BOUNDARIES.md).
- Security review and maintainer decisions for every P002 ADR: [P003 security decisions](P003_SECURITY_DECISIONS.md).
- Git, validation, CI/dependency evidence and P004 handoff: [P003 manifest](../manifests/PROMPT003_MANIFEST.md).

No product behavior or feature is implemented by this threat model.
