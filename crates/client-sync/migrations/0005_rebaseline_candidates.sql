-- Prompt 84: a candidate is deliberately separate from the authoritative
-- local_nodes projection.  SQLite commits the final replacement and this
-- handoff marker together; outbound tables are not referenced or rebuilt.
CREATE TABLE rebaseline_candidates (
    library_id TEXT PRIMARY KEY REFERENCES replicas(library_id) ON DELETE CASCADE,
    snapshot_id TEXT NOT NULL CHECK(length(snapshot_id) = 36),
    journal_epoch INTEGER NOT NULL CHECK(journal_epoch > 0),
    resume_sequence INTEGER NOT NULL CHECK(resume_sequence >= 0),
    expected_count INTEGER NOT NULL CHECK(expected_count >= 0),
    received_count INTEGER NOT NULL DEFAULT 0 CHECK(received_count >= 0 AND received_count <= expected_count),
    next_cursor BLOB CHECK(next_cursor IS NULL OR length(next_cursor) BETWEEN 1 AND 4096),
    terminal_fetched INTEGER NOT NULL DEFAULT 0 CHECK(terminal_fetched IN (0,1)),
    state TEXT NOT NULL CHECK(state IN ('FETCHING','COMPLETE','FAILED')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0)
) STRICT;

CREATE TABLE rebaseline_candidate_nodes (
    library_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL CHECK(length(snapshot_id) = 36),
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    parent_node_id TEXT CHECK(parent_node_id IS NULL OR length(parent_node_id) = 36),
    logical_name TEXT NOT NULL CHECK(length(CAST(logical_name AS BLOB)) BETWEEN 1 AND 1024),
    node_kind TEXT NOT NULL CHECK(node_kind IN ('FILE','DIRECTORY')),
    node_state TEXT NOT NULL CHECK(node_state IN ('ACTIVE','TRASHED')),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    current_version_id TEXT CHECK(current_version_id IS NULL OR length(current_version_id) = 36),
    content_length INTEGER CHECK(content_length IS NULL OR content_length >= 0),
    content_sha256 BLOB CHECK(content_sha256 IS NULL OR length(content_sha256) = 32),
    PRIMARY KEY(library_id, snapshot_id, node_id),
    FOREIGN KEY(library_id) REFERENCES rebaseline_candidates(library_id) ON DELETE CASCADE,
    CHECK((current_version_id IS NULL AND content_length IS NULL AND content_sha256 IS NULL)
       OR (current_version_id IS NOT NULL AND content_length IS NOT NULL AND content_sha256 IS NOT NULL))
) STRICT;

CREATE TABLE rebaseline_candidate_cursors (
    library_id TEXT NOT NULL REFERENCES rebaseline_candidates(library_id) ON DELETE CASCADE,
    cursor BLOB NOT NULL CHECK(length(cursor) BETWEEN 1 AND 4096),
    PRIMARY KEY(library_id, cursor)
) STRICT;

CREATE TABLE rebaseline_applied_handoffs (
    library_id TEXT PRIMARY KEY REFERENCES replicas(library_id) ON DELETE CASCADE,
    snapshot_id TEXT NOT NULL CHECK(length(snapshot_id) = 36),
    journal_epoch INTEGER NOT NULL CHECK(journal_epoch > 0),
    resume_sequence INTEGER NOT NULL CHECK(resume_sequence >= 0),
    applied_at_ms INTEGER NOT NULL CHECK(applied_at_ms >= 0)
) STRICT;
