# Synveil v0.3 — P003 security requirements

**Status:** proposed requirements for maintainer review; none of these requirements claims implementation.
**Baseline:** P002 head f63374f19a81bbd4b9ef6881445a165c07ba0700; evidence snapshot 2026-10-25.
**Related:** [threat model](P003_THREAT_MODEL.md), [trust boundaries](P003_TRUST_BOUNDARIES.md), [security decisions](P003_SECURITY_DECISIONS.md), [P002 compatibility contracts](../architecture/P002_COMPATIBILITY_CONTRACTS.md).

## 1. Normative language and status

MUST is mandatory for the defined supported scope. SHOULD is recommended; a deferral needs a documented reason and owner. MAY is optional. These are design requirements, not implementation status. Every row below is **PROPOSED** until a maintainer approves the relevant decision and implementation evidence passes its acceptance test.

Requirement and acceptance-test IDs are stable traceability identifiers. A test marked planned is not evidence that the test or control exists. Phase labels refer to proposed P002 planning ranges; the authoritative P001–P060 roadmap was unavailable and must be reconciled before phase commitments.

## 2. Security invariants

| Invariant | Rationale | Enforcement owner | Required verification | Limitation |
|---|---|---|---|---|
| SEC-001 — Least privilege | A client, service, worker and OS adapter should hold only the authority needed for its operation. | Auth/API policy; metadata/storage owners; install-engine and native platform adapters. | AT-07, AT-19, AT-20, AT-27, AT-30, AT-40. | A compromised root/admin account can exceed application boundaries. |
| SEC-002 — Explicit authorization | Login, possession of an identifier, tailnet reachability or cluster membership does not grant resource access. | API route and repository owners; future policy service. | AT-07, AT-15, AT-24, AT-34, AT-39, AT-40. | Existing checks need route-by-route coverage; future roles are absent. |
| SEC-003 — Fail closed | Unknown identity, invalid scope, expiry, stale approval or unverified privileged effect cannot succeed. | Auth/API, approval service, host helper, clients. | AT-03, AT-09–AT-12, AT-16–AT-25, AT-29, AT-31, AT-35, AT-37. | Outages may reduce availability; recovery must preserve evidence. |
| SEC-004 — Durable integrity | Interrupted or ambiguous work must not silently destroy content, queues, metadata or journal evidence. | Storage, metadata, client-sync and install-engine owners. | AT-12, AT-20–AT-22, AT-33, AT-35–AT-38. | PostgreSQL and ObjectStore are separate durability domains. |
| SEC-005 — Device isolation | One stolen device credential cannot imply another device, account or host authority. | Auth/API, credential stores, future DeviceVeil adapter. | AT-07, AT-13–AT-15, AT-39–AT-40. | A compromised unlocked device can use credentials available to its process. |
| SEC-006 — Credential confidentiality | Secrets do not belong in ordinary logs, URLs, process arguments, crash reports or unprotected configuration. | All client, API, installer and deployment owners. | AT-06, AT-13, AT-30. | A compromised endpoint or OS may observe live secrets. |
| SEC-007 — Attributable security evidence | Sensitive decisions and privileged outcomes need actor, scope, time and outcome evidence with integrity controls. | API/metadata and host journal owners. | AT-08, AT-17–AT-20, AT-32, AT-38. | Retention duration and legal obligations require maintainer/legal review. |
| SEC-008 — Backward compatibility | Security evolution cannot silently expand current v0.2 owner/device authority or weaken API v1 semantics. | Auth/API, metadata, client and release owners. | AT-07, AT-15–AT-16, AT-34. | Compatibility remains bounded by tested client/server versions. |
| SEC-009 — Secure defaults | Remote exposure, privilege elevation, federation and destructive actions start disabled or restricted. | Network, installer, API and platform owners. | AT-04–AT-05, AT-19, AT-23–AT-26, AT-39–AT-42. | Operator configuration remains a deployment responsibility. |
| SEC-010 — Recoverability | Recovery preserves user data where possible and refuses unsafe automatic repair when state is ambiguous. | Storage, backup, install-engine and client-sync owners. | AT-12, AT-20–AT-22, AT-33–AT-38, AT-42. | No atomic PostgreSQL/ObjectStore or full-host rollback guarantee exists. |

## 3. Requirement register

### Identity, credentials and authorization

#### SV-SEC-001 — Browser session lifecycle

- **Requirement:** Browser sessions MUST use fresh unpredictable bearers, server-side expiry and revocation checks on every protected request; fixation, replay after revocation and expired-session use MUST fail. Session reads MUST NOT silently renew or extend authority.
- **Component / threats:** synveil-auth, API middleware and web recovery; TM-001.
- **Failure behavior:** deny the request, clear/revoke only the relevant session as appropriate, and never replay a failed mutation.
- **Compatibility:** preserve current cookie/session meanings and API v1 routes; use the existing host-only production cookie policy.
- **Acceptance / phase / status:** AT-01, AT-02; P025–P030; PROPOSED. Depends on session lifetime and renewal policy approval.

#### SV-SEC-002 — Cookie, CSRF and provenance checks

- **Requirement:** Cookie-authenticated mutations MUST validate the session-bound CSRF proof and configured Origin/proxy provenance. Production cookies MUST remain host-only, HttpOnly, Secure and SameSite=Lax. Forwarded headers MUST be trusted only from explicitly configured proxies.
- **Component / threats:** API cookies/CSRF/middleware and deployment edge; TM-002.
- **Failure behavior:** reject the mutation when proof, origin or proxy provenance is absent or invalid; do not weaken checks for cross-origin requests.
- **Compatibility:** preserve same-origin web behavior; document reverse-proxy configuration as an explicit deployment contract.
- **Acceptance / phase / status:** AT-03; P025–P030; PROPOSED. Depends on proxy-origin configuration design.

#### SV-SEC-003 — Authentication abuse and bootstrap

- **Requirement:** Public login and account-recovery surfaces SHOULD enforce bounded rate and resource controls keyed to appropriate identity and network signals. Anonymous first-admin bootstrap MUST be one-time, concurrency-safe and unavailable on managed helper/edge paths once setup closes.
- **Component / threats:** auth, API, edge and host bootstrap; TM-003, TM-004.
- **Failure behavior:** throttle or deny abuse without revealing account existence; closed bootstrap cannot be reopened by racing requests.
- **Compatibility:** retain generic credential failure responses and current single-admin first-run workflow.
- **Acceptance / phase / status:** AT-04, AT-05; P007–P012 and P025–P030; PROPOSED. Rate-limit storage, proxy identity and bootstrap-close semantics require approval.

#### SV-SEC-004 — Secret handling and diagnostic redaction

- **Requirement:** Credentials, cookies, grants, QR payloads, signed URLs and private file data MUST be excluded from ordinary logs, URLs, command arguments and crash/diagnostic output. Secret-bearing client stores MUST use supported OS-protected storage and fail explicitly where that protection is unavailable.
- **Component / threats:** API, web, desktop, mobile, platform and deployment; TM-005.
- **Failure behavior:** redact before persistence/export; unsupported secure storage must not silently fall back to plaintext.
- **Compatibility:** preserve current Android Keystore, iOS device-only Keychain and desktop credential-store behavior.
- **Acceptance / phase / status:** AT-06; all feature phases; PROPOSED. Requires review of each new diagnostic and secret path.

#### SV-SEC-005 — Server-side principal and resource authorization

- **Requirement:** Every protected API operation MUST authenticate a typed browser or device principal, authorize the requested action against current owner/device/resource scope, and perform that check server-side before returning metadata or bytes.
- **Component / threats:** API handlers, auth policy, metadata and storage; TM-006.
- **Failure behavior:** deny unknown or mismatched scope without disclosing foreign resource existence; client-side checks are never sufficient.
- **Compatibility:** preserve current API v1 owner/device checks; opaque IDs, client labels and network membership are not credentials.
- **Acceptance / phase / status:** AT-07; P025–P030; PROPOSED. Requires a route-to-action authorization inventory.

#### SV-SEC-006 — Durable audit for sensitive operations

- **Requirement:** Future approval, revocation, role change, restore, Protected Areas change and privileged-operation lifecycle MUST produce actor-attributed records for request, decision and result. Records SHOULD be append-only or integrity-protected with access controls and clock/correlation context.
- **Component / threats:** API, metadata and host journal; TM-007.
- **Failure behavior:** operations designated audit-required MUST fail closed if required intent/outcome evidence cannot be durably recorded; ordinary request tracing is not a substitute.
- **Compatibility:** do not retrofit a generic audit schema without a reviewed migration and retention/access decision.
- **Acceptance / phase / status:** AT-08; P025–P030 and P049–P054; PROPOSED. Retention duration, integrity model and privacy access remain maintainer decisions.

### Device identity and enrollment

#### SV-SEC-007 — One-time grant and invitation lifecycle

- **Requirement:** Enrollment grants and pairing invitations MUST be high-entropy, short-lived, audience/owner/device-bound and atomically single-use. They MUST never contain a reusable permanent credential.
- **Component / threats:** auth, API, metadata and client enrollment; TM-008, TM-010.
- **Failure behavior:** expired, replayed, concurrent-loser or malformed redemption fails; a lost response requires explicit recovery, not reuse of an ambiguous grant.
- **Compatibility:** retain existing digest-at-rest and one-time exchange semantics; QR is only a transport for the grant.
- **Acceptance / phase / status:** AT-09, AT-10, AT-12; P019–P024; PROPOSED. Exact invitation state machine requires approval.

#### SV-SEC-008 — QR server identity and explicit pairing confirmation

- **Requirement:** A QR pairing flow MUST verify the intended server identity through authenticated TLS and present the account, device and endpoint being paired for explicit confirmation. QR possession alone MUST NOT authorize a permanent credential.
- **Component / threats:** Android/iOS clients and API enrollment; TM-009, TM-012.
- **Failure behavior:** endpoint mismatch, certificate failure, substitution or changed pairing identity aborts enrollment without storing a credential.
- **Compatibility:** preserve existing endpoint verification and redirect rejection; do not relax TLS for remote endpoints.
- **Acceptance / phase / status:** AT-11, AT-14; P019–P024; PROPOSED. Trust bootstrap/user verification method requires maintainer approval.

#### SV-SEC-009 — Device credential lifetime, rotation and revocation

- **Requirement:** Device credentials MUST have a documented maximum lifetime or equivalent rotation policy, be independently revocable per device/credential, and stop authorizing requests after revocation. Recovery from lost or cloned devices MUST be explicit and preserve unrelated devices.
- **Component / threats:** auth, metadata, API and mobile credential vaults; TM-011.
- **Failure behavior:** expired/revoked credentials fail closed; offline clients retain pending local work but cannot claim server authorization.
- **Compatibility:** do not invalidate existing credentials without a migration/re-enrollment plan.
- **Acceptance / phase / status:** AT-13; P019–P030; PROPOSED. Current server credential expiry/rotation was not evidenced.

### Roles, approvals and revocation

#### SV-SEC-010 — Legacy owner/device compatibility mapping

- **Requirement:** RBAC MUST map existing owner/device records to no more authority than the current v0.2 behavior. Missing, ambiguous or stale mapping MUST deny; role inheritance and explicit deny precedence MUST be documented before use.
- **Component / threats:** auth policy, API and metadata; TM-013.
- **Failure behavior:** reject authorization rather than defaulting to owner/admin or trusting client-supplied role data.
- **Compatibility:** API v1 and existing owner/device records remain the compatibility oracle; additive migrations require backfill review.
- **Acceptance / phase / status:** AT-15; P025–P030; PROPOSED. Owner mapping is a maintainer decision.

#### SV-SEC-011 — Current policy revalidation and revocation

- **Requirement:** High-impact operations MUST revalidate account status, device revocation, policy revision and exact resource scope immediately before commit/effect. Concurrent role or ownership changes MUST not leave a stale authorization window.
- **Component / threats:** auth, metadata transactions, storage and host adapters; TM-014.
- **Failure behavior:** abort or retry only when operation identity and idempotency are preserved; no stale authorization fallback.
- **Compatibility:** preserve stable API v1 mutation semantics and avoid partial publication.
- **Acceptance / phase / status:** AT-16; P025–P036; PROPOSED. Transaction boundaries and policy revision model require design.

#### SV-SEC-012 — Exact approval binding and replay resistance

- **Requirement:** An approval MUST bind request ID, requester, approver, operation type, exact resource, target host, privilege, expiry, nonce/idempotency identity and all material parameters. Execution MUST compare that binding and independently reauthorize current scope.
- **Component / threats:** API, metadata and native helper; TM-015.
- **Failure behavior:** expired, reused, changed-scope or stale-policy approval denies execution; approval never acts as a bearer or OS privilege.
- **Compatibility:** existing operations remain unchanged until an explicit policy requires approval.
- **Acceptance / phase / status:** AT-17; P025–P054; PROPOSED. Approval state machine and durable schema require approval.

#### SV-SEC-013 — Separation of duties and approval audit

- **Requirement:** High-impact approval SHOULD prohibit requester self-approval; any exception MUST be explicit, scoped and attributable. Approver role changes and both successful and failed execution outcomes MUST be auditable.
- **Component / threats:** auth policy, API, metadata and audit; TM-016.
- **Failure behavior:** where separation is required, same-actor or unverifiable approver identity denies; missing required audit record blocks operation.
- **Compatibility:** do not impose approval on legacy operations without an approved policy migration.
- **Acceptance / phase / status:** AT-18; P025–P054; PROPOSED. Maintainer must set operation classes, separation rule and retention.

### Privileged host operations

#### SV-SEC-014 — Closed privileged effect inventory

- **Requirement:** Privileged work MUST use a closed typed effect allowlist. A Synveil session/token, DeviceVeil membership, Tailscale peer or approval MUST NOT expose arbitrary shell, generic root RPC or implicit administrator authority.
- **Component / threats:** install-engine, server-host and native platform adapters; TM-017.
- **Failure behavior:** unknown operation or unsupported platform is rejected; no generic command fallback.
- **Compatibility:** preserve current helper’s bounded effect protocol and unprivileged wizard presentation.
- **Acceptance / phase / status:** AT-19; P007–P012 and P049–P054; PROPOSED. Exact supported effects and OS owners require review.

#### SV-SEC-015 — Native authorization and helper request binding

- **Requirement:** Each privileged effect MUST obtain platform-native authorization for its exact actor and operation, validate target and arguments, bind execution to a fresh reviewed plan/fingerprint, and reject replay or timeout ambiguity.
- **Component / threats:** native helper, install-engine and platform package operations; TM-018.
- **Failure behavior:** changed plan, actor mismatch, stale nonce, malformed frame or denied native authorization stops before effect and requires inspection.
- **Compatibility:** preserve Linux Polkit actor checks, bounded frames, effect sequence and Windows per-user scope.
- **Acceptance / phase / status:** AT-20; P007–P012 and P049–P054; PROPOSED. Native behavior must be tested per supported OS.

#### SV-SEC-016 — Artifact identity before privileged install

- **Requirement:** Privileged installation MUST verify artifact identity, expected digest/signature and provenance against a trusted policy before execution. A runtime manifest hash alone MUST NOT be represented as publisher authentication.
- **Component / threats:** installer, deployment, package and release pipeline; TM-019.
- **Failure behavior:** unknown, altered, revoked or unverifiable artifact stops installation and preserves existing data.
- **Compatibility:** retain native package-manager authenticity checks while documenting their trust source; do not equate development CI with official signing.
- **Acceptance / phase / status:** AT-21; P007–P012 and P055–P060; PROPOSED. Trusted publisher and signing model require decision.

#### SV-SEC-017 — Journaled interruption and safe recovery

- **Requirement:** Privileged multi-step changes MUST record intent, effect identity, verified result and recoverability before proceeding to a dependent step. Ambiguous partial effects MUST require inspection instead of blind retry or destructive rollback.
- **Component / threats:** install-engine, host journal, storage and platform adapters; TM-020.
- **Failure behavior:** cancellation/timeouts leave durable evidence and preserve user/server data; recovery is idempotent or explicitly operator-mediated.
- **Compatibility:** preserve append-only/versioned installer journals and existing data-preserving removal semantics.
- **Acceptance / phase / status:** AT-22; P007–P012 and P037–P054; PROPOSED. Per-effect compensation and recovery guarantees require review.

### Network, IPC and local command surfaces

#### SV-SEC-018 — Endpoint identity and transport verification

- **Requirement:** Remote clients MUST authenticate the server with TLS hostname/certificate verification. Tailscale MAY transport packets but MUST NOT replace Synveil authentication, authorization or server identity verification.
- **Component / threats:** server-network, host adapters and all clients; TM-021, TM-022.
- **Failure behavior:** identity failure, revoked peer or unavailable tunnel denies the connection; do not bypass TLS or silently trust a tailnet name.
- **Compatibility:** preserve operator-managed HTTPS and existing LAN behavior; explicit local debug loopback exceptions remain development-only.
- **Acceptance / phase / status:** AT-23, AT-24; P013–P024; PROPOSED. Tailscale control API and identity mapping are unimplemented.

#### SV-SEC-019 — Secure reachability and no downgrade

- **Requirement:** Managed services MUST default to local-only exposure. LAN, public-edge and tunnel reachability require explicit validated configuration. Failure MUST NOT trigger public exposure, HTTP downgrade or weaker automatic fallback.
- **Component / threats:** network planner, host firewall and client endpoint selection; TM-023, TM-024.
- **Failure behavior:** invalid plan or interface classification remains unavailable pending operator action.
- **Compatibility:** preserve current loopback managed backend, explicit private-LAN plan and operator TLS edge.
- **Acceptance / phase / status:** AT-25, AT-26; P013–P018; PROPOSED. Interface labels are hints, not identity.

#### SV-SEC-020 — Desktop IPC boundary

- **Requirement:** Local IPC MUST enforce OS peer/ACL and endpoint ownership checks, a versioned typed command allowlist, bounded frame/concurrency/time limits and sanitized errors. Same-user process access MUST be treated as shared-user authority.
- **Component / threats:** desktop client IPC; TM-025–TM-027.
- **Failure behavior:** reject wrong owner, remote peer, symlink/substituted endpoint, oversized frame and unknown command without disclosing credentials.
- **Compatibility:** preserve protocol v1 behavior and current Unix peer UID/Windows owner-only DACL controls.
- **Acceptance / phase / status:** AT-27–AT-29; P043–P048; PROPOSED. Cross-platform native testing is required.

#### SV-SEC-021 — Future CLI secret and command safety

- **Requirement:** A future CLI MUST store credentials in supported OS secure storage, bind profiles to verified host identity, use explicit typed API operations and require confirmation for destructive/approval-controlled changes. It MUST NOT expose arbitrary remote shell.
- **Component / threats:** proposed CLI, platform secret store and API v1; TM-028.
- **Failure behavior:** unsupported vault, host mismatch or missing confirmation prevents mutation; errors redact token and path secrets.
- **Compatibility:** CLI uses HTTPS API v1 and stable mutation semantics; mount-specific behavior remains a separate adapter.
- **Acceptance / phase / status:** AT-30; P043–P048; PROPOSED. CLI does not exist.

### Protected Areas and storage

#### SV-SEC-022 — Canonical API path and mutation checks

- **Requirement:** All supported write paths MUST authorize owner/resource before mutation and validate canonical object/path identity against traversal, separator, symlink and time-of-check/time-of-use substitution.
- **Component / threats:** API, storage, object-store and client-sync; TM-029.
- **Failure behavior:** invalid path or stale object identity rejects without overwriting a prior valid version.
- **Compatibility:** preserve API v1 logical IDs, immutable version semantics and current safe object-key rules.
- **Acceptance / phase / status:** AT-31; P031–P036; PROPOSED. Filesystem-specific acceptance must match supported backend.

#### SV-SEC-023 — State Protected Areas guarantee accurately

- **Requirement:** Application-managed Protected Areas MUST block unauthorized Synveil API/client mutations and identify the enforcement coverage. Any OS-enforced claim MUST name the platform mechanism, threat model and independently tested bypass boundary.
- **Component / threats:** API/storage/client and future per-OS enforcement; TM-030, TM-031.
- **Failure behavior:** stale baseline, missing approval/audit or partial-write state denies mutation and preserves prior content; app-managed mode must not claim to stop arbitrary privileged local writes.
- **Compatibility:** preserve file version/conflict semantics; no kernel module or driver is implied.
- **Acceptance / phase / status:** AT-32, AT-33; P031–P036 and P049–P054; PROPOSED. Platform support scope requires decision and native test evidence.

#### SV-SEC-024 — Object authorization and upload integrity

- **Requirement:** Object identifiers MUST be resolved through authorized metadata, never treated as backend capability. Upload publication MUST require verified length/hash, valid lease/generation and create-only promotion.
- **Component / threats:** API, metadata, storage and object-store; TM-032, TM-033.
- **Failure behavior:** cross-owner IDs, mismatched digest/length, stale lease or incomplete upload do not publish bytes or replace a valid object.
- **Compatibility:** preserve PostgreSQL ownership and opaque ObjectStore keys as separate authorities.
- **Acceptance / phase / status:** AT-34, AT-35; P031–P042; PROPOSED. Existing verification requires regression coverage.

#### SV-SEC-025 — Safe garbage collection and backup/recovery scope

- **Requirement:** Garbage collection MUST recheck live metadata references and pins at deletion time. Backup and restore MUST identify PostgreSQL metadata, ObjectStore bytes, client state and host snapshots as separate recovery domains and verify referenced content before restore.
- **Component / threats:** metadata, storage, backup API and operator recovery; TM-034–TM-036.
- **Failure behavior:** mismatch, missing bytes or stale reference aborts deletion/restore and quarantines evidence; database-only snapshots are not complete content backups.
- **Compatibility:** no claim of atomic cross-store rollback without a reviewed consistency protocol; restore remains owner/host scoped.
- **Acceptance / phase / status:** AT-36–AT-38; P037–P042 and P055–P060; PROPOSED. Combined recovery design is a blocker.

### DeviceVeil and software delivery

#### SV-SEC-026 — DeviceVeil host identity and isolation

- **Requirement:** Optional DeviceVeil integration MUST use verified per-host identity, scoped/revocable credentials and explicit resource authorization. Cluster membership MUST NOT grant Synveil user, storage or administrator authority.
- **Component / threats:** proposed DeviceVeil adapter and Synveil API/host; TM-037, TM-038.
- **Failure behavior:** unverified identity or cross-host scope denies; a compromised node is contained to its host-specific grants.
- **Compatibility:** single-host Synveil remains usable when DeviceVeil is disabled or unavailable.
- **Acceptance / phase / status:** AT-39, AT-40; before any DeviceVeil implementation; PROPOSED. Cross-project trust contract is absent.

#### SV-SEC-027 — One-time, verified DeviceVeil bootstrap and failure isolation

- **Requirement:** Any future install link/script MUST be authenticated, unpredictable, short-lived, single-use, revocable and bound to a verified host identity; install content MUST be signed or integrity-verified and installation least-privilege. Cluster resource use MUST be quota-bounded.
- **Component / threats:** proposed DeviceVeil bootstrap, host install and scheduler; TM-039, TM-040.
- **Failure behavior:** expired/replayed bootstrap, quota exhaustion, split-brain or cluster outage disables optional cluster work without disabling supported local service.
- **Compatibility:** no unreviewed arbitrary script execution; no mandatory federation dependency.
- **Acceptance / phase / status:** AT-41, AT-42; before DeviceVeil implementation; PROPOSED. Invitation, quota and partition policies require decision.

#### SV-SEC-028 — Dependency advisory handling

- **Requirement:** Rust and JavaScript dependencies MUST be checked against maintained advisory policy with lockfile integrity. High findings in executable developer/build dependencies MUST have an owner, remediation or time-bounded documented exception before release qualification.
- **Component / threats:** Rust/web dependency CI and developers; TM-041.
- **Failure behavior:** unresolved blocking advisory prevents the affected release qualification; a dev-only label is not an automatic exception.
- **Compatibility:** P003 records five current npm development-dependency findings and does not upgrade them; cargo-deny success is separately evidenced by P002-head CI.
- **Acceptance / phase / status:** AT-43; P055–P060 and before production web release; PROPOSED. Exact severity exception policy requires owner.

#### SV-SEC-029 — CI identity and build provenance

- **Requirement:** CI MUST grant least necessary workflow/job permissions, pin third-party actions to reviewed immutable revisions where feasible, isolate secrets from untrusted code and record source/dependency/build provenance for promoted artifacts.
- **Component / threats:** repository workflows, build runners and release pipeline; TM-042.
- **Failure behavior:** provenance or permission policy failure blocks artifact promotion; do not trust a successful build status as a publisher signature.
- **Compatibility:** preserve current checks while tightening workflow policy through a separately reviewed change.
- **Acceptance / phase / status:** AT-44; before official release signing; PROPOSED. Runner and repository administration remain trust assumptions.

#### SV-SEC-030 — Publisher authentication and downgrade resistance

- **Requirement:** Official update/install metadata MUST authenticate publisher identity and bind signatures to product, version and artifact; clients MUST reject unauthorized downgrade except through explicit recovery authority.
- **Component / threats:** release, installer and update clients; TM-043.
- **Failure behavior:** invalid signature, revoked key or disallowed downgrade stops installation and preserves existing data.
- **Compatibility:** distinguish development CI artifacts and content hashes from official release signing/approval.
- **Acceptance / phase / status:** AT-45; P049–P060; PROPOSED. Signing keys, rotation and rollback authority require maintainer decision.

## 4. Threat-to-requirement coverage

| Threat set | Requirements | Verification |
|---|---|---|
| TM-001–TM-007: browser, abuse, bootstrap, secrets, authorization and audit | SV-SEC-001–006 | AT-01–AT-08 |
| TM-008–TM-012: enrollment, QR, device lifetime and endpoint binding | SV-SEC-007–009 | AT-09–AT-14 |
| TM-013–TM-016: RBAC, stale policy and approval | SV-SEC-010–013 | AT-15–AT-18 |
| TM-017–TM-020: privilege, helper, artifact and interrupted effects | SV-SEC-014–017 | AT-19–AT-22 |
| TM-021–TM-024: endpoint, tailnet, downgrade and interface identity | SV-SEC-018–019 | AT-23–AT-26 |
| TM-025–TM-028: IPC and CLI | SV-SEC-020–021 | AT-27–AT-30 |
| TM-029–TM-036: path, Protected Areas, storage, GC and recovery | SV-SEC-022–025 | AT-31–AT-38 |
| TM-037–TM-040: DeviceVeil | SV-SEC-026–027 | AT-39–AT-42 |
| TM-041–TM-043: supply chain and updates | SV-SEC-028–030 | AT-43–AT-45 |

All 43 threat IDs are referenced above. All risks ranked High or Critical have at least one proposed mitigation and one planned acceptance test in the linked threat register and matrix. No test below is reported as run.

## 5. Security acceptance matrix

These tests are future acceptance criteria, not current test results. Unit and integration tests establish application behavior only. Cross-platform and native acceptance must run on actual supported OS versions; simulated tests do not prove OS enforcement.

| ID | Type | Component / environment | Expected result | Phase | Evidence required |
|---|---|---|---|---|---|
| AT-01 | Integration | Auth/API; PostgreSQL; browser cookie client | New login gets fresh session; expired, replayed-after-revoke and fixed session are denied. | P025–P030 | Request traces plus DB session/revocation assertions. |
| AT-02 | Integration | Web/API recovery; forced 401/timeout | Safe session read may recover; a failed mutation is never replayed. | P025–P030 | Web test log and server mutation count. |
| AT-03 | Integration | API behind configured proxy and direct test origin | Valid same-origin CSRF succeeds; missing proof, hostile Origin and untrusted forwarded header fail. | P025–P030 | HTTP cases and proxy configuration fixture. |
| AT-04 | Load/security integration | Auth edge; bounded test environment | Login abuse is throttled with generic responses and bounded resource use. | P025–P030 | Rate-limit counters, response samples and load bounds. |
| AT-05 | Concurrency integration | Bootstrap API and managed deployment | Concurrent first-admin claim has one winner; closed managed bootstrap cannot reopen. | P007–P012 | DB uniqueness/lock evidence and deployment route result. |
| AT-06 | Unit + client integration | API/web/desktop/mobile diagnostics | Secret canaries are absent from logs, exported diagnostics, URLs and error output. | Each feature phase | Captured sanitized output and redaction test results. |
| AT-07 | Authorization integration | API/metadata with two owners and multiple devices | Positive own-scope read/write works; cross-user/device/host reads and writes fail. | P025–P030 | Route matrix with principal/resource combinations. |
| AT-08 | Integration | API/metadata/host journal | Sensitive actions record actor, scope, decision and outcome; required audit failure blocks action. | P025–P030; P049–P054 | Durable record assertions and tamper/access-control check. |
| AT-09 | Auth integration | Enrollment API/metadata | Grant has high entropy, owner/device binding, bounded expiry and digest-only persistence. | P019–P024 | Entropy/state/schema assertions without logging raw grant. |
| AT-10 | Concurrency integration | Enrollment API; concurrent clients | Exactly one redemption succeeds; duplicate/replayed grant is denied. | P019–P024 | Concurrent request result and one credential record. |
| AT-11 | Cross-platform native | Android/iOS camera and TLS endpoint fixtures | Substituted QR host, invalid certificate or changed account/device is rejected before credential storage. | P019–P024 | Physical-device test record, endpoint identity and vault state. |
| AT-12 | Fault-injection integration | Enrollment API and mobile client | Lost exchange response cannot reuse grant; recovery preserves unrelated queue and requires explicit re-enrollment. | P019–P024 | Fault trace, grant state and local queue comparison. |
| AT-13 | Cross-platform native | Android Keystore, iOS Keychain, desktop vault; online/offline | Expired/revoked credential stops server use; lost-device recovery revokes only selected scope. | P019–P030 | Device matrix, server revocation result and secure-store evidence. |
| AT-14 | Cross-platform integration | Android/iOS/desktop endpoint profiles | Credential cannot be sent to substituted host; TLS and redirect failures reject. | P019–P030 | Endpoint fixtures, network capture and vault-state result. |
| AT-15 | Migration + authorization integration | Legacy v0.2 fixtures and proposed RBAC schema | Existing allowed actions retain equivalent rights; no legacy user/device gains broader role. | P025–P030 | Before/after permission matrix and migration review. |
| AT-16 | Concurrency integration | API/metadata transaction tests | Revocation or role change racing with mutation prevents stale authorization commit. | P025–P036 | Transaction schedule, final state and audit outcome. |
| AT-17 | Integration + replay | Approval API/metadata and privileged adapter | Altered resource, host, parameter, nonce, expiry or policy revision invalidates approval. | P025–P054 | Signed/canonical operation fingerprint and denied replay log. |
| AT-18 | Policy integration | Approval workflow with multiple principals | Self-approval is denied where policy requires; approver changes and outcomes are attributable. | P025–P054 | Actor matrix and durable audit records. |
| AT-19 | Security review + negative integration | All privileged adapters; remote token and DeviceVeil mocks | Remote auth, tailnet membership and cluster membership cannot call shell or arbitrary root effect. | P007–P012; P049–P054 | Closed effect registry, denied command cases and review record. |
| AT-20 | Native cross-platform | Linux Polkit helper, Windows per-user installer, supported macOS path | Wrong actor, stale plan, repeated nonce, malformed frame or native denial causes no effect. | P007–P012; P049–P054 | Per-OS native logs, effect journal and unchanged-state proof. |
| AT-21 | Integration + supply-chain | Installer/package pipeline with altered artifact | Wrong digest, signature, publisher or revoked artifact is rejected before privileged execution. | P007–P012; P055–P060 | Artifact provenance, signature verification and no-effect assertion. |
| AT-22 | Fault-injection native | Install engine/helper with interruption at each effect boundary | Recovery is inspectable/idempotent or stops safely; user/server data and journal remain. | P007–P012; P037–P054 | Fault point matrix, journal and preserved-data hashes. |
| AT-23 | Cross-platform network | Android/iOS/desktop against invalid TLS, DNS and endpoint fixtures | Hostname/certificate mismatch and redirect are rejected; no credential is sent. | P013–P024 | Client logs, packet capture and TLS fixture result. |
| AT-24 | Network integration | Two tailnet peers plus Synveil auth; partition/revoke peer | Reachable compromised peer still lacks user/resource authority; peer loss does not bypass TLS. | P013–P018 | ACL fixture, denied API calls and network partition trace. |
| AT-25 | Native deployment | Managed loopback, explicit LAN and public-edge configurations | Default remains loopback; invalid plan cannot expose port or downgrade to HTTP. | P013–P018 | Listener/firewall inspection for each deployment mode. |
| AT-26 | Cross-platform native | Linux/Windows network interface fixtures | Interface names or tunnel flags alone never establish server or peer identity. | P013–P018 | Interface matrix and resulting plan assertions. |
| AT-27 | IPC integration | Linux/Windows desktop UI and client process | Same-user commands remain allowlisted and cannot expand remote/server scope or reveal credentials. | P043–P048 | Command matrix and credential-canary output. |
| AT-28 | Native IPC | Unix socket and Windows named pipe | Symlink/path/owner/ACL/remote-client substitution is rejected. | P043–P048 | Filesystem/ACL state and rejection result per OS. |
| AT-29 | Fuzz + integration | IPC parser with malformed, oversized, slow and excess frames | Parser rejects safely; bounds/timeouts release resources without crash or secret leakage. | P043–P048 | Fuzz corpus, resource bounds and sanitizer logs. |
| AT-30 | Cross-platform CLI | Future CLI on supported OSes; mock API | Credentials use OS vault; host mismatch, destructive action without confirm and shell request fail. | P043–P048 | Vault inspection, API request capture and negative command cases. |
| AT-31 | Storage integration + filesystem | API/object store with traversal, separator and symlink fixtures | Invalid/noncanonical paths and stale target races cannot replace existing content. | P031–P036 | Path matrix and before/after hashes. |
| AT-32 | Native acceptance | Linux, Windows, Android and iOS Protected Areas | App-managed mode blocks supported API writes; claims about OS enforcement pass only the separately named native mechanism tests. | P031–P036; P049–P054 | Per-OS threat/scope statement and actual-device bypass evidence. |
| AT-33 | Concurrency + fault injection | Protected mutation, approval, baseline and object promotion | Revoked approval, baseline race or partial write denies safely and preserves prior valid version. | P031–P036 | Interleaving traces and version/hash comparison. |
| AT-34 | Authorization integration | API metadata/object store with forged/foreign IDs | Foreign object identifier cannot retrieve bytes or reveal existence; own authorized ID works. | P031–P042 | Owner matrix and storage access log. |
| AT-35 | Storage fault integration | Interrupted upload, wrong length/hash and stale lease | Incomplete/corrupt object is not published; valid existing object remains readable. | P031–P042 | Hash/length assertions and reconciliation state. |
| AT-36 | Concurrency integration | GC worker, PostgreSQL references and ObjectStore | Object referenced or pinned during deletion check is retained; orphan cleanup is bounded. | P037–P042 | Concurrent schedule, references and object inventory. |
| AT-37 | Backup/restore integration | PostgreSQL plus ObjectStore snapshots with missing/tampered object | Restore verifies both domains and rejects incomplete/mismatched backup before destructive action. | P037–P042; P055–P060 | Manifest, object hashes and no-destructive-change proof. |
| AT-38 | Authorization + recovery integration | Restore to another owner/host; historical credential fixture | Unauthorized target restore is denied; superseded secrets are invalidated or explicitly surfaced. | P037–P042 | Actor/resource matrix and secret-version evidence. |
| AT-39 | Multi-host security integration | DeviceVeil mock plus two isolated hosts | Unapproved or spoofed host identity/resource contribution is rejected. | Before DeviceVeil implementation | Host identity proofs, enrollment record and denied contribution. |
| AT-40 | Cross-host penetration test | Two isolated hosts with per-host credentials | Compromise of one host cannot use its credential for the other host/account/ObjectStore. | Before DeviceVeil implementation | Credential audience tests, network capture and cross-host denial. |
| AT-41 | Native supply-chain acceptance | DeviceVeil bootstrap on disposable hosts | Expired/replayed link, altered script, wrong signature or unapproved host has no install effect. | Before DeviceVeil implementation | One-time state, signature result, approval and host journal. |
| AT-42 | Fault injection / availability | DeviceVeil multi-host partition, quota and split-brain | Per-host quotas apply; cluster outage disables optional work while single-host Synveil remains usable. | Before DeviceVeil implementation | Partition trace, quota record and local service probe. |
| AT-43 | Dependency policy CI | Rust and npm lockfiles on CI | Advisory scan is current; high build/dev findings are triaged with owner/remediation or explicit exception. | P055–P060; before production web release | cargo-deny/npm audit reports and exception record. |
| AT-44 | CI security review | GitHub Actions on trusted/untrusted PR contexts | Minimal permissions, reviewed immutable action references and provenance checks prevent untrusted promotion. | Before official release signing | Effective permission report, action SHA inventory and provenance attestation. |
| AT-45 | Release/native acceptance | Installer and updater with invalid, revoked, old and valid artifacts | Invalid publisher/signature and unauthorized downgrade are rejected; valid signed version installs without data loss. | P049–P060 | Key policy, signed metadata, downgrade case and preserved-data evidence. |

## 6. Cross-platform limits and later-phase dependencies

- Linux helper behavior is backed by current Polkit and typed-effect source paths, but new privilege claims require native tests on supported distributions.
- Windows current-user installer scope is not proof of administrator-level or filesystem enforcement. Named-pipe ACL and any future privileged adapter require Windows-native acceptance.
- Android Keystore and app sandbox claims are bounded by the Android OS and app process state; rooted-device behavior is outside application guarantees.
- iOS Keychain and app sandbox claims do not imply arbitrary filesystem protection; File Provider behavior needs real-device acceptance.
- macOS desktop IPC is proposed coverage only where it appears in TM-025; it has no universal claim in the current P002 support matrix.
- DeviceVeil multi-host isolation, cluster quota and bootstrap tests need separate isolated hosts and an approved cross-project trust contract.
- API/metadata authorization, ObjectStore integrity and backup tests need disposable PostgreSQL/ObjectStore fixtures. Production restore must not be used as a test environment.
- Roadmap phase labels are provisional until the missing authoritative roadmap is supplied or maintainer confirms P002 phase mapping.

## 7. Deferred decisions and implementation gates

The linked [security decision register](P003_SECURITY_DECISIONS.md) keeps all P002 ADRs **PROPOSED** and identifies maintainer decisions. At minimum, implementation is blocked for the affected feature until there is an approved decision for: QR/server trust bootstrap; device credential rotation; legacy owner-to-role mapping; approval scope, separation and retention; exact privileged effect authority; Tailscale endpoint identity and fallback; Protected Areas enforcement claim; PostgreSQL/ObjectStore recovery consistency; DeviceVeil host trust; and publisher signing/downgrade policy.

No endpoint, schema, production behavior, dependency, workflow or native platform guarantee is implemented by this requirements document.
