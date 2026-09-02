-- Atomic backup prune execution: release target-snapshot retention pins,
-- handoff newly unreferenced Objects to the canonical GC pipeline, persist an
-- immutable execution receipt, and transition PLANNED -> EXECUTED.
--
-- The service assembles the execution and its per-Object evidence in one
-- PostgreSQL transaction (mirroring the restore-execution ASSEMBLING pattern).
-- ASSEMBLING is transaction-local: deferred sealing checks prevent an
-- incomplete execution or an EXECUTED plan without its canonical receipt from
-- becoming durable.
--
-- Prompt 42B pin immutability is preserved: direct arbitrary pin INSERT /
-- UPDATE / DELETE remain rejected. This migration adds ONE narrow authorized
-- release path gated by a PLANNED plan, an EXPIRED owner-scoped source
-- snapshot, and a transaction-local execution authorization token that only
-- the authorized release function may issue.

-- Extend prune plan lifecycle to include the terminal EXECUTED state.
ALTER TABLE backup_prune_plans
    DROP CONSTRAINT backup_prune_plans_state_value,
    ADD CONSTRAINT backup_prune_plans_state_value
        CHECK (state IN ('ASSEMBLING', 'PLANNED', 'STALE', 'EXECUTED')),
    DROP CONSTRAINT backup_prune_plans_stale_shape,
    ADD CONSTRAINT backup_prune_plans_stale_shape
        CHECK (
            (state IN ('ASSEMBLING', 'PLANNED', 'EXECUTED') AND stale_at IS NULL)
            OR (state = 'STALE' AND stale_at IS NOT NULL)
        );

-- The execution receipt FK references (id, owner_user_id), matching the
-- restore-execution pattern; ensure the parent key is unique.
ALTER TABLE backup_prune_plans
    ADD CONSTRAINT backup_prune_plans_id_owner_unique
    UNIQUE (id, owner_user_id);

-- Prompt 45's deferred seal trigger only accepted ASSEMBLING/PLANNED/STALE as
-- terminal plan states. Extend it in place (via CREATE OR REPLACE, leaving the
-- accepted migrations untouched) so an EXECUTED plan with its committed receipt
-- can commit. For EXECUTED rows it returns before the pre-release pin-count
-- rechecks, because those pins are legitimately released after execution.
CREATE OR REPLACE FUNCTION synveil_require_backup_prune_plan_sealed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    current_plan_state TEXT;
    source_state TEXT;
    source_manifest_count BIGINT;
    source_content_count BIGINT;
    source_pin_count BIGINT;
    source_distinct_object_count BIGINT;
    entry_count BIGINT;
    impact_count BIGINT;
    retained_count BIGINT;
    unreferenced_count BIGINT;
BEGIN
    -- This is a deferred constraint trigger. Its queued INSERT event still
    -- carries ASSEMBLING in NEW after the same transaction has sealed the row,
    -- so inspect the final durable row rather than the event image.
    SELECT state
      INTO current_plan_state
      FROM backup_prune_plans
     WHERE id = NEW.id;
    IF current_plan_state IS DISTINCT FROM 'ASSEMBLING'
       AND current_plan_state IS DISTINCT FROM 'PLANNED'
       AND current_plan_state IS DISTINCT FROM 'STALE'
       AND current_plan_state IS DISTINCT FROM 'EXECUTED' THEN
        RAISE EXCEPTION 'backup prune plan final state is invalid'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_prune_plan_final_state';
    END IF;
    IF current_plan_state = 'ASSEMBLING' THEN
        RAISE EXCEPTION 'backup prune plan assembly must be sealed before commit'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_unsealed';
    END IF;

    IF current_plan_state <> 'PLANNED' THEN
        RETURN NULL;
    END IF;

    SELECT state, manifest_item_count, content_reference_count
      INTO source_state, source_manifest_count, source_content_count
      FROM backup_snapshots
     WHERE id = NEW.snapshot_id
       AND backup_set_id = NEW.backup_set_id
       AND owner_user_id = NEW.owner_user_id;
    IF source_state IS DISTINCT FROM 'EXPIRED'
       OR source_manifest_count IS DISTINCT FROM NEW.snapshot_manifest_item_count
       OR source_content_count IS DISTINCT FROM NEW.snapshot_content_reference_count THEN
        RAISE EXCEPTION 'backup prune plan source provenance is invalid'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_prune_plan_source_provenance';
    END IF;

    SELECT count(*) INTO source_pin_count
      FROM backup_snapshot_content_pins
     WHERE snapshot_id = NEW.snapshot_id;
    SELECT count(*) INTO source_distinct_object_count
      FROM (
          SELECT DISTINCT object_id, object_dedup_domain_id
            FROM backup_snapshot_content_pins
           WHERE snapshot_id = NEW.snapshot_id
      ) AS source_objects;
    SELECT count(*) INTO entry_count
      FROM backup_prune_plan_entries
     WHERE plan_id = NEW.id;
    SELECT count(*),
           count(*) FILTER (WHERE impact = 'RETAINED_BY_OTHER_REFERENCE'),
           count(*) FILTER (WHERE impact = 'WOULD_BECOME_UNREFERENCED')
      INTO impact_count, retained_count, unreferenced_count
      FROM backup_prune_plan_object_impacts
     WHERE plan_id = NEW.id;

    IF entry_count IS DISTINCT FROM NEW.planned_pin_release_count
       OR source_pin_count IS DISTINCT FROM NEW.planned_pin_release_count
       OR impact_count IS DISTINCT FROM NEW.distinct_retained_content_count
       OR source_distinct_object_count IS DISTINCT FROM NEW.distinct_retained_content_count
       OR retained_count IS DISTINCT FROM NEW.retained_after_release_count
       OR unreferenced_count IS DISTINCT FROM NEW.would_become_unreferenced_count THEN
        RAISE EXCEPTION 'backup prune plan evidence counts are incomplete'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_prune_plan_evidence_counts';
    END IF;
    RETURN NULL;
END;
$$;

-- Extend prune-plan immutability to allow only PLANNED -> EXECUTED in addition
-- to the accepted ASSEMBLING -> PLANNED and PLANNED -> STALE transitions.
CREATE OR REPLACE FUNCTION synveil_enforce_backup_prune_plan_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup prune plans cannot be deleted in this phase'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.snapshot_id IS NOT DISTINCT FROM NEW.snapshot_id
       AND OLD.operation_id IS NOT DISTINCT FROM NEW.operation_id
       AND OLD.fingerprint_version IS NOT DISTINCT FROM NEW.fingerprint_version
       AND OLD.request_fingerprint IS NOT DISTINCT FROM NEW.request_fingerprint
       AND OLD.snapshot_manifest_item_count
           IS NOT DISTINCT FROM NEW.snapshot_manifest_item_count
       AND OLD.snapshot_content_reference_count
           IS NOT DISTINCT FROM NEW.snapshot_content_reference_count
       AND OLD.planned_pin_release_count
           IS NOT DISTINCT FROM NEW.planned_pin_release_count
       AND OLD.distinct_retained_content_count
           IS NOT DISTINCT FROM NEW.distinct_retained_content_count
       AND OLD.retained_after_release_count
           IS NOT DISTINCT FROM NEW.retained_after_release_count
       AND OLD.would_become_unreferenced_count
           IS NOT DISTINCT FROM NEW.would_become_unreferenced_count
       AND OLD.created_at IS NOT DISTINCT FROM NEW.created_at
       AND (
           (OLD.state IS NOT DISTINCT FROM NEW.state
            AND OLD.stale_at IS NOT DISTINCT FROM NEW.stale_at)
           OR (OLD.state = 'ASSEMBLING'
               AND NEW.state = 'PLANNED'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NULL)
           OR (OLD.state = 'PLANNED'
               AND NEW.state = 'STALE'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NOT NULL)
           OR (OLD.state = 'PLANNED'
               AND NEW.state = 'EXECUTED'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NULL)
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup prune plan provenance is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_prune_plan_immutable';
END;
$$;

-- Canonical immutable prune execution receipt. Only the service's
-- transaction-local ASSEMBLING row may be inserted; the deferred seal trigger
-- rejects an incomplete execution at commit, so a committed row is always a
-- canonical COMMITTED receipt.
CREATE TABLE backup_prune_executions (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    prune_plan_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    released_pin_count BIGINT NOT NULL,
    distinct_object_count BIGINT NOT NULL,
    retained_by_other_reference_count BIGINT NOT NULL,
    gc_handoff_object_count BIGINT NOT NULL,
    state TEXT NOT NULL,
    executed_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_prune_executions_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_executions_plan_owner_fk
        FOREIGN KEY (prune_plan_id, owner_user_id)
        REFERENCES backup_prune_plans (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_executions_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_executions_snapshot_owner_fk
        FOREIGN KEY (snapshot_id, owner_user_id)
        REFERENCES backup_snapshots (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_executions_snapshot_set_fk
        FOREIGN KEY (snapshot_id, backup_set_id)
        REFERENCES backup_snapshots (id, backup_set_id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_executions_plan_unique
        UNIQUE (prune_plan_id),
    -- One successful execution per source snapshot: a second plan for an
    -- already-pruned snapshot can never gain a second execution receipt.
    CONSTRAINT backup_prune_executions_snapshot_unique
        UNIQUE (snapshot_id),
    CONSTRAINT backup_prune_executions_state_value
        CHECK (state IN ('ASSEMBLING', 'COMMITTED')),
    CONSTRAINT backup_prune_executions_count_shape
        CHECK (
            released_pin_count >= 0
            AND distinct_object_count >= 0
            AND retained_by_other_reference_count >= 0
            AND gc_handoff_object_count >= 0
            AND retained_by_other_reference_count + gc_handoff_object_count
                = distinct_object_count
            AND released_pin_count >= distinct_object_count
        )
);

CREATE INDEX backup_prune_executions_owner_created_idx
    ON backup_prune_executions (owner_user_id, executed_at DESC, id DESC);

-- The receipt identity and every released count are bound at assembly time to
-- the locked PLANNED plan's immutable basis. A receipt can therefore never
-- record a different snapshot, backup set, or count from the plan it executes.
CREATE FUNCTION synveil_validate_backup_prune_execution_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    plan_backup_set_id UUID;
    plan_snapshot_id UUID;
    plan_state TEXT;
    plan_pin_release BIGINT;
    plan_distinct BIGINT;
    plan_retained BIGINT;
    plan_unreferenced BIGINT;
BEGIN
    IF NEW.state IS DISTINCT FROM 'ASSEMBLING' THEN
        RAISE EXCEPTION 'prune execution must begin in assembly state'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_execution_initial_state';
    END IF;

    SELECT backup_set_id, snapshot_id, state,
           planned_pin_release_count, distinct_retained_content_count,
           retained_after_release_count, would_become_unreferenced_count
      INTO plan_backup_set_id, plan_snapshot_id, plan_state,
           plan_pin_release, plan_distinct, plan_retained, plan_unreferenced
      FROM backup_prune_plans
     WHERE id = NEW.prune_plan_id
       AND owner_user_id = NEW.owner_user_id
     FOR SHARE;
    IF plan_backup_set_id IS NULL
       OR plan_state IS DISTINCT FROM 'PLANNED'
       OR plan_backup_set_id IS DISTINCT FROM NEW.backup_set_id
       OR plan_snapshot_id IS DISTINCT FROM NEW.snapshot_id
       OR NEW.released_pin_count IS DISTINCT FROM plan_pin_release
       OR NEW.distinct_object_count IS DISTINCT FROM plan_distinct
       OR NEW.retained_by_other_reference_count IS DISTINCT FROM plan_retained
       OR NEW.gc_handoff_object_count IS DISTINCT FROM plan_unreferenced THEN
        RAISE EXCEPTION 'prune execution identity does not match the planned basis'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_execution_identity';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_prune_executions_validate_identity
BEFORE INSERT ON backup_prune_executions
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_prune_execution_identity();

-- Once inserted, the receipt is immutable. Its only permitted mutation is the
-- transaction-local ASSEMBLING -> COMMITTED seal.
CREATE FUNCTION synveil_enforce_backup_prune_execution_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup prune executions cannot be deleted'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_execution_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.prune_plan_id IS NOT DISTINCT FROM NEW.prune_plan_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.snapshot_id IS NOT DISTINCT FROM NEW.snapshot_id
       AND OLD.released_pin_count IS NOT DISTINCT FROM NEW.released_pin_count
       AND OLD.distinct_object_count IS NOT DISTINCT FROM NEW.distinct_object_count
       AND OLD.retained_by_other_reference_count
           IS NOT DISTINCT FROM NEW.retained_by_other_reference_count
       AND OLD.gc_handoff_object_count IS NOT DISTINCT FROM NEW.gc_handoff_object_count
       AND OLD.executed_at IS NOT DISTINCT FROM NEW.executed_at
       AND (
           OLD.state IS NOT DISTINCT FROM NEW.state
           OR (OLD.state = 'ASSEMBLING' AND NEW.state = 'COMMITTED')
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup prune execution receipt is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_prune_execution_immutable';
END;
$$;

CREATE TRIGGER backup_prune_executions_immutable
BEFORE UPDATE OR DELETE ON backup_prune_executions
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_prune_execution_immutability();

CREATE FUNCTION synveil_require_backup_prune_execution_sealed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM backup_prune_executions
         WHERE id = NEW.id
           AND state = 'ASSEMBLING'
    ) THEN
        RAISE EXCEPTION 'backup prune execution assembly must be sealed before commit'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_execution_unsealed';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_prune_executions_must_be_sealed
AFTER INSERT OR UPDATE ON backup_prune_executions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_prune_execution_sealed();

-- Server-internal per-Object execution evidence. It is intentionally not
-- FK-bound to objects: a future normal GC lifecycle must be able to remove an
-- Object after a separately authorized release without destroying immutable
-- execution evidence. These rows are never exposed through the public backup
-- domain boundary.
CREATE TABLE backup_prune_execution_object_results (
    execution_id UUID NOT NULL,
    plan_id UUID NOT NULL,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    target_snapshot_pin_count BIGINT NOT NULL,
    post_release_live_file_version_count BIGINT NOT NULL,
    post_release_other_snapshot_pin_count BIGINT NOT NULL,
    post_release_authoritative_reference_count BIGINT NOT NULL,
    gc_candidate_created_or_reused BOOLEAN NOT NULL,
    gc_candidate_id UUID,
    gc_candidate_generation NUMERIC,
    CONSTRAINT backup_prune_execution_object_results_execution_fk
        FOREIGN KEY (execution_id) REFERENCES backup_prune_executions (id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_prune_execution_object_results_plan_fk
        FOREIGN KEY (plan_id) REFERENCES backup_prune_plans (id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_execution_object_results_target_pin_positive
        CHECK (target_snapshot_pin_count > 0),
    CONSTRAINT backup_prune_execution_object_results_count_shape
        CHECK (
            post_release_live_file_version_count >= 0
            AND post_release_other_snapshot_pin_count >= 0
            AND post_release_authoritative_reference_count >= 0
            AND post_release_authoritative_reference_count
                = post_release_live_file_version_count
                + post_release_other_snapshot_pin_count
        ),
    CONSTRAINT backup_prune_execution_object_results_candidate_shape
        CHECK (
            (gc_candidate_created_or_reused = FALSE
             AND gc_candidate_id IS NULL
             AND gc_candidate_generation IS NULL)
            OR (gc_candidate_created_or_reused = TRUE
                AND gc_candidate_id IS NOT NULL
                AND gc_candidate_generation IS NOT NULL)
        ),
    CONSTRAINT backup_prune_execution_object_results_candidate_generation_u64
        CHECK (
            gc_candidate_generation IS NULL
            OR (
                gc_candidate_generation >= 0
                AND gc_candidate_generation = trunc(gc_candidate_generation)
                AND gc_candidate_generation <= 18446744073709551615::NUMERIC
            )
        ),
    PRIMARY KEY (execution_id, object_id, object_dedup_domain_id)
);

CREATE FUNCTION synveil_validate_backup_prune_execution_object_result()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    execution_plan_id UUID;
    execution_state TEXT;
BEGIN
    SELECT prune_plan_id, state
      INTO execution_plan_id, execution_state
      FROM backup_prune_executions
     WHERE id = NEW.execution_id
     FOR SHARE;
    IF execution_plan_id IS NULL
       OR execution_state IS DISTINCT FROM 'ASSEMBLING'
       OR execution_plan_id IS DISTINCT FROM NEW.plan_id THEN
        RAISE EXCEPTION 'prune execution evidence is outside its assembly transaction'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_execution_object_result_identity';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_prune_execution_object_results_validate
BEFORE INSERT ON backup_prune_execution_object_results
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_prune_execution_object_result();

CREATE FUNCTION synveil_reject_backup_prune_execution_object_result_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup prune execution object results are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_prune_execution_object_result_immutable';
END;
$$;

CREATE TRIGGER backup_prune_execution_object_results_immutable
BEFORE UPDATE OR DELETE ON backup_prune_execution_object_results
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_prune_execution_object_result_mutation();

-- A plan cannot become EXECUTED without the committed canonical receipt. This
-- is deferred because the service seals the execution and the plan together.
CREATE FUNCTION synveil_require_backup_prune_plan_execution_receipt()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'EXECUTED'
       AND NOT EXISTS (
           SELECT 1
             FROM backup_prune_executions AS execution
            WHERE execution.prune_plan_id = NEW.id
              AND execution.owner_user_id = NEW.owner_user_id
              AND execution.snapshot_id = NEW.snapshot_id
              AND execution.state = 'COMMITTED'
       ) THEN
        RAISE EXCEPTION 'executed prune plan requires a committed execution receipt'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_execution_receipt';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_prune_plans_require_execution_receipt
AFTER INSERT OR UPDATE ON backup_prune_plans
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_prune_plan_execution_receipt();

-- Prompt 42B retention immutability: extend the pin DELETE gate so the ONLY
-- authorized release path is one that proves a currently-PLANNED plan owns the
-- exact source snapshot AND a transaction-local authorization token issued by
-- the narrow SECURITY DEFINER release function below. Any other DELETE against
-- an existing snapshot row remains rejected exactly as before.
CREATE OR REPLACE FUNCTION synveil_reject_direct_backup_pin_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_authorized_plan UUID;
    v_plan_state TEXT;
BEGIN
    IF EXISTS (
        SELECT 1
          FROM backup_snapshots
         WHERE id = OLD.snapshot_id
    ) THEN
        v_authorized_plan :=
            NULLIF(current_setting('synveil.active_prune_execution_plan', true), '')::UUID;
        IF v_authorized_plan IS NULL THEN
            RAISE EXCEPTION 'backup snapshot content pins may only be released by snapshot pruning'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_snapshot_content_pin_delete_forbidden';
        END IF;
        SELECT state
          INTO v_plan_state
          FROM backup_prune_plans
         WHERE id = v_authorized_plan
           AND snapshot_id = OLD.snapshot_id;
        IF v_plan_state IS DISTINCT FROM 'PLANNED' THEN
            RAISE EXCEPTION 'backup snapshot content pin release authorization is invalid'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_snapshot_content_pin_delete_forbidden';
        END IF;
    END IF;
    RETURN OLD;
END;
$$;

-- The single narrow authorized release protocol. It issues a transaction-local
-- authorization token, verifies one locked/current PLANNED plan owns the exact
-- source snapshot, verifies the source snapshot is EXPIRED, deletes every
-- target-snapshot pin once, and returns the released count. The token is
-- cleared before the function returns so no later statement in the same
-- transaction can reuse it. SECURITY DEFINER keeps ordinary application SQL
-- from ever issuing the token directly; only this function may.
CREATE FUNCTION synveil_authorized_prune_pin_release(
    p_prune_plan_id UUID,
    p_snapshot_id UUID
) RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    v_plan_state TEXT;
    v_snapshot_state TEXT;
    v_released BIGINT;
BEGIN
    SELECT state
      INTO v_plan_state
      FROM backup_prune_plans
     WHERE id = p_prune_plan_id;
    IF v_plan_state IS DISTINCT FROM 'PLANNED' THEN
        RAISE EXCEPTION 'prune pin release requires a PLANNED plan'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_pin_release_requires_planned_plan';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM backup_prune_plans
         WHERE id = p_prune_plan_id
           AND snapshot_id = p_snapshot_id
    ) THEN
        RAISE EXCEPTION 'prune pin release target does not match the plan snapshot'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_pin_release_snapshot_mismatch';
    END IF;
    SELECT state
      INTO v_snapshot_state
      FROM backup_snapshots
     WHERE id = p_snapshot_id;
    IF v_snapshot_state IS DISTINCT FROM 'EXPIRED' THEN
        RAISE EXCEPTION 'prune pin release requires an EXPIRED source snapshot'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_pin_release_requires_expired_snapshot';
    END IF;

    PERFORM set_config('synveil.active_prune_execution_plan', p_prune_plan_id::TEXT, true);
    BEGIN
        DELETE FROM backup_snapshot_content_pins
         WHERE snapshot_id = p_snapshot_id;
        GET DIAGNOSTICS v_released = ROW_COUNT;
    EXCEPTION WHEN OTHERS THEN
        PERFORM set_config('synveil.active_prune_execution_plan', '', true);
        RAISE;
    END;
    PERFORM set_config('synveil.active_prune_execution_plan', '', true);
    RETURN v_released;
END;
$$;