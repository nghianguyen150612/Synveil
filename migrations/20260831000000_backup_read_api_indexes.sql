-- Indexes for the authenticated, read-only backup inspection API.
--
-- These indexes are deliberately limited to owner/set/tree scope and the
-- public deterministic ordering keys. They do not add a lifecycle operation,
-- expose physical storage identity, or change any prior migration.

CREATE INDEX backup_sets_owner_id_idx
    ON backup_sets (owner_user_id, id);

CREATE INDEX backup_snapshots_owner_set_chronological_idx
    ON backup_snapshots (
        owner_user_id,
        backup_set_id,
        (COALESCE(committed_at, created_at)) DESC,
        id DESC
    );

CREATE INDEX backup_snapshot_nodes_snapshot_parent_node_idx
    ON backup_snapshot_nodes (snapshot_id, parent_node_id, node_id);

CREATE INDEX backup_maintenance_runs_owner_set_created_idx
    ON backup_maintenance_runs (owner_user_id, backup_set_id, created_at DESC, id DESC);
