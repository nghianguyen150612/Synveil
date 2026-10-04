# Prompt031 implementation manifest — Managed Server Configuration

## Handoff

- Starting `main` SHA: `3627ea54707ee57810ea4e803bb152d0e68c93ab`
- Branch: `codex/p031-managed-server-configuration`
- Inherited marker: `SYNVEIL_SERVER_SETUP_PRODUCT_CONTRACT_LOCKED`
- Inherited marker: `SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED`
- P028 Windows native acceptance: **pending / withheld**.
- P031 completion marker: `SYNVEIL_MANAGED_SERVER_CONFIGURATION_READY`
- No Windows-ready or guided-self-hosting-ready marker is emitted.

## Power-loss recovery

The resumed worktree was on the requested branch at the exact starting SHA,
with no Prompt031 commit or staged changes and no unrelated dirty files.
`origin/main` still matched the starting SHA. No `/etc/synveil`,
`/var/lib/synveil`, or `/run/synveil` state and no Prompt031 temporary write
files were found. Existing source, tests, docs, ADR, manifest, and validator
were preserved and reviewed before continuation.

## Implemented contract

- New focused `synveil-server-config` workspace crate; no domain-core or API
  transport dependency.
- Canonical authority: deterministic bounded JSON V1 at
  `/etc/synveil/server-config.json`; schema version `1`, maximum 32 KiB,
  `deny_unknown_fields`, UUIDv7 local installation identity, explicit
  generation and non-secret canonical fingerprint.
- Profiles: `PersonalHomeManaged` and `AdvancedExternal`; both require
  PostgreSQL major 17. Managed private runtime identity is distinct from
  `SERVER_DATABASE`; external mode remains operator-owned.
- Managed database directory policy: `/var/lib/synveil/postgresql/17` is a
  logical handoff only. P031 does not create it, initialize PostgreSQL, create
  roles/databases, or install services.
- Linux configuration path and modes: `/etc/synveil` `root:synveil 0750`;
  config `root:synveil 0640`.
- Linux secret source: `/etc/synveil/credentials` `root:root 0700`; source
  files are `root:root 0600`.
- Credential IDs: `database-url`, `database-password`,
  `rebaseline-token-key`. Config contains references only.
- New managed setup generates a 32-byte random managed role password and
  persistent 32-byte rebaseline key, represented as 64 lowercase hexadecimal
  characters. Existing values are preserved; missing/malformed/ambiguous state
  requires reconciliation and is not regenerated.
- External PostgreSQL credentials enter through a bounded typed PostgreSQL URL
  wrapper and are stored only in the protected source file.
- Secret creation is exclusive, no-follow, single-link checked, mode-restricted
  before writing, synced and reread. Config updates use same-directory atomic
  replace, directory sync, read-after-write verification, explicit generation,
  fingerprint CAS and directory locking. Fault injection covers interruption
  points; it is not power-cut evidence.
- Reconciliation distinguishes absence, valid current, unsupported schema,
  malformed, wrong type, symlink/redirect, permission mismatch, identity
  conflict, missing/invalid secret, and partial configuration. Unknown future
  data is never rewritten.
- Storage and network begin pending. Typed storage-only and local-loopback
  update boundaries preserve unrelated fields. No object root, listener,
  public origin, admin state, or service unit is selected here.
- API and worker now share protected `database-url` runtime loading. API also
  uses the bounded protected `rebaseline-token-key` loader. Explicit legacy
  environment input remains available when no file authority is selected;
  file plus plaintext environment fails as ambiguous. Managed configuration
  plus legacy configuration inputs also fails closed.
- API validates runtime configuration and required credentials, database and
  migrations, and storage before opening its listener. The managed config path
  is read-only to runtime; source secret files remain outside runtime access
  and are intended for later systemd `LoadCredential=` delivery per ADR-025.

## Implementation paths

The reviewed Prompt031 path inventory is:

- `Cargo.toml`, `Cargo.lock`
- `crates/api/Cargo.toml`
- `crates/api/src/bin/synveil-api.rs`, `crates/api/src/bin/synveil-worker.rs`
- `crates/api/src/lib.rs`, `crates/api/src/rebaseline.rs`
- `crates/api/src/runtime_database_credential.rs`
- `crates/api/src/runtime_rebaseline_credential.rs`
- `crates/api/src/runtime_server_configuration.rs`
- `crates/api/src/runtime_test_support.rs`
- `crates/metadata/Cargo.toml`, `crates/metadata/src/config.rs`
- `crates/server-config/Cargo.toml`
- `crates/server-config/src/lib.rs`, `model.rs`, `secret.rs`, `store.rs`
- `docs/adr/README.md`
- `docs/adr/ADR-061-v0.2-managed-server-configuration.md`
- `docs/en/BACKUP.md`, `DEPLOYMENT.md`, `SECURITY.md`, `STORAGE.md`,
  `UPGRADE_SAFETY.md`
- `docs/v0.2/MANAGED_SERVER_CONFIGURATION.md`
- `docs/v0.2/PROMPT031_MANIFEST.md`, `ROADMAP.md`
- `scripts/validate-managed-server-config.py`, `scripts/validate-docs.sh`

No P029/P030 contract, acceptance scenario, service unit, PostgreSQL migration,
package version, release tag, or readiness marker outside P031 was changed.

## Validation and limits

Passed:

- `cargo fmt --all -- --check`
- `CARGO_BUILD_JOBS=1 cargo check --workspace --locked` (12m51s; only an
  existing Qt/C++ `QChar` incomplete-type warning)
- `CARGO_BUILD_JOBS=1 cargo test -p synveil-server-config --locked` — 25/25
- 100 repeated runs of the two-writer config compare-and-swap test — pass
- `CARGO_BUILD_JOBS=1 cargo test -p synveil-api --lib --locked` — 118/118
- `CARGO_BUILD_JOBS=1 cargo test -p synveil-metadata --lib --locked` — 54/54
- `python3 scripts/validate-managed-server-config.py`
- `python3 -m py_compile scripts/validate-managed-server-config.py`
- `./scripts/validate-docs.sh` (including the P031 validator)
- `./scripts/validate-install-acceptance.sh` — 21 contract tests passed
- `python3 scripts/install_acceptance.py inventory` — `FIRST-RUN-2` remains
  `IMPLEMENTATION_PENDING`
- `git diff --check`

The first hosted PR validation found two P031 regressions: strict Clippy
rejected the new large inspection value and nested conditions, and macOS
operator-mode startup incorrectly probed the Linux `/etc` layout (a symlink on
macOS). Follow-up changes boxed the inspected config, simplified validation,
made Linux path discovery platform-specific, and made the shared test
environment lock recover safely from a poisoned mutex. After those changes,
these focused checks passed:

- `cargo fmt --all -- --check`
- `CARGO_BUILD_JOBS=1 cargo clippy -p synveil-server-config --all-targets --locked -- -D warnings`
- `CARGO_BUILD_JOBS=1 cargo clippy -p synveil-api --all-targets --locked -- -D warnings`
- `CARGO_BUILD_JOBS=1 cargo test -p synveil-server-config --locked` — 25/25
- `CARGO_BUILD_JOBS=1 cargo test -p synveil-api --lib --locked` — 118/118

Other failures in the first hosted run were outside P031: the dependency gate
flagged `quick-xml 0.38.4`, which is already in the starting `Cargo.lock`; the
Windows workspace check failed in unchanged `crates/client/src/launch.rs` at
`BackgroundClientAvailability::UnsafeState`; the Linux Qt job could not create
`/usr/src/synveil`; and the Windows candidate job invoked Git's Unix `link.exe`
instead of the Visual Studio linker. These do not establish Windows
managed-Host acceptance. Hosted checks are re-evaluated on the follow-up
commit.

`CARGO_BUILD_JOBS=1 cargo test -p synveil-metadata --locked` returned 101
because the unrelated P113 `release_artifact_units::artifact_unit_7` shell
probe reads `RUSTFLAGS` after its sourced helper unsets that variable under
`set -u`. The metadata library tests above passed. PostgreSQL integration tests
were ignored because no disposable PostgreSQL 17 database was configured.

An early config-crate run had one failure in the concurrency assertion because
it required exactly one successful acknowledgement. The assertion now checks
that at most one update reports success and inspects the durable final
configuration, allowing a safe outcome-unknown result after the write became
durable. The final suite and repeated concurrency runs passed.

Filesystem mode, permission, symlink, hard-link, atomic-write, secret-preservation
and fault-injection checks use disposable fixture roots. They do not prove
actual root-owned `/etc` permissions, physical interruption survival, live
PostgreSQL provisioning, systemd service delivery, clean-machine installation,
device acceptance, or production Host readiness.

## Acceptance-scenario mapping

`tests/install-acceptance/scenarios/first-run-2.json` remains
`IMPLEMENTATION_PENDING`. P031 provides source and fixture evidence for the
`SERVER_CONFIG` preservation fingerprint, protected-secret use, redacted
diagnostics, and fail-closed unknown-schema behavior. These are sub-boundary
implementation inputs only; they do not satisfy `SERVER_READY`, admin setup,
library readiness, the scenario's native clean-machine evidence minimum, or
the scenario availability gate.

Deferred: P032 object-storage selection; P033 PostgreSQL runtime provisioning,
cluster/role creation, service identity/units/LoadCredential wiring and health;
P034 reachability/TLS/firewall/public origin; P035 first-admin bootstrap; P036
end-to-end Host acceptance and the Phase-E checkpoint. No changes to immutable
v0.1.0, product versions, release tags, PostgreSQL migrations, P028 status, or
the first-run scenario's final readiness milestone.

ADR: [ADR-061](../adr/ADR-061-v0.2-managed-server-configuration.md), Accepted.
