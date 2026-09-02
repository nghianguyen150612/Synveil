-- Durable server-internal backup content retention accounting.
--
-- Prompt 41's public manifest deliberately records only logical
-- FileVersion identity and safe content metadata. This relation is the
-- private bridge to canonical Object identity required to preserve captured
-- bytes after the live Node/FileVersion history is purged. A pin belongs to
-- exactly one snapshot and survives every live-metadata deletion; only a
-- future explicit snapshot-prune operation may remove it by deleting its
-- owning snapshot.

CREATE TABLE backup_snapshot_content_pins (
    snapshot_id UUID NOT NULL,
    manifest_node_id UUID NOT NULL,
    file_version_id UUID NOT NULL,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT backup_snapshot_content_pins_snapshot_fk
        FOREIGN KEY (snapshot_id) REFERENCES backup_snapshots (id) ON DELETE CASCADE,
    CONSTRAINT backup_snapshot_content_pins_object_fk
        FOREIGN KEY (object_id, object_dedup_domain_id)
        REFERENCES objects (id, dedup_domain_id) ON DELETE RESTRICT,
    -- One pin is owned by each captured manifest content row. FileVersion IDs
    -- are globally unique, so this independently rejects replay duplication.
    PRIMARY KEY (snapshot_id, manifest_node_id),
    CONSTRAINT backup_snapshot_content_pins_snapshot_version_unique
        UNIQUE (snapshot_id, file_version_id)
);

-- Every purge/planning/physical-GC reference recheck is keyed by this exact
-- canonical Object identity. Snapshot ID remains in the index for bounded
-- ownership inspection and a future snapshot-prune cascade.
CREATE INDEX backup_snapshot_content_pins_object_reference_idx
    ON backup_snapshot_content_pins (object_id, object_dedup_domain_id, snapshot_id);

-- Make the BUILDING-only pin gate irreversible. This mirrors the closed
-- domain lifecycle and prevents raw SQL from reopening a committed owner in
-- order to create or alter retention ownership again.
CREATE FUNCTION synveil_enforce_backup_snapshot_state_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.state = NEW.state
       OR (OLD.state = 'BUILDING' AND NEW.state IN ('COMPLETED', 'FAILED'))
       OR (OLD.state = 'COMPLETED' AND NEW.state = 'EXPIRED') THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'backup snapshot lifecycle transition is not allowed'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_state_transition_forbidden';
END;
$$;

CREATE TRIGGER backup_snapshots_enforce_state_transition
BEFORE UPDATE OF state
ON backup_snapshots
FOR EACH ROW
EXECUTE FUNCTION synveil_enforce_backup_snapshot_state_transition();

-- A pin is created only as part of a still-building capture. Check the
-- complete logical-to-physical mapping at this boundary so malformed rows do
-- not reach the completion aggregate trigger as its first line of defense.
CREATE FUNCTION synveil_require_backup_pin_from_building_snapshot()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    snapshot_state TEXT;
    expected_file_version_id UUID;
    expected_object_id UUID;
    expected_dedup_domain_id UUID;
BEGIN
    SELECT state
      INTO snapshot_state
      FROM backup_snapshots
     WHERE id = NEW.snapshot_id
     FOR SHARE;

    IF snapshot_state IS NULL OR snapshot_state <> 'BUILDING' THEN
        RAISE EXCEPTION 'backup snapshot content pins may only be inserted while snapshot is BUILDING'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_content_pin_requires_building_snapshot';
    END IF;

    SELECT manifest.current_version_id,
           version.object_id,
           version.object_dedup_domain_id
      INTO expected_file_version_id, expected_object_id, expected_dedup_domain_id
      FROM backup_snapshot_nodes AS manifest
      INNER JOIN file_versions AS version
        ON version.id = manifest.current_version_id
     WHERE manifest.snapshot_id = NEW.snapshot_id
       AND manifest.node_id = NEW.manifest_node_id
       AND manifest.kind = 'FILE'
       AND manifest.current_version_id IS NOT NULL
     FOR SHARE;

    IF NOT FOUND
       OR NEW.file_version_id IS DISTINCT FROM expected_file_version_id
       OR NEW.object_id IS DISTINCT FROM expected_object_id
       OR NEW.object_dedup_domain_id IS DISTINCT FROM expected_dedup_domain_id THEN
        RAISE EXCEPTION 'backup snapshot content pin does not match manifest content'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_content_pin_manifest_mapping';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_content_pins_require_building_manifest_mapping
BEFORE INSERT
ON backup_snapshot_content_pins
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_pin_from_building_snapshot();

-- A completed capture remains historically committed when its lifecycle later
-- becomes EXPIRED. Preserve that commit evidence; expiry is not a rollback or
-- a deletion permission. This forward-only correction aligns the durable
-- shape with the Prompt 41 domain transition `Completed -> Expired`.
ALTER TABLE backup_snapshots
    DROP CONSTRAINT backup_snapshots_commit_shape,
    ADD CONSTRAINT backup_snapshots_commit_shape
        CHECK (
            (state IN ('COMPLETED', 'EXPIRED') AND committed_at IS NOT NULL)
            OR (state IN ('BUILDING', 'FAILED') AND committed_at IS NULL)
        );

-- A backup capture is a durable-reference creation just like a FileVersion
-- writer. Serialize it with physical-GC's Object-row lifecycle fence: if GC
-- already won and made the Object non-referenceable, capture fails and its
-- transaction rolls back rather than completing with reclaimed content.
CREATE FUNCTION synveil_require_backup_pin_referenceable_object()
RETURNS trigger
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

    IF current_lifecycle IS NULL OR current_lifecycle <> 'AVAILABLE' THEN
        RAISE EXCEPTION 'object is not referenceable for backup retention'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'backup_snapshot_content_pin_requires_available_object';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshot_content_pins_require_referenceable_object
BEFORE INSERT OR UPDATE OF object_id, object_dedup_domain_id
ON backup_snapshot_content_pins
FOR EACH ROW
EXECUTE FUNCTION synveil_require_backup_pin_referenceable_object();

-- Pins have no in-place update protocol. This keeps identity, mapping, and
-- creation evidence append-only even while a snapshot is BUILDING; capture
-- creates each complete row once and completion makes the set durable.
CREATE FUNCTION synveil_reject_backup_pin_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'backup snapshot content pins are append-only'
        USING ERRCODE = '23000',
              CONSTRAINT = 'backup_snapshot_content_pin_update_forbidden';
END;
$$;

CREATE TRIGGER backup_snapshot_content_pins_append_only
BEFORE UPDATE
ON backup_snapshot_content_pins
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_backup_pin_update();

-- Direct release is never a current operation. A cascade caused by deleting
-- an owning BUILDING/FAILED snapshot runs after the parent row is gone and is
-- therefore allowed through this boundary. Terminal snapshot deletion is
-- separately blocked below; a future pruning migration can replace that
-- explicit protocol without changing this FK's ON DELETE CASCADE semantics.
CREATE FUNCTION synveil_reject_direct_backup_pin_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM backup_snapshots
        WHERE id = OLD.snapshot_id
    ) THEN
        RAISE EXCEPTION 'backup snapshot content pins may only be released by snapshot pruning'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_content_pin_delete_forbidden';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER backup_snapshot_content_pins_reject_direct_delete
BEFORE DELETE
ON backup_snapshot_content_pins
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_direct_backup_pin_delete();

-- Preserve the pin set for committed snapshots. The existing cascade FK is
-- intentionally retained for a later explicit pruning migration/protocol;
-- ordinary current SQL cannot reach it for COMPLETED or EXPIRED snapshots.
CREATE FUNCTION synveil_reject_committed_backup_snapshot_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.state IN ('COMPLETED', 'EXPIRED') THEN
        RAISE EXCEPTION 'committed backup snapshots cannot be deleted before explicit pruning'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'backup_snapshot_committed_delete_forbidden';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER backup_snapshots_reject_committed_delete
BEFORE DELETE
ON backup_snapshots
FOR EACH ROW
EXECUTE FUNCTION synveil_reject_committed_backup_snapshot_delete();

-- Completion is the one state transition that makes a logical snapshot
-- durable. Defend the capture invariant at the database boundary as well as
-- in the service: every manifest content row must have exactly one matching
-- private pin and every pin must be sourced from that manifest's FileVersion.
CREATE FUNCTION synveil_require_completed_backup_snapshot_pins()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    manifest_content_count BIGINT;
    pin_count BIGINT;
BEGIN
    IF OLD.state = 'BUILDING' AND NEW.state = 'COMPLETED' THEN
        SELECT count(*)
          INTO manifest_content_count
          FROM backup_snapshot_nodes
         WHERE snapshot_id = NEW.id
           AND current_version_id IS NOT NULL;

        SELECT count(*)
          INTO pin_count
          FROM backup_snapshot_content_pins
         WHERE snapshot_id = NEW.id;

        IF manifest_content_count <> NEW.content_reference_count
           OR pin_count <> NEW.content_reference_count THEN
            RAISE EXCEPTION 'completed backup snapshot pin count does not match manifest'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'backup_snapshot_completed_pin_count';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM backup_snapshot_nodes AS manifest
            LEFT JOIN backup_snapshot_content_pins AS pin
              ON pin.snapshot_id = manifest.snapshot_id
             AND pin.manifest_node_id = manifest.node_id
            LEFT JOIN file_versions AS version
              ON version.id = manifest.current_version_id
            WHERE manifest.snapshot_id = NEW.id
              AND manifest.current_version_id IS NOT NULL
              AND (
                  pin.snapshot_id IS NULL
                  OR pin.file_version_id <> manifest.current_version_id
                  OR version.id IS NULL
                  OR pin.object_id <> version.object_id
                  OR pin.object_dedup_domain_id <> version.object_dedup_domain_id
              )
        ) OR EXISTS (
            SELECT 1
            FROM backup_snapshot_content_pins AS pin
            LEFT JOIN backup_snapshot_nodes AS manifest
              ON manifest.snapshot_id = pin.snapshot_id
             AND manifest.node_id = pin.manifest_node_id
            WHERE pin.snapshot_id = NEW.id
              AND (
                  manifest.node_id IS NULL
                  OR manifest.current_version_id IS NULL
                  OR manifest.current_version_id <> pin.file_version_id
              )
        ) THEN
            RAISE EXCEPTION 'completed backup snapshot pin does not match manifest content'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'backup_snapshot_completed_pin_mapping';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER backup_snapshots_require_content_pins_before_completion
BEFORE UPDATE OF state, content_reference_count
ON backup_snapshots
FOR EACH ROW
EXECUTE FUNCTION synveil_require_completed_backup_snapshot_pins();
