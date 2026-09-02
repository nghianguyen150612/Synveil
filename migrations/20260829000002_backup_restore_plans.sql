-- Durable, non-destructive restore planning and recovery preflight.
--
-- A plan records only logical source provenance and deterministic destination
-- identities. It never creates live Node/FileVersion rows, changes a journal,
-- reserves a retention pin, or stores Object/replica identity. Execution is a
-- later bounded capability and is deliberately not represented here.

-- The owner-pair keys let this migration enforce that a plan's source and
-- target references belong to the same authenticated owner as the plan row.
ALTER TABLE backup_sets
    ADD CONSTRAINT backup_sets_id_owner_unique
    UNIQUE (id, owner_user_id);

ALTER TABLE backup_snapshots
    ADD CONSTRAINT backup_snapshots_id_owner_unique
    UNIQUE (id, owner_user_id),
    ADD CONSTRAINT backup_snapshots_id_set_unique
    UNIQUE (id, backup_set_id);

CREATE TABLE backup_restore_plans (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    target_library_id UUID NOT NULL,
    target_parent_node_id UUID NOT NULL,
    operation_id TEXT NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    destination_name TEXT NOT NULL,
    base_journal_epoch BIGINT NOT NULL,
    base_journal_head BIGINT NOT NULL,
    item_count BIGINT NOT NULL,
    content_item_count BIGINT NOT NULL,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    stale_at TIMESTAMPTZ(6),
    CONSTRAINT backup_restore_plans_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_plans_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_plans_snapshot_owner_fk
        FOREIGN KEY (snapshot_id, owner_user_id)
        REFERENCES backup_snapshots (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_plans_snapshot_set_fk
        FOREIGN KEY (snapshot_id, backup_set_id)
        REFERENCES backup_snapshots (id, backup_set_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_plans_target_library_owner_fk
        FOREIGN KEY (target_library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_plans_target_parent_fk
        FOREIGN KEY (target_parent_node_id, target_library_id)
        REFERENCES nodes (id, library_id) ON DELETE RESTRICT,
    CONSTRAINT backup_restore_plans_operation_key_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_restore_plans_fingerprint_version_value
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_restore_plans_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_restore_plans_destination_name_length
        CHECK (octet_length(destination_name) BETWEEN 1 AND 1024),
    CONSTRAINT backup_restore_plans_epoch_positive
        CHECK (base_journal_epoch > 0),
    CONSTRAINT backup_restore_plans_head_nonnegative
        CHECK (base_journal_head >= 0),
    CONSTRAINT backup_restore_plans_item_count_positive
        CHECK (item_count > 0),
    CONSTRAINT backup_restore_plans_content_count_shape
        CHECK (content_item_count >= 0 AND content_item_count <= item_count),
    -- ASSEMBLING is an internal transaction-local seal state. It is never
    -- returned by the service: a successful transaction changes it to
    -- PLANNED, while a failure rolls the row back. Keeping it here lets the
    -- database reject direct entry inserts after a plan is sealed without a
    -- hidden session bypass flag.
    CONSTRAINT backup_restore_plans_state_value
        CHECK (state IN ('ASSEMBLING', 'PLANNED', 'STALE')),
    CONSTRAINT backup_restore_plans_stale_shape
        CHECK (
            (state IN ('ASSEMBLING', 'PLANNED') AND stale_at IS NULL)
            OR (state = 'STALE' AND stale_at IS NOT NULL)
        ),
    CONSTRAINT backup_restore_plans_owner_operation_unique
        UNIQUE (owner_user_id, operation_id)
);

CREATE INDEX backup_restore_plans_owner_created_idx
    ON backup_restore_plans (owner_user_id, created_at DESC, id DESC);

CREATE INDEX backup_restore_plans_owner_id_idx
    ON backup_restore_plans (owner_user_id, id);

-- A plan is immutable after insertion. The only permitted lifecycle mutation
-- is the one-way PLANNED -> STALE transition made by validation when the
-- target namespace no longer matches the captured epoch/head or destination
-- preflight. No current plan-delete or plan-edit protocol exists.
CREATE FUNCTION synveil_enforce_backup_restore_plan_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'backup restore plans cannot be deleted in this phase'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_plan_delete_forbidden';
    END IF;

    IF OLD.id IS NOT DISTINCT FROM NEW.id
       AND OLD.owner_user_id IS NOT DISTINCT FROM NEW.owner_user_id
       AND OLD.backup_set_id IS NOT DISTINCT FROM NEW.backup_set_id
       AND OLD.snapshot_id IS NOT DISTINCT FROM NEW.snapshot_id
       AND OLD.target_library_id IS NOT DISTINCT FROM NEW.target_library_id
       AND OLD.target_parent_node_id IS NOT DISTINCT FROM NEW.target_parent_node_id
       AND OLD.operation_id IS NOT DISTINCT FROM NEW.operation_id
       AND OLD.fingerprint_version IS NOT DISTINCT FROM NEW.fingerprint_version
       AND OLD.request_fingerprint IS NOT DISTINCT FROM NEW.request_fingerprint
       AND OLD.destination_name IS NOT DISTINCT FROM NEW.destination_name
       AND OLD.base_journal_epoch IS NOT DISTINCT FROM NEW.base_journal_epoch
       AND OLD.base_journal_head IS NOT DISTINCT FROM NEW.base_journal_head
       AND OLD.item_count IS NOT DISTINCT FROM NEW.item_count
       AND OLD.content_item_count IS NOT DISTINCT FROM NEW.content_item_count
       AND OLD.created_at IS NOT DISTINCT FROM NEW.created_at
       AND (
           (OLD.state IS NOT DISTINCT FROM NEW.state
            AND OLD.stale_at IS NOT DISTINCT FROM NEW.stale_at)
           OR (OLD.state = 'PLANNED'
               AND NEW.state = 'STALE'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NOT NULL)
           OR (OLD.state = 'ASSEMBLING'
               AND NEW.state = 'PLANNED'
               AND OLD.stale_at IS NULL
               AND NEW.stale_at IS NULL)
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup restore plan provenance is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_restore_plan_immutable';
END;
$$;

CREATE TRIGGER backup_restore_plans_immutable
BEFORE UPDATE OR DELETE
ON backup_restore_plans
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_restore_plan_immutability();

-- A successful service transaction must seal its internal assembly row before
-- commit. This prevents a direct SQL caller from leaving an ASSEMBLING plan
-- durable and then appending entries outside the one authoritative creation
-- transaction. The deferred check observes the final row state, so the
-- service's ASSEMBLING -> PLANNED transition remains valid.
CREATE FUNCTION synveil_require_backup_restore_plan_sealed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM backup_restore_plans
         WHERE id = NEW.id
           AND state = 'ASSEMBLING'
    ) THEN
        RAISE EXCEPTION 'backup restore plan assembly must be sealed before commit'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_plan_unsealed';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER backup_restore_plans_must_be_sealed
AFTER INSERT OR UPDATE
ON backup_restore_plans
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_restore_plan_sealed();

CREATE TABLE backup_restore_plan_entries (
    plan_id UUID NOT NULL,
    ordinal BIGINT NOT NULL,
    planned_node_id UUID NOT NULL,
    planned_parent_node_id UUID NOT NULL,
    source_snapshot_node_id UUID,
    source_parent_node_id UUID,
    source_state TEXT NOT NULL,
    action TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    source_revision NUMERIC,
    file_version_id UUID,
    content_length NUMERIC,
    content_sha256 BYTEA,
    CONSTRAINT backup_restore_plan_entries_plan_fk
        FOREIGN KEY (plan_id) REFERENCES backup_restore_plans (id) ON DELETE CASCADE,
    CONSTRAINT backup_restore_plan_entries_ordinal_nonnegative
        CHECK (ordinal >= 0),
    CONSTRAINT backup_restore_plan_entries_planned_id_nonzero
        CHECK (planned_node_id <> '00000000-0000-0000-0000-000000000000'::UUID),
    CONSTRAINT backup_restore_plan_entries_parent_id_nonzero
        CHECK (planned_parent_node_id <> '00000000-0000-0000-0000-000000000000'::UUID),
    CONSTRAINT backup_restore_plan_entries_source_id_nonzero
        CHECK (
            source_snapshot_node_id IS NULL
            OR source_snapshot_node_id <> '00000000-0000-0000-0000-000000000000'::UUID
        ),
    CONSTRAINT backup_restore_plan_entries_source_parent_id_nonzero
        CHECK (
            source_parent_node_id IS NULL
            OR source_parent_node_id <> '00000000-0000-0000-0000-000000000000'::UUID
        ),
    CONSTRAINT backup_restore_plan_entries_source_state_value
        CHECK (source_state IN ('ACTIVE', 'TRASHED')),
    CONSTRAINT backup_restore_plan_entries_action_value
        CHECK (action IN ('CREATE_DIRECTORY', 'CREATE_FILE')),
    CONSTRAINT backup_restore_plan_entries_kind_value
        CHECK (kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT backup_restore_plan_entries_name_length
        CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT backup_restore_plan_entries_revision_u64
        CHECK (
            source_revision IS NULL
            OR (
                source_revision >= 0
                AND source_revision = trunc(source_revision)
                AND source_revision <= 18446744073709551615::NUMERIC
            )
        ),
    CONSTRAINT backup_restore_plan_entries_sha256_length
        CHECK (content_sha256 IS NULL OR octet_length(content_sha256) = 32),
    CONSTRAINT backup_restore_plan_entries_content_length_u64
        CHECK (
            content_length IS NULL
            OR (
                content_length >= 0
                AND content_length = trunc(content_length)
                AND content_length <= 18446744073709551615::NUMERIC
            )
        ),
    CONSTRAINT backup_restore_plan_entries_content_shape
        CHECK (
            (
                kind = 'DIRECTORY'
                AND action = 'CREATE_DIRECTORY'
                AND file_version_id IS NULL
                AND content_length IS NULL
                AND content_sha256 IS NULL
            )
            OR (
                kind = 'FILE'
                AND action = 'CREATE_FILE'
                AND file_version_id IS NOT NULL
                AND content_length IS NOT NULL
                AND content_sha256 IS NOT NULL
            )
        ),
    CONSTRAINT backup_restore_plan_entries_wrapper_shape
        CHECK (
            (
                ordinal = 0
                AND source_snapshot_node_id IS NULL
                AND source_parent_node_id IS NULL
                AND source_state = 'ACTIVE'
                AND action = 'CREATE_DIRECTORY'
                AND kind = 'DIRECTORY'
                AND source_revision IS NULL
            )
            OR (
                ordinal > 0
                AND source_snapshot_node_id IS NOT NULL
                AND source_parent_node_id IS NOT NULL
                AND source_revision IS NOT NULL
            )
        ),
    PRIMARY KEY (plan_id, ordinal),
    CONSTRAINT backup_restore_plan_entries_planned_id_unique
        UNIQUE (plan_id, planned_node_id)
);

CREATE UNIQUE INDEX backup_restore_plan_entries_source_node_unique
    ON backup_restore_plan_entries (plan_id, source_snapshot_node_id)
    WHERE source_snapshot_node_id IS NOT NULL;

-- Entries are writable only during the one transaction that assembles a new
-- plan. Once the plan is sealed as PLANNED, ordinary SQL cannot append a
-- second wrapper/node or otherwise alter its immutable entry list.
CREATE FUNCTION synveil_require_backup_restore_plan_assembling()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    plan_state TEXT;
BEGIN
    SELECT state
      INTO plan_state
      FROM backup_restore_plans
     WHERE id = NEW.plan_id
     FOR SHARE;
    IF plan_state IS DISTINCT FROM 'ASSEMBLING' THEN
        RAISE EXCEPTION 'backup restore plan entries cannot be appended after sealing'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_restore_plan_entry_insert_forbidden';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_restore_plan_entries_require_assembling_plan
BEFORE INSERT
ON backup_restore_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_restore_plan_assembling();

-- The primary key is the bounded keyset pagination index. There is no update
-- or release protocol for entries in this phase; a future reviewed execution
-- migration may add an explicit plan lifecycle without weakening this row.
CREATE FUNCTION synveil_reject_backup_restore_plan_entry_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup restore plan entries are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_restore_plan_entry_immutable';
END;
$$;

CREATE TRIGGER backup_restore_plan_entries_immutable
BEFORE UPDATE OR DELETE
ON backup_restore_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_restore_plan_entry_mutation();
