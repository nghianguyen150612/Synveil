-- Metadata-only object-GC planning leases.
--
-- This migration extends the Prompt 26 candidate table only. It does not
-- delete Object rows, ObjectReplica rows, or object bytes, and it introduces
-- no physical-storage or synchronization behavior.

ALTER TABLE object_gc_candidates
    ADD COLUMN state TEXT NOT NULL DEFAULT 'ELIGIBLE',
    ADD COLUMN lease_id UUID,
    ADD COLUMN lease_generation NUMERIC NOT NULL DEFAULT 0,
    ADD COLUMN lease_acquired_at TIMESTAMPTZ(6),
    ADD COLUMN lease_expires_at TIMESTAMPTZ(6),
    ADD COLUMN validated_at TIMESTAMPTZ(6);

ALTER TABLE object_gc_candidates
    ADD CONSTRAINT object_gc_candidates_state_value
        CHECK (state IN ('ELIGIBLE', 'LEASED', 'READY')),
    ADD CONSTRAINT object_gc_candidates_lease_generation_u64
        CHECK (
            lease_generation >= 0
            AND lease_generation = trunc(lease_generation)
            AND lease_generation <= 18446744073709551615::NUMERIC
        ),
    ADD CONSTRAINT object_gc_candidates_lease_lifecycle_shape
        CHECK (
            (
                state = 'ELIGIBLE'
                AND lease_id IS NULL
                AND lease_acquired_at IS NULL
                AND lease_expires_at IS NULL
                AND validated_at IS NULL
            )
            OR (
                state IN ('LEASED', 'READY')
                AND lease_id IS NOT NULL
                AND lease_id <> '00000000-0000-0000-0000-000000000000'::UUID
                AND lease_acquired_at IS NOT NULL
                AND lease_expires_at IS NOT NULL
                AND lease_expires_at > lease_acquired_at
                AND validated_at IS NOT NULL
            )
        );

-- Claims use the same deterministic order as the worker query. The partial
-- indexes keep mature eligibility and expired-lease recovery bounded without
-- creating a general-purpose object deletion surface.
CREATE INDEX object_gc_candidates_eligible_claim_idx
    ON object_gc_candidates (unreferenced_at ASC, object_id ASC,
                             object_dedup_domain_id ASC)
    WHERE source = 'METADATA_PURGE' AND state = 'ELIGIBLE';

CREATE INDEX object_gc_candidates_expired_lease_idx
    ON object_gc_candidates (lease_expires_at ASC, object_id ASC,
                             object_dedup_domain_id ASC)
    WHERE source = 'METADATA_PURGE' AND state IN ('LEASED', 'READY');
