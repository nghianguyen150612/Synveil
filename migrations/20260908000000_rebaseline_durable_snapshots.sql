-- Prompt 82: durable, library-scoped rebaseline snapshot artifacts.
--
-- A creator holds the existing per-library namespace guard and materializes
-- one validated Prompt 81 logical snapshot cut in a REPEATABLE READ
-- transaction. The header, its journal boundary, and every copied logical
-- entry therefore publish atomically at commit. Later page reads use only
-- rebaseline_snapshot_entries; they do not join or reconstruct live Nodes.
--
-- This is transfer metadata only. It deliberately contains no Object ID,
-- ObjectReplica ID, storage key, backend locator, filesystem path, staging
-- handle, credential, or file bytes. A library deletion may cascade its
-- short-lived transfer artifacts, but snapshot rows never cascade into live
-- Library, Node, FileVersion, Object, journal, or checkpoint data.

CREATE TABLE rebaseline_snapshots (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    library_id UUID NOT NULL,
    journal_epoch BIGINT NOT NULL,
    snapshot_resume_sequence BIGINT NOT NULL,
    entry_count BIGINT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    expires_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT rebaseline_snapshots_library_owner_fk
        FOREIGN KEY (library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE CASCADE,
    CONSTRAINT rebaseline_snapshots_epoch_positive
        CHECK (journal_epoch > 0),
    CONSTRAINT rebaseline_snapshots_resume_nonnegative
        CHECK (snapshot_resume_sequence >= 0),
    CONSTRAINT rebaseline_snapshots_entry_count_nonnegative
        CHECK (entry_count >= 0),
    CONSTRAINT rebaseline_snapshots_expiry_after_creation
        CHECK (expires_at > created_at)
);

CREATE TABLE rebaseline_snapshot_entries (
    snapshot_id UUID NOT NULL,
    node_id UUID NOT NULL,
    parent_node_id UUID,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    revision NUMERIC NOT NULL,
    current_version_id UUID,
    content_length NUMERIC,
    content_sha256 BYTEA,
    CONSTRAINT rebaseline_snapshot_entries_snapshot_fk
        FOREIGN KEY (snapshot_id) REFERENCES rebaseline_snapshots (id)
        ON DELETE CASCADE,
    CONSTRAINT rebaseline_snapshot_entries_kind_value
        CHECK (kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT rebaseline_snapshot_entries_public_state_value
        CHECK (state IN ('ACTIVE', 'TRASHED')),
    CONSTRAINT rebaseline_snapshot_entries_name_length
        CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT rebaseline_snapshot_entries_parent_shape
        CHECK (
            (parent_node_id IS NULL AND kind = 'DIRECTORY' AND state = 'ACTIVE')
            OR parent_node_id IS NOT NULL
        ),
    CONSTRAINT rebaseline_snapshot_entries_not_self_parent
        CHECK (parent_node_id IS NULL OR parent_node_id <> node_id),
    CONSTRAINT rebaseline_snapshot_entries_revision_u64
        CHECK (
            revision >= 0
            AND revision = trunc(revision)
            AND revision <= 18446744073709551615::NUMERIC
        ),
    CONSTRAINT rebaseline_snapshot_entries_content_shape
        CHECK (
            (
                kind = 'FILE'
                AND current_version_id IS NOT NULL
                AND content_length IS NOT NULL
                AND content_sha256 IS NOT NULL
            )
            OR (
                current_version_id IS NULL
                AND content_length IS NULL
                AND content_sha256 IS NULL
            )
        ),
    CONSTRAINT rebaseline_snapshot_entries_content_length_u64
        CHECK (
            content_length IS NULL
            OR (
                content_length >= 0
                AND content_length = trunc(content_length)
                AND content_length <= 18446744073709551615::NUMERIC
            )
        ),
    CONSTRAINT rebaseline_snapshot_entries_sha256_length
        CHECK (content_sha256 IS NULL OR octet_length(content_sha256) = 32),
    PRIMARY KEY (snapshot_id, node_id)
);

-- The entry primary key is the exact immutable keyset index for
-- `WHERE snapshot_id = ? AND node_id > ? ORDER BY node_id LIMIT ?`; no OFFSET
-- and no additional speculative paging index are needed. Header lookup first
-- uses its opaque primary key and then checks the owner predicate, so a second
-- owner/index pair would be redundant.

CREATE FUNCTION synveil_reject_rebaseline_snapshot_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'rebaseline snapshot headers are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'rebaseline_snapshots_immutable';
END;
$$;

CREATE TRIGGER rebaseline_snapshots_no_update
    BEFORE UPDATE ON rebaseline_snapshots
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_rebaseline_snapshot_update();

CREATE FUNCTION synveil_reject_rebaseline_snapshot_entry_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'rebaseline snapshot entries are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'rebaseline_snapshot_entries_immutable';
END;
$$;

CREATE TRIGGER rebaseline_snapshot_entries_no_update
    BEFORE UPDATE ON rebaseline_snapshot_entries
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_rebaseline_snapshot_entry_update();

-- A transfer artifact has no explicit deletion API in this phase. Direct
-- deletion is rejected, while a nested referential-action delete remains
-- available when the owning live library is removed.
CREATE FUNCTION synveil_reject_rebaseline_snapshot_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF pg_trigger_depth() <= 1 THEN
        RAISE EXCEPTION 'rebaseline snapshots cannot be deleted directly'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'rebaseline_snapshots_delete_forbidden';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER rebaseline_snapshots_no_delete
    BEFORE DELETE ON rebaseline_snapshots
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_rebaseline_snapshot_delete();

CREATE FUNCTION synveil_reject_rebaseline_snapshot_entry_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF pg_trigger_depth() <= 1 THEN
        RAISE EXCEPTION 'rebaseline snapshot entries cannot be deleted directly'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'rebaseline_snapshot_entries_delete_forbidden';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER rebaseline_snapshot_entries_no_delete
    BEFORE DELETE ON rebaseline_snapshot_entries
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_rebaseline_snapshot_entry_delete();
