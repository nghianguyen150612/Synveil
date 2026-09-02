-- Durable logical snapshot/rebaseline bootstrap manifests.
--
-- A bootstrap start holds the existing per-library namespace guard and
-- materializes the current logical Node projection in the same transaction
-- that captures journal_epoch/sync_head. Later HTTP requests page only these
-- immutable rows. No object identity, replica locator, storage key, staging
-- handle, filesystem path, credential, or file byte is persisted here.

ALTER TABLE device_sync_checkpoints
    ADD COLUMN rebaseline_generation BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT device_sync_checkpoints_rebaseline_generation_nonnegative
        CHECK (rebaseline_generation >= 0);

CREATE TABLE sync_bootstraps (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    library_id UUID NOT NULL,
    generation BIGINT NOT NULL,
    snapshot_epoch BIGINT NOT NULL,
    snapshot_resume_sequence BIGINT NOT NULL,
    manifest_item_count BIGINT NOT NULL DEFAULT 0,
    terminal_node_id UUID,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    expires_at TIMESTAMPTZ(6) NOT NULL,
    completed_at TIMESTAMPTZ(6),
    CONSTRAINT sync_bootstraps_device_owner_fk
        FOREIGN KEY (device_id, owner_user_id)
        REFERENCES devices (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT sync_bootstraps_library_owner_fk
        FOREIGN KEY (library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT sync_bootstraps_generation_positive
        CHECK (generation > 0),
    CONSTRAINT sync_bootstraps_epoch_positive
        CHECK (snapshot_epoch > 0),
    CONSTRAINT sync_bootstraps_resume_nonnegative
        CHECK (snapshot_resume_sequence >= 0),
    CONSTRAINT sync_bootstraps_manifest_count_nonnegative
        CHECK (manifest_item_count >= 0),
    CONSTRAINT sync_bootstraps_terminal_shape
        CHECK (
            (manifest_item_count = 0 AND terminal_node_id IS NULL)
            OR (manifest_item_count > 0 AND terminal_node_id IS NOT NULL)
        ),
    CONSTRAINT sync_bootstraps_state_value
        CHECK (state IN ('OPEN', 'COMPLETED', 'ABORTED', 'EXPIRED')),
    CONSTRAINT sync_bootstraps_expiry_after_creation
        CHECK (expires_at > created_at),
    CONSTRAINT sync_bootstraps_completion_shape
        CHECK (
            (state = 'COMPLETED' AND completed_at IS NOT NULL)
            OR (state <> 'COMPLETED' AND completed_at IS NULL)
        ),
    CONSTRAINT sync_bootstraps_scope_generation_unique
        UNIQUE (device_id, library_id, generation)
);

-- At most one unfinished materialized cut may exist for a device/library.
-- Expiry is transitioned under the start lock before a replacement is made.
CREATE UNIQUE INDEX sync_bootstraps_one_open_per_scope
    ON sync_bootstraps (device_id, library_id)
    WHERE state = 'OPEN';

CREATE INDEX sync_bootstraps_owner_scope_idx
    ON sync_bootstraps (owner_user_id, device_id, library_id, created_at DESC);

CREATE INDEX sync_bootstraps_cleanup_idx
    ON sync_bootstraps (state, expires_at, completed_at, id);

CREATE TABLE sync_bootstrap_nodes (
    bootstrap_id UUID NOT NULL,
    node_id UUID NOT NULL,
    parent_node_id UUID,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    revision NUMERIC NOT NULL,
    current_version_id UUID,
    content_length NUMERIC,
    content_sha256 BYTEA,
    CONSTRAINT sync_bootstrap_nodes_bootstrap_fk
        FOREIGN KEY (bootstrap_id) REFERENCES sync_bootstraps (id)
        ON DELETE CASCADE,
    CONSTRAINT sync_bootstrap_nodes_kind_value
        CHECK (kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT sync_bootstrap_nodes_public_state_value
        CHECK (state IN ('ACTIVE', 'TRASHED')),
    CONSTRAINT sync_bootstrap_nodes_name_length
        CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT sync_bootstrap_nodes_parent_shape
        CHECK (
            (parent_node_id IS NULL AND kind = 'DIRECTORY' AND state = 'ACTIVE')
            OR parent_node_id IS NOT NULL
        ),
    CONSTRAINT sync_bootstrap_nodes_not_self_parent
        CHECK (parent_node_id IS NULL OR parent_node_id <> node_id),
    CONSTRAINT sync_bootstrap_nodes_revision_u64
        CHECK (
            revision >= 0
            AND revision = trunc(revision)
            AND revision <= 18446744073709551615::NUMERIC
        ),
    CONSTRAINT sync_bootstrap_nodes_content_shape
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
    CONSTRAINT sync_bootstrap_nodes_content_length_u64
        CHECK (
            content_length IS NULL
            OR (
                content_length >= 0
                AND content_length = trunc(content_length)
                AND content_length <= 18446744073709551615::NUMERIC
            )
        ),
    CONSTRAINT sync_bootstrap_nodes_sha256_length
        CHECK (content_sha256 IS NULL OR octet_length(content_sha256) = 32),
    PRIMARY KEY (bootstrap_id, node_id)
);

-- The primary key is the immutable Node-ID keyset index used by page reads.
-- No foreign key points back to mutable Node/FileVersion/Object rows, so a
-- later rename, content replacement, Trash transition, or purge cannot alter
-- or block the captured manifest.
