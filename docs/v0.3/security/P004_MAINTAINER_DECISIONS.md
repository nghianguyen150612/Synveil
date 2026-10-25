# Synveil v0.3 — P004 Maintainer Decision Packet

**Status:** recommendations for independent maintainer decisions; no proposal is accepted by P004.
**Authority rule:** merging P002/P003 documentation records does not approve their ADRs. P002 lists all eleven ADRs as PROPOSED and P003 explicitly preserves that status. Only an explicit maintainer decision recorded against an item below may change status.
**Evidence baseline:** `develop/v0.3` at `6696361a3a39d25f2606bb48356b2bf6b2c89a0c`; P001/P002/P003 and relevant source paths are listed in [P004 manifest](../manifests/PROMPT004_MANIFEST.md). Roadmap file unavailable; phase ranges are provisional.

## Decision summary

| Decision ID | P002 ADR | Area | Recommended default | Status |
|---|---|---|---|---|
| P004-D01 | ADR-P002-001 | Crate/process ownership | Extend current owner; add boundary only for demonstrated lifecycle/privilege need. | RECOMMENDED_PENDING_APPROVAL |
| P004-D02 | ADR-P002-002 | Installer privilege | Unprivileged presentation, closed typed effects, existing journal and narrow native adapters. | RECOMMENDED_PENDING_APPROVAL |
| P004-D03 | ADR-P002-003 | Tailscale | Optional transport only; local-only default; TLS and app policy unchanged. | RECOMMENDED_PENDING_APPROVAL |
| P004-D04 | ADR-P002-004 | API evolution | Preserve API v1 meanings; capability discovery is descriptive, never authority. | RECOMMENDED_PENDING_APPROVAL |
| P004-D05 | ADR-P002-005 | Owner-to-role mapping | Effective legacy permission is intersection of legacy result and new policy during migration. | RECOMMENDED_PENDING_APPROVAL |
| P004-D06 | ADR-P002-006 | Approval and audit | Exact request binding; independent approval where classified; bounded single-use execution authorization. | RECOMMENDED_PENDING_APPROVAL |
| P004-D07 | ADR-P002-007 | Protected Areas | Application-managed mutation gate first; no universal filesystem blocking claim. | RECOMMENDED_PENDING_APPROVAL |
| P004-D08 | ADR-P002-008 | Recovery | Name recovery domains separately; no cross-store atomicity claim without evidence. | RECOMMENDED_PENDING_APPROVAL |
| P004-D09 | ADR-P002-009 | CLI/mount | Typed API v1 client; OS-vault credentials; mount isolated from logical file API. | RECOMMENDED_PENDING_APPROVAL |
| P004-D10 | ADR-P002-010 | DeviceVeil | Optional, per-host identity/audience/revocation and quotas; single-host remains independent. | RECOMMENDED_PENDING_APPROVAL |
| P004-D11 | ADR-P002-011 | Installer presentation | Reuse desktop/platform surface for presentation; install-engine owns plan/effects/recovery. | RECOMMENDED_PENDING_APPROVAL |

## P004-D01 — Component and process ownership

1. **Decision ID:** P004-D01 / ADR-P002-001.
2. **Existing evidence:** P001/P002 found 18 established Rust crates and distinct auth, API, metadata, storage, host/platform and client boundaries; no current RBAC, approval, Tailscale, Protected Areas, CLI mount or DeviceVeil implementation. Existing API/service boundaries are where current owner/device checks occur.
3. **Threats and requirements:** TM-006, TM-017, TM-025, TM-032; SV-SEC-001, SV-SEC-005, SV-SEC-014, SV-SEC-020.
4. **Feasible alternatives:** (a) Extend existing owning crates and create a new crate/process only for an independent lifecycle, build target or privilege boundary. (b) Create a crate per feature name. (c) Concentrate all new behavior in API/core.
5. **Recommended default:** (a); policy enforcement remains at API/service boundaries and storage keys/native effects stay behind owning modules.
6. **Compatibility consequences:** No wire, persistent-state or process change by this choice; API v1 and current repository ownership remain.
7. **Security trade-offs:** (a) least new attack surface but requires careful module visibility; (b) names features but adds dependency edges and may not isolate authority; (c) centralizes but can bypass boundaries and create a confused deputy.
8. **Implementation consequences:** Each feature documents data, authority and dependency direction; no UI-to-DB/ObjectStore-key or remote-to-root path.
9. **Required verification:** AT-07, AT-19, AT-27; dependency graph/static review and route/adapter review.
10. **Exact maintainer question:** “Approve extending existing crate owners by default, with a new crate/process only when an independent lifecycle or privilege boundary is justified?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D02 — Installer process and native privilege

1. **Decision ID:** P004-D02 / ADR-P002-002.
2. **Existing evidence:** `crates/install-engine` models plans, effects, authority, privilege, verification, append-only journal and recovery. P003 reviewed the Linux bounded host helper/Polkit path; Windows Setup is current-user scoped. No generic privileged wizard should be inferred.
3. **Threats and requirements:** TM-017–TM-020, TM-039, TM-043; SV-SEC-014–SV-SEC-017, SV-SEC-030.
4. **Feasible alternatives:** (a) Unprivileged wizard plus typed confirmed plan and narrow native/package adapters. (b) Run the whole wizard elevated. (c) Add persistent privileged service for all effects.
5. **Recommended default:** (a), preserve the existing transaction journal, require OS authorization per effect and reject unsupported operations. Step-up authentication may supplement but cannot replace native OS authorization.
6. **Compatibility consequences:** Preserves Linux package manager/AppImage distinctions and Windows current-user install; no new format or effect is silently added.
7. **Security trade-offs:** (a) more adapter design, smaller privilege boundary; (b) simpler UI but all presentation input becomes privileged; (c) broad long-lived reachable authority. FIDO2/passkey feasibility and per-platform native prompt integration are unverified.
8. **Implementation consequences:** Before installer work, approve closed effect catalog, per-OS actor/elevation rules, artifact publisher trust and cancellation/repair behavior.
9. **Required verification:** AT-19–AT-22, AT-41, AT-45; Linux Polkit and package-manager tests; separate Windows UAC/SCM tests; fault-injected journal recovery.
10. **Exact maintainer question:** “Approve an unprivileged installer presentation with only closed, plan-bound effects and native per-effect OS authorization; which exact effects and platforms may elevate?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D03 — Optional Tailscale transport

1. **Decision ID:** P004-D03 / ADR-P002-003.
2. **Existing evidence:** `crates/server-network` has side-effect-free reachability planning and classifies tunnels separately; P001 says no Tailscale integration exists. Android/iOS require verified HTTPS outside debug loopback. No peer is application authority.
3. **Threats and requirements:** TM-021–TM-024; SV-SEC-018, SV-SEC-019.
4. **Feasible alternatives:** (a) Optional adapter with normal TLS and Synveil auth. (b) Treat tailnet peer identity as sufficient user/resource trust. (c) Build a Synveil VPN/relay.
5. **Recommended default:** (a), with local-only default, explicit opt-in, persistent server identity binding and no fallback on tunnel loss.
6. **Compatibility consequences:** Existing local/LAN and operator HTTPS semantics remain; no implicit listener expansion.
7. **Security trade-offs:** (a) requires independent endpoint trust; (b) confuses reachability with authorization and allows compromised peers; (c) significantly expands protocol and operations trust.
8. **Implementation consequences:** Endpoint identity, control API authority, local approval, peer revocation and failure UX must be decided before integration. Control API/platform behavior is unverified.
9. **Required verification:** AT-23–AT-26 and real-network outage/revocation/certificate mismatch evidence across supported OSes.
10. **Exact maintainer question:** “Approve Tailscale solely as optional transport with unchanged TLS/application authorization, unavailable-on-loss behavior and no public/LAN fallback?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D04 — API v1 and capability discovery

1. **Decision ID:** P004-D04 / ADR-P002-004.
2. **Existing evidence:** Product routes remain `/api/v1`; OpenAPI metadata version differs from product version. Existing owner/device semantics, methods, errors and clients form compatibility contracts.
3. **Threats and requirements:** TM-006, TM-021, TM-028, TM-043; SV-SEC-005, SV-SEC-018, SV-SEC-021, SV-SEC-022.
4. **Feasible alternatives:** (a) Preserve v1 and add optional descriptive capabilities. (b) Introduce v2 for all v0.3 changes. (c) Conditionally reinterpret v1 based on product version.
5. **Recommended default:** (a), with a security-sensitive action always independently authenticated/authorized and unavailable when required capability is missing.
6. **Compatibility consequences:** Best continuity for existing Android/iOS and other API v1 clients; unknown optional fields may be ignored only where behavior remains safe.
7. **Security trade-offs:** (a) requires explicit capability naming; (b) broad client migration; (c) downgrade and semantic ambiguity.
8. **Implementation consequences:** Capability version, minimum client policy and unsupported-operation UX must be specified before shipping such a surface.
9. **Required verification:** AT-07, AT-14, AT-23, AT-30 and mixed-version API contract tests.
10. **Exact maintainer question:** “Approve additive, descriptive capability discovery while preserving API v1 meanings and denying protected actions when capability/policy is unavailable?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D05 — Legacy owner/device to proposed roles

1. **Decision ID:** P004-D05 / ADR-P002-005.
2. **Existing evidence:** P001/P002/P003 and current API/auth/metadata show distinct browser/device principals, owner/device scope checks and owner-scoped resources; there is no general role model. Mobile profiles bind credential to endpoint/owner/device.
3. **Threats and requirements:** TM-006, TM-013, TM-014, TM-032; SV-SEC-005, SV-SEC-010, SV-SEC-011, SV-SEC-024.
4. **Feasible alternatives:** (a) Intersect exact legacy decision with new policy during transition, requiring explicit authority for additional users. (b) Backfill each owner as broad admin and replace old checks. (c) Leave legacy checks and authorize only new resources/features via new grants.
5. **Recommended default:** (a) for existing resources; (c) is a safe staged alternative if the intersection cannot be proven. No global host admin or implicit inheritance.
6. **Compatibility consequences:** Existing owner/device access can remain equivalent but cannot expand; device/library/checkpoint bindings persist. Old clients remain on existing effective rights.
7. **Security trade-offs:** (a) conservative and testable but requires dual-evaluation/migration proof; (b) risks cross-account expansion; (c) delays shared-resource features but minimizes migration risk.
8. **Implementation consequences:** Approve proposed roles/scopes, explicit-deny precedence, owner grantor power, host boundary and safe migration/rollback before SQL changes.
9. **Required verification:** AT-07, AT-15, AT-16 and legacy Android/iOS permission matrix, migration rehearsal and revocation race test.
10. **Exact maintainer question:** “Approve the legacy-result AND new-policy rule for every existing operation, and which exact owner/device mappings and candidate role scopes may be backfilled?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D06 — Approval state, separation and audit

1. **Decision ID:** P004-D06 / ADR-P002-006.
2. **Existing evidence:** No generic durable approval queue or persistent security audit model was found. Linux helper plan fingerprint/journal is not application approval. PostgreSQL and host journal are distinct records.
3. **Threats and requirements:** TM-007, TM-015, TM-016, TM-018, TM-036; SV-SEC-006, SV-SEC-012, SV-SEC-013, SV-SEC-015.
4. **Feasible alternatives:** (a) Durable exact request/decision/outcome with separate single-use execution authorization. (b) UI-memory confirmation only. (c) Treat authenticated login as standing approval.
5. **Recommended default:** (a); independent approver for privileged/destructive/cross-user actions; no bearer approval token; bind canonical exact operation and current policy revision. Block rather than silently allow self-approval in single-user installs unless a narrowly scoped explicit exception is approved.
6. **Compatibility consequences:** Existing actions remain unchanged until a defined policy applies; new storage would be additive and requires migration/retention controls.
7. **Security trade-offs:** (a) durable/replay resistant with privacy and lifecycle cost; (b) non-durable and unauditable; (c) collapses identity and consent. Retention period and privacy/legal access are unresolved.
8. **Implementation consequences:** Decide covered operation classes, approver authorities, self-approval exception, audit integrity/access and retention before schema/API.
9. **Required verification:** AT-08, AT-17, AT-18, AT-20; concurrency, replay, stale revision, audit-store failure and unauthorized audit-read tests.
10. **Exact maintainer question:** “Approve exact-operation, revision-bound approval plus bounded single-use execution authorization; which operations require independent approvers, what single-user exception is allowed, and who sets retention/access policy?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D07 — Protected Areas claim and enforcement

1. **Decision ID:** P004-D07 / ADR-P002-007.
2. **Existing evidence:** No Protected Areas policy or universal filesystem interception exists. Existing immutable file versions/conflict handling are application/storage behavior; OS/admin writes can bypass application gates.
3. **Threats and requirements:** TM-029–TM-031, TM-033; SV-SEC-022–SV-SEC-024.
4. **Feasible alternatives:** (a) Gate Synveil-managed mutations and claim only app-managed protection. (b) Require a separately named/tested native OS control on selected platforms. (c) Claim arbitrary filesystem writes are blocked by API checks.
5. **Recommended default:** (a); (b) only after separate platform approval and tests. Reject (c) as unsupported.
6. **Compatibility consequences:** Preserve immutable versions, existing conflict behavior and sync semantics; no kernel/driver change in P004.
7. **Security trade-offs:** (a) accurate but bounded promise; (b) may improve local enforcement but adds privileged components and platform divergence; (c) is false against arbitrary local writes.
8. **Implementation consequences:** Product wording, protected operation inventory, baseline/version binding and removal rules must match selected enforcement level.
9. **Required verification:** AT-31–AT-35; actual-device bypass and race tests per named platform before any OS-level claim.
10. **Exact maintainer question:** “Approve app-managed mutation gating as the initial Protected Areas guarantee, with no arbitrary filesystem blocking claim; which platform, if any, should receive a separate native enforcement design?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D08 — Snapshot and rollback domains

1. **Decision ID:** P004-D08 / ADR-P002-008.
2. **Existing evidence:** Existing logical snapshots are database-view artifacts; PostgreSQL metadata and ObjectStore bytes are separate authorities; file-version restore creates history. No cross-store atomic restore protocol or full-host rollback evidence exists.
3. **Threats and requirements:** TM-034–TM-036; SV-SEC-024, SV-SEC-025.
4. **Feasible alternatives:** (a) Separate recovery products and require verified coordinated manifest for combined DB/ObjectStore recovery. (b) Call logical snapshots “system backups”. (c) Operator-managed database and object backups with explicitly split responsibility.
5. **Recommended default:** (a), with (c) permitted as a declared operational boundary; no host rollback claim from application backups.
6. **Compatibility consequences:** Preserve file versions, content pins, manifests and current restore/prune behavior; no existing backup is reinterpreted as complete host recovery.
7. **Security trade-offs:** (a) honest completeness with staging and rehearsal cost; (b) simpler UX but likely silent data loss; (c) deployer burden and weaker product integration.
8. **Implementation consequences:** Decide consistency point, completeness manifest, signer, restore ordering, partial-restore boundary, secret rotation and operator ownership before snapshot feature/migration.
9. **Required verification:** AT-34–AT-38 plus fault injection with PostgreSQL/ObjectStore mismatch and real isolated restore rehearsal.
10. **Exact maintainer question:** “Approve distinct recovery-domain claims and block combined recovery claims until a manifest proves PostgreSQL/ObjectStore reference closure; who owns backup capture and restore operations?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D09 — CLI token and mount boundary

1. **Decision ID:** P004-D09 / ADR-P002-009.
2. **Existing evidence:** No Synveil CLI or network mount exists. API v1 has logical file operations; desktop IPC is local UI/client control, not a remote protocol. Platform secret stores exist.
3. **Threats and requirements:** TM-006, TM-025–TM-029; SV-SEC-020–SV-SEC-022.
4. **Feasible alternatives:** (a) Typed HTTPS API v1 client with OS-vault, bounded scopes and explicit destructive confirmation; mount adapter separate. (b) Reuse desktop IPC remotely. (c) CLI wraps arbitrary shell or broad bearer token.
5. **Recommended default:** (a); no remote shell and no credential in argv/config/plaintext profile.
6. **Compatibility consequences:** No effect until future CLI is approved; reuses existing API and owner/device policy.
7. **Security trade-offs:** (a) shares auth model but requires per-OS vault support; (b) crosses local OS assumptions; (c) broadens compromise impact.
8. **Implementation consequences:** Approve token scopes, lifetime, vault minimums, endpoint identity and mount support before CLI or mount release.
9. **Required verification:** AT-07, AT-14, AT-27–AT-30 on each supported OS, including host substitution and command-negative tests.
10. **Exact maintainer question:** “Approve a typed API-only CLI with scoped expiring vault-stored credentials and a separate mount adapter; which scopes, vaults and platforms are supported?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D10 — DeviceVeil host trust

1. **Decision ID:** P004-D10 / ADR-P002-010.
2. **Existing evidence:** DeviceVeil is absent from the integrated `develop/v0.3` source; a branch name is not evidence of an integrated or approved contract. P001/P002 establish single-host authority; no per-host trust root or credential audience exists.
3. **Threats and requirements:** TM-006, TM-037–TM-040; SV-SEC-026, SV-SEC-027.
4. **Feasible alternatives:** (a) Optional per-host identities, credentials/audiences, revocation, quotas, operator-approved bootstrap and host-local authority. (b) Shared cluster credential and implicit membership. (c) Keep DeviceVeil disabled/deferred and single-host only.
5. **Recommended default:** (a) prerequisites before integration; (c) is the current safe behavior. Reject (b).
6. **Compatibility consequences:** Single-host Synveil remains functional offline; no effect on API v1 owner/device contracts until integration is approved.
7. **Security trade-offs:** (a) stronger containment with identity/control-plane complexity; (b) convenient but lateral compromise; (c) delays federation without reducing current utility.
8. **Implementation consequences:** Issuer/root, host relationship, quota model, partition behavior, removal semantics and bootstrap publisher must be selected first.
9. **Required verification:** AT-39–AT-42 on at least two isolated hosts and compromised-host/partition exercises.
10. **Exact maintainer question:** “Approve DeviceVeil only as an optional per-host trust domain with independent identity, audience, revocation and quotas; who operates the trust root and how do partitions/removal behave?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D11 — Installer presentation/effect ownership

1. **Decision ID:** P004-D11 / ADR-P002-011.
2. **Existing evidence:** Desktop Qt/QML surfaces are present; install-engine owns plan/effect/recovery semantics; platform adapters differ. Current Linux host helper is bounded; Windows setup is per-user.
3. **Threats and requirements:** TM-017–TM-020; SV-SEC-014–SV-SEC-017.
4. **Feasible alternatives:** (a) Reuse desktop/platform UI for presentation; install-engine owns effect plan, journal and recovery. (b) Put effect execution in UI code. (c) Ship a privileged wizard for all platforms.
5. **Recommended default:** (a), with UI only sending typed choices/confirmation reference and the engine recomputing the fingerprint before effect.
6. **Compatibility consequences:** Preserves existing install-engine transaction journal, platform package lifecycle and current-user Windows scope.
7. **Security trade-offs:** (a) one enforcement owner; (b) duplicate/bypassable checks; (c) presentation code becomes privileged and platform semantics are falsely unified.
8. **Implementation consequences:** Define UI-to-engine schema, confirmation freshness and per-platform plan ownership before installer changes.
9. **Required verification:** AT-19–AT-22 on each supported installation surface; plan substitution, restart and no-effect assertions.
10. **Exact maintainer question:** “Approve the desktop/platform installer surface as presentation only, with install-engine owning plan, effects and recovery?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## Cross-cutting decisions not covered by one ADR

- **QR and server identity:** use a trusted in-person server identity fingerprint, or explicit comparison of a short authentication string; an untrusted QR hostname is insufficient. Compare with a previously pinned identity. Ordinary HTTPS proves only a separately trusted origin. This blocks pairing until selected; see P004-D05 only for authorization mapping, and resolve QR policy before P019–P024.
- **Credential rotation:** distinguish replacing one credential on the same bound device from replacing device identity. Select maximum credential lifetime, overlap and lost-device recovery before device credential implementation. Existing server expiry policy was not evidenced.
- **Publisher signing and downgrade:** select publisher root/key custody, signed metadata/artifact verification, revocation, rollback/downgrade policy and reproducible provenance before official release/update claims. Alternatives: native package-manager trust alone, Synveil publisher signatures, or both. Recommended: authenticated publisher metadata/artifacts with explicit anti-downgrade policy plus native package-manager checks where available. No signing format or key is approved. Affects TM-019/TM-043, SV-SEC-016/SV-SEC-030 and AT-21/AT-45.
- **Retention/privacy:** set audit and backup retention, access, export and deletion with product/privacy/legal review; P004 assigns no period.

## Decision recording and implementation gate

For each entry, record maintainer name/authority, date, decision (`ACCEPTED`, `REJECTED` or `DEFERRED`), rationale, approved alternative and any conditions in a follow-up decision record. Do not change status based on a merge, code existing elsewhere, or passing CI alone. Rejected/deferred items require an alternate safe contract before the blocked feature proceeds. P004 documentation can complete with all items pending; implementation readiness for affected features remains blocked until the applicable decision and acceptance evidence are approved.

## P004-D12 — QR first-contact server identity

1. **Decision ID:** P004-D12 / SV-SEC-008.
2. **Existing evidence:** Mobile endpoint models validate HTTPS/hostname and reject redirects; Android credential envelopes bind profile/origin/owner/device and iOS Keychain records validate endpoint. No QR pairing flow or cryptographic server-identity confirmation exists.
3. **Threats and requirements:** TM-009, TM-012; SV-SEC-007, SV-SEC-008; AT-09–AT-14.
4. **Feasible alternatives:** (a) Trusted in-person QR carrying server installation fingerprint, independently compared to trusted local display. (b) Explicit short authentication string comparison over an authenticated exchange. (c) Previously pinned server identity. (d) Ordinary HTTPS to an origin trusted independently before scan.
5. **Recommended default:** (a) first contact, with normal TLS and explicit confirmation of endpoint, server identity, account and device; (b) fallback if the user can compare channels; (c) reconnect. A hostname inside an untrusted QR is not sufficient. (d) is sufficient only for an already independently trusted origin.
6. **Compatibility consequences:** Existing endpoint validation remains; no relaxation of HTTPS or redirects. New enrollment stays additive and requires explicit user confirmation.
7. **Security trade-offs:** In-person verification depends on trusted display/operator; SAS depends on correct comparison and an approved authenticated exchange; pinning complicates recovery; HTTPS alone does not distinguish intended server from a substituted valid host.
8. **Implementation consequences:** Choose server identity representation, QR payload fields, SAS protocol if used, endpoint-change/recovery flow and invitation binding before pairing code. No cryptographic format is selected here.
9. **Required verification:** AT-11 and AT-14 on physical Android/iOS devices, wrong fingerprint, valid TLS wrong origin, invalid TLS, redirect, HTTP, copied QR and no-secret-storage negative tests.
10. **Exact maintainer question:** “Approve trusted in-person server fingerprint as the first-contact QR trust anchor, with SAS as a reviewed fallback and previously pinned identity on reconnect; what recovery ceremony applies when the server identity changes?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D13 — Device credential lifetime and rotation

1. **Decision ID:** P004-D13 / SV-SEC-009.
2. **Existing evidence:** Current enrollment grant is ten-minute, digest-only and transactionally one-use. Device credentials are individually revocable and device-bound; server credential expiry/automatic rotation was not evidenced. Android Keystore and iOS device-only Keychain protect client secrets. Android profile origin changes fence credentials.
3. **Threats and requirements:** TM-008, TM-010–TM-012; SV-SEC-007–SV-SEC-009; AT-09–AT-14.
4. **Feasible alternatives:** (a) Bounded credential lifetime and explicit owner-authorized replacement with short, controlled overlap. (b) Long-lived credentials with event-driven revocation only. (c) Re-enrollment that always replaces device identity.
5. **Recommended default:** (a), while preserving DeviceId, owner, server identity, library binding, checkpoints and queue when replacing a credential on the same proven device. A revoked credential never regains authority from a still-valid DeviceId. Identity replacement is a separate explicit process; do not silently reset checkpoints or adopt another device's persistent identity.
6. **Compatibility consequences:** Do not invalidate existing deployed credentials without staged migration/re-enrollment and old-client behavior tests. Offline clients keep pending work but cannot claim server acceptance after revocation.
7. **Security trade-offs:** Bounded lifetime reduces exposure but can strand offline clients; overlap aids rollover but enlarges compromise window; event-only revocation preserves availability but leaves stolen credentials valid until noticed; device identity reset is easy but risks impersonation/data loss.
8. **Implementation consequences:** Approve maximum lifetime, overlap, reauthentication/owner ceremony, per-credential vs per-device revoke behavior, lost-response recovery and queue reconciliation. Cryptographic format remains open.
9. **Required verification:** AT-12–AT-16 including expired/revoked credential, concurrent rotation, wrong origin, old Android/iOS clients, no checkpoint reset and only intended credential revoked.
10. **Exact maintainer question:** “Approve bounded device-credential lifetime with explicit same-device replacement preserving identity/checkpoints and immediate revocation; what maximum lifetime and overlap are acceptable, and what proof is required for identity replacement?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.

## P004-D14 — Publisher signing and downgrade policy

1. **Decision ID:** P004-D14 / SV-SEC-016, SV-SEC-030.
2. **Existing evidence:** P001/P003 distinguish source/build/runtime manifests and native package-manager trust; a manifest hash alone is not publisher authentication. P003 identified release signing and anti-downgrade as unresolved. Existing install engine verifies plans/journals but no approved publisher root or Synveil update-signing contract is established.
3. **Threats and requirements:** TM-019, TM-039, TM-043; SV-SEC-016, SV-SEC-027, SV-SEC-029, SV-SEC-030; AT-21, AT-41, AT-44, AT-45.
4. **Feasible alternatives:** (a) Rely on native package-manager publisher trust and restrict Synveil distribution claims to those channels. (b) Add Synveil publisher-authenticated artifact/metadata verification with key rotation/revocation and anti-downgrade policy. (c) Use both native channel checks and Synveil publisher verification for managed updates.
5. **Recommended default:** (c) where technically supported; otherwise (a) with explicit channel limitation. Never represent digest-only integrity or CI success as publisher authentication. Reject revoked, altered, unsigned where signature is required, and unauthorized downgrade artifacts.
6. **Compatibility consequences:** Existing package-manager workflows retain their native checks. Any new signature/metadata format must support key rotation, installed-version transition and safe failure without deleting data.
7. **Security trade-offs:** Native-only depends on distribution-channel trust; Synveil signing adds key custody, metadata and recovery burden; dual checks improve defense but require consistent revocation and release operations. Platform support and publisher key custody are unverified.
8. **Implementation consequences:** Decide publisher root, key custody, threshold, artifact/metadata canonicalization, provenance, revocation, rollback exception and downgrade floors before official release/update or DeviceVeil bootstrap claims. No algorithms, key IDs or formats are set here.
9. **Required verification:** AT-21, AT-41, AT-44, AT-45; altered/revoked/wrong publisher/old artifact negative tests; supported OS install/update/recovery evidence; key rotation rehearsal.
10. **Exact maintainer question:** “Approve dual native-channel and Synveil publisher verification where supported, with explicit anti-downgrade and revocation policy; who owns signing keys and what recovery exception permits a downgrade?”
11. **Current status:** RECOMMENDED_PENDING_APPROVAL.
