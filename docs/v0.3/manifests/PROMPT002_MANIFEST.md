# SYNVEIL v0.3 — PROMPT002 manifest

## Prompt identity and scope

| Field | Value |
|---|---|
| Prompt | SYNVEIL v0.3 — PROMPT002 |
| Phase | A — Foundation, Architecture & Security Requirements |
| Task | System architecture blueprint and compatibility contracts |
| Scope | Documentation-only design. No v0.3 feature, product behavior, runtime/client source, API implementation, configuration, migration, release artifact or P001 evidence changed. |
| Evidence date | 2026-10-24 |
| Repository | https://github.com/nghianguyen150612/Synveil |
| Roadmap | Synveil-v0.3-Context-Roadmap-P001-P060.md was not found in the repository or workspace. This limitation is recorded; no absent phase-specific constraints were inferred. |

## Git preflight and branch

| Ref or state | Evidence |
|---|---|
| Initial workspace branch and SHA | work at 604499fce0dfd134d82da3d5bb5d4a055c5eea8d; initially clean. |
| origin/main | 604499fce0dfd134d82da3d5bb5d4a055c5eea8d. |
| origin/develop/v0.3 | d69e167913feec5ebed548b60eb07cbdf40ba6e9, re-fetched and compared with ls-remote before branch creation. |
| Base tree | e89ae52dd38d16f89d98aaaa2deca84761ae22e3. |
| P002 working branch | docs/v03-p002-architecture, created directly from origin/develop/v0.3. |
| Existing P002 branch at preflight | No local or remote branch with this name was found. |
| Fetch configuration | The local origin refspec tracks main only. develop/v0.3 was explicitly fetched; no reset, rebase, force push, tag edit or shared-history rewrite was used. |
| Concurrent P001 changes | P001 PRs #156–#159 are merged. The source tree includes the baseline audit, dependency advisory repairs, deterministic Windows test repair and Tokio advisory repair. |

The PR #159 merge SHA is an ancestor of the P002 base. The P001 post-repair Rust CI run [40080072135](https://github.com/nghianguyen150612/Synveil/actions/runs/40080072135) completed with all 11 jobs successful on commit ce336966e293a1a2e747904123f7b26bc370c4ee. That commit has the same tree as P002 base d69e167913feec5ebed548b60eb07cbdf40ba6e9. This is baseline CI evidence, not v0.3 feature validation.

## Authoritative inputs and inspected source

P002 read the merged P001 artifacts:

- [P001 v0.2 baseline audit](../audits/P001_V02_BASELINE_AUDIT.md)
- [P001 compatibility baseline](../audits/P001_COMPATIBILITY_BASELINE.md)
- [P001 manifest](PROMPT001_MANIFEST.md)

Source and contract inspection included:

- Root Cargo workspace and package manifests; all 18 workspace crate names.
- Core/auth/API/metadata and existing API router, auth middleware, device-auth routes and OpenAPI contract.
- ObjectStore/storage, server configuration/network/storage/service/bootstrap/host modules, host helper protocol and deployment manifests/systemd units.
- Install engine plans, lifecycle policy, adapter boundary, error model and append-only journal; Linux package/AppImage deployment and Windows per-user installer.
- Desktop client-sync SQLite migrations, client process/local control IPC, Qt/QML surface and platform secure-secret abstractions.
- React web app, Android Room/profile/enrollment sources and iOS mutation/photo SQLite schemas, Keychain use and client layers.
- PostgreSQL migration directory, migration README and migration runner; validation script inventory and CI workflow files.
- Repository history, GitHub PR merge metadata and P001 Rust CI jobs.

The authoritative roadmap filename was searched in the repository and under /workspace and was absent.

## Verified architectural findings

| Finding | Evidence or qualification |
|---|---|
| Current baseline has 18 Rust crates, an Axum API, React web app, separate Rust/Qt desktop UI and client process, Android and iOS apps. | Root Cargo.toml, module sources and P001 audit. |
| API product paths remain under /api/v1; health/readiness routes are separate. OpenAPI info.version is 0.1.0-blueprint while workspace product version is 0.2.0. | crates/api/src/router.rs, api/openapi.yaml and Cargo.toml. |
| PostgreSQL is canonical for server metadata and identity; ObjectStore owns content bytes under opaque server keys. | synveil-metadata, synveil-object-store, synveil-storage and P001 contract inventory. |
| Authentication distinguishes browser sessions from device-bound credentials. Existing resource authorization is owner/device scoped. | synveil-auth, API auth/device-auth handlers and migrations. No general RBAC or approval queue was found. |
| Desktop UI/client are separate processes using local IPC. Android and iOS maintain independent local persistence and secure credential stores. | Client control IPC, client-sync migrations, Android Room/Keystore and iOS SQLite/Keychain source. |
| Current install engine already owns plans, effects, privilege requirements, verification, recovery and append-only journal. The three-step wizard is absent. | crates/install-engine sources and deploy adapters. Web bootstrap and mobile enrollment are separate. |
| Managed API backend is loopback at 127.0.0.1:3000 behind HTTPS edge port 443; operator HTTPS remains separate. The internal worker has no public listener. | API binary, server-network config and deployment sources. |
| Tailscale is not integrated. A Tailscale-named interface is classified as Tunnel and excluded from ordinary LAN discovery. | crates/server-host/src/native/network.rs and P001 search. |
| Current schema counts are 36 PostgreSQL migrations, desktop SQLite schema 8, Android Room 4, iOS mutation/sync 9 and iOS photo-observation 3. | migrations/, crates/client-sync, CacheDatabase.kt, MutationQueueSQLiteStore.swift and PhotoObservationSQLiteSchema.swift. |
| No general Protected Areas, generic approval/audit model, CLI, network mount or DeviceVeil cluster integration was found. | Bounded source inventory in P001 and this source inspection. |
| PostgreSQL and ObjectStore have separate consistency boundaries; no atomic cross-store or full-host restore protocol is established. | Metadata/storage ownership, backup manifests/content pins and P001 recovery findings. |

Existing component behavior is source-backed, but P001 does not qualify every client/server combination or v0.2 release compatibility. The current architecture and confidence labels are in [P002_SYSTEM_ARCHITECTURE.md](../architecture/P002_SYSTEM_ARCHITECTURE.md).

## Proposed architecture decisions

All new decisions remain PROPOSED:

- ADR-P002-001: extend the existing crate that owns each responsibility.
- ADR-P002-002 and ADR-P002-011: keep wizard presentation unprivileged and plan-driven; keep OS effects and journaling in the existing engine/adapters.
- ADR-P002-003: model Tailscale as an optional transport adapter.
- ADR-P002-004: preserve API v1 and negotiate optional capabilities.
- ADR-P002-005: add RBAC without changing legacy owner/device authorization.
- ADR-P002-006: persist approval separately from execution, privilege and audit outcome.
- ADR-P002-007: begin Protected Areas with app-managed mutation checks; require separate OS enforcement designs.
- ADR-P002-008: keep file, application, database, ObjectStore and host recovery surfaces distinct.
- ADR-P002-009: use HTTPS API v1 for CLI and isolate OS mount adapters.
- ADR-P002-010: keep DeviceVeil optional and separate from single-host Synveil authority.

See [P002 architecture decisions](../architecture/P002_ARCHITECTURE_DECISIONS.md) for alternatives, impact and proposed owners. No decision is marked ACCEPTED.

## Compatibility risks and open questions

1. No released v0.2 client/server support matrix or minimum-client policy is qualified.
2. Web browser support and exact Android/iOS live-server compatibility remain unknown; passing build/test jobs are not full-client qualification.
3. The API v1 OpenAPI description version is stale and needs a reviewed document-version policy; it must not be conflated with product or wire versions.
4. Exact role semantics, token lifetime/revocation policy, legacy owner mapping and resource sharing behavior need maintainer decision.
5. Pairing invitation persistence, QR trust bootstrap and device recovery/re-enrollment need P003 threat-model approval.
6. Tailscale enrollment/control API, endpoint identity, user-visible state and unavailable-tunnel policy are not implemented.
7. Approval retention, approver separation, audit access and privileged-operation authentication remain open.
8. Protected Areas cannot promise arbitrary write blocking without a selected OS enforcement mechanism and platform evidence.
9. PostgreSQL/ObjectStore recovery and full-host rollback are not atomic or qualified; operator restore boundaries must be stated.
10. DeviceVeil topology, unified logical storage view, host identity and failure isolation require an explicit cross-project contract.
11. The authoritative P001–P060 roadmap remains unavailable and should be supplied before any later prompt relies on unstated roadmap constraints.

These open decisions do not block P003 from producing the security requirements and threat model. They do block implementation of the affected feature contracts until approved.

## P003 security handoff

P003 should produce the threat model and resolve or explicitly defer: browser/proxy assumptions; lost, replayed or cloned device credentials and QR invitations; legacy owner-to-role authorization mapping; approval actor separation and retention; exact privileged OS effects and native authorization; tunnel endpoint substitution and fallback; API/worker/database/storage privilege; and DeviceVeil host compromise containment. The P002 trust matrix names assets, owners, least-privilege expectations, failure behavior and unresolved questions. P002 does not complete passkey/FIDO2 or privileged-authentication design.

## Validation and scope

Validation results are recorded after the local documentation checks complete. The planned scope is limited to these four new documents:

1. docs/v0.3/architecture/P002_SYSTEM_ARCHITECTURE.md
2. docs/v0.3/architecture/P002_COMPATIBILITY_CONTRACTS.md
3. docs/v0.3/architecture/P002_ARCHITECTURE_DECISIONS.md
4. docs/v0.3/manifests/PROMPT002_MANIFEST.md

No new validator or dependency is planned. The existing P001 validator is P001-only: its exact-file-set assertion rejects any non-P001 change, so it is not applicable to this P002 documentation scope.

| Check | Result |
|---|---|
| Documentation validator | PASS: ./scripts/validate-docs.sh completed all 24 documentation units. |
| Local relative links | PASS: the validator's primary relative-link check found no broken links, including the new architecture documents. |
| Mermaid syntax | mmdc is not installed. The six diagrams use Mermaid flowchart syntax and were reviewed as source; no dependency was added. |
| Architecture references | PASS: linked source paths and schema counts were checked against the baseline tree. |
| P001 validator | Not applicable because it enforces an exact four-file P001-only diff. |
| Whitespace and source scope | PASS: git diff --cached --check; the base-to-staged list contains exactly the four P002 documents. |
| Production/runtime source, client source, config, migration, API/auth/install behavior, release artifacts, P001 evidence | PASS: no changed path is outside the four documentation files. |
| Production feature validation | Not performed or claimed. |

## Files, commit and pull request

| Field | Value |
|---|---|
| Files added | The three P002 architecture documents above and this manifest. |
| Files modified | None expected outside those four documentation files. |
| Architecture content commit SHA | c9b7cf405b1d7e0391ab458d48b612e84bac6497 (introduces the four artifacts). |
| Pull request | [#160 — docs(v0.3): define P002 architecture and compatibility contracts](https://github.com/nghianguyen150612/Synveil/pull/160), targeting develop/v0.3. |
| Merge state | OPEN and unmerged. Review/check state must be rechecked at the final report; no merge performed. |

## P002 gate

P002 is complete when the four scoped documents pass applicable validation, changed-file scope contains documentation only, the branch is pushed and one PR targets develop/v0.3. Any outstanding CI/review requirements remain explicitly open; no P003 implementation is started.
