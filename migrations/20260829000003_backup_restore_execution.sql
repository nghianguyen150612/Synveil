-- Atomic metadata-only execution of one durable restore plan.
--
-- The service assembles the execution and its logical entry evidence in one
-- PostgreSQL transaction. ASSEMBLING is transaction-local: deferred sealing
-- checks prevent either an incomplete execution or an EXECUTED plan from
-- becoming durable without its canonical receipt.

ALTER TABLE backup_restore_plans
    ADD CONSTRAINT backup_restore_plans_id_owner_unique
    UNIQUE (id, owner_user_id);

ALTER TABLE backup_restore_plans
    DROP CONSTRAINT backup_restore_plans_state_value,
    ADD CONSTRAINT backup_restore_plans_state_value
        CHECK (state IN ('ASSEMBLING', 'PLANNED', 'STALE', 'EXECUTED')),
    DROP CONSTRAINT backup_restore_plans_stale_shape,
    ADD CONSTRAINT backup_restore_plans_stale_shape
        CHECK (
            (state IN ('ASSEMBLING', 'PLANNED', 'EXECUTED') AND stale_at IS NULL)
            OR (state = 'STALE' AND stale_at IS NOT NULL)
        );

-- Preserve every Prompt 43 provenance column and allow only the new terminal
-- lifecycle transition. The trigger remains the single update gate.
CREATE OR REPLACE FUNCTION synveil_enforce_backup_restore_plan_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup restore plans cannot be deleted in this phase'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_plan_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.snapshot_id IS NOT DISTINCT FROM NEW.snapshot_id
       AND OLD.target_library_id IS NOT DISTINCT FROM NEW.target_library_id
       AND OLD.target_parent_node_id IS NOT DISTINCT FROM NEW.target_parent_node_id
       AND OLD.operation_id IS NOT DISTINCT FROM NEW.operation_id
       AND OLD.fingerprint_version IS NOT DISTINCT FROM NEW.fingerprint_version
       AND OLD.request_fingerprint IS NOT DISTINCT FROM NEW.request_fingerprint
       AND OLD.destination_name IS NOT DISTINCT FROM NEW.destination_name
       AND OLD.base_journal_epoch IS NOT DISTINCT FROM NEW.base_journal_epoch
       AND OLD.base_journal_head IS NOT DISTINCT FROM NEW.base_journal_head
       AND OLD.item_count IS NOT DISTINCT FROM NEW.item_count
       AND OLD.content_item_count IS NOT DISTINCT FROM NEW.content_item_count
       AND OLD.created_at IS NOT DISTINCT FROM NEW.created_at
       AND (
           (OLD.state IS NOT DISTINCT FROM NEW.state
            AND OLD.stale_at IS NOT DISTINCT FROM NEW.stale_at)
           OR (OLD.state = 'PLANNED'
               AND NEW.state = 'STALE'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NOT NULL)
           OR (OLD.state = 'ASSEMBLING'
               AND NEW.state = 'PLANNED'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NULL)
           OR (OLD.state = 'PLANNED'
               AND NEW.state = 'EXECUTED'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NULL)
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup restore plan provenance is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_restore_plan_immutable';
END;
$$;

CREATE TABLE backup_restore_executions (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    restore_plan_id UUID NOT NULL,
    target_library_id UUID NOT NULL,
    journal_first_sequence BIGINT NOT NULL,
    journal_last_sequence BIGINT NOT NULL,
    created_node_count BIGINT NOT NULL,
    created_file_version_count BIGINT NOT NULL,
    state TEXT NOT NULL,
    executed_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_restore_executions_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_executions_plan_owner_fk
        FOREIGN KEY (restore_plan_id, owner_user_id)
        REFERENCES backup_restore_plans (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_executions_target_library_owner_fk
        FOREIGN KEY (target_library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_executions_plan_unique
        UNIQUE (restore_plan_id),
    CONSTRAINT backup_restore_executions_state_value
        CHECK (state IN ('ASSEMBLING', 'COMMITTED')),
    CONSTRAINT backup_restore_executions_sequence_shape
        CHECK (
            journal_first_sequence > 0
            AND journal_last_sequence >= journal_first_sequence
        ),
    CONSTRAINT backup_restore_executions_count_shape
        CHECK (
            created_node_count > 0
            AND created_file_version_count >= 0
            AND created_file_version_count <= created_node_count
        )
);

CREATE INDEX backup_restore_executions_owner_created_idx
    ON backup_restore_executions (owner_user_id, executed_at DESC, id DESC);

-- Execution identity is bound to the locked owner-scoped plan. Only the
-- service's transaction-local ASSEMBLING row may be inserted.
CREATE FUNCTION synveil_validate_backup_restore_execution_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    plan_target_library_id UUID;
    plan_state TEXT;
BEGIN
    IF NEW.state IS DISTINCT FROM 'ASSEMBLING' THEN
        RAISE EXCEPTION 'restore execution must begin in assembly state'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_execution_initial_state';
    END IF;

    SELECT target_library_id, state
      INTO plan_target_library_id, plan_state
      FROM backup_restore_plans
     WHERE id = NEW.restore_plan_id
       AND owner_user_id = NEW.owner_user_id
     FOR SHARE;
    IF plan_target_library_id IS NULL
       OR plan_state IS DISTINCT FROM 'PLANNED'
       OR plan_target_library_id IS DISTINCT FROM NEW.target_library_id THEN
        RAISE EXCEPTION 'restore execution identity does not match planned target'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_execution_identity';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_restore_executions_validate_identity
BEFORE INSERT ON backup_restore_executions
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_restore_execution_identity();

-- Once inserted, the receipt is immutable. Its only permitted mutation is the
-- transaction-local ASSEMBLING -> COMMITTED seal.
CREATE FUNCTION synveil_enforce_backup_restore_execution_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup restore executions cannot be deleted'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_execution_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.restore_plan_id IS NOT DISTINCT FROM NEW.restore_plan_id
       AND OLD.target_library_id IS NOT DISTINCT FROM NEW.target_library_id
       AND OLD.journal_first_sequence IS NOT DISTINCT FROM NEW.journal_first_sequence
       AND OLD.journal_last_sequence IS NOT DISTINCT FROM NEW.journal_last_sequence
       AND OLD.created_node_count IS NOT DISTINCT FROM NEW.created_node_count
       AND OLD.created_file_version_count IS NOT DISTINCT FROM NEW.created_file_version_count
       AND OLD.executed_at IS NOT DISTINCT FROM NEW.executed_at
       AND (
           OLD.state IS NOT DISTINCT FROM NEW.state
           OR (OLD.state = 'ASSEMBLING' AND NEW.state = 'COMMITTED')
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup restore execution receipt is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_restore_execution_immutable';
END;
$$;

CREATE TRIGGER backup_restore_executions_immutable
BEFORE UPDATE OR DELETE ON backup_restore_executions
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_restore_execution_immutability();

CREATE FUNCTION synveil_require_backup_restore_execution_sealed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM backup_restore_executions
         WHERE id = NEW.id
           AND state = 'ASSEMBLING'
    ) THEN
        RAISE EXCEPTION 'backup restore execution assembly must be sealed before commit'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_execution_unsealed';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_restore_executions_must_be_sealed
AFTER INSERT OR UPDATE ON backup_restore_executions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_restore_execution_sealed();

CREATE TABLE backup_restore_execution_entries (
    execution_id UUID NOT NULL,
    plan_id UUID NOT NULL,
    ordinal BIGINT NOT NULL,
    destination_node_id UUID NOT NULL,
    destination_file_version_id UUID,
    node_created_journal_sequence BIGINT,
    file_content_committed_journal_sequence BIGINT,
    CONSTRAINT backup_restore_execution_entries_execution_fk
        FOREIGN KEY (execution_id) REFERENCES backup_restore_executions (id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_restore_execution_entries_destination_node_fk
        FOREIGN KEY (destination_node_id) REFERENCES nodes (id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_restore_execution_entries_destination_version_fk
        FOREIGN KEY (destination_file_version_id) REFERENCES file_versions (id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_restore_execution_entries_plan_entry_fk
        FOREIGN KEY (plan_id, ordinal)
        REFERENCES backup_restore_plan_entries (plan_id, ordinal)
        ON DELETE RESTRICT,
    CONSTRAINT backup_restore_execution_entries_ordinal_nonnegative
        CHECK (ordinal >= 0),
    CONSTRAINT backup_restore_execution_entries_destination_id_nonzero
        CHECK (destination_node_id <> '00000000-0000-0000-0000-000000000000'::UUID),
    CONSTRAINT backup_restore_execution_entries_sequence_positive
        CHECK (
            (node_created_journal_sequence IS NULL
             OR node_created_journal_sequence > 0)
            AND (file_content_committed_journal_sequence IS NULL
                 OR file_content_committed_journal_sequence > 0)
        ),
    CONSTRAINT backup_restore_execution_entries_evidence_shape
        CHECK (
            (
                destination_file_version_id IS NULL
                AND node_created_journal_sequence IS NOT NULL
                AND file_content_committed_journal_sequence IS NULL
            )
            OR (
                destination_file_version_id IS NOT NULL
                AND node_created_journal_sequence IS NULL
                AND file_content_committed_journal_sequence IS NOT NULL
            )
        ),
    PRIMARY KEY (execution_id, ordinal),
    CONSTRAINT backup_restore_execution_entries_destination_unique
        UNIQUE (execution_id, destination_node_id),
    CONSTRAINT backup_restore_execution_entries_file_version_unique
        UNIQUE (execution_id, destination_file_version_id)
);

CREATE FUNCTION synveil_validate_backup_restore_execution_entry()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    execution_plan_id UUID;
    execution_state TEXT;
    plan_entry_node_id UUID;
BEGIN
    SELECT restore_plan_id, state
      INTO execution_plan_id, execution_state
      FROM backup_restore_executions
     WHERE id = NEW.execution_id
     FOR SHARE;
    IF execution_plan_id IS NULL
       OR execution_state IS DISTINCT FROM 'ASSEMBLING'
       OR execution_plan_id IS DISTINCT FROM NEW.plan_id THEN
        RAISE EXCEPTION 'restore execution entry is outside its assembly transaction'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_execution_entry_identity';
    END IF;

    SELECT planned_node_id
      INTO plan_entry_node_id
      FROM backup_restore_plan_entries
     WHERE plan_id = NEW.plan_id
       AND ordinal = NEW.ordinal;
    IF plan_entry_node_id IS NULL
       OR plan_entry_node_id IS DISTINCT FROM NEW.destination_node_id THEN
        RAISE EXCEPTION 'restore execution entry does not match its plan entry'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_execution_entry_plan_mismatch';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_restore_execution_entries_validate
BEFORE INSERT ON backup_restore_execution_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_restore_execution_entry();

CREATE FUNCTION synveil_reject_backup_restore_execution_entry_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup restore execution entries are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_restore_execution_entry_immutable';
END;
$$;

CREATE TRIGGER backup_restore_execution_entries_immutable
BEFORE UPDATE OR DELETE ON backup_restore_execution_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_restore_execution_entry_mutation();

-- A plan cannot become EXECUTED without the committed canonical receipt. This
-- is deferred because the service seals the execution and plan in one unit.
CREATE FUNCTION synveil_require_backup_restore_plan_execution_receipt()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'EXECUTED'
       AND NOT EXISTS (
           SELECT 1
             FROM backup_restore_executions AS execution
            WHERE execution.restore_plan_id = NEW.id
              AND execution.owner_user_id = NEW.owner_user_id
              AND execution.target_library_id = NEW.target_library_id
              AND execution.state = 'COMMITTED'
       ) THEN
        RAISE EXCEPTION 'executed restore plan requires a committed execution receipt'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_plan_execution_receipt';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_restore_plans_require_execution_receipt
AFTER INSERT OR UPDATE ON backup_restore_plans
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_restore_plan_execution_receipt();
