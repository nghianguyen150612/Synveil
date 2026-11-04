# P026 manifest

Status: provisional integration-only reference qualification. Production RBAC, role persistence, approval endpoints and token issuance remain disabled. P004-D05/D06/D09 are not approved. Roadmap reconciliation remains open.

Base: `14c2dbf51464d6893c1f19a8c1f8e041be5b338e` (P025 PR #186 merge).
P025 source: `54918f457784edb29b52dcbdac79c3539e33a3d8`, verified ancestor.
Branch: `feat/v03-p026-rbac-policy-delegation-qualification`; target `develop/v0.3`.
Worktree: `/workspace/Synveil-p026`; original clean `work` checkout preserved.

## Files

- `crates/auth/src/authorization_reference.rs`: v2 reference module declaration.
- `crates/auth/src/authorization_reference/rbac_v2.rs`: versioned typed fixture policy, permissions, bounded delegation and policy-derived approval; reuses P025 authentication/legacy rules.
- `crates/auth/tests/p026_reference.rs`: 4 deterministic matrix tests, 53 enumerated negative rows plus positive/compatibility/OS assertions.
- `crates/auth/tests/p026_transactions.rs`: 4 independent-connection SQLite qualification tests.
- `crates/auth/tests/support/p026.rs`: trusted fixture actors/ownership/assignments.
- `crates/auth/tests/support/p026.sql`: isolated test-only tables, tombstones and audit; no production migration.
- `.github/workflows/v03-p026-rbac-policy-delegation.yml`: exact-head Ubuntu/Windows reference and regression checks.
- [P026 security qualification](../security/P026_RBAC_POLICY_DELEGATION_QUALIFICATION.md): production authority inventory, trust contracts, PostgreSQL requirements and open gates.
- This manifest.

## Validation ledger

Initial P025 + P026 contracts: 11 passed (3 P025, 4 matrix, 4 SQLite), no ignored tests.
Documentation validation: passed all 24 sections before final manifest; final rerun pending.
Broad regressions/strict Clippy initially blocked by missing system libdbus development dependency. Isolated Debian packages extracted outside repository; retry in progress. No feature/security guard was disabled.
Hosted exact final commit Ubuntu/Windows and global Rust CI results pending; pending/cancelled/incomplete jobs are not passes. Final commit/run evidence will be recorded in the Draft PR and final delivery (a commit cannot embed its own SHA).

## Open requirements and release blockers

P025 inherited audit/provenance/approval/PostgreSQL concurrency/integration matrix remain incomplete for production. SQLite tests qualify fixture state only. Native abrupt-crash/fsync, production authority issuance, PostgreSQL locks/isolation and external effects/outbox still require qualification. Internal maintenance adapter call-graph audit remains open. Test fixture roles do not define approved production roles.

P028 native Windows interactive GUI, genuine logon and installed-component IPC evidence unavailable. Windows resilience run `41627469609`: missing installed LICENSE. Historical run `41437684142`: `INSTALLER_VERIFY_FAILURE: secret-like marker in output`. Distinct blockers; no verification weakened. Signing, mobile physical-device, real-network and independent security approval remain release gates.

Exactly one P026 Draft PR is required; no merge and no P027 work.
