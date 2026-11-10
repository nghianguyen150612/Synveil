# P027 manifest

Status: provisional reference qualification. Local Linux PostgreSQL evidence is complete; hosted closeout evidence is tracked per exact source head in PR #188. Production RBAC, role tables, approval endpoints, token issuance and pairing activation remain disabled. P027 does not approve production security or start P028.

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
| Actual Windows-hosted PostgreSQL races | The original head had no live Windows evidence. Closeout adds a separately gated, real Windows 2025 PostgreSQL 17.11 job; its exact final-head result is maintained in PR #188. Compilation/contracts remain distinct. |

The five baseline failures all report `Failed to create directory '/run/systemd/': Permission denied` from host `systemd-analyze verify` in this unprivileged executor:

- `linux_deployment_adversarial_units::adversarial_sandbox_negative_surface_locked`.
- `linux_install_lifecycle::systemd_artifacts_match_validated_definitions`.
- `linux_sandbox_hardening_units::hardened_unit_verifies_cleanly`.
- `linux_service_identity_units::systemd_units_verify_cleanly`.
- `scheduled_maintenance_lifecycle_units::systemd_units_parse_cleanly_with_analyze_if_available`.

Their original failures remain BLOCKED results at that executor checkpoint. Their tests and production units are unchanged. The closeout re-executed all five successfully in the isolated real-verifier environment described below; that new evidence does not relabel the earlier failures or qualify native service execution.

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

The closeout adds the test-only Windows PostgreSQL lifecycle/guard scripts and the separate live Windows workflow job described in the qualification document. It preserves all 15 PostgreSQL tests, P026's 55-case matrix and the passing Linux/Windows contract jobs. On source SHA `01bf7f061f225b1e263caa9cf81cfe9d0d8899fd`, focused [P027 run 42489705578](https://github.com/nghianguyen150612/Synveil/actions/runs/42489705578) completed **3/3 jobs successfully**: Linux PostgreSQL, Windows PostgreSQL 17.11, and Windows compilation/contracts. The Windows live job ran the same 15-test suite five times (75 passing executions), passed 13 provisioning/process guard negatives, and verified state across an actual PostgreSQL process restart. The exact job links, source checkout, image identity, and scope are recorded in the PR #188 ledger. This is reference qualification, not production authorization acceptance.

On that same source tree, [Rust CI push run 42489701033](https://github.com/nghianguyen150612/Synveil/actions/runs/42489701033) and [PR run 42489705766](https://github.com/nghianguyen150612/Synveil/actions/runs/42489705766) each passed all 11 jobs. P020–P026 PR workflows and both P015 PostgreSQL/ObjectStore runs passed. The pinned [AppImage run 42489701051](https://github.com/nghianguyen150612/Synveil/actions/runs/42489701051), [Windows installer 42489701050](https://github.com/nghianguyen150612/Synveil/actions/runs/42489701050), installer security (7/7), and resilience (10/10) passed without changes to acquisition pins or verifiers. The [Linux clean-machine workflow 42489705718](https://github.com/nghianguyen150612/Synveil/actions/runs/42489705718) passed its fixture build and adapter jobs; two native VM/Phase-C jobs were skipped because this run had no approved release-candidate input. These details qualify only their executed scopes.

The separate PostgreSQL 17 scheduled-maintenance PR workflow [42489705692](https://github.com/nghianguyen150612/Synveil/actions/runs/42489705692) had one failed first attempt in `scheduler_failure_prevents_worker_step`: two independently evaluated `clock_timestamp()` assignments intermittently violated the existing effective-time constraint in test setup. Its same-source push run [42489701109](https://github.com/nghianguyen150612/Synveil/actions/runs/42489701109) passed. The failed PR job was rerun without source changes; see the PR ledger for that attempt's conclusion. No P067 production code or test was changed in this P027 closeout.

Server self-host acceptance [run 42489705681](https://github.com/nghianguyen150612/Synveil/actions/runs/42489705681) remains blocked: `artifact-identity` failed with `BLOCKED_BY_EXTERNAL_INPUT: trusted publisher keyring variable is absent`; three dependent P036 jobs were cancelled while waiting for their self-hosted labels (`synveil-ubuntu-24.04-p036`, `synveil-fedora-42-p036`, and `synveil-p036-vm-control`), and the final gate failed. No trust key or signed production artifact is synthesized to bypass those P036 gates. A successful contract/source job does not qualify those missing native/artifact jobs.

Closeout source changes: `.github/workflows/v03-p027-postgres-authority-ledger.yml`, `scripts/p027-windows-postgres.ps1`, `scripts/test-p027-windows-postgres.ps1`, this manifest and the P027 security qualification. Production Rust, SQL authority semantics, acquisition scripts and lock files remain unchanged.


### Closeout local qualification and evidence interpretation

The closeout re-ran all library/integration/binary/doc tests for the eight boundary packages in one normal Cargo invocation: **1,456 passed, zero failed, 454 default-ignored**. Five complete P027 PostgreSQL suites additionally passed: **75 executions**, including genuine process exits and PostgreSQL conflicts. The fresh-database regression script passed **9/9** again. Of the 454 ignored default cases, 15 P027 and 9 historical tests were selected explicitly; the other 430 are not counted as passing. All 15 P027 test functions and P026's 55-row matrix remain unchanged.

The acquisition/runtime/manifest/channel/installer/Linux invocation and lifecycle Python suites passed **277 tests**, with zero failures or skips, plus the Windows installer, installer security and resilience structural validators. All eight boundary packages passed strict all-target Clippy with `-D warnings`; workspace formatting, all 24 documentation sections and Git whitespace passed. The two new PowerShell files parsed successfully under PowerShell 7.5.4 and their pure version/inventory/provenance negatives passed locally. Positive Windows provisioning or database behavior is counted only from the hosted Windows job.

For the five former systemd failures, the local executor still has an unprivileged host `/run`; no host permission guard was changed. A task-private, network-disabled container ran the exact installed Debian `systemd-analyze` 257.13-1~deb13u1 binary and its actual shared-library dependencies, with a writable private `/run`. Binary SHA-256: `d89a879c2c87c4465e3a09588e18f27a14dd9228898679bbdbbc978802f64dce`. The command proxy copied each requested real unit input into that container, preserved its bytes/mode, and checked a round trip for exact byte equality before invoking the same system-scope verifier with the original arguments. Host unit files, test assertions, production security directives and host permissions were untouched; no dummy verifier, forced success or `--user` fallback was used. All five exact assertions passed inside the full Cargo suite. This qualifies static unit verification in that environment, not systemd service execution, genuine native logon or P036 acceptance. The local proxy and logs remain in `/workspace/scratch/p027-systemd-bin` and `/workspace/scratch/p027-closeout-*.log`.

Local storage exhausted while preparing the extra verifier. Only task-generated artifacts were managed: the task's PostgreSQL container filesystem was exported, its database volume preserved, and the same root filesystem re-imported as a single-layer local fixture to remove Docker VFS intermediate copies. The PostgreSQL executable hash before and after was identical: `5fa5f4cb92953e720a6814653888183abd7988d31a505125dec3a12bd51f29be`. The database data was not reset, no upstream artifact pin was changed, and subsequent PostgreSQL tests used the same 17.11 executable. This storage operation is not counted as an additional crash/recovery test.

Closeout implementation began at `d20f1412e9279a697b5d239b8b580e11a994a4ba`. The first Windows live job failed closed before database execution because `windows-2025` selected the reviewed `win25-vs2026` image rather than `win25`; `327538025e1275530b3dfc37210830d9b59fc5da` explicitly added that independently checked image variant, retaining PostgreSQL 17.11. Its guard negatives passed, then `initdb` returned exit 1 while the original fixture helper discarded its native output. `e944b0c49d4f265adc1eb2e1818f2943a1306de7` retains bounded native diagnostics and fixture provenance on failure, and grants the new fixture's owner SID access directly instead of depending on an inherited Administrators ACL. An unobserved cause for the discarded native output is not asserted. Failed/superseded attempts do not count as Windows database passes.

The final source SHA, local final-head rechecks, and complete exact-head hosted workflow/job matrix are recorded in the [PR #188 delivery ledger](https://github.com/nghianguyen150612/Synveil/pull/188). That ledger distinguishes successful reference jobs, recovered artifact/candidate jobs, unavailable native/publisher inputs, failures, cancellations, skips and queued jobs. A successful candidate build does not approve P028; a missing P036 trust input or runner is not a P027 database defect. The PR remains Draft and unmerged, and all production/security gates listed above remain open.


The Windows fixture controller also uses explicit bounded native process invocation with argument arrays, a 120-second parent-process wait, nonzero-exit rejection and bounded diagnostics. It does not wait indefinitely for output EOF inherited by the persistent PostgreSQL child of `pg_ctl`. Actual server-version checks, all 15 tests and restart recovery remain required. The guard suite includes native nonzero-exit and timeout negatives (13 total provisioning/process guard cases); the process helper negatives also passed locally without claiming Windows database evidence. A trailing-space formatting defect in that test script was corrected in a follow-up commit; final diff whitespace is validated against the integration base as well as the worktree.
