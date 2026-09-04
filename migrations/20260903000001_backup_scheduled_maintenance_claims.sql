-- Prompt 66: durable scheduled-maintenance claim/lease with fenced
-- exactly-one-transition worker primitive.
--
-- One claim authorizes exactly one canonical Prompt 49 transition from one
-- expected maintenance state. Claim identity is
-- (maintenance_run_id, expected_state); the lease triple
-- (worker, token, generation) fences stale holders. A second worker may take
-- over only at or after lease expiry with generation N -> N+1 and a fresh
-- token. Completed receipts are immutable and history is never deleted.
-- This migration adds no daemon, polling loop, scheduler loop, heartbeat
-- renewal, retry/backoff loop, queue, notification, HTTP route, UI, SSE, or
-- WebSocket behavior. One explicit worker-step invocation performs at most
-- one semantic transition or one recovery reconciliation.

CREATE TABLE backup_scheduled_maintenance_claims (
    claim_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    schedule_id UUID NOT NULL,
    occurrence_id UUID NOT NULL,
    maintenance_run_id UUID NOT NULL,
    expected_state TEXT NOT NULL,
    resulting_state TEXT,
    lease_worker_id UUID NOT NULL,
    lease_token UUID NOT NULL,
    lease_generation BIGINT NOT NULL,
    lease_acquired_at TIMESTAMPTZ(6) NOT NULL,
    lease_expires_at TIMESTAMPTZ(6) NOT NULL,
    completed_at TIMESTAMPTZ(6),
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_scheduled_maintenance_claims_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_scheduled_maintenance_claims_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_scheduled_maintenance_claims_schedule_scope_fk
        FOREIGN KEY (schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedules (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_scheduled_maintenance_claims_occurrence_scope_fk
        FOREIGN KEY (occurrence_id, owner_user_id, backup_set_id, schedule_id)
        REFERENCES backup_schedule_occurrences (
            id, owner_user_id, backup_set_id, schedule_id
        ) ON DELETE RESTRICT,
    CONSTRAINT backup_scheduled_maintenance_claims_run_scope_fk
        FOREIGN KEY (maintenance_run_id, owner_user_id, backup_set_id)
        REFERENCES backup_maintenance_runs (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_scheduled_maintenance_claims_expected_value
        CHECK (expected_state IN (
            'CREATED',
            'SNAPSHOT_CAPTURED',
            'EXPIRY_PLANNED'
        )),
    CONSTRAINT backup_scheduled_maintenance_claims_result_shape
        CHECK (
            (expected_state = 'CREATED'
                AND (resulting_state IS NULL
                    OR resulting_state = 'SNAPSHOT_CAPTURED'))
            OR (expected_state = 'SNAPSHOT_CAPTURED'
                AND (resulting_state IS NULL
                    OR resulting_state = 'EXPIRY_PLANNED'))
            OR (expected_state = 'EXPIRY_PLANNED'
                AND (resulting_state IS NULL
                    OR resulting_state = 'COMPLETED'))
        ),
    CONSTRAINT backup_scheduled_maintenance_claims_completion_consistency
        CHECK (
            (completed_at IS NULL AND resulting_state IS NULL)
            OR (completed_at IS NOT NULL AND resulting_state IS NOT NULL)
        ),
    CONSTRAINT backup_scheduled_maintenance_claims_generation_value
        CHECK (lease_generation >= 1),
    CONSTRAINT backup_scheduled_maintenance_claims_lease_order
        CHECK (lease_expires_at > lease_acquired_at),
    CONSTRAINT backup_scheduled_maintenance_claims_updated_order
        CHECK (updated_at >= created_at),
    CONSTRAINT backup_scheduled_maintenance_claims_run_state_unique
        UNIQUE (maintenance_run_id, expected_state),
    CONSTRAINT backup_scheduled_maintenance_claims_lease_token_unique
        UNIQUE (lease_token)
);

CREATE INDEX backup_scheduled_maintenance_claims_run_incomplete_idx
    ON backup_scheduled_maintenance_claims (maintenance_run_id)
    WHERE completed_at IS NULL;

CREATE INDEX backup_scheduled_maintenance_claims_lease_expiry_idx
    ON backup_scheduled_maintenance_claims (lease_expires_at, claim_id)
    WHERE completed_at IS NULL;

CREATE INDEX backup_scheduled_maintenance_claims_owner_created_idx
    ON backup_scheduled_maintenance_claims (owner_user_id, created_at DESC, claim_id);

CREATE INDEX backup_scheduled_maintenance_claims_occurrence_idx
    ON backup_scheduled_maintenance_claims (occurrence_id);

-- Provenance fence: a claim may reference only a scheduled run whose Prompt 63
-- handoff durably binds the same occurrence, owner, BackupSet, and schedule.
-- Manual Prompt 49 runs have no handoff row and can never gain a claim.
CREATE FUNCTION synveil_validate_backup_scheduled_maintenance_claim()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.lease_generation <> 1 THEN
        RAISE EXCEPTION 'scheduled maintenance claim must start at generation 1'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_initial_generation';
    END IF;

    IF NEW.completed_at IS NOT NULL OR NEW.resulting_state IS NOT NULL THEN
        RAISE EXCEPTION 'scheduled maintenance claim must start incomplete'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_initial_incomplete';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM backup_schedule_occurrence_handoffs AS handoff
        WHERE handoff.occurrence_id = NEW.occurrence_id
          AND handoff.maintenance_run_id = NEW.maintenance_run_id
          AND handoff.owner_user_id = NEW.owner_user_id
          AND handoff.backup_set_id = NEW.backup_set_id
          AND handoff.schedule_id = NEW.schedule_id
    ) THEN
        RAISE EXCEPTION 'scheduled maintenance claim provenance has no matching handoff'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_handoff_scope';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_scheduled_maintenance_claims_validate
    BEFORE INSERT ON backup_scheduled_maintenance_claims
    FOR EACH ROW
    EXECUTE FUNCTION synveil_validate_backup_scheduled_maintenance_claim();

-- Lease state machine: completed receipts are immutable; incomplete claims
-- accept exactly two transitions. A takeover keeps the receipt open with
-- generation N -> N+1, a fresh token, and a new lease interval acquired at or
-- after the previous expiry. A completion seals the predetermined resulting
-- state with lease identity unchanged.
CREATE FUNCTION synveil_enforce_backup_scheduled_maintenance_claim_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.completed_at IS NOT NULL THEN
        RAISE EXCEPTION 'completed scheduled maintenance claims are immutable'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_completed_immutable';
    END IF;

    IF NEW.claim_id IS DISTINCT FROM OLD.claim_id
        OR NEW.owner_user_id IS DISTINCT FROM OLD.owner_user_id
        OR NEW.backup_set_id IS DISTINCT FROM OLD.backup_set_id
        OR NEW.schedule_id IS DISTINCT FROM OLD.schedule_id
        OR NEW.occurrence_id IS DISTINCT FROM OLD.occurrence_id
        OR NEW.maintenance_run_id IS DISTINCT FROM OLD.maintenance_run_id
        OR NEW.expected_state IS DISTINCT FROM OLD.expected_state
        OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'scheduled maintenance claim provenance is immutable'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_provenance_immutable';
    END IF;

    IF NEW.completed_at IS NULL THEN
        IF NEW.resulting_state IS NOT NULL THEN
            RAISE EXCEPTION 'incomplete scheduled maintenance claim cannot carry a result'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_scheduled_maintenance_claims_takeover_result';
        END IF;
        IF NEW.lease_generation <> OLD.lease_generation + 1 THEN
            RAISE EXCEPTION 'scheduled maintenance lease takeover must increment generation by one'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_scheduled_maintenance_claims_generation_monotonic';
        END IF;
        IF NEW.lease_token IS NOT DISTINCT FROM OLD.lease_token THEN
            RAISE EXCEPTION 'scheduled maintenance lease takeover requires a fresh token'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_scheduled_maintenance_claims_token_rotation';
        END IF;
        IF NEW.lease_acquired_at < OLD.lease_expires_at THEN
            RAISE EXCEPTION 'scheduled maintenance lease takeover requires expiry'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_scheduled_maintenance_claims_takeover_expiry';
        END IF;
        IF NEW.updated_at < OLD.updated_at THEN
            RAISE EXCEPTION 'scheduled maintenance claim update must not move updated_at backwards'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_scheduled_maintenance_claims_updated_monotonic';
        END IF;
        RETURN NEW;
    END IF;

    IF OLD.completed_at IS NOT NULL THEN
        RAISE EXCEPTION 'completed scheduled maintenance claims are immutable'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_completed_immutable';
    END IF;
    IF NEW.lease_worker_id IS DISTINCT FROM OLD.lease_worker_id
        OR NEW.lease_token IS DISTINCT FROM OLD.lease_token
        OR NEW.lease_generation IS DISTINCT FROM OLD.lease_generation
        OR NEW.lease_acquired_at IS DISTINCT FROM OLD.lease_acquired_at
        OR NEW.lease_expires_at IS DISTINCT FROM OLD.lease_expires_at THEN
        RAISE EXCEPTION 'scheduled maintenance claim completion cannot alter lease identity'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_completion_lease_frozen';
    END IF;
    IF (NEW.expected_state = 'CREATED'
            AND NEW.resulting_state IS DISTINCT FROM 'SNAPSHOT_CAPTURED')
        OR (NEW.expected_state = 'SNAPSHOT_CAPTURED'
            AND NEW.resulting_state IS DISTINCT FROM 'EXPIRY_PLANNED')
        OR (NEW.expected_state = 'EXPIRY_PLANNED'
            AND NEW.resulting_state IS DISTINCT FROM 'COMPLETED') THEN
        RAISE EXCEPTION 'scheduled maintenance claim result does not match its expected state'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_completion_result';
    END IF;
    IF NEW.updated_at < OLD.updated_at THEN
        RAISE EXCEPTION 'scheduled maintenance claim update must not move updated_at backwards'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_scheduled_maintenance_claims_updated_monotonic';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_scheduled_maintenance_claims_enforce_update
    BEFORE UPDATE ON backup_scheduled_maintenance_claims
    FOR EACH ROW
    EXECUTE FUNCTION synveil_enforce_backup_scheduled_maintenance_claim_update();

CREATE FUNCTION synveil_reject_backup_scheduled_maintenance_claim_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'scheduled maintenance claims cannot be deleted'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_scheduled_maintenance_claims_delete_forbidden';
END;
$$;

CREATE TRIGGER backup_scheduled_maintenance_claims_no_delete
    BEFORE DELETE ON backup_scheduled_maintenance_claims
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_scheduled_maintenance_claim_delete();
