-- Prompt 63: exactly-once occurrence-to-maintenance handoff.
--
-- This relation stores only logical provenance. It does not add a scheduler,
-- worker, lease, retry policy, progress state, snapshot side effect, journal
-- event, sync mutation, ObjectStore reference, or physical-storage identity.

-- The existing maintenance-run primary key is sufficient for ordinary reads,
-- but the composite key is required so this relation's foreign key proves the
-- owner/BackupSet scope of the referenced canonical run in the database.
ALTER TABLE backup_maintenance_runs
    ADD CONSTRAINT backup_maintenance_runs_id_owner_set_unique
    UNIQUE (id, owner_user_id, backup_set_id);

CREATE TABLE backup_schedule_occurrence_handoffs (
    occurrence_id UUID NOT NULL,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    schedule_id UUID NOT NULL,
    maintenance_run_id UUID NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_schedule_occurrence_handoffs_occurrence_unique
        UNIQUE (occurrence_id),
    CONSTRAINT backup_schedule_occurrence_handoffs_maintenance_unique
        UNIQUE (maintenance_run_id),
    CONSTRAINT backup_schedule_occurrence_handoffs_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrence_handoffs_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrence_handoffs_schedule_scope_fk
        FOREIGN KEY (schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedules (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrence_handoffs_occurrence_scope_fk
        FOREIGN KEY (occurrence_id, owner_user_id, backup_set_id, schedule_id)
        REFERENCES backup_schedule_occurrences (
            id, owner_user_id, backup_set_id, schedule_id
        ) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_occurrence_handoffs_maintenance_scope_fk
        FOREIGN KEY (maintenance_run_id, owner_user_id, backup_set_id)
        REFERENCES backup_maintenance_runs (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT
);

-- Handoff evidence is the immutable answer to “which run did this exact
-- occurrence produce?” It must never be retargeted or removed for replay.
CREATE FUNCTION synveil_reject_backup_schedule_occurrence_handoff_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup schedule occurrence handoffs are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_schedule_occurrence_handoffs_immutable';
END;
$$;

CREATE TRIGGER backup_schedule_occurrence_handoffs_immutable
    BEFORE UPDATE OR DELETE ON backup_schedule_occurrence_handoffs
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_schedule_occurrence_handoff_mutation();
