# Synveil v0.2.0 release-hardening R05 manifest

## Final disposition

**`R05_INTEGRATED_RELEASE_BLOCKED`.** R01–R04 and R05 source work are on
`main`. R05 merged as PR #154 with real merge commit
`deef2db375764d3157afb5371eb963bbd9b3f1ea`. The exact-source unsigned
candidate workflow passed source/version validation; Fedora server input
completed, while Ubuntu server, AppImage, Windows and Qt reproducibility jobs
were in progress and the DEB/RPM build was queued at the 05:44 UTC snapshot.
The separate exact-main AppImage workflow passed its build and bounded
hosted-runner checks. P036 production identity,
production signing, native qualification, upgrade/data-preservation evidence,
and P048 approval remain blocked. No `v0.2.0` tag, GitHub Release or
stable-channel activation exists.

This is a release handoff, not production signing or release approval. The
P047 gate set remains the unchanged set from historical P047 PR #104 at
`81303a469beb2edd5ae7c3f5efacfc617b405093`; that PR is closed and unmerged.

## Git integration

- Repository: `nghianguyen150612/Synveil`.
- Starting `origin/main`: `b01c3b2328b271529d02cb4ee30d97be43a42611`.
- R01 PR #148 merged by a real merge commit:
  `d8b5d3e7ec00e6992a44de6cee3b0af799667af3`.
- R02 PR #149 merged by a real merge commit:
  `ac8c3e5c7c5841baa5f55029bd9c77a1861f5ea1`.
- R03 PR #150 merged by a real merge commit:
  `df6afc2d7bba4ee063946f60f0bba9e067e487e0`.
- R04 PR #151 merged by a real merge commit:
  `fc23d4d356ba746d739feed30e0b5aa144f0c87b`.
- R05 PR #152 merged as `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`;
  it corrected the executable Git mode of `scripts/build-host-server-input.sh`.
- R05 PR #153 merged as `503920aeac5f087f78fea253192579b71ae5e620`.
- R05 PR #154, branch `release/v02-official-publication`, merged on
  2026-10-20 by a real merge commit:
  `deef2db375764d3157afb5371eb963bbd9b3f1ea`.
- Product-source integration SHA on `main`:
  `deef2db375764d3157afb5371eb963bbd9b3f1ea`.
- Each supplied R01–R04 verified head, the R05 #154 head
  `6f984752e2e03e49ecbd90bc2cd85f4344987582`, and the starting main SHA were
  verified reachable from integrated `main`. No squash, rebase, force-push or
  history rewrite was used. The R05 branch was retained after merge.
- Closed P045 PR #78 and P047 PR #104 were not reopened or merged. The existing
  iOS P001–P060 and desktop, server and Android source histories remain intact.
- GitHub reports merge commits enabled, `delete_branch_on_merge=false`, no
  required PR checks, and no repository rulesets. The branch-protection detail
  endpoint returned 403; no inaccessible policy is claimed absent. PR #154 was
  merged through the ordinary GitHub merge operation without `--admin`.

## Source and candidate identity

- Product version: `0.2.0`.
- Exact-source version audit on the integrated source:
  `python3 scripts/validate-release-version.py --expected-version 0.2.0 --source-commit deef2db375764d3157afb5371eb963bbd9b3f1ea`
  passed and reported 18 workspace crates.
- Qualified production candidate SHA: **none**.
- Exact integrated-main unsigned candidate workflow:
  [run 39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679),
  source `deef2db375764d3157afb5371eb963bbd9b3f1ea`, dispatched with
  `sign_candidate=false`; its source/version validation passed. At the 05:44
  UTC snapshot, Fedora server input completed, Ubuntu server, AppImage, Windows
  and desktop Qt reproducibility jobs were in progress, while the DEB/RPM build
  remained queued. It has not emitted a complete exact-source candidate.
- The candidate from earlier main SHA `503920aeac5f087f78fea253192579b71ae5e620`
  and all R03 test-merge artifacts are historical and are not R05 release
  artifacts.
- R05 PR #154 aligns the Windows Rust CI release build with Windows Installer:
  both now build `synveil-desktop` and `synveil-client` in one locked Cargo
  invocation. The clean-target Windows reproducibility verifier uses that same
  grouped invocation. The earlier same-source workflows differed by 1,163
  bytes in the PR test run; exact integrated-main cross-producer comparison is
pending the Windows Installer and Rust CI runs, which remain in progress or
queued. No ZIP is selected for
  publication until those exact main outputs match.
- PR #154 also keeps Qt diagnostics outside the source tree, strengthens
  committed-source fingerprint checks, and makes the Linux VM acceptance
  adapter executable. It changes build and acceptance infrastructure, not
  product runtime behavior.

## Final artifact and signing disposition

The [R05 final artifact matrix](RELEASE_FINAL_ARTIFACT_MATRIX.md) is the
identity record. The cross-platform candidate pipeline has not completed. The
exact-main unsigned Windows Setup and AppImage candidate identities are recorded
there; the Windows ZIP and Linux package identities are pending. No final
production-signed artifact exists. Any unsigned CI output remains a candidate
input and is not a final release artifact.

Production signing was not attempted. No authorized Authenticode certificate
and protected PFX/password, independently approved thumbprint and timestamp
endpoint; RPM/OpenPGP signing identity and trusted fingerprint; Ed25519 private
key and independent public trust policy; or production signing environment was
available. No private key, publisher identity, production digest, signature or
provenance was created. Signing must precede final distributed-byte hashing.

Signing behavior was exercised with nonproduction fixtures only:
`python3 -m unittest scripts.tests.test_release_signing scripts.tests.test_rpm_signing scripts.tests.test_p036_signing -v`
passed 5/5. These tests explicitly block missing signers, unknown trust keys
and missing inventory; they do not establish publisher trust. The exact-main
AppImage workflow did verify the pinned AppImageKit runtime signature against
upstream fingerprint `570C77ACEA40C0F1B758902CBF96CCA56490F695` and runtime SHA-256
`2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`; this is
third-party runtime provenance, not Synveil product signing.

## P036 production inventory

`acceptance/p036/production-artifacts.json` and
`acceptance/p036/production-artifacts.json.sig` are absent. The local validator
reports:

```text
BLOCKED_BY_EXTERNAL_INPUT: production-artifacts.json has not been supplied
```

The exact-main P036 workflow is
[run 39373791245](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245).
Its `artifact-identity` job failed at “Materialize approved public publisher
keyring”; the interruption, Ubuntu and Fedora native jobs remain queued. The source/contract job passed. The same failure was already visible on the
matching PR-head check
[39372788693](https://github.com/nghianguyen150612/Synveil/actions/runs/39372788693/job/118464161759).
The candidate pipeline produced an unsigned Fedora Synveil server source-input
bundle, but that bundle is not a production P036 entry: its notice explicitly
says third-party dependency license/notice evidence is incomplete. There are no
authentic signed eight-entry production bytes, independent fingerprints,
complete license/notice evidence, detached inventory signature or runtime
registration result. P036 is **BLOCKED**; no real Managed Host acceptance pass
is inferred.

## Native qualification

No `PASS_NATIVE` result was established at capture.

- Windows 11 x86_64: exact-main native acceptance
  [run 39373764284](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764284)
  completed with the expected environment-blocked result. Its build job
  produced an unsigned Setup candidate and its standard-user product acceptance
  step passed on GitHub-hosted Windows Server 2025. The final job correctly
  blocked because that runner cannot supply the requested interactive Windows
  11 GUI, genuine logon/restart, named-pipe and real-server journey.
- Ubuntu 24.04 x86_64 and Fedora 42 x86_64: the exact-main native Linux
  candidate consumer has not run because the exact candidate producer has not
  completed. No authenticated desktop install, first sync, repair and data
  preservation journey is recorded.
- AppImage: exact-main build, independent reproducibility, manifest/runtime
  inspection, smoke and current-user lifecycle workflow
  [run 39373764291](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764291)
  passed on GitHub-hosted Ubuntu 22.04. This is exact integrated-main build
  evidence, not native Ubuntu 24.04/Fedora 42 desktop acceptance.
- Managed Host P036: exact-main run 39373791245 failed artifact identity
  because the approved keyring is missing; its native/interruption jobs are
  queued. Production trust inputs, authorized native OS sessions and VM power
  control are unavailable.

Containers, offscreen Qt, source tests, a build runner, workflow fixtures,
queued or skipped guests, and candidate-only checks are not native acceptance.

## Upgrade, repair and data integrity

`deploy/release/native-upgrade-policy.json` and
`deploy/release/windows-upgrade-policy.json` both contain `upgrade_from: []`.
No prior version, including v0.1.0, is approved for automatic upgrade. No real
source-to-target upgrade, downgrade or rollback has passed. The P047 `UPGRADE`
gate remains blocked; this manifest does not mark it not applicable.

No exact signed native install/repair/uninstall journey establishes client
library identity, credentials, durable queue, server object storage,
configuration or database preservation. No real VM power interruption and
recovery was run. No rollback-success claim is made.

## P047 and P048

The qualification report records all 21 authoritative P047 gates with exact
source, artifacts, commands, runner, evidence, risks and status:
[RELEASE_CANDIDATE_QUALIFICATION_R05.md](RELEASE_CANDIDATE_QUALIFICATION_R05.md).
No gate is promoted from a merge, unsigned artifact, skipped job, fixture or
partial result.

**P048 decision: `BLOCKED`.** Required conditions still missing include all
mandatory P047 PASS evidence, authenticated production artifacts, approved
signers and trust roots, P036 inventory and Managed Host acceptance, native
Windows/Linux journeys, upgrade/data-preservation decisions, and an
authenticated stable channel. No stable tag or publication was attempted.

## Remaining external inputs and actions

1. Provide approved Authenticode signing material through protected custody,
   an independently approved thumbprint and approved timestamp endpoint.
2. Provide the approved RPM/OpenPGP signing identity and independently trusted
   fingerprint.
3. Provision an Ed25519 metadata signing key, independent public trust policy,
   key ID, minimum generation/freshness protection and key-rotation procedure.
4. Supply authentic, signed Ubuntu 24.04 and Fedora 42 desktop/server,
   PostgreSQL 17 and Caddy inputs, immutable source revisions, payload
   inventories, complete licenses/notices, approved keyring and independently
   pinned publisher fingerprints; then validate and sign the eight-entry P036
   inventory.
5. Provision authorized interactive Windows 11, Ubuntu 24.04 and Fedora 42
   test environments, a controlled test server, and P036 VM power-control
   runner. Bind interaction evidence to the exact signed candidate.
6. Complete exact-main producer and verifier runs; require Windows Installer
   and Rust CI ZIP equality; then bind acceptance to those exact final signed
   bytes.
7. Execute supported upgrade, repair, uninstall, preservation, interruption
   recovery and downgrade-rejection scenarios. Keep `upgrade_from: []` until a
   supported transition passes.
8. Pass all mandatory P047 gates and record an approved P048 decision before
   creating a tag, public release or stable-channel metadata.

## Publication state

- Product source integration SHA: `deef2db375764d3157afb5371eb963bbd9b3f1ea`.
- Tag `v0.2.0`: absent.
- GitHub Release: absent; no release URL.
- Stable-channel metadata/origin: not provisioned or activated.
- Final signed public artifact hashes and signatures: none.
- Current public product remains v0.1.0.
- Completion status: **`R05_INTEGRATED_RELEASE_BLOCKED`**.
