# PROMPT001A Manifest — CI & Security Gate Remediation

Status at this evidence checkpoint: **latest repair code checkpoint `7452d857072314498153475b87ce694724622e19`; local Rust and Web validation pass. Final-head hosted Rust CI is queued and applicable platform/package workflows are still running. PR #157 remains open and unmerged; P001 PR #156 has not been requalified.** This document does not certify Synveil v0.2.0 stable.

## Prompt identity and scope

- Prompt: `SYNVEIL v0.3 — PROMPT001A`, Phase A — Foundation.
- Scope: remediate the two Rust TLS trust advisories, correct Web recovery focus timing, and classify the existing Windows native acceptance gates.
- No v0.3 product features, database migrations, P028 implementation, workflow suppression, or P001 audit edits are included.
- P001A is a repair prompt; it does not advance roadmap numbering or begin P002.

## Baseline SHAs and Git state

- Repository: [nghianguyen150612/Synveil](https://github.com/nghianguyen150612/Synveil).
- Verified base: `origin/develop/v0.3` at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` when the repair branch was created.
- `origin/main` was also `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` at preflight; the refs were equal with no divergence.
- `fix/v03-p001a-ci-security` was absent remotely at preflight and was created from the verified base. No reset, rebase, force-push, or history rewrite was used.
- Repair code checkpoint: `8a5fa893395312a0839c409e7de350826c269c9b`.
- Latest repair code checkpoint, including the test-synchronization correction: `7452d857072314498153475b87ce694724622e19`.
- Separate commits: `4e186c42029fac7600a5da7d6dbb365ffe5a1d66` (Rust dependency policy) and `8a5fa893395312a0839c409e7de350826c269c9b` (Web focus restoration).
- P001 PR [#156](https://github.com/nghianguyen150612/Synveil/pull/156) remains open and unmerged at `244edff62f81f458bae32c1f4a10b544db623638` during this checkpoint. Its original four-file diff was revalidated separately; this repair branch does not modify those files.
- Repair PR [#157](https://github.com/nghianguyen150612/Synveil/pull/157) targets `develop/v0.3`, is open and unmerged, and has six changed files. The latest observed head is `7452d857072314498153475b87ce694724622e19`; the latest observed base remains `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`.
- Evidence collection date: 2026-10-23 UTC.

## Original CI failures and evidence

| Finding | Original evidence | Classification at this checkpoint |
|---|---|---|
| Rust dependency policy | [Rust CI run 39887640752, job 120199448064](https://github.com/nghianguyen150612/Synveil/actions/runs/39887640752/job/120199448064); duplicate on run 39887634885, job 120199644737 | `VERIFIED` on the repair code head: `cargo deny check` and hosted Dependency policy pass; patched graph contains neither vulnerable version |
| Web recovery focus | [Original run 39887640752, job 120199448280](https://github.com/nghianguyen150612/Synveil/actions/runs/39887640752/job/120199448280); later manifest-head [run 39918188913, job 120297238303](https://github.com/nghianguyen150612/Synveil/actions/runs/39918188913/job/120297238303) | AppShell recovery-heading repair passed on code head `8a5fa89`. The later Web run exposed a separate test-timing race in `BackupsPage.test.tsx`; the focus assertion now waits for the existing page-level effect. Local retest passes; hosted retest is pending. |
| P028 Windows native acceptance | [Windows native acceptance run 39887640690, job 120206304473](https://github.com/nghianguyen150612/Synveil/actions/runs/39887640690/job/120206304473) | `BLOCKED_BY_ENVIRONMENT` for interactive UI, real logon, and named-pipe end-to-end evidence; standard-user native acceptance passed |
| P044 Windows Inno interruption fixture | [Installation resilience run 39887640720, job 120199448080](https://github.com/nghianguyen150612/Synveil/actions/runs/39887640720/job/120199448080) | First attempt failed at the real Inno fixture. A rerun of that job passed, including evidence upload; the first attempt's exact cause remains unknown because its log could not be retrieved from the Actions log storage endpoint in this environment. |
| AppImage pinned runtime acquisition | [Repair PR run 39912679385, initial job 120279040608](https://github.com/nghianguyen150612/Synveil/actions/runs/39912679385/job/120279040608) | One attempt reported `BLOCKED_APPIMAGE_RUNTIME: upstream HTTPS request failed` before build or smoke. The exact same head passed a full independent run and the failed job's rerun; no pin, signature, host, or TLS check was weakened. |
| Recovery UX dependency policy | [Recovery UX run 39887640651, job 120199443111](https://github.com/nghianguyen150612/Synveil/actions/runs/39887640651/job/120199443111) | Same resolved Rust dependency graph; covered by the repair's `cargo deny check` and hosted rerun |

The P044 rerun on the original P001 run was job `120277478456` on run `39887640720`; the real Inno fixture and upload steps both passed. Its artifact was `p044-windows-inno-process-interruption`, ID `12483871251`, size 1123 bytes, SHA-256 `c981c93e51cd0afdd372f6af6da1bbc8a2c2c39a227562df68fe7f265f25a2f0`. A fresh P001A run also passed job `120279040572` on run `39912679237`, including `cargo build --release --locked -p synveil-client`, the real Inno interruption fixture, and evidence upload. The earlier one-off failure remains non-reproduced with unknown cause because its original log blob was inaccessible; neither successful rerun is represented as P028 GUI/logon evidence.

On the P001A head, AppImage run `39912661902` job `120278985212` passed the full build/reproduction/smoke/lifecycle flow. A separate attempt at run `39912679385`, job `120279040608`, failed only while acquiring the pinned signed upstream runtime due to an HTTPS request failure; rerunning that job as `120284279689` passed runtime acquisition, independent reproduction, artifact inspection, QML smoke, current-user lifecycle, ABI baseline, and evidence upload. The rerun artifact is `synveil-appimage-x86_64`, ID `12483859062`, 49,064,824 bytes, SHA-256 `572b6650cb9aa1c236914413cd011797ec391b850fcda4c4b7f6c1a18623fb16`. This is classified as a transient upstream fetch failure, not a package or security regression.

## Task A — Rust dependency security

The original lockfile resolved `rustls-webpki 0.103.15` and `webpki-roots 1.0.9`, matching RUSTSEC-2026-0382 and RUSTSEC-2026-0381. `cargo tree -i` showed `rustls-webpki` through `rustls 0.23.46` into `reqwest`/`hyper-rustls` and SQLx's Rustls stack; `webpki-roots` was resolved directly at 1.x through `reqwest`/`hyper-rustls`. No `webpki-roots 0.26.x` package was present in the resolved lockfile, so the separately resolved 1.x package was checked directly.

| Package | Before | After | Change |
|---|---:|---:|---|
| `rustls-webpki` | `0.103.15` | `0.103.16` | Cargo lock update; checksum now `4443b1b6d39d7c663139942cc4558262562d2c84123bb6282ef177009ca8b281` |
| `webpki-roots` | `1.0.9` | `1.0.10` | Cargo lock update; checksum now `1c1c8e20c8a38fc7d65abfe74eb7280e4aa667e8eec8bc84996d2874810748d9` |

The lockfile was regenerated with Cargo using `cargo update -p rustls-webpki@0.103.15 --precise 0.103.16` and `cargo update -p webpki-roots@1.0.9 --precise 1.0.10`. No other package or manifest was updated, and no direct dependency, major version, TLS feature, or verification policy changed. `Cargo.toml` retains `reqwest` with `default-features = false` and `rustls-tls`; SQLx retains `tls-rustls-ring-native-roots`. No invalid-certificate acceptance, custom trust logic, or advisory ignore was introduced. The exact-version license exceptions in `deny.toml` were advanced to the patched package versions so license checking remains narrow and active.

Compatibility review found `reqwest 0.12.28`, `rustls 0.23.46`, `hyper-rustls 0.27.9`, `tokio-rustls 0.26.4`, and SQLx `0.8.6` unchanged. The patched packages compiled in local server-host tests and hosted Ubuntu, macOS, and Windows workspace checks passed on the repair code head.

## Task B — Web focus restoration

The failed test was `[auth] suspends protected content on recovery network failure and resumes on user retry` in [`BackupOverviewPage.test.tsx`](../../../apps/web/src/features/backups/BackupOverviewPage.test.tsx). The app shell restored the page heading from a passive `useEffect`; the protected page could be rendered and discovered before that effect ran, leaving `document.body` focused at the test assertion. This is an asynchronous focus-restoration race after recovery, not a changed recovery authorization decision or an incorrect heading target. P001 changed only audit files, so the failure was present on the pre-repair source baseline. The local pre-fix test passed 20 repetitions, consistent with a timing-sensitive failure.

[`AppShell.tsx`](../../../apps/web/src/components/layout/AppShell.tsx) now performs the existing heading selection/focus in `useLayoutEffect` after the authenticated route DOM commits and before paint. Its early return while status is `recovering` or `recovery_error` is unchanged. The focused test now holds the successful session response pending, verifies the recovery screen retains focus while protected content is suspended, then resolves the same authenticated session and asserts focus on the restored Backup Control Center heading. It does not remove or weaken `toHaveFocus()`.

The full Web suite also exercises recovery error/retry and security boundaries. [`AuthProvider.test.tsx`](../../../apps/web/src/auth/AuthProvider.test.tsx) covers five concurrent retries deduplicating to one recovery, failed recovery remaining retryable, and an old recovery resolving after logout/new login without advancing the new identity. [`BackupOverviewPage.test.tsx`](../../../apps/web/src/features/backups/BackupOverviewPage.test.tsx) covers a cross-principal pending mutation and an old principal GET resolving after logout/new login without exposing its response. No session/token policy or protected-route boundary was changed.

A later full hosted Web run on manifest head `d87b1ca2bb8cb83c19bbba16cfb447edb8b88bfe` passed lint and typecheck but failed one assertion in [`BackupsPage.test.tsx`](../../../apps/web/src/features/backups/BackupsPage.test.tsx). Its log showed the page's not-found `h2` existed while the shell `h1` still had focus: `findByRole` awaited the DOM element but not the component's existing passive focus effect. The assertion now waits with `waitFor` for that same `h2` to receive focus. It was not removed, weakened, or given a broader timeout. The affected test passed 20 local repetitions and the complete Web suite passed locally after this test-only synchronization correction; hosted retest remains pending.

## Task C — Windows acceptance classification

The P028 workflow's hosted `windows-2025` run passed candidate build and standard-user native acceptance, then intentionally emitted `BLOCKED_BY_ENVIRONMENT` for required interactive desktop UI, genuine logon transition, and end-to-end named-pipe evidence. This is a release qualification blocker, not a source-code failure inferred from a red status. The hosted runner can validate the MSVC/Qt build, reproducibility and identity, `asInvoker`, and bounded standard-user setup. It cannot supply the documented `NATIVE_CLEAN_MACHINE` graphical/logon/IPC evidence.

Required qualification remains a disposable Windows 11 AMD64 environment with trustworthy interactive desktop automation, a real disposable user logon, and a passing named-pipe control harness. The P028 gate remains blocked until those evidence surfaces pass. On the P001A PR head, run `39912679384` candidate job `120279040449` passed; standard-user job `120286121009` passed native standard-user product acceptance, then failed closed at “Record required unavailable surfaces honestly” with `BLOCKED_BY_ENVIRONMENT` for GUI/logon evidence and the unwired named-pipe gate. Evidence upload passed. Artifact metadata: `windows-p028-native-evidence`, ID `12484457653`, 81,503 bytes, SHA-256 `ba3ddb1e7e734ce3ae765e9fcdc8bbd14561fab47fb8fd797c248466f72af77f`; the exact candidate artifact was `windows-p028-candidate`, ID `12483853452`, 32,279,332 bytes, SHA-256 `3da64f644f9e1d71c842d5ccfcd27432c892741df3ee4fb4c1187990a58fe0b8`. No Windows gate or intentional failure condition was changed.

The separate P044 Inno process-interruption test passed on rerun on the P001 head and uploaded its evidence artifact. Because the original job log was unavailable, its one failed first attempt is classified as non-reproduced with unknown transient cause, not as a proven environment limitation or code regression. The repair PR's own fresh P044 run also passed its real Inno interruption fixture and evidence upload (run `39912679237`, job `120279040572`).

## Files changed

Repair PR file set after adding this manifest:

- [`Cargo.lock`](../../../Cargo.lock)
- [`deny.toml`](../../../deny.toml)
- [`AppShell.tsx`](../../../apps/web/src/components/layout/AppShell.tsx)
- [`BackupOverviewPage.test.tsx`](../../../apps/web/src/features/backups/BackupOverviewPage.test.tsx)
- [`BackupsPage.test.tsx`](../../../apps/web/src/features/backups/BackupsPage.test.tsx)
- `docs/v0.3/manifests/PROMPT001A_MANIFEST.md`

No migration, database, server API, mobile client, installer, P001 audit, or workflow file changed.

## Validation commands and outcomes

Local environment: Linux `6.18.55`, x86_64; Rust/Cargo `1.99.1`; cargo-deny `0.20.2`; Node `v24.19.0`; npm `11.9.0`. The Rust toolchain and cargo-deny were installed under `/tmp` for validation; repository files were not changed by tool installation.

| Command | Result | Evidence / limitation |
|---|---|---|
| `cargo deny check` | `PASS` | `advisories ok, bans ok, licenses ok, sources ok`; no advisory suppression |
| `cargo tree -i rustls-webpki@0.103.16 --locked` | `PASS` | Only patched `0.103.16` resolved; path includes reqwest/hyper-rustls and SQLx |
| `cargo tree -i webpki-roots@1.0.10 --locked` | `PASS` | Only patched `1.0.10` resolved through reqwest/hyper-rustls |
| `cargo fmt --all -- --check` | `PASS` | Rust formatting clean |
| `cargo check --workspace --locked` | `BLOCKED` | Patched TLS graph compiled; the workspace build stopped at `libdbus-sys` because `dbus-1.pc` / `libdbus-1-dev` is absent |
| `cargo test -p synveil-server-host --locked` | `PASS` | 2 tests passed; patched Rustls and webpki packages compiled |
| `cargo clippy -p synveil-server-host --all-targets --locked -- -D warnings` | `PASS` | Strict Clippy clean for server-host targets |
| `cargo test -p synveil-server-network --locked` | `PASS` | 5 tests passed |
| `cargo test -p synveil-client-sync --locked` | `BLOCKED` | Build stopped before tests because this Linux container lacks `dbus-1.pc` / `libdbus-1-dev`; hosted matrix checks are required for this path |
| Targeted recovery test, repeated 20 times | `PASS` | Each run of `npm --prefix apps/web test -- src/features/backups/BackupOverviewPage.test.tsx -t 'suspends protected content on recovery network failure and resumes on user retry'` passed |
| Targeted backup not-found focus test, repeated 20 times | `PASS` | `npm --prefix apps/web test -- src/features/backups/BackupsPage.test.tsx -t 'conceals missing and foreign backup sets behind one generic not-found view'`; exact `toHaveFocus()` assertion preserved inside `waitFor` |
| `npm --prefix apps/web test` | `PASS` | 23 files; 251 tests passed |
| `npm --prefix apps/web run typecheck` | `PASS` | TypeScript project build completed |
| `npm --prefix apps/web run lint` | `PASS` | ESLint with zero warnings allowed |
| `npm --prefix apps/web run build` | `PASS` | Vite production build completed |
| `python3 -m unittest scripts.tests.test_release_source_fingerprint -v` | `PASS` | 4 release-source fingerprint boundary tests passed |
| Release/install/documentation validators (`validate-installer-engine`, `validate-install-journal`, `validate-install-lifecycle`, `validate-install-error-model`, `validate-linux-package-integration`, Debian/RPM UX, install acceptance, release manifest/download/channel, `validate-docs.sh`) | `PASS` | Re-run with the isolated Cargo toolchain on `PATH`; installation-engine, release-manifest, and documentation test suites passed. Linux package artifact tests that require locally built `.deb`/`.rpm` files reported their documented skip because those artifacts were not built locally. |
| `python3 scripts/validate-windows-installer.py` | `PASS` | Static Windows installer contract |
| `python3 scripts/validate-windows-native-acceptance.py` | `PASS` | Contract validator explicitly reports the checkpoint remains blocked |
| P001 `python3 scripts/validate_p001_audit.py --base origin/develop/v0.3` | `PASS` | Ran in a detached worktree at P001 head `244edff...`; required headings/links pass and only the original four audit files differ |
| `git diff --check` | `PASS` | No whitespace errors in repair changes before manifest addition |
| `cargo test --workspace --locked` | `NOT_RUN` | Full workspace test was not run locally because native build prerequisites are absent; the targeted client-sync test above confirms the missing D-Bus dependency. No claim of local native Windows, macOS, iOS, or simulator validation. |

An initial validator invocation without the isolated Cargo bin directory on `PATH` stopped at `cargo: command not found`; the complete validator set was rerun with the configured toolchain and passed.

## Hosted qualification on repair code head

The original code checks ran on `8a5fa893395312a0839c409e7de350826c269c9b`. PR event run [39912679340](https://github.com/nghianguyen150612/Synveil/actions/runs/39912679340) and matching push run [39912661911](https://github.com/nghianguyen150612/Synveil/actions/runs/39912661911) both reported PASS for Dependency policy (PR job `120279040636`), Web quality (`120279040605`), format/Clippy (`120279040726`), Ubuntu/macOS/Windows workspace checks (`120279040619`, `120279040583`, `120279040585`), and Ubuntu/macOS/Windows test jobs (`120279040682`, `120279040788`, `120279040759`). Native Linux Qt job `120279040329` also passed. The Web job includes lint, typecheck, all 251 tests, and production build.

On manifest head `d87b1ca2bb8cb83c19bbba16cfb447edb8b88bfe`, Dependency policy job `120297238321` passed, macOS Check job `120297238000` passed, Ubuntu Test job `120297238294` passed, and Web quality job `120297238303` failed only the `BackupsPage.test.tsx` focus timing assertion described above. The test synchronization fix passed locally; a fresh hosted run is required. Linux Qt reproducibility job `120296715843` passed on Linux package workflow `39918188864`, AppImage workflow `39918189443` passed, and iOS Build workflow `39918188916` passed. The Windows Qt job had not completed when this checkpoint was written.

Linux package workflow [39912679359](https://github.com/nghianguyen150612/Synveil/actions/runs/39912679359) passed DEB/RPM build, reproducibility, provenance, systemd, and package checks. Fedora DNF acceptance passed job `120292584767`; APT install/launch/removal/preservation passed job `120292584797`. P044 repair workflow [39912679237](https://github.com/nghianguyen150612/Synveil/actions/runs/39912679237), job `120279040572`, passed the release client build, real Inno interruption fixture, and evidence upload.

The separate P028 native acceptance workflow [39912679384](https://github.com/nghianguyen150612/Synveil/actions/runs/39912679384) intentionally ends BLOCKED: candidate build (`120279040449`) and standard-user native acceptance passed, but job `120286121009` records `BLOCKED_BY_ENVIRONMENT` for qualified interactive UI, genuine logon, and named-pipe end-to-end evidence, then uploads bounded evidence. The workflow and [Windows native acceptance policy](../../v0.2/WINDOWS_NATIVE_ACCEPTANCE.md) state that P028 remains in progress and requires `NATIVE_CLEAN_MACHINE` evidence. This is a release-qualification gate, not evidence of a repair regression; no blocker was converted to PASS. The workflow requires a qualified disposable Windows 11 AMD64 environment with interactive UI automation, real logon, and passing named-pipe control harness.

AppImage run `39912661902` passed, and the failed upstream-fetch attempt on `39912679385` passed on rerun `120284279689`. The first P044 job failure was also non-reproduced, as described above. Android CI was `NOT_RUN` because its workflow path filters do not include any changed Android source or workflow files; no Android files changed. Windows Qt native packaging qualification (PR job `120279040842` and matching push job `120278986014`) was canceled or still running when the branch advanced to the manifest commit; neither is claimed as a pass. No native GUI/logon, iOS, or Windows release qualification is inferred from Linux-local results.

### Final repair-code checkpoint `7452d857`

The PR and push runs below were triggered for repair-code checkpoint `7452d857072314498153475b87ce694724622e19` on 2026-10-23. At the latest live check, the results were:

| Workflow | Run / job | Result at evidence cutoff |
|---|---|---|
| Rust CI (PR) | [39922901068](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901068); Dependency policy `120312486073`, Web quality `120312486157` | `QUEUED`; all 11 jobs were queued. No final-head hosted PASS is claimed. |
| Rust CI (push) | [39922896291](https://github.com/nghianguyen150612/Synveil/actions/runs/39922896291) | `QUEUED`; macOS test/check jobs had started, remaining matrix jobs were queued. This does not replace the PR event run. |
| Windows native acceptance | [39922901171](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901171), candidate job `120312163520` | `IN_PROGRESS`; final-head P028 disposition not yet reported. The prior exact classification remains `BLOCKED_BY_ENVIRONMENT` pending qualified GUI/logon/named-pipe evidence. |
| Linux AppImage | [39922901177](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901177), job `120312163741` | `IN_PROGRESS` |
| Linux native DEB/RPM and Qt reproducibility | [39922901263](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901263), jobs `120312638764`, `120312639061`, `120312639169` | `IN_PROGRESS`; systemd 249 validation job `120312639169` passed; package and Qt jobs still running. |
| Windows installer | [39922901226](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901226), job `120312542587` | `IN_PROGRESS` |
| Linux clean-machine acceptance | [39922901013](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901013) | `IN_PROGRESS`; acceptance-contract job `120312162960` passed; artifact/fixture job `120312163337` still running. |
| iOS Build / Rust Apple / Simulator Tests | [39922901069](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901069), [39922901174](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901174), [39922901214](https://github.com/nghianguyen150612/Synveil/actions/runs/39922901214) | iOS Build and Rust Apple Build passed; Simulator Tests remained in progress. |

The final-head Web retest was not yet run because Web quality is queued in `39922901068`. Earlier hosted evidence on `8a5fa89` and the Dependency policy pass on `d87b1ca` do not replace the final-head result. The local full Web suite and dependency-policy check pass as listed above. No final-head run currently supports merging either PR.

## Verified findings, risks, and unresolved questions

- The two identified Rust advisories are absent from the locally resolved graph under the patched versions; `cargo deny check` passes without ignores.
- The AppShell recovery-heading fix passes local and hosted checks on code head `8a5fa89`. A separate asynchronous not-found focus assertion failed once in hosted Web quality on `d87b1ca`; the test now waits for the existing page-level focus effect, passes 20 local repetitions, and passes in the full local suite. Final-head hosted retest is queued.
- The first P044 interruption attempt failed once; reruns on the P001 and P001A heads passed and uploaded evidence. Its exact first-failure cause remains unknown because the Actions log blob was inaccessible here.
- P028 remains blocked for real GUI automation, real logon, and named-pipe end-to-end acceptance in a qualified Windows environment. This remains a v0.2 release qualification limitation.
- Local client-sync verification is blocked by missing Linux D-Bus development files; the full workspace test was not run locally. Hosted Linux/Windows/macOS CI on code head `8a5fa893395312a0839c409e7de350826c269c9b` passed the applicable workspace and test jobs.
- `gh pr checks --required` reported no required checks for either branch. The branch-protection endpoint returned HTTP 403 to the available integration, so required-check policy could not be independently inspected; any merge will use GitHub's ordinary non-admin path and remain subject to server enforcement.
- P001 PR #156 has not yet been requalified against a repaired base. It remains open, unmerged, and retains its original four-file diff. Gate B cannot start until P001A is integrated and updated-base checks are obtained.
- Live GitHub recheck on 2026-10-23 found no listed GitHub Releases and no `v0.2.0` tag.
- v0.2.0 remains unqualified/blocked; this repair does not create a tag, release, or stable designation.

## Repair PR URL and head SHA

- Repair PR: [#157 — SYNVEIL v0.3 P001A: CI and dependency security repairs](https://github.com/nghianguyen150612/Synveil/pull/157), targeting `develop/v0.3`.
- Latest repair code checkpoint: `7452d857072314498153475b87ce694724622e19`; final-head hosted checks are queued or in progress as listed above. This manifest update changes documentation only.
- PR #157 state at this checkpoint: open and unmerged. Gate A is blocked until final-head Dependency policy, Web quality, and applicable integration checks complete successfully.
- P001 PR #156: open and unmerged; Gate B requalification and merge are pending P001A integration. Its original four-file scope is unchanged.

## Final audit gate

`SYNVEIL_V03_P001A_BLOCKED` — the not-found focus assertion correction passes locally, but final-head Rust CI (including Dependency policy and Web quality) is queued and applicable platform/package workflows remain in progress. Keep PRs #157 and #156 open and unmerged. P001 requalification cannot start until Gate A integration. P028 release qualification remains explicitly blocked for qualified interactive UI, real-logon, and named-pipe evidence and must not be converted to PASS.
