-- Prompt 88: durable client-local conflict evidence. The authoritative remote
-- projection remains in local_nodes and the original outbound intent remains
-- immutable apart from its lifecycle state. No content bytes, credentials, or
-- physical paths are copied into this table.
CREATE TABLE sync_conflicts (
    conflict_id TEXT PRIMARY KEY CHECK(length(conflict_id) = 36),
    library_id TEXT NOT NULL REFERENCES replicas(library_id) ON DELETE CASCADE,
    intent_id TEXT NOT NULL UNIQUE REFERENCES outbound_intents(intent_id) ON DELETE CASCADE,
    node_id TEXT CHECK(node_id IS NULL OR length(node_id) = 36),
    kind TEXT NOT NULL CHECK(kind IN (
        'REMOTE_REVISION_CHANGED',
        'REMOTE_CONTENT_CHANGED',
        'REMOTE_STATE_CHANGED',
        'REMOTE_MISSING',
        'NAME_COLLISION',
        'PARENT_CHANGED_OR_UNAVAILABLE'
    )),
    local_base_revision INTEGER CHECK(local_base_revision IS NULL OR local_base_revision >= 0),
    remote_observed_revision INTEGER CHECK(remote_observed_revision IS NULL OR remote_observed_revision >= 0),
    remote_observed_state TEXT CHECK(remote_observed_state IS NULL OR remote_observed_state IN ('ACTIVE','TRASHED')),
    remote_parent_node_id TEXT CHECK(remote_parent_node_id IS NULL OR length(remote_parent_node_id) = 36),
    remote_epoch INTEGER CHECK(remote_epoch IS NULL OR remote_epoch >= 0),
    remote_sequence INTEGER CHECK(remote_sequence IS NULL OR remote_sequence >= 0),
    detected_at_ms INTEGER NOT NULL CHECK(detected_at_ms >= 0),
    status TEXT NOT NULL CHECK(status IN ('UNRESOLVED','RESOLVED')),
    resolution_id TEXT UNIQUE CHECK(resolution_id IS NULL OR length(resolution_id) = 36),
    resolution TEXT CHECK(resolution IS NULL OR resolution IN ('ACCEPT_REMOTE','RETRY_LOCAL_AGAINST_CURRENT_BASE')),
    resolved_at_ms INTEGER CHECK(resolved_at_ms IS NULL OR resolved_at_ms >= detected_at_ms),
    replacement_intent_id TEXT UNIQUE REFERENCES outbound_intents(intent_id),
    CHECK(
        (status = 'UNRESOLVED' AND resolution_id IS NULL AND resolution IS NULL
         AND resolved_at_ms IS NULL AND replacement_intent_id IS NULL)
        OR
        (status = 'RESOLVED' AND resolution_id IS NOT NULL AND resolution IS NOT NULL
         AND resolved_at_ms IS NOT NULL
         AND ((resolution = 'ACCEPT_REMOTE' AND replacement_intent_id IS NULL)
              OR (resolution = 'RETRY_LOCAL_AGAINST_CURRENT_BASE'
                  AND replacement_intent_id IS NOT NULL)))
    )
) STRICT;

CREATE INDEX sync_conflicts_library_status_page_idx
ON sync_conflicts(library_id, status, detected_at_ms, conflict_id);

CREATE INDEX sync_conflicts_node_idx
ON sync_conflicts(library_id, node_id, status, detected_at_ms, conflict_id);
