# P026 manifest

Status: provisional integration-only reference qualification. Production RBAC, role persistence, approval endpoints and token issuance remain disabled. P004-D05/D06/D09 are not approved. Roadmap reconciliation remains open.

Base: `14c2dbf51464d6893c1f19a8c1f8e041be5b338e` (P025 PR #186 merge).
P025 source: `54918f457784edb29b52dcbdac79c3539e33a3d8`, verified ancestor.
Branch: `feat/v03-p026-rbac-policy-delegation-qualification`; target `develop/v0.3`.
Worktree: `/workspace/Synveil-p026`; original clean `work` checkout preserved.

## Files

- `crates/auth/src/authorization_reference.rs`: v2 reference module declaration.
- `crates/auth/src/authorization_reference/rbac_v2.rs`: versioned typed fixture policy, permissions, bounded delegation and policy-derived approval; reuses P025 authentication/legacy rules.
- `crates/auth/tests/p026_reference.rs`: 5 deterministic matrix tests, 55 enumerated negative rows plus positive/compatibility/OS assertions.
- `crates/auth/tests/p026_transactions.rs`: 5 independent-connection SQLite qualification tests (including subprocess termination).
- `crates/auth/tests/support/p026.rs`: trusted fixture actors/ownership/assignments.
- `crates/auth/tests/support/p026.sql`: isolated test-only tables, tombstones and audit; no production migration.
- `.github/workflows/v03-p026-rbac-policy-delegation.yml`: exact-head Ubuntu/Windows reference and regression checks.
- [P026 security qualification](../security/P026_RBAC_POLICY_DELEGATION_QUALIFICATION.md): production authority inventory, trust contracts, PostgreSQL requirements and open gates.
- This manifest.

## Validation ledger

Implementation commits: `6bdaf3c0a643c74ac0ee2e3d08c52382cdf4166b` and `b91c495afc97e6b3c4444387c8488c2fa3a6b564`. Final delivery adds only this validation ledger; final SHA and exact hosted run links are recorded in Draft PR #187 (a commit cannot embed its own SHA).

Local executor: Debian 13 Linux, Cargo 1.99.1, disposable PostgreSQL 17.11 in Docker. These are local Linux results, not hosted Ubuntu/Windows results.

| Check | Observed result |
|---|---|
| P025 / P026 matrix / SQLite contracts | 13 passed: 3 P025, 5 P026 reference, 5 transactional; 55 enumerated negative rows plus additional assertions; no ignored tests |
| Eight boundary library suites: auth, API, metadata, client, client-sync, server-network, pairing-contracts, install-engine | 691 passed, zero failed/ignored |
| Full auth, install-engine and pairing-contracts suites, including installer integration and doc tests | 665 passed; 6 default-ignored PostgreSQL auth cases subsequently executed explicitly and passed individually |
| Six auth PostgreSQL tests, each in its own fresh disposable database | 6 passed (bootstrap precision, bootstrap race/atomicity, browser sessions, device enrollment/revocation, concurrent exchange, expiry after device-lock wait) |
| Real API P005 owner/device PostgreSQL regression | 1 passed |
| Real API P015 PostgreSQL + ObjectStore owner-to-byte matrix | 1 passed; `P015_MATRIX_COMPLETE` observed |
| Real metadata owner-scoped immutable version history | 1 passed |
| Strict Clippy, eight boundary packages, all targets, `--locked -- -D warnings` | Passed after final source changes |
| Rust formatting, docs validation (all 24 sections), `git diff --check` | Passed |
| Focused exact-head hosted Ubuntu/Windows and global Rust CI | Required, not yet complete; remain Draft. Final exact-SHA status and run links in PR #187. No queued/incomplete/cancelled job counted as passing. |

Representative reproducible commands (standard dependencies required):

```sh
cargo test -p synveil-auth --test p025_reference --test p026_reference --test p026_transactions --locked
cargo test -p synveil-auth -p synveil-api -p synveil-metadata -p synveil-client -p synveil-client-sync -p synveil-server-network -p synveil-pairing-contracts -p synveil-install-engine --lib --locked
cargo test -p synveil-auth -p synveil-install-engine -p synveil-pairing-contracts --locked
cargo clippy -p synveil-auth -p synveil-api -p synveil-metadata -p synveil-client -p synveil-client-sync -p synveil-server-network -p synveil-pairing-contracts -p synveil-install-engine --all-targets --locked -- -D warnings
cargo test -p synveil-api --test authorization_postgres --locked -- --ignored --test-threads=1
cargo test -p synveil-api --test p015_postgres_objectstore_authorization --locked -- --ignored --test-threads=1 --nocapture
cargo test -p synveil-metadata --test postgres --locked postgres_version_history_is_owner_scoped_keyset_paged_and_pointer_based -- --ignored --test-threads=1
cargo fmt --all -- --check
bash scripts/validate-docs.sh
git diff --check
```

Each ignored auth test must run separately with `SYNVEIL_TEST_DATABASE_URL` pointing to its own fresh database, using its exact test name and `--exact --ignored --test-threads=1`. Reusing the already populated P005/P015 database caused an initial bootstrap `BootstrapStateInvalid` failure; rerunning all six with fresh databases passed. This was test setup, not a product change.

Initial broad builds were blocked by absent libdbus development packages and then disk exhaustion. Resolved using isolated Debian libdbus/systemd development packages under `/workspace/scratch/p026-deps`, standard pkg-config search/sysroot variables, and `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`. Only this task's generated Cargo output was cleaned; no product behavior or security guard was changed. No Qt workspace Clippy pass or native acceptance is claimed; unchanged global Rust CI must supply its actual result.

## Open requirements and release blockers

P025 inherited audit/provenance/approval/PostgreSQL concurrency/integration matrix remain incomplete for production. SQLite tests qualify fixture state only. Power-loss/OS-crash/fsync, production authority issuance, PostgreSQL locks/isolation and external effects/outbox still require qualification. Internal maintenance adapter call-graph audit remains open. Test fixture roles do not define approved production roles.

P028 native Windows interactive GUI, genuine logon and installed-component IPC evidence unavailable. Windows resilience run `41627469609`: missing installed LICENSE. Historical run `41437684142`: `INSTALLER_VERIFY_FAILURE: secret-like marker in output`. Distinct blockers; no verification weakened. Signing, mobile physical-device, real-network and independent security approval remain release gates.

Exactly one P026 Draft PR is required; no merge and no P027 work.

## Windows fixture lifecycle repair

Starting PR #187 head was verified live as `5cc9b314b34ceacd5b759c9312e5a21972f86346`; original checkout and clean P026 worktree preserved. Original focused run `41745053251` passed Ubuntu but failed all five Windows transactional tests at database removal (job `126296298854`, OS error 32); global Rust CI run `41745053229`, Windows job `126296491997`, had the same failures. P024 run `41745053153` separately failed `host10_new_process_host_restarts_from_the_same_durable_state` during fixture-directory removal.

P026 root cause: teardown relied on `SqlitePool::close()` immediately followed by unlink. A Linux executable probe of `/proc/self/fd` after that await found live main database, WAL and SHM descriptors, even though Linux unlink succeeded. SQLx 0.8.6 returns pooled connections through asynchronously spawned return work and SQLite connections use worker threads; pool shutdown alone did not give this fixture a sufficient file-handle release boundary. This is not merely an orphan WAL filename or a still-running crash subprocess. The subprocess uses `Command::status()` (wait/reap), exits with code 26, and recovery assertions precede teardown. Transactions commit/rollback explicitly; the deliberate connection-loss transaction is closed explicitly.

Repair in `crates/auth/tests/p026_transactions.rs`: before every normal close or restart, acquire all pool permits while retaining the acquired connections. This waits for all outstanding fixture work to return and prevents simultaneous query use. Run `PRAGMA wal_checkpoint(TRUNCATE)` and assert no busy readers/writers, then explicitly await each `PoolConnection::close()` (SQLite worker shutdown acknowledgment), then close the empty pool. The same executable descriptor probe showed no matching live handles after this sequence. Before unlink, assert WAL/SHM sidecars are gone; report database path and cleanup error on failure. No retry, sleep, cleanup suppression, in-memory substitution or WAL removal was introduced. Each test retains its UUID-named database; no other test's files are touched. Crash tests still terminate without destructors and recover committed authority before normal teardown.

P024 root cause: the second `DesktopSyncHost` was shut down but still alive when `HostFixture::close()` ran. `HostInner.state` retained an `Arc<LocalStateStore>`; that object owns `writer_lock: File`, released only on final drop. The existing ten-second cleanup retry cannot release a handle whose owner remains in scope. This test restarts host objects within one OS process, not an external child; it has no child process to kill or reap. Repair in `crates/client-sync/src/host.rs`: drop the event subscription and second host after awaited shutdown, then assert the fixture is the sole state owner before removing its files. Durable-state and cycle assertions remain unchanged. No shared cleanup helper or production sync/credential/authorization code changed.

Initial repaired local checks: all 13 P025/P026 contracts and the exact host10 test passed. Repeated concurrency, full regressions and exact final-head hosted status are recorded below/in the existing Draft PR as validation completes. Hosted Linux success is not Windows evidence; pending/cancelled/skipped runs are never passes. Production RBAC/persistence/approvals/token issuance and all previously listed security/release gates remain disabled or unresolved.
