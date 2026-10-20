# Synveil v0.2 Release Hardening Tracker

R01–R05 are release-hardening execution tasks. They do not add to or renumber the existing P001–P048 product roadmap.

| Release-hardening task | Status | Closeout / next action |
|---|---|---|
| R01 — Full CI recovery, build stabilization and cross-platform regression repair | `SOURCE_COMPLETE_WITH_BLOCKERS` | PR [#148](https://github.com/nghianguyen150612/Synveil/pull/148) remains open on source head `dbba3f6650e057ad5526ae86953036b78b69f2ce`, based on starting `main` `b01c3b2328b271529d02cb4ee30d97be43a42611`. At 2026-10-18 22:17 UTC, all four dedicated iOS workflows passed; PR Rust CI was 9/11 passed with both Qt jobs pending, and push Rust CI was 10/11 passed with only the Windows Qt job pending. Windows Installer passed in both events, 1/1 each. Linux package systemd validation passed in both events, and push Qt reproducibility passed; package build/validation remains pending. PostgreSQL 17 passed 51 targets (671/0/0) on the push event while its PR event remained in progress. Exact-SHA Windows native candidate authentication and standard-user acceptance passed on the push event before the hosted GUI/new-logon/named-pipe gate returned `BLOCKED_BY_ENVIRONMENT`; the PR event remained active. Across exact-SHA PR and push check jobs, 74 passed, 5 failed, 11 were pending, and 2 were skipped. The full local Web suite passed 251/251. AppImage and P036 remain blocked on missing trusted inputs/inventory. The exact-head matrix and remaining blockers are recorded in the [R01 manifest](RELEASE_HARDENING_R01_MANIFEST.md). |
| R02 — Not started | `NOT_STARTED` | Begin only after R01 closeout records the final CI evidence and external blockers. |
| R03 — Not started | `NOT_STARTED` | Do not renumber or modify the P001–P048 product roadmap. |
| R04 — Not started | `NOT_STARTED` | Do not begin before its release-hardening dependencies are recorded. |
| R05 — Not started | `NOT_STARTED` | Final release readiness work remains separate from R01; no release tag or GitHub Release is authorized by this tracker. |
