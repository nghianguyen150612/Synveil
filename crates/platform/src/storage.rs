use std::{fs, io::ErrorKind, path::Path};

pub use synveil_object_store::{
    CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
    StorageCapabilities, StorageCapability, StorageCapabilityDiscovery, StorageDiscovery,
};

/// Minimal portable discovery. It only checks whether the selected path is an
/// accessible directory; all filesystem accelerators remain unknown.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadOnlyStorageDiscovery;

impl StorageCapabilityDiscovery for ReadOnlyStorageDiscovery {
    fn discover(&self, path: &Path) -> StorageCapabilities {
        let availability = match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => StorageAvailability::Available,
            Ok(_) => StorageAvailability::NotDirectory,
            Err(error) if error.kind() == ErrorKind::NotFound => StorageAvailability::Missing,
            Err(_) => StorageAvailability::Inaccessible,
        };

        StorageCapabilities::for_location(
            StorageBackendKind::LocalFilesystem,
            availability,
            CapabilityEvidence::ReadOnlyPathInspection,
        )
    }
}
