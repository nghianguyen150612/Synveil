# P047 release-candidate qualification — R05

## Decision

**`BLOCKED`. P048 is not approved.** The exact integrated product source under
evaluation is `deef2db375764d3157afb5371eb963bbd9b3f1ea`, product version
`0.2.0`. The 21 gate IDs and their definitions remain the authoritative P047
set from closed, unmerged PR #104 at
`81303a469beb2edd5ae7c3f5efacfc617b405093`; no old P047 draft code was merged.

Snapshot: **2026-10-20 05:44 UTC**. The exact-source candidate workflow
[39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679)
passed source/version validation; Fedora server input completed, Ubuntu server,
AppImage and Windows candidate jobs were in progress, and the DEB/RPM build
job remained queued.
Exact-main Rust CI, Windows Installer, DEB/RPM, PostgreSQL 17 and AppImage runs use source
`deef2db375764d3157afb5371eb963bbd9b3f1ea`. The AppImage run passed build,
reproducibility, manifest/runtime inspection, smoke and current-user lifecycle
checks on hosted Ubuntu 22.04. Rust CI, Windows Installer, DEB/RPM and
PostgreSQL 17 were in progress. Rust CI's Ubuntu/macOS/Windows test jobs and
Ubuntu/macOS/Windows checks and quality job passed; Windows/Linux Qt, web
quality and dependency-policy jobs were not complete. The Windows native workflow completed with
`BLOCKED_BY_ENVIRONMENT`: its build and standard-user acceptance passed on
Windows Server 2025, but that runner does not provide the required Windows 11
GUI and logon session. The exact-main P036 `artifact-identity` job failed while
materializing the approved public keyring; native/interruption jobs were
queued; the P036 source/contract checks passed. No artifact identity is
inferred from an unfinished candidate workflow.

## Source checks and available evidence

- Exact integrated source/version check:
  `python3 scripts/validate-release-version.py --expected-version 0.2.0 --source-commit deef2db375764d3157afb5371eb963bbd9b3f1ea`
  passed; 18 workspace crates.
- Source fingerprint tests on R05 head: `python3 -m unittest
  scripts.tests.test_release_source_fingerprint -v` passed 4/4.
- Linux acceptance adapter self-tests:
  `python3 scripts/linux_acceptance.py self-test` passed 15/15. These validate
  adapter behavior and do not execute a native guest.
- Installation acceptance contract:
  `bash scripts/validate-install-acceptance.sh` passed 21/21; it states that
  native execution was not performed.
- Nonproduction release/RPM/P036 signing fixtures:
  `python3 -m unittest scripts.tests.test_release_signing
  scripts.tests.test_rpm_signing scripts.tests.test_p036_signing -v` passed
  5/5. Fixture keys do not establish publisher identity.
- P036 production validator exits 2:
  `python3 scripts/validate-p036-production-artifacts.py` reports
  `BLOCKED_BY_EXTERNAL_INPUT: production-artifacts.json has not been supplied`.
- Bilingual v0.2 user-doc validator passed 7 matched topics and 14
  troubleshooting cases. `bash scripts/validate-docs.sh` passed its 24
  documentation units. This does not provide a public v0.2 download or final
  release verification instructions.
- R05 PR #154 workflow evidence is on head
  `6f984752e2e03e49ecbd90bc2cd85f4344987582`, whose tree is integrated at
  `deef2db`. The PR-head AppImage run
  [39372784222](https://github.com/nghianguyen150612/Synveil/actions/runs/39372784222)
  passed build, independent byte reproducibility, manifest/runtime inspection,
  smoke and current-user lifecycle checks. It is an unsigned PR-head output,
  not an exact integrated-main production artifact or native desktop pass.
- Exact-main iOS checks passed: Static Validation [39373790719](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790719), Rust Apple Build [39373790728](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790728), iOS Build [39373790899](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790899), and Simulator Tests [39373790837](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790837). These are source/platform checks; they do not claim an iOS store release or change v0.2 support scope.
- The exact-main P036 `artifact-identity` check
  [failed at the keyring step](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245).
  The matching PR-head failure is recorded at
  [job 118464161759](https://github.com/nghianguyen150612/Synveil/actions/runs/39372788693/job/118464161759).
  Neither run registered or verified a production runtime.

## P047 gate outcomes

Every row below uses exact source SHA
`deef2db375764d3157afb5371eb963bbd9b3f1ea`. The expected R05 product artifact
set is `SynveilSetup.exe`, `synveil-0.2.0-windows-x86_64.zip`,
`synveil_0.2.0_amd64.deb`, `synveil-0.2.0-1.x86_64.rpm`,
`Synveil-0.2.0-x86_64.AppImage`, and the exact-source
`SYNVEIL-RELEASE-MANIFEST.json`. The cross-platform candidate workflow is
incomplete. Individual unsigned exact-main Setup and AppImage identities are
recorded in the artifact matrix; the package/ZIP aggregate and production
signatures/URLs are not available. “Evidence” links identify exact-main runs unless stated otherwise.

| # | Gate | Required artifact identity / verification | Runner and result | Remaining risk and final status |
| ---: | --- | --- | --- | --- |
| 1 | `SOURCE_QUALITY` | Source SHA above; `python3 scripts/validate-release-version.py --expected-version 0.2.0 --source-commit deef2db375764d3157afb5371eb963bbd9b3f1ea`; `cargo fmt --all -- --check`; Rust CI | Version audit passed locally. Exact-main [Rust CI 39373764252](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764252): Ubuntu/macOS/Windows tests, quality, and Ubuntu/macOS/Windows checks passed; Windows/Linux Qt, web quality and dependency policy remained incomplete. | Full exact-main source/workspace CI incomplete. **BLOCKED** |
| 2 | `DESKTOP_RUNTIME` | Windows Setup/ZIP, DEB, RPM and AppImage from exact candidate; launch, IPC, lifecycle and clean shutdown | Exact-main Windows native [39373764284](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764284) completed with `BLOCKED_BY_ENVIRONMENT`: build and standard-user product acceptance passed on Windows Server 2025, but interactive GUI/logon/named-pipe evidence is unavailable. Linux exact-candidate consumers await the candidate pipeline. | No interactive Windows 11/Ubuntu/Fedora release journey. **BLOCKED** |
| 3 | `CLIENT_SYNC` | Exact signed platform package, controlled Synveil server, authentication/library/initial sync/restart/preservation evidence | No signed exact candidate or authenticated real-server session. | No qualified Connect or sync evidence. **BLOCKED** |
| 4 | `SERVER_RUNTIME` | Signed P036 Synveil server, PostgreSQL 17 and Caddy entries; installed service/runtime/storage evidence | Exact-main [PG17 39373764287](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764287) remained in progress. Prompt033 service [64326](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764326), Prompt034 network [64277](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764277), and Prompt035 bootstrap [64314](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764314) passed on Ubuntu 24.04 hosted runners. No production P036 inventory or installed runtime. | These source/service checks cannot replace signed installed Host acceptance. **BLOCKED** |
| 5 | `FIRST_RUN_CONNECT` | Exact signed Windows/Linux candidate, controlled server, authentication, library selection and first sync | No exact candidate or authorized interactive Connect run. | First-run path not qualified. **BLOCKED** |
| 6 | `FIRST_RUN_HOST` | Signed P036 eight-entry inventory, PostgreSQL 17/Caddy, bootstrap, library, sync and recovery | Exact-main [P036 39373791245](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245): `artifact-identity` failed at approved keyring materialization; source/contract checks passed; native/interruption jobs remain queued. The candidate pipeline produced an unsigned Fedora Synveil server input bundle (artifact 12242708471), but its own notice records incomplete third-party dependency notices and it has no trusted publisher signature. The eight-entry production inventory/signature remain absent. | No authorized Host journey or trusted runtime. **BLOCKED** |
| 7 | `WINDOWS_INSTALLER` | Signed `SynveilSetup.exe` and ZIP; install, GUI, first run, repair and preservation on Windows 11 x86_64 | Exact-main Windows native run [39373764284](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764284) built Setup twice with identical bytes (32,816,688 bytes; SHA-256 `76f811347eaeb88b9099db29ff3140803d1fac5b5c33e3882231f9bf4ed98e96`). Retrieved artifact 12243017639 matched `p028-candidate.json` and a passing local release-manifest validation. Its standard-user product acceptance step passed on Windows Server 2025; workflow then correctly failed at `BLOCKED_BY_ENVIRONMENT` for required GUI/logon/named-pipe surfaces. Windows Installer run [39373764312](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764312) remained in progress. | No interactive Windows 11 GUI/logon, trusted Authenticode signer, or production-signed Setup/ZIP. **BLOCKED** |
| 8 | `UBUNTU_DEB` | Signed `synveil_0.2.0_amd64.deb`; Ubuntu 24.04 install, GUI, Connect, repair and preservation | Exact-main [DEB/RPM 39373764286](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764286) was building and reproducing packages. Exact-candidate native consumer not dispatched because the candidate is incomplete. | No authenticated package or native Ubuntu journey. **BLOCKED** |
| 9 | `FEDORA_RPM` | Signed `synveil-0.2.0-1.x86_64.rpm`; Fedora 42 DNF5 install, GUI, sync and repair | Exact-main [DEB/RPM 39373764286](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764286) was building and reproducing packages. No trusted RPM/OpenPGP signer or native Fedora consumer. | No authenticated package or native Fedora journey. **BLOCKED** |
| 10 | `LINUX_APPIMAGE` | Signed/source-bound `Synveil-0.2.0-x86_64.AppImage`; Ubuntu/Fedora runtime, relocation and lifecycle | Exact-main [AppImage 39373764291](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764291) passed build, independent reproducibility, manifest/runtime inspection, smoke and current-user lifecycle on Ubuntu 22.04 hosted runner. | No production signature or native Ubuntu 24.04/Fedora 42 lifecycle/relocation result. **BLOCKED** |
| 11 | `QUICK_INSTALL` | Signed manifest, public trust policy, stable origin, authenticated download and safe install | Exact-main [installer security 39373791297](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791297): Ubuntu/Windows acquisition-channel-downgrade checks and Linux invocation/path/hook checks passed; Windows ownership/security remained in progress and lifecycle/structural/dependency jobs remained queued. No stable origin, production key bootstrap or generation. | No authenticated stable acquisition. **BLOCKED** |
| 12 | `UPGRADE` | Actual supported prior installation, exact target package, migration and downgrade test | `deploy/release/native-upgrade-policy.json` and `deploy/release/windows-upgrade-policy.json` both have `upgrade_from: []`; no real source-to-target run. | v0.1.0 upgrade and rollback are unapproved and untested. **BLOCKED** |
| 13 | `REPAIR` | Exact signed installed package; repair preserving package/user ownership boundaries | Exact-main [repair/recovery UX 39373791048](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791048): static/docs, desktop/QML and installer-repair contract jobs passed; formatting, recovery projection, quality, root/data safety, client lifecycle and dependency-policy jobs remained queued. Exact-candidate native repair not run. | No full installed-package repair evidence. **BLOCKED** |
| 14 | `UNINSTALL` | Exact signed package; ownership-bounded uninstall/reinstall evidence | No exact candidate or native uninstall/reinstall result. | Preserve user data and remove only owned components remains unverified. **BLOCKED** |
| 15 | `DATA_PRESERVATION` | Before/after identity for client library, credentials, durable queue, server objects, config and DB | [Resilience 39373791405](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791405): AppImage partial recovery, Windows ENOSPC and ownership/interruption fixtures, and Ubuntu ENOSPC passed; lifecycle/preservation/quality jobs remained queued. No complete exact-package native lifecycle snapshot. | No database/client/server data preservation result. **BLOCKED** |
| 16 | `POWER_CYCLE_RECOVERY` | Exact signed P036 runtime; genuine VM power loss at transaction boundaries and recovery | P036 run 39373791245 interruption-recovery job queued; no authorized VM power-control evidence. | No genuine power interruption was executed. **BLOCKED** |
| 17 | `ARTIFACT_INTEGRITY` | Exact source, product filename, platform/architecture, payload, size, SHA-256, provenance and acceptance binding | Exact-main candidate [39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679) passed source/version validation; Fedora server input completed, Ubuntu server, AppImage, desktop Qt reproducibility and Windows candidate jobs were in progress, and DEB/RPM build remained queued. Exact-main AppImage [39373764291](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764291) produced 49,183,224 bytes with SHA-256 `746485e81835a885037d91b2bbe6cfaebc09741caa0f987b6dc60bdbde3833b1`, independently matched against the retrieved candidate manifest. The full cross-platform candidate manifest is unavailable. | Exact-source candidate and independent acceptance binding incomplete. **BLOCKED** |
| 18 | `RELEASE_SIGNING` | Authenticode, RPM/OpenPGP and Ed25519 signatures verified against independently approved roots | No production signing environment/credentials/trust policy. Five nonproduction signing fixtures passed only. | Production signatures and final signed-byte hashes absent. **BLOCKED** |
| 19 | `TRUST_BOOTSTRAP` | Independent public keyring/policy, signer pins, minimum generation, freshness and rotation | P036 keyring materialization failed on missing approved input; no independent Ed25519 public trust policy or generation. | No production trust bootstrap or rotation test. **BLOCKED** |
| 20 | `DISTRIBUTION_CHANNEL` | Public HTTPS origin, authenticated stable metadata, exact files, freshness/rollback checks | No stable origin or channel metadata; no tag/release or public files. | No public distribution or rollback resistance. **BLOCKED** |
| 21 | `DOCUMENTATION` | English/Vietnamese install, support, trust, checksum, upgrade and troubleshooting instructions matching released bytes | `python3 scripts/validate-v02-user-docs.py` and `bash scripts/validate-docs.sh` passed locally. No qualified public artifact/link/checksum exists. | Guides cannot describe final downloads or production verification yet. **BLOCKED** |

## Per-gate verification command and runner record

All entries below are for source SHA
`deef2db375764d3157afb5371eb963bbd9b3f1ea`. These commands and actions define
the required evidence. A command described as not run is not treated as a
pass. The preceding gate table records the required artifact identity,
observed result, remaining risk and final status for each gate.

| Gate | Verification command or required action | Runner/OS and observed result |
| --- | --- | --- |
| `SOURCE_QUALITY` | `python3 scripts/validate-release-version.py --expected-version 0.2.0 --source-commit deef2db375764d3157afb5371eb963bbd9b3f1ea`; `cargo fmt --all -- --check`; exact-main Rust CI | Local version audit passed. Rust CI run [39373764252](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764252), GitHub-hosted Ubuntu/macOS/Windows: tests, quality, and Ubuntu/macOS/Windows checks passed; Windows/Linux Qt, web quality and dependency policy remained incomplete at snapshot. |
| `DESKTOP_RUNTIME` | Verify imported candidate with `python3 scripts/verify_release_candidate_run.py --run-id 39373799679 --artifact-name synveil-release-candidate --repository nghianguyen150612/Synveil --expected-source-commit deef2db375764d3157afb5371eb963bbd9b3f1ea --artifact-root target/packages --output target/packages/RELEASE-CANDIDATE-VERIFICATION.json`; perform installed GUI/IPC/shutdown journeys | Candidate verifier awaits producer run [39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679). Windows hosted run is Server 2025; no qualifying Windows 11 or Linux desktop session. |
| `CLIENT_SYNC` | `cargo test -p synveil-client-sync --lib --locked`; then authenticate, select a library, sync, pause/resume, restart and compare durable state against the controlled test server | Rust CI hosted Ubuntu workspace tests passed; exact-main PostgreSQL run [39373764287](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764287) was in progress. No authenticated installed-client session was run. |
| `SERVER_RUNTIME` | `./scripts/run-p036-native-acceptance.sh ubuntu-24.04`; `./scripts/run-p036-native-acceptance.sh fedora-42`; PostgreSQL 17 scheduled-maintenance workflow | P036 declares self-hosted Ubuntu 24.04/Fedora 42 runners, but no authorized native host evidence is available; host jobs remain queued and artifact identity failed for missing keyring in [run 39373791245](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245). Prompt033/034/035 hosted Ubuntu workflows passed scoped source/service checks. |
| `FIRST_RUN_CONNECT` | On an installed signed client, launch Connect, authenticate to the controlled server, select a library, complete first sync, restart and verify identity/data | Required operator-led native GUI action was not run. No authorized interactive Windows 11, Ubuntu 24.04 or Fedora 42 session is available. |
| `FIRST_RUN_HOST` | `python3 scripts/validate-p036-production-artifacts.py --platform ubuntu-24.04 --inventory acceptance/p036/production-artifacts.json --inventory-signature acceptance/p036/production-artifacts.json.sig --artifact-root acceptance/p036/artifacts`; then `./scripts/run-p036-native-acceptance.sh ubuntu-24.04` and Fedora equivalent | P036 identity job failed before validator execution because approved public keyring variables are absent. No inventory files or authorized native host inputs exist. |
| `WINDOWS_INSTALLER` | `python scripts/validate-windows-installer.py`; `./scripts/invoke-windows-standard-user-test.ps1 -Setup candidate/windows-installer/SynveilSetup.exe -EvidenceDirectory target/windows-native-evidence`; inspect/install/repair/uninstall on Windows 11 | Exact-main installer run [39373764312](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764312) remained in progress. Native run [39373764284](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764284) used GitHub-hosted Windows Server 2025; the Setup build and standard-user product acceptance passed but GUI/logon/named-pipe evidence correctly blocked the run. |
| `UBUNTU_DEB` | `./deploy/packages/build.sh`; `dpkg-deb --info target/packages/synveil_0.2.0_amd64.deb`; install and run candidate on Ubuntu 24.04 as non-root | Exact-main package run [39373764286](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764286) was building on hosted Linux. Candidate consumer has not run. |
| `FEDORA_RPM` | `./deploy/packages/build.sh`; `rpm -qp --info target/packages/synveil-0.2.0-1.x86_64.rpm`; install through DNF5 and run on Fedora 42 as non-root | Exact-main package run [39373764286](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764286) was building on hosted Linux. No trusted RPM signature or native Fedora consumer. |
| `LINUX_APPIMAGE` | `./deploy/packages/build-appimage.sh --build-only`; `./scripts/validate-appimage-build.sh --artifact target/packages/Synveil-0.2.0-x86_64.AppImage --manifest target/packages/SYNVEIL-RELEASE-MANIFEST.json`; `python3 scripts/release_manifest.py validate --artifact-root target/packages target/packages/SYNVEIL-RELEASE-MANIFEST.json` | Exact-main AppImage run [39373764291](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764291) passed on GitHub-hosted Ubuntu 22.04: independent rebuild, manifest/runtime inspection, smoke and current-user lifecycle. Candidate hash logged as `746485e81835a885037d91b2bbe6cfaebc09741caa0f987b6dc60bdbde3833b1`; this does not establish Ubuntu 24.04/Fedora 42 native support or production signing. |
| `QUICK_INSTALL` | `python3 scripts/release_download.py --trust-policy <approved-trust-policy.json> --expected-version 0.2.0 --expected-source-commit deef2db375764d3157afb5371eb963bbd9b3f1ea <signed-manifest.json>`; `python3 scripts/release_channel.py validate <channel.json>` | Installer security run [39373791297](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791297) passed scoped unsigned fixture checks on Ubuntu and Windows; remaining hardening jobs were incomplete. Not run against production trust/origin: no production trust policy, authenticated manifest, stable origin or generation exists. |
| `UPGRADE` | Execute each proposed source-to-target upgrade and downgrade scenario from `deploy/release/native-upgrade-policy.json` and `deploy/release/windows-upgrade-policy.json` with before/after DB, config and user-state comparisons | Not run. Both policies keep `upgrade_from: []`; no supported transition is authorized. |
| `REPAIR` | `cargo test -p synveil-install-engine --test lifecycle_contract --test engine_contract --test error_model_contract --locked`; execute repair against exact signed installed packages and compare preserved state | Exact-main repair/recovery run [39373791048](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791048): static/docs, desktop/QML and installer-repair contract jobs passed; remaining jobs were queued. Signed-package native repair not run. |
| `UNINSTALL` | `bash scripts/validate-install-acceptance.sh`; execute installed-package uninstall/reinstall with ownership checks and before/after data inventory | Contract validator passed 21/21 and explicitly says native execution was not performed. No exact-candidate uninstall/reinstall journey was run. |
| `DATA_PRESERVATION` | Record and compare client library identity, credentials, durable queue, configuration, server objects and PostgreSQL state around install/repair/reinstall/recovery | AppImage recovery, Windows lifecycle, and Windows/Ubuntu ENOSPC scoped fixtures in resilience run [39373791405](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791405) passed; preservation and other jobs remained queued. No integrated native state comparison was run. |
| `POWER_CYCLE_RECOVERY` | `./scripts/run-p036-power-interruption.sh` on the authorized P036 VM control runner; validate snapshots and recovery state | Exact-main P036 interruption job [39373791245](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245) queued. VM power-control runner and authentic runtime inputs unavailable. |
| `ARTIFACT_INTEGRITY` | `python3 scripts/verify_release_candidate_run.py` with run `39373799679`; `python3 scripts/release_manifest.py validate --artifact-root target/packages target/packages/SYNVEIL-RELEASE-MANIFEST.json`; compare producer ZIP SHA-256 values | Candidate pipeline passed source/version validation. Fedora server input completed, Ubuntu server, AppImage and Windows builds plus desktop Qt reproduction were in progress, and DEB/RPM build was queued. AppImage hash is recorded above; exact aggregate candidate manifest, cross-platform sizes and Windows cross-producer ZIP equality remain outstanding. |
| `RELEASE_SIGNING` | `python3 -m unittest scripts.tests.test_release_signing scripts.tests.test_rpm_signing scripts.tests.test_p036_signing -v`; after protected signing, verify Authenticode, RPM and Ed25519 signatures against approved roots and rehash final bytes | Fixture tests passed 5/5 locally with nonproduction keys. Production signing was not attempted because approved credentials, fingerprints and trust policy are absent. |
| `TRUST_BOOTSTRAP` | `python3 scripts/materialize-p036-trust-keyring.py`; provision and verify independent P036 keyring/fingerprints and Ed25519 freshness/rollback policy | Exact-main P036 keyring step failed because authorized variables were absent. No independent production trust bootstrap exists. |
| `DISTRIBUTION_CHANNEL` | `python3 scripts/release_channel.py validate <channel.json>`; `python3 scripts/release_channel.py select-fresh --trusted-sha256 <trusted-channel-sha256> <channel.json>`; download and verify generation, signature, artifacts and rollback rejection | Not run: no stable origin or metadata. No tag, GitHub Release or channel activation exists. |
| `DOCUMENTATION` | `python3 scripts/validate-v02-user-docs.py`; `bash scripts/validate-docs.sh`; verify final public links, checksums, version and support claims after release | Local validators passed (7 matched topics/14 troubleshooting cases; 24 docs units). Public artifact links/checksums cannot be qualified before signed publication. |

No gate is `NOT_APPLICABLE`. No partial build, merge, fixture, skipped job,
unsigned artifact or contract check is promoted to `PASS`. A blocked gate may
contain scoped passing subchecks; that does not satisfy its full requirement.

## P048 prerequisite decision

P048 is **`BLOCKED`** because mandatory release gates remain blocked, there is
no authenticated final artifact set, native Windows/Linux/P036 journeys have
not passed, no production signing/trust inputs exist, upgrade and data
preservation are unqualified, and no authenticated stable channel exists. No
stable tag or publication was attempted.
