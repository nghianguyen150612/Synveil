# P047 release-candidate qualification — R05

## Decision

**`BLOCKED`. P048 is not approved.** The exact integrated release source under
evaluation is `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`, product version
`0.2.0`. The P047 gate set and gate definitions are the unchanged authoritative
21 entries in historical P047 PR #104, source
`81303a469beb2edd5ae7c3f5efacfc617b405093`. That PR remains closed and
unmerged; its machine record is used as the definition, not reintroduced into
main.

This snapshot was recorded at **2026-10-20 02:41 UTC**. Exact-source
candidate producer run
[39359470865](https://github.com/nghianguyen150612/Synveil/actions/runs/39359470865)
passed its source/version check. Its Fedora 42 host-input job failed before
compilation: rustup rejected `HOME=/github/home` for the root user in the
Fedora container (`euid-obtained home directory: /root`, exit 128). Package,
AppImage, Windows and Ubuntu host-input builds were still in progress. The
workflow cannot assemble a candidate from this run. R05 adds job-level
`HOME: /root`; that change requires an exact-source rerun. No release artifact,
final hash, production signature or native result is inferred from this run.

## Source checks and current workflow evidence

- Local exact-source version audit:
  `python3 scripts/validate-release-version.py --expected-version 0.2.0 --source-commit d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`
  passed and reported 18 workspace crates.
- Nonproduction signing-contract fixtures:
  `python3 -m unittest scripts.tests.test_release_signing scripts.tests.test_rpm_signing scripts.tests.test_p036_signing -v`
  passed 5/5 tests. The tests cover explicit missing-signer, unknown-trust-key
  and missing-inventory blockers; fixture signing is not publisher evidence.
- P036 production validator:
  `python3 scripts/validate-p036-production-artifacts.py` exited 2 with
  `BLOCKED_BY_EXTERNAL_INPUT: production-artifacts.json has not been supplied`.
- Exact-source Rust CI [39359450796](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450796),
  Windows installer [39359450646](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450646),
  Linux DEB/RPM [39359450795](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450795),
  PostgreSQL 17 [39359450706](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450706),
  Windows native acceptance [39359450736](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450736),
  and AppImage [39359450814](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450814)
  were triggered from `d0b1c43`; not all had completed at the snapshot.
- The exact-source server-services workflow [39359450749](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450749)
  passed. Server network-reachability [39359450920](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450920)
  and first-admin [39359450844](https://github.com/nghianguyen150612/Synveil/actions/runs/39359450844)
  were in progress. A static service check does not establish P036 production
  runtime acceptance.
- Installer security [39359469940](https://github.com/nghianguyen150612/Synveil/actions/runs/39359469940),
  installation resilience [39359470275](https://github.com/nghianguyen150612/Synveil/actions/runs/39359470275),
  iOS static [39359471036](https://github.com/nghianguyen150612/Synveil/actions/runs/39359471036),
  iOS Rust Apple [39359469727](https://github.com/nghianguyen150612/Synveil/actions/runs/39359469727),
  iOS build [39359469825](https://github.com/nghianguyen150612/Synveil/actions/runs/39359469825),
  and iOS simulator [39359470334](https://github.com/nghianguyen150612/Synveil/actions/runs/39359470334)
  were triggered on the exact source and were pending or in progress at the
  snapshot.
- Repair and Recovery UX checks passed on the R04 PR source, but that workflow
  did not support `workflow_dispatch` or push to `main`. R05 adds a manual
  trigger in the closeout PR so it can be run against the integrated main SHA.
  Its exact-main result is not counted until that run completes.
- Historical Actions runs on `15da713` used a tree identical to `d0b1c43` but
  a different source SHA. Superseded queued/in-progress duplicate runs were
  cancelled; they are not exact-source qualification evidence.

The GitHub-hosted Windows native workflow explicitly records unavailable
interactive desktop, real-logon and named-pipe surfaces as blockers. The
Linux clean-machine workflow also documents that its native evidence gate
must remain non-PASS without actual guest acceptance. A source test, build
runner, container, partial job or skipped guest is not a native result.

## Authoritative P047 gate outcomes

Every gate retains its original requirement. Exact source under evaluation is
`d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` for all rows. `BLOCKED` means
mandatory evidence is absent or incomplete, even if a scoped subcheck passed.

| # | Existing gate ID | Required evidence (unchanged contract) | Verification / runner | Current result and remaining evidence | Final status |
| ---: | --- | --- | --- | --- | --- |
| 1 | `SOURCE_QUALITY` | Strict formatting, lint, dependency policy and applicable workspace checks on exact source | Rust CI run 39359450796, GitHub-hosted Linux/macOS/Windows; exact-source version audit | Version audit passed; Rust CI was queued. Complete exact-source CI, including all platform and desktop jobs. | **BLOCKED** |
| 2 | `DESKTOP_RUNTIME` | Desktop/client startup, IPC, real surface and clean shutdown | Windows native run 39359450736; Linux exact-candidate consumer not yet dispatched | No qualified interactive Windows 11, Ubuntu 24.04 or Fedora 42 desktop session. | **BLOCKED** |
| 3 | `CLIENT_SYNC` | Authentication, library creation, initial sync, pause/resume, recovery, restart and preserved data | Exact signed candidate plus controlled test service and native session | No exact candidate or authenticated first-sync journey. | **BLOCKED** |
| 4 | `SERVER_RUNTIME` | Services, private listener and database/runtime ownership contracts | Server services run 39359450749 passed; network and bootstrap runs 39359450920/39359450844 in progress; PG17 run 39359450706 | Scoped service check passed. Production-signed P036 server/database/edge inputs and actual installed runtime remain absent. | **BLOCKED** |
| 5 | `FIRST_RUN_CONNECT` | Installed Connect flow, secure server entry, authentication, library choice and first sync | Exact signed Windows/Linux candidate and controlled server | No installed exact-candidate Connect journey ran. | **BLOCKED** |
| 6 | `FIRST_RUN_HOST` | Production Host first-admin bootstrap, login, library setup, sync and recovery | P036 signed inventory; Ubuntu/Fedora native P036 runners and controlled services | P036 inventory and approved keyring/fingerprints are absent; no Host journey ran. | **BLOCKED** |
| 7 | `WINDOWS_INSTALLER` | Windows 11 GUI Setup, install, launch and first-run behavior | Windows installer 39359450646 and Windows native 39359450736 | Producer/native runs were pending or in progress; no approved Authenticode identity or Windows 11 interactive acceptance. | **BLOCKED** |
| 8 | `UBUNTU_DEB` | Ubuntu 24.04 package install, launch, first run, repair and preservation | Linux package run 39359450795; exact-candidate clean-machine consumer required | Exact R05 DEB bytes and native Ubuntu 24.04 GUI/lifecycle journey were not established at snapshot. | **BLOCKED** |
| 9 | `FEDORA_RPM` | Fedora 42 package install, launch, first run, repair and preservation | Linux package run 39359450795; approved RPM signature and native Fedora journey required | Exact R05 RPM bytes, approved OpenPGP signer and native Fedora 42 journey were not established at snapshot. | **BLOCKED** |
| 10 | `LINUX_APPIMAGE` | Authenticated AppImage launch and first run on qualified clean machines | AppImage run 39359450814; Ubuntu/Fedora native sessions required | Exact R05 image build was in progress; no authenticated image or qualified native lifecycle/relocation result. | **BLOCKED** |
| 11 | `QUICK_INSTALL` | Authenticated channel selection, exact download verification, authorization and safe install | Installer security run 39359469940; trusted production origin and keys required | Fixture/source contracts do not provide a production bootstrap, authenticated stable origin, or current channel generation. | **BLOCKED** |
| 12 | `UPGRADE` | Upgrade from supported prior installation preserving configuration, credentials and library data | `deploy/packages/native-upgrade-policy.json`, `deploy/packages/windows-upgrade-policy.json`; real source-to-target test | Both policies have `upgrade_from: []`; no prior version, including v0.1.0, is approved. No test or rollback pass. | **BLOCKED** |
| 13 | `REPAIR` | Idempotent repair of package-owned state preserving user-owned state | Repair and Recovery UX workflow; native exact-package repair required | R04 PR source checks passed and R05 adds an exact-main manual trigger; exact-main result and exact-candidate installed repair are outstanding. | **BLOCKED** |
| 14 | `UNINSTALL` | Remove only owned components and preserve user libraries/configuration | Exact signed installers and native uninstall/reinstall journeys | No exact R05 package uninstall/reinstall journey or signed package exists. | **BLOCKED** |
| 15 | `DATA_PRESERVATION` | Upgrade, repair, interruption and uninstall preserve user data and credentials | Original-state snapshots, native client/server storage checks and verification report | No complete native lifecycle evidence for library identity, credentials, durable queue, objects, configuration and database state. | **BLOCKED** |
| 16 | `POWER_CYCLE_RECOVERY` | Genuine power loss at transaction boundaries; safe reconciliation without data loss | Authorized disposable P036 VM/control runner and exact signed inputs | No VM-control evidence or genuine power interruption was supplied. | **BLOCKED** |
| 17 | `ARTIFACT_INTEGRITY` | Exact source/tree, platform, version, build, artifact ID, size, SHA-256, provenance and acceptance binding | Candidate producer 39359470865; independent verifier; candidate-bound acceptance consumers | Candidate was not yet assembled at snapshot. No final R05 artifact identity or acceptance binding. | **BLOCKED** |
| 18 | `RELEASE_SIGNING` | Production signatures and independently verified references | Protected `synveil-release-signing` environment; approved Authenticode, RPM/OpenPGP and Ed25519 keys | Protected environment lookup returned 404; no authorized credentials/trust policy supplied. Five nonproduction fixture tests passed only. | **BLOCKED** |
| 19 | `TRUST_BOOTSTRAP` | Trusted bootstrap, key rotation and minimum generation without rollback | Independent public trust policy and freshness/rollback test | No production public trust root, key rotation evidence or approved minimum generation. | **BLOCKED** |
| 20 | `DISTRIBUTION_CHANNEL` | Authenticated production publication, safe download and rollback resistance | Publisher-controlled HTTPS origin; signed channel metadata and rollback tests | No origin, stable metadata or activated channel. | **BLOCKED** |
| 21 | `DOCUMENTATION` | English/Vietnamese guidance matches qualified behavior, commands, limits, security and data handling | `scripts/validate-docs.sh`, `scripts/validate-v02-user-docs.py`; final exact-byte links required | R05 docs are being updated to describe blocked reality. No qualified downloads, hashes, signing verification or release links exist. | **BLOCKED** |

No gate is `NOT_APPLICABLE`. No partial result, merge, skipped job, workflow
fixture or unsigned artifact is promoted to `PASS`.

## P048 prerequisite check

The authoritative P047 record requires all of the following before P048 may
freeze a source, create `v0.2.0`, or publish:

1. All 21 P047 gates pass against exact authenticated v0.2.0 artifacts, with
   no required FAIL/BLOCKED/ERROR/SKIPPED evidence — **not met**.
2. P045 Windows 11 interactive and Ubuntu/Fedora native clean-machine journeys,
   lifecycle/data preservation and real hard-power-cycle recovery — **not met**.
3. P036 production identity and installed Host bootstrap/login/library/sync/
   recovery/preservation against controlled services — **not met**.
4. Production signing, independent trust bootstrap, authenticated distribution,
   rollback protection and key rotation — **not met**.
5. Accepted P047 PR merged to main before final tagging — **not met; no accepted
   P047 release decision exists**.
6. Update P045 against current main, resolve its documentation validator
   conflict without losing P046 bilingual and P045 matrix checks, and rerun
   evidence — **not met**.

**P048 decision: `BLOCKED`.** There is no qualified candidate SHA, tag, public
GitHub Release, release URL or stable-channel activation.
