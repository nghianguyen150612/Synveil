use std::path::PathBuf;

/// Finite inspection result for a selected SERVER_OBJECT_DATA root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageRootClassification {
    NewCandidate,
    EmptyDirectory,
    MatchingManagedStorage,
    InterruptedMatchingSetup,
    ForeignManagedStorage,
    RootIdentityMismatch,
    LegacyObjectStoreNeedsReview,
    NonEmptyUnknownDirectory,
    Unavailable,
    ReadOnly,
    WrongType,
    SymlinkOrRedirected,
    PermissionUnsafe,
    InsufficientCapacity,
    CapacityUnknown,
    UnsupportedFilesystemState,
    MalformedStorageIdentity,
    ObjectStoreLayoutNeedsRepair,
    MountRootNeedsDedicatedChild,
    UnsupportedPlatform,
}

/// User-facing state. Platform and filesystem mechanics stay out of this
/// vocabulary so P036 can present the result without exposing syscalls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageProductStatus {
    Recommended,
    Available,
    ReadyToUse,
    AlreadyConfigured,
    NotEnoughSpace,
    ReadOnly,
    FolderAlreadyContainsData,
    BelongsToAnotherServer,
    Disconnected,
    NeedsAttention,
    UnsupportedLocation,
    ChooseDedicatedFolder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageCapacity {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub available_files: Option<u64>,
}

/// Whether the platform can reliably identify removable storage media.
/// Prompt032's Linux adapter reports `Unknown` rather than infer a value from
/// a path name or mount label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageRemovability {
    Removable,
    NonRemovable,
    Unknown,
}

/// UI-neutral, non-authoritative inspection view. The selected path is shown
/// only because it is product-relevant; no directory entries are included.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageLocationView {
    pub selected_location: PathBuf,
    pub classification: StorageRootClassification,
    pub status: StorageProductStatus,
    pub capacity: Option<StorageCapacity>,
    pub removability: StorageRemovability,
    pub proposed_dedicated_location: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CurrentStorageStatus {
    NotConfigured,
    Preparing(StorageLocationView),
    Configured(StorageLocationView),
}
