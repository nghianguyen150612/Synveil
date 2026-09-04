-- Prompt 62: durable schedule-occurrence identity and activation fences.
--
-- This migration materializes only immutable control-plane firing evidence.
-- It adds no execution state, worker, lease, retry, snapshot, maintenance run,
-- journal event, sync mutation, ObjectStore reference, HTTP route, or UI.

-- Prompt 61's updated_at moved on semantic edits and enable transitions, but
-- was not a dedicated scheduling contract. Backfilling from it is the most
-- conservative durable boundary available: no pre-migration occurrence at or
-- before the last recorded schedule mutation can be newly materialized.
ALTER TABLE backup_schedules
    ADD COLUMN effective_from TIMESTAMPTZ(6);

UPDATE backup_schedules
   SET effective_from = updated_at;

ALTER TABLE backup_schedules
    ALTER COLUMN effective_from SET NOT NULL,
    ADD CONSTRAINT backup_schedules_effective_time_order
        CHECK (created_at <= effective_from AND effective_from <= updated_at);

-- Stable schedule identity remains immutable. A semantic revision change or
-- enabled-state transition must advance the dedicated activation boundary;
-- unrelated/no-op updates must not move it.
CREATE OR REPLACE FUNCTION synveil_enforce_backup_schedule_integrity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    previous_revision_number BIGINT;
    next_revision_number BIGINT;
    semantic_transition BOOLEAN;
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.owner_user_id IS DISTINCT FROM OLD.owner_user_id
       OR NEW.backup_set_id IS DISTINCT FROM OLD.backup_set_id THEN
        RAISE EXCEPTION 'backup schedule identity is immutable'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_schedules_identity_immutable';
    END IF;

    semantic_transition :=
        NEW.current_revision_id IS DISTINCT FROM OLD.current_revision_id
        OR NEW.enabled IS DISTINCT FROM OLD.enabled;

    IF NEW.effective_from IS DISTINCT FROM OLD.effective_from THEN
        IF NOT semantic_transition OR NEW.effective_from <= OLD.effective_from THEN
            RAISE EXCEPTION 'backup schedule effective boundary is invalid'
                USING ERRCODE = '23000',
                      CONSTRAINT = 'backup_schedules_effective_from_transition';
        END IF;
    ELSIF semantic_transition THEN
        RAISE EXCEPTION 'backup schedule semantic transition requires a new effective boundary'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_schedules_effective_from_required';
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

CREATE TABLE backup_schedule_occurrences (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    schedule_id UUID NOT NULL,
    schedule_revision_id UUID NOT NULL,
    local_calendar_date DATE NOT NULL,
    resolved_local_time_minute SMALLINT NOT NULL,
    scheduled_for_utc TIMESTAMPTZ(6) NOT NULL,
    materialized_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_schedule_occurrences_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrences_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrences_schedule_scope_fk
        FOREIGN KEY (schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedules (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrences_revision_scope_fk
        FOREIGN KEY (schedule_revision_id, schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedule_revisions (id, schedule_id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrences_local_minute_value
        CHECK (resolved_local_time_minute BETWEEN 0 AND 1439),
    CONSTRAINT backup_schedule_occurrences_id_scope_unique
        UNIQUE (id, owner_user_id, backup_set_id, schedule_id),
    CONSTRAINT backup_schedule_occurrences_logical_key_unique
        UNIQUE (schedule_revision_id, local_calendar_date),
    CONSTRAINT backup_schedule_occurrences_schedule_instant_unique
        UNIQUE (schedule_id, scheduled_for_utc)
);

CREATE INDEX backup_schedule_occurrences_owner_time_idx
    ON backup_schedule_occurrences
        (owner_user_id, scheduled_for_utc, id);

CREATE INDEX backup_schedule_occurrences_schedule_time_idx
    ON backup_schedule_occurrences
        (schedule_id, scheduled_for_utc, id);

-- Occurrence rows are historical firing evidence. Future claim/execution
-- state must live in a separate relation and must never rewrite this ledger.
CREATE FUNCTION synveil_reject_backup_schedule_occurrence_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup schedule occurrences are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_schedule_occurrences_immutable';
END;
$$;

CREATE TRIGGER backup_schedule_occurrences_immutable
    BEFORE UPDATE OR DELETE ON backup_schedule_occurrences
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_schedule_occurrence_mutation();
