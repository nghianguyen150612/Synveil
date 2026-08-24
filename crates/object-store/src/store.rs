use async_trait::async_trait;
use bytes::Bytes;

use crate::{
    ByteRange, ByteStream, DeleteOutcome, IntegrityExpectation, ObjectKey, ObjectMetadata,
    ObjectRead, ObjectStoreError, ObjectVersion, PromotionReceipt, PutRequest, StagedMetadata,
    StagingHandle, StagingProgress, StorageCapabilities,
};

/// Replaceable, backend-neutral binary object store port.
///
/// All byte transfer methods use streams. Implementations own the temporary
/// write, verification, promotion, and backend-specific durability details;
/// application code must not call a filesystem or provider SDK directly.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Return evidence-backed capabilities. Unknown is not supported.
    fn capabilities(&self) -> StorageCapabilities;

    /// Convenience create-only write. Implementations must not overwrite an
    /// existing immutable key.
    async fn put(&self, request: PutRequest) -> Result<ObjectMetadata, ObjectStoreError>;

    /// Allocate an opaque staging target without exposing a physical locator.
    async fn begin_staged_write(&self) -> Result<StagingHandle, ObjectStoreError>;

    /// Stream and verify bytes into an existing staging target.
    async fn write_staged(
        &self,
        handle: &StagingHandle,
        body: ByteStream,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError>;

    /// Append one bounded chunk at an explicit offset to a still-open staging
    /// handle. The adapter must compare the offset against its durable current
    /// length before writing; it must never silently fill a gap or duplicate a
    /// replayed chunk.
    async fn append_staged(
        &self,
        handle: &StagingHandle,
        expected_offset: u64,
        chunk: Bytes,
        maximum_length: u64,
    ) -> Result<StagingProgress, ObjectStoreError>;

    /// Inspect durable staging progress after reconnect or process restart.
    /// The result contains only an opaque handle and length/checksum evidence.
    async fn staging_progress(
        &self,
        handle: &StagingHandle,
    ) -> Result<StagingProgress, ObjectStoreError>;

    /// Verify and freeze an append-built staging handle so it can be promoted.
    /// Repeating this operation with the same expectation is idempotent.
    async fn finalize_staged(
        &self,
        handle: &StagingHandle,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError>;

    /// Promote verified staged bytes to a never-overwritten final key.
    async fn promote_temp(
        &self,
        handle: &StagingHandle,
        destination: &ObjectKey,
    ) -> Result<PromotionReceipt, ObjectStoreError>;

    /// Abort a known staging handle. Repeating this operation is safe.
    async fn abort_staged(&self, handle: &StagingHandle) -> Result<(), ObjectStoreError>;

    /// Stream one complete immutable object.
    async fn get(&self, key: &ObjectKey) -> Result<ObjectRead, ObjectStoreError>;

    /// Stream one logical plaintext range without returning representation
    /// bytes under a misleading file-offset label.
    async fn range_read(
        &self,
        key: &ObjectKey,
        range: ByteRange,
    ) -> Result<ObjectRead, ObjectStoreError>;

    /// Check authorized existence without exposing backend/provider details.
    async fn exists(&self, key: &ObjectKey) -> Result<bool, ObjectStoreError>;

    /// Read immutable object metadata/head evidence.
    async fn metadata(&self, key: &ObjectKey) -> Result<ObjectMetadata, ObjectStoreError>;

    /// Delete a server-authorized key. Absence is idempotent success.
    async fn delete(&self, key: &ObjectKey) -> Result<DeleteOutcome, ObjectStoreError>;

    /// Delete only when the caller supplies the current opaque backend version.
    async fn conditional_delete(
        &self,
        key: &ObjectKey,
        expected_version: &ObjectVersion,
    ) -> Result<DeleteOutcome, ObjectStoreError>;
}
