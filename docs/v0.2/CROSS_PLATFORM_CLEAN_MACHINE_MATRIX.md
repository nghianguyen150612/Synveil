# Cross-platform clean-machine matrix (P045)

Status: **acceptance withheld**. This document and the machine-readable matrix
define the current-source acceptance contract. They do not claim a tested
product journey or authorize the v0.2.0 release.

The contract is limited to Windows 11 x86_64, Ubuntu 24.04 x86_64 and Fedora
42 x86_64. Ubuntu and Fedora cover their native DEB/RPM route, AppImage and
quick-install. Debian is `detected-not-qualified`; other Linux distributions
are `not-qualified`. The architecture outside this matrix is not supported by
this qualification contract.

| Matrix row | Product route | Required scenarios |
| --- | --- | --- |
| `windows-11-amd64-setup` | Windows Setup | `INSTALL-JOURNEY-1`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` |
| `ubuntu-24.04-x86_64-deb` | Ubuntu DEB | `INSTALL-JOURNEY-2`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` |
| `ubuntu-24.04-x86_64-appimage` | Ubuntu AppImage | `INSTALL-JOURNEY-4`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` |
| `ubuntu-24.04-x86_64-quick-install` | Ubuntu quick-install / DEB | `INSTALL-JOURNEY-5` |
| `fedora-42-x86_64-rpm` | Fedora RPM | `INSTALL-JOURNEY-3`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` |
| `fedora-42-x86_64-appimage` | Fedora AppImage | `INSTALL-JOURNEY-4`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` |
| `fedora-42-x86_64-quick-install` | Fedora quick-install / RPM | `INSTALL-JOURNEY-5` |

`deploy/acceptance/p045-matrix-v1.json` is the machine-readable source of these
rows. `scripts/validate-cross-platform-clean-machine-matrix.py` checks the
matrix against the current acceptance scenario definitions and exact Linux
platform policy. Its aggregation mode requires one result for every
row/scenario pair, exact source and artifact identities, verified bytes,
native execution evidence, required assertions and cleanup. Missing evidence
remains `BLOCKED`; fixtures, containers, offscreen runs, stale candidates and
process termination cannot satisfy native or hard-power-cycle requirements.

The current P046 documentation implementation remains in force. The P045
matrix validator is DOC-UNIT-24, while DOC-UNIT-23 continues to run the P046
bilingual guide validator. P045 remains open until exact-source artifacts and
the required native Windows/Linux and power-cycle journeys pass. No release
tag, stable channel or support claim follows from this matrix definition.
