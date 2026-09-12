-- Prompt 86: bounded journal retention and durable snapshot handoff proofs.
--
-- `libraries.minimum_retained_sequence` already is the current-epoch durable
-- retention watermark established by the journal foundation. Prompt 86 locks
-- its exact meaning to the highest sequence physically compacted through. A
-- cursor exactly at this value remains valid because feed reads use
-- `sequence > cursor`; a lower cursor requires rebaseline.
COMMENT ON COLUMN libraries.minimum_retained_sequence IS
    'Highest current-journal-epoch sequence physically compacted through; cursors below it require rebaseline';

-- The proof deliberately has no foreign key to rebaseline_snapshots: it must
-- survive deletion of the large transfer payload. Library deletion may remove
-- both journal and proof because no future handoff can target an absent live
-- library. The deadline is immutable and bounded at thirty days after payload
-- expiry for Gen 1.
CREATE TABLE rebaseline_snapshot_handoff_proofs (
    snapshot_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    library_id UUID NOT NULL,
    journal_epoch BIGINT NOT NULL,
    snapshot_resume_sequence BIGINT NOT NULL,
    snapshot_created_at TIMESTAMPTZ(6) NOT NULL,
    snapshot_expires_at TIMESTAMPTZ(6) NOT NULL,
    proof_expires_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT rebaseline_snapshot_handoff_proofs_library_owner_fk
        FOREIGN KEY (library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE CASCADE,
    CONSTRAINT rebaseline_snapshot_handoff_proofs_epoch_positive
        CHECK (journal_epoch > 0),
    CONSTRAINT rebaseline_snapshot_handoff_proofs_resume_nonnegative
        CHECK (snapshot_resume_sequence >= 0),
    CONSTRAINT rebaseline_snapshot_handoff_proofs_snapshot_expiry_after_creation
        CHECK (snapshot_expires_at > snapshot_created_at),
    CONSTRAINT rebaseline_snapshot_handoff_proofs_proof_expiry_after_snapshot
        CHECK (proof_expires_at > snapshot_expires_at)
);

-- The oldest compatible proof boundary is the journal-compaction pin.
CREATE INDEX rebaseline_handoff_proofs_library_epoch_boundary_idx
    ON rebaseline_snapshot_handoff_proofs
        (library_id, journal_epoch, snapshot_resume_sequence);

-- Bounded proof cleanup scans the oldest deadline and then opaque ID.
CREATE INDEX rebaseline_handoff_proofs_expiry_idx
    ON rebaseline_snapshot_handoff_proofs (proof_expires_at, snapshot_id);

-- Bounded payload cleanup scans the oldest expired artifact and opaque ID.
CREATE INDEX rebaseline_snapshots_expiry_idx
    ON rebaseline_snapshots (expires_at, id);

-- Backfill every Prompt 82 durable snapshot before cleanup becomes possible.
-- Applying this migration therefore cannot make an existing artifact unable to
-- complete the Prompt 85 checkpoint handoff.
INSERT INTO rebaseline_snapshot_handoff_proofs
    (snapshot_id, owner_user_id, library_id, journal_epoch,
     snapshot_resume_sequence, snapshot_created_at, snapshot_expires_at,
     proof_expires_at)
SELECT id, owner_user_id, library_id, journal_epoch,
       snapshot_resume_sequence, created_at, expires_at,
       expires_at + INTERVAL '30 days'
FROM rebaseline_snapshots;

-- Proof identity, scope, boundary, and retention deadline never mutate.
-- Direct deletion is reserved for the transaction-scoped internal retention
-- primitive. Nested library deletion remains available for referential cleanup.
CREATE FUNCTION synveil_reject_rebaseline_handoff_proof_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'rebaseline snapshot handoff proofs are immutable'
            USING ERRCODE = '23000',
                  CONSTRAINT = 'rebaseline_snapshot_handoff_proofs_immutable';
    END IF;
    IF current_setting('synveil.retention_cleanup', true) = 'on'
       OR pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'rebaseline snapshot handoff proofs cannot be deleted directly'
        USING ERRCODE = '23000',
              CONSTRAINT = 'rebaseline_snapshot_handoff_proofs_delete_forbidden';
END;
$$;

CREATE TRIGGER rebaseline_snapshot_handoff_proofs_no_mutation
    BEFORE UPDATE OR DELETE ON rebaseline_snapshot_handoff_proofs
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_rebaseline_handoff_proof_mutation();

-- Prompt 31 rejected every physical journal delete. Prompt 86 retains the
-- append-only rule except inside one explicitly marked, transaction-local,
-- internal cleanup operation. The service advances the durable floor in the
-- same transaction and verifies the exact contiguous row count.
CREATE OR REPLACE FUNCTION synveil_reject_change_journal_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND current_setting('synveil.retention_cleanup', true) = 'on' THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'change_journal is append-only outside retention cleanup';
END;
$$;

-- Prompt 82 rejected direct payload deletion. Retention may now delete an
-- expired header only after verifying that its independent proof exists;
-- entry deletion continues to happen atomically through the existing cascade.
CREATE OR REPLACE FUNCTION synveil_reject_rebaseline_snapshot_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF current_setting('synveil.retention_cleanup', true) = 'on'
       OR pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'rebaseline snapshots cannot be deleted directly'
        USING ERRCODE = '23000',
              CONSTRAINT = 'rebaseline_snapshots_delete_forbidden';
END;
$$;
