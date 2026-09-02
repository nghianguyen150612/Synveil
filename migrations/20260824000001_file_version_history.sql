-- Bounded newest-first immutable file-version history pagination.
--
-- FileVersion IDs are globally unique, so node_id is the leading scope key;
-- committed_at and id are the stable descending keyset ordering keys.

CREATE INDEX file_versions_node_history_page_idx
    ON file_versions (node_id, committed_at DESC, id DESC);
