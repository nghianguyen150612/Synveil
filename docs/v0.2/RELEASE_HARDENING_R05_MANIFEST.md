# Synveil v0.2.0 release-hardening R05 manifest

## Final disposition

**`R05_INTEGRATED_RELEASE_BLOCKED`.** R01–R04 are integrated on `main` in
their required order, and R05 repaired a source-level workflow defect. The
exact-source unsigned candidate pipeline is running against `d0b1c43` as of
2026-10-20 02:38 UTC. Production signing, P036, native qualification and the
P048 release decision remain blocked. No `v0.2.0` tag, GitHub Release or
stable-channel activation exists.

This record is a release handoff, not production signing or release approval.
The authoritative P047 definition remains the unchanged historical record in
[PR #104 at `81303a4`](https://github.com/nghianguyen150612/Synveil/pull/104).

## Git integration

- Repository: `nghianguyen150612/Synveil`.
- Starting `origin/main`: `b01c3b2328b271529d02cb4ee30d97be43a42611`.
- Initial live check found R01–R04 open, stacked in order, with their heads
  matching the supplied verified snapshot. No v0.2.0 tag or GitHub Release
  existed.
- R01 PR #148 merged by a real merge commit: `d8b5d3e7ec00e6992a44de6cee3b0af799667af3`.
- R02 PR #149 merged by a real merge commit: `ac8c3e5c7c5841baa5f55029bd9c77a1861f5ea1`.
- R03 PR #150 merged by a real merge commit: `df6afc2d7bba4ee063946f60f0bba9e067e487e0`.
- R04 PR #151 merged by a real merge commit: `fc23d4d356ba746d739feed30e0b5aa144f0c87b`.
- Each merge retained the predecessor history. The R01, R02, R03 and R04
  original verified heads remain ancestors of `main`; P001–P060 history and
  platform source trees remain in place.
- R05 branch: `release/v02-official-publication`.
- R05 source-fix PR #152 merged as `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`.
  It makes `scripts/build-host-server-input.sh` executable. The candidate
  pipeline had failed on the prior integrated source because both host-input
  jobs received exit 126 (`Permission denied`). The script has a shebang and
  direct workflow callers; the fix changes only its Git mode (`100644` to
  `100755`).
- Exact R05 candidate source under evaluation: `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`.
- R01–R04 integrated baseline before R05 fix: `fc23d4d356ba746d739feed30e0b5aa144f0c87b`.
- Required merge policy was checked: merge commits are enabled; no repository
  rulesets or required checks were returned; `main` and the PR branches were
  reported unprotected. The branch-protection detail API returned 403, so no
  inaccessible rule is claimed absent. No force-push, squash merge, rebase
  merge or protected-rule bypass was used.

## Product and candidate identity

- Product version: `0.2.0`.
- Exact-source release-candidate workflow: [run 39359470865](https://github.com/nghianguyen150612/Synveil/actions/runs/39359470865),
  `workflow_dispatch`, source `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`,
  `sign_candidate=false`.
- Source/version validation passed. Package, AppImage and Windows builds were
  in progress. The Ubuntu host-input build was in progress; the Fedora
  host-input job failed before compilation because rustup rejected
  `HOME=/github/home` for the root user in the Fedora container
  (`euid-obtained home directory: /root`, exit 128). The workflow cannot
  assemble a candidate from this run. R05 adds job-level `HOME: /root`, then
  will rerun the candidate on the resulting integrated main source.
- Qualified production candidate SHA: **none**.
- No R03 test-merge artifact is reused as an R05 release artifact. The R03
  candidate source was `7f88d114e272ac220056db216030acda84be90e1`; its artifacts
  are historical diagnostics only.
- No official release URL exists.

## Artifact and signing disposition

The exact-source unsigned build is still running. The
[R05 final artifact matrix](RELEASE_FINAL_ARTIFACT_MATRIX.md) distinguishes
candidate workflow bytes from production-qualified and distributed artifacts.
No R05 product file is declared final until the producer and independent
verifier finish and the exact byte identities are recorded there.

Production signing was not attempted. The protected GitHub environment
`synveil-release-signing` was not configured (the environment lookup returned
404). The task supplied no authorized Authenticode PFX/password and approved
thumbprint/timestamp endpoint, RPM/OpenPGP private identity and trusted
fingerprint, Ed25519 release key and independent trust policy, or P036
publisher keyring/fingerprints. Repository secret and self-hosted runner
enumeration endpoints returned 403; their contents are not inferred. The
nonproduction fixture suite passed 5/5 tests, including explicit missing-key
and missing-inventory blockers. Fixture signatures do not establish a
production publisher identity.

No signature, publisher identity, production digest, artifact provenance or
signed inventory was fabricated. Signing must happen before final distributed
byte hashes are calculated; the final manifest must bind those exact signed
bytes.

## P036 production inventory

`acceptance/p036/production-artifacts.json` and
`acceptance/p036/production-artifacts.json.sig` are absent. Only the schema is
present. `python3 scripts/validate-p036-production-artifacts.py` exits 2 with
`BLOCKED_BY_EXTERNAL_INPUT: production-artifacts.json has not been supplied`.
The exact-source R05 run produced one unsigned Synveil server source input for
Ubuntu 24.04; its archive identity and closed payload inventory are recorded
in the [artifact matrix](RELEASE_FINAL_ARTIFACT_MATRIX.md). Its bundled notice
incorrectly identifies a desktop/Qt package. The R05 fix supplies a server
notice that states the input's scope and explicitly does not claim third-party
dependency notices. The Fedora server input failed before compilation due the
container HOME mismatch. Neither input is production-signed or P036-ready.

The R05 candidate workflow's production signing and P036 jobs were deliberately
not enabled. The required eight signed entries, PostgreSQL 17/Caddy artifacts,
publisher keyring, independent fingerprints, complete license/notice
evidence, detached inventory signature and runtime verification do not exist.
P036 is not a pass, and no real Managed Host journey ran.

## Native qualification

No `PASS_NATIVE` result was established.

- Windows 11 x86_64: no qualified interactive Windows 11 session was supplied.
  GitHub-hosted `windows-2025` checks cannot establish first-run GUI, genuine
  logon/restart or named-pipe acceptance. Exact-candidate native consumption
  has not yet run.
- Ubuntu 24.04 x86_64: no authenticated exact-candidate native desktop install
  and first-run session has completed.
- Fedora 42 x86_64: no authenticated exact-candidate native desktop install
  and first-run session has completed.
- AppImage: the exact R05 image is pending the producer; no qualified Ubuntu
  24.04 and Fedora 42 lifecycle/relocation session is established.
- Managed Host P036: blocked by the missing authenticated eight-entry
  inventory, public keyring/fingerprints and authorized native/VM control
  environment.

The exact candidate consumers must be dispatched only with a successful
producer run ID. Container checks, source tests, workflow fixture builds and
skipped jobs are not native acceptance.

## Upgrade, repair and data integrity

The current Windows and native package policies both have `upgrade_from: []`.
No prior version, including v0.1.0, is approved for automatic upgrade. No
source-to-target upgrade, downgrade or rollback has passed. Do not advertise
v0.1-to-v0.2 automatic upgrades. The release remains fresh-install-only only
if the authoritative P047 requirements and product contract later accept that
distribution path; this record does not mark `UPGRADE` not applicable.

No native v0.2 install/repair/uninstall journey established client library,
credentials, durable queue, server object-store, configuration or PostgreSQL
data preservation. No genuine VM power interruption and recovery was run. No
rollback-success claim is made. Before any later release decision, preserve
user data and verify recovery from interrupted installation/update boundaries
without resetting storage identity or credentials.

## P047 and P048

The qualification report records every one of the authoritative 21 P047 gates
and its evidence: [RELEASE_CANDIDATE_QUALIFICATION_R05.md](RELEASE_CANDIDATE_QUALIFICATION_R05.md).
All 21 remain `BLOCKED` because full source, exact artifact, signing/trust,
native journey or data-preservation evidence is missing; no status is inferred
from a merge or a partial workflow.

**P048 decision: `BLOCKED`.** Prerequisites absent include all 21 P047 PASS
results, native Windows/Ubuntu/Fedora and AppImage evidence, P036 production
identity and Host journeys, approved production signers and independent trust
roots, authenticated stable distribution, and release-specific upgrade/data
preservation decisions. No stable tag or publication was attempted.

## Exact remaining external inputs and actions

1. Provide approved Authenticode signing material through protected custody,
   the independently approved thumbprint, and approved timestamp endpoint.
2. Provide the approved RPM/OpenPGP signer and independently trusted
   fingerprint.
3. Provision an Ed25519 release metadata signing key, public trust policy,
   key ID, freshness/rollback generation and key-rotation procedure.
4. Supply authentic Ubuntu 24.04 and Fedora 42 desktop/server/PostgreSQL 17/
   Caddy bytes with immutable source revisions, payload inventories, signed
   provenance, license/notice evidence, approved publisher keyring and
   independently pinned fingerprints; validate and sign the eight-entry
   P036 inventory.
5. Provision authorized interactive Windows 11, Ubuntu 24.04 and Fedora 42
   test environments plus a controlled test server and P036 VM power-control
   runner. Supply exact-candidate interaction evidence, not screenshots or
   logs detached from the run.
6. Complete the exact-source candidate build and verifier, resolve the
   Windows portable ZIP producer discrepancy using member-level evidence,
   and bind all evidence to the final signed bytes.
7. Execute upgrade, repair, uninstall, preservation and interruption recovery
   tests against supported starting states; retain `upgrade_from: []` until a
   real path passes.
8. Complete every P047 gate, record the P048 decision as approved only after
   its prerequisites pass, then follow the ordered tag, GitHub Release,
   download verification and stable-channel publication procedure.

## Publication state

- Tag `v0.2.0`: absent.
- GitHub Release: absent; no release URL.
- Stable-channel metadata/origin: not provisioned or activated.
- Public download hashes/signatures: none.
- Current released version remains `v0.1.0`.
- Completion status: **`R05_INTEGRATED_RELEASE_BLOCKED`**.
