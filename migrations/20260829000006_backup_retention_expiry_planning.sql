-- Durable, versioned snapshot-retention policy and non-destructive expiry
-- planning. This migration adds metadata evidence only. It never transitions a
-- snapshot to EXPIRED, releases backup content pins, prunes metadata, touches
-- Object/ObjectReplica lifecycle, mutates GC, or performs ObjectStore I/O.

CREATE TABLE backup_snapshot_retention_policy_revisions (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    revision_number BIGINT NOT NULL,
    operation_id TEXT NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    keep_latest_completed BIGINT NOT NULL,
    expire_after_seconds BIGINT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_snapshot_retention_policy_revisions_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_retention_policy_revisions_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_retention_policy_revisions_revision_positive
        CHECK (revision_number > 0),
    CONSTRAINT backup_snapshot_retention_policy_revisions_operation_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_snapshot_retention_policy_revisions_fingerprint_version
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_snapshot_retention_policy_revisions_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_snapshot_retention_policy_revisions_keep_positive
        CHECK (keep_latest_completed > 0),
    CONSTRAINT backup_snapshot_retention_policy_revisions_age_positive
        CHECK (expire_after_seconds > 0 AND expire_after_seconds <= 315576000000),
    CONSTRAINT backup_snapshot_retention_policy_revisions_set_revision_unique
        UNIQUE (backup_set_id, revision_number),
    CONSTRAINT backup_snapshot_retention_policy_revisions_owner_operation_unique
        UNIQUE (owner_user_id, operation_id),
    CONSTRAINT backup_snapshot_retention_policy_revisions_scope_unique
        UNIQUE (id, revision_number, backup_set_id, owner_user_id)
);

CREATE INDEX backup_snapshot_retention_policy_current_idx
    ON backup_snapshot_retention_policy_revisions
       (backup_set_id, revision_number DESC);

CREATE INDEX backup_snapshot_retention_policy_owner_created_idx
    ON backup_snapshot_retention_policy_revisions
       (owner_user_id, created_at DESC, id DESC);

-- Every insert participates in the same backup-set row fence used by snapshot
-- capture and expiry planning. This makes MAX+1 safe here: the trigger holds
-- the owned set row while checking that the supplied revision is exactly the
-- next committed revision. There is no global policy lock.
CREATE FUNCTION synveil_validate_backup_snapshot_retention_policy_revision()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_revision BIGINT;
BEGIN
    PERFORM 1
      FROM backup_sets
     WHERE id = NEW.backup_set_id
       AND owner_user_id = NEW.owner_user_id
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'retention policy backup set is not owner scoped'
            USING ERRCODE = '23503',
                  CONSTRAINT = 'backup_snapshot_retention_policy_set_scope';
    END IF;

    SELECT COALESCE(MAX(revision_number), 0) + 1
      INTO expected_revision
      FROM backup_snapshot_retention_policy_revisions
     WHERE backup_set_id = NEW.backup_set_id;
    IF NEW.revision_number IS DISTINCT FROM expected_revision THEN
        RAISE EXCEPTION 'retention policy revision is not the next revision'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_retention_policy_revision_order';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_retention_policy_revision_order
BEFORE INSERT ON backup_snapshot_retention_policy_revisions
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_snapshot_retention_policy_revision();

CREATE FUNCTION synveil_reject_backup_snapshot_retention_policy_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup snapshot retention policy revisions are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_retention_policy_revision_immutable';
END;
$$;

CREATE TRIGGER backup_snapshot_retention_policy_revisions_immutable
BEFORE UPDATE OR DELETE ON backup_snapshot_retention_policy_revisions
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_snapshot_retention_policy_mutation();

CREATE TABLE backup_snapshot_expiry_plans (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    policy_revision_id UUID NOT NULL,
    policy_revision_number BIGINT NOT NULL,
    operation_id TEXT NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    evaluated_at TIMESTAMPTZ(6) NOT NULL,
    cutoff_at TIMESTAMPTZ(6) NOT NULL,
    snapshot_basis_fingerprint_version SMALLINT NOT NULL,
    snapshot_basis_fingerprint BYTEA NOT NULL,
    evaluated_completed_snapshot_count BIGINT NOT NULL,
    expire_candidate_count BIGINT NOT NULL,
    keep_latest_count BIGINT NOT NULL,
    keep_recent_count BIGINT NOT NULL,
    blocked_active_restore_count BIGINT NOT NULL,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    stale_at TIMESTAMPTZ(6),
    CONSTRAINT backup_snapshot_expiry_plans_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_plans_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_plans_policy_scope_fk
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
    CONSTRAINT backup_snapshot_expiry_plans_operation_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_snapshot_expiry_plans_fingerprint_version
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_snapshot_expiry_plans_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_snapshot_expiry_plans_basis_version
        CHECK (snapshot_basis_fingerprint_version = 1),
    CONSTRAINT backup_snapshot_expiry_plans_basis_length
        CHECK (octet_length(snapshot_basis_fingerprint) = 32),
    CONSTRAINT backup_snapshot_expiry_plans_time_shape
        CHECK (cutoff_at < evaluated_at AND created_at >= evaluated_at),
    CONSTRAINT backup_snapshot_expiry_plans_count_shape
        CHECK (
            evaluated_completed_snapshot_count >= 0
            AND expire_candidate_count >= 0
            AND keep_latest_count >= 0
            AND keep_recent_count >= 0
            AND blocked_active_restore_count >= 0
            AND expire_candidate_count
                + keep_latest_count
                + keep_recent_count
                + blocked_active_restore_count
                = evaluated_completed_snapshot_count
        ),
    -- ASSEMBLING exists only within the authoritative creation transaction.
    CONSTRAINT backup_snapshot_expiry_plans_state_value
        CHECK (state IN ('ASSEMBLING', 'PLANNED', 'STALE')),
    CONSTRAINT backup_snapshot_expiry_plans_stale_shape
        CHECK (
            (state IN ('ASSEMBLING', 'PLANNED') AND stale_at IS NULL)
            OR (state = 'STALE' AND stale_at IS NOT NULL AND stale_at >= created_at)
        ),
    CONSTRAINT backup_snapshot_expiry_plans_owner_operation_unique
        UNIQUE (owner_user_id, operation_id)
);

CREATE INDEX backup_snapshot_expiry_plans_owner_created_idx
    ON backup_snapshot_expiry_plans (owner_user_id, created_at DESC, id DESC);

CREATE UNIQUE INDEX backup_snapshot_expiry_plans_one_active_set
    ON backup_snapshot_expiry_plans (backup_set_id)
    WHERE state IN ('ASSEMBLING', 'PLANNED');

CREATE FUNCTION synveil_require_backup_snapshot_expiry_plan_initial_assembly()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state IS DISTINCT FROM 'ASSEMBLING' OR NEW.stale_at IS NOT NULL THEN
        RAISE EXCEPTION 'backup snapshot expiry plan must begin in assembly state'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_initial_state';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_plans_require_initial_assembly
BEFORE INSERT ON backup_snapshot_expiry_plans
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_snapshot_expiry_plan_initial_assembly();

CREATE FUNCTION synveil_enforce_backup_snapshot_expiry_plan_immutability()
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
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup snapshot expiry plan provenance is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_expiry_plan_immutable';
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_plans_immutable
BEFORE UPDATE OR DELETE ON backup_snapshot_expiry_plans
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_snapshot_expiry_plan_immutability();

CREATE TABLE backup_snapshot_expiry_plan_entries (
    plan_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    committed_at TIMESTAMPTZ(6) NOT NULL,
    recency_rank BIGINT NOT NULL,
    decision TEXT NOT NULL,
    CONSTRAINT backup_snapshot_expiry_plan_entries_plan_fk
        FOREIGN KEY (plan_id) REFERENCES backup_snapshot_expiry_plans (id) ON DELETE RESTRICT,
    -- RESTRICT preserves expiry-plan audit evidence if a later lifecycle adds
    -- snapshot-row cleanup; no cascade may erase a historical decision.
    CONSTRAINT backup_snapshot_expiry_plan_entries_snapshot_fk
        FOREIGN KEY (snapshot_id) REFERENCES backup_snapshots (id) ON DELETE RESTRICT,
    CONSTRAINT backup_snapshot_expiry_plan_entries_rank_positive
        CHECK (recency_rank > 0),
    CONSTRAINT backup_snapshot_expiry_plan_entries_decision_value
        CHECK (decision IN (
            'KEEP_LATEST',
            'KEEP_RECENT',
            'BLOCKED_ACTIVE_RESTORE_PLAN',
            'EXPIRE'
        )),
    PRIMARY KEY (plan_id, recency_rank),
    CONSTRAINT backup_snapshot_expiry_plan_entries_snapshot_unique
        UNIQUE (plan_id, snapshot_id)
);

CREATE INDEX backup_snapshot_expiry_plan_entries_snapshot_idx
    ON backup_snapshot_expiry_plan_entries (snapshot_id, plan_id);

CREATE FUNCTION synveil_validate_backup_snapshot_expiry_plan_entry()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    plan_state TEXT;
    plan_backup_set_id UUID;
    plan_owner_user_id UUID;
    snapshot_backup_set_id UUID;
    snapshot_owner_user_id UUID;
    snapshot_state TEXT;
    snapshot_committed_at TIMESTAMPTZ;
BEGIN
    SELECT state, backup_set_id, owner_user_id
      INTO plan_state, plan_backup_set_id, plan_owner_user_id
      FROM backup_snapshot_expiry_plans
     WHERE id = NEW.plan_id
     FOR SHARE;
    IF plan_state IS DISTINCT FROM 'ASSEMBLING' THEN
        RAISE EXCEPTION 'backup snapshot expiry entry cannot be appended after sealing'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_entry_insert_forbidden';
    END IF;

    SELECT backup_set_id, owner_user_id, state, committed_at
      INTO snapshot_backup_set_id, snapshot_owner_user_id,
           snapshot_state, snapshot_committed_at
      FROM backup_snapshots
     WHERE id = NEW.snapshot_id
     FOR SHARE;
    IF snapshot_state IS DISTINCT FROM 'COMPLETED'
       OR snapshot_backup_set_id IS DISTINCT FROM plan_backup_set_id
       OR snapshot_owner_user_id IS DISTINCT FROM plan_owner_user_id
       OR snapshot_committed_at IS DISTINCT FROM NEW.committed_at THEN
        RAISE EXCEPTION 'backup snapshot expiry entry does not match completed cohort'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_expiry_plan_entry_snapshot_basis';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_plan_entries_validate_basis
BEFORE INSERT ON backup_snapshot_expiry_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_snapshot_expiry_plan_entry();

CREATE FUNCTION synveil_reject_backup_snapshot_expiry_plan_entry_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup snapshot expiry plan entries are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_expiry_plan_entry_immutable';
END;
$$;

CREATE TRIGGER backup_snapshot_expiry_plan_entries_immutable
BEFORE UPDATE OR DELETE ON backup_snapshot_expiry_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_snapshot_expiry_plan_entry_mutation();

-- A deferred seal rejects partial entry sets and independently checks the
-- mandatory ranking/decision precedence against the original evaluated_at
-- cutoff. It performs reads only against snapshots, policy, and restore plans.
CREATE FUNCTION synveil_require_backup_snapshot_expiry_plan_sealed()
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
       AND current_plan_state IS DISTINCT FROM 'STALE' THEN
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

CREATE CONSTRAINT TRIGGER backup_snapshot_expiry_plans_must_be_sealed
AFTER INSERT OR UPDATE ON backup_snapshot_expiry_plans
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_snapshot_expiry_plan_sealed();
