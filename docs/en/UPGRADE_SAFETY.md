# v0.1 upgrade and persistent-state safety

The frozen schema contains **36 PostgreSQL migrations**, **7 client SQLite
migrations**, and `LOCAL_SCHEMA_VERSION = 7`. Migrations are forward-only;
Synveil has no automatic downgrade or database-reset recovery path.

Stop the application before replacing its package. Preserve a verified backup
of configuration and durable state before migration. Start the new application
against the existing state, then verify profile/library identity, bindings,
checkpoint progress, unresolved conflicts, and pause state before resuming work.
A binary rollback is safe only when that binary supports the database schema;
otherwise use compatible software or an administrator-verified restore process.
Never erase a database to make an older binary start.

SQLite startup holds the existing single-writer lock, requires a complete
successful migration prefix, validates the required tables, columns, indexes,
and safety triggers, rejects unrecognized schema objects and columns, and checks
SQLx migration checksums. Unknown historical columns or foreign-key tables must
fail before migration 0004 rebuilds intents; otherwise unknown data could be lost. Existing objects
without a migration ledger are rejected. Each SQLite migration and its version
record commit in one transaction. A failed migration retains the last completed
version; restarting retries the next migration. Successfully committed previous
migrations remain applied. Interrupted execution-time bookkeeping is not a
schema failure. The tests inject a SQL failure; they do not simulate physical
power loss or damaged storage hardware.

`LOCAL_SCHEMA_UNSUPPORTED` means the database records a version newer than
this client supports: preserve it and install compatible software.
`LOCAL_SCHEMA_INVALID` means missing/invalid migration metadata or required
schema objects: preserve the database and investigate or restore a verified
backup. `LOCAL_DATABASE_UNAVAILABLE` includes unreadable/non-SQLite files.
Startup never deletes, recreates over, or repairs unknown state. These checks
validate migration bookkeeping and schema availability; they are not a complete
forensic validator for every possible malicious schema or corrupt data value.

PostgreSQL SQLx migration execution uses advisory locking, checksum validation,
and one transaction per frozen migration. Unknown recorded versions return the
redacted typed `database_schema_unsupported` failure. A failed migration connection is closed so its session advisory lock cannot
remain in the pool. Missing required current tables fail startup even when the
ledger reports every migration successful. Other migration failures
stop startup; no PostgreSQL-to-SQLite fallback or automatic reset exists.
The live PG17 migration-from-empty gate remains a separate environment gate.
The migration sequence includes data-preserving backfills for Trash timestamps,
schedule activation, misfire defaults, and retained snapshot handoff proofs.
Constraint replacement occurs inside migration transactions. Historical SQL
files must not be rewritten after release.

Persistent state has separate owners:

| State | Owner and upgrade behavior |
| --- | --- |
| Profiles and enrollment references | SQLite preserves opaque profile IDs, canonical origins, owner/device association, and credential IDs. Migration 0007 permits canonical lifecycle origin reconfiguration while keeping profile ID immutable. Legacy v1 replicas remain explicitly unbound. |
| Device credential bytes | Platform SecretStore; SQLite stores references and cleanup intents only. Database backup is not a SecretStore backup. PostgreSQL authentication verifier records are server-owned; client credential bytes are not moved into them. |
| Library bindings and sync state | SQLite retains library/profile/root-binding identity, remote root identity, checkpoint, bootstrap, outbound intent and recovery records. Desktop configuration and root markers retain physical roots. |
| Pending/ambiguous/completed work | Persistent intent/request/result identities fence recovery. Reopen does not reinterpret `SUBMITTING` as a new request or silently replay completed work. Controller `OutcomeUnknown` is a transient response classification; durable canonical state must be refreshed before retry. |
| Conflicts | Unresolved conflict identity and original intent remain durable; existing resolution actions and stale-state checks still apply. |
| User pause | `sync-state.conf` remains separate from SQLite; restart reloads it before synchronization. Unknown pause text fails closed. |
| Runtime recovery | Authentication, root availability, pending setup, and retryable server conditions are reconstructed from durable records, SecretStore, and current probes. A missing root stays `RootUnavailable` and cannot produce a mass remote delete. |

The package manifest separates application payloads from configuration and state.
Upgrade replaces package artifacts and preserves `/etc/synveil`,
`/var/lib/synveil`, client configuration/SQLite, credential references and
SecretStore, external roots, and PostgreSQL data. An interrupted package
replacement may leave a partial application payload; reinstall repairs that
payload while preserving durable data. This is not an atomic whole-package
rollback guarantee.

For a managed Host, Prompt031's `/etc/synveil/server-config.json` and protected
credential sources under `/etc/synveil/credentials` are part of the preserved
configuration boundary. Ordinary upgrade, repair, and reinstall do not rewrite
an unknown/newer schema, replace an installation ID, or rotate database or
rebaseline secrets. Missing or malformed managed state requires explicit
reconciliation. The separate destructive `--purge` operation retains the
documented `/etc/synveil` ownership scope above.

Ordinary uninstall removes only package-owned artifacts. Explicit `--purge`
removes documented Synveil-owned `/etc/synveil` and `/var/lib/synveil` state,
including administrator credentials there. It does not remove external library
content, external object/backup roots, unrelated home data, or PostgreSQL data.
Do not place external content or mounted data volumes inside purge-owned paths.
Final purge symlinks are unlinked without following targets; escaped parents
are rejected before unlinking. Parent symlinks redirecting to unrelated state
even inside the staged root are also rejected. Staged lifecycle tests do not establish real-host
package-manager or mount-race acceptance.

There is no general release-version negotiation protocol or supported arbitrary
client/server downgrade matrix. Current adapters validate the expected API,
control protocol, remote scope, and event schema (version 1); incompatible or
malformed responses fail before they become local mutation facts. Ship matched
v0.1 components and run the PostgreSQL and platform gates in the deployment
that will be supported. Migration-prefix fixtures prove the repository's frozen
schema transitions, not compatibility with unnamed historical releases.
