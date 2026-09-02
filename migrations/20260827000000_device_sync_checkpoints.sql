-- Durable per-device, per-library consumer progress for the one-way sync
-- transport. This migration deliberately stores checkpoint state only; journal
-- payloads and physical storage identities remain in their canonical tables.

ALTER TABLE devices
    ADD CONSTRAINT devices_owner_pair_unique
        UNIQUE (id, owner_user_id);

CREATE TABLE device_sync_checkpoints (
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    library_id UUID NOT NULL,
    journal_epoch BIGINT NOT NULL,
    acknowledged_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    last_seen_high_watermark BIGINT,
    CONSTRAINT device_sync_checkpoints_device_owner_fk
        FOREIGN KEY (device_id, owner_user_id)
        REFERENCES devices (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT device_sync_checkpoints_library_owner_fk
        FOREIGN KEY (library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT device_sync_checkpoints_epoch_positive
        CHECK (journal_epoch > 0),
    CONSTRAINT device_sync_checkpoints_ack_nonnegative
        CHECK (acknowledged_sequence >= 0),
    CONSTRAINT device_sync_checkpoints_last_seen_nonnegative
        CHECK (last_seen_high_watermark IS NULL OR last_seen_high_watermark >= 0),
    CONSTRAINT device_sync_checkpoints_scope_unique
        PRIMARY KEY (device_id, library_id)
);

CREATE INDEX device_sync_checkpoints_owner_library_idx
    ON device_sync_checkpoints (owner_user_id, library_id, device_id);

CREATE INDEX device_sync_checkpoints_library_progress_idx
    ON device_sync_checkpoints (library_id, journal_epoch, acknowledged_sequence);
