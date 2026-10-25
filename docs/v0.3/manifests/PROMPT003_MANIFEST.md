# Synveil v0.3 — PROMPT003 manifest

## Prompt identity and scope

| Field | Value |
|---|---|
| Prompt | SYNVEIL v0.3 — PROMPT003 |
| Phase | A — Foundation & Security Architecture |
| Task | Threat model, trust boundaries, security requirements and decision review |
| Scope | Documentation-only. Five P003 documents; no product code, API, database, configuration, validator or CI behavior changed. |
| Evidence snapshot | 2026-10-25 |
| Repository | https://github.com/nghianguyen150612/Synveil |
| Roadmap | Synveil-v0.3-Context-Roadmap-P001-P060.md was not found in the repository or workspace. P002 records the same limitation; unseen requirements were not inferred. |

## Git baseline and P002 dependency

| Ref or state | Evidence |
|---|---|
| Initial workspace branch and SHA | work at 604499fce0dfd134d82da3d5bb5d4a055c5eea8d; initially clean. |
| origin/main | 604499fce0dfd134d82da3d5bb5d4a055c5eea8d. |
| origin/develop/v0.3 | d69e167913feec5ebed548b60eb07cbdf40ba6e9. |
| P002 PR #160 | Open and unmerged at preflight; head branch docs/v03-p002-architecture, head f63374f19a81bbd4b9ef6881445a165c07ba0700, base develop/v0.3. |
| P003 working branch | docs/v03-p003-security-threat-model, created from the verified P002 PR head. No reset, rebase, force push or shared-history rewrite. |
| P003 PR target | docs/v03-p002-architecture while P002 remains open. Do not merge P003 before P002 integration. |
| P001 state | PRs #156–#159 are merged per P002 manifest. |
| P003 content commit | 2902ba3cfd3199e8079a54f5111c232c5105975e; introduces exactly the five requested files. |
| P003 PR URL / merge state | [#161](https://github.com/nghianguyen150612/Synveil/pull/161), OPEN and unmerged as of 2026-10-25. |

The stacked diff preserves P002 commits and does not duplicate P002 documents. After P002 merges, retarget P003 to develop/v0.3, verify the effective diff contains only the five P003 files and rerun applicable validation before considering merge.

## Inspected evidence

P003 read the P001 audit and compatibility baseline, P002 architecture, compatibility contracts, decision register and manifest. Source inspection covered auth sessions/passwords/device credentials; API cookies, CSRF, middleware, routes, files, downloads, mutations, sync and device enrollment; metadata authorization/device credential repositories; storage upload/local-store behavior and ObjectStore keys; backup/snapshot contracts; install-engine plans/journals; Linux host helper protocol/native adapters/Polkit; server-network and interface classification; desktop IPC and tests; web authentication recovery; Android endpoint/Keystore and iOS endpoint/Keychain; deployment artifacts; package manifests/lockfiles and GitHub Actions workflows.

Principal evidence paths are enumerated in the [threat model evidence ledger](../security/P003_THREAT_MODEL.md#10-evidence-ledger). Existing P001/P002 contracts are linked rather than copied.

### Dependency and CI evidence

- A read-only npm audit of apps/web/package-lock.json on 2026-10-25 reported five development-dependency advisories: three high and two moderate. Vite is a direct dev dependency with a high advisory; Vitest is a direct dev dependency with a moderate advisory through @vitest/mocker; brace-expansion and source-map-js are transitive high findings; @vitest/mocker is moderate. npm reported a fix available for each. P003 made no dependency changes.
- Local cargo-deny could not run because Cargo is not installed in the workspace. GitHub Rust CI on exact P002 head f63374f19a81bbd4b9ef6881445a165c07ba0700 passed its Dependency policy job (workflow run 40096147562, job 120878719608). This is remote baseline evidence, not a local run or release qualification.
- P002-head Windows native acceptance run 40096147597 failed; P003 does not claim to repair or qualify it.
- Web CI runs install, lint, typecheck, tests and build; no npm audit step was found. Workflow/action permission pinning and official release signing remain incomplete or unqualified.

## Threat-model method and findings

STRIDE is applied across browser/API, device and QR enrollment, authorization/approval, privileged host effects, network/Tailscale, IPC/CLI, Protected Areas, storage/recovery, DeviceVeil and delivery/update trust boundaries. Abuse and recovery cases supplement STRIDE. Likelihood and impact use 1–5 scales; score is L × I. Critical is 20–25, High 12–19, Moderate 6–11 and Low 1–5. Existing, proposed and unknown controls are kept distinct; source-backed current controls are not treated as platform-wide proof.

The register contains 43 unique threats, TM-001 through TM-043, with assets, actors, entry boundaries, attacks, impacts, score, verified controls, proposed mitigations, residual risk, acceptance test, owner, phase, evidence and status. High and Critical threats have at least one planned acceptance test. The top blockers are QR/server identity, device credential lifetime, legacy owner/RBAC mapping, approval replay/scope, arbitrary-root prevention, artifact identity, Tailscale identity, Protected Areas limits, cross-store recovery, DeviceVeil host isolation, npm build dependency findings and release publisher/downgrade protection.

## Deliverables

| File | Contents |
|---|---|
| [P003_THREAT_MODEL.md](../security/P003_THREAT_MODEL.md) | Actors, 20 protected assets, STRIDE coverage, 43 scored threats, evidence ledger and priorities. |
| [P003_TRUST_BOUNDARIES.md](../security/P003_TRUST_BOUNDARIES.md) | Nine trust zones, identity/approval/privilege flows, sensitive data crossings, Mermaid diagrams, fail-closed behavior and platform limits. |
| [P003_SECURITY_REQUIREMENTS.md](../security/P003_SECURITY_REQUIREMENTS.md) | SEC-001–SEC-010 invariants, SV-SEC-001–SV-SEC-030 normative requirements, threat traceability and AT-01–AT-45 acceptance matrix. |
| [P003_SECURITY_DECISIONS.md](../security/P003_SECURITY_DECISIONS.md) | Security review of all 11 P002 ADRs; alternatives, mitigations, maintainer decisions, acceptance gates and explicit deferrals. |
| PROMPT003_MANIFEST.md | Git/evidence/validation record and P004 handoff. |

Every P002 ADR remains PROPOSED. No maintainer approval is inferred.

## Validation results

| Check | Result |
|---|---|
| Documentation validator | PASS: ./scripts/validate-docs.sh completed all 24 documentation units. |
| Markdown and relative links | PASS: validator and read-only local-link scan found no broken links. |
| Mermaid | mmdc is not installed; both diagrams were reviewed as Mermaid flowchart source. No dependency was added. |
| Threat and requirement traceability | PASS: 43 unique TM IDs, 30 unique SV-SEC IDs and 45 unique AT IDs; every High/Critical threat has mitigation, owner/status and an acceptance-test reference; all 11 P002 ADRs were reviewed once and remain PROPOSED. |
| Source-reference consistency | PASS: linked repository source paths and evidence links resolve in the checked-out P002 tree. |
| Whitespace and changed-file scope | PASS: git diff --cached --check; the P003 commit and PR comparison to P002 contain exactly five Markdown files. |
| Production behavior and feature tests | Not run locally; no product implementation changed and none is claimed. GitHub PR workflows were automatically queued; latest live outcomes are available on PR #161. |

## Open high-risk decisions

1. Exact privileged effect inventory, actor authority, native authorization, step-up and recovery per OS.
2. QR first-contact server identity, pairing confirmation, grant lifecycle and lost-device credential rotation.
3. Legacy owner/device to role mapping, resource/host scopes, revocation and stale-policy transaction behavior.
4. Approval operation classes, exact binding, self-approval separation, audit integrity/access and retention.
5. Tailscale local approval, endpoint identity, peer revocation and failure behavior without downgrade.
6. Protected Areas scope: application-managed mutation gating versus a separately supported native OS mechanism.
7. PostgreSQL/ObjectStore consistency point, backup completeness, restore ordering and mismatch recovery.
8. DeviceVeil host identity issuer, per-host credentials, resource quotas, partition behavior and bootstrap signing.
9. Official publisher keys, artifact provenance, signature verification, downgrade prevention and key rotation.
10. Missing authoritative P001–P060 roadmap and any phase-specific constraints it contains.

## P004 handoff

Before P004 implementation, obtain maintainer decisions on the blockers above and reconcile the missing roadmap. P004 should translate approved requirements into an exact scope, compatibility matrix and implementation/test plan; retain API v1 owner/device semantics; use fail-closed authorization; keep approvals separate from OS privilege; preserve loopback/no-downgrade defaults; and avoid combined backup or OS-protection claims without native evidence. P004 must identify which requirements it implements, run the corresponding AT tests, and report unimplemented requirements without treating this design as proof. Do not begin P004 automatically as part of P003.

## Commit and pull request closeout

| Field | Value |
|---|---|
| P003 content commit SHA | 2902ba3cfd3199e8079a54f5111c232c5105975e. |
| P003 closeout manifest commit SHA | This manifest is part of the follow-up closeout commit on the P003 branch; the resulting branch head is the PR #161 head. |
| Branch head SHA | Recorded in the live PR #161 head and final engineering report. |
| Pull request | [#161 — docs(v0.3): define P003 threat model and security requirements](https://github.com/nghianguyen150612/Synveil/pull/161). |
| Pull request base | docs/v03-p002-architecture while P002 PR #160 remains open. |
| Merge state | OPEN and unmerged. P002 PR #160 remains OPEN; P003 must not merge before P002 integration, required checks and maintainer approval. |
