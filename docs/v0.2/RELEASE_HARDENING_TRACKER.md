# Synveil v0.2 release-hardening tracker

R01–R05 are release-hardening execution tasks. They do not add to or renumber
the existing P001–P048 product roadmap.

## Integrated source history

Starting `origin/main` was
`b01c3b2328b271529d02cb4ee30d97be43a42611`. Live GitHub state showed PRs
#148–#151 still open and stacked R01 → R02 → R03 → R04. They were integrated
in that order using real merge commits, with every original verified head and
predecessor history retained.

| Task | PR | Integration commit | Current disposition |
| --- | --- | --- | --- |
| R01 — Full CI recovery, build stabilization and cross-platform regression repair | [#148](https://github.com/nghianguyen150612/Synveil/pull/148) | `d8b5d3e7ec00e6992a44de6cee3b0af799667af3` | Integrated. The R01 PostgreSQL fixture defect was repaired in R02; see [R01 manifest](RELEASE_HARDENING_R01_MANIFEST.md). |
| R02 — Managed Host production readiness and P036 acceptance | [#149](https://github.com/nghianguyen150612/Synveil/pull/149) | `ac8c3e5c7c5841baa5f55029bd9c77a1861f5ea1` | Integrated. Production P036 inventory, trusted publisher inputs and native Host journey remain blocked; see [R02 manifest](RELEASE_HARDENING_R02_MANIFEST.md). |
| R03 — Production artifact identity, signatures, manifests and distribution | [#150](https://github.com/nghianguyen150612/Synveil/pull/150) | `df6afc2d7bba4ee063946f60f0bba9e067e487e0` | Integrated. R03 unsigned test-merge files are historical and are not R05 release artifacts; see [R03 manifest](RELEASE_HARDENING_R03_MANIFEST.md). |
| R04 — Native acceptance, upgrade safety and data preservation | [#151](https://github.com/nghianguyen150612/Synveil/pull/151) | `fc23d4d356ba746d739feed30e0b5aa144f0c87b` | Integrated. Native, upgrade, P036 and P047 qualification remain blocked; see [R04 manifest](RELEASE_HARDENING_R04_MANIFEST.md). |

R01–R04 verified heads are respectively
`24f84d6e9460d82380f3ab017a3a2d36d24f2879`,
`f6f2591f44caf54ff13de2b0fd869257674de655`,
`524ef926324a0e965735bf2097d67792095a525a`, and
`a4079010398c14e1bcb04914aab14c4fc8fe78c2`; all are ancestors of the current
integrated main. Closed P045 PR #78 and P047 PR #104 were not reopened or
merged. Existing iOS P001–P060 and desktop, server and Android source history
remain intact.

## R05 execution

| Task | Status | Closeout / next action |
| --- | --- | --- |
| R05 — Final integration, production qualification and conditional publication | `R05_INTEGRATED_RELEASE_BLOCKED` | R05 branch: `release/v02-official-publication`. PR #152 merged as `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`; it fixes the non-executable `scripts/build-host-server-input.sh` that failed both server candidate jobs with exit 126. The exact-source candidate producer is [run 39359470865](https://github.com/nghianguyen150612/Synveil/actions/runs/39359470865), `sign_candidate=false`; see [R05 manifest](RELEASE_HARDENING_R05_MANIFEST.md), [P047 report](RELEASE_CANDIDATE_QUALIFICATION_R05.md), and [final artifact matrix](RELEASE_FINAL_ARTIFACT_MATRIX.md). P036 production inventory, production signing/trust, native journeys, Windows ZIP reconciliation, all 21 P047 gates and P048 approval remain blocked. No tag, GitHub Release or stable channel was created. |

The release candidate and qualification decision are separate. Integrating
source does not pass a P047 gate. Only P048 may approve the final exact-source,
signed artifact set and authorize `v0.2.0` publication after every mandatory
gate has qualifying evidence.

## Current public release state

- Product version under preparation: `0.2.0`.
- Current released product remains `v0.1.0`.
- Public v0.2.0 Git tag: absent.
- GitHub Release: absent.
- Production signing: blocked; no approved signing identity was supplied.
- P036 signed production inventory: absent.
- Stable distribution channel: not provisioned or activated.
- Final qualification and P048 decision: **BLOCKED**.
