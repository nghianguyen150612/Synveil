-- Persisted resumable upload intent, opaque staging recovery state, and the
-- first verified object-replica record. Byte transport remains outside HTTP
-- until the upload application service has its own release gate.

CREATE TABLE object_replicas (
    id UUID PRIMARY KEY,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    backend_kind TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    stored_length NUMERIC NOT NULL,
    stored_sha256 BYTEA NOT NULL,
    backend_version TEXT,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    verified_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT object_replicas_object_fk
        FOREIGN KEY (object_id, object_dedup_domain_id)
        REFERENCES objects (id, dedup_domain_id)
        ON DELETE RESTRICT,
    CONSTRAINT object_replicas_backend_kind_value CHECK (
        backend_kind IN ('LOCAL_FILESYSTEM', 'OBJECT_STORE', 'UNKNOWN')
    ),
    CONSTRAINT object_replicas_storage_key_length
        CHECK (octet_length(storage_key) BETWEEN 1 AND 1024),
    CONSTRAINT object_replicas_stored_length_u64 CHECK (
        stored_length >= 0
        AND stored_length = trunc(stored_length)
        AND stored_length <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT object_replicas_stored_sha256_length
        CHECK (octet_length(stored_sha256) = 32),
    CONSTRAINT object_replicas_state_value CHECK (state IN ('COPYING', 'VERIFIED', 'MISSING', 'CORRUPT', 'DELETING')),
    CONSTRAINT object_replicas_identity_unique
        UNIQUE (object_id, object_dedup_domain_id, backend_kind, storage_key)
);

CREATE TABLE upload_sessions (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    library_id UUID NOT NULL,
    operation TEXT NOT NULL,
    target_node_id UUID NOT NULL,
    target_parent_node_id UUID,
    target_name TEXT,
    expected_node_revision NUMERIC,
    expected_length NUMERIC NOT NULL,
    expected_sha256 BYTEA,
    object_id UUID NOT NULL,
    object_replica_id UUID NOT NULL,
    object_key TEXT NOT NULL,
    staging_handle TEXT NOT NULL,
    bytes_received NUMERIC NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'OPEN',
    lease_generation NUMERIC NOT NULL DEFAULT 0,
    lease_expires_at TIMESTAMPTZ(6),
    cancel_requested_at TIMESTAMPTZ(6),
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    expires_at TIMESTAMPTZ(6) NOT NULL,
    last_error_code TEXT,
    terminal_failure_code TEXT,
    durability_backend_kind TEXT,
    durability_backend_version TEXT,
    durability_length NUMERIC,
    durability_sha256 BYTEA,
    durable_at TIMESTAMPTZ(6),
    completed_node_id UUID,
    completed_file_version_id UUID,
    completed_object_id UUID,
    completed_object_replica_id UUID,
    completed_node_revision NUMERIC,
    completed_at TIMESTAMPTZ(6),
    CONSTRAINT upload_sessions_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT upload_sessions_library_fk
        FOREIGN KEY (library_id) REFERENCES libraries (id) ON DELETE RESTRICT,
    CONSTRAINT upload_sessions_parent_fk
        FOREIGN KEY (target_parent_node_id, library_id)
        REFERENCES nodes (id, library_id)
        ON DELETE RESTRICT,
    CONSTRAINT upload_sessions_operation_value
        CHECK (operation IN ('CREATE_FILE', 'REPLACE_CONTENT')),
    CONSTRAINT upload_sessions_target_shape CHECK (
        (
            operation = 'CREATE_FILE'
            AND target_parent_node_id IS NOT NULL
            AND target_name IS NOT NULL
            AND expected_node_revision IS NULL
        )
        OR (
            operation = 'REPLACE_CONTENT'
            AND target_parent_node_id IS NULL
            AND target_name IS NULL
            AND expected_node_revision IS NOT NULL
        )
    ),
    CONSTRAINT upload_sessions_target_name_length
        CHECK (target_name IS NULL OR octet_length(target_name) BETWEEN 1 AND 1024),
    CONSTRAINT upload_sessions_expected_length_u64 CHECK (
        expected_length >= 0
        AND expected_length = trunc(expected_length)
        AND expected_length <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT upload_sessions_expected_sha256_shape
        CHECK (expected_sha256 IS NULL OR octet_length(expected_sha256) = 32),
    CONSTRAINT upload_sessions_object_key_length
        CHECK (octet_length(object_key) BETWEEN 1 AND 1024),
    CONSTRAINT upload_sessions_staging_handle_length
        CHECK (octet_length(staging_handle) BETWEEN 1 AND 256),
    CONSTRAINT upload_sessions_bytes_received_u64 CHECK (
        bytes_received >= 0
        AND bytes_received = trunc(bytes_received)
        AND bytes_received <= expected_length
        AND bytes_received <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT upload_sessions_state_value CHECK (
        state IN ('OPEN', 'VERIFYING', 'COMMITTING', 'COMMITTED', 'FAILED', 'EXPIRED', 'ABORTED')
    ),
    CONSTRAINT upload_sessions_lease_generation_u64 CHECK (
        lease_generation >= 0
        AND lease_generation = trunc(lease_generation)
        AND lease_generation <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT upload_sessions_expiry_after_creation CHECK (expires_at > created_at),
    CONSTRAINT upload_sessions_durability_shape CHECK (
        (
            durability_length IS NULL
            AND durability_sha256 IS NULL
            AND durability_backend_kind IS NULL
            AND durability_backend_version IS NULL
            AND durable_at IS NULL
        )
        OR (
            durability_length IS NOT NULL
            AND durability_sha256 IS NOT NULL
            AND durability_backend_kind IS NOT NULL
            AND durable_at IS NOT NULL
        )
    ),
    CONSTRAINT upload_sessions_durability_length_u64 CHECK (
        durability_length IS NULL
        OR (
            durability_length >= 0
            AND durability_length = trunc(durability_length)
            AND durability_length <= 18446744073709551615::NUMERIC
        )
    ),
    CONSTRAINT upload_sessions_durability_sha256_shape
        CHECK (durability_sha256 IS NULL OR octet_length(durability_sha256) = 32),
    CONSTRAINT upload_sessions_durability_backend_kind_value CHECK (
        durability_backend_kind IS NULL
        OR durability_backend_kind IN ('LOCAL_FILESYSTEM', 'OBJECT_STORE', 'UNKNOWN')
    ),
    CONSTRAINT upload_sessions_completed_shape CHECK (
        (
            completed_node_id IS NULL
            AND completed_file_version_id IS NULL
            AND completed_object_id IS NULL
            AND completed_object_replica_id IS NULL
            AND completed_node_revision IS NULL
            AND completed_at IS NULL
        )
        OR (
            completed_node_id IS NOT NULL
            AND completed_file_version_id IS NOT NULL
            AND completed_object_id IS NOT NULL
            AND completed_object_replica_id IS NOT NULL
            AND completed_node_revision IS NOT NULL
            AND completed_at IS NOT NULL
        )
    ),
    CONSTRAINT upload_sessions_completed_revision_u64 CHECK (
        completed_node_revision IS NULL
        OR (
            completed_node_revision >= 0
            AND completed_node_revision = trunc(completed_node_revision)
            AND completed_node_revision <= 18446744073709551615::NUMERIC
        )
    )
);

CREATE UNIQUE INDEX upload_sessions_object_key_unique
    ON upload_sessions (object_key);

CREATE UNIQUE INDEX upload_sessions_staging_handle_unique
    ON upload_sessions (staging_handle);

CREATE INDEX upload_sessions_owner_state_idx
    ON upload_sessions (owner_user_id, state, updated_at);

CREATE INDEX upload_sessions_expiry_idx
    ON upload_sessions (state, expires_at)
    WHERE state = 'OPEN';

CREATE INDEX upload_sessions_lease_idx
    ON upload_sessions (state, lease_expires_at)
    WHERE state IN ('VERIFYING', 'COMMITTING');
