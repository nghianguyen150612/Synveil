-- Durable manual backup maintenance-run orchestration. This migration adds only
-- coordinator evidence for explicitly chaining snapshot capture, snapshot
-- expiry planning, and expiry execution. It does not add scheduling, pruning,
-- retention-pin release, GC handoff, Object/ObjectReplica lifecycle changes, or
-- ObjectStore I/O.

CREATE TABLE backup_maintenance_runs (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    policy_revision_id UUID NOT NULL,
    policy_revision_number BIGINT NOT NULL,
    operation_id TEXT NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    capture_operation_id TEXT NOT NULL,
    expiry_plan_operation_id TEXT NOT NULL,
    state TEXT NOT NULL,
    captured_snapshot_id UUID,
    expiry_plan_id UUID,
    expiry_execution_id UUID,
    snapshot_captured_at TIMESTAMPTZ(6),
    expiry_planned_at TIMESTAMPTZ(6),
    maintenance_completed_at TIMESTAMPTZ(6),
    stale_at TIMESTAMPTZ(6),
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_maintenance_runs_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_policy_scope_fk
        FOREIGN KEY (
            policy_revision_id,
            policy_revision_number,
            backup_set_id,
            owner_user_id
        ) REFERENCES backup_snapshot_retention_policy_revisions (
            id,
            revision_number,
            backup_set_id,
            owner_user_id
        ) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_snapshot_owner_fk
        FOREIGN KEY (captured_snapshot_id, owner_user_id)
        REFERENCES backup_snapshots (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_snapshot_set_fk
        FOREIGN KEY (captured_snapshot_id, backup_set_id)
        REFERENCES backup_snapshots (id, backup_set_id) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_expiry_plan_owner_fk
        FOREIGN KEY (expiry_plan_id, owner_user_id)
        REFERENCES backup_snapshot_expiry_plans (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_expiry_execution_fk
        FOREIGN KEY (expiry_execution_id)
        REFERENCES backup_snapshot_expiry_executions (id) ON DELETE RESTRICT,
    CONSTRAINT backup_maintenance_runs_operation_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_maintenance_runs_capture_operation_length
        CHECK (octet_length(capture_operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_maintenance_runs_expiry_plan_operation_length
        CHECK (octet_length(expiry_plan_operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_maintenance_runs_child_operation_distinct
        CHECK (
            operation_id <> capture_operation_id
            AND operation_id <> expiry_plan_operation_id
            AND capture_operation_id <> expiry_plan_operation_id
        ),
    CONSTRAINT backup_maintenance_runs_fingerprint_version_value
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_maintenance_runs_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_maintenance_runs_state_value
        CHECK (state IN (
            'CREATED',
            'SNAPSHOT_CAPTURED',
            'EXPIRY_PLANNED',
            'COMPLETED',
            'STALE'
        )),
    CONSTRAINT backup_maintenance_runs_state_reference_shape
        CHECK (
            (
                state = 'CREATED'
                AND captured_snapshot_id IS NULL
                AND expiry_plan_id IS NULL
                AND expiry_execution_id IS NULL
                AND snapshot_captured_at IS NULL
                AND expiry_planned_at IS NULL
                AND maintenance_completed_at IS NULL
                AND stale_at IS NULL
            ) OR (
                state = 'SNAPSHOT_CAPTURED'
                AND captured_snapshot_id IS NOT NULL
                AND expiry_plan_id IS NULL
                AND expiry_execution_id IS NULL
                AND snapshot_captured_at IS NOT NULL
                AND expiry_planned_at IS NULL
                AND maintenance_completed_at IS NULL
                AND stale_at IS NULL
            ) OR (
                state = 'EXPIRY_PLANNED'
                AND captured_snapshot_id IS NOT NULL
                AND expiry_plan_id IS NOT NULL
                AND expiry_execution_id IS NULL
                AND snapshot_captured_at IS NOT NULL
                AND expiry_planned_at IS NOT NULL
                AND maintenance_completed_at IS NULL
                AND stale_at IS NULL
            ) OR (
                state = 'COMPLETED'
                AND captured_snapshot_id IS NOT NULL
                AND expiry_plan_id IS NOT NULL
                AND expiry_execution_id IS NOT NULL
                AND snapshot_captured_at IS NOT NULL
                AND expiry_planned_at IS NOT NULL
                AND maintenance_completed_at IS NOT NULL
                AND stale_at IS NULL
            ) OR (
                state = 'STALE'
                AND expiry_execution_id IS NULL
                AND maintenance_completed_at IS NULL
                AND stale_at IS NOT NULL
            )
        ),
    CONSTRAINT backup_maintenance_runs_timestamp_order
        CHECK (
            (snapshot_captured_at IS NULL OR snapshot_captured_at >= created_at)
            AND (expiry_planned_at IS NULL OR (
                snapshot_captured_at IS NOT NULL
                AND expiry_planned_at >= snapshot_captured_at
            ))
            AND (maintenance_completed_at IS NULL OR (
                expiry_planned_at IS NOT NULL
                AND maintenance_completed_at >= expiry_planned_at
            ))
            AND (stale_at IS NULL OR stale_at >= created_at)
        ),
    CONSTRAINT backup_maintenance_runs_owner_operation_unique
        UNIQUE (owner_user_id, operation_id)
);

CREATE INDEX backup_maintenance_runs_owner_created_idx
    ON backup_maintenance_runs (owner_user_id, created_at DESC, id DESC);

CREATE UNIQUE INDEX backup_maintenance_runs_one_active_set
    ON backup_maintenance_runs (backup_set_id)
    WHERE state IN ('CREATED', 'SNAPSHOT_CAPTURED', 'EXPIRY_PLANNED');

CREATE FUNCTION synveil_validate_backup_maintenance_run_child_references()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    snapshot_operation_id TEXT;
    snapshot_state TEXT;
    plan_operation_id TEXT;
    plan_backup_set_id UUID;
    plan_policy_revision_id UUID;
    plan_policy_revision_number BIGINT;
    execution_plan_id UUID;
BEGIN
    IF NEW.captured_snapshot_id IS NOT NULL THEN
        SELECT operation_id, state
          INTO snapshot_operation_id, snapshot_state
          FROM backup_snapshots
         WHERE id = NEW.captured_snapshot_id
           AND owner_user_id = NEW.owner_user_id
           AND backup_set_id = NEW.backup_set_id;
        IF snapshot_operation_id IS DISTINCT FROM NEW.capture_operation_id
           OR snapshot_state NOT IN ('COMPLETED', 'EXPIRED') THEN
            RAISE EXCEPTION 'maintenance run snapshot reference does not match its child operation'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_maintenance_run_snapshot_child_identity';
        END IF;
    END IF;

    IF NEW.expiry_plan_id IS NOT NULL THEN
        SELECT operation_id, backup_set_id, policy_revision_id,
               policy_revision_number
          INTO plan_operation_id, plan_backup_set_id, plan_policy_revision_id,
               plan_policy_revision_number
          FROM backup_snapshot_expiry_plans
         WHERE id = NEW.expiry_plan_id
           AND owner_user_id = NEW.owner_user_id;
        IF plan_operation_id IS DISTINCT FROM NEW.expiry_plan_operation_id
           OR plan_backup_set_id IS DISTINCT FROM NEW.backup_set_id
           OR plan_policy_revision_id IS DISTINCT FROM NEW.policy_revision_id
           OR plan_policy_revision_number IS DISTINCT FROM NEW.policy_revision_number THEN
            RAISE EXCEPTION 'maintenance run expiry-plan reference does not match its child operation'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_maintenance_run_expiry_plan_child_identity';
        END IF;
    END IF;

    IF NEW.expiry_execution_id IS NOT NULL THEN
        SELECT expiry_plan_id
          INTO execution_plan_id
          FROM backup_snapshot_expiry_executions
         WHERE id = NEW.expiry_execution_id
           AND owner_user_id = NEW.owner_user_id
           AND state = 'COMMITTED';
        IF execution_plan_id IS DISTINCT FROM NEW.expiry_plan_id THEN
            RAISE EXCEPTION 'maintenance run expiry-execution reference does not match its plan'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_maintenance_run_expiry_execution_child_identity';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_maintenance_runs_validate_child_references
BEFORE INSERT OR UPDATE ON backup_maintenance_runs
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_maintenance_run_child_references();

CREATE FUNCTION synveil_enforce_backup_maintenance_run_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup maintenance runs cannot be deleted'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_maintenance_run_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.policy_revision_id IS NOT DISTINCT FROM NEW.policy_revision_id
       AND OLD.policy_revision_number IS NOT DISTINCT FROM NEW.policy_revision_number
       AND OLD.operation_id IS NOT DISTINCT FROM NEW.operation_id
       AND OLD.fingerprint_version IS NOT DISTINCT FROM NEW.fingerprint_version
       AND OLD.request_fingerprint IS NOT DISTINCT FROM NEW.request_fingerprint
       AND OLD.capture_operation_id IS NOT DISTINCT FROM NEW.capture_operation_id
       AND OLD.expiry_plan_operation_id IS NOT DISTINCT FROM NEW.expiry_plan_operation_id
       AND OLD.created_at IS NOT DISTINCT FROM NEW.created_at
       AND (
           OLD.captured_snapshot_id IS NOT DISTINCT FROM NEW.captured_snapshot_id
           OR (OLD.captured_snapshot_id IS NULL AND NEW.captured_snapshot_id IS NOT NULL)
       )
       AND (
           OLD.expiry_plan_id IS NOT DISTINCT FROM NEW.expiry_plan_id
           OR (OLD.expiry_plan_id IS NULL AND NEW.expiry_plan_id IS NOT NULL)
       )
       AND (
           OLD.expiry_execution_id IS NOT DISTINCT FROM NEW.expiry_execution_id
           OR (OLD.expiry_execution_id IS NULL AND NEW.expiry_execution_id IS NOT NULL)
       )
       AND (
           OLD.snapshot_captured_at IS NOT DISTINCT FROM NEW.snapshot_captured_at
           OR (OLD.snapshot_captured_at IS NULL AND NEW.snapshot_captured_at IS NOT NULL)
       )
       AND (
           OLD.expiry_planned_at IS NOT DISTINCT FROM NEW.expiry_planned_at
           OR (OLD.expiry_planned_at IS NULL AND NEW.expiry_planned_at IS NOT NULL)
       )
       AND (
           OLD.maintenance_completed_at IS NOT DISTINCT FROM NEW.maintenance_completed_at
           OR (OLD.maintenance_completed_at IS NULL AND NEW.maintenance_completed_at IS NOT NULL)
       )
       AND (
           OLD.stale_at IS NOT DISTINCT FROM NEW.stale_at
           OR (OLD.stale_at IS NULL AND NEW.stale_at IS NOT NULL)
       )
       AND (
           (OLD.state = NEW.state)
           OR (OLD.state = 'CREATED' AND NEW.state IN ('SNAPSHOT_CAPTURED', 'STALE'))
           OR (OLD.state = 'SNAPSHOT_CAPTURED' AND NEW.state IN ('EXPIRY_PLANNED', 'STALE'))
           OR (OLD.state = 'EXPIRY_PLANNED' AND NEW.state IN ('COMPLETED', 'STALE'))
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup maintenance run provenance is immutable and monotonic'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_maintenance_run_immutable';
END;
$$;

CREATE TRIGGER backup_maintenance_runs_immutable
BEFORE UPDATE OR DELETE ON backup_maintenance_runs
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_maintenance_run_immutability();
