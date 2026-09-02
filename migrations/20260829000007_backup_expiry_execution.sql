-- Atomic snapshot-expiry execution. This migration extends the Plan 47
-- PLANNED/STALE lifecycle with the terminal EXECUTED state, persists one
-- canonical immutable execution receipt and per-entry immutable evidence, and
-- authorizes ONLY the plan-bound COMPLETED -> EXPIRED lifecycle transition.
--
-- The service assembles the execution and its per-entry evidence in one
-- PostgreSQL transaction (mirroring the restore/prune ASSEMBLING pattern).
-- ASSEMBLING is transaction-local: deferred sealing checks prevent an
-- incomplete execution, an EXECUTED plan without its canonical receipt, or a
-- snapshot expiry without the matching committed evidence from becoming
-- durable. Execution releases no retention pin, deletes nothing, prunes
-- nothing, mutates no GC/Object/ObjectStore state, and changes logical
-- snapshot lifecycle only.

-- Extend expiry-plan lifecycle to include the terminal EXECUTED state.
ALTER TABLE backup_snapshot_expiry_plans
    DROP CONSTRAINT backup_snapshot_expiry_plans_state_value,
    ADD CONSTRAINT backup_snapshot_expiry_plans_state_value
        CHECK (state IN ('ASSEMBLING', 'PLANNED', 'STALE', 'EXECUTED')),
    DROP CONSTRAINT backup_snapshot_expiry_plans_stale_shape,
    ADD CONSTRAINT backup_snapshot_expiry_plans_stale_shape
        CHECK (
            (state IN ('ASSEMBLING', 'PLANNED', 'EXECUTED') AND stale_at IS NULL)
            OR (state = 'STALE' AND stale_at IS NOT NULL AND stale_at >= created_at)
        );

-- The execution receipt FK references (id, owner_user_id), matching the
-- prune-execution pattern; ensure the parent key is unique.
ALTER TABLE backup_snapshot_expiry_plans
    ADD CONSTRAINT backup_snapshot_expiry_plans_id_owner_unique
    UNIQUE (id, owner_user_id);

-- Plan 47's immutability trigger only accepted ASSEMBLING -> PLANNED and
-- PLANNED -> STALE. Extend it in place so PLANNED -> EXECUTED is also allowed.
-- Provenance fields remain immutable; the only mutable columns are state and
-- stale_at through the allowed transitions.
CREATE OR REPLACE FUNCTION synveil_enforce_backup_snapshot_expiry_plan_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup snapshot expiry plans cannot be deleted'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.policy_revision_id IS NOT DISTINCT FROM NEW.policy_revision_id
       AND OLD.policy_revision_number IS NOT DISTINCT FROM NEW.policy_revision_number
       AND OLD.operation_id IS NOT DISTINCT FROM NEW.operation_id
       AND OLD.fingerprint_version IS NOT DISTINCT FROM NEW.fingerprint_version
       AND OLD.request_fingerprint IS NOT DISTINCT FROM NEW.request_fingerprint
       AND OLD.evaluated_at IS NOT DISTINCT FROM NEW.evaluated_at
       AND OLD.cutoff_at IS NOT DISTINCT FROM NEW.cutoff_at
       AND OLD.snapshot_basis_fingerprint_version
           IS NOT DISTINCT FROM NEW.snapshot_basis_fingerprint_version
       AND OLD.snapshot_basis_fingerprint IS NOT DISTINCT FROM NEW.snapshot_basis_fingerprint
       AND OLD.evaluated_completed_snapshot_count
           IS NOT DISTINCT FROM NEW.evaluated_completed_snapshot_count
       AND OLD.expire_candidate_count IS NOT DISTINCT FROM NEW.expire_candidate_count
       AND OLD.keep_latest_count IS NOT DISTINCT FROM NEW.keep_latest_count
       AND OLD.keep_recent_count IS NOT DISTINCT FROM NEW.keep_recent_count
       AND OLD.blocked_active_restore_count
           IS NOT DISTINCT FROM NEW.blocked_active_restore_count
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

    RAISE EXCEPTION 'backup snapshot expiry plan provenance is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_expiry_plan_immutable';
END;
$$;

-- Canonical immutable expiry execution receipt. Only the service's
-- transaction-local ASSEMBLING row may be inserted; the deferred seal trigger
-- rejects an incomplete execution at commit, so a committed row is always a
-- canonical COMMITTED receipt.
CREATE TABLE backup_snapshot_expiry_executions (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    expiry_plan_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    policy_revision_id UUID NOT NULL,
    evaluated_at TIMESTAMPTZ(6) NOT NULL,
    evaluated_snapshot_count BIGINT NOT NULL,
    expired_snapshot_count BIGINT NOT NULL,
    unchanged_snapshot_count BIGINT NOT NULL,
    state TEXT NOT NULL,
    executed_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_snapshot_expiry_executions_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_executions_plan_owner_fk
        FOREIGN KEY (expiry_plan_id, owner_user_id)
        REFERENCES backup_snapshot_expiry_plans (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_executions_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_executions_policy_fk
        FOREIGN KEY (policy_revision_id) REFERENCES backup_snapshot_retention_policy_revisions (id)
        ON DELETE RESTRICT,
    -- One successful execution per expiry plan: repeat semantic executions are
    -- impossible and replay returns the same canonical receipt.
    CONSTRAINT backup_snapshot_expiry_executions_plan_unique
        UNIQUE (expiry_plan_id),
    CONSTRAINT backup_snapshot_expiry_executions_state_value
        CHECK (state IN ('ASSEMBLING', 'COMMITTED')),
    CONSTRAINT backup_snapshot_expiry_executions_count_shape
        CHECK (
            evaluated_snapshot_count >= 0
            AND expired_snapshot_count >= 0
            AND unchanged_snapshot_count >= 0
            AND expired_snapshot_count + unchanged_snapshot_count
                = evaluated_snapshot_count
        ),
    CONSTRAINT backup_snapshot_expiry_executions_time_shape
        CHECK (executed_at >= evaluated_at)
);

CREATE INDEX backup_snapshot_expiry_executions_owner_created_idx
    ON backup_snapshot_expiry_executions (owner_user_id, executed_at DESC, id DESC);

-- The receipt identity and every count are bound at assembly time to the
-- locked PLANNED plan's immutable basis. A receipt can therefore never record a
-- different backup set, policy revision, evaluation time, or count from the
-- plan it executes.
CREATE FUNCTION synveil_validate_backup_snapshot_expiry_execution_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    plan_backup_set_id UUID;
    plan_owner_user_id UUID;
    plan_policy_revision_id UUID;
    plan_state TEXT;
    plan_evaluated_at TIMESTAMPTZ;
    plan_evaluated_count BIGINT;
    plan_expire_count BIGINT;
BEGIN
    IF NEW.state IS DISTINCT FROM 'ASSEMBLING' THEN
        RAISE EXCEPTION 'expiry execution must begin in assembly state'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_initial_state';
    END IF;

    SELECT backup_set_id, owner_user_id, policy_revision_id, state,
           evaluated_at, evaluated_completed_snapshot_count, expire_candidate_count
      INTO plan_backup_set_id, plan_owner_user_id, plan_policy_revision_id,
           plan_state, plan_evaluated_at, plan_evaluated_count, plan_expire_count
      FROM backup_snapshot_expiry_plans
     WHERE id = NEW.expiry_plan_id
       AND owner_user_id = NEW.owner_user_id
     FOR SHARE;
    IF plan_backup_set_id IS NULL
       OR plan_state IS DISTINCT FROM 'PLANNED'
       OR plan_owner_user_id IS DISTINCT FROM NEW.owner_user_id
       OR plan_backup_set_id IS DISTINCT FROM NEW.backup_set_id
       OR plan_policy_revision_id IS DISTINCT FROM NEW.policy_revision_id
       OR plan_evaluated_at IS DISTINCT FROM NEW.evaluated_at
       OR NEW.evaluated_snapshot_count IS DISTINCT FROM plan_evaluated_count
       OR NEW.expired_snapshot_count IS DISTINCT FROM plan_expire_count
       OR NEW.unchanged_snapshot_count
           IS DISTINCT FROM (plan_evaluated_count - plan_expire_count) THEN
        RAISE EXCEPTION 'expiry execution identity does not match the planned basis'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_identity';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_executions_validate_identity
BEFORE INSERT ON backup_snapshot_expiry_executions
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_snapshot_expiry_execution_identity();

-- Once inserted, the receipt is immutable. Its only permitted mutation is the
-- transaction-local ASSEMBLING -> COMMITTED seal.
CREATE FUNCTION synveil_enforce_backup_snapshot_expiry_execution_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup snapshot expiry executions cannot be deleted'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.expiry_plan_id IS NOT DISTINCT FROM NEW.expiry_plan_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.policy_revision_id IS NOT DISTINCT FROM NEW.policy_revision_id
       AND OLD.evaluated_at IS NOT DISTINCT FROM NEW.evaluated_at
       AND OLD.evaluated_snapshot_count IS NOT DISTINCT FROM NEW.evaluated_snapshot_count
       AND OLD.expired_snapshot_count IS NOT DISTINCT FROM NEW.expired_snapshot_count
       AND OLD.unchanged_snapshot_count IS NOT DISTINCT FROM NEW.unchanged_snapshot_count
       AND OLD.executed_at IS NOT DISTINCT FROM NEW.executed_at
       AND (
           OLD.state IS NOT DISTINCT FROM NEW.state
           OR (OLD.state = 'ASSEMBLING' AND NEW.state = 'COMMITTED')
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup snapshot expiry execution receipt is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_expiry_execution_immutable';
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_executions_immutable
BEFORE UPDATE OR DELETE ON backup_snapshot_expiry_executions
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_snapshot_expiry_execution_immutability();

CREATE FUNCTION synveil_require_backup_snapshot_expiry_execution_sealed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM backup_snapshot_expiry_executions
         WHERE id = NEW.id
           AND state = 'ASSEMBLING'
    ) THEN
        RAISE EXCEPTION 'backup snapshot expiry execution assembly must be sealed before commit'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_unsealed';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_snapshot_expiry_executions_must_be_sealed
AFTER INSERT OR UPDATE ON backup_snapshot_expiry_executions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_snapshot_expiry_execution_sealed();

-- Immutable per-plan-entry execution evidence. Every persisted Plan 47 decision
-- entry yields one evidence row recording whether its snapshot lifecycle
-- transitioned (COMPLETED -> EXPIRED) this execution. No Object/Replica/storage
-- identity appears.
CREATE TABLE backup_snapshot_expiry_execution_entries (
    execution_id UUID NOT NULL,
    expiry_plan_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    original_decision TEXT NOT NULL,
    transitioned BOOLEAN NOT NULL,
    CONSTRAINT backup_snapshot_expiry_execution_entries_execution_fk
        FOREIGN KEY (execution_id) REFERENCES backup_snapshot_expiry_executions (id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_execution_entries_plan_fk
        FOREIGN KEY (expiry_plan_id) REFERENCES backup_snapshot_expiry_plans (id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_execution_entries_snapshot_fk
        FOREIGN KEY (snapshot_id) REFERENCES backup_snapshots (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_execution_entries_decision_value
        CHECK (original_decision IN (
            'KEEP_LATEST',
            'KEEP_RECENT',
            'BLOCKED_ACTIVE_RESTORE_PLAN',
            'EXPIRE'
        )),
    CONSTRAINT backup_snapshot_expiry_execution_entries_transition_shape
        CHECK (
            (original_decision = 'EXPIRE' AND transitioned = TRUE)
            OR (original_decision <> 'EXPIRE' AND transitioned = FALSE)
        ),
    PRIMARY KEY (execution_id, snapshot_id),
    CONSTRAINT backup_snapshot_expiry_execution_entries_snapshot_unique
        UNIQUE (execution_id, snapshot_id)
);

CREATE INDEX backup_snapshot_expiry_execution_entries_snapshot_idx
    ON backup_snapshot_expiry_execution_entries (snapshot_id, execution_id);

-- Evidence may only be inserted while its execution is still ASSEMBLING, and
-- each evidence snapshot must correspond exactly to one immutable plan entry.
CREATE FUNCTION synveil_validate_backup_snapshot_expiry_execution_entry()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    execution_plan_id UUID;
    execution_state TEXT;
    plan_decision TEXT;
BEGIN
    SELECT expiry_plan_id, state
      INTO execution_plan_id, execution_state
      FROM backup_snapshot_expiry_executions
     WHERE id = NEW.execution_id
     FOR SHARE;
    IF execution_plan_id IS NULL
       OR execution_state IS DISTINCT FROM 'ASSEMBLING'
       OR execution_plan_id IS DISTINCT FROM NEW.expiry_plan_id THEN
        RAISE EXCEPTION 'expiry execution evidence is outside its assembly transaction'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_entry_identity';
    END IF;

    SELECT decision
      INTO plan_decision
      FROM backup_snapshot_expiry_plan_entries
     WHERE plan_id = NEW.expiry_plan_id
       AND snapshot_id = NEW.snapshot_id;
    IF plan_decision IS NULL
       OR plan_decision IS DISTINCT FROM NEW.original_decision THEN
        RAISE EXCEPTION 'expiry execution evidence does not match a plan entry'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_entry_basis';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_execution_entries_validate
BEFORE INSERT ON backup_snapshot_expiry_execution_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_snapshot_expiry_execution_entry();

CREATE FUNCTION synveil_reject_backup_snapshot_expiry_execution_entry_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup snapshot expiry execution entries are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_expiry_execution_entry_immutable';
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_execution_entries_immutable
BEFORE UPDATE OR DELETE ON backup_snapshot_expiry_execution_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_snapshot_expiry_execution_entry_mutation();

-- Count integrity: the evidence set must cover exactly every plan entry and
-- exactly the EXPIRE transitions recorded on the receipt. This is deferred
-- because the service inserts the receipt and evidence together.
CREATE FUNCTION synveil_validate_backup_snapshot_expiry_execution_entry_counts()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    execution_state TEXT;
    execution_evaluated_count BIGINT;
    execution_expired_count BIGINT;
    plan_entry_count BIGINT;
    evidence_count BIGINT;
    transitioned_count BIGINT;
BEGIN
    SELECT state, evaluated_snapshot_count, expired_snapshot_count
      INTO execution_state, execution_evaluated_count, execution_expired_count
      FROM backup_snapshot_expiry_executions
     WHERE id = NEW.id;
    IF execution_state IS DISTINCT FROM 'ASSEMBLING' THEN
        RETURN NULL;
    END IF;

    SELECT count(*) INTO plan_entry_count
      FROM backup_snapshot_expiry_plan_entries
     WHERE plan_id = NEW.expiry_plan_id;
    SELECT count(*),
           count(*) FILTER (WHERE transitioned = TRUE)
      INTO evidence_count, transitioned_count
      FROM backup_snapshot_expiry_execution_entries
     WHERE execution_id = NEW.id;

    IF evidence_count IS DISTINCT FROM plan_entry_count
       OR evidence_count IS DISTINCT FROM execution_evaluated_count
       OR transitioned_count IS DISTINCT FROM execution_expired_count THEN
        RAISE EXCEPTION 'backup snapshot expiry execution evidence counts are incomplete'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_expiry_execution_entry_counts';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_snapshot_expiry_execution_entries_must_be_complete
AFTER INSERT OR UPDATE ON backup_snapshot_expiry_executions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_snapshot_expiry_execution_entry_counts();

-- A plan cannot become EXECUTED without the committed canonical receipt. This
-- is deferred because the service seals the execution and the plan together.
CREATE FUNCTION synveil_require_backup_snapshot_expiry_plan_execution_receipt()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'EXECUTED'
       AND NOT EXISTS (
           SELECT 1
             FROM backup_snapshot_expiry_executions AS execution
            WHERE execution.expiry_plan_id = NEW.id
              AND execution.owner_user_id = NEW.owner_user_id
              AND execution.state = 'COMMITTED'
       ) THEN
        RAISE EXCEPTION 'executed expiry plan requires a committed execution receipt'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_execution_receipt';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_snapshot_expiry_plans_require_execution_receipt
AFTER INSERT OR UPDATE ON backup_snapshot_expiry_plans
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_snapshot_expiry_plan_execution_receipt();

-- The single narrow authorized snapshot-expiry transition. It verifies the
-- exact currently-PLANNED owner plan owns an EXECUTE entry for the target
-- snapshot, verifies the snapshot is COMPLETED and owner-scoped to the plan,
-- and transitions it to EXPIRED atomically. It returns the affected row count.
-- SECURITY DEFINER keeps ordinary application SQL from issuing an unchecked
-- arbitrary COMPLETED -> EXPIRED transition; only this function may.
CREATE FUNCTION synveil_authorized_snapshot_expiry_transition(
    p_expiry_plan_id UUID,
    p_snapshot_id UUID
) RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    v_plan_state TEXT;
    v_plan_snapshot_decision TEXT;
    v_snapshot_state TEXT;
    v_snapshot_backup_set_id UUID;
    v_plan_backup_set_id UUID;
    v_snapshot_owner_user_id UUID;
    v_plan_owner_user_id UUID;
    v_transitioned BIGINT;
BEGIN
    SELECT state, owner_user_id, backup_set_id
      INTO v_plan_state, v_plan_owner_user_id, v_plan_backup_set_id
      FROM backup_snapshot_expiry_plans
     WHERE id = p_expiry_plan_id;
    IF v_plan_state IS DISTINCT FROM 'PLANNED' THEN
        RAISE EXCEPTION 'snapshot expiry transition requires a PLANNED plan'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_transition_requires_planned_plan';
    END IF;

    SELECT decision
      INTO v_plan_snapshot_decision
      FROM backup_snapshot_expiry_plan_entries
     WHERE plan_id = p_expiry_plan_id
       AND snapshot_id = p_snapshot_id;
    IF v_plan_snapshot_decision IS DISTINCT FROM 'EXPIRE' THEN
        RAISE EXCEPTION 'snapshot expiry transition requires an EXPIRE plan entry'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_transition_requires_expire_entry';
    END IF;

    SELECT state, backup_set_id, owner_user_id
      INTO v_snapshot_state, v_snapshot_backup_set_id, v_snapshot_owner_user_id
      FROM backup_snapshots
     WHERE id = p_snapshot_id;
    IF v_snapshot_state IS DISTINCT FROM 'COMPLETED'
       OR v_snapshot_backup_set_id IS DISTINCT FROM v_plan_backup_set_id
       OR v_snapshot_owner_user_id IS DISTINCT FROM v_plan_owner_user_id THEN
        RAISE EXCEPTION 'snapshot expiry transition target is not a COMPLETED owner-scoped snapshot'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_transition_target_invalid';
    END IF;

    UPDATE backup_snapshots
       SET state = 'EXPIRED', expired_at = clock_timestamp()
     WHERE id = p_snapshot_id
       AND state = 'COMPLETED';
    GET DIAGNOSTICS v_transitioned = ROW_COUNT;
    RETURN v_transitioned;
END;
$$;

-- Plan 47's deferred seal trigger only accepted ASSEMBLING/PLANNED/STALE as
-- terminal plan states. Extend it in place (via CREATE OR REPLACE) so an
-- EXECUTED plan with its committed execution receipt can commit. For EXECUTED
-- rows it returns before the pre-execution COMPLETED-cohort rechecks, because
-- those snapshots are legitimately EXPIRED after execution.
CREATE OR REPLACE FUNCTION synveil_require_backup_snapshot_expiry_plan_sealed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    current_plan_state TEXT;
    entry_count BIGINT;
    distinct_snapshot_count BIGINT;
    distinct_rank_count BIGINT;
    minimum_rank BIGINT;
    maximum_rank BIGINT;
    keep_latest_entry_count BIGINT;
    keep_recent_entry_count BIGINT;
    blocked_entry_count BIGINT;
    expire_entry_count BIGINT;
    current_completed_count BIGINT;
    invalid_decision_count BIGINT;
    current_policy_revision_id UUID;
BEGIN
    SELECT state
      INTO current_plan_state
      FROM backup_snapshot_expiry_plans
     WHERE id = NEW.id;
    IF current_plan_state = 'ASSEMBLING' THEN
        RAISE EXCEPTION 'backup snapshot expiry plan assembly must be sealed before commit'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_unsealed';
    END IF;
    IF current_plan_state IS DISTINCT FROM 'PLANNED'
       AND current_plan_state IS DISTINCT FROM 'STALE'
       AND current_plan_state IS DISTINCT FROM 'EXECUTED' THEN
        RAISE EXCEPTION 'backup snapshot expiry plan final state is invalid'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_final_state';
    END IF;
    IF current_plan_state <> 'PLANNED' THEN
        RETURN NULL;
    END IF;

    SELECT id
      INTO current_policy_revision_id
      FROM backup_snapshot_retention_policy_revisions
     WHERE backup_set_id = NEW.backup_set_id
     ORDER BY revision_number DESC
     LIMIT 1;
    IF current_policy_revision_id IS DISTINCT FROM NEW.policy_revision_id THEN
        RAISE EXCEPTION 'backup snapshot expiry plan policy is not current at seal'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_policy_current';
    END IF;

    SELECT count(*),
           count(DISTINCT snapshot_id),
           count(DISTINCT recency_rank),
           min(recency_rank),
           max(recency_rank),
           count(*) FILTER (WHERE decision = 'KEEP_LATEST'),
           count(*) FILTER (WHERE decision = 'KEEP_RECENT'),
           count(*) FILTER (WHERE decision = 'BLOCKED_ACTIVE_RESTORE_PLAN'),
           count(*) FILTER (WHERE decision = 'EXPIRE')
      INTO entry_count, distinct_snapshot_count, distinct_rank_count,
           minimum_rank, maximum_rank, keep_latest_entry_count,
           keep_recent_entry_count, blocked_entry_count, expire_entry_count
      FROM backup_snapshot_expiry_plan_entries
     WHERE plan_id = NEW.id;

    SELECT count(*)
      INTO current_completed_count
      FROM backup_snapshots
     WHERE backup_set_id = NEW.backup_set_id
       AND owner_user_id = NEW.owner_user_id
       AND state = 'COMPLETED';

    IF entry_count IS DISTINCT FROM NEW.evaluated_completed_snapshot_count
       OR distinct_snapshot_count IS DISTINCT FROM entry_count
       OR distinct_rank_count IS DISTINCT FROM entry_count
       OR current_completed_count IS DISTINCT FROM entry_count
       OR (entry_count = 0 AND (minimum_rank IS NOT NULL OR maximum_rank IS NOT NULL))
       OR (entry_count > 0 AND (minimum_rank <> 1 OR maximum_rank <> entry_count))
       OR keep_latest_entry_count IS DISTINCT FROM NEW.keep_latest_count
       OR keep_recent_entry_count IS DISTINCT FROM NEW.keep_recent_count
       OR blocked_entry_count IS DISTINCT FROM NEW.blocked_active_restore_count
       OR expire_entry_count IS DISTINCT FROM NEW.expire_candidate_count THEN
        RAISE EXCEPTION 'backup snapshot expiry plan entries or counts are incomplete'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_entry_counts';
    END IF;

    WITH ranked AS (
        SELECT snapshot.id,
               snapshot.committed_at,
               row_number() OVER (
                   ORDER BY snapshot.committed_at DESC, snapshot.id DESC
               ) AS recency_rank,
               EXISTS (
                   SELECT 1
                     FROM backup_restore_plans AS restore_plan
                    WHERE restore_plan.owner_user_id = NEW.owner_user_id
                      AND restore_plan.backup_set_id = NEW.backup_set_id
                      AND restore_plan.snapshot_id = snapshot.id
                      AND restore_plan.state = 'PLANNED'
               ) AS active_restore_blocker
          FROM backup_snapshots AS snapshot
         WHERE snapshot.backup_set_id = NEW.backup_set_id
           AND snapshot.owner_user_id = NEW.owner_user_id
           AND snapshot.state = 'COMPLETED'
    ), expected AS (
        SELECT ranked.id,
               ranked.committed_at,
               ranked.recency_rank,
               CASE
                   WHEN ranked.recency_rank <= policy.keep_latest_completed
                       THEN 'KEEP_LATEST'
                   WHEN ranked.committed_at > NEW.cutoff_at
                       THEN 'KEEP_RECENT'
                   WHEN ranked.active_restore_blocker
                       THEN 'BLOCKED_ACTIVE_RESTORE_PLAN'
                   ELSE 'EXPIRE'
               END AS decision
          FROM ranked
          CROSS JOIN backup_snapshot_retention_policy_revisions AS policy
         WHERE policy.id = NEW.policy_revision_id
    )
    SELECT count(*)
      INTO invalid_decision_count
      FROM expected
      LEFT JOIN backup_snapshot_expiry_plan_entries AS entry
        ON entry.plan_id = NEW.id
       AND entry.snapshot_id = expected.id
     WHERE entry.snapshot_id IS NULL
        OR entry.committed_at IS DISTINCT FROM expected.committed_at
        OR entry.recency_rank IS DISTINCT FROM expected.recency_rank
        OR entry.decision IS DISTINCT FROM expected.decision;
    IF invalid_decision_count <> 0 THEN
        RAISE EXCEPTION 'backup snapshot expiry decisions do not match policy precedence'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_decision_basis';
    END IF;
    RETURN NULL;
END;
$$;
