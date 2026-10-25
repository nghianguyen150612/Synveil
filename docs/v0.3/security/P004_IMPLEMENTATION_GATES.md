# Synveil v0.3 — P004 Implementation and Verification Gates

**Status:** future implementation checklist; every control is proposed and unimplemented unless repository evidence explicitly says otherwise.
**Baseline:** P003 integrated at `6696361a3a39d25f2606bb48356b2bf6b2c89a0c`.
**Roadmap:** `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in repository/workspace. P002 phase ranges below are tentative ownership references only; reconcile the authoritative roadmap before committing sequencing or claiming roadmap closure.

## 1. Readiness rule

A feature is implementation-ready only when its applicable decision packet entries are explicitly approved, the exact scope/owner is assigned, the relevant P003 tests are implemented and pass, and platform/real-network/native evidence is collected where required. Documentation passing is not product security validation. Unapproved decisions remain blockers even when code or an external branch exists. Each gate below lists its minimum evidence and blocked decisions. No production control is marked complete by this document.

All future mutations must preserve the five independent checks: authenticated identity, current resource authorization, exact approval when required, native OS authority when needed, and attributable audit evidence. Unknown/stale state denies; current revocation wins; errors conceal foreign resource existence.

## 2. Feature gates

| Gate | Dependencies and owner | Minimum evidence | Cross-platform / evidence bar | Explicit blockers |
|---|---|---|---|---|
| Security foundation | P004-D01, D04–D06; auth/API/metadata owners | Typed principal/context/action/resource/policy revision model; route-to-action matrix; denial taxonomy; current-policy revalidation; audit confidentiality/integrity design; migration proof for legacy scope | Rust/API integration and DB transaction/revocation races; audit access controls; compatibility against legacy fixtures | Maintainer approval of role mapping, approval classes/self-approval, audit integrity/access/retention; roadmap reconciliation |
| Installer | P004-D02, D11, D14; install-engine/platform/release owners | Closed effect registry, exact plan fingerprint, actor/effect binding, artifact identity policy, durable intent/result and recovery; existing transaction journal compatibility | Linux package/helper/Polkit tests; Windows current-user and any proposed elevation/UAC tests separately; macOS only if supported | No general helper; per-OS native authorization unknown; artifact publisher and downgrade decision; Windows standard-user CI failed at P003 baseline |
| SynveilNet | P004-D03; server-network/host/client owners | Local-only default; listener/firewall plan; canonical server identity binding; TLS/redirect/mismatch rejection; unavailable state without fallback | Linux/Windows listeners and firewall; Android/iOS/desktop client TLS; actual LAN/tailnet partition, revoke and certificate evidence | Tailscale authority/identity approval; operator HTTPS/proxy contract; real-network proof; no public exposure decision |
| Pairing | P004-D12, D13; auth/API/Android/iOS owners | Invitation state machine, atomic one-use transition, endpoint fingerprint/SAS verification, explicit confirmation, no permanent secret in QR, lost-response recovery and negative matrix | Physical Android/iOS camera/device tests plus server transaction tests; TLS interception/substitution fixture | First-contact identity and cryptographic format not approved; credential lifetime/rotation not approved; AT-11 blocker |
| Identity/RBAC | P004-D05, D13; auth/API/metadata/web/mobile owners | Legacy/new result comparison; no-broader-than proof; explicit role/scope/deny/inheritance rules; deterministic revisioned decisions; transactional revocation fence | Web, desktop, Android and iOS legacy-owner and non-owner matrix; migration rehearsal preserves bindings/checkpoints | Owner-to-role decision; old clients' precise effective rights; no SQL or backfill until approved |
| Protected Areas | P004-D07; API/metadata/storage/client/platform owners | Operation inventory; stable resource/version/hash binding; guarded supported mutation paths; safe replacement/path race design; accurate app-managed claim | API and every supported client; per-OS native bypass only for separately named/tested mechanism | App-vs-native product claim; native OS support/privilege design; no universal filesystem blocking claim |
| Snapshots/recovery | P004-D08; metadata/storage/operations owners | Domain completeness matrix; manifest signer/integrity and object inventory; reference closure; staged restore, ordering, interrupted restore journal and secret rotation | PostgreSQL and ObjectStore separate fault injection; isolated end-to-end restore rehearsal; host-specific claims tested by platform/ops | Consistency point, signer/key custody, restore ownership, retention; no cross-store atomicity assumption |
| CLI/mount | P004-D09; client/platform/API owners | Typed allowlisted calls, endpoint pinning, scoped expiring token, OS vault failure behavior, destructive confirmation, mount isolated from API | Linux/macOS/Windows vault and host substitution tests; mount semantics per named OS/filesystem | Token scope/lifetime, supported vaults/platforms, mount support; no shell endpoint |
| Privileged operations | P004-D02, D06, D07, D08, D11; host/platform/metadata owners | Closed catalog; exact actor/target/params; approval and native authorization independently verified; audited journal and safe rejection/recovery | Linux Polkit/helper tests; Windows SCM/UAC/firewall tests separately; platform claims cannot transfer | Per-effect native authorization/step-up decisions; approver separation; unsupported OS denies; no remote-token privilege |
| DeviceVeil | P004-D10; independent DeviceVeil/Synveil owners | Trust-root and issuer design, per-host audience/revocation, quotas, one-time signed bootstrap, partition/removal/compromise isolation | Two or more isolated real hosts; cross-host penetration and partition exercises | Issuer, trust root, operator, quotas, host removal, bootstrap signer; remain optional and disabled absent approval |
| Release qualification | P004-D14; release/security/repository owners | Current dependency advisory triage, least-privilege CI/actions, artifact provenance, publisher verification, downgrade/revocation policy and reproducible evidence | Supported release artifact verification and install/update tests on every claimed OS; production key custody exercise | No official publisher trust or anti-downgrade decision; docs checks do not qualify release; P003 records npm advisories and CI limits |

## 3. Required compatibility proof for identity changes

Before a persistent role or identity change:

1. Freeze a representative legacy fixture with user, owner, device, library, version, checkpoint, queued mutation, conflict and credential-revocation state.
2. Enumerate current allowed/denied operations by principal and resource using the existing API v1 owner/device rule.
3. Evaluate the proposed policy at one explicit revision and prove every existing allowed action remains allowed only where required for compatibility, and no disallowed action becomes allowed.
4. Prove legacy owner retains effective prior access; legacy non-owner remains denied; device/library binding is unchanged; cross-account access remains denied; role assignment requires an authorized grantor; revoked access racing an operation cannot commit; old Android/iOS clients remain within their established permissions.
5. Rehearse additive migration, backfill, verification and interruption/restart on a copy. Preserve opaque IDs, checkpoints, queues and versions; do not reset or infer identity from resource UUIDs.
6. Keep old checks in place until the new decision is equivalent and approved. Any uncertain mapping denies; no SQL migration is authorized by P004.

Acceptance references: AT-07, AT-13–AT-16, AT-34. Requirements: SV-SEC-005, SV-SEC-009–SV-SEC-011, SV-SEC-024–SV-SEC-025.

## 4. Critical and High threat traceability

The following are all P003 Critical and High threats. Identifiers are reused; `TM-026` and `TM-027` are Moderate and remain in P003's existing AT-28/AT-29 test plan. P004 contract references are sections of [P004 policy contracts](P004_SECURITY_POLICY_CONTRACTS.md). Feature ranges are provisional P002 labels, not roadmap commitments. Where there is no confirmed roadmap owner, the named component owner is proposed and the roadmap dependency remains unresolved.

| Threat | P003 requirement → policy contract | Responsible component / proposed enforcement | Failure behavior | Verification | Roadmap dependency / unresolved owner |
|---|---|---|---|---|---|
| TM-001 | SV-SEC-001 → §3 | Auth service/API middleware: fresh session, server expiry/revocation per request | Deny expired/revoked; no mutation replay | AT-01, AT-02 | P025–P030 tentative; auth owner |
| TM-002 | SV-SEC-002 → §3 | API cookies/CSRF/proxy configuration: session-bound proof and trusted-origin rules | Reject missing/invalid CSRF or provenance | AT-03 | P025–P030 tentative; API/deployment owner |
| TM-003 | SV-SEC-003 → §3 | Auth/API/edge: bounded abuse controls and generic response | Throttle/deny with no account enumeration | AT-04 | P025–P030 tentative; rate-limit authority/storage unresolved |
| TM-004 | SV-SEC-003 → §3, §6 | Auth/bootstrap/API/host: atomic one-time claim and closed state | One winner; managed closed bootstrap remains closed | AT-05 | P007–P012/P025–P030 tentative; deployment owner |
| TM-005 | SV-SEC-004 → §3, §5 | API/web/desktop/mobile/host diagnostics: redact secrets and content | Redact before durable/exported output; fail closed on unsupported vault | AT-06 | Each future feature; crash/telemetry owner unresolved |
| TM-006 | SV-SEC-005 → §2, §4 | API/auth/metadata: route-action-resource matrix and repository scope predicates | Generic deny/not-found; no cross-user existence leak | AT-07 | P025–P030 tentative; route inventory owner |
| TM-007 | SV-SEC-006 → §5 | API/metadata/host journal: tamper-protected actor/decision/effect record | Audit-required operation blocks if durable evidence unavailable | AT-08 | P025–P030/P049–P054 tentative; retention/access decision pending |
| TM-008 | SV-SEC-007 → §3.1 | Auth/API/metadata/mobile: one-use grant bound to audience and target | Replay/concurrent loser denied; no secret recovery | AT-09, AT-10 | P019–P024 tentative; grant owner |
| TM-009 | SV-SEC-008 → §3.1 | Android/iOS/API: authenticated server identity plus explicit user confirmation | Abort before secret storage on mismatch, HTTP, redirect or invalid TLS | AT-11 | P019–P024 tentative; first-contact decision P004-D12 pending |
| TM-010 | SV-SEC-007 → §3.1 | Auth/metadata/mobile: atomic state transition and explicit fresh-enrollment recovery | Single winner; lost response never replays credential | AT-12 | P019–P024 tentative; enrollment recovery owner |
| TM-011 | SV-SEC-009 → §3 | Auth/metadata/mobile secure stores: separate credential and device identity rotation | Revoked credential stays denied; queue retained but not trusted as server authorization | AT-13 | P019–P030 tentative; maximum lifetime/overlap P004-D13 pending |
| TM-012 | SV-SEC-008, SV-SEC-009 → §3, §7 | Client profile/endpoint and auth: bind credential to origin/server/install/owner/device | Mismatch blocks credential transmission; explicit re-enrollment | AT-14 | P019–P030 tentative; endpoint migration owner |
| TM-013 | SV-SEC-010 → §4 | Auth/API/metadata migration: legacy AND proposed result, no broader grants | Ambiguous mapping denies; abort migration | AT-15 | P025–P030 tentative; role/owner mapping P004-D05 pending |
| TM-014 | SV-SEC-011 → §2, §4, §5 | Auth/metadata/storage: monotonic policy revision and commit-time fence | Abort stale commit; retry only with same idempotency identity | AT-16 | P025–P036 tentative; transaction boundary owner |
| TM-015 | SV-SEC-012 → §5, §6 | Approval/API/metadata/helper: exact fingerprint and atomic single-use authorization | Changed/replayed/stale operation has no effect | AT-17 | P025–P054 tentative; canonicalization/replay owner |
| TM-016 | SV-SEC-013 → §5 | Approval/auth/metadata: approver authority separate from requester | Deny self-approval where required; audit approver changes/outcomes | AT-18 | P025–P054 tentative; single-user exception pending |
| TM-017 | SV-SEC-014 → §6 | Install-engine/server-host/platform: closed typed effect inventory | Unknown effect/remote shell rejected before OS call | AT-19 | P007–P012/P049–P054 tentative; effect catalog owner |
| TM-018 | SV-SEC-015 → §6 | Linux helper/install-engine and future Windows adapter: actor/plan/nonce binding | Native denial, replay or changed plan causes no effect | AT-20 | P007–P012/P049–P054 tentative; per-OS authorization pending |
| TM-019 | SV-SEC-016 → §6, §10, D14 | Installer/release/deploy: verify publisher artifact identity before privilege | Unknown/revoked/altered artifact rejected, preserve data | AT-21 | P007–P012/P055–P060 tentative; signing decision P004-D14 pending |
| TM-020 | SV-SEC-017 → §6, §9 | Install-engine/host journal/storage: intent, effect and result checkpoints | Ambiguous effect pauses for inspection; data/journal retained | AT-22 | P007–P012/P037–P054 tentative; compensation owner |
| TM-021 | SV-SEC-018 → §7 | Server-network/host and all clients: TLS hostname/certificate identity | Do not send credential on mismatch/redirect | AT-23 | P013–P024 tentative; endpoint pin owner |
| TM-022 | SV-SEC-018 → §7, §10 | Network/host/Tailscale adapter: peer reachability not app authority | Peer compromise/revocation cannot authorize resource; outage unavailable | AT-24 | P013–P018 tentative; Tailscale identity/control owner pending |
| TM-023 | SV-SEC-019 → §7 | Network planner/host firewall/clients: local-only default and no downgrade | Invalid plan stays unavailable; no HTTP/public fallback | AT-25 | P013–P018 tentative; listener/firewall owner |
| TM-024 | SV-SEC-019 → §7 | Network/platform: interface labels only as observed hints, not identity | Reject identity inference from name/tunnel flag | AT-26 | P013–P018 tentative; interface evidence owner |
| TM-025 | SV-SEC-020 → §2 | Desktop IPC/client: same-user command allowlist and local OS endpoint ownership | Reject endpoint substitution, credential disclosure and scope expansion | AT-27 | P043–P054 tentative; desktop IPC owner |
| TM-028 | SV-SEC-021 → §2, §4, §7 | Future CLI/platform/API: typed calls, scoped vault token, no shell | Host mismatch/destructive action without confirm/shell request denied | AT-30 | P043–P048 tentative; CLI decision P004-D09 pending |
| TM-029 | SV-SEC-022 → §2, §8 | API/storage/client-sync/filesystem: canonical paths and handle-relative replacement | Reject traversal/race; prior valid data remains | AT-31 | P031–P036 tentative; platform path owner |
| TM-030 | SV-SEC-023 → §8 | Product/API/platform: separate app-managed and named native claims | Never claim arbitrary local writes blocked by app checks | AT-32 | P031–P036/P049–P054 tentative; native mechanism decision pending |
| TM-031 | SV-SEC-023 → §5, §8 | API/metadata/storage/client sync: hash/version and approval recheck at replacement | Abort stale/partial write; preserve previous immutable version | AT-33 | P031–P036 tentative; atomicity owner |
| TM-032 | SV-SEC-024 → §2, §8 | API/metadata/storage: owner-scoped object lookup, backend keys hidden | Foreign ID is indistinguishable from absent; no bytes returned | AT-34 | P031–P042 tentative; repository owner |
| TM-033 | SV-SEC-024 → §8, §9 | Upload/API/ObjectStore: verify length/hash before publish | Candidate not published; valid existing object remains readable | AT-35 | P031–P042 tentative; integrity owner |
| TM-034 | SV-SEC-025 → §9 | Metadata/storage/GC: protect referenced/pinned object through delete fence | Retain object if reference status uncertain | AT-36 | P037–P042 tentative; GC transaction owner |
| TM-035 | SV-SEC-025 → §9 | Backup operator/PostgreSQL/ObjectStore: coordinated completeness manifest and closure | Mark point incomplete; reject destructive restore | AT-37 | P037–P042/P055–P060 tentative; consistency/signing owner pending |
| TM-036 | SV-SEC-025 → §2, §5, §9 | API/metadata/storage/host: authorize target, verify old-state secrets, audit restore | Deny unauthorized target; rotate/revoke restored secrets before exposure | AT-38 | P037–P042 tentative; restore owner/secret rotation pending |
| TM-037 | SV-SEC-026, SV-SEC-027 → §10 | DeviceVeil adapter/trust issuer: authenticated per-host relationship and quota | Reject spoofed/unapproved host/contribution | AT-39 | Before DeviceVeil; trust root/issuer owner pending |
| TM-038 | SV-SEC-026 → §2, §10 | DeviceVeil/host ObjectStore: host-audience credential and independent resource policy | Revoke/isolate compromised host; unrelated host/object store denies | AT-40 | Before DeviceVeil; cross-host authority owner pending |
| TM-039 | SV-SEC-027 → §6, §10 | Bootstrap/release/platform: signed/pinned artifact and expiring one-use approval | Replay, wrong signer or altered script has no install effect | AT-41 | Before DeviceVeil; bootstrap signer/native owner pending |
| TM-040 | SV-SEC-027 → §10 | Cluster scheduler/host service: per-host quota and partition isolation | Disable optional cluster work; single-host service remains usable | AT-42 | Before DeviceVeil; quota/partition owner pending |
| TM-041 | SV-SEC-028 → §11 | Web build/dependency owner: advisory scanning, owner and remediation/exception | Release gate fails for untriaged high build dependency finding | AT-43 | P055–P060 tentative; advisories identified by P003 need current triage |
| TM-042 | SV-SEC-029 → §11 | Repository admin/CI: least permissions, immutable reviewed actions, provenance | Untrusted workflow cannot promote trusted artifact | AT-44 | Before official release signing; CI owner and roadmap pending |
| TM-043 | SV-SEC-030 → §6, §9, §11 | Release/update/platform: publisher-authenticated metadata, revocation and anti-downgrade | Reject unsigned, revoked, substituted or unauthorized older artifact | AT-45 | P049–P060 tentative; publisher/downgrade P004-D14 pending |

### Coverage notes

- Critical threats: TM-009, TM-011, TM-013, TM-015, TM-017, TM-019, TM-022, TM-030, TM-035, TM-037–TM-039, TM-043. Each has proposed enforcement, denial behavior, responsible component and a named P003 test above; design/decision gates remain pending.
- High threats: TM-001–TM-008, TM-010, TM-012, TM-014, TM-016, TM-018, TM-020–TM-021, TM-023–TM-025, TM-028–TM-029, TM-031–TM-034, TM-036, TM-040–TM-042. Each has corresponding row, owner, failure behavior and existing AT ID.
- TM-025 local IPC is covered by SV-SEC-020 and AT-27 in P003; TM-026/TM-027 remain Moderate and use AT-28/AT-29. P004 does not supersede or renumber them.
- These are proposed owners. P002 phase references are not proof that a roadmap owner exists. The missing P001–P060 roadmap and any unassigned cross-component responsibility remain explicit blockers to scheduling implementation.

## 5. Validation checklist for a future implementation PR

- Every referenced `TM-xxx`, `SV-SEC-xxx` and `AT-xx` exists in P003 and remains unchanged.
- Every recommended control maps to a named code/platform owner and a deny/failure mode.
- Legacy matrix and revocation-race tests pass before replacing an existing owner/device check.
- Cross-user denial responses do not reveal resource existence.
- Critical controls have test evidence appropriate to their claims: isolated hosts, actual clients, real listeners, native authorization, storage fault injection or signed release artifact as applicable.
- A docs validator or unit suite alone is never cited as native OS, cryptographic or production security validation.
