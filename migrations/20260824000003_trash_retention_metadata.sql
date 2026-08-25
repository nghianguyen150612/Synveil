-- Canonical logical-Trash timestamp and bounded purge-candidate scan support.
--
-- `trashed_at` is the only persisted deletion timestamp. Restore clears it;
-- purge-in-progress preserves it for reconciliation. The restore deadline is
-- derived from this value and the configured retention policy at application
-- time, so it is not duplicated in the schema.

ALTER TABLE nodes
    ADD COLUMN trashed_at TIMESTAMPTZ(6);

-- Preserve the meaning of rows created by the earlier logical-trash slice.
-- Their updated_at is the only authoritative server-observed deletion instant
-- available before this migration.
UPDATE nodes
SET trashed_at = updated_at
WHERE state IN ('TRASHED', 'PURGING')
  AND trashed_at IS NULL;

ALTER TABLE nodes
    ADD CONSTRAINT nodes_trash_timestamp_shape CHECK (
        (state = 'ACTIVE' AND trashed_at IS NULL)
        OR (state IN ('TRASHED', 'PURGING') AND trashed_at IS NOT NULL)
    );

CREATE INDEX nodes_purge_candidates_idx
    ON nodes (trashed_at ASC, id ASC)
    WHERE state = 'TRASHED' AND parent_node_id IS NOT NULL;
