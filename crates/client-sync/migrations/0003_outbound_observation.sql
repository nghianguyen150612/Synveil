-- Prompt 38: durable local filesystem observation. These tables are strictly
-- local control-plane state; they neither contain file bytes nor map to a
-- server ClientMutationId.

CREATE TABLE observation_state (
    library_id TEXT PRIMARY KEY REFERENCES replicas(library_id) ON DELETE CASCADE,
    rescan_required INTEGER NOT NULL DEFAULT 1 CHECK(rescan_required IN (0,1)),
    scan_active INTEGER NOT NULL DEFAULT 0 CHECK(scan_active IN (0,1)),
    scan_generation INTEGER NOT NULL DEFAULT 0 CHECK(scan_generation >= 0),
    last_reconciled_at_ms INTEGER CHECK(last_reconciled_at_ms IS NULL OR last_reconciled_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0)
) STRICT;

-- The current physical projection of a server Node when a local intent has
-- changed its path or observed bytes. The immutable server projection remains
-- in local_nodes; this overlay prevents an unsubmitted local rename from being
-- mistaken for a server acknowledgement.
CREATE TABLE observation_nodes (
    library_id TEXT NOT NULL,
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    relative_path TEXT NOT NULL CHECK(length(relative_path) <= 32768),
    present INTEGER NOT NULL CHECK(present IN (0,1)),
    observed_kind TEXT CHECK(observed_kind IS NULL OR observed_kind IN ('FILE','DIRECTORY')),
    observed_length INTEGER CHECK(observed_length IS NULL OR observed_length >= 0),
    observed_sha256 BLOB CHECK(observed_sha256 IS NULL OR length(observed_sha256) = 32),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    PRIMARY KEY(library_id, node_id),
    UNIQUE(library_id, relative_path),
    FOREIGN KEY(library_id, node_id) REFERENCES local_nodes(library_id, node_id) ON DELETE CASCADE,
    CHECK((present = 0 AND observed_kind IS NULL AND observed_length IS NULL AND observed_sha256 IS NULL)
       OR (present = 1 AND observed_kind = 'DIRECTORY' AND observed_length IS NULL AND observed_sha256 IS NULL)
       OR (present = 1 AND observed_kind = 'FILE' AND observed_length IS NOT NULL AND observed_sha256 IS NOT NULL))
) STRICT;

CREATE TABLE outbound_intents (
    intent_id TEXT PRIMARY KEY CHECK(length(intent_id) = 36),
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    node_id TEXT CHECK(node_id IS NULL OR length(node_id) = 36),
    parent_node_id TEXT CHECK(parent_node_id IS NULL OR length(parent_node_id) = 36),
    intent_kind TEXT NOT NULL CHECK(intent_kind IN ('CREATE_DIRECTORY','CREATE_FILE','RENAME_NODE','MOVE_NODE','DELETE_OR_TRASH_NODE','MODIFY_FILE_CONTENT')),
    state TEXT NOT NULL CHECK(state IN ('PENDING','BLOCKED','SUPERSEDED','CANCELLED','NEEDS_REBASE_VALIDATION')),
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

CREATE UNIQUE INDEX active_outbound_intent_dedupe
ON outbound_intents(library_id, dedupe_version, dedupe_sha256)
WHERE state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION');

CREATE INDEX outbound_intents_pending_idx
ON outbound_intents(library_id, state, created_at_ms, intent_id);

CREATE INDEX outbound_intents_node_idx
ON outbound_intents(library_id, node_id, state, created_at_ms);

-- Durable evidence that a particular state was created by a Prompt 36 local
-- operation. Suppression is content/path/node proof based, never a timer.
CREATE TABLE observation_suppressions (
    operation_id TEXT NOT NULL CHECK(length(operation_id) = 36),
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    expected_relative_path TEXT NOT NULL CHECK(length(expected_relative_path) <= 32768),
    expected_present INTEGER NOT NULL CHECK(expected_present IN (0,1)),
    expected_kind TEXT CHECK(expected_kind IS NULL OR expected_kind IN ('FILE','DIRECTORY')),
    expected_length INTEGER CHECK(expected_length IS NULL OR expected_length >= 0),
    expected_sha256 BLOB CHECK(expected_sha256 IS NULL OR length(expected_sha256) = 32),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    PRIMARY KEY(operation_id, expected_relative_path),
    CHECK((expected_present = 0 AND expected_kind IS NULL AND expected_length IS NULL AND expected_sha256 IS NULL)
       OR (expected_present = 1 AND expected_kind = 'DIRECTORY' AND expected_length IS NULL AND expected_sha256 IS NULL)
       OR (expected_present = 1 AND expected_kind = 'FILE' AND expected_length IS NOT NULL AND expected_sha256 IS NOT NULL))
) STRICT;

CREATE INDEX observation_suppressions_path_idx
ON observation_suppressions(library_id, expected_relative_path, created_at_ms);

CREATE TABLE observation_issues (
    issue_id TEXT PRIMARY KEY CHECK(length(issue_id) = 36),
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    node_id TEXT CHECK(node_id IS NULL OR length(node_id) = 36),
    relative_path TEXT CHECK(relative_path IS NULL OR length(relative_path) <= 32768),
    issue_kind TEXT NOT NULL CHECK(issue_kind IN ('WATCHER_OVERFLOW','OBSERVATION_BUSY','UNSUPPORTED_ENTRY_TYPE','UNREPRESENTABLE_NAME','NAME_COLLISION','AMBIGUOUS_RENAME','ROOT_INVALID','BASE_STATE_CHANGED','HASH_UNSTABLE')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    resolved_at_ms INTEGER CHECK(resolved_at_ms IS NULL OR resolved_at_ms >= created_at_ms)
) STRICT;

CREATE UNIQUE INDEX one_unresolved_observation_issue_per_fact
ON observation_issues(library_id, COALESCE(node_id, ''), COALESCE(relative_path, ''), issue_kind)
WHERE resolved_at_ms IS NULL;

CREATE TABLE observation_scan_work (
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    relative_path TEXT NOT NULL CHECK(length(relative_path) <= 32768),
    cursor_name TEXT CHECK(cursor_name IS NULL OR length(CAST(cursor_name AS BLOB)) BETWEEN 1 AND 1024),
    PRIMARY KEY(library_id, generation, relative_path)
) STRICT;

CREATE TABLE observation_scan_seen (
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    PRIMARY KEY(library_id, generation, node_id)
) STRICT;

-- Server Nodes are covered by `observation_scan_seen`; locally created
-- entries deliberately have no forgeable NodeId. Track their durable intent
-- identity separately so a restart scan can cancel a vanished create without
-- repeatedly reprocessing every still-present create.
CREATE TABLE observation_scan_seen_intents (
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    intent_id TEXT NOT NULL CHECK(length(intent_id) = 36),
    PRIMARY KEY(library_id, generation, intent_id),
    FOREIGN KEY(intent_id) REFERENCES outbound_intents(intent_id) ON DELETE CASCADE
) STRICT;

-- Forward migration for replicas bound before Prompt 38 existed. New bindings
-- also insert this row idempotently in LocalStateStore::bind_replica_inner.
INSERT OR IGNORE INTO observation_state (library_id, updated_at_ms)
SELECT library_id, updated_at_ms FROM replicas;
