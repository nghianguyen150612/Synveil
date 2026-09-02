-- Durable backup domain and immutable snapshot manifest foundation.
--
-- This migration stores only the backup domain: one owner-scoped `backup_sets`
-- config carrying retention metadata, an immutable logical `backup_snapshots`
-- capture lifecycle, and immutable `backup_snapshot_nodes` manifest rows. The
-- snapshot captures the current logical namespace cut at commit time. No object
-- identity, replica locator, storage key, staging handle, filesystem path, or
-- file bytes is persisted here.
--
-- Once a snapshot reaches `COMPLETED`, its manifest rows are never updated or
-- deleted by the application. Explicit expiry execution changes only snapshot
-- lifecycle metadata; explicit prune execution may later release its exact
-- retention pins while the snapshot and manifest remain as audit history.
-- Renames, moves, Trash/purge, or content replacement on the live library do
-- not mutate historical backup contents.

CREATE TABLE backup_sets (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    name TEXT NOT NULL,
    source_library_id UUID NOT NULL,
    source TEXT NOT NULL,
    retention_days INTEGER,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    revision NUMERIC NOT NULL,
    CONSTRAINT backup_sets_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_sets_library_owner_fk
        FOREIGN KEY (source_library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_sets_name_length
        CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT backup_sets_source_value CHECK (source IN ('LIBRARY')),
    CONSTRAINT backup_sets_retention_shape
        CHECK (retention_days IS NULL OR retention_days > 0),
    CONSTRAINT backup_sets_state_value CHECK (state IN ('CREATED', 'ACTIVE', 'DISABLED')),
    CONSTRAINT backup_sets_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT backup_sets_owner_name_unique UNIQUE (owner_user_id, name)
);

CREATE INDEX backup_sets_owner_scope_idx
    ON backup_sets (owner_user_id, state, created_at DESC);

CREATE TABLE backup_snapshots (
    id UUID PRIMARY KEY,
    backup_set_id UUID NOT NULL,
    owner_user_id UUID NOT NULL,
    source_library_id UUID NOT NULL,
    operation_id TEXT NOT NULL,
    snapshot_epoch BIGINT NOT NULL,
    snapshot_resume_sequence BIGINT NOT NULL,
    manifest_item_count BIGINT NOT NULL DEFAULT 0,
    terminal_node_id UUID,
    content_reference_count BIGINT NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    committed_at TIMESTAMPTZ(6),
    expired_at TIMESTAMPTZ(6),
    CONSTRAINT backup_snapshots_set_fk
        FOREIGN KEY (backup_set_id) REFERENCES backup_sets (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshots_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshots_library_owner_fk
        FOREIGN KEY (source_library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshots_operation_key_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_snapshots_owner_operation_unique
        UNIQUE (owner_user_id, backup_set_id, operation_id),
    CONSTRAINT backup_snapshots_epoch_positive CHECK (snapshot_epoch > 0),
    CONSTRAINT backup_snapshots_resume_nonnegative CHECK (snapshot_resume_sequence >= 0),
    CONSTRAINT backup_snapshots_manifest_count_nonnegative CHECK (manifest_item_count >= 0),
    CONSTRAINT backup_snapshots_content_count_nonnegative CHECK (content_reference_count >= 0),
    CONSTRAINT backup_snapshots_terminal_shape
        CHECK (
            (manifest_item_count = 0 AND terminal_node_id IS NULL)
            OR (manifest_item_count > 0 AND terminal_node_id IS NOT NULL)
        ),
    CONSTRAINT backup_snapshots_state_value
        CHECK (state IN ('BUILDING', 'COMPLETED', 'FAILED', 'EXPIRED')),
    CONSTRAINT backup_snapshots_commit_shape
        CHECK (
            (state = 'COMPLETED' AND committed_at IS NOT NULL)
            OR (state NOT IN ('COMPLETED') AND committed_at IS NULL)
        ),
    CONSTRAINT backup_snapshots_expiry_shape
        CHECK (
            (state = 'EXPIRED' AND expired_at IS NOT NULL)
            OR (state <> 'EXPIRED' AND expired_at IS NULL)
        )
);

-- At most one unfinished build may exist for a backup set. Expiry/retention is
-- applied to a terminal snapshot only after completion.
CREATE UNIQUE INDEX backup_snapshots_one_building_per_set
    ON backup_snapshots (backup_set_id)
    WHERE state = 'BUILDING';

CREATE INDEX backup_snapshots_owner_scope_idx
    ON backup_snapshots (owner_user_id, backup_set_id, state, committed_at DESC);

CREATE INDEX backup_snapshots_retention_idx
    ON backup_snapshots (state, source_library_id, committed_at, id);

CREATE TABLE backup_snapshot_nodes (
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
    node_created_at TIMESTAMPTZ(6) NOT NULL,
    node_updated_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_snapshot_nodes_snapshot_fk
        FOREIGN KEY (snapshot_id) REFERENCES backup_snapshots (id) ON DELETE CASCADE,
    CONSTRAINT backup_snapshot_nodes_kind_value
        CHECK (kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT backup_snapshot_nodes_public_state_value
        CHECK (state IN ('ACTIVE', 'TRASHED')),
    CONSTRAINT backup_snapshot_nodes_name_length
        CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT backup_snapshot_nodes_parent_shape
        CHECK (
            (parent_node_id IS NULL AND kind = 'DIRECTORY' AND state = 'ACTIVE')
            OR parent_node_id IS NOT NULL
        ),
    CONSTRAINT backup_snapshot_nodes_not_self_parent
        CHECK (parent_node_id IS NULL OR parent_node_id <> node_id),
    CONSTRAINT backup_snapshot_nodes_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT backup_snapshot_nodes_content_shape
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
    CONSTRAINT backup_snapshot_nodes_content_length_u64
        CHECK (
            content_length IS NULL
            OR (
                content_length >= 0
                AND content_length = trunc(content_length)
                AND content_length <= 18446744073709551615::NUMERIC
            )
        ),
    CONSTRAINT backup_snapshot_nodes_sha256_length
        CHECK (content_sha256 IS NULL OR octet_length(content_sha256) = 32),
    PRIMARY KEY (snapshot_id, node_id)
);

-- The primary key is the immutable Node-ID keyset index used by page reads.
-- No foreign key points back to mutable Node/FileVersion/Object rows, so a
-- later rename, content replacement, Trash transition, or purge cannot alter
-- or block the captured manifest. The manifest is immutable once its snapshot
-- leaves the BUILDING state; the application must reject mutation attempts.

CREATE INDEX backup_snapshot_nodes_content_idx
    ON backup_snapshot_nodes (snapshot_id, current_version_id)
    WHERE current_version_id IS NOT NULL;

-- Enforce immutability at the database boundary as well as in the service
-- contract. Manifest rows are only ever inserted during capture and never
-- modified afterward. Future retention/pruning may DELETE entire snapshots
-- (cascade to manifest); this trigger blocks in-place mutation, not deletion.
CREATE FUNCTION synveil_reject_backup_snapshot_node_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup_snapshot_nodes is append-only';
END;
$$;

CREATE TRIGGER backup_snapshot_nodes_append_only
    BEFORE UPDATE ON backup_snapshot_nodes
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_snapshot_node_mutation();
