-- Bounded logical metadata listing support.
--
-- This migration intentionally does not add a sibling-name uniqueness rule or
-- a filesystem comparison key. That policy remains an explicit open decision;
-- the current schema therefore preserves duplicate logical sibling names.

CREATE INDEX nodes_active_children_page_idx
    ON nodes (library_id, parent_node_id, id)
    WHERE state = 'ACTIVE';
