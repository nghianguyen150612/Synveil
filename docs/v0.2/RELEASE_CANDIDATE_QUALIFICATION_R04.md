# P047 release candidate qualification report — R04 snapshot

## Qualification decision

**BLOCKED.** This report keeps the authoritative P047 gate set of exactly 21
categories from the historical machine-readable record in PR #104. The current
R04 product source has not produced its own exact candidate set. The candidate
producer workflow is not yet dispatchable because R03 remains stacked and its
workflow is absent from `main`. No production signatures, P036 inventory, or
native desktop acceptance results exist for the R04 candidate. This report
does not qualify v0.2.0 for P048.

Historical definition: [P047 PR #104, source
`81303a469beb2edd5ae7c3f5efacfc617b405093`](https://github.com/nghianguyen150612/Synveil/pull/104).
PR #104 was closed unmerged. Its original report remains unchanged. Historical
P045 PR #78 was also closed unmerged; see its original evidence through the
[PR #78 record](https://github.com/nghianguyen150612/Synveil/pull/78).

## Source and exact candidate scope

- R01, R02 and R03 remain open in order: #148 → #149 → #150.
- R04 PR #151 targets `release/v02-production-artifact-readiness`.
- R04 implementation source at the first exact-head run:
  `2488d6ea20856d1633512dab3b77e55bb630c639`.
- Latest product-and-acceptance source under exact-head CI:
  `17c70d870fbdd743494469eb97b09b1541a5bfc1` (second source commit corrects
  the Windows manual-candidate signature-check path).
- Product version: `0.2.0`.
- Exact R04 candidate producer run: **not available**. The manual dispatch
  attempt returned 404 because `release-artifact-candidate.yml` is absent from
  default branch `main` while R03 is unmerged.
- The earlier R03 artifact source `7f88d114e272ac220056db216030acda84be90e1`
  is a test-merge commit and cannot stand in for the R04 product source.
- No v0.2.0 tag or GitHub Release was created.

## Gate outcomes

Statuses use the P047 contract exactly: `PASS`, `FAIL`, `BLOCKED`,
`NOT_EXECUTED`, or `NOT_APPLICABLE`. Every gate retains its original
requirement; no gate was removed or weakened.

| # | Existing P047 gate ID | Required evidence | R04 status | Evidence / remaining requirement |
| ---: | --- | --- | --- | --- |
| 1 | `SOURCE_QUALITY` | Strict formatting, lint, dependency policy, and applicable source/workspace checks on exact source | `BLOCKED` | Local fmt, scoped client-sync Clippy, cargo deny, and `cargo test/clippy --workspace --exclude synveil-desktop` pass. Full desktop build lacks Qt locally; latest exact-head hosted Rust CI at `17c70d8` was pending at report capture. |
| 2 | `DESKTOP_RUNTIME` | Desktop/client start, IPC, real surface, clean shutdown | `BLOCKED` | No exact R04 candidate or supported Windows 11 / Ubuntu 24.04 / Fedora 42 interactive session. GitHub Windows 2025 is not an interactive Windows 11 logon session. |
| 3 | `CLIENT_SYNC` | Authentication, library, initial sync, pause/resume, recovery, restart and preserved state | `BLOCKED` | 296 client-sync library tests pass, including overflow repair; no real authenticated service or exact-candidate first sync was run. |
| 4 | `SERVER_RUNTIME` | Services, private listener and database/runtime ownership contracts | `BLOCKED` | Static service checks and disposable PostgreSQL 17 migration tests pass; production signed P036 server/runtime inventory and actual supported-host execution are absent. |
| 5 | `FIRST_RUN_CONNECT` | Installed Connect flow, authentication, library selection and first sync | `BLOCKED` | Exact candidate and reachable controlled production-compatible server acceptance were unavailable. |
| 6 | `FIRST_RUN_HOST` | Production first-admin bootstrap, login, library setup, sync and recovery | `BLOCKED` | P036 inventory, approved public keyring/fingerprints, authentic PostgreSQL/Caddy inputs and native runners are absent. |
| 7 | `WINDOWS_INSTALLER` | Windows 11 GUI Setup, install, launch and first run | `BLOCKED` | No R04 Setup bytes, Authenticode identity or Windows 11 interactive runner. The hosted workflow is harness evidence only. |
| 8 | `UBUNTU_DEB` | Ubuntu 24.04 package install, launch, first run, upgrade/repair and preservation | `BLOCKED` | No R04 DEB bytes or native Ubuntu desktop result. A Debian 13 development container is not this gate. |
| 9 | `FEDORA_RPM` | Fedora 42 install, launch, first run, upgrade/repair and preservation | `BLOCKED` | No R04 RPM bytes, RPM signing identity or native Fedora 42 desktop result. |
| 10 | `LINUX_APPIMAGE` | Authenticated AppImage launch and first run on supported clean machines | `BLOCKED` | Upstream AppImageKit runtime source can be authenticated, but no R04 AppImage bytes or native AppImage session exists. A single host cannot support a universal Linux claim. |
| 11 | `QUICK_INSTALL` | Authenticated channel selection, download verification, authorization and install | `BLOCKED` | Source/fixture tests pass; trusted production bootstrap and authenticated stable channel are absent. |
| 12 | `UPGRADE` | Approved prior installation upgrade with config, credentials and library preserved | `BLOCKED` | Both production `upgrade_from` policies remain empty. No source-to-target upgrade is approved; v0.1-to-v0.2 remains unsupported. |
| 13 | `REPAIR` | Idempotent package-owned repair preserving user-owned state | `BLOCKED` | Harness/static and fixture tests pass; exact R04 package and native repair journey were unavailable. |
| 14 | `UNINSTALL` | Remove owned components and preserve user libraries/configuration | `BLOCKED` | No R04 native uninstall/reinstall journey. Older R03 fixture results remain historical only. |
| 15 | `DATA_PRESERVATION` | Upgrade, repair, interruption and uninstall preserve data and credentials | `BLOCKED` | No full native journey or original-state rollback evidence. PostgreSQL 17 disposable migration does not prove installed-data preservation. |
| 16 | `POWER_CYCLE_RECOVERY` | Real power loss at transaction boundaries and safe reconciliation | `BLOCKED` | P036 VM control runner availability is unverified; no power-interruption run occurred. |
| 17 | `ARTIFACT_INTEGRITY` | Exact source/tree, platform, version, build, artifact ID, size, SHA and acceptance binding | `BLOCKED` | Candidate verifier tests pass, but no R04 producer run is available until R03 workflow integration. R03 hashes remain separate historical identities. |
| 18 | `RELEASE_SIGNING` | Production signatures and independently verified references | `BLOCKED` | Authenticode PFX/thumbprint, RPM key and approved Ed25519 signing/trust inputs are absent. |
| 19 | `TRUST_BOOTSTRAP` | Trusted bootstrap, key rotation and minimum generation without rollback | `BLOCKED` | No independently trusted public bootstrap or production key-rotation evidence. |
| 20 | `DISTRIBUTION_CHANNEL` | Authenticated production publication and rollback-resistant download policy | `BLOCKED` | 101 source fixture tests pass; no authenticated production channel was published or consumed. |
| 21 | `DOCUMENTATION` | EN/VI docs match qualified behavior, commands, limits, security and data handling | `BLOCKED` | Documentation validators pass locally, but exact R04 candidates and their native qualification are unavailable for final candidate-specific review. |

No gate is `NOT_APPLICABLE`: all 21 gates belong to the authoritative P047
release scope. No gate is counted as passed solely because a job was skipped.

## Exact-head workflow snapshot

Snapshot source at `2026-10-20 00:23 UTC`:
`17c70d870fbdd743494469eb97b09b1541a5bfc1`. The previous
implementation-only head `2488d6ea20856d1633512dab3b77e55bb630c639` had its
unfinished Actions runs cancelled after the Windows candidate-path harness
correction; those cancelled runs are not counted as final-head evidence.

| Workflow | Exact run | Observed status at snapshot | Qualification interpretation |
| --- | --- | --- | --- |
| Rust CI, push | [39346807827](https://github.com/nghianguyen150612/Synveil/actions/runs/39346807827) | Queued overall; one component passed, three in progress and seven queued | Final exact-head source quality result pending; no complete workflow pass inferred. |
| Rust CI, PR | [39346814240](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814240) | Queued overall; dependency policy passed and ten components queued | Synthetic merge check pending; not a native clean-machine result. |
| Repair and Recovery UX | [39346814249](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814249) | Queued; nine jobs queued | R04 observer fix has local pass evidence; exact hosted regression result pending. |
| Windows installer, push / PR | [39346807840](https://github.com/nghianguyen150612/Synveil/actions/runs/39346807840) / [39346814197](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814197) | Queued / in progress | Builds and fixture checks do not replace signed candidate identity or Windows 11 native GUI/logon/IPC acceptance. |
| Windows native acceptance, push / PR | [39346807825](https://github.com/nghianguyen150612/Synveil/actions/runs/39346807825) / [39346814195](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814195) | In progress / in progress | Hosted Windows 2025 fixture/checkpoint is not an interactive Windows 11 P045 result. |
| Linux package producer, push / PR | [39346807815](https://github.com/nghianguyen150612/Synveil/actions/runs/39346807815) / [39346814186](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814186) | Each in progress overall; sysusers validation passed, package jobs in progress or queued | No exact assembled R04 candidate or Ubuntu/Fedora native GUI journey is established. |
| AppImage producer, push / PR | [39346807812](https://github.com/nghianguyen150612/Synveil/actions/runs/39346807812) / [39346814192](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814192) | In progress / in progress | Producer checks are not AppImage GUI/first-run acceptance on Ubuntu 24.04 and Fedora 42. |
| Linux clean-machine acceptance | [39346814265](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814265) | In progress; acceptance-contract/adapter job passed, fixture build/import job in progress | PR path does not run the native guest matrix; the manual exact-candidate guest path did not run. |
| Managed Host / P036 | [39346814246](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814246), artifact job [118381286745](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814246/job/118381286745) | Queued overall; `artifact-identity` failed, contract/source in progress, three native/control jobs queued | The exact failing step was `Materialize approved public publisher keyring`; inventory-schema and production-file checks were skipped. The inventory/signature and approved keyring/fingerprints are absent. No Host journey ran. |
| PostgreSQL 17 hosted integration, push / PR | [39346807847](https://github.com/nghianguyen150612/Synveil/actions/runs/39346807847) / [39346814357](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814357) | Queued / queued | Local disposable PostgreSQL 17 migration gate passed 2/2; hosted full PG17 suite is separate and pending. |
| Installer security hardening | [39346814255](https://github.com/nghianguyen150612/Synveil/actions/runs/39346814255) | Three checks passed; four queued | Source and security fixture checks are partial; no production signatures or native installation result. |
| iOS static validation | [39346830699](https://github.com/nghianguyen150612/Synveil/actions/runs/39346830699) | Passed | Exact R04 source `17c70d8`; source regression workflow pass. |
| iOS Rust Apple build | [39346831152](https://github.com/nghianguyen150612/Synveil/actions/runs/39346831152) | In progress | Exact-head result pending. |
| iOS build | [39346831277](https://github.com/nghianguyen150612/Synveil/actions/runs/39346831277) | In progress | Exact-head result pending. |
| iOS simulator tests | [39346831075](https://github.com/nghianguyen150612/Synveil/actions/runs/39346831075) | Queued | Exact-head result pending. |

An exact R04 release-candidate dispatch was attempted with
`sign_candidate=false`, but GitHub returned HTTP 404: the producer workflow
exists on the unmerged R03/R04 stack and not on default branch `main`. The
current consumer harness requires that exact producer workflow run and source
SHA, so no R03 test-merge artifact or other workflow ZIP was substituted.
This also means the manual native acceptance workflows could not be dispatched
with a valid R04 candidate run ID.

## Source defect repair

R03 repair workflow failure [39266772357](https://github.com/nghianguyen150612/Synveil/actions/runs/39266772357)
was investigated and reproduced. Linux watcher queue overflow dropped one or
more events from a paired move; reconciliation then risked interpreting the
tracked source as a deletion while persisting a create at the destination.
R04 clears incomplete hints after overflow, performs full conservative
reconciliation, persists source/destination ambiguity, and blocks rather than
cancels a durable unsubmitted delete. The corresponding create remains
durable. No timeout was increased to mask the failure.

The exact-candidate Windows harness also had a stale signature-check path: it
looked in the staging directory before the downloaded Setup was copied there.
R04 commit `17c70d870fbdd743494469eb97b09b1541a5bfc1` changes the signature
check to inspect the downloaded candidate directly and adds a static contract
assertion for that path and step order.

R04 local results: observer tests 22/22, full client-sync library tests 296/296,
strict client-sync Clippy, workspace Clippy excluding `synveil-desktop`, and
workspace tests excluding `synveil-desktop` passed. The original broad failing test is split
into deterministic/native boundaries for rename or safe ambiguity, atomic save
and delete, and symlink restrictions.

## Exact Windows ZIP reproducibility status

The R03 selected Windows portable ZIP and separate Rust CI ZIP differ by 857
bytes. Their product hashes and distinct Actions archive hash are preserved in
[the candidate matrix](RELEASE_CANDIDATE_ACCEPTANCE_MATRIX.md) and historical
[R03 artifact matrix](RELEASE_ARTIFACT_MATRIX.md). R04 inspected timestamp
normalization, member ordering, compression settings, embedded package
manifest fields, Qt runtime closure production, source revision, source
fingerprint, and producer workflow inputs. Both original archives were
requested again; the GitHub artifact endpoint returned HTTP 403. The archive
members and exact embedded manifests could not be retrieved, so the underlying
cross-workflow byte divergence remains unresolved. R04 requires a same-job
independent rebuild of the selected producer's ZIP and embedded manifest, but
does not represent that as a root-cause finding for the prior discrepancy.
That discrepancy remains a release blocker until member-level evidence
establishes the cause and an exact selected candidate is reproduced.

## Data, recovery and upgrade policy

The Windows and Linux native upgrade policies continue to list no approved
source version. R04 does not advertise automatic v0.1-to-v0.2 upgrade. No
rollback guarantee is claimed. Existing PostgreSQL data, server object-store
identity, credentials, client library identity, sync queues and checkpoints
were not reset in tests. No production credential rotation or server identity
recreation occurred.

The PostgreSQL 17 migration gate ran against a disposable PostgreSQL 17
container: two tests passed, including 36 migrations from empty and a
fail-closed missing/future-table case. This is scoped DB evidence, not the P036
Host journey or power-cycle recovery.

## R05/P048 handoff

P048 is not eligible. It requires R01–R03 integrated in order, then a new
candidate from the integrated product source, exact final byte hashes,
approved production signer identities and verification, authenticated
manifest and distribution origin, resolved Windows ZIP reproducibility,
P036 eight-entry inventory, native Windows 11/Ubuntu 24.04/Fedora 42 journeys,
AppImage acceptance on each advertised distribution, power-interruption
recovery, data-preservation evidence, and PASS outcomes for every applicable
P047 gate. Any product/source/artifact change invalidates affected evidence.
