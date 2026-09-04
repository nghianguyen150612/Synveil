-- Prompt 65: revisioned bounded misfire policy and immutable expired-prefix
-- resolution. This adds no daemon, worker, lease, retry, maintenance
-- advancement, snapshot, ObjectStore, journal, sync, HTTP, or UI behavior.

ALTER TABLE backup_schedule_revisions
    ADD COLUMN misfire_mode TEXT NOT NULL DEFAULT 'LATEST_ONLY',
    ADD COLUMN max_lateness_seconds INTEGER NOT NULL DEFAULT 604800,
    DROP CONSTRAINT backup_schedule_revisions_fingerprint_version,
    ADD CONSTRAINT backup_schedule_revisions_fingerprint_version
        CHECK (fingerprint_version IN (1, 2)),
    ADD CONSTRAINT backup_schedule_revisions_misfire_mode
        CHECK (misfire_mode IN ('REPLAY_ONE_BY_ONE', 'LATEST_ONLY')),
    ADD CONSTRAINT backup_schedule_revisions_max_lateness
        CHECK (max_lateness_seconds BETWEEN 60 AND 2678400);

ALTER TABLE backup_schedule_revisions
    ALTER COLUMN misfire_mode DROP DEFAULT,
    ALTER COLUMN max_lateness_seconds DROP DEFAULT;

ALTER TABLE backup_schedule_operations
    DROP CONSTRAINT backup_schedule_operations_fingerprint_version,
    ADD CONSTRAINT backup_schedule_operations_fingerprint_version
        CHECK (fingerprint_version IN (1, 2));

CREATE TABLE backup_schedule_misfire_skips (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    schedule_id UUID NOT NULL,
    schedule_revision_id UUID NOT NULL,
    activation_effective_from TIMESTAMPTZ(6) NOT NULL,
    resolved_from_exclusive_utc TIMESTAMPTZ(6) NOT NULL,
    resolved_through_utc TIMESTAMPTZ(6) NOT NULL,
    observed_at_utc TIMESTAMPTZ(6) NOT NULL,
    misfire_mode TEXT NOT NULL,
    max_lateness_seconds INTEGER NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_schedule_misfire_skips_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_misfire_skips_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_misfire_skips_schedule_scope_fk
        FOREIGN KEY (schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedules (id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_misfire_skips_revision_scope_fk
        FOREIGN KEY (schedule_revision_id, schedule_id, owner_user_id, backup_set_id)
        REFERENCES backup_schedule_revisions (id, schedule_id, owner_user_id, backup_set_id)
        ON DELETE RESTRICT,
    CONSTRAINT backup_schedule_misfire_skips_range_order
        CHECK (resolved_through_utc > resolved_from_exclusive_utc),
    CONSTRAINT backup_schedule_misfire_skips_not_future
        CHECK (resolved_through_utc <= observed_at_utc),
    CONSTRAINT backup_schedule_misfire_skips_observation_order
        CHECK (observed_at_utc <= created_at),
    CONSTRAINT backup_schedule_misfire_skips_misfire_mode
        CHECK (misfire_mode IN ('REPLAY_ONE_BY_ONE', 'LATEST_ONLY')),
    CONSTRAINT backup_schedule_misfire_skips_max_lateness
        CHECK (max_lateness_seconds BETWEEN 60 AND 2678400),
    CONSTRAINT backup_schedule_misfire_skips_id_scope_unique
        UNIQUE (id, owner_user_id, backup_set_id, schedule_id),
    CONSTRAINT backup_schedule_misfire_skips_boundary_unique
        UNIQUE (
            schedule_id,
            schedule_revision_id,
            activation_effective_from,
            resolved_through_utc
        )
);

CREATE INDEX backup_schedule_misfire_skips_activation_latest_idx
    ON backup_schedule_misfire_skips (
        schedule_id,
        schedule_revision_id,
        activation_effective_from,
        resolved_through_utc DESC
    );

CREATE INDEX backup_schedule_misfire_skips_owner_created_idx
    ON backup_schedule_misfire_skips (owner_user_id, created_at DESC, id);

-- A direct writer must not mix policy snapshots, move an activation backward,
-- or append against a stale revision/enable epoch. The service already holds
-- BackupSet then BackupSchedule locks before insertion; this trigger is the
-- database fence for the same monotonic contract.
CREATE FUNCTION synveil_validate_backup_schedule_misfire_skip()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    revision_mode TEXT;
    revision_lateness INTEGER;
    schedule_current_revision UUID;
    schedule_effective_from TIMESTAMPTZ(6);
    schedule_enabled BOOLEAN;
    latest_handoff TIMESTAMPTZ(6);
    latest_skip TIMESTAMPTZ(6);
    resolution_reference TIMESTAMPTZ(6);
BEGIN
    SELECT misfire_mode, max_lateness_seconds
      INTO revision_mode, revision_lateness
      FROM backup_schedule_revisions
     WHERE id = NEW.schedule_revision_id
       AND schedule_id = NEW.schedule_id
       AND owner_user_id = NEW.owner_user_id
       AND backup_set_id = NEW.backup_set_id;
    IF NOT FOUND
       OR revision_mode IS DISTINCT FROM NEW.misfire_mode
       OR revision_lateness IS DISTINCT FROM NEW.max_lateness_seconds THEN
        RAISE EXCEPTION 'misfire skip policy snapshot is invalid'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_schedule_misfire_skips_policy_scope';
    END IF;

    SELECT current_revision_id, effective_from, enabled
      INTO schedule_current_revision, schedule_effective_from, schedule_enabled
      FROM backup_schedules
     WHERE id = NEW.schedule_id
       AND owner_user_id = NEW.owner_user_id
       AND backup_set_id = NEW.backup_set_id;
    IF NOT FOUND
       OR schedule_current_revision IS DISTINCT FROM NEW.schedule_revision_id
       OR schedule_effective_from IS DISTINCT FROM NEW.activation_effective_from
       OR NOT schedule_enabled THEN
        RAISE EXCEPTION 'misfire skip activation epoch is stale'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_schedule_misfire_skips_activation_scope';
    END IF;

    SELECT max(occurrence.scheduled_for_utc)
      INTO latest_handoff
      FROM backup_schedule_occurrence_handoffs AS handoff
      JOIN backup_schedule_occurrences AS occurrence
        ON occurrence.id = handoff.occurrence_id
       AND occurrence.owner_user_id = handoff.owner_user_id
       AND occurrence.backup_set_id = handoff.backup_set_id
       AND occurrence.schedule_id = handoff.schedule_id
     WHERE handoff.owner_user_id = NEW.owner_user_id
       AND handoff.backup_set_id = NEW.backup_set_id
       AND handoff.schedule_id = NEW.schedule_id
       AND occurrence.schedule_revision_id = NEW.schedule_revision_id
       AND occurrence.scheduled_for_utc > NEW.activation_effective_from;

    SELECT max(resolved_through_utc)
      INTO latest_skip
      FROM backup_schedule_misfire_skips
     WHERE owner_user_id = NEW.owner_user_id
       AND backup_set_id = NEW.backup_set_id
       AND schedule_id = NEW.schedule_id
       AND schedule_revision_id = NEW.schedule_revision_id
       AND activation_effective_from = NEW.activation_effective_from;

    resolution_reference := GREATEST(
        NEW.activation_effective_from,
        COALESCE(latest_handoff, NEW.activation_effective_from),
        COALESCE(latest_skip, NEW.activation_effective_from)
    );
    IF NEW.resolved_from_exclusive_utc IS DISTINCT FROM resolution_reference THEN
        RAISE EXCEPTION 'misfire skip does not advance current resolution reference'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_schedule_misfire_skips_monotonic';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_schedule_misfire_skips_validate
    BEFORE INSERT ON backup_schedule_misfire_skips
    FOR EACH ROW
    EXECUTE FUNCTION synveil_validate_backup_schedule_misfire_skip();

CREATE FUNCTION synveil_reject_backup_schedule_misfire_skip_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup schedule misfire skips are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_schedule_misfire_skips_immutable';
END;
$$;

CREATE TRIGGER backup_schedule_misfire_skips_immutable
    BEFORE UPDATE OR DELETE ON backup_schedule_misfire_skips
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_backup_schedule_misfire_skip_mutation();
