-- Durable client-mutation identity, canonical fingerprint, and terminal
-- result state. The row is scoped to the authenticated owner, registered
-- device, library, and client mutation ID. It contains logical metadata only:
-- no object key, replica locator, staging handle, filesystem path, or bytes.

CREATE TABLE device_mutation_operations (
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    library_id UUID NOT NULL,
    client_mutation_id UUID NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    fingerprint BYTEA NOT NULL,
    kind TEXT NOT NULL,
    base_epoch BIGINT NOT NULL,
    base_sequence BIGINT NOT NULL,
    outcome TEXT NOT NULL,
    resource_id UUID,
    result_parent_node_id UUID,
    result_kind TEXT,
    result_name TEXT,
    result_state TEXT,
    result_current_version_id UUID,
    result_revision NUMERIC,
    result_trashed_at TIMESTAMPTZ(6),
    result_created_at TIMESTAMPTZ(6),
    result_updated_at TIMESTAMPTZ(6),
    journal_event_id UUID,
    journal_sequence BIGINT,
    conflict_reason TEXT,
    conflict_expected_revision NUMERIC,
    conflict_current_revision NUMERIC,
    conflict_current_state TEXT,
    conflict_current_parent_id UUID,
    conflict_current_name TEXT,
    server_epoch BIGINT NOT NULL,
    server_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    completed_at TIMESTAMPTZ(6),
    CONSTRAINT device_mutation_operations_device_owner_fk
        FOREIGN KEY (device_id, owner_user_id)
        REFERENCES devices (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT device_mutation_operations_library_owner_fk
        FOREIGN KEY (library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT device_mutation_operations_fingerprint_version_value
        CHECK (fingerprint_version = 1),
    CONSTRAINT device_mutation_operations_fingerprint_length
        CHECK (octet_length(fingerprint) = 32),
    CONSTRAINT device_mutation_operations_kind_value
        CHECK (
            kind IN (
                'CREATE_DIRECTORY',
                'RENAME_NODE',
                'MOVE_NODE',
                'TRASH_NODE',
                'RESTORE_NODE'
            )
        ),
    CONSTRAINT device_mutation_operations_base_epoch_positive
        CHECK (base_epoch > 0),
    CONSTRAINT device_mutation_operations_base_sequence_nonnegative
        CHECK (base_sequence >= 0),
    CONSTRAINT device_mutation_operations_outcome_value
        CHECK (outcome IN ('IN_PROGRESS', 'APPLIED', 'CONFLICT')),
    CONSTRAINT device_mutation_operations_result_kind_value
        CHECK (result_kind IS NULL OR result_kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT device_mutation_operations_result_state_value
        CHECK (result_state IS NULL OR result_state IN ('ACTIVE', 'TRASHED')),
    CONSTRAINT device_mutation_operations_conflict_reason_value
        CHECK (
            conflict_reason IS NULL
            OR conflict_reason IN (
                'REVISION_MISMATCH',
                'NODE_STATE_CHANGED',
                'PARENT_CHANGED',
                'NAME_OCCUPIED',
                'DESTINATION_CHANGED',
                'RESOURCE_PURGED'
            )
        ),
    CONSTRAINT device_mutation_operations_conflict_state_value
        CHECK (
            conflict_current_state IS NULL
            OR conflict_current_state IN ('ACTIVE', 'TRASHED', 'PURGING')
        ),
    CONSTRAINT device_mutation_operations_revision_u64
        CHECK (
            (result_revision IS NULL OR (
                result_revision >= 0
                AND result_revision = trunc(result_revision)
                AND result_revision <= 18446744073709551615::NUMERIC
            ))
            AND (conflict_expected_revision IS NULL OR (
                conflict_expected_revision >= 0
                AND conflict_expected_revision = trunc(conflict_expected_revision)
                AND conflict_expected_revision <= 18446744073709551615::NUMERIC
            ))
            AND (conflict_current_revision IS NULL OR (
                conflict_current_revision >= 0
                AND conflict_current_revision = trunc(conflict_current_revision)
                AND conflict_current_revision <= 18446744073709551615::NUMERIC
            ))
        ),
    CONSTRAINT device_mutation_operations_server_epoch_positive
        CHECK (server_epoch > 0),
    CONSTRAINT device_mutation_operations_server_sequence_nonnegative
        CHECK (server_sequence >= 0),
    CONSTRAINT device_mutation_operations_terminal_shape
        CHECK (
            (outcome = 'IN_PROGRESS' AND completed_at IS NULL)
            OR (
                outcome = 'APPLIED'
                AND completed_at IS NOT NULL
                AND resource_id IS NOT NULL
                AND result_kind IS NOT NULL
                AND result_name IS NOT NULL
                AND result_state IS NOT NULL
                AND result_revision IS NOT NULL
                AND result_created_at IS NOT NULL
                AND result_updated_at IS NOT NULL
                AND journal_event_id IS NOT NULL
                AND journal_sequence IS NOT NULL
                AND journal_sequence > 0
                AND conflict_reason IS NULL
            )
            OR (
                outcome = 'CONFLICT'
                AND completed_at IS NOT NULL
                AND resource_id IS NOT NULL
                AND conflict_reason IS NOT NULL
                AND journal_event_id IS NULL
                AND journal_sequence IS NULL
            )
        ),
    CONSTRAINT device_mutation_operations_result_name_length
        CHECK (result_name IS NULL OR octet_length(result_name) BETWEEN 1 AND 1024),
    CONSTRAINT device_mutation_operations_conflict_name_length
        CHECK (conflict_current_name IS NULL OR octet_length(conflict_current_name) BETWEEN 1 AND 1024),
    PRIMARY KEY (owner_user_id, device_id, library_id, client_mutation_id)
);

CREATE INDEX device_mutation_operations_scope_created_idx
    ON device_mutation_operations
        (owner_user_id, device_id, library_id, created_at DESC);
