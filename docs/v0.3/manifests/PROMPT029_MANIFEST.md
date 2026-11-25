# Synveil v0.3 — P029 manifest

**Status:** `PROVISIONAL_REFERENCE`; authoritative roadmap reconciliation remains blocked. This manifest records only the bounded audit/revocation reference foundation specified for this checkpoint. It is not an official P029 roadmap completion marker.

## Recovery and scope reconciliation

| Item | Verified state |
|---|---|
| Starting checkout | `/workspace/Synveil` was absent; created a fresh checkout without modifying any pre-existing repository/worktree. The unrelated `/workspace/Beszel-iOS` checkout was preserved. |
| Base | `origin/develop/v0.3` fetched and verified at `b8beb109e58c847cb136607e4948c564cf6d2aa7`; P027 PR #188 is in its ancestry. |
| P028 | PR #189 / `feat/v03-p028-windows-native-acceptance` is separate, Draft and unmerged. It was not changed. `/workspace/Synveil-p028` did not exist in this environment and was not overwritten. |
| Existing P029 work | No P029 audit/revocation branch or implementation was present in remote heads, local refs, GitHub P029 audit/revocation PR search, or this checkout. |
| Authoritative roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in the checkout, workspace, or GitHub code search. No conflicting authoritative P029 scope was available. This implementation remains provisional until roadmap reconciliation. |
| P004 policy | P004-D05 owner-to-role mapping and P004-D06 approval/audit remain `RECOMMENDED_PENDING_APPROVAL`; this work does not approve them. |
| P025–P027 | Source/manifests/qualifications reviewed. P025/P026 remain isolated reference evaluators. P027's PostgreSQL schema and transactions are test-only; P029 extends that disposable schema with evidence/checkpoint tables and does not create another authority ledger. |

The selected base matched the user-supplied last verified SHA exactly. No reset, force push, unrelated worktree change, P028 change, P036 signing work or v0.2 behavior change was made.

## Architecture and implementation

Production authorization remains at existing owners: browser/session authentication and revocation in `auth`/metadata; device credential authentication and revocation in `auth`/metadata; API principal and CSRF checks at API middleware/routes; owner/device/resource checks in API services and metadata repositories. P025–P027 reference inputs and fixtures are not trusted production identity or policy sources.

P029 adds a versioned typed model and verifier in `crates/auth/src/authorization_reference/audit_v1.rs`. The test-only PostgreSQL extension stores owner-scoped events/checkpoints in a generated P027 fixture schema. Event sequence and hash-head updates occur in the authority transaction. Each P029 record points to the corresponding P027 global audit sequence; per-owner hash chaining supplies integrity linkage and a deterministic read order. A complete read validates the closed schema, owner scope, sequence, authority order, digest chain, checkpoint, and P027 operation/revocation relationship before returning a bounded page.

The reference model supports authentication, authorization, ownership, roles/delegation, approvals, session/device/credential/grant revocation, policy revisions and rejected privileged operations. P029's own persisted adapter records authority allow/deny, approval consumption, and grant/session/device revocation in the P027 reference fixture. Approval issuance and policy/delegation stale-state coverage also remains in inherited P027 fixtures; other event categories are typed only. This does not imply that production events are being captured.

Sensitive values are excluded by schema: no credential, token, password, recovery secret, request body or unbounded metadata fields. Actor/device/resource/approval identifiers are bounded references. Foreign/unknown resource denials omit the resource ID. Supported page size is 1–100; complete verification is capped at 10,000 records and fails closed above the cap or on incomplete evidence.

The digest chain is tamper-evident only relative to a separately preserved checkpoint. The chain and checkpoint here share one database trust domain. No administrator-resistant integrity, external delivery, immutable retention, production audit access or privacy policy is claimed.

## Changed files

| File | Purpose |
|---|---|
| `crates/auth/src/authorization_reference.rs` | Exports the isolated `audit_v1` reference module. |
| `crates/auth/src/authorization_reference/audit_v1.rs` | Closed typed schema, validation, canonical digest, completeness verifier and bounded owner-scoped pagination; includes pure adversarial tests. |
| `crates/auth/tests/support/p027.rs` | Adds a fixture constructor that extends the same disposable P027 schema only for P029 tests. |
| `crates/auth/tests/support/p029.sql` | P029 evidence-only event/checkpoint tables, version/type constraints and append/head advancement constraints; no migration. |
| `crates/auth/tests/support/p029.rs` | Test-only append/read adapter; same P027 transaction, current-state lookup, P027 order reference and cross-record verification. |
| `crates/auth/tests/p029_audit_postgres.rs` | Disposable PostgreSQL transaction, denial, rollback, revocation race, replay, missing/tamper, scope and interruption tests. |
| `.github/workflows/v03-p029-security-audit-revocation.yml` | Focused Linux PostgreSQL and Windows contract/compile qualification. |
| `docs/v0.3/security/P029_AUDIT_REVOCATION_QUALIFICATION.md` | Trust boundaries, evidence model, integrity limits and security gate states. |
| This manifest | Recovery, scope, changed files and validation ledger. |

## Validation ledger

Local validation was run against the current P029 source before commit using Rust 1.100.0 and PostgreSQL 17.12 (`17.12 (Debian 17.12-1.pgdg13+1)`). The exact pushed branch SHA remains the required head for hosted qualification.

| Check | Local result |
|---|---|
| `cargo fmt --all -- --check` | PASS on final local source. |
| P029 pure verifier tests | PASS, 4 tests. |
| P029 PostgreSQL 17 test matrix | PASS, 13 tests against PostgreSQL 17.12. |
| P029 PostgreSQL server restart | PASS; prepare and verify phases passed around a real disposable server-container restart. |
| P025/P026 compatibility contracts | PASS, 13 tests. |
| P027 PostgreSQL regressions | PASS, 15 tests. |
| Auth all-target Clippy with `--locked -- -D warnings` | PASS on final local source. |
| Documentation validation and `git diff --check` | PASS; all 24 documentation validation units passed. |
| Exact-head hosted Linux/Windows workflow | PASS at PR source SHA `a9142118ce620017c2798128b590d7a1afae3d03`; Linux and Windows jobs passed. The final pushed head and latest run are recorded in PR #190: https://github.com/nghianguyen150612/Synveil/pull/190. |

Executed commands:

```text
cargo test -p synveil-auth --lib authorization_reference::audit_v1 --locked
cargo test -p synveil-auth --test p025_reference --test p026_reference --test p026_transactions --locked
cargo test -p synveil-auth --test p027_postgres --locked -- --ignored --test-threads=4
cargo test -p synveil-auth --test p029_audit_postgres --locked -- --ignored --test-threads=4
cargo fmt --all -- --check
cargo clippy -p synveil-auth --all-targets --locked -- -D warnings
bash scripts/validate-docs.sh
git diff --check
```

The local Rust 1.100.0 build emitted 40 existing deprecation warnings from vendored `sqlx-sqlite`/`time` code. These are separate from the scoped P029 lint result; dependency policy and vendored code were not changed. The P027/P028 exact-head CI failures and advisory history remain independent; see the delivery report for their exact run/job links. A queued, skipped, cancelled or incomplete job is not a pass. Native Windows acceptance remains independently blocked by P028's existing evidence limitations.

## Qualification gates and handoff

The reference implementation is `REFERENCE_QUALIFIED` for a pushed source head only when the focused Linux and Windows jobs pass. The first exact-head hosted qualification passed at `a9142118ce620017c2798128b590d7a1afae3d03`; PR #190 records the latest exact-head result. The roadmap scope remains provisional. Production security remains **BLOCKED** on approved role/policy semantics; retention/privacy/access rules; independent integrity anchor; production schema/migration/rollback; production call-site and API review; and independent security review. This work makes no production authority change.

P030 may safely consider only the reviewed P027 transaction/outbox boundaries and this test-only evidence contract as context. Reconcile the authoritative roadmap before assigning P030 scope. Maintainer approval is still required for P004-D05/D06 and any production audit policy. Preserve separate P027 SQLx/dependency advisories and failures, P028 native acceptance blockers, and P036 signing dependencies; this PR does not resolve them. Do not merge this PR or start P030 from this checkpoint.
