-- Prompt 61: durable backup scheduling domain only.
--
-- A schedule is one stable owner/BackupSet identity with one current pointer
-- and append-only timing revisions. This migration deliberately adds no
-- scheduled-occurrence, job, lease, worker, retry, or execution table.

CREATE TABLE backup_schedules (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    current_revision_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_schedules_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedules_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedules_id_owner_set_unique
        UNIQUE (id, owner_user_id, backup_set_id),
    CONSTRAINT backup_schedules_one_per_backup_set
        UNIQUE (backup_set_id)
);

CREATE TABLE backup_schedule_revisions (
    id UUID PRIMARY KEY,
    schedule_id UUID NOT NULL,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    revision_number BIGINT NOT NULL,
    operation_id TEXT NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    recurrence_kind TEXT NOT NULL,
    timezone TEXT NOT NULL,
    local_time_minute SMALLINT NOT NULL,
    weekly_days SMALLINT[] NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_schedule_revisions_schedule_scope_fk
        FOREIGN KEY (schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedules (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_revisions_revision_positive
        CHECK (revision_number > 0),
    CONSTRAINT backup_schedule_revisions_operation_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_schedule_revisions_fingerprint_version
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_schedule_revisions_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_schedule_revisions_recurrence_value
        CHECK (recurrence_kind IN ('DAILY', 'WEEKLY')),
    CONSTRAINT backup_schedule_revisions_timezone_length
        CHECK (octet_length(timezone) BETWEEN 1 AND 255),
    CONSTRAINT backup_schedule_revisions_local_minute_value
        CHECK (local_time_minute BETWEEN 0 AND 1439),
    CONSTRAINT backup_schedule_revisions_weekly_days_count
        CHECK (cardinality(weekly_days) BETWEEN 0 AND 7),
    CONSTRAINT backup_schedule_revisions_weekly_days_values
        CHECK (weekly_days <@ ARRAY[1, 2, 3, 4, 5, 6, 7]::SMALLINT[]),
    CONSTRAINT backup_schedule_revisions_recurrence_days_shape
        CHECK (
            (recurrence_kind = 'DAILY' AND cardinality(weekly_days) = 0)
            OR (recurrence_kind = 'WEEKLY' AND cardinality(weekly_days) BETWEEN 1 AND 7)
        ),
    CONSTRAINT backup_schedule_revisions_schedule_number_unique
        UNIQUE (schedule_id, revision_number),
    CONSTRAINT backup_schedule_revisions_id_scope_unique
        UNIQUE (id, schedule_id, owner_user_id, backup_set_id),
    CONSTRAINT backup_schedule_revisions_owner_operation_unique
        UNIQUE (owner_user_id, operation_id)
);

-- The pointer is deferred only to permit atomic first configuration: the
-- schedule row can carry its preallocated current revision ID before the
-- revision row is inserted in the same transaction.
ALTER TABLE backup_schedules
    ADD CONSTRAINT backup_schedules_current_revision_scope_fk
    FOREIGN KEY (current_revision_id, id, owner_user_id, backup_set_id)
    REFERENCES backup_schedule_revisions (id, schedule_id, owner_user_id, backup_set_id)
    ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;

CREATE INDEX backup_schedule_revisions_schedule_order_idx
    ON backup_schedule_revisions (schedule_id, revision_number DESC);

-- A no-op configuration does not create a revision, but its operation key and
-- canonical result must still be durable so a same-key retry cannot acquire a
-- different meaning after a later edit.
CREATE TABLE backup_schedule_operations (
    owner_user_id UUID NOT NULL,
    operation_id TEXT NOT NULL,
    backup_set_id UUID NOT NULL,
    schedule_id UUID NOT NULL,
    result_revision_id UUID NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_schedule_operations_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_operations_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_operations_schedule_scope_fk
        FOREIGN KEY (schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedules (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_operations_revision_scope_fk
        FOREIGN KEY (result_revision_id, schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedule_revisions (id, schedule_id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_operations_operation_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_schedule_operations_fingerprint_version
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_schedule_operations_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_schedule_operations_owner_operation_unique
        UNIQUE (owner_user_id, operation_id)
);

CREATE INDEX backup_schedule_operations_schedule_idx
    ON backup_schedule_operations (schedule_id, created_at DESC);

-- Weekday arrays are persisted only in normalized Monday=1..Sunday=7 order.
-- The service performs the same normalization before semantic comparison; the
-- trigger prevents a direct SQL writer from creating a noncanonical revision.
CREATE FUNCTION synveil_validate_backup_schedule_revision_shape()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    normalized_days SMALLINT[];
BEGIN
    IF EXISTS (
        SELECT 1
        FROM unnest(NEW.weekly_days) AS value
        WHERE value IS NULL OR value < 1 OR value > 7
    ) THEN
        RAISE EXCEPTION 'backup schedule weekday value is invalid'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_schedule_revisions_weekly_days_values';
    END IF;

    SELECT COALESCE(array_agg(normalized.day ORDER BY normalized.day), '{}'::SMALLINT[])
      INTO normalized_days
      FROM (
          SELECT DISTINCT value AS day
          FROM unnest(NEW.weekly_days) AS value
      ) AS normalized;

    IF NEW.weekly_days IS DISTINCT FROM normalized_days THEN
        RAISE EXCEPTION 'backup schedule weekdays are not canonically ordered'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_schedule_revisions_weekly_days_canonical';
    END IF;

    IF NEW.recurrence_kind = 'DAILY' AND cardinality(NEW.weekly_days) <> 0 THEN
        RAISE EXCEPTION 'daily backup schedules cannot carry weekdays'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_schedule_revisions_daily_weekdays_empty';
    END IF;

    IF NEW.recurrence_kind = 'WEEKLY' AND cardinality(NEW.weekly_days) = 0 THEN
        RAISE EXCEPTION 'weekly backup schedules require at least one weekday'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_schedule_revisions_weekly_weekdays_required';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_schedule_revisions_validate_shape
    BEFORE INSERT ON backup_schedule_revisions
    FOR EACH ROW
    EXECUTE FUNCTION synveil_validate_backup_schedule_revision_shape();

-- Historical timing configuration is immutable at the database boundary.
CREATE FUNCTION synveil_reject_backup_schedule_revision_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup schedule revisions are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_schedule_revisions_immutable';
END;
$$;

CREATE TRIGGER backup_schedule_revisions_immutable
    BEFORE UPDATE OR DELETE ON backup_schedule_revisions
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_schedule_revision_mutation();

-- Stable schedule identity cannot be transferred or pointed backwards. The
-- current pointer can move only to a strictly higher committed revision.
CREATE FUNCTION synveil_enforce_backup_schedule_integrity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    previous_revision_number BIGINT;
    next_revision_number BIGINT;
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.owner_user_id IS DISTINCT FROM OLD.owner_user_id
       OR NEW.backup_set_id IS DISTINCT FROM OLD.backup_set_id THEN
        RAISE EXCEPTION 'backup schedule identity is immutable'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_schedules_identity_immutable';
    END IF;

    IF NEW.current_revision_id IS DISTINCT FROM OLD.current_revision_id THEN
        SELECT revision_number
          INTO previous_revision_number
          FROM backup_schedule_revisions
         WHERE id = OLD.current_revision_id
           AND schedule_id = OLD.id
           AND owner_user_id = OLD.owner_user_id
           AND backup_set_id = OLD.backup_set_id;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'backup schedule previous current revision is invalid'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_schedules_current_revision_previous_scope';
        END IF;

        SELECT revision_number
          INTO next_revision_number
          FROM backup_schedule_revisions
         WHERE id = NEW.current_revision_id
           AND schedule_id = NEW.id
           AND owner_user_id = NEW.owner_user_id
           AND backup_set_id = NEW.backup_set_id;
        IF NOT FOUND OR next_revision_number <= previous_revision_number THEN
            RAISE EXCEPTION 'backup schedule current revision must advance monotonically'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_schedules_current_revision_monotonic';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_schedules_integrity
    BEFORE UPDATE ON backup_schedules
    FOR EACH ROW
    EXECUTE FUNCTION synveil_enforce_backup_schedule_integrity();

-- Operation evidence is also append-only; deleting a schedule cannot remove
-- its revision history or replay fence through a direct metadata mutation.
CREATE FUNCTION synveil_reject_backup_schedule_operation_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup schedule operations are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_schedule_operations_immutable';
END;
$$;

CREATE TRIGGER backup_schedule_operations_immutable
    BEFORE UPDATE OR DELETE ON backup_schedule_operations
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_schedule_operation_mutation();
