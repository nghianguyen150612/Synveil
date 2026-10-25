# Synveil v0.3 — P003 security decision register

**Status:** all entries are recommendations pending explicit maintainer review. No decision is ACCEPTED by this document.
**Baseline:** P002 architecture head f63374f19a81bbd4b9ef6881445a165c07ba0700.
**Related:** [P002 architecture decisions](../architecture/P002_ARCHITECTURE_DECISIONS.md), [threat model](P003_THREAT_MODEL.md), [security requirements](P003_SECURITY_REQUIREMENTS.md).

## 1. Decision status and authority

P002 records eleven architecture decisions as PROPOSED. P003 assesses their security impact and recommends constraints; it does not change their status or approve product policy. A maintainer must record an explicit decision before a later implementation changes a public contract, persists new authorization state, increases privilege or claims a supported platform guarantee.

The roadmap file Synveil-v0.3-Context-Roadmap-P001-P060.md was not present in the repository or workspace at the evidence snapshot. P002 phase ranges below are planning labels, not verified roadmap commitments.

## 2. Review of all P002 ADRs

### ADR-P002-001 — Prefer existing crate ownership

- **P002 proposal / compatibility:** Extend the crate that owns each responsibility; add a crate/process only for an independent lifecycle or platform boundary. Compatible with current auth/API/metadata/storage/client boundaries if dependencies do not bypass authorization.
- **Threats introduced:** Shared code could bypass API policy, expose DB/ObjectStore keys or concentrate secrets; premature process separation could create confused deputy channels. TM-006, TM-017, TM-025, TM-032.
- **Required mitigation:** Keep principal resolution and resource authorization at API/service boundaries; keep raw storage keys and privileged adapters behind their owning modules. New crate boundaries need explicit data, identity and privilege contracts.
- **Alternative / trade-off:** A new crate for each feature improves naming separation but adds dependency and lifecycle complexity without necessarily adding a security boundary.
- **Maintainer decision needed:** Approve the ownership rule and require a documented boundary before adding a crate or process.
- **Security acceptance:** AT-07 and AT-19; static dependency review shows no UI-to-DB, UI-to-ObjectStore-key or remote-to-root path.
- **Status:** PROPOSED; maintainer approval outstanding.

### ADR-P002-002 — Keep installer presentation outside the privileged effect boundary

- **P002 proposal / compatibility:** Keep the wizard unprivileged and plan-driven; use install-engine and narrow native adapters/helper. Compatible with Linux package manager, AppImage user scope, Windows current-user setup, bootstrap and mobile enrollment as distinct flows.
- **Threats introduced:** UI-to-helper confused deputy, plan substitution, stale confirmation, replay, artifact substitution and unsafe recovery. TM-017–TM-020.
- **Required mitigation:** Closed effect inventory; exact actor, target, parameters and plan fingerprint; native OS authorization per effect; signed/verified artifact identity; journal intent/result and data-preserving recovery.
- **Alternative / trade-off:** Running the entire wizard as root simplifies file access but gives presentation code excessive authority. A persistent privileged daemon increases reachable attack surface. Both are less safe than a narrow adapter.
- **Maintainer decision needed:** Approve which exact effects may elevate on each supported OS, whether step-up authentication is required, and who may confirm.
- **Security acceptance:** AT-19–AT-22, including native rejection and interruption tests. Passkeys/FIDO2 or hardware-backed user presence remain unverified platform options.
- **Status:** PROPOSED; root authority and step-up choice outstanding.

### ADR-P002-003 — Model Tailscale as an optional transport adapter

- **P002 proposal / compatibility:** Add an optional adapter while preserving HTTPS, application authorization, current LAN behavior and local-only default. No Tailscale integration currently exists.
- **Threats introduced:** Peer compromise, endpoint substitution, DNS changes, tunnel loss, control API privilege abuse and accidental exposure/fallback. TM-021–TM-024.
- **Required mitigation:** TLS hostname/server verification remains mandatory; Tailscale identity is reachability context only; local enrollment requires native approval; tunnel loss does not downgrade or widen listening.
- **Alternative / trade-off:** Treating a tailnet peer as trusted is convenient but confuses network membership with user authorization. Building a new VPN/relay is out of scope and expands the trust base.
- **Maintainer decision needed:** Select supported Tailscale enrollment/control mechanism, endpoint identity binding, local approval, revocation and unavailable-tunnel UX.
- **Security acceptance:** AT-23–AT-26 with compromised-peer, interface spoof, TLS failure and no-fallback cases.
- **Status:** PROPOSED; endpoint identity and control authority outstanding.

### ADR-P002-004 — Preserve API v1 and negotiate optional capabilities

- **P002 proposal / compatibility:** Keep API v1 meanings stable; add optional endpoints/capability discovery only with a versioned contract. Existing route and auth behavior is the compatibility baseline.
- **Threats introduced:** Capability flags may be mistaken for authorization; mixed-version clients may silently take a weaker path; response changes may confuse authorization or safety checks. TM-006, TM-021, TM-028, TM-043.
- **Required mitigation:** Every operation authenticates and authorizes independently; unknown capabilities fail unavailable for protected operations; no insecure fallback. Keep product, OpenAPI and wire versions distinct.
- **Alternative / trade-off:** A global API v2 reduces ambiguity but disrupts existing clients; changing v1 behavior conditionally risks silent downgrade.
- **Maintainer decision needed:** Approve capability naming/versioning and minimum-client policy without conflating discovery with permission.
- **Security acceptance:** AT-07, AT-14, AT-23, AT-30 and mixed-version contract tests before any capability endpoint ships.
- **Status:** PROPOSED; capability and minimum-client contract outstanding.

### ADR-P002-005 — Extend authorization without changing owner semantics

- **P002 proposal / compatibility:** Add an explicit policy layer that maps current owner/device behavior equivalently before introducing roles/scopes. Compatible only if no existing record gains broader permission.
- **Threats introduced:** Overbroad role defaults, implicit inheritance, cross-account/device scope, stale revocation and TOCTOU. TM-006, TM-013, TM-014.
- **Required mitigation:** Typed principal; action/resource-specific checks; deny on missing mapping; preserve exact owner/device compatibility; revalidate current policy at commit; auditable additive migration.
- **Alternative / trade-off:** Replacing owner checks in one migration is simpler but can broaden legacy access. Inferring a role from client labels, UUID possession or network position has no authority basis.
- **Maintainer decision needed:** Define role names, grants, explicit deny precedence, owner compatibility mapping, device/host/resource scope and migration rollback policy.
- **Security acceptance:** AT-07, AT-15 and AT-16 against legacy fixtures and concurrent revocation schedules.
- **Status:** PROPOSED; owner mapping is a critical implementation gate.

### ADR-P002-006 — Persist approval separately from execution and OS privilege

- **P002 proposal / compatibility:** Persist exact scoped requests, decisions and outcomes in PostgreSQL; approval is separate from action execution and native privilege. No generic approval/audit system exists today.
- **Threats introduced:** Approval replay, operation substitution, self-approval, stale policy, unaudited outcomes and retention exposure. TM-007, TM-015, TM-016, TM-020.
- **Required mitigation:** Bind request and approver identities, exact operation/resource/host/parameters, privilege, expiry, nonce and policy revision; reauthorize before effect; durable audit intent and result.
- **Alternative / trade-off:** UI-only approval is easy but not durable or replay-safe; treating login as approval collapses identity and consent.
- **Maintainer decision needed:** Define which operations require approval, self-approval/separation rules, authorized approver roles, audit access/integrity and retention. Do not invent legal retention terms.
- **Security acceptance:** AT-08, AT-17, AT-18 and AT-20; mutation blocked if required durable evidence cannot be written.
- **Status:** PROPOSED; actor separation and retention outstanding.

### ADR-P002-007 — Begin Protected Areas with application-managed enforcement

- **P002 proposal / compatibility:** Gate Synveil-managed mutations first and design OS enforcement separately. This accurately limits claims across platforms and preserves existing file-version/conflict semantics.
- **Threats introduced:** Arbitrary local writes bypass app checks; symlink/race/partial-write attacks; tampered integrity baselines; approval removal. TM-029–TM-031.
- **Required mitigation:** Stable logical identity, expected prior hash/revision, all supported mutation paths guarded, audit and baseline persistence before effect; separate platform threat model for OS-level enforcement.
- **Alternative / trade-off:** Claiming app checks prevent privileged local writes is false. Kernel drivers/modules introduce substantial platform and signing scope and are not a P003 choice.
- **Maintainer decision needed:** Decide whether initial feature is app-managed only, which paths are in scope, and whether any OS enforcement platform will be separately supported.
- **Security acceptance:** AT-31–AT-33; real native bypass tests before making OS-level claims.
- **Status:** PROPOSED; coverage and platform claims outstanding.

### ADR-P002-008 — Keep recovery products and rollback domains distinct

- **P002 proposal / compatibility:** Separate file versions, application snapshots, PostgreSQL, ObjectStore and host rollback. Compatible with existing distinct metadata/content stores and current restore semantics.
- **Threats introduced:** Incomplete backup, cross-store mismatch, unauthorized restore, old secrets reintroduced, GC race and destructive rollback. TM-034–TM-036.
- **Required mitigation:** Manifests bind each domain and content hash/reference; pins protect live bytes; restore reauthorizes target and verifies all referenced data before destructive changes; no atomic combined claim without a protocol.
- **Alternative / trade-off:** A single “system snapshot” promise is simpler for users but unsupported without coordinated capture, consistency and rollback semantics.
- **Maintainer decision needed:** Select supported backup units, consistency point, restore order, operator responsibilities and mismatch recovery.
- **Security acceptance:** AT-36–AT-38 plus fault-injected restore where PostgreSQL and ObjectStore disagree.
- **Status:** PROPOSED; combined-recovery claims remain blocked.

### ADR-P002-009 — Use API v1 for CLI and isolate mount-specific behavior

- **P002 proposal / compatibility:** Future CLI uses HTTPS API v1 and device credentials; remote logical file operations remain separate from OS mount adapters. No CLI or mount exists.
- **Threats introduced:** Token leakage, host-profile substitution, destructive command confusion, unsafe local paths and arbitrary command exposure. TM-006, TM-028, TM-029.
- **Required mitigation:** OS secure credential store, TLS host binding, typed allowlisted API calls, explicit confirmation for destructive/approval actions, sanitized errors and revocation.
- **Alternative / trade-off:** Reusing local desktop IPC remotely would bypass its local OS assumptions; a second remote protocol duplicates authorization.
- **Maintainer decision needed:** Decide CLI scopes, supported vaults, device token lifetime, profile identity format and mount support boundary.
- **Security acceptance:** AT-07, AT-14, AT-30 and AT-31; no remote shell path.
- **Status:** PROPOSED; token and mount policy outstanding.

### ADR-P002-010 — Keep DeviceVeil optional and separate from single-host Synveil authority

- **P002 proposal / compatibility:** DeviceVeil is an optional authenticated extension; single-host Synveil remains independently useful and authoritative. No integration exists.
- **Threats introduced:** Host spoofing, cross-host credential reuse, malicious resource contribution, lateral movement, quota abuse, split brain and bootstrap script compromise. TM-037–TM-040.
- **Required mitigation:** Per-host identity/audience and revocation, resource scope and quotas, authenticated one-time bootstrap, signed install content, explicit operator approval, isolation during partition.
- **Alternative / trade-off:** Shared cluster credentials and implicit membership simplify topology but create a cross-host compromise path. Making federation mandatory harms single-host availability.
- **Maintainer decision needed:** Define host identity issuer, trust root, host removal, resource policy, quotas, partition behavior and the separate authority relationship.
- **Security acceptance:** AT-39–AT-42 on at least two isolated hosts plus partition and compromised-host exercises.
- **Status:** PROPOSED; cross-project host trust is a critical blocker.

### ADR-P002-011 — Keep wizard UI and installation effects separately owned

- **P002 proposal / compatibility:** Reuse desktop/platform presentation; keep plan/effect/recovery in install-engine. Compatible with current Qt/QML desktop, native package operations and Windows per-user setup.
- **Threats introduced:** UI manipulation, confirmation mismatch, changed target, duplicated/retried effects and resumed stale plan. TM-017–TM-020.
- **Required mitigation:** UI sends typed choices only; engine recomputes and fingerprints plan; confirmation is bound and expires; adapter revalidates authority/effect; journal is durable.
- **Alternative / trade-off:** Put effects in UI code and gain superficial simplicity at the cost of duplicate policy and bypassable checks.
- **Maintainer decision needed:** Approve presentation ownership per platform, confirmation semantics and whether any platform-specific shell may add authority.
- **Security acceptance:** AT-19–AT-22 on each supported installer surface, including changed plan and process restart.
- **Status:** PROPOSED; UI confirmation and platform ownership outstanding.

## 3. Cross-cutting security recommendations

1. Keep all eleven P002 ADR statuses PROPOSED until maintainers record an explicit decision. A later implementation prompt must link the decision and acceptance evidence.
2. Resolve the critical gates before enabling the affected feature: root effect inventory and native authorization; QR/server identity; owner-to-role compatibility; exact approval scope; DeviceVeil host identity; backup consistency; signed publisher/update policy.
3. Do not treat a recommendation in this register as approval to add a migration, public endpoint, OS privilege, cross-host credential or new supported platform guarantee.
4. Keep application authorization, user approval and operating-system authorization as separate checks with separate evidence.
5. Preserve the current single-host, API v1, owner/device, loopback and separate PostgreSQL/ObjectStore contracts unless an approved compatibility plan explicitly changes them.

## 4. Explicitly deferred decisions

| Decision area | Deferred question | Required owner / gate |
|---|---|---|
| Root authority and step-up | Which native OS effects need elevation, what native prompt is acceptable, and whether passkeys/FIDO2 are feasible on each platform? | Maintainer and platform owners; before P007–P012/P049–P054. |
| QR trust bootstrap | What authenticates server identity on first pairing when the client has no pinned endpoint identity? | Auth/mobile owners; before P019–P024. |
| RBAC owner mapping | What exact roles/scopes preserve legacy user/device authority and how are old records mapped? | Auth/API owner; before P025–P030. |
| Approval separation and retention | Which actions need independent approvers, how are exceptions audited, and how long are records retained? | Maintainer/privacy owner; before approval persistence. |
| Tailscale identity | Which local approval and endpoint binding are used, and what does revoked/unavailable tailnet access do? | Network/platform owner; before P013–P018. |
| Protected Areas | Is the product promise app-managed mutation gating only or a named native OS mechanism on selected platforms? | Product/platform owners; before P031–P036. |
| Recovery consistency | What constitutes a consistent PostgreSQL/ObjectStore recovery point and who owns restore ordering? | Storage/operations owner; before P037–P042. |
| DeviceVeil trust | Who issues host identity, how are host credentials scoped/revoked, and how are quotas/partitions handled? | DeviceVeil and Synveil maintainers; before integration. |
| Supply-chain signing | Which publisher keys, attestations, downgrade rules and key-rotation process qualify official releases? | Release/security owners; before P049–P060. |
| Roadmap mapping | Does the absent P001–P060 roadmap add sequencing or acceptance constraints not present in P002? | Maintainer to supply or confirm omission before later phase commitment. |

These deferrals do not prevent P003 documentation from completing. They block production-ready claims and implementation of the affected security-sensitive feature until resolved.
