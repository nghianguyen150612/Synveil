-- Prompt 35: durable, inspectable optimistic-concurrency conflict evidence
-- and explicit manual-resolution idempotency. Both tables contain logical
-- metadata only. They deliberately contain no request JSON, object-store key,
-- replica locator, staging handle, backend credential, or filesystem path.

-- Prompt 31-34 are an uncommitted continuation baseline. Fail closed rather
-- than inventing incomplete original-intent evidence if this preview migration
-- is applied to a database that already accepted Prompt 34 conflicts.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM device_mutation_operations WHERE outcome = 'CONFLICT'
    ) THEN
        RAISE EXCEPTION
            'Prompt 35 requires a conflict-free Prompt 34 preview database';
    END IF;
END;
$$;

ALTER TABLE device_mutation_operations
    ADD COLUMN conflict_id UUID;

CREATE TABLE sync_conflicts (
    conflict_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    library_id UUID NOT NULL,
    original_client_mutation_id UUID NOT NULL,
    mutation_kind TEXT NOT NULL,
    resource_id UUID NOT NULL,
    conflict_reason TEXT NOT NULL,

    -- Closed, typed projection of the original Prompt 34 semantic intent.
    intent_node_id UUID,
    intent_parent_node_id UUID,
    intent_requested_parent_id UUID,
    intent_expected_revision NUMERIC,
    intent_expected_parent_revision NUMERIC,
    intent_requested_name TEXT,

    -- Immutable server observation captured when the intent was rejected.
    historical_resource_id UUID NOT NULL,
    historical_expected_revision NUMERIC,
    historical_server_revision NUMERIC,
    historical_server_state TEXT,
    historical_server_parent_id UUID,
    historical_server_name TEXT,
    historical_server_epoch BIGINT NOT NULL,
    historical_server_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,

    lifecycle TEXT NOT NULL DEFAULT 'OPEN',
    terminal_resolution_id UUID,
    terminal_action TEXT,
    terminal_journal_event_id UUID,
    terminal_journal_sequence BIGINT,
    terminal_at TIMESTAMPTZ(6),

    CONSTRAINT sync_conflicts_original_operation_fk
        FOREIGN KEY (
            owner_user_id, device_id, library_id, original_client_mutation_id
        )
        REFERENCES device_mutation_operations (
            owner_user_id, device_id, library_id, client_mutation_id
        )
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT sync_conflicts_original_operation_unique
        UNIQUE (
            owner_user_id, device_id, library_id, original_client_mutation_id
        ),
    CONSTRAINT sync_conflicts_operation_link_unique
        UNIQUE (
            owner_user_id, device_id, library_id,
            original_client_mutation_id, conflict_id
        ),
    CONSTRAINT sync_conflicts_scope_identity_unique
        UNIQUE (owner_user_id, device_id, library_id, conflict_id),
    CONSTRAINT sync_conflicts_kind_value
        CHECK (
            mutation_kind IN (
                'CREATE_DIRECTORY',
                'RENAME_NODE',
                'MOVE_NODE',
                'TRASH_NODE',
                'RESTORE_NODE'
            )
        ),
    CONSTRAINT sync_conflicts_reason_value
        CHECK (
            conflict_reason IN (
                'REVISION_MISMATCH',
                'NODE_STATE_CHANGED',
                'PARENT_CHANGED',
                'NAME_OCCUPIED',
                'DESTINATION_CHANGED',
                'RESOURCE_PURGED'
            )
        ),
    CONSTRAINT sync_conflicts_intent_shape
        CHECK (
            (
                mutation_kind = 'CREATE_DIRECTORY'
                AND intent_node_id IS NULL
                AND intent_parent_node_id IS NOT NULL
                AND intent_requested_parent_id IS NULL
                AND intent_expected_revision IS NULL
                AND intent_expected_parent_revision IS NOT NULL
                AND intent_requested_name IS NOT NULL
                AND resource_id = intent_parent_node_id
            )
            OR (
                mutation_kind = 'RENAME_NODE'
                AND intent_node_id IS NOT NULL
                AND intent_parent_node_id IS NULL
                AND intent_requested_parent_id IS NULL
                AND intent_expected_revision IS NOT NULL
                AND intent_expected_parent_revision IS NULL
                AND intent_requested_name IS NOT NULL
                AND resource_id = intent_node_id
            )
            OR (
                mutation_kind = 'MOVE_NODE'
                AND intent_node_id IS NOT NULL
                AND intent_parent_node_id IS NULL
                AND intent_requested_parent_id IS NOT NULL
                AND intent_expected_revision IS NOT NULL
                AND intent_expected_parent_revision IS NOT NULL
                AND intent_requested_name IS NULL
                AND resource_id = intent_node_id
            )
            OR (
                mutation_kind = 'TRASH_NODE'
                AND intent_node_id IS NOT NULL
                AND intent_parent_node_id IS NULL
                AND intent_requested_parent_id IS NULL
                AND intent_expected_revision IS NOT NULL
                AND intent_expected_parent_revision IS NULL
                AND intent_requested_name IS NULL
                AND resource_id = intent_node_id
            )
            OR (
                mutation_kind = 'RESTORE_NODE'
                AND intent_node_id IS NOT NULL
                AND intent_parent_node_id IS NOT NULL
                AND intent_requested_parent_id IS NULL
                AND intent_expected_revision IS NOT NULL
                AND intent_expected_parent_revision IS NOT NULL
                AND intent_requested_name IS NULL
                AND resource_id = intent_node_id
            )
        ),
    CONSTRAINT sync_conflicts_lifecycle_value
        CHECK (lifecycle IN ('OPEN', 'RESOLVED', 'DISMISSED')),
    CONSTRAINT sync_conflicts_terminal_action_value
        CHECK (
            terminal_action IS NULL
            OR terminal_action IN ('ACCEPT_SERVER', 'APPLY_CLIENT_INTENT')
        ),
    CONSTRAINT sync_conflicts_historical_state_value
        CHECK (
            historical_server_state IS NULL
            OR historical_server_state IN ('ACTIVE', 'TRASHED', 'PURGING')
        ),
    CONSTRAINT sync_conflicts_revision_u64
        CHECK (
            (intent_expected_revision IS NULL OR (
                intent_expected_revision >= 0
                AND intent_expected_revision = trunc(intent_expected_revision)
                AND intent_expected_revision <= 18446744073709551615::NUMERIC
            ))
            AND (intent_expected_parent_revision IS NULL OR (
                intent_expected_parent_revision >= 0
                AND intent_expected_parent_revision = trunc(intent_expected_parent_revision)
                AND intent_expected_parent_revision <= 18446744073709551615::NUMERIC
            ))
            AND (historical_expected_revision IS NULL OR (
                historical_expected_revision >= 0
                AND historical_expected_revision = trunc(historical_expected_revision)
                AND historical_expected_revision <= 18446744073709551615::NUMERIC
            ))
            AND (historical_server_revision IS NULL OR (
                historical_server_revision >= 0
                AND historical_server_revision = trunc(historical_server_revision)
                AND historical_server_revision <= 18446744073709551615::NUMERIC
            ))
        ),
    CONSTRAINT sync_conflicts_name_bounds
        CHECK (
            (intent_requested_name IS NULL
                OR octet_length(intent_requested_name) BETWEEN 1 AND 1024)
            AND (historical_server_name IS NULL
                OR octet_length(historical_server_name) BETWEEN 1 AND 1024)
        ),
    CONSTRAINT sync_conflicts_historical_clock
        CHECK (
            historical_server_epoch > 0
            AND historical_server_sequence >= 0
        ),
    CONSTRAINT sync_conflicts_terminal_shape
        CHECK (
            (
                lifecycle = 'OPEN'
                AND terminal_resolution_id IS NULL
                AND terminal_action IS NULL
                AND terminal_journal_event_id IS NULL
                AND terminal_journal_sequence IS NULL
                AND terminal_at IS NULL
            )
            OR (
                lifecycle = 'DISMISSED'
                AND terminal_resolution_id IS NOT NULL
                AND terminal_action = 'ACCEPT_SERVER'
                AND terminal_journal_event_id IS NULL
                AND terminal_journal_sequence IS NULL
                AND terminal_at IS NOT NULL
            )
            OR (
                lifecycle = 'RESOLVED'
                AND terminal_resolution_id IS NOT NULL
                AND terminal_action = 'APPLY_CLIENT_INTENT'
                AND terminal_journal_event_id IS NOT NULL
                AND terminal_journal_sequence > 0
                AND terminal_at IS NOT NULL
            )
        ),
    CONSTRAINT sync_conflicts_terminal_journal_fk
        FOREIGN KEY (terminal_journal_event_id)
        REFERENCES change_journal (entry_id) ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX sync_conflicts_open_page_idx
    ON sync_conflicts (
        owner_user_id, device_id, library_id,
        created_at DESC, conflict_id DESC
    )
    WHERE lifecycle = 'OPEN';

CREATE TABLE sync_conflict_resolutions (
    resolution_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    library_id UUID NOT NULL,
    conflict_id UUID NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    fingerprint BYTEA NOT NULL,
    action TEXT NOT NULL,
    expected_current_revision NUMERIC,
    expected_current_parent_revision NUMERIC,
    outcome TEXT NOT NULL,
    stale_reason TEXT,
    stale_resource_id UUID,
    stale_expected_revision NUMERIC,
    stale_server_revision NUMERIC,
    stale_server_state TEXT,
    stale_server_parent_id UUID,
    stale_server_name TEXT,
    stale_server_epoch BIGINT,
    stale_server_sequence BIGINT,
    journal_event_id UUID,
    journal_sequence BIGINT,
    created_at TIMESTAMPTZ(6) NOT NULL,
    completed_at TIMESTAMPTZ(6),
    CONSTRAINT sync_conflict_resolutions_conflict_fk
        FOREIGN KEY (owner_user_id, device_id, library_id, conflict_id)
        REFERENCES sync_conflicts (
            owner_user_id, device_id, library_id, conflict_id
        )
        ON DELETE RESTRICT,
    CONSTRAINT sync_conflict_resolutions_conflict_identity_unique
        UNIQUE (
            owner_user_id, device_id, library_id, conflict_id, resolution_id
        ),
    CONSTRAINT sync_conflict_resolutions_fingerprint_version_value
        CHECK (fingerprint_version = 1),
    CONSTRAINT sync_conflict_resolutions_fingerprint_length
        CHECK (octet_length(fingerprint) = 32),
    CONSTRAINT sync_conflict_resolutions_action_value
        CHECK (action IN ('ACCEPT_SERVER', 'APPLY_CLIENT_INTENT')),
    CONSTRAINT sync_conflict_resolutions_request_shape
        CHECK (
            (
                action = 'ACCEPT_SERVER'
                AND expected_current_revision IS NULL
                AND expected_current_parent_revision IS NULL
            )
            OR (
                action = 'APPLY_CLIENT_INTENT'
                AND expected_current_revision IS NOT NULL
            )
        ),
    CONSTRAINT sync_conflict_resolutions_outcome_value
        CHECK (
            outcome IN (
                'IN_PROGRESS',
                'ACCEPTED_SERVER',
                'APPLIED_CLIENT_INTENT',
                'STALE'
            )
        ),
    CONSTRAINT sync_conflict_resolutions_stale_reason_value
        CHECK (
            stale_reason IS NULL
            OR stale_reason IN (
                'REVISION_MISMATCH',
                'NODE_STATE_CHANGED',
                'PARENT_CHANGED',
                'NAME_OCCUPIED',
                'DESTINATION_CHANGED',
                'RESOURCE_PURGED'
            )
        ),
    CONSTRAINT sync_conflict_resolutions_stale_state_value
        CHECK (
            stale_server_state IS NULL
            OR stale_server_state IN ('ACTIVE', 'TRASHED', 'PURGING')
        ),
    CONSTRAINT sync_conflict_resolutions_revision_u64
        CHECK (
            (expected_current_revision IS NULL OR (
                expected_current_revision >= 0
                AND expected_current_revision = trunc(expected_current_revision)
                AND expected_current_revision <= 18446744073709551615::NUMERIC
            ))
            AND (expected_current_parent_revision IS NULL OR (
                expected_current_parent_revision >= 0
                AND expected_current_parent_revision = trunc(expected_current_parent_revision)
                AND expected_current_parent_revision <= 18446744073709551615::NUMERIC
            ))
            AND (stale_expected_revision IS NULL OR (
                stale_expected_revision >= 0
                AND stale_expected_revision = trunc(stale_expected_revision)
                AND stale_expected_revision <= 18446744073709551615::NUMERIC
            ))
            AND (stale_server_revision IS NULL OR (
                stale_server_revision >= 0
                AND stale_server_revision = trunc(stale_server_revision)
                AND stale_server_revision <= 18446744073709551615::NUMERIC
            ))
        ),
    CONSTRAINT sync_conflict_resolutions_terminal_shape
        CHECK (
            (
                outcome = 'IN_PROGRESS'
                AND completed_at IS NULL
                AND stale_reason IS NULL
                AND stale_resource_id IS NULL
                AND stale_expected_revision IS NULL
                AND stale_server_revision IS NULL
                AND stale_server_state IS NULL
                AND stale_server_parent_id IS NULL
                AND stale_server_name IS NULL
                AND stale_server_epoch IS NULL
                AND stale_server_sequence IS NULL
                AND journal_event_id IS NULL
                AND journal_sequence IS NULL
            )
            OR (
                outcome = 'ACCEPTED_SERVER'
                AND action = 'ACCEPT_SERVER'
                AND completed_at IS NOT NULL
                AND stale_reason IS NULL
                AND stale_resource_id IS NULL
                AND stale_expected_revision IS NULL
                AND stale_server_revision IS NULL
                AND stale_server_state IS NULL
                AND stale_server_parent_id IS NULL
                AND stale_server_name IS NULL
                AND stale_server_epoch IS NULL
                AND stale_server_sequence IS NULL
                AND journal_event_id IS NULL
                AND journal_sequence IS NULL
            )
            OR (
                outcome = 'APPLIED_CLIENT_INTENT'
                AND action = 'APPLY_CLIENT_INTENT'
                AND completed_at IS NOT NULL
                AND stale_reason IS NULL
                AND stale_resource_id IS NULL
                AND stale_expected_revision IS NULL
                AND stale_server_revision IS NULL
                AND stale_server_state IS NULL
                AND stale_server_parent_id IS NULL
                AND stale_server_name IS NULL
                AND stale_server_epoch IS NULL
                AND stale_server_sequence IS NULL
                AND journal_event_id IS NOT NULL
                AND journal_sequence > 0
            )
            OR (
                outcome = 'STALE'
                AND action = 'APPLY_CLIENT_INTENT'
                AND completed_at IS NOT NULL
                AND stale_reason IS NOT NULL
                AND stale_resource_id IS NOT NULL
                AND stale_server_epoch > 0
                AND stale_server_sequence >= 0
                AND journal_event_id IS NULL
                AND journal_sequence IS NULL
            )
        ),
    CONSTRAINT sync_conflict_resolutions_stale_name_bounds
        CHECK (
            stale_server_name IS NULL
            OR octet_length(stale_server_name) BETWEEN 1 AND 1024
        ),
    CONSTRAINT sync_conflict_resolutions_journal_fk
        FOREIGN KEY (journal_event_id)
        REFERENCES change_journal (entry_id) ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED
);

ALTER TABLE sync_conflicts
    ADD CONSTRAINT sync_conflicts_terminal_resolution_fk
        FOREIGN KEY (
            owner_user_id, device_id, library_id,
            conflict_id, terminal_resolution_id
        )
        REFERENCES sync_conflict_resolutions (
            owner_user_id, device_id, library_id,
            conflict_id, resolution_id
        )
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE device_mutation_operations
    ADD CONSTRAINT device_mutation_operations_conflict_link_fk
        FOREIGN KEY (
            owner_user_id, device_id, library_id,
            client_mutation_id, conflict_id
        )
        REFERENCES sync_conflicts (
            owner_user_id, device_id, library_id,
            original_client_mutation_id, conflict_id
        )
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE device_mutation_operations
    DROP CONSTRAINT device_mutation_operations_terminal_shape,
    ADD CONSTRAINT device_mutation_operations_terminal_shape
        CHECK (
            (
                outcome = 'IN_PROGRESS'
                AND completed_at IS NULL
                AND conflict_id IS NULL
            )
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
                AND conflict_id IS NULL
            )
            OR (
                outcome = 'CONFLICT'
                AND completed_at IS NOT NULL
                AND resource_id IS NOT NULL
                AND conflict_reason IS NOT NULL
                AND journal_event_id IS NULL
                AND journal_sequence IS NULL
                AND conflict_id IS NOT NULL
            )
        );

-- Original conflict evidence and completed resolution results are immutable.
-- The conflict row may transition once from OPEN to one terminal lifecycle;
-- no terminal decision may be reopened or rewritten.
CREATE FUNCTION synveil_enforce_sync_conflict_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF ROW(
        OLD.conflict_id,
        OLD.owner_user_id,
        OLD.device_id,
        OLD.library_id,
        OLD.original_client_mutation_id,
        OLD.mutation_kind,
        OLD.resource_id,
        OLD.conflict_reason,
        OLD.intent_node_id,
        OLD.intent_parent_node_id,
        OLD.intent_requested_parent_id,
        OLD.intent_expected_revision,
        OLD.intent_expected_parent_revision,
        OLD.intent_requested_name,
        OLD.historical_resource_id,
        OLD.historical_expected_revision,
        OLD.historical_server_revision,
        OLD.historical_server_state,
        OLD.historical_server_parent_id,
        OLD.historical_server_name,
        OLD.historical_server_epoch,
        OLD.historical_server_sequence,
        OLD.created_at
    ) IS DISTINCT FROM ROW(
        NEW.conflict_id,
        NEW.owner_user_id,
        NEW.device_id,
        NEW.library_id,
        NEW.original_client_mutation_id,
        NEW.mutation_kind,
        NEW.resource_id,
        NEW.conflict_reason,
        NEW.intent_node_id,
        NEW.intent_parent_node_id,
        NEW.intent_requested_parent_id,
        NEW.intent_expected_revision,
        NEW.intent_expected_parent_revision,
        NEW.intent_requested_name,
        NEW.historical_resource_id,
        NEW.historical_expected_revision,
        NEW.historical_server_revision,
        NEW.historical_server_state,
        NEW.historical_server_parent_id,
        NEW.historical_server_name,
        NEW.historical_server_epoch,
        NEW.historical_server_sequence,
        NEW.created_at
    ) THEN
        RAISE EXCEPTION 'sync conflict evidence is immutable';
    END IF;

    IF OLD.lifecycle <> 'OPEN' THEN
        RAISE EXCEPTION 'terminal sync conflict is immutable';
    END IF;
    IF NEW.lifecycle NOT IN ('RESOLVED', 'DISMISSED') THEN
        RAISE EXCEPTION 'sync conflict lifecycle transition is invalid';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_conflicts_update_guard
    BEFORE UPDATE ON sync_conflicts
    FOR EACH ROW
    EXECUTE FUNCTION synveil_enforce_sync_conflict_update();

CREATE FUNCTION synveil_enforce_sync_conflict_resolution_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.outcome <> 'IN_PROGRESS' THEN
        RAISE EXCEPTION 'terminal sync conflict resolution is immutable';
    END IF;
    IF ROW(
        OLD.resolution_id,
        OLD.owner_user_id,
        OLD.device_id,
        OLD.library_id,
        OLD.conflict_id,
        OLD.fingerprint_version,
        OLD.fingerprint,
        OLD.action,
        OLD.expected_current_revision,
        OLD.expected_current_parent_revision,
        OLD.created_at
    ) IS DISTINCT FROM ROW(
        NEW.resolution_id,
        NEW.owner_user_id,
        NEW.device_id,
        NEW.library_id,
        NEW.conflict_id,
        NEW.fingerprint_version,
        NEW.fingerprint,
        NEW.action,
        NEW.expected_current_revision,
        NEW.expected_current_parent_revision,
        NEW.created_at
    ) THEN
        RAISE EXCEPTION 'sync conflict resolution identity is immutable';
    END IF;
    IF NEW.outcome = 'IN_PROGRESS' THEN
        RAISE EXCEPTION 'sync conflict resolution must terminalize';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_conflict_resolutions_update_guard
    BEFORE UPDATE ON sync_conflict_resolutions
    FOR EACH ROW
    EXECUTE FUNCTION synveil_enforce_sync_conflict_resolution_update();
