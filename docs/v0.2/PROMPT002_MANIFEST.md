# Prompt002 manifest — existing installation surface audit

Status: **Audit complete; repository validation passed.** This manifest belongs
to Prompt002 and records the evidence scope, deliverables and limits.

## Baseline

| Item | Value |
| --- | --- |
| Repository | `nghianguyen150612/Synveil` |
| Branch | `main` |
| Starting HEAD | `8014ce27d84771c548c4f0639ea7c9fd3c160f8e` |
| Starting `origin/main` | `8014ce27d84771c548c4f0639ea7c9fd3c160f8e` |
| Live remote `main` at start | `8014ce27d84771c548c4f0639ea7c9fd3c160f8e` |
| Immutable product baseline | `v0.1.0` → `fa23232ff0154f627ebdd221ec5435134f177af0` |
| Starting worktree | Clean |

The starting HEAD equals the prompt's requested SHA. Its only changes beyond
the v0.1.0 tag are the v0.2 contract/roadmap documentation, so implementation
claims are audited against v0.1.0.

## Deliverables

- `docs/v0.2/EXISTING_INSTALLATION_SURFACE_AUDIT.md` — source-backed inventory,
  evidence classifications, manual steps, blocker IDs, v0.1→v0.2 delta and
  future prompt ownership.
- `docs/v0.2/PROMPT002_MANIFEST.md` — this scope and validation record.
- `docs/v0.2/ROADMAP.md` — P002 completion record only.
- `docs/README.md` — one navigation link to the audit.

## Evidence examined

- Linux and Windows package builders, DEB/RPM metadata and lifecycle hooks,
  package-neutral install manifest/install/uninstall scripts, desktop entry,
  client user service and scheduled-maintenance service/timer.
- Desktop and client entrypoints, background launch/supervisor policies, local
  IPC transports, platform state paths and OS SecretStore adapters.
- API/worker configuration, PostgreSQL migration startup, object-root setup,
  release operations/packaging guides, CI workflow definitions and focused
  existing validation descriptions.
- CI definitions were treated as evidence of configured checks only. No claim
  is made about an unobserved CI run, production deployment or graphical
  package-manager interaction.

## Validation

| Command | Result |
| --- | --- |
| `./scripts/validate-docs.sh` | Passed: DOC-UNIT-1 through DOC-UNIT-7. |
| `cargo fmt --all -- --check` | Passed. |
| `git diff --check` | Passed. |

No broad build/test matrix or native OS installation was run for this audit.
The Windows CI workflow provides native Windows build and packaged QML smoke
coverage (x86_64 Windows, Windows runner, Qt 6.8.3/MSVC 2022, extracted ZIP in
an isolated runner temp directory, `--qml-smoke-test`); this does not establish
interactive installer, Task Scheduler reboot or installed-app lifecycle
acceptance. Native DEB/RPM GUI install and production server deployment were
not exercised by this audit.

## Git and publication

Prompt002 requires one focused commit, push to `origin/main`, live remote SHA
verification and a clean worktree after validation. The final report records
those results. The `v0.1.0` tag is immutable and is not moved or modified.
