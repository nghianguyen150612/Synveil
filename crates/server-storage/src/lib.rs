//! Managed server object-storage selection and bootstrap.
//!
//! This crate owns the read-only inspection, explicit confirmation,
//! resumable initialization, and existing-only runtime boundary for
//! `SERVER_OBJECT_DATA`. It does not own client library synchronization.

mod capacity;
mod identity;
mod model;
mod path;
mod wizard;

pub use identity::ServerStorageIdentity;
pub use model::{
    CurrentStorageStatus, StorageCapacity, StorageLocationView, StorageProductStatus,
    StorageRemovability, StorageRootClassification,
};
pub use wizard::{
    StorageExclusionSet, StorageLocationWizard, StorageSelectionPlan, StorageSetupError,
    open_existing_managed_local_storage,
};
