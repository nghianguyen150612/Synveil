-- Minimal persisted identity for retry-safe historical file-version restore.
--
-- A row is inserted and completed in the same transaction as the new
-- FileVersion and Node-head mutation. Failed restore attempts therefore leave
-- no replay record, while a committed response can be recovered after a
-- transport failure without appending a second version.

CREATE TABLE file_version_restore_operations (
    owner_user_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    library_id UUID NOT NULL,
    node_id UUID NOT NULL,
    source_version_id UUID NOT NULL,
    result_version_id UUID,
    result_node_revision NUMERIC,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT file_version_restore_operations_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT file_version_restore_operations_node_fk
        FOREIGN KEY (node_id, library_id)
        REFERENCES nodes (id, library_id) ON DELETE RESTRICT,
    CONSTRAINT file_version_restore_operations_result_fk
        FOREIGN KEY (result_version_id, library_id)
        REFERENCES file_versions (id, library_id) ON DELETE RESTRICT,
    CONSTRAINT file_version_restore_operations_key_length
        CHECK (octet_length(idempotency_key) BETWEEN 8 AND 256),
    CONSTRAINT file_version_restore_operations_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT file_version_restore_operations_result_shape CHECK (
        (result_version_id IS NULL AND result_node_revision IS NULL)
        OR (result_version_id IS NOT NULL AND result_node_revision IS NOT NULL)
    ),
    CONSTRAINT file_version_restore_operations_revision_u64 CHECK (
        result_node_revision IS NULL
        OR (
            result_node_revision >= 0
            AND result_node_revision = trunc(result_node_revision)
            AND result_node_revision <= 18446744073709551615::NUMERIC
        )
    ),
    PRIMARY KEY (owner_user_id, idempotency_key)
);

CREATE INDEX file_version_restore_operations_result_idx
    ON file_version_restore_operations (result_version_id)
    WHERE result_version_id IS NOT NULL;
