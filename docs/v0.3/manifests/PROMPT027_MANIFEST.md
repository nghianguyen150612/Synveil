# P027 manifest

Status: provisional reference qualification. Local Linux PostgreSQL evidence is complete; final-head hosted qualification remains required. Production RBAC, role tables, approval endpoints, token issuance and pairing activation remain disabled. P027 does not approve production security or start P028.

## Starting state and delivery

- Repository: `nghianguyen150612/Synveil`.
- Target: `develop/v0.3`; exact verified base `c55ca38eb3fcfac030ad90e9863a5d16475e3fd0`.
- P026 merged [PR #187](https://github.com/nghianguyen150612/Synveil/pull/187); source `b772943fa5bc98f3f32bcfb7ff8db846e852e9ec` verified ancestor.
- Branch: `feat/v03-p027-authority-ledger-reference`, initially identical to base.
- Workspace: `/workspace/Synveil`, fresh clean clone, one worktree. No existing P027 implementation or PR found. No unrelated changes, reset, force push or history rewrite.
- Authoritative P001–P060 roadmap unavailable; scope remains provisional.
- Single [Draft PR #188](https://github.com/nghianguyen150612/Synveil/pull/188), targeting `develop/v0.3`, remains unmerged.
- Implementation: `04b4cbed8bf2adfe51a1b4677bcc55a94260a18c`; cycle hardening: `aa3b38d6de4faa63fac2a91856cf0816fe2c357b`; final Rust/SQL test source: `dcbc651ec41d3552faec698680221d15f4e541cc`; evidence-output repair: `79ae1a33e026501534b4766bffe01bbdca77f844`.
- Final delivery SHA and exact-head hosted states are recorded in the PR body. A commit cannot embed its own SHA. Preceding-head results do not qualify the final head.

## Changed files

| File | Purpose |
|---|---|
| `crates/auth/tests/support/p027.sql` | Private PostgreSQL schema, fixture identities, persisted ownership/roles/delegation, immutable grants/tombstones, approvals, idempotent operation outcomes, audit and synthetic outbox. |
| `crates/auth/tests/support/p027.rs` | Test-only SERIALIZABLE adapter, current authority reconstruction, P026 evaluation, bounded conflict retries, transactional consumption/execution and recovery. |
| `crates/auth/tests/p027_postgres.rs` | 15 real PostgreSQL qualification tests; independent connections, ordered contention, negative matrices, backend/process failure, reconnect and optional external database restart probe. |
| `scripts/qualify-p027-postgres-regressions.sh` | Six auth and three API/metadata PostgreSQL regressions, each in a new disposable database; requires exactly one executed test per case. |
| `.github/workflows/v03-p027-postgres-authority-ledger.yml` | Exact source checkout; Linux PostgreSQL 17 repeated tests and actual server restart; Windows harness compilation and contract/installer/sync regressions. |
| `docs/v0.3/security/P027_POSTGRES_AUTHORITY_LEDGER_QUALIFICATION.md` | Production boundary audit, schema/lock/recovery contracts, inventory, limitations and open gates. |
| This manifest | Reproducible validation and delivery evidence. |

Production source, migrations and P025/P026/P020–P024 contracts remain unchanged.

## Local validation ledger

Executor: Debian 13 Linux, Rust/Cargo 1.99.1. Disposable loopback-only Docker PostgreSQL 17.11, image digest `sha256:ff87138949ad9a46f32ff23bd6852b52ce024a13dffe9b4087be98292cdd3b4f`. This is Linux evidence, not Windows-hosted PostgreSQL race evidence.

| Check | Actual observed result |
|---|---|
| Final P027 PostgreSQL suite | 15 tests × 10 complete repeated suites = 150 passed, zero failed/ignored. Includes 10 real child-process exits, backend termination, SERIALIZABLE conflicts, actual deadlocks, ordered revocation/expiry, negative scope/provenance matrix and single-use approval/idempotency. Same final Rust/SQL source throughout; workflow-only evidence repair occurred during repetition. Earlier 10/13/14-test passes are historical and are not substituted for this count. |
| Actual database server restart | Both prepare and verify phases passed; Docker PostgreSQL start time changed from `2026-11-08T08:29:12.817791371Z` to `2026-11-08T08:57:31.382895992Z`. Committed grant, revocation, approval consumption, operation, audit/outbox and status recovery survived. This was an ordinary server restart, not power loss. |
| Fresh-database PostgreSQL regressions | Full final regression script: 9/9 passed, each in a newly created disposable database: 6 auth/bootstrap/session/device tests; P005 owner/device; P015 joined router/ObjectStore matrix (`P015_MATRIX_COMPLETE`); metadata owner-scoped immutable version history. |
| P025/P026 reference and SQLite | 13/13 passed (3 + 5 + 5); original 55 negative reference rows unchanged. Included in regression counts below. |
| Eight boundary library suites | 691 passed, zero failed/ignored: auth, API, metadata, client, client-sync, server-network, pairing-contracts and install-engine. |
| Complete integration/binary/doc target inventory for those packages | 90 targets evaluated separately to fit disk: 760 passed, 5 failed due to unavailable systemd runtime, 439 existing default-ignored cases. New P027 target was excluded here and explicitly run above. Of the historical ignored cases, 9 were subsequently selected and passed in real PostgreSQL; the other 430 are not claimed as passes. |
| Combined existing regression result | 1,451 passed, 5 environment-blocked failures; no test weakened, new ignore or suppressed failure. This is not a green full-suite result. |
| Release-artifact source boundary | 9/9 passed after moving generated qualification evidence under ignored `target/p027-evidence`. The initial untracked-source rejection was real and corrected without weakening source validation. |
| Strict Clippy | All targets of the eight boundary packages passed `--locked -- -D warnings` on final Rust/SQL source. Whole-workspace Qt Clippy is left to global hosted CI, not claimed locally. |
| Formatting, docs and whitespace | Workspace rustfmt, all 24 documentation validation sections, shell syntax and Git whitespace passed. Final delivery checks are also recorded in the PR body. |
| Hosted exact final-head gates | Required; status/run links recorded in PR #188. Queued/in-progress, failed, cancelled, skipped and unavailable checks are distinct from passes. |
| Actual Windows-hosted PostgreSQL races | UNAVAILABLE in existing Linux service-container infrastructure. Windows compilation/contracts are a separate hosted gate. |

The five baseline failures all report `Failed to create directory '/run/systemd/': Permission denied` from host `systemd-analyze verify` in this unprivileged executor:

- `linux_deployment_adversarial_units::adversarial_sandbox_negative_surface_locked`.
- `linux_install_lifecycle::systemd_artifacts_match_validated_definitions`.
- `linux_sandbox_hardening_units::hardened_unit_verifies_cleanly`.
- `linux_service_identity_units::systemd_units_verify_cleanly`.
- `scheduled_maintenance_lifecycle_units::systemd_units_parse_cleanly_with_analyze_if_available`.

Their tests and production units remain unchanged. These are BLOCKED locally, not PASS. Hosted Linux must supply its actual results.

The initial broad build encountered missing libdbus development metadata, then exhausted disk while linking many integration executables. Development packages were downloaded from the configured Debian snapshot and extracted under `/workspace/scratch/p027-deps`. Only this task's generated Cargo outputs were cleaned; large non-executable Rust cache artifacts were moved to task-private `/dev/shm/synveil-p027-rust-cache` with links in `target`, freeing disk for normal executable output. Debug info/incremental output were disabled and targets executed in batches. No product dependency, system security guard or test assertion changed.

An initial PostgreSQL bootstrap run returned connection-unavailable errors while independent live qualification was overlapping; that attempt was not counted as a pass. A later local cleanup race prevented one metadata executable from starting. After serializing the live regression runner and retaining its binaries until completion, the full nine-case script passed against nine fresh databases. Qualification output initially made the release-source unit reject untracked inputs; the script/workflow now retain evidence under ignored build output, and that complete nine-test unit target passed afterward. Failed setup/build attempts are not test passes.

Local logs are retained under `/workspace/scratch/p027-*.log`, `/workspace/scratch/p027-batched`, and `target/p027-evidence`. The focused hosted workflow uploads its own exact-source evidence. No bearer credentials or real device material are included.

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

Observed preceding-head evidence: focused [run 42464989871](https://github.com/nghianguyen150612/Synveil/actions/runs/42464989871) on `79ae1a33e026501534b4766bffe01bbdca77f844` reached Linux live qualification and passed the Windows harness compilation/P025/P026 steps while the workflow was still in progress. Those intermediate steps are not a completed workflow pass. Earlier implementation-head runs were cancelled by subsequent pushes; they are not passes. Preceding-head AppImage [run 42464664534](https://github.com/nghianguyen150612/Synveil/actions/runs/42464664534) failed acquiring the pinned runtime with `BLOCKED_APPIMAGE_RUNTIME: upstream HTTPS request failed`; downstream jobs/steps were skipped, not qualified. Final-head results must be assessed independently.

Required exact-head evidence: P027 Linux live PostgreSQL and Windows contracts; P025/P026; P020–P024 compatibility; global Rust CI; existing PostgreSQL owner/device authorization; Windows installer/client-sync gates. Results must identify source SHA, run URL, job and state. The PR remains Draft and unmerged. A skipped mandatory job does not qualify its workflow as green.

Open security/release gates: P004-D05/D06/D09 and all remaining decisions, D12/D13 pairing trust/lifecycle, exhaustive maintenance call-graph audit, authenticated production authority issuance, approval/audit/privacy/retention and external-effect/outbox integration, independent security review, P028 real native Windows acceptance, P036 publisher signing, physical Android/iOS pairing/recovery, real LAN/Tailscale, roadmap reconciliation, historical missing-LICENSE and installer secret-marker root-cause investigations. Recent resilience successes do not resolve those historical investigations.

Transactional PostgreSQL writes prove reference consistency only. Database administrators can defeat application triggers; no tamper-evident audit, external-effect atomicity, general production authority provenance or Windows native acceptance is claimed.
