-- Metadata-only purge execution and authoritative object-reference handoff.
--
-- FileVersion history is permanently removed with its purged Node. The
-- canonical Object and ObjectReplica rows remain until a later, independently
-- reviewed GC phase. The purge operation row is intentionally not linked to a
-- Node: it is the minimal replay identity that survives the Node deletion.

ALTER TABLE file_versions
    DROP CONSTRAINT file_versions_parent_fk,
    ADD CONSTRAINT file_versions_parent_fk
    FOREIGN KEY (parent_version_id, library_id)
    REFERENCES file_versions (id, library_id)
    ON DELETE NO ACTION
    DEFERRABLE INITIALLY DEFERRED;

-- The purge survivor test is keyed by canonical Object identity and then
-- excludes the node being deleted. Keep that authoritative relation query
-- set-based even when a library has a large version history.
CREATE INDEX file_versions_object_reference_idx
    ON file_versions (object_id, object_dedup_domain_id, node_id);

-- A CREATE_FILE upload session retains its target parent through an explicit
-- ON DELETE RESTRICT FK. Keep purge candidate and execution rechecks bounded;
-- staged/session state is not a committed FileVersion reference.
CREATE INDEX upload_sessions_target_parent_idx
    ON upload_sessions (library_id, target_parent_node_id)
    WHERE target_parent_node_id IS NOT NULL;

CREATE TABLE metadata_purge_operations (
    node_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    library_id UUID NOT NULL,
    purge_revision NUMERIC NOT NULL,
    completed_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT metadata_purge_operations_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT metadata_purge_operations_library_fk
        FOREIGN KEY (library_id) REFERENCES libraries (id) ON DELETE RESTRICT,
    CONSTRAINT metadata_purge_operations_revision_u64 CHECK (
        purge_revision >= 0
        AND purge_revision = trunc(purge_revision)
        AND purge_revision <= 18446744073709551615::NUMERIC
    )
);

CREATE TABLE object_gc_candidates (
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    unreferenced_at TIMESTAMPTZ(6) NOT NULL,
    source TEXT NOT NULL,
    CONSTRAINT object_gc_candidates_object_fk
        FOREIGN KEY (object_id, object_dedup_domain_id)
        REFERENCES objects (id, dedup_domain_id) ON DELETE RESTRICT,
    CONSTRAINT object_gc_candidates_source_value
        CHECK (source IN ('METADATA_PURGE')),
    PRIMARY KEY (object_id, object_dedup_domain_id)
);

CREATE INDEX object_gc_candidates_unreferenced_idx
    ON object_gc_candidates (unreferenced_at ASC, object_id ASC,
                             object_dedup_domain_id ASC);
