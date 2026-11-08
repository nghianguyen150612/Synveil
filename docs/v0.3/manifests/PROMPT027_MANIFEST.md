# P027 manifest

Status: provisional reference qualification. Production RBAC, role tables, approval endpoints, token issuance and pairing activation remain disabled. P027 does not approve production security or start P028.

## Starting state and delivery

- Repository: `nghianguyen150612/Synveil`.
- Target: `develop/v0.3`; exact verified base `c55ca38eb3fcfac030ad90e9863a5d16475e3fd0`.
- P026 merged [PR #187](https://github.com/nghianguyen150612/Synveil/pull/187); source `b772943fa5bc98f3f32bcfb7ff8db846e852e9ec` verified ancestor.
- Branch: `feat/v03-p027-authority-ledger-reference`, initially identical to base.
- Workspace: `/workspace/Synveil`, fresh clean clone, one worktree. No existing P027 implementation or PR found. No unrelated changes, reset, force push or history rewrite.
- Authoritative P001–P060 roadmap unavailable; scope remains provisional.
- Implementation/final commit identifiers and the single Draft PR link are recorded in the PR delivery ledger. A commit cannot embed its own SHA. Exact-head hosted results are recorded separately from preceding-head results.

## Changed files

| File | Purpose |
|---|---|
| `crates/auth/tests/support/p027.sql` | Private PostgreSQL schema, fixture identities, persisted ownership/roles/delegation, immutable grants/tombstones, approvals, idempotent operation outcomes, audit and synthetic outbox. |
| `crates/auth/tests/support/p027.rs` | Test-only SERIALIZABLE adapter, current authority reconstruction, P026 evaluation, bounded conflict retries, transactional consumption/execution and recovery. |
| `crates/auth/tests/p027_postgres.rs` | 14 real PostgreSQL qualification tests; independent connections, ordered contention, negative matrices, backend/process failure, reconnect and optional external database restart probe. |
| `scripts/qualify-p027-postgres-regressions.sh` | Six auth and three API/metadata PostgreSQL regressions, each in a new disposable database; requires exactly one executed test per case. |
| `.github/workflows/v03-p027-postgres-authority-ledger.yml` | Exact source checkout; Linux PostgreSQL 17 repeated tests and actual server restart; Windows harness compilation and contract/installer/sync regressions. |
| `docs/v0.3/security/P027_POSTGRES_AUTHORITY_LEDGER_QUALIFICATION.md` | Production boundary audit, schema/lock/recovery contracts, inventory, limitations and open gates. |
| This manifest | Reproducible validation and delivery evidence. |

Production source, migrations and P025/P026/P020–P024 contracts remain unchanged.

## Local validation ledger

Executor: Debian 13 Linux, Rust/Cargo 1.99.1. Disposable loopback-only Docker PostgreSQL 17.11, image digest `sha256:ff87138949ad9a46f32ff23bd6852b52ce024a13dffe9b4087be98292cdd3b4f`. This is Linux evidence, not Windows-hosted PostgreSQL race evidence.

| Check | Actual observed result at implementation commit |
|---|---|
| Initial P027 PostgreSQL qualification | 10/10 passed; expanded suite then 13/13 passed, including genuine process exit and database lock/error probes. Final 14-test suite and repeat count pending below/PR ledger. |
| P026 reference matrix | 5/5 passed, original 55-case negative matrix retained. |
| Final repeated P027, actual database restart, PostgreSQL regressions | In progress; not yet counted as PASS. |
| Eight boundary package library/integration/doc regressions | In progress; not yet counted as PASS. |
| Strict Clippy, formatting, docs, whitespace | In progress; not yet counted as PASS. |
| Hosted final-head checks | Not yet observed. Pending, failed, cancelled, skipped or unavailable jobs are not passes. |
| Actual Windows-hosted PostgreSQL races | UNAVAILABLE in existing Linux service-container infrastructure. Windows compilation/contracts are a distinct gate. |

The initial broad build encountered missing libdbus development metadata, then exhausted local disk while linking many integration executables. Development packages were downloaded from the configured Debian snapshot and extracted under `/workspace/scratch/p027-deps`; no system package policy or product dependency was changed. Only this task's generated Cargo outputs were removed. Debug info/incremental output are disabled locally, and regressions are batched to fit the workspace. The failed build attempts are infrastructure failures, not passing tests.

Reproduction with PostgreSQL 17 and standard build dependencies:

```sh
export SYNVEIL_P027_DATABASE_URL=postgresql://USER:PASSWORD@127.0.0.1:PORT/DISPOSABLE_DATABASE
export SYNVEIL_P027_DISPOSABLE=1
cargo test -p synveil-auth --test p027_postgres --locked -- --ignored --test-threads=4 --nocapture
cargo test -p synveil-auth --test p025_reference --test p026_reference --test p026_transactions --locked
export SYNVEIL_TEST_DATABASE_URL="$SYNVEIL_P027_DATABASE_URL"
bash scripts/qualify-p027-postgres-regressions.sh
cargo test -p synveil-auth -p synveil-api -p synveil-metadata -p synveil-client -p synveil-client-sync -p synveil-server-network -p synveil-pairing-contracts -p synveil-install-engine --locked
cargo clippy -p synveil-auth -p synveil-api -p synveil-metadata -p synveil-client -p synveil-client-sync -p synveil-server-network -p synveil-pairing-contracts -p synveil-install-engine --all-targets --locked -- -D warnings
cargo fmt --all -- --check
bash scripts/validate-docs.sh
git diff --check
```

Database restart protocol: set `SYNVEIL_P027_RESTART_FILE` to a task-private absolute file. Run the exact `durable_reconnect_and_database_restart_probe` with `SYNVEIL_P027_RESTART_PHASE=prepare`, restart **only the disposable test server**, wait for readiness, then run with `SYNVEIL_P027_RESTART_PHASE=verify`. The focused workflow implements this sequence. It validates ordinary server restart, not power-loss or forced database crash. Normal suite execution performs reconnect without claiming a server restart.

## Hosted gates and release limitations

Required exact-head evidence: P027 Linux live PostgreSQL and Windows contracts; P025/P026; P020–P024 compatibility; global Rust CI; existing PostgreSQL owner/device authorization; Windows installer/client-sync gates. Results must identify source SHA, run URL, job and state. The PR remains Draft and unmerged. A skipped mandatory job does not qualify its workflow as green.

Open security/release gates: P004-D05/D06/D09 and all remaining decisions, D12/D13 pairing trust/lifecycle, exhaustive maintenance call-graph audit, authenticated production authority issuance, approval/audit/privacy/retention and external-effect/outbox integration, independent security review, P028 real native Windows acceptance, P036 publisher signing, physical Android/iOS pairing/recovery, real LAN/Tailscale, roadmap reconciliation, historical missing-LICENSE and installer secret-marker root-cause investigations. Recent resilience successes do not resolve those historical investigations.

Transactional PostgreSQL writes prove reference consistency only. Database administrators can defeat application triggers; no tamper-evident audit, external-effect atomicity, general production authority provenance or Windows native acceptance is claimed.
