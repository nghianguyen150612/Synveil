#![forbid(unsafe_code)]

//! Storage composition boundary for Synveil.
//!
//! This crate connects reviewed application contracts to metadata and
//! object-store ports. Production filesystem behavior remains in an explicit
//! local adapter; no API handler or domain type performs filesystem I/O.

mod downloads;
mod gc;
mod gc_worker;
mod local;
mod uploads;

pub use synveil_core;
pub use synveil_metadata as metadata;
pub use synveil_object_store as object_store;
pub use synveil_object_store::{
    ByteRange, ByteStream, CapabilityEvidence, CapabilitySupport, DeleteOutcome,
    DeleteReconciliation, IntegrityExpectation, ObjectKey, ObjectKeyError, ObjectMetadata,
    ObjectRead, ObjectStore, ObjectStoreError, ObjectVersion, OpaqueTokenError, PromotionReceipt,
    PutRequest, StagedMetadata, StagingHandle, StagingProgress, StorageAvailability,
    StorageBackendKind, StorageCapabilities, StorageCapability, StorageCapabilityDiscovery,
    StorageDiscovery, boxed_stream,
};

pub use downloads::{
    ContentByteStream, ContentDescriptor, ContentMetadata, ContentReadApplicationService,
    ContentReadError, boxed_content_stream,
};
pub use gc::{
    ObjectGcExecutionConfigurationError, ObjectGcExecutionError, ObjectGcExecutionService,
    ObjectGcExecutionStep, ObjectGcStepOutcome,
};
pub use gc_worker::{
    GcWorker, GcWorkerBackend, GcWorkerCycleReport, GcWorkerCycleStatus, GcWorkerError,
    GcWorkerLimits, GcWorkerWaitOutcome, GcWorkerWorkReport,
};
pub use local::LocalFilesystemObjectStore;
pub use uploads::{
    CapacityAdmission, CreateUploadSessionRequest, NoCapacityAdmission, UploadApplicationService,
    UploadByteStream, UploadCleanupWork, UploadConfigurationError, UploadError, UploadLimits,
    UploadProgress, UploadSessionView, UploadTargetRequest, UploadTargetView, boxed_upload_stream,
};

use std::path::Path;

/// Construct the local adapter from an already-resolved storage root.
///
/// Platform path discovery and product configuration stay outside this crate;
/// this function never selects a default directory.
pub fn open_local_object_store(
    root: impl AsRef<Path>,
) -> Result<LocalFilesystemObjectStore, ObjectStoreError> {
    LocalFilesystemObjectStore::open(root)
}
