-- Bounded durable scheduling and intervention state for the internal GC
-- worker. This migration contains no storage paths, no public control plane,
-- and no physical deletion: Prompt 28 remains the only execution boundary.

ALTER TABLE object_gc_operations
    DROP CONSTRAINT object_gc_operations_state_value;

ALTER TABLE object_gc_operations
    ADD CONSTRAINT object_gc_operations_state_value
        CHECK (state IN (
            'ACTIVE', 'RECOVERY_REQUIRED', 'NEEDS_ATTENTION', 'COMPLETED'
        ));

ALTER TABLE object_gc_replica_actions
    ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN next_attempt_at TIMESTAMPTZ(6);

ALTER TABLE object_gc_replica_actions
    ADD CONSTRAINT object_gc_replica_actions_attempt_count_nonnegative
        CHECK (attempt_count >= 0),
    ADD CONSTRAINT object_gc_replica_actions_retry_schedule_shape
        CHECK (
            next_attempt_at IS NULL
            OR (
                state IN ('RETRYABLE', 'RECONCILIATION_REQUIRED')
                AND last_attempt_at IS NOT NULL
                AND next_attempt_at >= last_attempt_at
            )
        );

-- Existing `object_gc_replica_actions_next_idx` preserves deterministic
-- per-operation ordering. These partial indexes support bounded oldest-first
-- recovery and due-retry discovery without a full metadata scan.
CREATE INDEX object_gc_operations_recovery_idx
    ON object_gc_operations (updated_at ASC, operation_id ASC)
    WHERE state IN ('ACTIVE', 'RECOVERY_REQUIRED');

CREATE INDEX object_gc_replica_actions_retry_schedule_idx
    ON object_gc_replica_actions (next_attempt_at ASC, operation_id ASC, ordinal ASC)
    WHERE state IN ('RETRYABLE', 'RECONCILIATION_REQUIRED');
