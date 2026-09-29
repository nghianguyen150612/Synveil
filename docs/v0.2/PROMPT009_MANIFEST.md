# Prompt009 manifest

## Provenance and implementation

- Starting main: `ea6c4cbc5944d96b1b57dc128134fb825e692080` (`fix: harden installation journal recovery`).
- Task branch: `prompt009-upgrade-repair-uninstall-contract`; one commit and PR to `main`.
- Lifecycle schema: `LIFECYCLE_SCHEMA_VERSION = 1`, separate from engine/journal schema 1.
- Code: `crates/install-engine/src/lifecycle.rs`; tests: `crates/install-engine/tests/lifecycle_contract.rs`.
- Public API: `LifecyclePolicy`, `InstalledStateSnapshot`, `LifecycleRequest`, `LifecycleDecision`, `UpgradeCompatibility`, `PreservationSnapshot`, `StartupPreference`, typed `LifecycleOperation`, and separate `PurgeRequest`/authorization/target types.

## Contract evidence

Upgrade requires exact explicit source/target compatibility and P006 artifact identity; unknown, unsupported, intermediate, and downgrade cases fail closed. Repair is same-version, proven package/integration restoration. Ordinary uninstall preserves all eight durable classes. Disabled startup stays disabled. Purge requires separate intent, confirmation, exact class/target scope, ownership, containment, and symlink evidence; the generic installer purge type represents only application configuration. Credential removal remains SecretStore-owned, client-state removal remains synveil-client-owned, and library/server/external deletion is unrepresentable. Lifecycle operations now require an exact operation/owner/resource/authority tuple, matching native/integration/runtime unknown states fail closed, and credential preservation requires coherent presence/identity evidence.

The P007 engine remains the executor and P008 remains the mandatory durable mutation journal. Active or ambiguous transactions block new mutation. Prompt009/P009A has 107 dedicated lifecycle tests; the install-engine total is 266 (81 P007 + 78 P008/P008A + 107 P009/P009A).

INSTALL-JOURNEY-6, INSTALL-JOURNEY-7, and UPGRADE-TEMPLATE-1 are fixture-level policy evidence only. Native acceptance remains pending. The upgrade template preserves v0.1.0 commit `fa23232ff0154f627ebdd221ec5435134f177af0` without a shipping compatibility claim.

## Validation and boundaries

Validation commands are the required cargo fmt/check/test/clippy commands plus installer-engine, journal, lifecycle, acceptance, release-manifest, release-download, docs, and `git diff --check` scripts. No Prompt009-specific environment limitation is known. Existing unrelated platform/package/PostgreSQL CI limitations remain outside scope.

P010 retains user-facing errors; P011 retains channels/version selection; later prompts retain native mechanics. Publication uses the required PR title/body and squash merge; this manifest does not predict the squash SHA.
