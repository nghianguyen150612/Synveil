#![forbid(unsafe_code)]

//! Storage composition boundary for Synveil.
//!
//! This crate connects reviewed application contracts to metadata and
//! object-store ports. Production filesystem behavior remains in an explicit
//! local adapter; no API handler or domain type performs filesystem I/O.

mod local;
mod uploads;

pub use synveil_core;
pub use synveil_metadata as metadata;
pub use synveil_object_store as object_store;
pub use synveil_object_store::{
    ByteRange, ByteStream, CapabilityEvidence, CapabilitySupport, DeleteOutcome,
    IntegrityExpectation, ObjectKey, ObjectKeyError, ObjectMetadata, ObjectRead, ObjectStore,
    ObjectStoreError, ObjectVersion, OpaqueTokenError, PromotionReceipt, PutRequest,
    StagedMetadata, StagingHandle, StagingProgress, StorageAvailability, StorageBackendKind,
    StorageCapabilities, StorageCapability, StorageCapabilityDiscovery, StorageDiscovery,
    boxed_stream,
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
