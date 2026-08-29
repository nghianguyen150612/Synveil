//! Platform-neutral contracts for one-way device synchronization progress.
//!
//! A checkpoint records only consumer progress for one authenticated owner's
//! registered device and one library. It contains no journal payload and is
//! never a mutation-authority token.

use crate::{DeviceId, LibraryId, Sequence, Timestamp, UserId};

/// Durable progress for one device/library consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceSyncCheckpoint {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    journal_epoch: Sequence,
    acknowledged_sequence: Sequence,
    created_at: Timestamp,
    updated_at: Timestamp,
    last_seen_high_watermark: Option<Sequence>,
}

impl DeviceSyncCheckpoint {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        journal_epoch: Sequence,
        acknowledged_sequence: Sequence,
        created_at: Timestamp,
        updated_at: Timestamp,
        last_seen_high_watermark: Option<Sequence>,
    ) -> Self {
        Self {
            owner_user_id,
            device_id,
            library_id,
            journal_epoch,
            acknowledged_sequence,
            created_at,
            updated_at,
            last_seen_high_watermark,
        }
    }

    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn acknowledged_sequence(self) -> Sequence {
        self.acknowledged_sequence
    }

    #[must_use]
    pub const fn created_at(self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(self) -> Timestamp {
        self.updated_at
    }

    #[must_use]
    pub const fn last_seen_high_watermark(self) -> Option<Sequence> {
        self.last_seen_high_watermark
    }
}
