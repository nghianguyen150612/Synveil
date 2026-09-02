PRAGMA foreign_keys = ON;

-- SQLite cannot alter a table CHECK constraint in place. Rebuild the
-- Prompt 38 outbound_intents table while preserving dependent scan references
-- and recreating the original indexes with the expanded Prompt 39 lifecycle.
ALTER TABLE observation_scan_seen_intents RENAME TO observation_scan_seen_intents_prompt38;
DROP INDEX active_outbound_intent_dedupe;
DROP INDEX outbound_intents_pending_idx;
DROP INDEX outbound_intents_node_idx;
ALTER TABLE outbound_intents RENAME TO outbound_intents_prompt38;

CREATE TABLE outbound_intents (
    intent_id TEXT PRIMARY KEY CHECK(length(intent_id) = 36),
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    node_id TEXT CHECK(node_id IS NULL OR length(node_id) = 36),
    parent_node_id TEXT CHECK(parent_node_id IS NULL OR length(parent_node_id) = 36),
    intent_kind TEXT NOT NULL CHECK(intent_kind IN ('CREATE_DIRECTORY','CREATE_FILE','RENAME_NODE','MOVE_NODE','DELETE_OR_TRASH_NODE','MODIFY_FILE_CONTENT')),
    state TEXT NOT NULL CHECK(state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','SERVER_APPLIED','CONFLICT','BLOCKED','SUPERSEDED','CANCELLED','NEEDS_REBASE_VALIDATION','RECONCILED')),
    observed_relative_path TEXT NOT NULL CHECK(length(observed_relative_path) <= 32768),
    old_relative_path TEXT CHECK(old_relative_path IS NULL OR length(old_relative_path) <= 32768),
    observed_kind TEXT CHECK(observed_kind IS NULL OR observed_kind IN ('FILE','DIRECTORY')),
    observed_length INTEGER CHECK(observed_length IS NULL OR observed_length >= 0),
    observed_sha256 BLOB CHECK(observed_sha256 IS NULL OR length(observed_sha256) = 32),
    base_epoch INTEGER NOT NULL CHECK(base_epoch >= 0),
    base_applied_sequence INTEGER NOT NULL CHECK(base_applied_sequence >= 0),
    base_revision INTEGER CHECK(base_revision IS NULL OR base_revision >= 0),
    base_current_version_id TEXT CHECK(base_current_version_id IS NULL OR length(base_current_version_id) = 36),
    base_parent_revision INTEGER CHECK(base_parent_revision IS NULL OR base_parent_revision >= 0),
    dedupe_version INTEGER NOT NULL CHECK(dedupe_version = 1),
    dedupe_sha256 BLOB NOT NULL CHECK(length(dedupe_sha256) = 32),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    CHECK((observed_kind IS NULL AND observed_length IS NULL AND observed_sha256 IS NULL)
       OR (observed_kind = 'DIRECTORY' AND observed_length IS NULL AND observed_sha256 IS NULL)
       OR (observed_kind = 'FILE' AND observed_length IS NOT NULL AND observed_sha256 IS NOT NULL))
) STRICT;

INSERT INTO outbound_intents (
    intent_id, library_id, node_id, parent_node_id, intent_kind, state,
    observed_relative_path, old_relative_path, observed_kind, observed_length,
    observed_sha256, base_epoch, base_applied_sequence, base_revision,
    base_current_version_id, base_parent_revision, dedupe_version,
    dedupe_sha256, created_at_ms, updated_at_ms
)
SELECT
    intent_id, library_id, node_id, parent_node_id, intent_kind, state,
    observed_relative_path, old_relative_path, observed_kind, observed_length,
    observed_sha256, base_epoch, base_applied_sequence, base_revision,
    base_current_version_id, base_parent_revision, dedupe_version,
    dedupe_sha256, created_at_ms, updated_at_ms
FROM outbound_intents_prompt38;

CREATE UNIQUE INDEX active_outbound_intent_dedupe
ON outbound_intents(library_id, dedupe_version, dedupe_sha256)
WHERE state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED','NEEDS_REBASE_VALIDATION');

CREATE INDEX outbound_intents_pending_idx
ON outbound_intents(library_id, state, created_at_ms, intent_id);

CREATE INDEX outbound_intents_node_idx
ON outbound_intents(library_id, node_id, state, created_at_ms);

CREATE TABLE observation_scan_seen_intents (
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    intent_id TEXT NOT NULL CHECK(length(intent_id) = 36),
    PRIMARY KEY(library_id, generation, intent_id),
    FOREIGN KEY(intent_id) REFERENCES outbound_intents(intent_id) ON DELETE CASCADE
) STRICT;

INSERT INTO observation_scan_seen_intents (library_id, generation, intent_id)
SELECT library_id, generation, intent_id FROM observation_scan_seen_intents_prompt38;

DROP TABLE observation_scan_seen_intents_prompt38;
DROP TABLE outbound_intents_prompt38;

CREATE TABLE outbound_mutation_requests (
    intent_id TEXT PRIMARY KEY CHECK(length(intent_id) = 36),
    mutation_id TEXT NOT NULL UNIQUE CHECK(length(mutation_id) = 36),
    mutation_kind TEXT NOT NULL CHECK(mutation_kind IN ('CREATE_DIRECTORY','RENAME_NODE','MOVE_NODE','TRASH_NODE')),
    base_epoch INTEGER NOT NULL CHECK(base_epoch >= 0),
    base_sequence INTEGER NOT NULL CHECK(base_sequence >= 0),
    request_json TEXT NOT NULL CHECK(length(CAST(request_json AS BLOB)) BETWEEN 1 AND 16384),
    fingerprint_version INTEGER NOT NULL CHECK(fingerprint_version = 1),
    fingerprint_sha256 BLOB NOT NULL CHECK(length(fingerprint_sha256) = 32),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    FOREIGN KEY(intent_id) REFERENCES outbound_intents(intent_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE outbound_upload_sessions (
    intent_id TEXT PRIMARY KEY CHECK(length(intent_id) = 36),
    upload_session_id TEXT CHECK(upload_session_id IS NULL OR length(upload_session_id) = 36),
    operation TEXT NOT NULL CHECK(operation IN ('CREATE_FILE','REPLACE_CONTENT')),
    staging_relative_path TEXT NOT NULL CHECK(length(staging_relative_path) <= 32768),
    expected_length INTEGER NOT NULL CHECK(expected_length >= 0),
    expected_sha256 BLOB NOT NULL CHECK(length(expected_sha256) = 32),
    acknowledged_offset INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged_offset >= 0 AND acknowledged_offset <= expected_length),
    state TEXT NOT NULL CHECK(state IN ('STAGED','SESSION_CREATED','UPLOADING','COMPLETING','COMPLETED','FAILED')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    FOREIGN KEY(intent_id) REFERENCES outbound_intents(intent_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE outbound_submission_results (
    intent_id TEXT PRIMARY KEY CHECK(length(intent_id) = 36),
    mutation_id TEXT CHECK(mutation_id IS NULL OR length(mutation_id) = 36),
    upload_session_id TEXT CHECK(upload_session_id IS NULL OR length(upload_session_id) = 36),
    outcome TEXT NOT NULL CHECK(outcome IN ('SERVER_APPLIED','CONFLICT','BLOCKED')),
    resource_id TEXT CHECK(resource_id IS NULL OR length(resource_id) = 36),
    result_revision INTEGER CHECK(result_revision IS NULL OR result_revision >= 0),
    file_version_id TEXT CHECK(file_version_id IS NULL OR length(file_version_id) = 36),
    result_length INTEGER CHECK(result_length IS NULL OR result_length >= 0),
    result_sha256 BLOB CHECK(result_sha256 IS NULL OR length(result_sha256) = 32),
    journal_event_id TEXT CHECK(journal_event_id IS NULL OR length(journal_event_id) = 36),
    journal_sequence INTEGER CHECK(journal_sequence IS NULL OR journal_sequence >= 0),
    conflict_id TEXT CHECK(conflict_id IS NULL OR length(conflict_id) = 36),
    conflict_reason TEXT CHECK(conflict_reason IS NULL OR length(conflict_reason) BETWEEN 1 AND 128),
    replayed INTEGER NOT NULL DEFAULT 0 CHECK(replayed IN (0,1)),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    FOREIGN KEY(intent_id) REFERENCES outbound_intents(intent_id) ON DELETE CASCADE,
    CHECK((file_version_id IS NULL AND result_length IS NULL AND result_sha256 IS NULL)
       OR (file_version_id IS NOT NULL AND result_length IS NOT NULL AND result_sha256 IS NOT NULL))
) STRICT;
