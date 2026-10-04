use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt as _;
use synveil_server_config::{
    ConfigFingerprint, ConfigInspection, ConfigStoreError, ServerConfig, ServerConfigStore,
    StorageConfiguration, StorageId, StorageRootIdentity,
};
use synveil_storage::{
    CapabilitySupport, LocalFilesystemObjectStore, ObjectStore, ObjectStoreError,
    StorageCapabilities, StorageCapability, initialize_local_object_store_root,
    open_existing_local_object_store_with_capabilities,
};
use uuid::Uuid;

use crate::{
    capacity::{CapacityReport, inspect_capacity},
    identity::{MARKER_NAME, MarkerError, initialize_identity, read_identity, verify_identity},
    model::{
        CurrentStorageStatus, StorageLocationView, StorageProductStatus, StorageRemovability,
        StorageRootClassification,
    },
    path::{
        CandidatePath, PathIdentity, PathInspectionError, inspect_candidate, metadata_device,
        metadata_inode, validate_root_exclusions,
    },
};

const RECOMMENDED_STORAGE_ROOT: &str = "/var/lib/synveil/storage";
const LOCAL_STORE_MARKER: &str = ".synveil-storage-root";
const LOCAL_STORE_MARKER_CONTENT: &[u8] = b"synveil-local-object-store-v1\n";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StorageExclusionSet {
    roots: Vec<PathBuf>,
}

impl StorageExclusionSet {
    #[must_use]
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            roots: roots.into_iter().collect(),
        }
    }

    pub fn add_root(&mut self, root: impl Into<PathBuf>) {
        self.roots.push(root.into());
    }

    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
}

/// A reviewable, in-memory plan. SERVER_CONFIG remains the only durable setup
/// authority; this plan expires whenever its config or filesystem evidence
/// changes.
#[derive(Clone, Debug)]
pub struct StorageSelectionPlan {
    store_fingerprint: ConfigFingerprint,
    config_generation: u64,
    server_installation_id: String,
    config_storage: StorageConfiguration,
    candidate: CandidatePath,
    view: StorageLocationView,
    required_bytes: Option<u64>,
}

impl StorageSelectionPlan {
    #[must_use]
    pub const fn review(&self) -> &StorageLocationView {
        &self.view
    }

    #[must_use]
    pub const fn required_bytes(&self) -> Option<u64> {
        self.required_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageSetupError {
    UnsupportedPlatform,
    ConfigurationUnavailable,
    ConfigurationNotReady,
    InvalidLocation,
    StalePlan,
    NeedsReview,
    IdentityConflict,
    StorageUnavailable,
    ReadOnly,
    PermissionUnsafe,
    InsufficientCapacity,
    CapacityUnknown,
    UnsupportedFilesystemState,
    StorageRelocationRequiresMigration,
    PendingSelectionConflict,
    SetupInProgress,
    NeedsRepair,
    OutcomeUnknown,
    ObjectStoreUnavailable,
    ConfigurationWriteFailed,
}

impl std::fmt::Display for StorageSetupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedPlatform => {
                "managed server storage is not yet qualified on this platform"
            }
            Self::ConfigurationUnavailable => "managed server configuration is unavailable",
            Self::ConfigurationNotReady => "server storage configuration is not ready",
            Self::InvalidLocation => "the selected storage location is not supported",
            Self::StalePlan => "storage changed while this choice was being reviewed",
            Self::NeedsReview => "this location needs review before Synveil can use it",
            Self::IdentityConflict => "this location belongs to a different Synveil server",
            Self::StorageUnavailable => "the configured server storage is unavailable",
            Self::ReadOnly => "the selected location is read-only",
            Self::PermissionUnsafe => "the selected location does not have safe permissions",
            Self::InsufficientCapacity => {
                "the selected location does not have enough available space"
            }
            Self::CapacityUnknown => {
                "Synveil could not determine available space for this location"
            }
            Self::UnsupportedFilesystemState => "the selected filesystem state is not supported",
            Self::StorageRelocationRequiresMigration => {
                "moving server data requires a separate migration"
            }
            Self::PendingSelectionConflict => "another storage setup is already in progress",
            Self::SetupInProgress => "storage setup is already running",
            Self::NeedsRepair => "server storage needs attention before it can be used",
            Self::OutcomeUnknown => {
                "storage setup outcome is unknown; inspect the current server storage state"
            }
            Self::ObjectStoreUnavailable => "the server storage layout could not be verified",
            Self::ConfigurationWriteFailed => "server storage configuration could not be updated",
        })
    }
}

impl std::error::Error for StorageSetupError {}

#[derive(Clone)]
pub struct StorageLocationWizard {
    store: ServerConfigStore,
    exclusions: StorageExclusionSet,
}

struct ConfigSnapshot {
    config: ServerConfig,
    fingerprint: ConfigFingerprint,
}

impl StorageLocationWizard {
    #[must_use]
    pub fn new(store: ServerConfigStore, exclusions: StorageExclusionSet) -> Self {
        Self { store, exclusions }
    }

    #[must_use]
    pub fn suggested_location(&self) -> PathBuf {
        PathBuf::from(RECOMMENDED_STORAGE_ROOT)
    }

    /// Read-only inspection. This does not create the candidate directory,
    /// either marker, ObjectStore layout, or a storage ID.
    pub fn inspect_location(&self, path: &Path) -> Result<StorageLocationView, StorageSetupError> {
        let snapshot = self.snapshot()?;
        self.inspect_with_config(path, &snapshot.config, None)
    }

    /// Build a confirmation plan bound to the current config fingerprint,
    /// selected root identity, classification, and capacity snapshot.
    pub fn prepare_selection(
        &self,
        path: &Path,
        required_bytes: Option<u64>,
    ) -> Result<StorageSelectionPlan, StorageSetupError> {
        if !cfg!(target_os = "linux") {
            return Err(StorageSetupError::UnsupportedPlatform);
        }
        let snapshot = self.snapshot()?;
        let (candidate, view) =
            self.inspect_candidate_with_config(path, &snapshot.config, required_bytes)?;
        let selected_root = candidate_root_string(&candidate)?;
        match &snapshot.config.storage {
            StorageConfiguration::ConfiguredLocal { root, .. } if root != &selected_root => {
                return Err(StorageSetupError::StorageRelocationRequiresMigration);
            }
            StorageConfiguration::PreparingLocal { root, .. } if root != &selected_root => {
                return Err(StorageSetupError::PendingSelectionConflict);
            }
            _ => {}
        }
        if !matches!(
            view.classification,
            StorageRootClassification::NewCandidate
                | StorageRootClassification::EmptyDirectory
                | StorageRootClassification::MatchingManagedStorage
                | StorageRootClassification::InterruptedMatchingSetup
        ) {
            return Err(error_for_classification(view.classification));
        }
        if view.status == StorageProductStatus::ChooseDedicatedFolder {
            return Err(StorageSetupError::NeedsReview);
        }
        Ok(StorageSelectionPlan {
            store_fingerprint: snapshot.fingerprint,
            config_generation: snapshot.config.generation,
            server_installation_id: snapshot.config.server_installation_id.clone(),
            config_storage: snapshot.config.storage.clone(),
            candidate,
            view,
            required_bytes,
        })
    }

    /// Explicit confirmation boundary. The method re-inspects all plan inputs
    /// before its first durable effect, records PreparingLocal, and reconciles
    /// unknown acknowledgements against SERVER_CONFIG.
    pub fn confirm_selection(
        &self,
        plan: StorageSelectionPlan,
    ) -> Result<CurrentStorageStatus, StorageSetupError> {
        if !cfg!(target_os = "linux") {
            return Err(StorageSetupError::UnsupportedPlatform);
        }
        let _lock = acquire_setup_lock(&plan.candidate.parent)?;
        let current = self.snapshot()?;
        if current.fingerprint != plan.store_fingerprint
            || current.config.generation != plan.config_generation
            || current.config.server_installation_id != plan.server_installation_id
            || current.config.storage != plan.config_storage
        {
            return Err(StorageSetupError::StalePlan);
        }
        let (fresh_candidate, fresh_view) = self.inspect_candidate_with_config(
            &plan.candidate.requested,
            &current.config,
            plan.required_bytes,
        )?;
        if fresh_candidate != plan.candidate
            || fresh_view.classification != plan.view.classification
        {
            return Err(StorageSetupError::StalePlan);
        }

        match &current.config.storage {
            StorageConfiguration::ConfiguredLocal { root, .. } => {
                if root != &candidate_root_string(&plan.candidate)? {
                    return Err(StorageSetupError::StorageRelocationRequiresMigration);
                }
                let _store = open_existing_managed_local_storage(&current.config)?;
                let view = ready_view(&plan.candidate, true);
                return Ok(CurrentStorageStatus::Configured(view));
            }
            StorageConfiguration::NotConfigured => {
                if !matches!(
                    plan.view.classification,
                    StorageRootClassification::NewCandidate
                        | StorageRootClassification::EmptyDirectory
                ) {
                    return Err(StorageSetupError::NeedsReview);
                }
            }
            StorageConfiguration::PreparingLocal { root, .. } => {
                if root != &candidate_root_string(&plan.candidate)? {
                    return Err(StorageSetupError::PendingSelectionConflict);
                }
            }
        }

        let (storage_id, preparing_config) = self.ensure_preparing_state(&current, &plan)?;
        let (capabilities, root_identity) =
            self.initialize_selected_root(&plan.candidate, &preparing_config, storage_id.clone())?;
        let configured = self.commit_configured_state(
            &plan.candidate,
            storage_id.clone(),
            root_identity,
            capabilities.clone(),
        )?;
        let _verified_store = open_existing_managed_local_storage(&configured)?;
        Ok(CurrentStorageStatus::Configured(ready_view(
            &plan.candidate,
            false,
        )))
    }

    /// Resume only the durable root and storage ID already recorded as
    /// PreparingLocal. A different root or ID is never generated here.
    pub fn resume_pending_selection(&self) -> Result<CurrentStorageStatus, StorageSetupError> {
        let snapshot = self.snapshot()?;
        match &snapshot.config.storage {
            StorageConfiguration::NotConfigured => Err(StorageSetupError::ConfigurationNotReady),
            StorageConfiguration::ConfiguredLocal { root, .. } => {
                let view = self.inspect_with_config(Path::new(root), &snapshot.config, None)?;
                if view.classification != StorageRootClassification::MatchingManagedStorage {
                    return Err(error_for_classification(view.classification));
                }
                let _store = open_existing_managed_local_storage(&snapshot.config)?;
                let candidate = inspect_candidate(Path::new(root)).map_err(error_for_path)?;
                Ok(CurrentStorageStatus::Configured(ready_view(
                    &candidate, true,
                )))
            }
            StorageConfiguration::PreparingLocal { root, .. } => {
                let plan = self.prepare_selection(Path::new(root), None)?;
                self.confirm_selection(plan)
            }
        }
    }

    pub fn current_storage_status(&self) -> Result<CurrentStorageStatus, StorageSetupError> {
        let snapshot = self.snapshot()?;
        match &snapshot.config.storage {
            StorageConfiguration::NotConfigured => Ok(CurrentStorageStatus::NotConfigured),
            StorageConfiguration::PreparingLocal { root, .. } => {
                Ok(CurrentStorageStatus::Preparing(self.inspect_with_config(
                    Path::new(root),
                    &snapshot.config,
                    None,
                )?))
            }
            StorageConfiguration::ConfiguredLocal { root, .. } => {
                let mut view = self.inspect_with_config(Path::new(root), &snapshot.config, None)?;
                if view.classification == StorageRootClassification::MatchingManagedStorage {
                    view.status = StorageProductStatus::AlreadyConfigured;
                }
                Ok(CurrentStorageStatus::Configured(view))
            }
        }
    }

    fn snapshot(&self) -> Result<ConfigSnapshot, StorageSetupError> {
        match self.store.load_config_read_only() {
            Ok(Some((config, fingerprint))) => Ok(ConfigSnapshot {
                config,
                fingerprint,
            }),
            Ok(None) => Err(StorageSetupError::ConfigurationUnavailable),
            Err(ConfigInspection::Absent) => Err(StorageSetupError::ConfigurationUnavailable),
            Err(_) => Err(StorageSetupError::ConfigurationNotReady),
        }
    }

    fn inspect_with_config(
        &self,
        path: &Path,
        config: &ServerConfig,
        required_bytes: Option<u64>,
    ) -> Result<StorageLocationView, StorageSetupError> {
        self.inspect_candidate_with_config(path, config, required_bytes)
            .map(|(_, view)| view)
    }

    fn inspect_candidate_with_config(
        &self,
        path: &Path,
        config: &ServerConfig,
        required_bytes: Option<u64>,
    ) -> Result<(CandidatePath, StorageLocationView), StorageSetupError> {
        if !cfg!(target_os = "linux") {
            return Ok((
                CandidatePath {
                    requested: path.to_path_buf(),
                    canonical: path.to_path_buf(),
                    parent: path.parent().unwrap_or(Path::new("/")).to_path_buf(),
                    exists: false,
                    identity: PathIdentity {
                        device: 0,
                        inode: 0,
                        parent_device: 0,
                        parent_inode: 0,
                    },
                    is_mount_point: false,
                },
                view(
                    path,
                    StorageRootClassification::UnsupportedPlatform,
                    None,
                    None,
                ),
            ));
        }
        let candidate = match inspect_candidate(path) {
            Ok(candidate) => candidate,
            Err(error) => {
                return Ok((
                    CandidatePath {
                        requested: path.to_path_buf(),
                        canonical: path.to_path_buf(),
                        parent: path.parent().unwrap_or(Path::new("/")).to_path_buf(),
                        exists: false,
                        identity: PathIdentity {
                            device: 0,
                            inode: 0,
                            parent_device: 0,
                            parent_inode: 0,
                        },
                        is_mount_point: false,
                    },
                    view(path, error.classification(), None, None),
                ));
            }
        };
        let mut reserved = vec![
            self.store.layout().root().to_path_buf(),
            self.store.layout().credential_directory(),
            self.store.layout().config_path(),
        ];
        reserved.extend(self.exclusions.roots().iter().cloned());
        if validate_root_exclusions(&candidate.canonical, &reserved).is_err() {
            return Ok((
                candidate.clone(),
                view(
                    path,
                    StorageRootClassification::UnsupportedFilesystemState,
                    None,
                    None,
                ),
            ));
        }
        if candidate.is_mount_point {
            let capacity = inspect_capacity(&candidate.canonical).ok();
            let classification = classify_mount_root(capacity, required_bytes);
            return Ok((
                candidate.clone(),
                view(
                    path,
                    classification,
                    capacity.map(|report| report.capacity),
                    Some(candidate.canonical.join("Synveil")),
                ),
            ));
        }
        let initial_classification = classify_candidate(&candidate, config);
        let configured_root_missing = matches!(
            &config.storage,
            StorageConfiguration::ConfiguredLocal { root, .. }
                if root == &candidate_root_string(&candidate)? && !candidate.exists
        );
        let capacity = if configured_root_missing {
            None
        } else {
            let capacity_path = if candidate.exists {
                candidate.canonical.as_path()
            } else {
                candidate.parent.as_path()
            };
            inspect_capacity(capacity_path).ok()
        };
        let classification = qualify_capacity(initial_classification, capacity, required_bytes);
        let view = view(
            path,
            classification,
            capacity.map(|report| report.capacity),
            None,
        );
        Ok((candidate, view))
    }

    fn ensure_preparing_state(
        &self,
        initial: &ConfigSnapshot,
        plan: &StorageSelectionPlan,
    ) -> Result<(StorageId, ServerConfig), StorageSetupError> {
        let root = candidate_root_string(&plan.candidate)?;
        match &initial.config.storage {
            StorageConfiguration::PreparingLocal {
                root: current_root,
                storage_id,
                ..
            } if current_root == &root => Ok((storage_id.clone(), initial.config.clone())),
            StorageConfiguration::NotConfigured => {
                let storage_id = StorageId::new_v7();
                let root_identity = durable_root_identity(&plan.candidate);
                match self.store.begin_storage_preparation(
                    initial.fingerprint,
                    root,
                    storage_id.clone(),
                    root_identity,
                ) {
                    Ok(config) => Ok((storage_id, config)),
                    Err(ConfigStoreError::OutcomeUnknown) => {
                        let snapshot = self.snapshot()?;
                        match &snapshot.config.storage {
                            StorageConfiguration::PreparingLocal {
                                root: current_root,
                                storage_id: current_id,
                                ..
                            } if current_root == &candidate_root_string(&plan.candidate)?
                                && current_id == &storage_id =>
                            {
                                Ok((storage_id, snapshot.config))
                            }
                            _ => Err(StorageSetupError::OutcomeUnknown),
                        }
                    }
                    Err(ConfigStoreError::ConcurrentModification) => {
                        let latest = self.snapshot()?;
                        match &latest.config.storage {
                            StorageConfiguration::PreparingLocal {
                                root: latest_root,
                                storage_id: latest_id,
                                ..
                            }
                            | StorageConfiguration::ConfiguredLocal {
                                root: latest_root,
                                storage_id: latest_id,
                                ..
                            } if latest_root == &candidate_root_string(&plan.candidate)? => {
                                Ok((latest_id.clone(), latest.config))
                            }
                            StorageConfiguration::PreparingLocal { .. }
                            | StorageConfiguration::ConfiguredLocal { .. } => {
                                Err(StorageSetupError::PendingSelectionConflict)
                            }
                            StorageConfiguration::NotConfigured => {
                                Err(StorageSetupError::StalePlan)
                            }
                        }
                    }
                    Err(ConfigStoreError::StorageStateConflict) => {
                        Err(StorageSetupError::PendingSelectionConflict)
                    }
                    Err(_) => Err(StorageSetupError::ConfigurationWriteFailed),
                }
            }
            StorageConfiguration::ConfiguredLocal { .. } => {
                Err(StorageSetupError::StorageRelocationRequiresMigration)
            }
            StorageConfiguration::PreparingLocal { .. } => {
                Err(StorageSetupError::PendingSelectionConflict)
            }
        }
    }
}

impl StorageLocationWizard {
    fn initialize_selected_root(
        &self,
        planned: &CandidatePath,
        config: &ServerConfig,
        storage_id: StorageId,
    ) -> Result<(StorageCapabilities, StorageRootIdentity), StorageSetupError> {
        let root = planned.canonical.as_path();
        let root_string = candidate_root_string(planned)?;
        let expected_identity = match &config.storage {
            StorageConfiguration::PreparingLocal {
                root: configured_root,
                storage_id: configured_id,
                root_identity,
            }
            | StorageConfiguration::ConfiguredLocal {
                root: configured_root,
                storage_id: configured_id,
                root_identity,
                ..
            } if configured_root == &root_string && configured_id == &storage_id => {
                root_identity.clone()
            }
            _ => return Err(StorageSetupError::IdentityConflict),
        };
        let fresh = inspect_candidate(&planned.requested).map_err(error_for_path)?;
        if fresh.canonical != planned.canonical
            || fresh.identity.parent_device != planned.identity.parent_device
            || fresh.identity.parent_inode != planned.identity.parent_inode
        {
            return Err(StorageSetupError::StalePlan);
        }
        if let Err(error) = validate_root_exclusions(root, &self.exclusion_roots()) {
            return Err(error_for_path(error));
        }

        if let StorageConfiguration::ConfiguredLocal { .. } = &config.storage {
            let current_identity = durable_root_identity(&fresh);
            if !same_directory_identity(&expected_identity, &current_identity) {
                return Err(StorageSetupError::IdentityConflict);
            }
            let object_store = open_existing_managed_local_storage(config)?;
            return Ok((object_store.capabilities(), current_identity));
        }

        let root_candidate = match &expected_identity {
            StorageRootIdentity::Directory { device, inode } => {
                if !fresh.exists
                    || fresh.identity.device != *device
                    || fresh.identity.inode != *inode
                {
                    return Err(StorageSetupError::IdentityConflict);
                }
                match read_identity(root) {
                    Ok(_) => {
                        verify_identity(root, config, &storage_id).map_err(error_for_marker)?
                    }
                    Err(MarkerError::Absent) if is_directory_empty(root)? => {
                        initialize_identity(root, config, storage_id.clone())
                            .map_err(error_for_marker)?;
                    }
                    Err(MarkerError::Absent) => return Err(StorageSetupError::NeedsReview),
                    Err(error) => return Err(error_for_marker(error)),
                }
                fresh
            }
            StorageRootIdentity::MissingLeaf {
                parent_device,
                parent_inode,
            } => {
                if fresh.identity.parent_device != *parent_device
                    || fresh.identity.parent_inode != *parent_inode
                {
                    return Err(StorageSetupError::IdentityConflict);
                }
                if fresh.exists {
                    verify_identity(root, config, &storage_id).map_err(error_for_marker)?;
                    fresh
                } else {
                    create_managed_root_with_identity(root, &fresh, config, &storage_id)?
                }
            }
        };
        if !root_candidate.exists {
            return Err(StorageSetupError::StorageUnavailable);
        }
        verify_identity(root, config, &storage_id).map_err(error_for_marker)?;
        let directory_identity = durable_root_identity(&root_candidate);
        self.bind_directory_identity(&root_string, storage_id.clone(), directory_identity.clone())?;

        let object_store =
            initialize_local_object_store_root(root).map_err(error_for_object_store)?;
        let capabilities = object_store.capabilities();
        if capabilities.support(StorageCapability::ExclusiveCreate) != CapabilitySupport::Supported
            || capabilities.support(StorageCapability::DurableFsync) != CapabilitySupport::Supported
            || capabilities.support(StorageCapability::AtomicRename) != CapabilitySupport::Supported
        {
            return Err(StorageSetupError::UnsupportedFilesystemState);
        }
        run_write_durability_probe(root)?;
        verify_identity(root, config, &storage_id).map_err(error_for_marker)?;
        let after_probe = inspect_candidate(root).map_err(error_for_path)?;
        if !after_probe.exists
            || !same_directory_identity(&directory_identity, &durable_root_identity(&after_probe))
        {
            return Err(StorageSetupError::IdentityConflict);
        }
        verify_identity(root, config, &storage_id).map_err(error_for_marker)?;
        Ok((capabilities, directory_identity))
    }

    fn commit_configured_state(
        &self,
        candidate: &CandidatePath,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
        capabilities: StorageCapabilities,
    ) -> Result<ServerConfig, StorageSetupError> {
        let root = candidate_root_string(candidate)?;
        for _ in 0..3 {
            let snapshot = self.snapshot()?;
            match &snapshot.config.storage {
                StorageConfiguration::ConfiguredLocal {
                    root: current_root,
                    storage_id: current_id,
                    root_identity: current_identity,
                    capabilities: current_capabilities,
                } if current_root == &root
                    && current_id == &storage_id
                    && current_identity == &root_identity
                    && current_capabilities == &capabilities =>
                {
                    return Ok(snapshot.config);
                }
                StorageConfiguration::PreparingLocal {
                    root: current_root,
                    storage_id: current_id,
                    root_identity: current_identity,
                } if current_root == &root
                    && current_id == &storage_id
                    && current_identity == &root_identity =>
                {
                    match self.store.complete_storage_preparation(
                        snapshot.fingerprint,
                        root.clone(),
                        storage_id.clone(),
                        root_identity.clone(),
                        capabilities.clone(),
                    ) {
                        Ok(config) => return Ok(config),
                        Err(ConfigStoreError::ConcurrentModification) => continue,
                        Err(ConfigStoreError::OutcomeUnknown) => {
                            let after = self.snapshot()?;
                            if matches!(
                                after.config.storage,
                                StorageConfiguration::ConfiguredLocal {
                                    root: ref actual_root,
                                    storage_id: ref actual_id,
                                    root_identity: ref actual_identity,
                                    capabilities: ref actual_capabilities,
                                } if actual_root == &root
                                    && actual_id == &storage_id
                                    && actual_identity == &root_identity
                                    && actual_capabilities == &capabilities
                            ) {
                                return Ok(after.config);
                            }
                            return Err(StorageSetupError::OutcomeUnknown);
                        }
                        Err(_) => return Err(StorageSetupError::ConfigurationWriteFailed),
                    }
                }
                _ => return Err(StorageSetupError::IdentityConflict),
            }
        }
        Err(StorageSetupError::StalePlan)
    }

    fn bind_directory_identity(
        &self,
        root: &str,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
    ) -> Result<(), StorageSetupError> {
        for _ in 0..3 {
            let snapshot = self.snapshot()?;
            match &snapshot.config.storage {
                StorageConfiguration::PreparingLocal {
                    root: current_root,
                    storage_id: current_id,
                    root_identity: current_identity,
                } if current_root == root && current_id == &storage_id => {
                    if same_directory_identity(current_identity, &root_identity) {
                        return Ok(());
                    }
                    if !matches!(current_identity, StorageRootIdentity::MissingLeaf { .. }) {
                        return Err(StorageSetupError::IdentityConflict);
                    }
                    match self.store.record_storage_directory_identity(
                        snapshot.fingerprint,
                        root.to_owned(),
                        storage_id.clone(),
                        root_identity.clone(),
                    ) {
                        Ok(_) => return Ok(()),
                        Err(ConfigStoreError::ConcurrentModification) => continue,
                        Err(ConfigStoreError::OutcomeUnknown) => {
                            let after = self.snapshot()?;
                            return if matches!(
                                &after.config.storage,
                                StorageConfiguration::PreparingLocal {
                                    root: actual_root,
                                    storage_id: actual_id,
                                    root_identity: actual_identity,
                                } if actual_root == root
                                    && actual_id == &storage_id
                                    && actual_identity == &root_identity
                            ) {
                                Ok(())
                            } else {
                                Err(StorageSetupError::OutcomeUnknown)
                            };
                        }
                        Err(_) => return Err(StorageSetupError::ConfigurationWriteFailed),
                    }
                }
                StorageConfiguration::ConfiguredLocal {
                    root: current_root,
                    storage_id: current_id,
                    root_identity: current_identity,
                    ..
                } if current_root == root
                    && current_id == &storage_id
                    && current_identity == &root_identity =>
                {
                    return Ok(());
                }
                _ => return Err(StorageSetupError::IdentityConflict),
            }
        }
        Err(StorageSetupError::StalePlan)
    }

    fn exclusion_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![
            self.store.layout().root().to_path_buf(),
            self.store.layout().credential_directory(),
            self.store.layout().config_path(),
        ];
        roots.extend(self.exclusions.roots().iter().cloned());
        roots
    }
}

/// Managed API and worker runtime entry point. It verifies the durable server
/// identity, the existing LocalFilesystemObjectStore marker/layout, and never
/// creates a missing root or layout.
pub fn open_existing_managed_local_storage(
    config: &ServerConfig,
) -> Result<LocalFilesystemObjectStore, StorageSetupError> {
    if !cfg!(target_os = "linux") {
        return Err(StorageSetupError::UnsupportedPlatform);
    }
    config
        .validate()
        .map_err(|_| StorageSetupError::ConfigurationNotReady)?;
    let StorageConfiguration::ConfiguredLocal {
        root,
        storage_id,
        root_identity,
        capabilities,
    } = &config.storage
    else {
        return Err(StorageSetupError::ConfigurationNotReady);
    };
    let candidate = inspect_candidate(Path::new(root)).map_err(error_for_path)?;
    if !candidate.exists || candidate.canonical != Path::new(root) {
        return Err(StorageSetupError::StorageUnavailable);
    }
    if !same_directory_identity(root_identity, &durable_root_identity(&candidate)) {
        return Err(StorageSetupError::IdentityConflict);
    }
    validate_runtime_capacity(inspect_capacity(&candidate.canonical).ok())?;
    let mut reserved = vec![
        PathBuf::from("/etc/synveil"),
        PathBuf::from("/etc/synveil/credentials"),
        PathBuf::from("/var/lib/synveil/postgresql"),
        PathBuf::from("/run"),
    ];
    if let Some(config_file) = std::env::var_os("SYNVEIL_SERVER_CONFIG_FILE")
        && let Some(config_root) = PathBuf::from(config_file).parent()
    {
        reserved.push(config_root.to_path_buf());
        reserved.push(config_root.join("credentials"));
    }
    if let Ok(current) = std::env::current_dir() {
        let current = crate::path::canonical_if_exists(&current).unwrap_or(current);
        if candidate.canonical == current {
            return Err(StorageSetupError::InvalidLocation);
        }
    }
    if let Some(workspace) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) {
        reserved.push(workspace.to_path_buf());
    }
    if validate_root_exclusions(&candidate.canonical, &reserved).is_err() {
        return Err(StorageSetupError::InvalidLocation);
    }
    verify_identity(&candidate.canonical, config, storage_id).map_err(error_for_marker)?;
    let object_store = open_existing_local_object_store_with_capabilities(
        &candidate.canonical,
        capabilities.clone(),
    )
    .map_err(error_for_object_store)?;
    let after = inspect_candidate(Path::new(root)).map_err(error_for_path)?;
    if !after.exists || after.identity != candidate.identity {
        return Err(StorageSetupError::StorageUnavailable);
    }
    verify_identity(&after.canonical, config, storage_id).map_err(error_for_marker)?;
    Ok(object_store)
}

fn classify_candidate(
    candidate: &CandidatePath,
    config: &ServerConfig,
) -> StorageRootClassification {
    let root = match candidate_root_string(candidate) {
        Ok(root) => root,
        Err(_) => return StorageRootClassification::UnsupportedFilesystemState,
    };
    let configured = match &config.storage {
        StorageConfiguration::PreparingLocal {
            root: configured_root,
            storage_id,
            ..
        }
        | StorageConfiguration::ConfiguredLocal {
            root: configured_root,
            storage_id,
            ..
        } if configured_root == &root => Some(storage_id),
        _ => None,
    };
    let selected_identity = match &config.storage {
        StorageConfiguration::PreparingLocal {
            root: configured_root,
            root_identity,
            ..
        }
        | StorageConfiguration::ConfiguredLocal {
            root: configured_root,
            root_identity,
            ..
        } if configured_root == &root => Some(root_identity),
        _ => None,
    };

    if !candidate.exists {
        return match (&config.storage, selected_identity) {
            (
                StorageConfiguration::PreparingLocal { root: current, .. },
                Some(StorageRootIdentity::MissingLeaf {
                    parent_device,
                    parent_inode,
                }),
            ) if current == &root
                && candidate.identity.parent_device == *parent_device
                && candidate.identity.parent_inode == *parent_inode =>
            {
                StorageRootClassification::InterruptedMatchingSetup
            }
            (StorageConfiguration::PreparingLocal { root: current, .. }, Some(_))
            | (StorageConfiguration::ConfiguredLocal { root: current, .. }, Some(_))
                if current == &root =>
            {
                StorageRootClassification::Unavailable
            }
            _ => StorageRootClassification::NewCandidate,
        };
    }

    if let Some(identity) = selected_identity {
        let matches = match identity {
            StorageRootIdentity::MissingLeaf {
                parent_device,
                parent_inode,
            } => {
                candidate.identity.parent_device == *parent_device
                    && candidate.identity.parent_inode == *parent_inode
            }
            StorageRootIdentity::Directory { device, inode } => {
                candidate.identity.device == *device && candidate.identity.inode == *inode
            }
        };
        if !matches {
            return StorageRootClassification::RootIdentityMismatch;
        }
    }

    match read_identity(&candidate.canonical) {
        Ok(_) => {
            let Some(storage_id) = configured else {
                return StorageRootClassification::ForeignManagedStorage;
            };
            if verify_identity(&candidate.canonical, config, storage_id).is_err() {
                return StorageRootClassification::ForeignManagedStorage;
            }
            match &config.storage {
                StorageConfiguration::PreparingLocal { .. } => {
                    if recognized_partial_managed_layout(&candidate.canonical).unwrap_or(false) {
                        StorageRootClassification::InterruptedMatchingSetup
                    } else {
                        StorageRootClassification::ObjectStoreLayoutNeedsRepair
                    }
                }
                StorageConfiguration::ConfiguredLocal { .. } => {
                    match inspect_local_layout(&candidate.canonical) {
                        Ok(true) => StorageRootClassification::MatchingManagedStorage,
                        Ok(false) | Err(_) => {
                            StorageRootClassification::ObjectStoreLayoutNeedsRepair
                        }
                    }
                }
                StorageConfiguration::NotConfigured => {
                    StorageRootClassification::ForeignManagedStorage
                }
            }
        }
        Err(MarkerError::Absent) => {
            match &config.storage {
                StorageConfiguration::ConfiguredLocal { root: current, .. } if current == &root => {
                    return StorageRootClassification::MalformedStorageIdentity;
                }
                StorageConfiguration::PreparingLocal {
                    root: current,
                    root_identity: StorageRootIdentity::Directory { .. },
                    ..
                } if current == &root => {
                    return if is_directory_empty(&candidate.canonical).unwrap_or(false) {
                        StorageRootClassification::InterruptedMatchingSetup
                    } else {
                        StorageRootClassification::ObjectStoreLayoutNeedsRepair
                    };
                }
                StorageConfiguration::PreparingLocal { root: current, .. } if current == &root => {
                    return StorageRootClassification::RootIdentityMismatch;
                }
                _ => {}
            }
            if local_store_marker_exists(&candidate.canonical) {
                return StorageRootClassification::LegacyObjectStoreNeedsReview;
            }
            match is_directory_empty(&candidate.canonical) {
                Ok(true) => StorageRootClassification::EmptyDirectory,
                Ok(false) => StorageRootClassification::NonEmptyUnknownDirectory,
                Err(_) => StorageRootClassification::Unavailable,
            }
        }
        Err(MarkerError::Malformed) => StorageRootClassification::MalformedStorageIdentity,
        Err(MarkerError::SymlinkOrRedirected) => StorageRootClassification::SymlinkOrRedirected,
        Err(MarkerError::WrongType) => StorageRootClassification::MalformedStorageIdentity,
        Err(MarkerError::Foreign) => StorageRootClassification::ForeignManagedStorage,
        Err(MarkerError::Io) => StorageRootClassification::Unavailable,
    }
}

fn inspect_local_layout(root: &Path) -> Result<bool, StorageSetupError> {
    let marker = root.join(LOCAL_STORE_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(StorageSetupError::ObjectStoreUnavailable),
    }
    if !inspect_local_marker(&marker)? {
        return Err(StorageSetupError::NeedsRepair);
    }
    let objects = root.join("objects");
    let staging = root.join("staging");
    let layout = objects.join("v1");
    for directory in [&objects, &staging, &layout] {
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(StorageSetupError::NeedsRepair),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(StorageSetupError::ObjectStoreUnavailable),
        }
    }
    Ok(true)
}

fn recognized_partial_managed_layout(root: &Path) -> Result<bool, StorageSetupError> {
    if !entries_are_within(
        root,
        &[MARKER_NAME, LOCAL_STORE_MARKER, "objects", "staging"],
    )? {
        return Ok(false);
    }
    if read_identity(root).is_err() {
        return Ok(false);
    }
    let local_marker = root.join(LOCAL_STORE_MARKER);
    if fs::symlink_metadata(&local_marker).is_ok() && !inspect_local_marker(&local_marker)? {
        return Ok(false);
    }
    let objects = root.join("objects");
    if fs::symlink_metadata(&objects).is_ok() && !entries_are_within(&objects, &["v1"])? {
        return Ok(false);
    }
    let layout = objects.join("v1");
    if fs::symlink_metadata(&layout).is_ok() && !directory_is_empty(&layout)? {
        return Ok(false);
    }
    let staging = root.join("staging");
    if fs::symlink_metadata(&staging).is_ok() && !directory_is_empty(&staging)? {
        return Ok(false);
    }
    Ok(true)
}

fn inspect_local_marker(marker: &Path) -> Result<bool, StorageSetupError> {
    let metadata = fs::symlink_metadata(marker).map_err(|_| StorageSetupError::NeedsRepair)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 128 {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Ok(false);
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    set_no_follow(&mut options);
    let mut file = options
        .open(marker)
        .map_err(|_| StorageSetupError::NeedsRepair)?;
    let opened = file
        .metadata()
        .map_err(|_| StorageSetupError::NeedsRepair)?;
    if !opened.is_file() || !same_file(&metadata, &opened) {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(129)
        .read_to_end(&mut bytes)
        .map_err(|_| StorageSetupError::NeedsRepair)?;
    Ok(bytes == LOCAL_STORE_MARKER_CONTENT)
}

fn entries_are_within(path: &Path, allowed: &[&str]) -> Result<bool, StorageSetupError> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(_) => return Err(StorageSetupError::StorageUnavailable),
    };
    for entry in entries {
        let entry = entry.map_err(|_| StorageSetupError::StorageUnavailable)?;
        if !allowed.iter().any(|name| entry.file_name() == *name) {
            return Ok(false);
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| StorageSetupError::StorageUnavailable)?;
        if metadata.file_type().is_symlink() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn directory_is_empty(path: &Path) -> Result<bool, StorageSetupError> {
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(StorageSetupError::StorageUnavailable),
    }
}

fn local_store_marker_exists(root: &Path) -> bool {
    match fs::symlink_metadata(root.join(LOCAL_STORE_MARKER)) {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(_) => false,
    }
}

fn is_directory_empty(path: &Path) -> Result<bool, StorageSetupError> {
    let mut entries = fs::read_dir(path).map_err(|_| StorageSetupError::StorageUnavailable)?;
    Ok(entries.next().is_none())
}

fn view(
    path: &Path,
    classification: StorageRootClassification,
    capacity: Option<crate::model::StorageCapacity>,
    proposed_dedicated_location: Option<PathBuf>,
) -> StorageLocationView {
    let status = match classification {
        StorageRootClassification::NewCandidate if path == Path::new(RECOMMENDED_STORAGE_ROOT) => {
            StorageProductStatus::Recommended
        }
        StorageRootClassification::NewCandidate | StorageRootClassification::EmptyDirectory => {
            StorageProductStatus::Available
        }
        StorageRootClassification::MatchingManagedStorage => StorageProductStatus::ReadyToUse,
        StorageRootClassification::InterruptedMatchingSetup
        | StorageRootClassification::MalformedStorageIdentity
        | StorageRootClassification::ObjectStoreLayoutNeedsRepair
        | StorageRootClassification::PermissionUnsafe
        | StorageRootClassification::UnsupportedFilesystemState => {
            StorageProductStatus::NeedsAttention
        }
        StorageRootClassification::ForeignManagedStorage => {
            StorageProductStatus::BelongsToAnotherServer
        }
        StorageRootClassification::RootIdentityMismatch => StorageProductStatus::NeedsAttention,
        StorageRootClassification::LegacyObjectStoreNeedsReview
        | StorageRootClassification::NonEmptyUnknownDirectory => {
            StorageProductStatus::FolderAlreadyContainsData
        }
        StorageRootClassification::Unavailable => StorageProductStatus::Disconnected,
        StorageRootClassification::ReadOnly => StorageProductStatus::ReadOnly,
        StorageRootClassification::WrongType
        | StorageRootClassification::SymlinkOrRedirected
        | StorageRootClassification::UnsupportedPlatform => {
            StorageProductStatus::UnsupportedLocation
        }
        StorageRootClassification::InsufficientCapacity => StorageProductStatus::NotEnoughSpace,
        StorageRootClassification::CapacityUnknown => StorageProductStatus::NeedsAttention,
        StorageRootClassification::MountRootNeedsDedicatedChild => {
            StorageProductStatus::ChooseDedicatedFolder
        }
    };
    StorageLocationView {
        selected_location: path.to_path_buf(),
        classification,
        status,
        capacity,
        removability: StorageRemovability::Unknown,
        proposed_dedicated_location,
    }
}

fn ready_view(candidate: &CandidatePath, already_configured: bool) -> StorageLocationView {
    StorageLocationView {
        selected_location: candidate.requested.clone(),
        classification: StorageRootClassification::MatchingManagedStorage,
        status: if already_configured {
            StorageProductStatus::AlreadyConfigured
        } else {
            StorageProductStatus::ReadyToUse
        },
        capacity: inspect_capacity(&candidate.canonical)
            .ok()
            .map(|report| report.capacity),
        removability: StorageRemovability::Unknown,
        proposed_dedicated_location: None,
    }
}

fn candidate_root_string(candidate: &CandidatePath) -> Result<String, StorageSetupError> {
    candidate
        .canonical
        .to_str()
        .map(str::to_owned)
        .filter(|root| !root.is_empty() && root.len() <= 4096)
        .ok_or(StorageSetupError::InvalidLocation)
}

fn durable_root_identity(candidate: &CandidatePath) -> StorageRootIdentity {
    if candidate.exists {
        StorageRootIdentity::Directory {
            device: candidate.identity.device,
            inode: candidate.identity.inode,
        }
    } else {
        StorageRootIdentity::MissingLeaf {
            parent_device: candidate.identity.parent_device,
            parent_inode: candidate.identity.parent_inode,
        }
    }
}

fn same_directory_identity(left: &StorageRootIdentity, right: &StorageRootIdentity) -> bool {
    matches!(
        (left, right),
        (
            StorageRootIdentity::Directory { device: left_device, inode: left_inode },
            StorageRootIdentity::Directory { device: right_device, inode: right_inode }
        ) if left_device == right_device && left_inode == right_inode
    )
}

fn capacity_sensitive(classification: StorageRootClassification) -> bool {
    matches!(
        classification,
        StorageRootClassification::NewCandidate
            | StorageRootClassification::EmptyDirectory
            | StorageRootClassification::MatchingManagedStorage
            | StorageRootClassification::InterruptedMatchingSetup
    )
}

fn classify_mount_root(
    capacity: Option<CapacityReport>,
    required_bytes: Option<u64>,
) -> StorageRootClassification {
    match qualify_capacity(
        StorageRootClassification::NewCandidate,
        capacity,
        required_bytes,
    ) {
        StorageRootClassification::NewCandidate => {
            StorageRootClassification::MountRootNeedsDedicatedChild
        }
        classification => classification,
    }
}

fn qualify_capacity(
    classification: StorageRootClassification,
    capacity: Option<CapacityReport>,
    required_bytes: Option<u64>,
) -> StorageRootClassification {
    if !capacity_sensitive(classification) {
        return classification;
    }
    let Some(capacity) = capacity else {
        return StorageRootClassification::CapacityUnknown;
    };
    if capacity.read_only {
        return StorageRootClassification::ReadOnly;
    }
    if required_bytes.is_some_and(|required| capacity.capacity.available_bytes < required) {
        return StorageRootClassification::InsufficientCapacity;
    }
    classification
}

fn validate_runtime_capacity(capacity: Option<CapacityReport>) -> Result<(), StorageSetupError> {
    match capacity {
        Some(report) if report.read_only => Err(StorageSetupError::ReadOnly),
        Some(_) => Ok(()),
        None => Err(StorageSetupError::CapacityUnknown),
    }
}

fn error_for_classification(classification: StorageRootClassification) -> StorageSetupError {
    match classification {
        StorageRootClassification::NewCandidate
        | StorageRootClassification::EmptyDirectory
        | StorageRootClassification::MatchingManagedStorage
        | StorageRootClassification::InterruptedMatchingSetup => StorageSetupError::NeedsReview,
        StorageRootClassification::ForeignManagedStorage => StorageSetupError::IdentityConflict,
        StorageRootClassification::RootIdentityMismatch => StorageSetupError::IdentityConflict,
        StorageRootClassification::LegacyObjectStoreNeedsReview
        | StorageRootClassification::NonEmptyUnknownDirectory
        | StorageRootClassification::MalformedStorageIdentity
        | StorageRootClassification::ObjectStoreLayoutNeedsRepair => StorageSetupError::NeedsReview,
        StorageRootClassification::Unavailable => StorageSetupError::StorageUnavailable,
        StorageRootClassification::ReadOnly => StorageSetupError::ReadOnly,
        StorageRootClassification::WrongType | StorageRootClassification::SymlinkOrRedirected => {
            StorageSetupError::InvalidLocation
        }
        StorageRootClassification::PermissionUnsafe => StorageSetupError::PermissionUnsafe,
        StorageRootClassification::InsufficientCapacity => StorageSetupError::InsufficientCapacity,
        StorageRootClassification::CapacityUnknown => StorageSetupError::CapacityUnknown,
        StorageRootClassification::UnsupportedFilesystemState => {
            StorageSetupError::UnsupportedFilesystemState
        }
        StorageRootClassification::MountRootNeedsDedicatedChild => StorageSetupError::NeedsReview,
        StorageRootClassification::UnsupportedPlatform => StorageSetupError::UnsupportedPlatform,
    }
}

fn error_for_path(error: PathInspectionError) -> StorageSetupError {
    match error {
        PathInspectionError::Invalid => StorageSetupError::InvalidLocation,
        PathInspectionError::Unavailable => StorageSetupError::StorageUnavailable,
        PathInspectionError::WrongType => StorageSetupError::InvalidLocation,
        PathInspectionError::SymlinkOrRedirected => StorageSetupError::InvalidLocation,
        PathInspectionError::PermissionUnsafe => StorageSetupError::PermissionUnsafe,
        PathInspectionError::Unsupported => StorageSetupError::UnsupportedPlatform,
    }
}

fn error_for_marker(error: MarkerError) -> StorageSetupError {
    match error {
        MarkerError::Absent => StorageSetupError::NeedsRepair,
        MarkerError::Foreign => StorageSetupError::IdentityConflict,
        MarkerError::Malformed | MarkerError::WrongType => StorageSetupError::NeedsRepair,
        MarkerError::SymlinkOrRedirected => StorageSetupError::InvalidLocation,
        MarkerError::Io => StorageSetupError::StorageUnavailable,
    }
}

fn error_for_object_store(error: ObjectStoreError) -> StorageSetupError {
    match error {
        ObjectStoreError::InvalidRequest | ObjectStoreError::InvalidKey => {
            StorageSetupError::InvalidLocation
        }
        ObjectStoreError::NotFound | ObjectStoreError::StorageUnavailable => {
            StorageSetupError::StorageUnavailable
        }
        _ => StorageSetupError::ObjectStoreUnavailable,
    }
}

#[cfg(target_os = "linux")]
fn acquire_setup_lock(parent: &Path) -> Result<File, StorageSetupError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory = options
        .open(parent)
        .map_err(|_| StorageSetupError::StorageUnavailable)?;
    match directory.try_lock_exclusive() {
        Ok(()) => Ok(directory),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            Err(StorageSetupError::SetupInProgress)
        }
        Err(_) => Err(StorageSetupError::StorageUnavailable),
    }
}

#[cfg(not(target_os = "linux"))]
fn acquire_setup_lock(_parent: &Path) -> Result<File, StorageSetupError> {
    Err(StorageSetupError::UnsupportedPlatform)
}

fn create_managed_root_with_identity(
    root: &Path,
    selected: &CandidatePath,
    config: &ServerConfig,
    storage_id: &StorageId,
) -> Result<CandidatePath, StorageSetupError> {
    let parent = root.parent().ok_or(StorageSetupError::InvalidLocation)?;
    let temporary = parent.join(format!(".synveil-storage-setup-{}", storage_id.as_str()));
    let temporary_candidate = inspect_candidate(&temporary).map_err(error_for_path)?;
    if temporary_candidate.identity.parent_device != selected.identity.parent_device
        || temporary_candidate.identity.parent_inode != selected.identity.parent_inode
    {
        return Err(StorageSetupError::IdentityConflict);
    }
    if !temporary_candidate.exists {
        create_exact_directory(&temporary, &temporary_candidate)?;
        initialize_identity(&temporary, config, storage_id.clone()).map_err(error_for_marker)?;
    } else {
        match read_identity(&temporary) {
            Ok(_) => verify_identity(&temporary, config, storage_id).map_err(error_for_marker)?,
            Err(MarkerError::Absent) if is_directory_empty(&temporary)? => {
                // PreparingLocal binds this deterministic temporary root to the
                // same server/storage IDs and selected parent. If power failed
                // after mkdir but before marker creation, finishing this empty
                // known step is safe.
                initialize_identity(&temporary, config, storage_id.clone())
                    .map_err(error_for_marker)?;
            }
            Err(MarkerError::Absent) => return Err(StorageSetupError::NeedsReview),
            Err(error) => return Err(error_for_marker(error)),
        }
    }
    if !entries_are_within(&temporary, &[MARKER_NAME])? {
        return Err(StorageSetupError::NeedsRepair);
    }
    let ready_temporary = inspect_candidate(&temporary).map_err(error_for_path)?;
    if !ready_temporary.exists
        || ready_temporary.identity.parent_device != selected.identity.parent_device
        || ready_temporary.identity.parent_inode != selected.identity.parent_inode
    {
        return Err(StorageSetupError::IdentityConflict);
    }

    match rename_exclusive(
        &temporary,
        root,
        selected.identity.parent_device,
        selected.identity.parent_inode,
    ) {
        Ok(()) => {}
        Err(_) => {
            let current = inspect_candidate(root).map_err(error_for_path)?;
            if !current.exists || verify_identity(root, config, storage_id).is_err() {
                return Err(StorageSetupError::OutcomeUnknown);
            }
        }
    }
    sync_directory(parent).map_err(|_| StorageSetupError::OutcomeUnknown)?;
    let created = inspect_candidate(root).map_err(error_for_path)?;
    if !created.exists
        || created.identity.parent_device != selected.identity.parent_device
        || created.identity.parent_inode != selected.identity.parent_inode
    {
        return Err(StorageSetupError::IdentityConflict);
    }
    verify_identity(root, config, storage_id).map_err(error_for_marker)?;
    Ok(created)
}

fn create_exact_directory(path: &Path, candidate: &CandidatePath) -> Result<(), StorageSetupError> {
    let parent = path.parent().ok_or(StorageSetupError::InvalidLocation)?;
    let directory = open_directory_no_follow(parent)?;
    let parent_metadata = directory
        .metadata()
        .map_err(|_| StorageSetupError::StorageUnavailable)?;
    if metadata_device(&parent_metadata) != candidate.identity.parent_device
        || metadata_inode(&parent_metadata) != candidate.identity.parent_inode
    {
        return Err(StorageSetupError::IdentityConflict);
    }
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::fd::AsRawFd, os::unix::ffi::OsStrExt};
        let leaf = CString::new(
            path.file_name()
                .ok_or(StorageSetupError::InvalidLocation)?
                .as_bytes(),
        )
        .map_err(|_| StorageSetupError::InvalidLocation)?;
        // SAFETY: The directory fd is open with O_DIRECTORY|O_NOFOLLOW, the
        // leaf contains no NUL, and mkdirat creates only this single child.
        let result = unsafe { libc::mkdirat(directory.as_raw_fd(), leaf.as_ptr(), 0o700) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                StorageSetupError::StalePlan
            } else if error.kind() == std::io::ErrorKind::PermissionDenied {
                StorageSetupError::PermissionUnsafe
            } else {
                StorageSetupError::StorageUnavailable
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    return Err(StorageSetupError::UnsupportedPlatform);
    directory
        .sync_all()
        .map_err(|_| StorageSetupError::OutcomeUnknown)
}

fn open_directory_no_follow(path: &Path) -> Result<File, StorageSetupError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        options
            .open(path)
            .map_err(|_| StorageSetupError::StorageUnavailable)
    }
    #[cfg(not(target_os = "linux"))]
    {
        File::open(path).map_err(|_| StorageSetupError::UnsupportedPlatform)
    }
}

fn run_write_durability_probe(root: &Path) -> Result<(), StorageSetupError> {
    let directory = root.join("staging");
    let metadata = fs::symlink_metadata(&directory).map_err(|_| StorageSetupError::NeedsRepair)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StorageSetupError::NeedsRepair);
    }
    let suffix = Uuid::now_v7().simple().to_string();
    let source = directory.join(format!(".synveil-bootstrap-probe-{suffix}.tmp"));
    let destination = directory.join(format!(".synveil-bootstrap-probe-{suffix}.renamed"));
    let mut owned_identity = None;
    let result = (|| {
        let mut file = OpenOptions::new();
        file.write(true).create_new(true);
        set_create_mode(&mut file, 0o600);
        let mut handle = file
            .open(&source)
            .map_err(|_| StorageSetupError::ObjectStoreUnavailable)?;
        let original = handle
            .metadata()
            .map_err(|_| StorageSetupError::ObjectStoreUnavailable)?;
        owned_identity = Some(original.clone());
        handle
            .write_all(b"Synveil storage durability probe")
            .map_err(|_| StorageSetupError::ObjectStoreUnavailable)?;
        handle
            .sync_all()
            .map_err(|_| StorageSetupError::UnsupportedFilesystemState)?;
        sync_directory(&directory)?;
        rename_exclusive(
            &source,
            &destination,
            metadata_device(&metadata),
            metadata_inode(&metadata),
        )
        .map_err(|_| StorageSetupError::UnsupportedFilesystemState)?;
        sync_directory(&directory)?;
        let destination_metadata = fs::symlink_metadata(&destination)
            .map_err(|_| StorageSetupError::ObjectStoreUnavailable)?;
        if !same_file(&original, &destination_metadata) || !destination_metadata.is_file() {
            return Err(StorageSetupError::UnsupportedFilesystemState);
        }
        let mut read_options = OpenOptions::new();
        read_options.read(true);
        set_no_follow(&mut read_options);
        let mut reader = read_options
            .open(&destination)
            .map_err(|_| StorageSetupError::ObjectStoreUnavailable)?;
        let mut contents = Vec::new();
        Read::by_ref(&mut reader)
            .take(128)
            .read_to_end(&mut contents)
            .map_err(|_| StorageSetupError::ObjectStoreUnavailable)?;
        if contents != b"Synveil storage durability probe" {
            return Err(StorageSetupError::UnsupportedFilesystemState);
        }
        remove_exact_file(&destination, &destination_metadata)?;
        sync_directory(&directory)?;
        Ok(())
    })();
    if result.is_err()
        && let Some(identity) = owned_identity.as_ref()
    {
        for path in [&source, &destination] {
            if let Ok(metadata) = fs::symlink_metadata(path)
                && metadata.is_file()
                && !metadata.file_type().is_symlink()
                && same_file(identity, &metadata)
            {
                let _ = fs::remove_file(path);
            }
        }
        let _ = sync_directory(&directory);
    }
    result
}

#[cfg(target_os = "linux")]
fn rename_exclusive(
    source: &Path,
    destination: &Path,
    expected_parent_device: u64,
    expected_parent_inode: u64,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };

    let source_parent = source
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    if destination.parent() != Some(source_parent) {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let parent = options.open(source_parent)?;
    let parent_metadata = parent.metadata()?;
    if metadata_device(&parent_metadata) != expected_parent_device
        || metadata_inode(&parent_metadata) != expected_parent_inode
    {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    }
    let source = CString::new(
        source
            .file_name()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?
            .as_bytes(),
    )
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination = CString::new(
        destination
            .file_name()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?
            .as_bytes(),
    )
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: The parent fd pins one opened directory, both leaves are
    // NUL-terminated, and RENAME_NOREPLACE prevents replacement.
    let result = unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            source.as_ptr(),
            parent.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_exclusive(
    _source: &Path,
    _destination: &Path,
    _expected_parent_device: u64,
    _expected_parent_inode: u64,
) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

fn sync_directory(path: &Path) -> Result<(), StorageSetupError> {
    open_directory_no_follow(path)?
        .sync_all()
        .map_err(|_| StorageSetupError::UnsupportedFilesystemState)
}

fn remove_exact_file(path: &Path, expected: &fs::Metadata) -> Result<(), StorageSetupError> {
    let current = fs::symlink_metadata(path).map_err(|_| StorageSetupError::NeedsRepair)?;
    if current.file_type().is_symlink() || !same_file(expected, &current) {
        return Err(StorageSetupError::NeedsRepair);
    }
    fs::remove_file(path).map_err(|_| StorageSetupError::NeedsRepair)
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
}

fn set_no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    let _ = options;
}

fn set_create_mode(options: &mut OpenOptions, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    let _ = (options, mode);
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::identity::marker_path;
    use crate::model::StorageCapacity;
    use synveil_server_config::{
        DeploymentProfile, ExistingServerEvidence, InitializeResult, LinuxConfigLayout,
        NewServerConfig,
    };

    fn fixture() -> (
        tempfile::TempDir,
        ServerConfigStore,
        StorageLocationWizard,
        ServerConfig,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let layout = LinuxConfigLayout::at_root(temp.path().join("etc/synveil")).unwrap();
        let store = ServerConfigStore::new(layout);
        let config = match store
            .initialize_managed(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::PersonalHomeManaged,
                },
                ExistingServerEvidence::NoKnownServerState,
            )
            .unwrap()
        {
            InitializeResult::Created(config) | InitializeResult::Reused(config) => config,
        };
        let wizard = StorageLocationWizard::new(store.clone(), StorageExclusionSet::default());
        (temp, store, wizard, config)
    }

    fn candidate(temp: &tempfile::TempDir, leaf: &str) -> PathBuf {
        temp.path().canonicalize().unwrap().join(leaf)
    }

    fn begin_pending(store: &ServerConfigStore, root: &Path) -> (ServerConfig, StorageId) {
        let (_, fingerprint) = store.load_config_read_only().unwrap().unwrap();
        let storage_id = StorageId::new_v7();
        let candidate = inspect_candidate(root).unwrap();
        let preparing = store
            .begin_storage_preparation(
                fingerprint,
                root.to_str().unwrap().to_owned(),
                storage_id.clone(),
                durable_root_identity(&candidate),
            )
            .unwrap();
        (preparing, storage_id)
    }

    fn persisted_config(store: &ServerConfigStore) -> ServerConfig {
        store.load_config_read_only().unwrap().unwrap().0
    }

    #[test]
    fn recommended_location_and_browse_inspection_do_not_mutate() {
        let (temp, store, wizard, _config) = fixture();
        assert_eq!(
            wizard.suggested_location(),
            PathBuf::from("/var/lib/synveil/storage")
        );
        let root = candidate(&temp, "server-object-data");
        let view = wizard.inspect_location(&root).unwrap();
        assert_eq!(view.classification, StorageRootClassification::NewCandidate);
        assert_eq!(view.status, StorageProductStatus::Available);
        assert!(view.capacity.is_some());
        assert_eq!(view.removability, StorageRemovability::Unknown);
        assert!(!root.exists());
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::NotConfigured
        ));
    }

    #[test]
    fn configured_client_library_roots_are_excluded_component_wise() {
        let (temp, store, _, _) = fixture();
        let base = temp.path().canonicalize().unwrap();
        let library = base.join("Documents");
        fs::create_dir(&library).unwrap();
        let inside_library = library.join(".synveil-server");
        let wizard =
            StorageLocationWizard::new(store.clone(), StorageExclusionSet::new([library.clone()]));
        assert_eq!(
            wizard
                .inspect_location(&inside_library)
                .unwrap()
                .classification,
            StorageRootClassification::UnsupportedFilesystemState
        );

        let server_root = base.join("external/Synveil");
        fs::create_dir_all(base.join("external")).unwrap();
        let nested_library = server_root.join("MyFiles");
        let wizard_with_nested_library =
            StorageLocationWizard::new(store.clone(), StorageExclusionSet::new([nested_library]));
        assert_eq!(
            wizard_with_nested_library
                .inspect_location(&server_root)
                .unwrap()
                .classification,
            StorageRootClassification::UnsupportedFilesystemState
        );

        let sibling = base.join("Documents2");
        let sibling_wizard = StorageLocationWizard::new(store, StorageExclusionSet::new([library]));
        assert_eq!(
            sibling_wizard
                .inspect_location(&sibling)
                .unwrap()
                .classification,
            StorageRootClassification::NewCandidate
        );
    }

    #[test]
    fn concurrent_different_root_selections_commit_at_most_one_root() {
        use std::sync::{Arc, Barrier};

        let (first_parent, store, first_wizard, _) = fixture();
        let second_parent = tempfile::tempdir().unwrap();
        let first_root = candidate(&first_parent, "concurrent-first-root");
        let second_root = candidate(&second_parent, "concurrent-second-root");
        let first_plan = first_wizard.prepare_selection(&first_root, None).unwrap();
        let second_wizard =
            StorageLocationWizard::new(store.clone(), StorageExclusionSet::default());
        let second_plan = second_wizard.prepare_selection(&second_root, None).unwrap();

        let start = Arc::new(Barrier::new(3));
        let first_start = Arc::clone(&start);
        let first_worker = std::thread::spawn(move || {
            first_start.wait();
            first_wizard.confirm_selection(first_plan)
        });
        let second_start = Arc::clone(&start);
        let second_worker = std::thread::spawn(move || {
            second_start.wait();
            second_wizard.confirm_selection(second_plan)
        });
        start.wait();
        let outcomes = [first_worker.join().unwrap(), second_worker.join().unwrap()];

        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        let configured = persisted_config(&store);
        let StorageConfiguration::ConfiguredLocal { root: selected, .. } = &configured.storage
        else {
            panic!("one concurrent confirmed selection should become authoritative")
        };
        let rejected_root = if selected == first_root.to_str().unwrap() {
            &second_root
        } else {
            assert_eq!(selected, second_root.to_str().unwrap());
            &first_root
        };
        assert!(!rejected_root.exists());
    }

    #[test]
    fn explicit_confirmation_initializes_identity_layout_and_typed_config() {
        let (temp, store, wizard, initial) = fixture();
        let root = candidate(&temp, "server-object-data");
        let plan = wizard.prepare_selection(&root, Some(1)).unwrap();
        assert!(!root.exists(), "review must not create the selected root");
        let confirmed = wizard.confirm_selection(plan).unwrap();
        assert!(matches!(
            &confirmed,
            CurrentStorageStatus::Configured(StorageLocationView {
                status: StorageProductStatus::ReadyToUse,
                ..
            })
        ));
        let CurrentStorageStatus::Configured(confirmed_view) = confirmed else {
            unreachable!();
        };
        assert_eq!(confirmed_view.selected_location, root);
        assert!(confirmed_view.capacity.is_some());
        assert!(matches!(
            wizard.current_storage_status().unwrap(),
            CurrentStorageStatus::Configured(StorageLocationView {
                status: StorageProductStatus::AlreadyConfigured,
                ..
            })
        ));
        let configured = persisted_config(&store);
        let StorageConfiguration::ConfiguredLocal {
            root: stored_root,
            storage_id,
            root_identity,
            capabilities,
        } = &configured.storage
        else {
            panic!("storage setup did not commit ConfiguredLocal")
        };
        assert_eq!(stored_root, root.to_str().unwrap());
        assert!(matches!(
            root_identity,
            StorageRootIdentity::Directory { .. }
        ));
        assert_eq!(
            configured.server_installation_id,
            initial.server_installation_id
        );
        assert!(matches!(
            read_identity(&root),
            Ok(identity)
                if identity.server_installation_id() == configured.server_installation_id
                    && identity.storage_id() == storage_id
        ));
        assert!(root.join(".synveil-storage-root").is_file());
        assert!(root.join("objects/v1").is_dir());
        assert!(root.join("staging").is_dir());
        let reopened = open_existing_managed_local_storage(&configured).unwrap();
        assert_eq!(reopened.capabilities(), capabilities.clone());
        assert!(
            reopened
                .capabilities()
                .supports(StorageCapability::AtomicPromotion)
        );
        assert!(
            reopened
                .capabilities()
                .supports(StorageCapability::DurableFlush)
        );

        let generation = configured.generation;
        let idempotent = wizard
            .confirm_selection(wizard.prepare_selection(&root, None).unwrap())
            .unwrap();
        assert!(matches!(idempotent, CurrentStorageStatus::Configured(_)));
        assert_eq!(persisted_config(&store).generation, generation);
    }

    #[test]
    fn managed_existing_only_open_rejects_missing_markers_and_layout_without_repair() {
        for missing in 0..3 {
            let (temp, store, wizard, _) = fixture();
            let root = candidate(&temp, &format!("existing-only-{missing}"));
            wizard
                .confirm_selection(wizard.prepare_selection(&root, None).unwrap())
                .unwrap();
            let config = persisted_config(&store);
            match missing {
                0 => fs::remove_file(marker_path(&root)).unwrap(),
                1 => fs::remove_file(root.join(LOCAL_STORE_MARKER)).unwrap(),
                _ => fs::remove_dir(root.join("objects/v1")).unwrap(),
            }
            assert!(open_existing_managed_local_storage(&config).is_err());
            assert!(match missing {
                0 => !marker_path(&root).exists(),
                1 => !root.join(LOCAL_STORE_MARKER).exists(),
                _ => !root.join("objects/v1").exists(),
            });
            assert!(matches!(
                persisted_config(&store).storage,
                StorageConfiguration::ConfiguredLocal { .. }
            ));
        }
    }

    #[test]
    fn an_existing_empty_directory_is_usable_but_unknown_content_is_preserved() {
        let (temp, _store, wizard, _) = fixture();
        let empty = candidate(&temp, "empty-server-data");
        fs::create_dir(&empty).unwrap();
        assert_eq!(
            wizard.inspect_location(&empty).unwrap().classification,
            StorageRootClassification::EmptyDirectory
        );
        wizard
            .confirm_selection(wizard.prepare_selection(&empty, None).unwrap())
            .unwrap();

        let (unknown_temp, _store, unknown_wizard, _) = fixture();
        let unknown = candidate(&unknown_temp, "unknown-data");
        fs::create_dir(&unknown).unwrap();
        let canary = unknown.join("keep.bin");
        fs::write(&canary, b"do not change").unwrap();
        assert_eq!(
            unknown_wizard
                .inspect_location(&unknown)
                .unwrap()
                .classification,
            StorageRootClassification::NonEmptyUnknownDirectory
        );
        assert_eq!(
            unknown_wizard
                .prepare_selection(&unknown, None)
                .unwrap_err(),
            StorageSetupError::NeedsReview
        );
        assert_eq!(fs::read(&canary).unwrap(), b"do not change");
        assert_eq!(fs::read_dir(&unknown).unwrap().count(), 1);
    }

    #[test]
    fn no_automatic_legacy_adoption_or_malformed_identity_rewrite() {
        let (temp, _store, wizard, _) = fixture();
        let legacy = candidate(&temp, "legacy-object-store");
        fs::create_dir(&legacy).unwrap();
        synveil_storage::initialize_local_object_store_root(&legacy).unwrap();
        assert_eq!(
            wizard.inspect_location(&legacy).unwrap().classification,
            StorageRootClassification::LegacyObjectStoreNeedsReview
        );

        let malformed = candidate(&temp, "malformed-identity");
        fs::create_dir(&malformed).unwrap();
        let marker = marker_path(&malformed);
        fs::write(&marker, b"{}\n").unwrap();
        assert_eq!(
            wizard.inspect_location(&malformed).unwrap().classification,
            StorageRootClassification::MalformedStorageIdentity
        );
        assert_eq!(fs::read(marker).unwrap(), b"{}\n");
    }

    #[test]
    fn foreign_server_or_storage_identity_is_not_reused() {
        for foreign_server in [false, true] {
            let (temp, store, wizard, _) = fixture();
            let root = candidate(&temp, "foreign-storage");
            fs::create_dir(&root).unwrap();
            let (preparing, storage_id) = begin_pending(&store, &root);
            let marker_config = if foreign_server {
                let mut config = preparing.clone();
                config.server_installation_id = StorageId::new_v7().as_str().to_owned();
                config
            } else {
                preparing.clone()
            };
            let marker_id = if foreign_server {
                storage_id.clone()
            } else {
                StorageId::new_v7()
            };
            initialize_identity(&root, &marker_config, marker_id).unwrap();
            assert_eq!(
                wizard.inspect_location(&root).unwrap().classification,
                StorageRootClassification::ForeignManagedStorage
            );
        }
    }

    #[test]
    fn preparation_resumes_same_id_after_root_identity_and_object_layout_boundaries() {
        // PreparingLocal with no root: recovery creates only the selected leaf.
        let (temp, store, wizard, initial) = fixture();
        let root = candidate(&temp, "resume-before-root");
        let (_, storage_id) = begin_pending(&store, &root);
        wizard.resume_pending_selection().unwrap();
        let configured = persisted_config(&store);
        assert!(matches!(
            &configured.storage,
            StorageConfiguration::ConfiguredLocal { root: actual, storage_id: actual_id, .. }
                if actual == root.to_str().unwrap() && actual_id == &storage_id
        ));
        assert_eq!(
            configured.server_installation_id,
            initial.server_installation_id
        );

        // Durable server identity but no LocalFilesystemObjectStore layout.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-server-marker");
        fs::create_dir(&root).unwrap();
        let (preparing, storage_id) = begin_pending(&store, &root);
        initialize_identity(&root, &preparing, storage_id.clone()).unwrap();
        assert_eq!(
            wizard.inspect_location(&root).unwrap().classification,
            StorageRootClassification::InterruptedMatchingSetup
        );
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));

        // Interruption after the exact hidden leaf was created but before its
        // server-storage identity marker was written.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-hidden-leaf");
        let (_preparing, storage_id) = begin_pending(&store, &root);
        let temporary = root
            .parent()
            .unwrap()
            .join(format!(".synveil-storage-setup-{}", storage_id.as_str()));
        let selected_temp = inspect_candidate(&temporary).unwrap();
        create_exact_directory(&temporary, &selected_temp).unwrap();
        assert!(!root.exists());
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));

        // Interruption after the hidden root's identity marker is durable but
        // before the selected root becomes visible.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-hidden-marker");
        let (preparing, storage_id) = begin_pending(&store, &root);
        let temporary = root
            .parent()
            .unwrap()
            .join(format!(".synveil-storage-setup-{}", storage_id.as_str()));
        let selected_temp = inspect_candidate(&temporary).unwrap();
        create_exact_directory(&temporary, &selected_temp).unwrap();
        initialize_identity(&temporary, &preparing, storage_id.clone()).unwrap();
        assert!(!root.exists());
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));

        // Interruption after a nonexistent leaf was atomically published with
        // its matching server identity marker, before config recorded its inode.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-root-publication");
        let (preparing, storage_id) = begin_pending(&store, &root);
        let selected = inspect_candidate(&root).unwrap();
        let created =
            create_managed_root_with_identity(&root, &selected, &preparing, &storage_id).unwrap();
        assert_eq!(
            wizard.inspect_location(&root).unwrap().classification,
            StorageRootClassification::InterruptedMatchingSetup
        );
        assert!(matches!(
            &persisted_config(&store).storage,
            StorageConfiguration::PreparingLocal {
                root_identity: StorageRootIdentity::MissingLeaf { .. },
                storage_id: actual,
                ..
            } if actual == &storage_id
        ));
        assert!(created.exists);
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));

        // Interruption after the LocalFilesystemObjectStore root marker.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-object-marker");
        fs::create_dir(&root).unwrap();
        let (preparing, storage_id) = begin_pending(&store, &root);
        initialize_identity(&root, &preparing, storage_id.clone()).unwrap();
        fs::write(root.join(LOCAL_STORE_MARKER), LOCAL_STORE_MARKER_CONTENT).unwrap();
        assert_eq!(
            wizard.inspect_location(&root).unwrap().classification,
            StorageRootClassification::InterruptedMatchingSetup
        );
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));

        // Interruption after the ObjectStore subdirectories are initialized.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-object-layout");
        fs::create_dir(&root).unwrap();
        let (preparing, storage_id) = begin_pending(&store, &root);
        initialize_identity(&root, &preparing, storage_id.clone()).unwrap();
        synveil_storage::initialize_local_object_store_root(&root).unwrap();
        assert_eq!(
            wizard.inspect_location(&root).unwrap().classification,
            StorageRootClassification::InterruptedMatchingSetup
        );
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));

        // Interrupt after individual durable directory boundaries inside the
        // canonical adapter layout. Recovery may complete these exact empty
        // layout paths but must preserve anything outside that known shape.
        for (leaf, known_directories) in [
            ("resume-after-objects-directory", vec!["objects"]),
            (
                "resume-after-object-layout-directory",
                vec!["objects", "objects/v1"],
            ),
            (
                "resume-after-staging-directory",
                vec!["objects", "objects/v1", "staging"],
            ),
        ] {
            let (temp, store, wizard, _) = fixture();
            let root = candidate(&temp, leaf);
            fs::create_dir(&root).unwrap();
            let (preparing, storage_id) = begin_pending(&store, &root);
            initialize_identity(&root, &preparing, storage_id.clone()).unwrap();
            fs::write(root.join(LOCAL_STORE_MARKER), LOCAL_STORE_MARKER_CONTENT).unwrap();
            for directory in known_directories {
                fs::create_dir(root.join(directory)).unwrap();
            }
            assert_eq!(
                wizard.inspect_location(&root).unwrap().classification,
                StorageRootClassification::InterruptedMatchingSetup
            );
            wizard.resume_pending_selection().unwrap();
            assert!(matches!(
                persisted_config(&store).storage,
                StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                    if actual == storage_id
            ));
        }

        // Interruption after the setup capability and write/durability probes,
        // before the ConfiguredLocal CAS.
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "resume-after-capability-probe");
        fs::create_dir(&root).unwrap();
        let (preparing, storage_id) = begin_pending(&store, &root);
        initialize_identity(&root, &preparing, storage_id.clone()).unwrap();
        let object_store = initialize_local_object_store_root(&root).unwrap();
        assert_eq!(
            object_store
                .capabilities()
                .support(StorageCapability::DurableFsync),
            CapabilitySupport::Supported
        );
        run_write_durability_probe(&root).unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::PreparingLocal { storage_id: actual, .. }
                if actual == storage_id
        ));
        wizard.resume_pending_selection().unwrap();
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id: actual, .. }
                if actual == storage_id
        ));
        drop(temp);
    }

    #[test]
    fn unknown_artifacts_in_a_pending_root_require_repair_and_are_preserved() {
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "pending-with-unknown-file");
        fs::create_dir(&root).unwrap();
        let (preparing, storage_id) = begin_pending(&store, &root);
        initialize_identity(&root, &preparing, storage_id).unwrap();
        let canary = root.join("unexpected.bin");
        fs::write(&canary, b"retain").unwrap();
        assert_eq!(
            wizard.inspect_location(&root).unwrap().classification,
            StorageRootClassification::ObjectStoreLayoutNeedsRepair
        );
        assert_eq!(
            wizard.resume_pending_selection().unwrap_err(),
            StorageSetupError::NeedsReview
        );
        assert_eq!(fs::read(&canary).unwrap(), b"retain");
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::PreparingLocal { .. }
        ));
        drop(temp);
    }

    #[test]
    fn configured_unavailable_root_is_not_replaced_and_relocation_is_rejected() {
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "configured-root");
        wizard
            .confirm_selection(wizard.prepare_selection(&root, None).unwrap())
            .unwrap();
        let configured = persisted_config(&store);
        let StorageConfiguration::ConfiguredLocal { storage_id, .. } = &configured.storage else {
            unreachable!()
        };
        let original_id = storage_id.clone();
        fs::remove_dir_all(&root).unwrap();
        assert!(open_existing_managed_local_storage(&configured).is_err());
        let status = wizard.current_storage_status().unwrap();
        assert!(matches!(
            status,
            CurrentStorageStatus::Configured(StorageLocationView {
                classification: StorageRootClassification::Unavailable,
                status: StorageProductStatus::Disconnected,
                ..
            })
        ));
        assert!(!root.exists());
        assert!(matches!(
            persisted_config(&store).storage,
            StorageConfiguration::ConfiguredLocal { storage_id, .. }
                if storage_id == original_id
        ));
        fs::create_dir(&root).unwrap();
        assert_eq!(
            wizard.inspect_location(&root).unwrap().classification,
            StorageRootClassification::RootIdentityMismatch
        );
        assert_eq!(
            wizard.prepare_selection(&root, None).unwrap_err(),
            StorageSetupError::IdentityConflict
        );
        assert!(open_existing_managed_local_storage(&persisted_config(&store)).is_err());
        assert!(directory_is_empty(&root).unwrap());
        let alternative = candidate(&temp, "replacement-root");
        assert_eq!(
            wizard.prepare_selection(&alternative, None).unwrap_err(),
            StorageSetupError::StorageRelocationRequiresMigration
        );
        assert!(!alternative.exists());
    }

    #[test]
    fn stale_review_and_capacity_constraints_fail_before_mutation() {
        let (temp, store, wizard, _) = fixture();
        let root = candidate(&temp, "stale-plan-root");
        let plan = wizard.prepare_selection(&root, None).unwrap();
        begin_pending(&store, &candidate(&temp, "other-process-choice"));
        assert_eq!(
            wizard.confirm_selection(plan).unwrap_err(),
            StorageSetupError::StalePlan
        );
        assert!(!root.exists());

        let (temp, _store, wizard, _) = fixture();
        let capacity_root = candidate(&temp, "capacity-root");
        assert_eq!(
            wizard
                .prepare_selection(&capacity_root, Some(u64::MAX))
                .unwrap_err(),
            StorageSetupError::InsufficientCapacity
        );
        assert!(!capacity_root.exists());
    }

    #[test]
    fn capacity_model_rejects_read_only_unknown_and_insufficient_results() {
        let available = CapacityReport {
            capacity: StorageCapacity {
                total_bytes: 100,
                available_bytes: 40,
                available_files: Some(5),
            },
            read_only: false,
        };
        assert_eq!(
            qualify_capacity(
                StorageRootClassification::NewCandidate,
                Some(available),
                Some(40)
            ),
            StorageRootClassification::NewCandidate
        );
        assert_eq!(
            qualify_capacity(
                StorageRootClassification::NewCandidate,
                Some(available),
                Some(41)
            ),
            StorageRootClassification::InsufficientCapacity
        );
        assert_eq!(
            qualify_capacity(
                StorageRootClassification::NewCandidate,
                Some(CapacityReport {
                    read_only: true,
                    ..available
                }),
                None
            ),
            StorageRootClassification::ReadOnly
        );
        assert_eq!(
            validate_runtime_capacity(Some(CapacityReport {
                read_only: true,
                ..available
            })),
            Err(StorageSetupError::ReadOnly)
        );
        assert_eq!(
            validate_runtime_capacity(None),
            Err(StorageSetupError::CapacityUnknown)
        );
        assert_eq!(
            qualify_capacity(StorageRootClassification::EmptyDirectory, None, None),
            StorageRootClassification::CapacityUnknown
        );
        assert_eq!(
            classify_mount_root(Some(available), Some(40)),
            StorageRootClassification::MountRootNeedsDedicatedChild
        );
        assert_eq!(
            classify_mount_root(Some(available), Some(41)),
            StorageRootClassification::InsufficientCapacity
        );
        assert_eq!(
            classify_mount_root(
                Some(CapacityReport {
                    read_only: true,
                    ..available
                }),
                None,
            ),
            StorageRootClassification::ReadOnly
        );
        assert_eq!(
            classify_mount_root(None, None),
            StorageRootClassification::CapacityUnknown
        );
    }
}
