#![forbid(unsafe_code)]

//! Backend-neutral object-store boundary for Synveil.
//!
//! The staged-write, immutable-read, capability, integrity, and lifecycle
//! contract is specified by the repository ADRs and storage documentation. No
//! storage adapter or mutation behavior is implemented here yet.

mod capabilities;
mod errors;
mod keys;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use capabilities::{
    CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
    StorageCapabilities, StorageCapability, StorageCapabilityDiscovery, StorageDiscovery,
};
pub use errors::ObjectStoreError;
pub use keys::{ObjectKey, ObjectKeyError, ObjectVersion, OpaqueTokenError, StagingHandle};
pub use store::ObjectStore;
pub use types::{
    ByteRange, ByteStream, DeleteOutcome, IntegrityExpectation, ObjectMetadata, ObjectRead,
    PromotionReceipt, PutRequest, StagedMetadata, boxed_stream,
};

pub use synveil_core;
