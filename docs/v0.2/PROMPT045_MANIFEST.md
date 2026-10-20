# Prompt045 manifest — Cross-platform clean-machine matrix

Status: **matrix contract updated on the R05 source; acceptance withheld**.
The P045/P046 documentation-validator conflict is resolved in the current
source: `scripts/validate-docs.sh` retains the P046 bilingual user-guide check
as DOC-UNIT-23 and adds the P045 matrix check as DOC-UNIT-24. P046 is
implemented; P045 native acceptance remains incomplete.

## Scope

This update carries forward only the matrix contract and its evidence
validator/test coverage needed by the current release gate. It does not merge
the old P045 draft branch or claim its historical workflow results. The exact
matrix is in [`CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md`](CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md)
and [`deploy/acceptance/p045-matrix-v1.json`](../../deploy/acceptance/p045-matrix-v1.json).

The seven matrix rows are Windows 11 x86_64 Setup; Ubuntu 24.04 x86_64 DEB,
AppImage and quick-install; and Fedora 42 x86_64 RPM, AppImage and
quick-install. Debian is `detected-not-qualified`; other Linux distributions
are `not-qualified`. The release remains `not-created`.

## Current evidence state

No current-source P045 matrix of authenticated final artifacts and complete
native result records is available. Windows 11 interactive GUI/logon and
named-pipe journeys, Ubuntu/Fedora graphical installed-product journeys,
hard VM power-cycle recovery, and P036 Host acceptance remain blocked by
missing authorized native environments and production inputs. Workflow runs
on Windows Server 2025 or in Linux containers do not satisfy those journeys.
The R05 qualification report records exact-source workflow results and the
remaining blockers; no PASS is inferred from a skipped, queued or static job.

## Validation contract

The validator binds all result rows to current scenario definitions, pinned
guest image identities, the exact source commit, the exact producer manifest
and product bytes. It rejects incomplete coverage, stale source identities,
unauthenticated quick-install evidence, fixture/offscreen evidence, missing
preservation checks, and non-hard power interruption for
`INSTALL-JOURNEY-8`. Aggregate preflight with absent native records produces
BLOCKED entries rather than PASS.

The current P046 check remains enabled independently. Matrix structural and
adversarial validator tests are in
`tests/install-acceptance/test_p045_matrix.py`; the normal documentation
validator invokes the P045 structural check as DOC-UNIT-24.

## Release boundary

P045 acceptance remains withheld pending exact-source authenticated candidate
artifacts, complete native Windows/Linux consumer evidence and hard-power-cut
recovery. P036, signing/trust and the P047 gates are separate requirements.
This manifest does not qualify the product, approve a v0.2.0 release, or
authorize a stable channel.
