# Synveil v0.3 — PROMPT004 manifest

## Prompt identity and scope

| Field | Value |
|---|---|
| Prompt | SYNVEIL v0.3 — PROMPT004 |
| Phase / type | A — Security Foundation / Security policy design, architecture decision preparation and validation |
| Repository | https://github.com/nghianguyen150612/Synveil |
| Target branch | `develop/v0.3` |
| Work branch | `docs/v03-p004-security-policy` |
| Scope | Four documentation deliverables only; no runtime, API, schema, client, installer, host-helper, network, workflow or release changes. |
| Evidence date | 2026-10-25 |

## Git preflight and P002/P003 integration

| Ref/state | Verified evidence |
|---|---|
| Initial workspace | `/workspace/Synveil`, branch `work`, clean at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; no existing changes were overwritten. |
| `origin/main` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` (verified by remote ref). |
| `origin/develop/v0.3` | `6696361a3a39d25f2606bb48356b2bf6b2c89a0c` (verified by remote ref and clean P004 worktree HEAD). |
| P001 | PRs #156–#159 are all MERGED; P001 is closed. No P001 content was changed. |
| P002 PR #160 | MERGED at `f256d868645fb5987df73cfaa39909171fb121b2`; target `develop/v0.3`. |
| P003 PR #161 | MERGED at `6696361a3a39d25f2606bb48356b2bf6b2c89a0c`; target `develop/v0.3`. |
| P003 full integration diff | Reviewed `6696361^1..6696361`: exactly five added Markdown documents—`PROMPT003_MANIFEST.md`, `P003_SECURITY_DECISIONS.md`, `P003_SECURITY_REQUIREMENTS.md`, `P003_THREAT_MODEL.md`, `P003_TRUST_BOUNDARIES.md`; 1,422 insertions; no runtime/schema/config changes. |
| P004 worktree | Separate clean worktree `/workspace/Synveil-P004`, created as `docs/v03-p004-security-policy` directly from verified `origin/develop/v0.3`; existing workspace branch and history preserved. |

### Latest integration CI checked

GitHub workflow runs on exact merged `develop/v0.3` SHA `6696361…` were queried after merge. All six post-merge workflows completed; five succeeded and Windows native acceptance failed. It was not reported as passed.

| Workflow | Result |
|---|---|
| Rust CI | SUCCESS |
| Linux AppImage | SUCCESS |
| PostgreSQL 17 scheduled-maintenance | SUCCESS |
| Linux native packages (DEB + RPM) | SUCCESS |
| Windows installer | SUCCESS |
| Windows native acceptance | FAILURE — standard-user-native job failed at “Record required unavailable surfaces honestly”; see run `40163894577`. |

PR #161 check history also contains two failed Windows `standard-user-native` runs at P003 head `734cce10fd8f30a665f3b167f98738584f70b8f8` on the same unavailable-surfaces evidence step. Rust, documentation, dependency, Linux package and other reported checks were inspected. This is baseline CI evidence only; P004 does not repair or claim qualification of Windows native acceptance.

## Source evidence inspected

- P001: `docs/v0.3/audits/P001_V02_BASELINE_AUDIT.md`; `docs/v0.3/audits/P001_COMPATIBILITY_BASELINE.md`.
- P002: `docs/v0.3/architecture/P002_SYSTEM_ARCHITECTURE.md`; `P002_COMPATIBILITY_CONTRACTS.md`; `P002_ARCHITECTURE_DECISIONS.md`.
- P003: `P003_THREAT_MODEL.md`; `P003_TRUST_BOUNDARIES.md`; `P003_SECURITY_REQUIREMENTS.md`; `P003_SECURITY_DECISIONS.md`; `docs/v0.3/manifests/PROMPT003_MANIFEST.md`.
- Source spot checks included `crates/auth/src/sessions.rs`, `crates/auth/src/device_credentials.rs`, `crates/api/src/device_auth.rs`, `crates/server-network/src/lib.rs`, `crates/server-host/src/protocol.rs`, `crates/metadata/src/snapshot.rs`, Android `ServerProfileRepository.kt`/credential and transport boundaries, iOS `ServerEndpoint.swift`/Keychain references, plus P003 evidence ledger paths for API authorization, ObjectStore, installer, backup, IPC, deployment and CI.
- Observations used with limits: current session/device principals and one-use ten-minute enrollment grant are present; server-side device credential expiry/automatic rotation was not evidenced; reachability planning is separate from Tailscale (no integration in this baseline); Linux helper is bounded but is not a generic effect grant; Windows installer is current-user oriented; mobile endpoints enforce HTTPS outside debug loopback; no universal Protected Areas enforcement, combined PostgreSQL/ObjectStore atomic recovery or integrated DeviceVeil trust root was found.

## Roadmap availability

`Synveil-v0.3-Context-Roadmap-P001-P060.md` was searched for in the repository and workspace and was unavailable. P002/P003 record the same gap. P004 does not invent requirements or treat proposed P002 phase assignments as authoritative. Reconcile the roadmap before feature sequencing or authorizing work that depends on it.

## Deliverables and changed-file scope

| File | Purpose |
|---|---|
| `docs/v0.3/security/P004_SECURITY_POLICY_CONTRACTS.md` | Typed policy model, separate identity/authorization/approval/OS privilege/audit, credential and QR lifecycle, legacy roles, approvals/replay, privileged effects, networking, Protected Areas, recovery and DeviceVeil. |
| `docs/v0.3/security/P004_MAINTAINER_DECISIONS.md` | Independent decision packets for all eleven P002 ADRs and cross-cutting QR, credential rotation and publisher/downgrade policy. All remain pending. |
| `docs/v0.3/security/P004_IMPLEMENTATION_GATES.md` | Future feature gates, compatibility proof, all P003 Critical/High threat traceability, owners, failures, tests and roadmap blockers. |
| `docs/v0.3/manifests/PROMPT004_MANIFEST.md` | This evidence, validation, Git and handoff record. |

Exact-scope validation must show only these four paths changed. No general-purpose validator or production dependency was added.

## Threat and requirement coverage

P004 reuses, without renumbering, P003's `TM-001`–`TM-043`, `SV-SEC-001`–`SV-SEC-030`, and `AT-01`–`AT-45`. The implementation-gates table provides a row per Critical/High threat with its requirement, proposed enforcement component, deny/failure behavior, test IDs and provisional roadmap dependency/unresolved owner. Moderate TM-026/TM-027 retain existing AT-28/AT-29 traceability.

Critical/high future control coverage is proposed, not implemented. Critical feature blockers include QR/server identity, credential lifetime, owner-to-role mapping, approval replay, privileged native authority, Tailscale endpoint trust, native Protected Areas claims, PostgreSQL/ObjectStore consistency, DeviceVeil host trust and publisher/downgrade policy. Each has a pending decision and acceptance test.

## Decision status

All eleven P002 ADRs are represented in P004-D01 through P004-D11 and remain `RECOMMENDED_PENDING_APPROVAL`. QR first-contact identity (D12), credential lifetime/rotation (D13) and publisher signing/downgrade (D14) are separately packeted and pending. All 14 P004 packets remain `RECOMMENDED_PENDING_APPROVAL`. No decision is `ACCEPTED`; no authorization is inferred from P002/P003 merges or CI.

## Validation results

| Check | Result |
|---|---|
| `./scripts/validate-docs.sh` | PASS: all 24 documentation units and existing documentation contracts passed. |
| Local relative-link check | PASS: all Markdown links in the four P004 documents resolve locally. |
| `git diff --check` | PASS on the staged four-document diff. |
| Exact changed-file scope | PASS: staged diff contains exactly the four listed Markdown files. |
| ADR status consistency | PASS by read-only check: all 11 P002 ADRs are represented and remain PROPOSED; all 14 P004 decisions are pending; no ACCEPTED status is assigned. |
| Policy/identifier uniqueness | PASS: P004 defines no new P003 identifiers; all 43 threat IDs, 30 requirement IDs and 45 acceptance IDs are unique in source registers. |
| Traceability/source reference check | PASS: referenced threat/requirement/test IDs resolve; all 41 Critical/High threats have a P004 row; named inspected source files exist. |
| Mermaid syntax | `mmdc` is unavailable; both Mermaid blocks were manually reviewed. No renderer-based validation is claimed. |
| Runtime/native/cryptographic/release security validation | Not performed and not claimed; P004 changes documentation only. |

## Unresolved implementation blockers

1. Missing authoritative P001–P060 roadmap and feature ownership/sequencing confirmation.
2. Maintainer approval of applicable P004-D01–D14 decisions; all are currently recommendations.
3. Per-platform native authorization/elevation and Windows standard-user acceptance evidence.
4. Approved QR/server identity, device credential lifetime/rotation and endpoint migration contracts.
5. Approved role mapping and migration proof preserving owner/device/client state.
6. Approval class, separation-of-duties, audit access/integrity and retention/privacy decisions.
7. Tailscale endpoint/control authority and real-network failure/revocation evidence.
8. Protected Areas native scope or explicit app-managed-only product claim.
9. PostgreSQL/ObjectStore consistency manifest and restore ownership/rehearsal.
10. DeviceVeil trust issuer/root, per-host quota, partition/removal and bootstrap publisher decisions.
11. Publisher signing, key custody/revocation and downgrade policy; dependency/CI/release evidence remains a release qualification gate.

## P005 handoff

P005 may begin only after maintainers provide/confirm the authoritative roadmap and explicitly approve or reject the policy decisions relevant to its scope. The handoff must separate: (a) approved contracts with evidence, (b) proposed contracts still pending, and (c) blocked controls with exact missing decision/test/owner. P005 must preserve API v1 and current owner/device permissions, must not interpret this policy document as implementation, and must not begin any feature whose decision or roadmap dependency remains unresolved. P004 does not start P005.

## Commit and PR closeout

| Field | Value |
|---|---|
| P004 commit SHA | Pending final commit. |
| PR URL / target | Pending creation; target must be `develop/v0.3`. |
| PR merge status | Pending; merge only after review and required integration gates pass. Never merge to `main`. |
