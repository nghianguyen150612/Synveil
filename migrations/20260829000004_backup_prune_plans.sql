-- Durable, non-destructive backup prune planning and retention-release
-- preflight. A plan records what an explicit future release would affect; it
-- does not release a retention pin, delete a snapshot/manifest, create a GC
-- candidate, or authorize physical deletion.
--
-- Logical plan entries intentionally contain only snapshot-node/FileVersion
-- provenance and content metadata. Distinct-Object reference-accounting
-- evidence is private to this PostgreSQL schema and is never projected through
-- the public backup domain.

CREATE TABLE backup_prune_plans (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    backup_set_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    operation_id TEXT NOT NULL,
    fingerprint_version SMALLINT NOT NULL,
    request_fingerprint BYTEA NOT NULL,
    snapshot_manifest_item_count BIGINT NOT NULL,
    snapshot_content_reference_count BIGINT NOT NULL,
    planned_pin_release_count BIGINT NOT NULL,
    distinct_retained_content_count BIGINT NOT NULL,
    retained_after_release_count BIGINT NOT NULL,
    would_become_unreferenced_count BIGINT NOT NULL,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    stale_at TIMESTAMPTZ(6),
    CONSTRAINT backup_prune_plans_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_plans_set_owner_fk
        FOREIGN KEY (backup_set_id, owner_user_id)
        REFERENCES backup_sets (id, owner_user_id) ON DELETE RESTRICT,
    -- Restrict, rather than cascade, preserves future restore and prune audit
    -- provenance if a later execution protocol gains manifest cleanup.
    CONSTRAINT backup_prune_plans_snapshot_owner_fk
        FOREIGN KEY (snapshot_id, owner_user_id)
        REFERENCES backup_snapshots (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_plans_snapshot_set_fk
        FOREIGN KEY (snapshot_id, backup_set_id)
        REFERENCES backup_snapshots (id, backup_set_id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_plans_operation_key_length
        CHECK (octet_length(operation_id) BETWEEN 8 AND 256),
    CONSTRAINT backup_prune_plans_fingerprint_version_value
        CHECK (fingerprint_version = 1),
    CONSTRAINT backup_prune_plans_fingerprint_length
        CHECK (octet_length(request_fingerprint) = 32),
    CONSTRAINT backup_prune_plans_count_shape
        CHECK (
            snapshot_manifest_item_count >= 0
            AND snapshot_content_reference_count >= 0
            AND planned_pin_release_count >= 0
            AND distinct_retained_content_count >= 0
            AND retained_after_release_count >= 0
            AND would_become_unreferenced_count >= 0
            AND planned_pin_release_count = snapshot_content_reference_count
            AND distinct_retained_content_count <= planned_pin_release_count
            AND retained_after_release_count + would_become_unreferenced_count
                = distinct_retained_content_count
        ),
    -- ASSEMBLING is transaction-local only. A deferred seal trigger rejects it
    -- at commit, so callers can never observe or extend an incomplete plan.
    CONSTRAINT backup_prune_plans_state_value
        CHECK (state IN ('ASSEMBLING', 'PLANNED', 'STALE')),
    CONSTRAINT backup_prune_plans_stale_shape
        CHECK (
            (state IN ('ASSEMBLING', 'PLANNED') AND stale_at IS NULL)
            OR (state = 'STALE' AND stale_at IS NOT NULL)
        ),
    CONSTRAINT backup_prune_plans_owner_operation_unique
        UNIQUE (owner_user_id, operation_id)
);

CREATE INDEX backup_prune_plans_owner_created_idx
    ON backup_prune_plans (owner_user_id, created_at DESC, id DESC);

-- One expired snapshot has one active preflight owner. Including the internal
-- assembly state closes the concurrent-insert window; the service also locks
-- the source snapshot and turns this into a deterministic product result.
CREATE UNIQUE INDEX backup_prune_plans_one_active_snapshot
    ON backup_prune_plans (snapshot_id)
    WHERE state IN ('ASSEMBLING', 'PLANNED');

CREATE FUNCTION synveil_require_backup_prune_plan_initial_assembly()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state IS DISTINCT FROM 'ASSEMBLING' OR NEW.stale_at IS NOT NULL THEN
        RAISE EXCEPTION 'backup prune plan must begin in assembly state'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_initial_state';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_prune_plans_require_initial_assembly
BEFORE INSERT ON backup_prune_plans
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_prune_plan_initial_assembly();

-- All provenance is immutable. The only allowable durable lifecycle update is
-- PLANNED -> STALE after a later validation finds accounting drift.
CREATE FUNCTION synveil_enforce_backup_prune_plan_immutability()
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
       ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup prune plan provenance is immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_prune_plan_immutable';
END;
$$;

CREATE TRIGGER backup_prune_plans_immutable
BEFORE UPDATE OR DELETE ON backup_prune_plans
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_prune_plan_immutability();

CREATE TABLE backup_prune_plan_entries (
    plan_id UUID NOT NULL,
    ordinal BIGINT NOT NULL,
    source_snapshot_node_id UUID NOT NULL,
    file_version_id UUID NOT NULL,
    content_length NUMERIC NOT NULL,
    content_sha256 BYTEA NOT NULL,
    CONSTRAINT backup_prune_plan_entries_plan_fk
        FOREIGN KEY (plan_id) REFERENCES backup_prune_plans (id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_plan_entries_ordinal_nonnegative CHECK (ordinal >= 0),
    CONSTRAINT backup_prune_plan_entries_source_node_nonzero
        CHECK (source_snapshot_node_id <> '00000000-0000-0000-0000-000000000000'::UUID),
    CONSTRAINT backup_prune_plan_entries_file_version_nonzero
        CHECK (file_version_id <> '00000000-0000-0000-0000-000000000000'::UUID),
    CONSTRAINT backup_prune_plan_entries_content_length_u64
        CHECK (
            content_length >= 0
            AND content_length = trunc(content_length)
            AND content_length <= 18446744073709551615::NUMERIC
        ),
    CONSTRAINT backup_prune_plan_entries_sha256_length
        CHECK (octet_length(content_sha256) = 32),
    PRIMARY KEY (plan_id, ordinal),
    CONSTRAINT backup_prune_plan_entries_source_node_unique
        UNIQUE (plan_id, source_snapshot_node_id)
);

-- Server-internal accounting evidence. It is intentionally not FK-bound to
-- objects: a future normal GC lifecycle must be able to remove an Object after
-- a separately authorized release without destroying immutable plan evidence.
CREATE TABLE backup_prune_plan_object_impacts (
    plan_id UUID NOT NULL,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    target_snapshot_pin_count BIGINT NOT NULL,
    surviving_live_file_version_reference_count BIGINT NOT NULL,
    surviving_other_snapshot_pin_count BIGINT NOT NULL,
    predicted_post_release_reference_count BIGINT NOT NULL,
    impact TEXT NOT NULL,
    CONSTRAINT backup_prune_plan_object_impacts_plan_fk
        FOREIGN KEY (plan_id) REFERENCES backup_prune_plans (id) ON DELETE RESTRICT,
    CONSTRAINT backup_prune_plan_object_impacts_target_pin_positive
        CHECK (target_snapshot_pin_count > 0),
    CONSTRAINT backup_prune_plan_object_impacts_count_shape
        CHECK (
            surviving_live_file_version_reference_count >= 0
            AND surviving_other_snapshot_pin_count >= 0
            AND predicted_post_release_reference_count >= 0
            AND predicted_post_release_reference_count
                = surviving_live_file_version_reference_count
                  + surviving_other_snapshot_pin_count
        ),
    CONSTRAINT backup_prune_plan_object_impacts_value
        CHECK (impact IN ('RETAINED_BY_OTHER_REFERENCE', 'WOULD_BECOME_UNREFERENCED')),
    CONSTRAINT backup_prune_plan_object_impacts_classification_shape
        CHECK (
            (predicted_post_release_reference_count > 0
             AND impact = 'RETAINED_BY_OTHER_REFERENCE')
            OR (predicted_post_release_reference_count = 0
                AND impact = 'WOULD_BECOME_UNREFERENCED')
        ),
    PRIMARY KEY (plan_id, object_id, object_dedup_domain_id)
);

CREATE FUNCTION synveil_require_backup_prune_plan_assembling()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    plan_state TEXT;
BEGIN
    SELECT state
      INTO plan_state
      FROM backup_prune_plans
     WHERE id = NEW.plan_id
     FOR SHARE;
    IF plan_state IS DISTINCT FROM 'ASSEMBLING' THEN
        RAISE EXCEPTION 'backup prune plan evidence cannot be appended after sealing'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_evidence_insert_forbidden';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION synveil_validate_backup_prune_plan_entry()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    source_snapshot_id UUID;
    expected_file_version_id UUID;
    expected_content_length NUMERIC;
    expected_content_sha256 BYTEA;
BEGIN
    SELECT snapshot_id
      INTO source_snapshot_id
      FROM backup_prune_plans
     WHERE id = NEW.plan_id
     FOR SHARE;
    IF source_snapshot_id IS NULL THEN
        RAISE EXCEPTION 'backup prune plan entry has no source plan'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_entry_plan_missing';
    END IF;

    SELECT current_version_id, content_length, content_sha256
      INTO expected_file_version_id, expected_content_length, expected_content_sha256
      FROM backup_snapshot_nodes
     WHERE snapshot_id = source_snapshot_id
       AND node_id = NEW.source_snapshot_node_id
       AND kind = 'FILE'
       AND current_version_id IS NOT NULL;
    IF NOT FOUND
       OR NEW.file_version_id IS DISTINCT FROM expected_file_version_id
       OR NEW.content_length IS DISTINCT FROM expected_content_length
       OR NEW.content_sha256 IS DISTINCT FROM expected_content_sha256 THEN
        RAISE EXCEPTION 'backup prune plan entry does not match source manifest content'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_prune_plan_entry_manifest_mapping';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_prune_plan_entries_require_assembling_plan
BEFORE INSERT ON backup_prune_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_prune_plan_assembling();

CREATE TRIGGER backup_prune_plan_entries_validate_manifest_mapping
BEFORE INSERT ON backup_prune_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_prune_plan_entry();

CREATE FUNCTION synveil_reject_backup_prune_plan_entry_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup prune plan entries are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_prune_plan_entry_immutable';
END;
$$;

CREATE TRIGGER backup_prune_plan_entries_immutable
BEFORE UPDATE OR DELETE ON backup_prune_plan_entries
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_prune_plan_entry_mutation();

CREATE FUNCTION synveil_validate_backup_prune_plan_object_impact()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    source_snapshot_id UUID;
    target_pin_count BIGINT;
    live_reference_count BIGINT;
    other_snapshot_pin_count BIGINT;
    object_state TEXT;
BEGIN
    SELECT snapshot_id
      INTO source_snapshot_id
      FROM backup_prune_plans
     WHERE id = NEW.plan_id
     FOR SHARE;
    IF source_snapshot_id IS NULL THEN
        RAISE EXCEPTION 'backup prune object impact has no source plan'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_prune_plan_impact_plan_missing';
    END IF;

    SELECT lifecycle_state
      INTO object_state
      FROM objects
     WHERE id = NEW.object_id
       AND dedup_domain_id = NEW.object_dedup_domain_id
     FOR SHARE;
    IF object_state IS DISTINCT FROM 'AVAILABLE' THEN
        RAISE EXCEPTION 'backup prune object impact does not resolve to retained object'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_prune_plan_impact_object_mapping';
    END IF;

    SELECT count(*) INTO target_pin_count
      FROM backup_snapshot_content_pins
     WHERE snapshot_id = source_snapshot_id
       AND object_id = NEW.object_id
       AND object_dedup_domain_id = NEW.object_dedup_domain_id;
    SELECT count(*) INTO live_reference_count
      FROM file_versions
     WHERE object_id = NEW.object_id
       AND object_dedup_domain_id = NEW.object_dedup_domain_id;
    SELECT count(*) INTO other_snapshot_pin_count
      FROM backup_snapshot_content_pins
     WHERE snapshot_id <> source_snapshot_id
       AND object_id = NEW.object_id
       AND object_dedup_domain_id = NEW.object_dedup_domain_id;
    IF target_pin_count IS DISTINCT FROM NEW.target_snapshot_pin_count
       OR live_reference_count
           IS DISTINCT FROM NEW.surviving_live_file_version_reference_count
       OR other_snapshot_pin_count
           IS DISTINCT FROM NEW.surviving_other_snapshot_pin_count
       OR NEW.predicted_post_release_reference_count
           IS DISTINCT FROM live_reference_count + other_snapshot_pin_count THEN
        RAISE EXCEPTION 'backup prune object impact does not match reference accounting'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_prune_plan_impact_reference_accounting';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_prune_plan_impacts_require_assembling_plan
BEFORE INSERT ON backup_prune_plan_object_impacts
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_prune_plan_assembling();

CREATE TRIGGER backup_prune_plan_impacts_validate_reference_accounting
BEFORE INSERT ON backup_prune_plan_object_impacts
FOR EACH ROW
EXECUTE FUNCTION synveil_validate_backup_prune_plan_object_impact();

CREATE FUNCTION synveil_reject_backup_prune_plan_impact_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup prune object impacts are immutable'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_prune_plan_impact_immutable';
END;
$$;

CREATE TRIGGER backup_prune_plan_impacts_immutable
BEFORE UPDATE OR DELETE ON backup_prune_plan_object_impacts
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_prune_plan_impact_mutation();

-- The final transaction fence proves a sealed plan represents all and only
-- source retention pins. It deliberately contains no DELETE/UPDATE against
-- snapshots, pins, Objects, replicas, candidates, or leases.
CREATE FUNCTION synveil_require_backup_prune_plan_sealed()
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
       AND current_plan_state IS DISTINCT FROM 'STALE' THEN
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

CREATE CONSTRAINT TRIGGER backup_prune_plans_must_be_sealed
AFTER INSERT OR UPDATE ON backup_prune_plans
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_prune_plan_sealed();
