# Synveil iOS — Persistence Infrastructure (`clients/ios/Infrastructure/Persistence/`)

## Purpose & Ownership
Implements `LocalCacheStoreProtocol` using a durable local SQLite database (`Library/Application Support/synveil_cache.sqlite`).

### Responsibilities
- Storing cached libraries, logical nodes, and parent-child directory tree projections.
- Storing local outbound mutation intent queue (`CREATE_DIRECTORY`, `RENAME_NODE`, `MOVE_NODE`, `TRASH_NODE`, `RESTORE_NODE`).
- Persisting sync checkpoints, sequence cursors, and pending signed ACK tokens per profile.
- Staging rebaseline manifest pages and performing atomic cache swap transactions.

## Prohibited
- Storing `svd1_` device bearer tokens, enrollment tokens, or private keys (strictly forbidden in SQLite).
- Storing raw file byte content (files are staged in `FileManager` staging storage).
