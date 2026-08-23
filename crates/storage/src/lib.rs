#![forbid(unsafe_code)]

//! Storage composition boundary for Synveil.
//!
//! This crate will connect reviewed application contracts to metadata and
//! object-store ports in later phases. It intentionally contains no filesystem
//! access, object mutation, upload handling, or product behavior.

pub use synveil_core;
pub use synveil_metadata as metadata;
pub use synveil_object_store as object_store;
pub use synveil_object_store::{
    ByteRange, ByteStream, CapabilityEvidence, CapabilitySupport, DeleteOutcome,
    IntegrityExpectation, ObjectKey, ObjectKeyError, ObjectMetadata, ObjectRead, ObjectStore,
    ObjectStoreError, ObjectVersion, OpaqueTokenError, PromotionReceipt, PutRequest,
    StagedMetadata, StagingHandle, StorageAvailability, StorageBackendKind, StorageCapabilities,
    StorageCapability, StorageCapabilityDiscovery, StorageDiscovery, boxed_stream,
};
