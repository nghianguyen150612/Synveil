PRAGMA foreign_keys = ON;

CREATE TABLE replicas (
    library_id TEXT PRIMARY KEY CHECK(length(library_id) = 36),
    owner_user_id TEXT NOT NULL CHECK(length(owner_user_id) = 36),
    device_id TEXT NOT NULL CHECK(length(device_id) = 36),
    root_binding_id TEXT NOT NULL UNIQUE CHECK(length(root_binding_id) = 36),
    root_node_id TEXT CHECK(root_node_id IS NULL OR length(root_node_id) = 36),
    journal_epoch INTEGER NOT NULL DEFAULT 0 CHECK(journal_epoch >= 0),
    applied_sequence INTEGER NOT NULL DEFAULT 0 CHECK(applied_sequence >= 0),
    acknowledged_sequence INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged_sequence >= 0 AND acknowledged_sequence <= applied_sequence),
    status TEXT NOT NULL DEFAULT 'IDLE' CHECK(status IN ('IDLE','BOOTSTRAPPING','APPLYING','ACK_PENDING','BLOCKED_LOCAL_ISSUE','OFFLINE','ERROR')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0)
) STRICT;

CREATE TABLE local_nodes (
    library_id TEXT NOT NULL,
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    parent_node_id TEXT CHECK(parent_node_id IS NULL OR length(parent_node_id) = 36),
    relative_path TEXT NOT NULL CHECK(length(relative_path) <= 32768),
    collision_key TEXT NOT NULL CHECK(length(collision_key) <= 32768),
    logical_name TEXT NOT NULL CHECK(length(CAST(logical_name AS BLOB)) BETWEEN 1 AND 1024),
    node_kind TEXT NOT NULL CHECK(node_kind IN ('FILE','DIRECTORY')),
    node_state TEXT NOT NULL CHECK(node_state IN ('ACTIVE','TRASHED')),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    current_version_id TEXT CHECK(current_version_id IS NULL OR length(current_version_id) = 36),
    content_length INTEGER CHECK(content_length IS NULL OR content_length >= 0),
    content_sha256 BLOB CHECK(content_sha256 IS NULL OR length(content_sha256) = 32),
    local_length INTEGER CHECK(local_length IS NULL OR local_length >= 0),
    local_sha256 BLOB CHECK(local_sha256 IS NULL OR length(local_sha256) = 32),
    bootstrap_generation INTEGER NOT NULL DEFAULT 0 CHECK(bootstrap_generation >= 0),
    present INTEGER NOT NULL CHECK(present IN (0,1)),
    quarantine_relative_path TEXT CHECK(quarantine_relative_path IS NULL OR length(quarantine_relative_path) <= 32768),
    PRIMARY KEY(library_id, node_id),
    UNIQUE(library_id, collision_key),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE,
    FOREIGN KEY(library_id, parent_node_id) REFERENCES local_nodes(library_id, node_id) DEFERRABLE INITIALLY DEFERRED,
    CHECK((node_kind = 'DIRECTORY' AND current_version_id IS NULL AND content_length IS NULL AND content_sha256 IS NULL AND local_length IS NULL AND local_sha256 IS NULL)
       OR (node_kind = 'FILE')),
    CHECK((current_version_id IS NULL AND content_length IS NULL AND content_sha256 IS NULL)
       OR (current_version_id IS NOT NULL AND content_length IS NOT NULL AND content_sha256 IS NOT NULL)),
    CHECK((local_length IS NULL AND local_sha256 IS NULL) OR (local_length IS NOT NULL AND local_sha256 IS NOT NULL))
) STRICT;

CREATE INDEX local_nodes_parent_idx ON local_nodes(library_id, parent_node_id);

CREATE TABLE bootstrap_sessions (
    library_id TEXT PRIMARY KEY,
    bootstrap_id TEXT NOT NULL CHECK(length(bootstrap_id) = 36),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    snapshot_epoch INTEGER NOT NULL CHECK(snapshot_epoch >= 0),
    resume_sequence INTEGER NOT NULL CHECK(resume_sequence >= 0),
    manifest_item_count INTEGER NOT NULL CHECK(manifest_item_count >= 0),
    state TEXT NOT NULL CHECK(state IN ('FETCHING','MANIFEST_DURABLE','APPLYING','LOCAL_COMPLETE','COMPLETION_PENDING')),
    next_cursor BLOB CHECK(next_cursor IS NULL OR length(next_cursor) BETWEEN 1 AND 4096),
    completion_evidence BLOB CHECK(completion_evidence IS NULL OR length(completion_evidence) BETWEEN 1 AND 4096),
    terminal_fetched INTEGER NOT NULL DEFAULT 0 CHECK(terminal_fetched IN (0,1)),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE bootstrap_nodes (
    library_id TEXT NOT NULL,
    bootstrap_id TEXT NOT NULL,
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    parent_node_id TEXT CHECK(parent_node_id IS NULL OR length(parent_node_id) = 36),
    logical_name TEXT NOT NULL CHECK(length(CAST(logical_name AS BLOB)) BETWEEN 1 AND 1024),
    node_kind TEXT NOT NULL CHECK(node_kind IN ('FILE','DIRECTORY')),
    node_state TEXT NOT NULL CHECK(node_state IN ('ACTIVE','TRASHED')),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    current_version_id TEXT CHECK(current_version_id IS NULL OR length(current_version_id) = 36),
    content_length INTEGER CHECK(content_length IS NULL OR content_length >= 0),
    content_sha256 BLOB CHECK(content_sha256 IS NULL OR length(content_sha256) = 32),
    applied INTEGER NOT NULL DEFAULT 0 CHECK(applied IN (0,1)),
    PRIMARY KEY(library_id, bootstrap_id, node_id),
    FOREIGN KEY(library_id) REFERENCES bootstrap_sessions(library_id) ON DELETE CASCADE,
    CHECK((current_version_id IS NULL AND content_length IS NULL AND content_sha256 IS NULL)
       OR (current_version_id IS NOT NULL AND content_length IS NOT NULL AND content_sha256 IS NOT NULL))
) STRICT;

CREATE TABLE pending_pages (
    library_id TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL CHECK(epoch >= 0),
    from_sequence INTEGER NOT NULL CHECK(from_sequence >= 0),
    through_sequence INTEGER NOT NULL CHECK(through_sequence >= from_sequence),
    high_watermark INTEGER NOT NULL CHECK(high_watermark >= through_sequence),
    has_more INTEGER NOT NULL CHECK(has_more IN (0,1)),
    ack_evidence BLOB CHECK(ack_evidence IS NULL OR length(ack_evidence) BETWEEN 1 AND 4096),
    state TEXT NOT NULL CHECK(state IN ('RECEIVED','APPLYING','LOCALLY_COMMITTED','ACK_PENDING')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE pending_events (
    library_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    event_id TEXT NOT NULL CHECK(length(event_id) = 36),
    schema_version INTEGER NOT NULL CHECK(schema_version > 0),
    resource_id TEXT NOT NULL CHECK(length(resource_id) = 36),
    change_kind TEXT NOT NULL CHECK(change_kind IN ('NODE_CREATED','NODE_RENAMED','NODE_MOVED','NODE_TRASHED','NODE_RESTORED','FILE_CONTENT_COMMITTED','FILE_VERSION_RESTORED','NODE_PURGED')),
    resource_revision INTEGER NOT NULL CHECK(resource_revision >= 0),
    parent_node_id TEXT CHECK(parent_node_id IS NULL OR length(parent_node_id) = 36),
    logical_name TEXT CHECK(logical_name IS NULL OR length(CAST(logical_name AS BLOB)) BETWEEN 1 AND 1024),
    node_kind TEXT CHECK(node_kind IS NULL OR node_kind IN ('FILE','DIRECTORY')),
    node_state TEXT CHECK(node_state IS NULL OR node_state IN ('ACTIVE','TRASHED')),
    current_version_id TEXT CHECK(current_version_id IS NULL OR length(current_version_id) = 36),
    content_length INTEGER CHECK(content_length IS NULL OR content_length >= 0),
    content_sha256 BLOB CHECK(content_sha256 IS NULL OR length(content_sha256) = 32),
    PRIMARY KEY(library_id, sequence),
    UNIQUE(library_id, event_id),
    FOREIGN KEY(library_id) REFERENCES pending_pages(library_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE pending_acknowledgements (
    library_id TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL CHECK(epoch >= 0),
    from_sequence INTEGER NOT NULL CHECK(from_sequence >= 0),
    through_sequence INTEGER NOT NULL CHECK(through_sequence > from_sequence),
    high_watermark INTEGER NOT NULL CHECK(high_watermark >= through_sequence),
    evidence BLOB NOT NULL CHECK(length(evidence) BETWEEN 1 AND 4096),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER pending_ack_requires_local_apply
BEFORE INSERT ON pending_acknowledgements
FOR EACH ROW
WHEN NEW.through_sequence > (
    SELECT applied_sequence FROM replicas WHERE library_id = NEW.library_id
)
BEGIN
    SELECT RAISE(ABORT, 'pending acknowledgement exceeds durable local apply');
END;

CREATE TABLE local_operations (
    operation_id TEXT PRIMARY KEY CHECK(length(operation_id) = 36),
    library_id TEXT NOT NULL,
    node_id TEXT NOT NULL CHECK(length(node_id) = 36),
    server_sequence INTEGER CHECK(server_sequence IS NULL OR server_sequence >= 0),
    bootstrap_generation INTEGER CHECK(bootstrap_generation IS NULL OR bootstrap_generation >= 0),
    operation_kind TEXT NOT NULL CHECK(operation_kind IN ('CREATE_DIRECTORY','REPLACE_FILE','RENAME','MOVE','TRASH','RESTORE','PURGE')),
    source_relative_path TEXT CHECK(source_relative_path IS NULL OR length(source_relative_path) <= 32768),
    destination_relative_path TEXT CHECK(destination_relative_path IS NULL OR length(destination_relative_path) <= 32768),
    staging_relative_path TEXT CHECK(staging_relative_path IS NULL OR length(staging_relative_path) <= 32768),
    expected_kind TEXT CHECK(expected_kind IS NULL OR expected_kind IN ('FILE','DIRECTORY')),
    expected_length INTEGER CHECK(expected_length IS NULL OR expected_length >= 0),
    expected_sha256 BLOB CHECK(expected_sha256 IS NULL OR length(expected_sha256) = 32),
    desired_revision INTEGER NOT NULL CHECK(desired_revision >= 0),
    desired_version_id TEXT CHECK(desired_version_id IS NULL OR length(desired_version_id) = 36),
    desired_length INTEGER CHECK(desired_length IS NULL OR desired_length >= 0),
    desired_sha256 BLOB CHECK(desired_sha256 IS NULL OR length(desired_sha256) = 32),
    state TEXT NOT NULL CHECK(state IN ('PREPARED','FILESYSTEM_APPLIED','DATABASE_COMMITTED','NEEDS_ATTENTION')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE,
    CHECK((server_sequence IS NULL) != (bootstrap_generation IS NULL))
) STRICT;

CREATE INDEX local_operations_recovery_idx ON local_operations(library_id, state, created_at_ms);

CREATE TABLE applied_events (
    library_id TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK(epoch >= 0),
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    event_id TEXT NOT NULL CHECK(length(event_id) = 36),
    resource_id TEXT NOT NULL CHECK(length(resource_id) = 36),
    resource_revision INTEGER NOT NULL CHECK(resource_revision >= 0),
    PRIMARY KEY(library_id, epoch, sequence),
    UNIQUE(library_id, event_id),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE local_apply_issues (
    issue_id TEXT PRIMARY KEY CHECK(length(issue_id) = 36),
    library_id TEXT NOT NULL,
    node_id TEXT CHECK(node_id IS NULL OR length(node_id) = 36),
    server_sequence INTEGER CHECK(server_sequence IS NULL OR server_sequence >= 0),
    bootstrap_generation INTEGER CHECK(bootstrap_generation IS NULL OR bootstrap_generation >= 0),
    issue_kind TEXT NOT NULL CHECK(issue_kind IN ('LOCAL_DIVERGENCE','LOCAL_PATH_OCCUPIED','LOCAL_NAME_UNREPRESENTABLE','LOCAL_NAME_COLLISION','LOCAL_PARENT_MISSING','LOCAL_TYPE_MISMATCH','LOCAL_IO_UNAVAILABLE','CONTENT_INTEGRITY_MISMATCH','LOCAL_RECOVERY_AMBIGUOUS')),
    expected_state TEXT NOT NULL CHECK(length(expected_state) BETWEEN 1 AND 1024),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    resolved_at_ms INTEGER CHECK(resolved_at_ms IS NULL OR resolved_at_ms >= created_at_ms),
    FOREIGN KEY(library_id) REFERENCES replicas(library_id) ON DELETE CASCADE
) STRICT;

CREATE UNIQUE INDEX one_unresolved_issue_per_fact
ON local_apply_issues(library_id, COALESCE(node_id, ''), COALESCE(server_sequence, -1), COALESCE(bootstrap_generation, -1), issue_kind)
WHERE resolved_at_ms IS NULL;
