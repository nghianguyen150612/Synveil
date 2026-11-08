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


## Hosted recovery closeout

Starting checkpoint: clean `a42de8538e051107f68d222cc0bbc7d8a6c89bd9` on the existing P027 branch/PR #188; base/P026 ancestry unchanged. Live hosted verification confirmed P027 Linux transactions/recovery, Windows compilation/contracts, all 11 global Rust CI jobs, P020–P026 and P015 passed on that checkpoint. Earlier queued snapshots above are historical, not their current conclusions.

Original acquisition failures shared two implementations: AppImage/clean-machine used `scripts/acquire-appimage-runtime.py`; Windows installer/security/resilience/native-candidate used `scripts/build-windows-installer.ps1`. The Python downloader allowed at most five redirects, approved HTTPS hosts and a 30-second socket timeout, with no acquisition retry. Inno used three attempts, 90 seconds per request and 2/4-second waits; its HttpClient follows HTTPS delivery redirects. Neither acquisition path trusted an existence-only shared cache. Both staged downloads before identity validation; AppImage published only after all verification, and Inno executed only after its locked SHA-256 matched. The old catch-all errors suppressed HTTP status/exception details: their original specific HTTP/DNS/TLS/timeout causes cannot be reconstructed from the retained logs and must not be invented.

The original pinned endpoints were probed without changing sources: all returned HTTP 200 locally in approximately 0.4–1.0 seconds, with redirects delivering assets from `release-assets.githubusercontent.com`. Exact bytes and existing verification succeeded:

| Identity | Existing pin |
|---|---|
| AppImage runtime release/source | `AppImage/type2-runtime`, tag `20251108`, source `dd6cebedcbddde9c82f89b011e8e1d40b6e43868` |
| Runtime endpoint | `https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64` |
| Runtime size/SHA-256 | 944,632 bytes; `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d` |
| Detached signature SHA-256 | `de8262d60b5f82bcff11b77598aedef86d2c375fc246d6a675f6cac68b6dfda7` |
| Public key SHA-256 / signer | `db8b615eb5bbf5e8418d52906b4a492960926370e2c5041ad6acaccda56ef0c6` / `570C77ACEA40C0F1B758902CBF96CCA56490F695` |
| License SHA-256 | `aa154fc9070614bbe7921f89db11efd1dba7a1f3a41685958110e2230f9c0ca1` |
| Inno Setup version/endpoint | `6.7.3`; `https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe` |
| Inno size/SHA-256 | 10,592,232 bytes; `9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732` |

The unchanged AppImage acquisition completed local ELF, release/tag metadata, digest, license and GnuPG signer verification. Attempt 2 of the same six hosted runs was requested before source edits. AppImage `42465362183`, clean-machine `42465362222`, installer security `42465362206`, and installation resilience `42465362246` recovered without acquisition changes. Windows installer `42465362321` and native candidate `42465362618` were still executing at this documentation checkpoint; final outcomes are recorded in the PR's exact-head job ledger. This demonstrates recovered availability, not the unavailable original HTTP error code. No retry/cache/mirror/pin/signature/trust-root modification was made to either acquisition implementation.

The closeout adds the test-only Windows PostgreSQL lifecycle/guard scripts and the separate live Windows workflow job described in the qualification document. It preserves all 15 PostgreSQL tests, P026's 55-case matrix and the passing Linux/Windows contract jobs. New source/implementation commit identifiers and the exact final-head run/job matrix are recorded in [Draft PR #188](https://github.com/nghianguyen150612/Synveil/pull/188), including the distinction between actual Windows-hosted PostgreSQL evidence and compilation. Until a live job finishes successfully, it remains unqualified.

Server self-host acceptance `42465362141` is independently blocked: `artifact-identity` failed with `BLOCKED_BY_EXTERNAL_INPUT: trusted publisher keyring variable is absent`. The Ubuntu/Fedora/VM-control jobs await self-hosted labels `synveil-ubuntu-24.04-p036`, `synveil-fedora-42-p036`, and `synveil-p036-vm-control`. No trust key or signed production artifact is synthesized to bypass those P036 gates. A successful contract/source job does not qualify those missing native/artifact jobs.

Closeout source changes: `.github/workflows/v03-p027-postgres-authority-ledger.yml`, `scripts/p027-windows-postgres.ps1`, `scripts/test-p027-windows-postgres.ps1`, this manifest and the P027 security qualification. Production Rust, SQL authority semantics, acquisition scripts and lock files remain unchanged.
