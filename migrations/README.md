# Database migrations

This directory is reserved for reviewed, ordered, forward-only PostgreSQL
migrations. `synveil-metadata` loads this directory through SQLx at runtime and
runs it through its one-shot `MigrationRunner`; SQLx owns the PostgreSQL
migration lock, checksum validation, and `_sqlx_migrations` bookkeeping.

The initial canonical domain migration creates only `users`, `devices`,
`libraries`, `nodes`, `objects`, and `file_versions`. Authentication sessions,
credentials, journals, outbox/jobs, replicas, sharing, and upload state remain
later migrations. The authentication migration adds the documented instance
administrator flag, `user_credentials`, and a singleton persistent
`bootstrap_state`. The browser-session migration adds verifier-only `sessions`
rows with persistent expiry and revocation; it does not add refresh tokens,
device credentials, recovery, journals, outbox/jobs, replicas, sharing, or
upload state. The product migrations must be reviewed and immutable after
release. A migration failure is surfaced to the caller; the runner never drops
schemas, resets a database, or silently falls back to SQLite.
