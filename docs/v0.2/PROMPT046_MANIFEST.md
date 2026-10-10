# Prompt046 manifest — Documentation and Distribution Readiness

Status: **P046 documentation implemented and locally validated; publication is pending.** P046 does not complete P045, qualify a native platform, publish v0.2, or authorize P047/P048.

## Repository and branch

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Starting verified `origin/main`: `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.
- P046 branch: `docs/p046-documentation-distribution-readiness`, created from `origin/main` at the starting SHA.
- P045 remains separate: branch `feat/cross-platform-clean-machine-matrix`, PR [#78](https://github.com/nghianguyen150612/Synveil/pull/78), open and draft at observed head `035497c05c145e5c6129e744f9455937069680f4`. P046 does not modify its branch or PR.
- Product version in the P046 baseline is `0.1.0` (`Cargo.toml`). v0.1.0 remains the current released product; v0.2 is upcoming and not released.

## Changed-file manifest

- `README.md`
- `docs/README.md`
- `docs/en/v0.2/README.md`
- `docs/en/v0.2/INSTALLATION.md`
- `docs/en/v0.2/FIRST_RUN.md`
- `docs/en/v0.2/TROUBLESHOOTING.md`
- `docs/en/v0.2/UPGRADE_REPAIR_UNINSTALL.md`
- `docs/en/v0.2/ADVANCED_INSTALLATION.md`
- `docs/en/v0.2/DISTRIBUTION_READINESS.md`
- `docs/vi/v0.2/README.md`
- `docs/vi/v0.2/INSTALLATION.md`
- `docs/vi/v0.2/FIRST_RUN.md`
- `docs/vi/v0.2/TROUBLESHOOTING.md`
- `docs/vi/v0.2/UPGRADE_REPAIR_UNINSTALL.md`
- `docs/vi/v0.2/ADVANCED_INSTALLATION.md`
- `docs/vi/v0.2/DISTRIBUTION_READINESS.md`
- `docs/v0.2/PROMPT046_MANIFEST.md`
- `scripts/validate-v02-user-docs.py`
- `scripts/validate-docs.sh`

## Documentation deliverables

English and Vietnamese guides cover installation, first run and initial sync, troubleshooting, lifecycle operations, Advanced installation, and distribution readiness. Both language indexes are linked from the repository root and `docs/README.md`. Existing v0.1 installation, operations, and upgrade guides were preserved and remain the instructions for the released product.

The user guides identify the release state before presenting any v0.2 procedure. They describe the implemented Connect/sign-in/library/progress flow, identify Host as not production-qualified, and direct operators to the current v0.1 deployment guide for the released server procedure. No PostgreSQL manual setup is introduced as a workaround for managed Host.

`scripts/validate-v02-user-docs.py` adds standard-library checks for locale topic parity, the 14 matching troubleshooting categories, Markdown link targets, exact inline code/filename/flag parity, required OS/version/architecture/artifact labels, release-status distinctions, existing CLI flags, and placeholder URL/credential/digest hygiene. `scripts/validate-docs.sh` invokes it, so existing documentation-focused workflows run the P046 checks.

## Distribution-readiness findings

- Artifact naming confirmed in producers: `SynveilSetup.exe`; `synveil_<version>_amd64.deb`; `synveil-<version>-1.x86_64.rpm`; `Synveil-<version>-x86_64.AppImage`.
- Architecture target is x86_64. Windows has no exact qualified OS version in the P046 baseline. Ubuntu 24.04 x86_64 and Fedora 42 x86_64 are named Linux qualification candidates, not passed qualifications. Debian and other Debian-family distributions are not qualified. Other RPM distributions are not implied by RPM format. No generic Linux compatibility baseline is qualified.
- Source producers and CI workflows exist, but CI artifacts are unsigned and are not authenticated production releases. P045 Linux artifact build run `37970834983` passed on the P045 source head; its Ubuntu 24.04 DEB and Fedora 42 RPM/AppImage guest jobs failed before product assertions during boot/readiness, and the aggregate evidence gate failed.
- P045 Windows installer run `37970834502` passed its scoped installer workflow, including a standard-user step. Windows native acceptance run `37970834767` failed while recording required unavailable surfaces. This is scoped native testing, not native qualification.
- P045's Linux clean-machine and Windows native evidence remains incomplete. The current P045 PR is intentionally deferred and still open/draft.
- P036 end-to-end Host remains blocked. Self-host acceptance run `37970834595` failed its production-artifact identity gate; platform host jobs did not produce a passing Host qualification.
- `gh release list --repo nghianguyen150612/Synveil --limit 20` returned no GitHub Releases at audit time. No v0.2 public download entrypoint, stable-channel payload, production signing key, public trust bootstrap, or final artifact set is published. P043 provides verification code, not a production key or signing service.
- Release readiness requires P045 and P036 acceptance, exact artifact/source identities, production trust-root and signing operations, authenticated channel/manifest publication, release-specific OS/runtime claims, and P047 validation. P046 creates no v0.2.0 tag.

## Supported and unsupported claims

- Supported documentation claim: v0.1.0 is the current released product in the repository documentation. Use its existing guides today.
- Supported implementation claim: the v0.2 source contains the listed producers, package naming patterns, Connect first-run UI, sign-in, first-library setup, bounded sync progress, Linux quick-install trust checks, and lifecycle/recovery policies.
- Build evidence: the cited P045 CI source head produced Linux candidate artifacts and exercised the Windows installer workflow. These are build/scoped test results only.
- Not claimed: v0.2 is published; CI artifacts are signed/authenticated production releases; P045 native acceptance passed; Windows or Linux is native-qualified; every Debian/RPM distribution or generic Linux system is supported; Host is production-ready; v0.1-to-v0.2 upgrade is qualified; or uninstall preservation has passed the entire P045 native matrix.
- Unsupported/outside the v0.2 target matrix: ARM64/aarch64, 32-bit architectures, macOS, iOS, Android, unlisted Windows versions, unlisted Debian/RPM distributions, and unqualified generic-Linux environments.

## English/Vietnamese parity

Status: **Passed locally.** The validator found the same seven topics and 14 troubleshooting categories, identical inline commands, filenames and flags, matching fenced examples, valid relative links, matching release status, and the same platform/version/architecture/artifact facts in both locales.

## Commands and test results

Required local validation results:

```text
./scripts/validate-docs.sh
./scripts/validate-install-acceptance.sh
git diff --check
```

- `python3 scripts/validate-v02-user-docs.py` — passed: 7 paired topics and 14 troubleshooting categories, relative links, inline code/flag parity, release-status distinctions, platform facts, and trust hygiene.
- `./scripts/validate-docs.sh` — passed all 23 documentation units, including the new P046 checker and existing contract validators.
- `./scripts/validate-install-acceptance.sh` — passed: 11 scenario definitions and 21 contract tests. Native execution was not performed.
- `git diff --check` and `git diff --cached --check` — passed for the complete 19-file P046 change set.

P046-specific remote CI has not run yet; the PR is not open. CI run IDs will be added after publication.

Other relevant repository/remote inspection commands included `git fetch origin --prune`, a remote P045 ref fetch, `gh pr view 78`, `gh run view`, and `gh release list`. Exact local and hosted results will be recorded before publication.

## CI runs and known blockers

- P045 Linux cross-platform matrix: run `37970834983` — artifact build passed; guest readiness and aggregate acceptance failed.
- P045 Windows installer: run `37970834502` — scoped installer workflow passed.
- P045 Windows native acceptance: run `37970834767` — failed on required unavailable surfaces.
- P036 server self-host acceptance: run `37970834595` — production artifact identity gate failed; no Host qualification.
- Remaining blockers: P045 native clean-machine acceptance; P036 end-to-end Host evidence; production key custody/public trust-root delivery; stable channel, authenticated manifest and public download publication; final exact artifact identities; and P047 acceptance.

## Publication record

- PR URL: **pending**.
- P046 commit SHA: **pending**.
- P046 CI run IDs: **pending**.
- Merge status: **pending**.
- Final verified `origin/main` SHA: **pending**.

P045 is intentionally deferred. P046 completion does not mean P045 passed, v0.2 shipped, or P047 may begin.
