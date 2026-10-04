# Prompt032 implementation manifest — Server Storage Location Wizard

## Handoff and inherited status

- Starting authoritative `main` SHA: `94c03e4bf7344d5c9e7f6f44fb5701d2499cb9e5`
- Branch: `codex/p032-storage-location-wizard`
- Inherited markers: `SYNVEIL_SERVER_SETUP_PRODUCT_CONTRACT_LOCKED`,
  `SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED`,
  `SYNVEIL_MANAGED_SERVER_CONFIGURATION_READY`.
- P028 Windows native acceptance remains pending/withheld, including the
  existing Git/MSYS `link.exe` collision.
- P031 hosted run `37170217148` was inspected. The macOS fixture-path failure
  was a direct P031 regression: temporary paths through `/var` hit macOS's
  `/private/var` symlink while P031 enforced no-symlink config roots. P032
  corrected the API configuration test helper to canonicalize the temporary
  fixture root; focused API tests pass. The remaining recorded hosted failures
  are outside P032: baseline `quick-xml 0.38.4` dependency policy, Linux P113
  `RUSTFLAGS` unset in release-artifact tests, Windows client
  `BackgroundClientAvailability::UnsafeState`, client Clippy
  `chunks_exact_to_as_chunks`, PostgreSQL migration fixture count 34 vs 36,
  Qt setup, and the separately withheld P028 Windows linker issue. These are
  not silently treated as evidence that P032 passed or failed.
- No P028, P033, P034, P035, or P036 completion marker is emitted.

## Product and implementation boundary

`SERVER_OBJECT_DATA` means opaque canonical server-side object bytes.
`CLIENT_LIBRARY` means the user's visible synchronized files and folders. The
wizard wording is “Where should Synveil store server data?” and never calls
both locations “Synveil folder.” P032 adds a UI-neutral controller and DTOs;
it does not implement the final Host/Connect QML flow or P037 Welcome journey.

The recommended Linux Personal/Home location is `/var/lib/synveil/storage`.
It is a visible suggestion and is not initialized before explicit confirmation.
It remains separate from `/var/lib/synveil/postgresql/17`, `/etc/synveil`, and
`/etc/synveil/credentials`. A broad mount-root choice proposes a dedicated
`Synveil` child. P032 adds no disk formatting, partitioning, filesystem
creation, mount, encryption, RAID, database, service, network, or admin setup.

The canonical byte adapter remains `LocalFilesystemObjectStore`. P032 adds the
server bootstrap/ownership layer above it and does not duplicate object I/O.
Existing operator `SYNVEIL_OBJECT_ROOT` mode retains the explicit-root adapter
behavior and is not converted into managed storage.

## Durable configuration and identity

P031's unreleased V1 typed storage boundary is extended as:

```text
NotConfigured
PreparingLocal { root, storage_id, root_identity }
ConfiguredLocal { root, storage_id, root_identity, capabilities }
```

P031 typed CAS transitions preserve server installation ID, database state,
network state, secret references, and unrelated configuration. Setup generates
one UUIDv7 `storage_id` only after the user confirms, persists `PreparingLocal`
before filesystem initialization, and never replaces that ID on retry. A
missing selected leaf records its parent device/inode; an existing empty
directory records its own device/inode. After an exact new root is atomically
published with the matching server marker, configuration binds its final
directory identity. `ConfiguredLocal` is committed only after all checks and
probes pass. It persists the adapter's validated LocalFilesystemObjectStore
capability report. Managed runtime restores that exact report without
reprobing; the existing upload service requires atomic promotion, checksumming,
read-after-write, and `DurableFlush`. The current adapter has no portable
promotion fallback and requires its same-root hard-link promotion path.

`.synveil-server-storage.json` is a canonical, newline-terminated, <=4096-byte
strict version-1 JSON marker with `deny_unknown_fields`. It records
`schema_version`, `kind=synveil-server-object-storage`,
`server_installation_id`, UUIDv7 `storage_id`, `backend=local_filesystem`, and
`object_layout_version=1`; it contains no secrets or user filenames. Unknown
schema/fields, malformed markers, and foreign identity fail closed without
rewrite. The separate LocalFilesystemObjectStore `.synveil-storage-root`
marker remains canonical for that adapter's `objects/`, `objects/v1/`, and
`staging/` layout.

Both configuration identities must match the P032 marker. Managed runtime
also compares the configured root device/inode. A textual path is not
authoritative.

## Inspection, safety, and recovery

- Absolute, bounded, traversal-free paths only; rejects filesystem root,
  home/profile, current working directory, workspace/package, config,
  credentials, PostgreSQL, runtime, `/tmp`, `/var/tmp`, symlink/redirect,
  wrong type, and unsafe permissions.
- Component-aware exclusions reject both server-root-inside-client-library
  and client-library-inside-server-root. Sibling textual prefixes remain
  distinct. `StorageExclusionSet` is generic and does not depend on client
  configuration internals.
- A missing selection must be one leaf below an existing safe parent. The
  selected leaf is created exactly, with restrictive permissions; no recursive
  path creation, recursive chmod/chown, or mount ownership transfer occurs.
- Linux capacity uses native `statvfs` total/available bytes, available inode
  count where provided, and read-only flags. Required bytes are caller supplied;
  no marketing minimum is invented. Unknown capacity fails the managed Host
  selection closed. The configured missing-root status omits parent capacity.
- The user-neutral location view returns removability as `Unknown`; P032 does
  not infer internal/removable status from path or mount labels.
- After confirmation, the bounded write probe verifies exclusive create,
  write/read, file fsync, no-replace same-filesystem rename, directory sync,
  and exact probe cleanup. Existing capability types from ObjectStore are
  reused. Filesystem acceleration is optional.
- A new root is created as one leaf under a pinned, identity-checked parent;
  the server marker is published with the root through a same-parent
  no-replace rename. An existing empty dedicated root is initialized only
  after its directory identity is durably bound.
- Matching roots are verified and reused. Non-empty unknown roots and
  `.synveil-storage-root`-only legacy roots require review; no auto-adoption or
  deletion. Foreign server/storage identities are never overwritten.
- A new root is published only after its exact-leaf creation and matching P032
  identity marker are durable. An existing empty dedicated root is initialized
  in place after its directory identity is bound. Matching ready roots reopen
  idempotently. Legacy, malformed, and foreign roots are preserved unchanged.
- Unknown artifacts in a pending root require repair and remain untouched.
  Interrupted setup resumes the same root and storage ID. A configured missing
  root remains configured and disconnected. A replacement directory at the
  same path fails its saved root identity check.
- Re-selecting the exact configured root verifies idempotently; another root
  returns `StorageRelocationRequiresMigration`. P032 does not move/copy/delete
  objects or implement a migration.

## Runtime, evidence, and remaining owners

Managed API and worker now call
`open_existing_managed_local_storage`, which verifies P032 identity,
LocalFilesystemObjectStore marker/layout, configured native root identity,
and typed capability evidence without creating, reprobeing, or repairing
storage. Runtime also rechecks native capacity inspection and the filesystem
read-only flag, failing closed if either is unknown or read-only. The reopened
adapter retains the original capability report and
upload requirements. Legacy explicit-root operator behavior remains intact.
API configuration now rejects `PreparingLocal` as runtime-not-ready. Runtime
disappearance is not converted to `NotConfigured` or an empty replacement.

The Ubuntu leg of Rust CI now runs the focused ObjectStore, server-storage,
server-config, storage, and API library tests before the broader workspace
suite. This keeps P032's native Linux storage evidence visible even when an
unrelated earlier workspace test fails.

Native Linux tests exercise actual temporary-directory `statvfs`, mode/type and
symlink handling, marker persistence, ObjectStore initialization/reopen,
write/fsync/rename/remove probe, config CAS, stale confirmation, and
interruption reconciliation. Fixture interruptions are logical failpoints,
not physical power-cut evidence. No Fedora-native/container acceptance result
or filesystem-diversity claim is inferred from the local Linux host.

`tests/install-acceptance/scenarios/first-run-2.json` remains
`IMPLEMENTATION_PENDING`; Prompt032 completes only its storage-selection
sub-boundary. It does not establish `SERVER_READY` or whole Host readiness.

P033 owns service provisioning and final service ownership; P034 owns network
and reachability; P035 owns first-admin bootstrap; P036 owns end-to-end Host
journey and clean-machine acceptance. P037 owns unified Welcome, and P040 owns
client-library first-run UX.

## Changed paths

- `.github/workflows/ci.yml`
- `Cargo.toml`, `Cargo.lock`
- `crates/api/Cargo.toml`
- `crates/api/src/bin/synveil-api.rs`
- `crates/api/src/bin/synveil-worker.rs`
- `crates/api/src/runtime_server_configuration.rs`
- `crates/object-store/Cargo.toml`
- `crates/object-store/src/capabilities.rs`
- `crates/server-config/Cargo.toml`
- `crates/server-config/src/lib.rs`
- `crates/server-config/src/model.rs`
- `crates/server-config/src/store.rs`
- `crates/server-storage/Cargo.toml`
- `crates/server-storage/src/capacity.rs`
- `crates/server-storage/src/identity.rs`
- `crates/server-storage/src/lib.rs`
- `crates/server-storage/src/model.rs`
- `crates/server-storage/src/path.rs`
- `crates/server-storage/src/wizard.rs`
- `crates/storage/Cargo.toml`
- `crates/storage/src/lib.rs`
- `crates/storage/src/local.rs`
- `docs/README.md`
- `docs/adr/README.md`
- `docs/adr/ADR-062-v0.2-managed-server-storage-location.md`
- `docs/en/BACKUP.md`
- `docs/en/DEPLOYMENT.md`
- `docs/en/PLATFORM.md`
- `docs/en/SECURITY.md`
- `docs/en/STORAGE.md`
- `docs/en/TESTING.md`
- `docs/v0.2/MANAGED_SERVER_CONFIGURATION.md`
- `docs/v0.2/PROMPT032_MANIFEST.md`
- `docs/v0.2/ROADMAP.md`
- `docs/v0.2/SERVER_STORAGE_LOCATION.md`
- `scripts/validate-docs.sh`
- `scripts/validate-managed-server-config.py`
- `scripts/validate-server-storage-location.py`

No P033, P034, P035, or P036 implementation paths are included.

## Validation record

- `cargo fmt --all -- --check`: PASS.
- `CARGO_BUILD_JOBS=1 cargo check --workspace --locked`: PASS. The build printed
  an existing Qt 6 `QChar` C++ warning; no Rust check failed.
- Focused library command
  `CARGO_BUILD_JOBS=1 cargo test -p synveil-object-store -p synveil-server-storage -p synveil-server-config -p synveil-storage -p synveil-api --lib --locked`:
  PASS; API 119, ObjectStore 6, server-storage 23, server-config 28, storage 8.
- `CARGO_BUILD_JOBS=1 cargo test -p synveil-storage --locked`: PASS; 61 unit and
  integration tests passed. Six PostgreSQL-GC tests were intentionally ignored
  because `SYNVEIL_TEST_DATABASE_URL` was not set.
- Strict Clippy for ObjectStore, server-storage, server-config, storage, and
  API all-targets with `-D warnings`: PASS.
- `python3 scripts/validate-server-storage-location.py`: PASS.
- `python3 -m py_compile scripts/validate-server-storage-location.py`: PASS.
- `./scripts/validate-docs.sh`: PASS.
- `./scripts/validate-install-acceptance.sh`: PASS; 21 contract tests and all
  11 scenario definitions validate. Native acceptance was not performed.
- `python3 scripts/install_acceptance.py inventory`: PASS; FIRST-RUN-2 remains
  `IMPLEMENTATION_PENDING`.
- `git diff --check`: PASS before final staging.
- Linux local temporary-directory tests exercised native `statvfs`, write,
  file/directory sync, no-replace rename, exact cleanup, ObjectStore setup and
  reopen, config commit, and logical interruption recovery. These are not
  clean-machine Host, physical power-cut, Fedora, or filesystem-diversity proof.
- Windows and macOS target libraries are not installed in this environment and
  `rustup` is unavailable, so local cross-target compile evidence is absent.
  Hosted results will be recorded from the P032 pull request only.

P031 residual hosted failures above remain distinguished from P032-owned
evidence. No P032 hosted result is claimed before CI reports it.

Completion marker, only after all gates pass:

`SYNVEIL_SERVER_STORAGE_LOCATION_READY`
