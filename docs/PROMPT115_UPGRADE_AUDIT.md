# Prompt115 upgrade and migration audit

Starting branch: `main`. Starting local/tracking/live remote HEAD:
`d0d28cd8e435072773a7da103cb9f5ca45e1f5fb`. Worktree was clean.

## Frozen migration review

The 36 ordered PostgreSQL migrations and seven ordered SQLite migrations are
unchanged. `LOCAL_SCHEMA_VERSION` remains seven. No new migration is required.
The embedded catalogs guard ordering/counts, transaction flags, checksums,
required schema objects, and the latest server version `20260910000000`.

All server migrations were reviewed for top-level schema/data operations and
ordering. They create tables, scoped foreign keys, constraints, indexes and
transactional trigger/function fences. There are no concurrent-index/nontransaction
exceptions, developer-home paths, external SQL imports or database-reset steps.
Constraint alterations preserve existing rows; the explicit data transitions
backfill Trash timestamps from recorded `updated_at`, schedule activation from
its previous update boundary, conservative legacy misfire defaults, and snapshot
handoff proofs from durable snapshot rows. The pin-deletion SQL belongs to an
authorized stored prune function; migration application does not invoke it.
SQLx PostgreSQL applies each frozen migration and its ledger entry in a single
transaction. Its error path can retain a session lock when a pooled connection
is returned; the runner now closes that failed connection. Current embedded
startup additionally rejects missing required tables.

SQLite 0004 rebuilds outbound intents and scan references transactionally,
preserving rows and recreating indexes. 0007 changes the origin-editing trigger
while retaining opaque profile-ID immutability and Prompt102 canonical credential
cleanup/fencing. Startup now rejects invalid/gapped/failed ledgers, missing
required objects/columns, unrecognized objects/columns, and future versions without resetting user state.
Failure closes the SQLite pool and releases the existing writer lock on return.

A disposable v3 intent fixture with an extra `future_evidence` column proved
that required-column presence alone was insufficient: the old guard permitted
0004 to advance to version seven while dropping that column and its value.
Startup now checks exact frozen column counts and rejects unrecognized schema
objects before any migration; unknown foreign-key tables/triggers must not be
allowed to participate in a rebuild. The regression requires the v3 ledger,
intent and unknown value to remain unchanged. Evidence for transaction rollback
uses a canonical profile row, not an extra table that would itself be unsupported.

## Evidence and scope

| Prompt gate | Coverage |
| --- | --- |
| MIGRATION-UNIT-1 | Existing `pg17_migration_from_empty_is_current_36`; static embedded ordering/count/transaction guard. Live execution blocked by environment. |
| MIGRATION-UNIT-2 | Existing fresh/reopen/single-writer test and new frozen-prefix matrix. |
| MIGRATION-UNIT-3 | Existing v1 profile, v4 rebaseline and v5 conflict fixtures; new file-backed prefix 1–7 schema-equivalence matrix. Fixtures use repository migration scripts; no unnamed release compatibility claim. |
| MIGRATION-UNIT-4 | New failed migration deletes fixture evidence and creates a table before injected SQL failure; reconnect proves rollback of data, DDL and version, then canonical restart succeeds. SQL failure injection, not physical power loss. |
| MIGRATION-UNIT-5 | New SQLite future-version evidence-preservation rejection; typed/redacted server mapping unit; new isolated-schema PG17 gate (environment-blocked live). |
| MIGRATION-UNIT-6 | Existing v1/profile/SecretStore reopen tests; new v6 profile/enrollment snapshot across upgrade and two reopens. |
| MIGRATION-UNIT-7 | New v6 library, profile, root-binding ID, remote root and checkpoint snapshot; existing host restart and missing/root-loss tests. Physical root remains in existing configuration/root markers. |
| MIGRATION-UNIT-8 | New pending, submitting, completed and conflict intent snapshots; existing namespace/upload crash and lost-response matrices preserve request identity and avoid duplicate commits. |
| MIGRATION-UNIT-9 | New unresolved v6 conflict snapshot and existing conflict detection/resolution reopen/stale-fencing tests. |
| MIGRATION-UNIT-10 | Strengthened pause-store test constructs a new store while paused; existing runtime pause and launch tests. |
| MIGRATION-UNIT-11 | Existing ordinary-uninstall/reinstall/config/state/credential/external-content lifecycle matrix. |
| MIGRATION-UNIT-12 | Existing purge/link containment matrix plus new escaped-parent/final-symlink regression for both `/etc` and `/var/lib`. |

New migration fixtures contain only non-secret IDs, metadata and synthetic
one-byte acknowledgement evidence. SecretStore ownership, IPC validation,
containment, redaction and existing recovery fencing are preserved. Unknown
state is never interpreted as a deleted root or a reset request. Runtime recovery
conditions reconstruct from canonical state and current probes; controller
`OutcomeUnknown` is not a separate durable database state.

A disposable before/after reproduction showed the previous purge script exited
successfully and removed an external symlink reached through an escaped `/etc`
parent. The hardened script exits with failure and preserves both the external
link and its content. Redirected parents inside the staged root are also rejected, preventing purge
of unrelated internal state. Existing safe final-link unlink behavior remains covered.

Local PostgreSQL binaries are absent. Docker is installed but socket access is
denied; noninteractive sudo requires a password. No established accessible PG17
service was available. Live PostgreSQL migration, historical data and server
startup gates are **blocked by environment**. PG tests remain ignored during the
ordinary workspace run and must not be counted as live passes. No new database
infrastructure was constructed. Real-host package-manager, native Windows and
physical power-loss acceptance are outside this local audit. The expensive
Prompt113 two-root deterministic release build was not rerun.

Upgrade/rollback/ownership and version-skew policy is documented consistently in
`docs/en/UPGRADE_SAFETY.md` and `docs/vi/UPGRADE_SAFETY.md`. There is no new protocol
negotiation or automatic downgrade. Staged package replacement can leave partial
application payloads on failure; durable state is preserved for reinstall.

## Validation

The first workspace run passed the new SQLite matrix, both existing large
snapshot fixtures, durable pause, and all 37 staged lifecycle tests, then failed
`packaged_binary_matches_release_sha`. The existing package manifest identified
source `964d714362c246258e2bd3b0cc57742e28a3ca8a`; its desktop hash
`9ab90fb8f493fc5f203febbbe36ac0f5642c93e4261196c31412b9786ae252f7`
matched the DEB, while the release desktop hash was
`f454d1fe547b812948f5b9b2645633ef8fdf0ae4dba78a69a263493a2f4bb51b`.
The strict source-fingerprint validator also rejected that stale manifest.
Old artifacts were preserved outside the repository before rebuilding all three
release executables and both packages with the existing canonical builder.
No parity test or artifact policy was relaxed. This is a single-checkout package
refresh, not the Prompt113 two-root determinism experiment.

The optional paired P113 manifest comparison encountered a secondary fixture
from source `964d714362c246258e2bd3b0cc57742e28a3ca8a`, fingerprint
`b649a07bc5b69d507e96e00d12bf7cbbf5e723afe66cab64ef305c25a0c2580f`,
epoch `1789886669`. These are different inputs from Prompt115; equality is not
a determinism claim for those inputs. That coherent old generated directory was
preserved in a temporary archive outside the active artifact lookup paths.
The optional paired comparison is **skipped**, as the instruction excludes a
new expensive two-root build. Primary current-source validation and all real
artifact mutation checks remain mandatory and unchanged. No reference hashes
were replaced with assumed values and no validator/test implementation changed.

Final sealed-source validation:

| Command / gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **passed** |
| `CARGO_BUILD_JOBS=1 cargo check --workspace --locked` | **passed** |
| `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | **passed** |
| `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` | **passed**: 972 passed, zero failed, 443 ignored across 86 reported suites. Ignored tests are not acceptance evidence. |
| `cargo deny check` | **passed** |
| `git diff --check` | **passed** |
| Focused SQLite migration safety | **passed**: six tests, including prefixes 1–7, reopen, failed transaction, unknown shapes and future versions; all included in the 285 passing client-sync unit tests. |
| Focused metadata migration/readiness units | **passed**: five tests. |
| Staged lifecycle/native packaging/production packaging | **passed**: 37 + 26 + 11 = 74 tests, including both new purge regressions and actual packaged executable parity. |
| Canonical single-root DEB/RPM build and strict current-source artifact validator | **passed**; final documentation refresh uses the same cached build and validator. |
| Release-artifact unit suite | **passed**: seven test functions; the optional paired two-root comparison within ARTIFACT-UNIT-1 is **skipped**, as detailed above. Current primary artifact and mutation checks executed. |
| Live PG17 fresh/historical/startup/future/corrupt-schema gates | **blocked by environment**; compiled, ignored tests are not live passes. |
| New two-root determinism run | **skipped** per Prompt115 scope. |
| Physical power loss, real-host package-manager installation, native Windows | **skipped**; disposable SQL failure and staged Linux fixtures do not establish these acceptance claims. |

The final documentation update changes no executable source. Its package manifest
is refreshed and validated before the explicit source staging audit.

## Explicit Prompt115 source manifest

Only these paths belong to the Prompt115 commit:

```text
crates/client-sync/src/error.rs
crates/client-sync/src/host.rs
crates/client-sync/src/lib.rs
crates/client-sync/src/local_migrations.rs
crates/client-sync/src/local_migrations/tests.rs
crates/client-sync/src/state.rs
crates/client/src/config.rs
crates/metadata/src/errors.rs
crates/metadata/src/migrations.rs
crates/metadata/tests/linux_install_lifecycle.rs
crates/metadata/tests/pg17_migration_gate_postgres.rs
deploy/install/uninstall.sh
docs/PROMPT115_UPGRADE_AUDIT.md
docs/en/TESTING.md
docs/en/UPGRADE_SAFETY.md
docs/vi/TESTING.md
docs/vi/UPGRADE_SAFETY.md
```

Disposable SQLite/PostgreSQL fixtures, packages, build artifacts, logs and secrets
are excluded. Commit SHA and remote equality are verified after the single
Prompt115 commit and push; they are reported outside this self-referential file.
