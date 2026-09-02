-- Crash-safe physical Object GC execution and canonical reference fencing.
--
-- PostgreSQL remains the authority for whether an Object may acquire a new
-- durable reference. External ObjectStore I/O is deliberately not performed
-- inside these transactions; durable operation/action rows fence each exact
-- side effect and retain its recovery evidence.

ALTER TABLE objects
    ADD COLUMN lifecycle_state TEXT NOT NULL DEFAULT 'AVAILABLE';

ALTER TABLE objects
    ADD CONSTRAINT objects_lifecycle_state_value
        CHECK (lifecycle_state IN ('AVAILABLE', 'GC_DELETING'));

-- This is the smallest canonical hold boundary needed before byte deletion.
-- No backup/share/sync producer exists yet. Any future durable reference class
-- must first register an active row here (or supersede this contract) before it
-- may coexist with physical GC.
CREATE TABLE object_gc_holds (
    hold_id UUID PRIMARY KEY,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    hold_class TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    expires_at TIMESTAMPTZ(6),
    released_at TIMESTAMPTZ(6),
    CONSTRAINT object_gc_holds_object_fk
        FOREIGN KEY (object_id, object_dedup_domain_id)
        REFERENCES objects (id, dedup_domain_id) ON DELETE CASCADE,
    CONSTRAINT object_gc_holds_class_shape CHECK (
        octet_length(hold_class) BETWEEN 1 AND 64
        AND hold_class ~ '^[A-Z][A-Z0-9_]*$'
    ),
    CONSTRAINT object_gc_holds_time_shape CHECK (
        (expires_at IS NULL OR expires_at > created_at)
        AND (released_at IS NULL OR released_at >= created_at)
    )
);

CREATE INDEX object_gc_holds_active_object_idx
    ON object_gc_holds (object_id, object_dedup_domain_id, expires_at)
    WHERE released_at IS NULL;

CREATE TABLE object_gc_operations (
    operation_id UUID PRIMARY KEY,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    candidate_generation NUMERIC NOT NULL,
    lease_id UUID NOT NULL,
    lease_generation NUMERIC NOT NULL,
    state TEXT NOT NULL,
    started_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    completed_at TIMESTAMPTZ(6),
    last_error_code TEXT,
    CONSTRAINT object_gc_operations_identity_unique
        UNIQUE (object_id, object_dedup_domain_id),
    CONSTRAINT object_gc_operations_candidate_generation_u64 CHECK (
        candidate_generation >= 0
        AND candidate_generation = trunc(candidate_generation)
        AND candidate_generation <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT object_gc_operations_lease_generation_u64 CHECK (
        lease_generation >= 0
        AND lease_generation = trunc(lease_generation)
        AND lease_generation <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT object_gc_operations_state_value
        CHECK (state IN ('ACTIVE', 'RECOVERY_REQUIRED', 'COMPLETED')),
    CONSTRAINT object_gc_operations_time_shape CHECK (
        updated_at >= started_at
        AND (
            (state <> 'COMPLETED' AND completed_at IS NULL)
            OR (state = 'COMPLETED' AND completed_at IS NOT NULL
                AND completed_at >= started_at)
        )
    ),
    CONSTRAINT object_gc_operations_error_code_shape CHECK (
        last_error_code IS NULL
        OR octet_length(last_error_code) BETWEEN 1 AND 128
    )
);

CREATE TABLE object_gc_replica_actions (
    operation_id UUID NOT NULL,
    replica_id UUID NOT NULL,
    ordinal INTEGER NOT NULL,
    backend_kind TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    expected_stored_length NUMERIC NOT NULL,
    expected_stored_sha256 BYTEA NOT NULL,
    expected_backend_version TEXT,
    state TEXT NOT NULL DEFAULT 'PENDING',
    action_generation NUMERIC NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ(6),
    updated_at TIMESTAMPTZ(6) NOT NULL,
    deleted_at TIMESTAMPTZ(6),
    last_outcome TEXT,
    last_error_code TEXT,
    PRIMARY KEY (operation_id, replica_id),
    CONSTRAINT object_gc_replica_actions_operation_fk
        FOREIGN KEY (operation_id) REFERENCES object_gc_operations (operation_id)
        ON DELETE RESTRICT,
    CONSTRAINT object_gc_replica_actions_ordinal_unique
        UNIQUE (operation_id, ordinal),
    CONSTRAINT object_gc_replica_actions_ordinal_nonnegative CHECK (ordinal >= 0),
    CONSTRAINT object_gc_replica_actions_backend_kind_value CHECK (
        backend_kind IN ('LOCAL_FILESYSTEM', 'OBJECT_STORE')
    ),
    CONSTRAINT object_gc_replica_actions_storage_key_length
        CHECK (octet_length(storage_key) BETWEEN 1 AND 1024),
    CONSTRAINT object_gc_replica_actions_expected_length_u64 CHECK (
        expected_stored_length >= 0
        AND expected_stored_length = trunc(expected_stored_length)
        AND expected_stored_length <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT object_gc_replica_actions_expected_sha256_length
        CHECK (octet_length(expected_stored_sha256) = 32),
    CONSTRAINT object_gc_replica_actions_backend_version_shape CHECK (
        expected_backend_version IS NULL
        OR octet_length(expected_backend_version) BETWEEN 1 AND 1024
    ),
    CONSTRAINT object_gc_replica_actions_state_value CHECK (
        state IN (
            'PENDING', 'DELETE_FENCED', 'RECONCILIATION_REQUIRED',
            'RETRYABLE', 'DELETED', 'FAILED'
        )
    ),
    CONSTRAINT object_gc_replica_actions_generation_u64 CHECK (
        action_generation >= 0
        AND action_generation = trunc(action_generation)
        AND action_generation <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT object_gc_replica_actions_time_shape CHECK (
        (state = 'DELETED' AND deleted_at IS NOT NULL)
        OR (state <> 'DELETED' AND deleted_at IS NULL)
    ),
    CONSTRAINT object_gc_replica_actions_outcome_value CHECK (
        last_outcome IS NULL
        OR last_outcome IN (
            'DELETED', 'ALREADY_ABSENT', 'RECONCILED_ABSENT',
            'STILL_PRESENT', 'DELETE_IN_PROGRESS', 'UNKNOWN', 'MISMATCH'
        )
    ),
    CONSTRAINT object_gc_replica_actions_error_code_shape CHECK (
        last_error_code IS NULL
        OR octet_length(last_error_code) BETWEEN 1 AND 128
    )
);

CREATE INDEX object_gc_replica_actions_next_idx
    ON object_gc_replica_actions (operation_id, ordinal)
    WHERE state <> 'DELETED';

-- Database-enforced defense in depth for every FileVersion/ObjectReplica
-- writer, including future code that does not yet call the application helper.
-- FOR SHARE serializes with the GC lifecycle-row UPDATE. A writer that wins
-- first commits its reference before GC can revalidate; GC that wins first
-- changes the lifecycle and makes the writer fail closed.
CREATE FUNCTION synveil_require_referenceable_object()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    current_lifecycle TEXT;
BEGIN
    SELECT lifecycle_state
      INTO current_lifecycle
      FROM objects
     WHERE id = NEW.object_id
       AND dedup_domain_id = NEW.object_dedup_domain_id
     FOR SHARE;

    IF current_lifecycle IS NOT NULL AND current_lifecycle <> 'AVAILABLE' THEN
        RAISE EXCEPTION 'object is not referenceable'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'object_reference_requires_available_lifecycle';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER file_versions_require_referenceable_object
BEFORE INSERT OR UPDATE OF object_id, object_dedup_domain_id
ON file_versions
FOR EACH ROW
EXECUTE FUNCTION synveil_require_referenceable_object();

CREATE TRIGGER object_replicas_require_referenceable_object
BEFORE INSERT OR UPDATE OF object_id, object_dedup_domain_id
ON object_replicas
FOR EACH ROW
EXECUTE FUNCTION synveil_require_referenceable_object();

-- Hold acquisition is a reference-class transition too. Inserting a live hold
-- or reactivating an expired/released row takes the same Object row share lock
-- as FileVersion creation. Releasing or shortening an inactive hold remains
-- possible while GC is in progress because it cannot make deletion less safe.
CREATE FUNCTION synveil_require_holdable_object()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    current_lifecycle TEXT;
BEGIN
    IF NEW.released_at IS NOT NULL
       OR (NEW.expires_at IS NOT NULL AND NEW.expires_at <= clock_timestamp()) THEN
        RETURN NEW;
    END IF;

    SELECT lifecycle_state
      INTO current_lifecycle
      FROM objects
     WHERE id = NEW.object_id
       AND dedup_domain_id = NEW.object_dedup_domain_id
     FOR SHARE;

    IF current_lifecycle IS NOT NULL AND current_lifecycle <> 'AVAILABLE' THEN
        RAISE EXCEPTION 'object cannot acquire a GC hold'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'object_gc_hold_requires_available_lifecycle';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER object_gc_holds_require_holdable_object
BEFORE INSERT OR UPDATE OF object_id, object_dedup_domain_id, expires_at, released_at
ON object_gc_holds
FOR EACH ROW
EXECUTE FUNCTION synveil_require_holdable_object();
