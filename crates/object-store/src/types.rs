use std::{fmt, pin::Pin};

use bytes::Bytes;
use futures_core::Stream;
use synveil_core::Sha256Digest;

use crate::{ObjectKey, ObjectStoreError, ObjectVersion, StagingHandle};

/// A bounded chunk stream. Implementations must not require callers to buffer
/// an entire object before sending or receiving it.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ObjectStoreError>> + Send + 'static>>;

/// Box a runtime-neutral byte stream for an object-store call.
pub fn boxed_stream<S>(stream: S) -> ByteStream
where
    S: Stream<Item = Result<Bytes, ObjectStoreError>> + Send + 'static,
{
    Box::pin(stream)
}

/// Optional caller-supplied integrity expectations for a streamed write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IntegrityExpectation {
    expected_length: Option<u64>,
    expected_sha256: Option<Sha256Digest>,
}

impl IntegrityExpectation {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            expected_length: None,
            expected_sha256: None,
        }
    }

    #[must_use]
    pub const fn with_length(mut self, expected_length: u64) -> Self {
        self.expected_length = Some(expected_length);
        self
    }

    #[must_use]
    pub const fn with_sha256(mut self, expected_sha256: Sha256Digest) -> Self {
        self.expected_sha256 = Some(expected_sha256);
        self
    }

    #[must_use]
    pub const fn expected_length(self) -> Option<u64> {
        self.expected_length
    }

    #[must_use]
    pub const fn expected_sha256(self) -> Option<Sha256Digest> {
        self.expected_sha256
    }
}

/// A create-only streamed write request. Existing immutable objects are never
/// overwritten by this contract.
pub struct PutRequest {
    key: ObjectKey,
    body: ByteStream,
    integrity: IntegrityExpectation,
}

impl PutRequest {
    #[must_use]
    pub fn new(key: ObjectKey, body: ByteStream) -> Self {
        Self {
            key,
            body,
            integrity: IntegrityExpectation::none(),
        }
    }

    #[must_use]
    pub fn with_integrity(mut self, integrity: IntegrityExpectation) -> Self {
        self.integrity = integrity;
        self
    }

    #[must_use]
    pub fn key(&self) -> &ObjectKey {
        &self.key
    }

    #[must_use]
    pub const fn integrity(&self) -> IntegrityExpectation {
        self.integrity
    }

    #[must_use]
    pub fn into_parts(self) -> (ObjectKey, ByteStream, IntegrityExpectation) {
        (self.key, self.body, self.integrity)
    }
}

/// A validated range of logical plaintext bytes, using an exclusive end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    start: u64,
    end_exclusive: u64,
}

impl ByteRange {
    pub fn new(start: u64, end_exclusive: u64) -> Result<Self, ObjectStoreError> {
        if start >= end_exclusive {
            return Err(ObjectStoreError::InvalidRange);
        }
        Ok(Self {
            start,
            end_exclusive,
        })
    }

    pub fn from_start_length(start: u64, length: u64) -> Result<Self, ObjectStoreError> {
        let end_exclusive = start
            .checked_add(length)
            .ok_or(ObjectStoreError::InvalidRange)?;
        Self::new(start, end_exclusive)
    }

    #[must_use]
    pub const fn start(self) -> u64 {
        self.start
    }

    #[must_use]
    pub const fn end_exclusive(self) -> u64 {
        self.end_exclusive
    }

    #[must_use]
    pub const fn length(self) -> u64 {
        self.end_exclusive - self.start
    }
}

/// Backend metadata returned by `metadata`, `get`, and promotion.
#[derive(Clone, Eq, PartialEq)]
pub struct ObjectMetadata {
    key: ObjectKey,
    length: u64,
    sha256: Option<Sha256Digest>,
    version: Option<ObjectVersion>,
}

impl fmt::Debug for ObjectMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectMetadata")
            .field("key", &self.key)
            .field("length", &self.length)
            .field("sha256", &self.sha256.as_ref().map(|_| "<redacted>"))
            .field("version", &self.version)
            .finish()
    }
}

impl ObjectMetadata {
    #[must_use]
    pub fn new(
        key: ObjectKey,
        length: u64,
        sha256: Option<Sha256Digest>,
        version: Option<ObjectVersion>,
    ) -> Self {
        Self {
            key,
            length,
            sha256,
            version,
        }
    }

    #[must_use]
    pub const fn key(&self) -> &ObjectKey {
        &self.key
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(&self) -> Option<&Sha256Digest> {
        self.sha256.as_ref()
    }

    #[must_use]
    pub const fn version(&self) -> Option<&ObjectVersion> {
        self.version.as_ref()
    }
}

/// Metadata for bytes that are durable in staging but not yet visible as a
/// final object.
#[derive(Clone, Eq, PartialEq)]
pub struct StagedMetadata {
    handle: StagingHandle,
    length: u64,
    sha256: Option<Sha256Digest>,
}

/// Restart-safe progress for a staging handle. A partial append has a
/// durable length but no canonical checksum until the staging object is
/// finalized; a verified handle carries both.
#[derive(Clone, Eq, PartialEq)]
pub struct StagingProgress {
    handle: StagingHandle,
    length: u64,
    sha256: Option<Sha256Digest>,
    verified: bool,
}

impl fmt::Debug for StagingProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagingProgress")
            .field("handle", &self.handle)
            .field("length", &self.length)
            .field("sha256", &self.sha256.as_ref().map(|_| "<redacted>"))
            .field("verified", &self.verified)
            .finish()
    }
}

impl StagingProgress {
    #[must_use]
    pub const fn partial(handle: StagingHandle, length: u64) -> Self {
        Self {
            handle,
            length,
            sha256: None,
            verified: false,
        }
    }

    #[must_use]
    pub const fn verified(handle: StagingHandle, length: u64, sha256: Sha256Digest) -> Self {
        Self {
            handle,
            length,
            sha256: Some(sha256),
            verified: true,
        }
    }

    #[must_use]
    pub const fn handle(&self) -> &StagingHandle {
        &self.handle
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(&self) -> Option<&Sha256Digest> {
        self.sha256.as_ref()
    }

    #[must_use]
    pub const fn is_verified(&self) -> bool {
        self.verified
    }
}

impl fmt::Debug for StagedMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagedMetadata")
            .field("handle", &self.handle)
            .field("length", &self.length)
            .field("sha256", &self.sha256.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl StagedMetadata {
    #[must_use]
    pub fn new(handle: StagingHandle, length: u64, sha256: Option<Sha256Digest>) -> Self {
        Self {
            handle,
            length,
            sha256,
        }
    }

    #[must_use]
    pub const fn handle(&self) -> &StagingHandle {
        &self.handle
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(&self) -> Option<&Sha256Digest> {
        self.sha256.as_ref()
    }
}

/// Evidence returned after a staging handle is promoted to a create-only final
/// key. The application layer decides when this evidence is sufficient for a
/// PostgreSQL `VERIFIED` transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromotionReceipt {
    metadata: ObjectMetadata,
}

impl PromotionReceipt {
    #[must_use]
    pub fn new(metadata: ObjectMetadata) -> Self {
        Self { metadata }
    }

    #[must_use]
    pub const fn metadata(&self) -> &ObjectMetadata {
        &self.metadata
    }
}

/// A streamed immutable read. The optional range identifies the logical
/// portion represented by the stream; the stream itself remains bounded.
pub struct ObjectRead {
    metadata: ObjectMetadata,
    range: Option<ByteRange>,
    body: ByteStream,
}

impl ObjectRead {
    #[must_use]
    pub fn new(metadata: ObjectMetadata, range: Option<ByteRange>, body: ByteStream) -> Self {
        Self {
            metadata,
            range,
            body,
        }
    }

    #[must_use]
    pub const fn metadata(&self) -> &ObjectMetadata {
        &self.metadata
    }

    #[must_use]
    pub const fn range(&self) -> Option<ByteRange> {
        self.range
    }

    #[must_use]
    pub fn into_stream(self) -> ByteStream {
        self.body
    }
}

/// Result of an idempotent or conditional delete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteOutcome {
    Deleted,
    AlreadyAbsent,
}

/// Read-only reconciliation evidence for a previously attempted delete.
///
/// `InProgress` is distinct from `Absent`: an adapter has durable evidence of
/// a backend-owned partial deletion/tombstone that still needs a fenced retry.
/// When immutable metadata survives, it is returned so callers can re-check
/// the exact key/hash/length/version before continuing. `None` is allowed only
/// for a terminal tombstone whose content and visibility marker are already
/// absent. Callers must not remove replica metadata for either form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeleteReconciliation {
    Absent,
    Present(ObjectMetadata),
    InProgress(Option<ObjectMetadata>),
}
