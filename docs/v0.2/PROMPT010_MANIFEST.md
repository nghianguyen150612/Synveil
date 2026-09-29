# Prompt010 manifest

## Provenance

- Starting accepted main: `6fa83cf01609980feef1304bf5b38cf51bdb2e8d` (`feat: add installation lifecycle contract`).
- Task branch: `prompt010-installer-error-model`.
- Preferred commit/PR subject: `feat: add installer error model`.
- Error-model schema: `ERROR_MODEL_SCHEMA_VERSION = 1`.
- Product remains 0.1.0; server/client migrations and local schema are unchanged.

## Contract inventory

Source families: 8 — Engine, Journal, Recovery, Lifecycle, Purge, Acquisition, Preflight, Platform.

User categories: 19.

Recommended actions: 17.

Retry policies: 6.

Presentation stages: 10 — Preflight, Plan, Acquire, VerifyArtifact, Install, Integrate, VerifyInstallation, Complete, Recovery, LifecyclePolicy.

P006 acquisition bridge: 19 exact codes, machine-readable in `tests/install-error-model/p006-acquisition-codes.json`. Unknown codes fail closed and are not echoed.

Diagnostic limits:

```text
MAX_DIAGNOSTIC_SERIALIZED_BYTES = 4096
MAX_SAFE_IDENTIFIER_BYTES = 96
MAX_SAFE_CODE_BYTES = 64
MAX_SECONDARY_ACTIONS = 2
```

The diagnostic surface is typed and excludes raw paths, URLs, commands, exception text, stdout/stderr, environment dumps, secrets, credentials, headers, cookies, connection strings, private keys, and P007 free-form evidence.

## Safety

- OutcomeUnknown and StillUnknown require reconciliation before retry.
- Integrity/trust failures require fresh official evidence and expose no bypass action.
- NETWORK_ERROR is the only P006 acquisition condition with SafeImmediate / RetryDownload.
- Ownership, preservation, and purge failures expose no destructive workaround.
- Fresh, ResumeReady, ReconciledApplied, and AlreadyCompleted recovery dispositions are not errors.
- Generic EffectFailed is only specialized when typed stage/resource context proves the category.
- No telemetry or diagnostic upload is introduced.

## Tests

Prompt010 adds 115 focused Rust integration tests in `error_model_contract.rs` and four Python cross-language acquisition bridge tests.

Accepted retained inventory:

```text
Prompt007                 81
Prompt008/P008A           78
Prompt009/P009A          107
Prompt010 Rust           115
install-engine total     381
Prompt010 Python           4
```

Required validation:

```text
cargo fmt --all -- --check
cargo check -p synveil-install-engine --locked
cargo test -p synveil-install-engine --locked
cargo clippy -p synveil-install-engine --all-targets -- -D warnings
./scripts/validate-installer-engine.sh
./scripts/validate-install-journal.sh
./scripts/validate-install-lifecycle.sh
./scripts/validate-install-error-model.sh
./scripts/validate-install-acceptance.sh
./scripts/validate-release-manifest.sh
./scripts/validate-release-download.sh
./scripts/validate-docs.sh
git diff --check
```

The focused Prompt010 validator is network-free and requires no Qt, PostgreSQL, package manager, root/admin privilege, or SecretStore.

## Scope boundaries

P011 retains release channel/version selection. Later Linux/Windows/first-run prompts render UI and execute platform-specific recovery. Prompt010 does not add an updater, installer UI, telemetry, diagnostic upload, network retry executor, package-manager recovery executor, database reset, credential reset, library deletion, or server-data deletion.

Only P010 is complete in the roadmap; the Phase-B checkpoint remains pending until P011.
