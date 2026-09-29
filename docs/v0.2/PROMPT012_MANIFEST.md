# Prompt012 manifest

## Provenance

- Starting accepted main: `2d411e94b883df3fbeef20e443e6debb9fd40acd` (`feat: add release channel selection`).
- Task branch: `prompt012-linux-package-integration-reconciliation`.
- Task commit: created with subject `feat: reconcile linux package integration`; authoritative SHA is reported with the PR record.
- PR number/final squash SHA: pending hosted publication and merge.
- Expected final parent absent concurrent work: `2d411e94b883df3fbeef20e443e6debb9fd40acd`.
- Expected immutable `v0.1.0` tag object is `da6bd2fee5c0b266f95d0d7c53cd0aaf69fc4a9c`; this checkout has no tag refs or remote, so hosted verification remains pending. No v0.2 tag is created.

## Contract and surfaces

Linux package integration schema version 1 reconciles DEB and RPM. Package
surfaces are the existing three `/usr/bin` executables, immutable shared
documentation/template payload, desktop entry/icon, user client unit, system
maintenance units, sysusers, and tmpfiles definitions. All are package-database
owned even where their semantic purpose is integration. Application config,
credentials, client sync state, user libraries, server
configuration/database/object state, and external dependencies are mandatory
preservation surfaces.

Files added or changed are the Rust Linux package model and tests, schema-1
JSON contract, focused validator, Quality gate, reconciliation/ADR/manifest
documentation, documentation indexes, and the P012 roadmap entry.

## Evidence

- Dedicated Prompt012 Rust tests: 119 (contract, DEB/RPM, cross-format,
  preservation, engine authority, interruption types, and manifest identity).
- Total related install-engine count: 500 (381 retained Phase-B plus 119 Prompt012).
- Focused command: `./scripts/validate-linux-package-integration.sh`.
- Retained commands: P004 acceptance and P005–P011 focused validators, plus
  install-engine check/test/clippy and documentation validation.
- Quality CI runs **Validate v0.2 Linux package integration reconciliation**.
- The unchanged **Linux native packages (DEB + RPM)** workflow remains the
  package-build/inspection evidence. Local static/fixture evidence is not
  native clean-machine acceptance.
- Hosted **Linux native packages (DEB + RPM)** currently fails at **Shell syntax
  validation (packages + install lifecycle)** on
  `scripts/verify-release-build-reproducibility.sh:58`, ShellCheck `SC2155`.
  This is pre-existing, untouched by Prompt012, and is not hidden or disabled.

## Limitations and deferred scope

No root package installation or graphical session is required by the focused
gate. Hosted native-package results must be reported separately and never
upgraded to P020 acceptance. P013, P014, P015, P016, P017, P018, P019, and P020
remain pending. Product version 0.1.0, migrations, and prior schema versions are
unchanged; there is no AppImage, updater, quick install, distro detection, or
first-launch implementation.


## Final hosted-review hardening

The schema-v1 Rust parser now pins the current product version, binary identity, desktop application identity, and the complete ordered package-shipped surface set. The focused validator enforces two-way set equality between PACKAGE/TEMPLATE records in `deploy/install/MANIFEST` and the machine-readable reconciliation contract, so either an omitted package file or an invented extra package surface fails the gate.
