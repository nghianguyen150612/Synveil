# Database migrations

This directory is reserved for reviewed, ordered, forward-only PostgreSQL
migrations. `synveil-metadata` loads this directory through SQLx at runtime and
runs it through its one-shot `MigrationRunner`; SQLx owns the PostgreSQL
migration lock, checksum validation, and `_sqlx_migrations` bookkeeping.

The initial canonical domain migration creates only `users`, `devices`,
`libraries`, `nodes`, `objects`, and `file_versions`. Authentication sessions,
credentials, journals, outbox/jobs, sharing, and most replica/lifecycle work
remain later migrations. The authentication migration adds the documented
instance administrator flag, `user_credentials`, and a singleton persistent
`bootstrap_state`. The browser-session migration adds verifier-only `sessions`
rows with persistent expiry and revocation. The upload-session migration adds
the first verified `object_replicas` table and persisted resumable
`upload_sessions`; it does not add sync tables or HTTP transport. Product
migrations now add the bounded immutable file-version history index; they do
not mutate or delete historical `file_versions` rows. The restore-operation
migration adds only the owner-scoped persisted identity and committed outcome
needed to replay a successful historical-version restore without creating a
duplicate `FileVersion`; it does not add purge, retention, sync, or backup
state. The Trash-retention migration adds the single canonical `nodes.trashed_at`
timestamp and a partial `(trashed_at, id)` candidate-scan index; it does not
delete metadata, references, replicas, or object bytes. Product migrations must
be reviewed and immutable after release. A migration failure is
surfaced to the caller; the runner never drops schemas, resets a database, or
silently falls back to SQLite.
The metadata-purge migration adds the minimal completed-purge replay identity,
metadata-only object GC-candidate table, the deferrable self-parent constraint,
the Object-reference index, and the upload-parent reference index needed to
remove one node's immutable FileVersion history and evaluate survivors
set-wise in one transaction. It does not delete Object rows, ObjectReplica
rows, or object bytes, and it does not define a physical-GC grace deadline.
