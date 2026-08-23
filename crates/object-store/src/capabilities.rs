use std::{collections::BTreeMap, path::Path};

/// Storage behavior that an adapter may prove without changing the portable
/// correctness path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StorageCapability {
    Reflink,
    BlockClone,
    CopyOnWriteClone,
    NativeSnapshot,
    Compression,
    Checksumming,
    SparseFiles,
    AtomicRename,
    DurableFsync,
    RangeReads,
    FilesystemHealth,
    AtomicPromotion,
    ExclusiveCreate,
    DurableFlush,
    ReadAfterWrite,
    ConditionalDelete,
}

impl StorageCapability {
    pub const ALL: [Self; 16] = [
        Self::Reflink,
        Self::BlockClone,
        Self::CopyOnWriteClone,
        Self::NativeSnapshot,
        Self::Compression,
        Self::Checksumming,
        Self::SparseFiles,
        Self::AtomicRename,
        Self::DurableFsync,
        Self::RangeReads,
        Self::FilesystemHealth,
        Self::AtomicPromotion,
        Self::ExclusiveCreate,
        Self::DurableFlush,
        Self::ReadAfterWrite,
        Self::ConditionalDelete,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reflink => "reflink",
            Self::BlockClone => "block_clone",
            Self::CopyOnWriteClone => "copy_on_write_clone",
            Self::NativeSnapshot => "native_snapshot",
            Self::Compression => "compression",
            Self::Checksumming => "checksumming",
            Self::SparseFiles => "sparse_files",
            Self::AtomicRename => "atomic_rename",
            Self::DurableFsync => "durable_fsync",
            Self::RangeReads => "range_reads",
            Self::FilesystemHealth => "filesystem_health",
            Self::AtomicPromotion => "atomic_promotion",
            Self::ExclusiveCreate => "exclusive_create",
            Self::DurableFlush => "durable_flush",
            Self::ReadAfterWrite => "read_after_write",
            Self::ConditionalDelete => "conditional_delete",
        }
    }
}

/// Evidence state for one capability. Unknown is different from unsupported.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

/// Evidence boundary for a capability report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityEvidence {
    NotProbed,
    ReadOnlyPathInspection,
    AdapterProbe { version: u16 },
    ConformanceTested { version: u16 },
}

/// Kind of backend associated with a capability report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageBackendKind {
    LocalFilesystem,
    ObjectStore,
    Unknown,
}

/// Safe, non-destructive availability result for a storage location.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageAvailability {
    Available,
    Missing,
    NotDirectory,
    Inaccessible,
    NotProbed,
}

/// Evidence-backed storage capabilities. No capability is inferred from an
/// operating-system enum or from a filesystem name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageCapabilities {
    backend: StorageBackendKind,
    availability: StorageAvailability,
    evidence: CapabilityEvidence,
    capabilities: BTreeMap<StorageCapability, CapabilitySupport>,
}

impl Default for StorageCapabilities {
    fn default() -> Self {
        Self::unknown()
    }
}

impl StorageCapabilities {
    #[must_use]
    pub fn unknown() -> Self {
        Self::for_location(
            StorageBackendKind::Unknown,
            StorageAvailability::NotProbed,
            CapabilityEvidence::NotProbed,
        )
    }

    #[must_use]
    pub fn for_location(
        backend: StorageBackendKind,
        availability: StorageAvailability,
        evidence: CapabilityEvidence,
    ) -> Self {
        let capabilities = StorageCapability::ALL
            .into_iter()
            .map(|capability| (capability, CapabilitySupport::Unknown))
            .collect();

        Self {
            backend,
            availability,
            evidence,
            capabilities,
        }
    }

    #[must_use]
    pub const fn backend(&self) -> StorageBackendKind {
        self.backend
    }

    #[must_use]
    pub const fn availability(&self) -> StorageAvailability {
        self.availability
    }

    #[must_use]
    pub const fn evidence(&self) -> CapabilityEvidence {
        self.evidence
    }

    #[must_use]
    pub fn support(&self, capability: StorageCapability) -> CapabilitySupport {
        self.capabilities
            .get(&capability)
            .copied()
            .unwrap_or(CapabilitySupport::Unknown)
    }

    #[must_use]
    pub fn supports(&self, capability: StorageCapability) -> bool {
        self.support(capability) == CapabilitySupport::Supported
    }

    #[must_use]
    pub fn with_support(
        mut self,
        capability: StorageCapability,
        support: CapabilitySupport,
    ) -> Self {
        self.capabilities.insert(capability, support);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = (StorageCapability, CapabilitySupport)> + '_ {
        self.capabilities
            .iter()
            .map(|(capability, support)| (*capability, *support))
    }
}

/// A storage-capability discovery port. Platform/storage adapters implement
/// this without making the portable object-store contract depend on an OS.
pub trait StorageCapabilityDiscovery: Send + Sync {
    /// Inspect a path without creating, deleting, or modifying anything.
    fn discover(&self, path: &Path) -> StorageCapabilities;
}

/// Compatibility name for callers that use the shorter discovery term.
pub use StorageCapabilityDiscovery as StorageDiscovery;

#[cfg(test)]
mod tests {
    use super::{
        CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
        StorageCapabilities, StorageCapability,
    };

    #[test]
    fn capabilities_are_unknown_until_evidence_sets_them() {
        let unknown = StorageCapabilities::unknown();

        assert_eq!(unknown.backend(), StorageBackendKind::Unknown);
        assert_eq!(unknown.availability(), StorageAvailability::NotProbed);
        assert_eq!(unknown.evidence(), CapabilityEvidence::NotProbed);
        assert_eq!(
            unknown.support(StorageCapability::AtomicPromotion),
            CapabilitySupport::Unknown
        );
        assert!(!unknown.supports(StorageCapability::AtomicPromotion));

        let tested = StorageCapabilities::for_location(
            StorageBackendKind::ObjectStore,
            StorageAvailability::Available,
            CapabilityEvidence::ConformanceTested { version: 1 },
        )
        .with_support(StorageCapability::RangeReads, CapabilitySupport::Supported)
        .with_support(
            StorageCapability::NativeSnapshot,
            CapabilitySupport::Unsupported,
        );

        assert!(tested.supports(StorageCapability::RangeReads));
        assert_eq!(
            tested.support(StorageCapability::NativeSnapshot),
            CapabilitySupport::Unsupported
        );
    }

    #[test]
    fn capability_names_are_stable_and_backend_neutral() {
        assert_eq!(
            StorageCapability::ExclusiveCreate.as_str(),
            "exclusive_create"
        );
        assert_eq!(StorageCapability::DurableFlush.as_str(), "durable_flush");
        assert_eq!(StorageCapability::Reflink.as_str(), "reflink");
    }
}
