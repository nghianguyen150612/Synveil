#![forbid(unsafe_code)]

//! Backend-neutral object-store boundary for Synveil.
//!
//! The staged-write, immutable-read, capability, integrity, and lifecycle
//! contract is specified by the repository ADRs and storage documentation.
//! Production adapters remain outside this crate; the first local-filesystem
//! implementation lives behind the `synveil-storage` composition boundary.

mod capabilities;
#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod conformance;
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
    ByteRange, ByteStream, DeleteOutcome, DeleteReconciliation, IntegrityExpectation,
    ObjectMetadata, ObjectRead, PromotionReceipt, PutRequest, StagedMetadata, StagingProgress,
    boxed_stream,
};

pub use synveil_core;
