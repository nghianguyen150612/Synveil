# Prompt013 manifest

## Provenance

- Starting accepted Prompt012 SHA: `754032c381a58d81df990b0a7ad2884947370d3c`.
- Fresh branch: `codex/p013-debian-ubuntu-package-ux-recreated`.
- Commit subject: `feat: deliver Debian Ubuntu package UX`.
- Server migrations remain 36, client migrations remain 7,
  `LOCAL_SCHEMA_VERSION` remains 7, and product version remains 0.1.0.

## Changed files

- Package contract: `deploy/packages/build.sh`,
  `deploy/packages/debian/control.tmpl`,
  `deploy/linux/package-integration-v1.json`, and
  `crates/install-engine/src/linux_package.rs`.
- Focused validation: `scripts/validate-debian-package-ux.py`,
  `scripts/validate-debian-package-ux.sh`,
  `crates/metadata/tests/debian_package_ux_units.rs`, and `scripts/README.md`.
- Hosted gates: `.github/workflows/ci.yml` and
  `.github/workflows/linux-packages.yml`.
- Documentation: `docs/v0.2/DEBIAN_UBUNTU_PACKAGE_UX.md`, this manifest,
  `docs/v0.2/ROADMAP.md`, and `docs/README.md`.

## Delivered contract

The canonical native builder produces `synveil_<version>_amd64.deb` with a
product-oriented control record and the complete desktop runtime dependency
closure. The authoritative install manifest owns the executable, desktop
entry, hicolor icon, and user service unit. Maintainer hooks remain bounded,
noninteractive, data-preserving, and incapable of launching the GUI or
enabling/starting the user service.

The package workflow inspects the produced archive, performs a real second
build, compares DEBs byte-for-byte, validates primary and rebuilt provenance,
and publishes the exact newly built artifact to a fresh Ubuntu runner. That
runner installs through APT, launches the installed desktop offscreen as its
ordinary user, removes through APT, and checks controlled configuration/state
sentinels survived.

## DEB-UX evidence mapping

| IDs | Evidence |
| --- | --- |
| DEB-UX-1–4 | Focused validator checks filename/identity, launcher executable, icon identity, and canonical payload paths. |
| DEB-UX-5–6 | Focused validator locks runtime families and rejects development dependencies. |
| DEB-UX-7–9 | Manifest and hook audit lock the user-unit location and prohibit launch, service activation, autostart, and home guessing. |
| DEB-UX-10 | Produced-DEB validation checks archive ownership and exact modes. |
| DEB-UX-11 | Fresh hosted runner installs the downloaded workflow artifact with APT. |
| DEB-UX-12 | Hosted runner executes the installed desktop's bounded QML smoke as non-root with build overrides cleared. |
| DEB-UX-13–14 | APT removal and controlled system/user preservation sentinels. |
| DEB-UX-15 | Canonical second build and strict primary/rebuilt DEB byte comparison. |
| DEB-UX-16 | Primary and rebuilt artifact/release manifests are validated after production. |
| DEB-UX-17–18 | Regression validator proves producer-before-consumer ordering and exact shared reproducible output root. |

## Evidence recording policy

The authoritative hosted environment is the final PR-head GitHub Actions
`ubuntu-latest` run. Its logs record the DEB filename, both SHA-256 values,
comparison result, package inspection, APT transaction, installed smoke,
removal, preservation, and provenance validation. Those results and hashes are
not predeclared here: they must be reported from the actual final-head run.
Local artifact checks are reported as skipped when native packaging inputs or
tools are absent.

RPM build/static checks are recorded separately as shared regression evidence;
they do not complete P014. Graphical package-store interaction is not inferred
from the offscreen smoke. P014, P018, P019, and P020 remain deferred, as do all
other P014–P020 roadmap deliverables.
