# Prompt007 implementation manifest

## Baseline and scope

- Starting main SHA: `9c58e510a8b88a8ec8267f216685329a27b0409e`.
- Task branch: `prompt007-installer-common-engine`.
- New package: `synveil-install-engine` at `crates/install-engine`.
- Workspace change: one new Rust member using only workspace `serde` and
  `serde_json`; product version remains `0.1.0`.
- Engine schema: `1`.

The public contract comprises `InstallerEngine`, `InstallationAdapter`,
`InstallationRequest`, `InstallationPlan`, `InstallationEffect`,
`InstallationResult`, `EngineEvent`, artifact/preflight/verification records,
and closed policy/result enums. Stages are Preflight, Plan, Install, Integrate,
Verify, Complete. Effect result states are verified success, verified no-op,
failure before mutation, known partial mutation, and outcome unknown.

The ownership taxonomy contains package-owned, native-package state,
platform-integration-owned, application config, credential state, client sync
state, user library content, server config, server database, server object data,
external dependency, and ephemeral runtime state.

## Evidence

The focused integration suite contains **81 tests** using a deterministic fake
adapter. It covers lifecycle/order, validation with zero mutation, precondition
drift, privilege, every result class, uncertainty reconciliation and call
counts, compensation limits, ownership, completion boundaries, serialization,
and table-driven invariants.

Validation commands:

```text
./scripts/validate-installer-engine.sh
cargo fmt --all -- --check
cargo check -p synveil-install-engine --locked
cargo test -p synveil-install-engine --locked
cargo clippy -p synveil-install-engine --all-targets -- -D warnings
cargo check --workspace --exclude synveil-desktop --locked
./scripts/validate-install-acceptance.sh
./scripts/validate-release-manifest.sh
./scripts/validate-release-download.sh
./scripts/validate-docs.sh
```

CI runs the focused script in the quality job before the wider build. The local
checkout had no `origin`, so remote main and tag state could not be fetched or
published from Git; this is an environment limitation, not invented evidence.

## Deferred ownership

P006 retains artifact acquisition and trust. P008 owns durable journals and
resume. P009 owns detailed upgrade/repair/uninstall/purge semantics. P010 owns
user-facing error categories. P011 owns channels and version selection. Native
installers, first-run, server bootstrap, and sync remain outside Prompt007.

## PR workflow

Implementation is prepared as one logical commit titled
`feat: add installer common engine` for a PR targeting `main`. No final squash
SHA is predicted here. Publication, review, squash merge, and live-main
verification must occur before the Prompt007 readiness token is emitted.
