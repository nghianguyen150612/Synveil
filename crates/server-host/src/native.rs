//! Root-side Host session. Every mutation is fenced by the full reviewed plan.
mod account;
mod network;
mod services;

use crate::protocol::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use synveil_server_bootstrap::{
    CanonicalOwners, CoordinatorError, OwnerObservation, PrivilegedEffect, ReviewedHostPlan,
};
use synveil_server_config::{
    DeploymentProfile, ExternalDatabaseCredential, NetworkConfiguration, NewServerConfig,
    ServerConfig, ServerConfigStore, StorageConfiguration,
};
use synveil_server_service::{
    native::{self, Result},
    runtime::{RuntimeManifest, sha256},
};
use synveil_server_storage::{
    StorageExclusionSet, StorageLocationWizard, open_existing_managed_local_storage,
};

/// Return only the number of suitable Private LAN candidates. The underlying
/// inspection uses the existing bounded `ip` read-only probes and does not
/// retain or publish interface identifiers or addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateLanInspectError {
    InspectionFailed,
}

pub fn inspect_private_lan_candidate_count() -> std::result::Result<u8, PrivateLanInspectError> {
    network::candidates()
        .map(|candidates| candidates.len().min(8) as u8)
        .map_err(|_| PrivateLanInspectError::InspectionFailed)
}

const JOURNAL: &str = "/etc/synveil/host-setup.json";
const CATALOG: &str = "/etc/synveil/host-runtime-catalog.json";

fn external_target(value: &str) -> Result<ExternalDatabaseTarget> {
    ExternalDatabaseCredential::new(value.to_owned())
        .map_err(|_| "invalid external database credential")?;
    let parsed = url::Url::parse(value).map_err(|_| "invalid external database URL")?;
    if !["postgres", "postgresql"].contains(&parsed.scheme())
        || parsed.username().is_empty()
        || parsed.fragment().is_some()
        || parsed.path().len() < 2
        || parsed.path()[1..].contains('/')
    {
        return Err("Advanced setup requires one explicit TCP database and role");
    }
    let mut transport = "private_tcp".to_owned();
    for (key, value) in parsed.query_pairs() {
        // Root filesystem paths and alternate hosts supplied through URL
        // options are outside this purpose-specific helper boundary.
        if key != "sslmode" || !["disable", "verify-full"].contains(&value.as_ref()) {
            return Err(
                "external database connection option is outside the reviewed Host boundary",
            );
        }
        transport = value.to_string();
    }
    let host = parsed.host_str().ok_or("external TCP host required")?;
    if transport != "verify-full"
        && host != "localhost"
        && !host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| {
                ip.is_loopback() || matches!(ip, std::net::IpAddr::V4(ip) if ip.is_private())
            })
    {
        return Err("external PostgreSQL requires verified TLS or an explicit private IP endpoint");
    }
    Ok(ExternalDatabaseTarget {
        host: host.to_owned(),
        port: parsed.port().unwrap_or(5432),
        role: parsed.username().to_owned(),
        database: parsed.path()[1..].to_owned(),
        transport,
    })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCatalog {
    pub schema_version: u32,
    pub qualified_os: String,
    pub artifacts: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    actor_uid: u32,
    server_id: String,
    selection: HostSelection,
    #[serde(default)]
    pending_network_selection: Option<HostSelection>,
    consumed_nonces: Vec<String>,
    owned_files: BTreeMap<String, String>,
    #[serde(default)]
    pending_files: BTreeMap<String, FileIntent>,
    #[serde(default)]
    runtime_intents: BTreeMap<String, String>,
    #[serde(default)]
    retained_runtimes: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pending_postgres_archive: Option<PendingPostgresArchive>,
    #[serde(default)]
    preserved_postgres_data: Vec<PreservedPostgresData>,
    postgres_port: Option<u16>,
    postgres_data_identity: Option<(u64, u64)>,
    postgres_system_id: Option<String>,
    network_id: String,
    verified_admin: Option<String>,
    library_id: Option<String>,
    last_effect: Option<PrivilegedEffect>,
    #[serde(default)]
    client_device_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIntent {
    old_sha256: Option<String>,
    new_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingPostgresArchive {
    archive_name: String,
    device: u64,
    inode: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreservedPostgresData {
    archive_name: String,
    device: u64,
    inode: u64,
}
impl Journal {
    fn owns_file_outcome(&self, path: &str, actual: &str) -> bool {
        self.owned_files.get(path).is_some_and(|old| old == actual)
            || self.pending_files.get(path).is_some_and(|intent| {
                intent.old_sha256.as_deref() == Some(actual) || intent.new_sha256 == actual
            })
    }
}
struct Authorization {
    context: ReviewedContext,
    next: usize,
    snapshot: String,
    external_database_url: Option<zeroize::Zeroizing<String>>,
}
pub struct NativeOwners {
    actor: u32,
    store: ServerConfigStore,
    review: Option<ReviewedContext>,
    review_external: Option<zeroize::Zeroizing<String>>,
    authorization: Option<Authorization>,
    lock: Option<fs::File>,
}
impl NativeOwners {
    pub fn new(actor: u32) -> Result<Self> {
        if !nix::unistd::geteuid().is_root() || actor == 0 {
            return Err("authenticated non-root Host actor required");
        }
        Ok(Self {
            actor,
            store: ServerConfigStore::managed_linux(),
            review: None,
            review_external: None,
            authorization: None,
            lock: None,
        })
    }
    fn journal(&self) -> Result<Option<Journal>> {
        if !Path::new(JOURNAL).exists() {
            return Ok(None);
        }
        use std::os::unix::fs::MetadataExt;
        if fs::symlink_metadata(JOURNAL)
            .map_err(|_| "Host journal unavailable")?
            .mode()
            & 0o7777
            != 0o600
        {
            return Err("Host journal permissions need repair");
        }
        let bytes = native::read_owned(Path::new(JOURNAL), FRAME_LIMIT)?;
        let journal: Journal =
            serde_json::from_slice(&bytes).map_err(|_| "Host journal needs repair")?;
        if journal.schema_version != 1
            || journal.actor_uid != self.actor
            || journal.consumed_nonces.len() > 1024
            || journal.preserved_postgres_data.len() > 16
            || (journal.pending_postgres_archive.is_some()
                && journal.preserved_postgres_data.len() >= 16)
        {
            return Err("Host belongs to another local account or journal needs repair");
        }
        ServerConfig::proposed_host(journal.selection.profile, journal.server_id.clone())
            .map_err(|_| "invalid Host journal identity")?;
        Ok(Some(journal))
    }
    fn persist(&self, journal: &Journal) -> Result<()> {
        let previous = if Path::new(JOURNAL).exists() {
            Some(native::read_owned(Path::new(JOURNAL), FRAME_LIMIT)?)
        } else {
            None
        };
        native::write_owned(
            Path::new(JOURNAL),
            &serde_json::to_vec(journal).map_err(|_| "Host journal encoding failed")?,
            previous.as_deref(),
            0o600,
        )
    }
    fn config(&self) -> Result<Option<ServerConfig>> {
        self.store
            .load_config_read_only()
            .map(|value| value.map(|(config, _)| config))
            .map_err(|_| "server configuration needs repair")
    }
    fn fingerprint(&self) -> Result<String> {
        Ok(self
            .store
            .load_config_read_only()
            .map_err(|_| "server configuration needs repair")?
            .map_or_else(
                || "absent".to_owned(),
                |(_, fingerprint)| fingerprint.to_hex(),
            ))
    }
    fn snapshot(&self) -> Result<String> {
        let journal = if Path::new(JOURNAL).exists() {
            native::read_owned(Path::new(JOURNAL), FRAME_LIMIT)?
        } else {
            Vec::new()
        };
        let mut bytes = self.fingerprint()?.into_bytes();
        if Path::new(CATALOG).exists() {
            bytes.extend_from_slice(
                sha256(&native::read_owned(Path::new(CATALOG), 16 * 1024)?).as_bytes(),
            );
        }
        bytes.extend_from_slice(&journal);
        if let Some(journal) = self.journal()? {
            let selection = journal
                .pending_network_selection
                .as_ref()
                .unwrap_or(&journal.selection);
            bytes.extend_from_slice(
                self.path_identity(Path::new(&selection.storage_root))?
                    .as_bytes(),
            );
            bytes.extend_from_slice(network::input_fingerprint(selection)?.as_bytes());
        }
        let mut owned_inputs = BTreeMap::new();
        if let Some(journal) = self.journal()? {
            owned_inputs.extend(journal.owned_files);
            owned_inputs.extend(
                journal
                    .pending_files
                    .into_iter()
                    .map(|(path, intent)| (path, intent.new_sha256)),
            );
        }
        for (path, expected) in owned_inputs {
            let actual = if Path::new(&path).exists() {
                sha256(&native::read_owned(Path::new(&path), 16 * 1024 * 1024)?)
            } else {
                "absent".to_owned()
            };
            bytes.extend_from_slice(path.as_bytes());
            bytes.extend_from_slice(expected.as_bytes());
            bytes.extend_from_slice(actual.as_bytes());
        }
        Ok(sha256(&bytes))
    }
    fn path_identity(&self, path: &Path) -> Result<String> {
        use std::os::unix::fs::MetadataExt;
        let mut identity = String::new();
        let mut at = PathBuf::from("/");
        for part in path.components().skip(1) {
            if !matches!(part, std::path::Component::Normal(_)) {
                return Err("invalid storage path");
            }
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    identity.push_str(&format!(
                        "{}:{}:{}:{}:{}:{};",
                        at.display(),
                        metadata.dev(),
                        metadata.ino(),
                        metadata.uid(),
                        metadata.gid(),
                        metadata.mode()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    identity.push_str(&format!("{}:absent;", at.display()));
                }
                _ => return Err("storage ancestry changed or redirected"),
            }
        }
        Ok(identity)
    }
    fn catalog(
        &self,
        profile: DeploymentProfile,
        mode: synveil_server_config::ReachabilityMode,
    ) -> Result<(RuntimeCatalog, BTreeMap<String, RuntimeManifest>)> {
        let os = crate::supported_platform().ok_or("Managed Host platform unsupported")?;
        let bytes = native::read_owned(Path::new(CATALOG), 16 * 1024)
            .map_err(|_| "verified runtime catalog missing: supply signed pinned runtime inputs")?;
        let catalog: RuntimeCatalog =
            serde_json::from_slice(&bytes).map_err(|_| "invalid runtime catalog")?;
        if catalog.schema_version != 1 || catalog.qualified_os != os || catalog.artifacts.len() > 3
        {
            return Err("runtime catalog qualification mismatch");
        }
        let mut required = vec!["server"];
        if profile == DeploymentProfile::PersonalHomeManaged {
            required.push("postgresql");
        }
        if mode != synveil_server_config::ReachabilityMode::AdvancedExternalHttps {
            required.push("caddy");
        }
        let mut manifests = BTreeMap::new();
        for kind in required {
            let digest = catalog
                .artifacts
                .get(kind)
                .ok_or("required verified runtime input missing")?;
            let manifest = native::verify_runtime(
                &Path::new("/var/cache/synveil/host-runtime").join(digest),
                digest,
                &os,
            )?;
            if manifest.kind != kind {
                return Err("runtime catalog kind mismatch");
            }
            manifests.insert(kind.to_owned(), manifest);
        }
        Ok((catalog, manifests))
    }
    fn wizard(&self) -> Result<StorageLocationWizard> {
        let actor = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(self.actor))
            .map_err(|_| "Host account unavailable")?
            .ok_or("Host account unavailable")?;
        Ok(StorageLocationWizard::new(
            self.store.clone(),
            StorageExclusionSet::new([
                actor.dir,
                PathBuf::from("/var/lib/synveil/postgresql"),
                PathBuf::from("/var/cache/synveil"),
                PathBuf::from("/opt/synveil"),
            ]),
        ))
    }
    fn build_review(
        &self,
        selection: HostSelection,
        purpose: HostPurpose,
        id: Option<String>,
        nonce: Option<String>,
    ) -> Result<ReviewedContext> {
        let os = crate::supported_platform()
            .ok_or("Managed Host supports Ubuntu 24.04 and Fedora 42 x86_64")?;
        if selection.storage_root.len() > 4096 || !selection.storage_root.starts_with('/') {
            return Err("select a dedicated absolute server storage folder");
        }
        let journal = self.journal()?;
        if let Some(journal) = journal.as_ref() {
            if journal.pending_network_selection.is_some()
                && matches!(purpose, HostPurpose::Setup | HostPurpose::RepairHost)
            {
                return Err("a pending access change requires a fresh network review");
            }
            let effective = journal
                .pending_network_selection
                .as_ref()
                .unwrap_or(&journal.selection);
            if effective != &selection {
                let permitted_network_change = purpose == HostPurpose::ReconfigureNetwork
                    && journal.selection.profile == selection.profile
                    && journal.selection.storage_root == selection.storage_root
                    && journal.selection.external_database_target
                        == selection.external_database_target;
                if !permitted_network_change {
                    return Err(
                        "existing Host selections require an explicitly reviewed network change; storage and profile relocation need a separate migration",
                    );
                }
            }
        }
        let existing = self.config()?;
        let server_id = existing
            .as_ref()
            .map(|c| c.server_installation_id.clone())
            .or_else(|| journal.as_ref().map(|j| j.server_id.clone()))
            .or(id)
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let proposed = existing.unwrap_or(
            ServerConfig::proposed_host(selection.profile, server_id.clone())
                .map_err(|_| "invalid Host profile")?,
        );
        let network_id = journal
            .as_ref()
            .map(|journal| journal.network_id.clone())
            .or_else(|| {
                self.review
                    .as_ref()
                    .filter(|review| review.plan.server_installation_id == server_id)
                    .map(|review| review.network_integration_id.clone())
            })
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        if proposed.deployment_profile != selection.profile {
            return Err("existing deployment mode cannot be changed by setup");
        }
        if selection.storage_root == "/var/lib/synveil" {
            return Err("select a dedicated child folder, not the shared Synveil state parent");
        }
        let wizard = self.wizard()?;
        let inspection = match native::service_user("synveil") {
            Ok((uid, gid)) => wizard.inspect_proposed_parent_handoff(
                Path::new(&selection.storage_root),
                &proposed,
                uid,
                gid,
            ),
            Err(_) => wizard.inspect_proposed_host(Path::new(&selection.storage_root), &proposed),
        };
        let view =
            inspection.map_err(|_| "server storage requires a dedicated safe folder or repair")?;
        use synveil_server_storage::StorageRootClassification as C;
        if !matches!(
            view.classification,
            C::NewCandidate
                | C::EmptyDirectory
                | C::MatchingManagedStorage
                | C::InterruptedMatchingSetup
        ) {
            return Err(
                "selected folder contains foreign data or an incompatible storage identity",
            );
        }
        network::validate_selection(&selection)?;
        let (catalog, manifests) = self.catalog(selection.profile, selection.mode)?;
        services::preflight(self, &catalog, &selection)?;
        let service_inventory = services::units(&catalog, &selection)?;
        let postgres_repair = if purpose == HostPurpose::RepairHost {
            services::review_postgres_repair(self, &catalog, &selection)?
        } else {
            None
        };
        let old_artifacts = if purpose == HostPurpose::Upgrade {
            services::review_upgrade(self, &catalog, &selection)?
        } else {
            Vec::new()
        };
        let network_plan =
            network::reviewed_plan(&selection, &proposed, &network_id, &self.fingerprint()?)?;
        let plan = ReviewedHostPlan {
            generation: proposed.generation,
            server_installation_id: server_id,
            config_fingerprint: self.fingerprint()?,
            storage_plan_fingerprint: sha256(
                format!(
                    "{selection:?}:{:?}:{:?}:{}:{}",
                    view.classification,
                    view.status,
                    self.path_identity(Path::new(&selection.storage_root))?,
                    self.snapshot()?
                )
                .as_bytes(),
            ),
            service_plan_fingerprint: sha256(
                &serde_json::to_vec(&service_inventory)
                    .map_err(|_| "unit inventory encoding failed")?,
            ),
            network_plan_fingerprint: sha256(
                &serde_json::to_vec(&(
                    &selection,
                    &network_plan,
                    network::input_fingerprint(&selection)?,
                ))
                .map_err(|_| "network selection encoding failed")?,
            ),
            platform_qualification: os,
            artifact_fingerprints: manifests
                .iter()
                .map(|(kind, manifest)| {
                    format!(
                        "{kind}:{}:{}",
                        catalog.artifacts[kind],
                        sha256(&serde_json::to_vec(manifest).expect("manifest serialization"))
                    )
                })
                .collect(),
            expected_privileged_effects: match purpose {
                HostPurpose::Stop => vec![PrivilegedEffect::StopOwnedServices],
                HostPurpose::RemoveSoftware => vec![
                    PrivilegedEffect::StopOwnedServices,
                    PrivilegedEffect::RemoveOwnedSoftwarePreservingData,
                ],
                HostPurpose::Setup => vec![
                    PrivilegedEffect::InstallVerifiedDependencies,
                    PrivilegedEffect::PrepareConfiguration,
                    PrivilegedEffect::PrepareStorage,
                    PrivilegedEffect::ReconcileOwnedServices,
                    PrivilegedEffect::ConfigureOwnedReachability,
                ],
                HostPurpose::Upgrade => vec![
                    PrivilegedEffect::StopOwnedServices,
                    PrivilegedEffect::InstallVerifiedDependencies,
                    PrivilegedEffect::ReconcileOwnedServices,
                    PrivilegedEffect::ConfigureOwnedReachability,
                ],
                HostPurpose::ReconfigureNetwork => vec![
                    PrivilegedEffect::InstallVerifiedDependencies,
                    PrivilegedEffect::StopOwnedServices,
                    PrivilegedEffect::ReconcileOwnedServices,
                    PrivilegedEffect::ConfigureOwnedReachability,
                ],
                HostPurpose::RepairHost => vec![
                    PrivilegedEffect::InstallVerifiedDependencies,
                    PrivilegedEffect::PrepareConfiguration,
                    PrivilegedEffect::PrepareStorage,
                    PrivilegedEffect::ReconcileOwnedServices,
                    PrivilegedEffect::ConfigureOwnedReachability,
                ],
            },
        };
        let mut plan = plan;
        plan.artifact_fingerprints.extend(old_artifacts);
        plan.artifact_fingerprints
            .push(format!("host-purpose:{purpose:?}"));
        if let Some((archive_name, device, inode)) = &postgres_repair {
            plan.artifact_fingerprints.push(format!(
                "postgresql-preserve-partial:{archive_name}:{device}:{inode}"
            ));
        } else if purpose == HostPurpose::RepairHost {
            plan.artifact_fingerprints
                .push("postgresql-preserving-resume:no-directory-move".to_owned());
        }
        let binding = plan.binding();
        let mut summary = vec![
            format!("Server identity: {}", plan.server_installation_id),
            format!("Qualified platform: {}", plan.platform_qualification),
            format!("Server files: {}", selection.storage_root),
            format!(
                "Client address: {} ({:?})",
                selection.canonical_origin, selection.mode
            ),
        ];
        summary.extend(manifests.iter().map(|(kind, manifest)| {
            format!(
                "Install verified {kind} {} — manifest {}",
                manifest.version, catalog.artifacts[kind]
            )
        }));
        summary.push(format!(
            "Owned service inventory: {}",
            service_inventory
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if let Some(target) = &selection.external_database_target {
            summary.push(format!("Operator PostgreSQL 17 target: {}:{} / {} as role {} ({}) — apply canonical Synveil migrations; no database/server takeover.", target.host, target.port, target.database, target.role, target.transport));
        }
        summary.push(if selection.mode == synveil_server_config::ReachabilityMode::PrivateLan { format!("Open only HTTPS port 443 for the selected address {} and interface {} through the supported active firewall manager.", selection.listener_address, selection.interface_id.as_deref().unwrap_or("<missing>")) } else { "No managed firewall rule is added for this access mode.".to_owned() });
        summary.push("Protect the shared /var/lib/synveil parent as root:synveil 0755. Preserve its inode and all child data; only the closed server storage directories and managed database data receive service permissions.".to_owned());
        summary.push("Prepare protected configuration, reconcile the listed owned services, and verify actual HTTPS and database health. Existing database content, files, identities and private CA are preserved. The operating system's global trust store is unchanged.".to_owned());
        if purpose != HostPurpose::Setup {
            let change = match purpose {
                HostPurpose::Stop => {
                    "Stop only the owned services with bounded draining; installed software, enablement and all server data are preserved."
                }
                HostPurpose::RemoveSoftware => {
                    "Stop and disable the owned services. Remove only verified owned units, runtime files and the exact managed firewall rule. Preserve PostgreSQL content, server storage, configuration, identities, credentials and the private CA."
                }
                HostPurpose::Upgrade => {
                    "Stop the owned services, install the verified new runtime inventory, reconcile canonical migrations and units, and verify actual health and HTTPS. Preserve the PostgreSQL 17 system identity, storage, administrator, Library and private CA. Retain old owned runtime files for recovery."
                }
                HostPurpose::ReconfigureNetwork => {
                    "Apply the reviewed access mode, origin, listener and interface to this existing Host. Stop and reconcile only owned services, replace only the exact owned firewall rule, preserve server identity, database, storage, credentials, administrator, Library and CA, then verify the new HTTPS authority before committing it."
                }
                HostPurpose::RepairHost => {
                    "Preserve the exact interrupted PostgreSQL initialization directory intact under the protected recovery archive, then initialize and verify a new managed PostgreSQL 17 directory. No existing cluster is overwritten or deleted."
                }
                HostPurpose::Setup => unreachable!(),
            };
            summary = vec![
                format!("Server identity: {}", plan.server_installation_id),
                format!("Reviewed operation: {purpose:?}"),
                change.to_owned(),
                postgres_repair.as_ref().map_or_else(
                    || "No PostgreSQL directory move is planned; continue only from verified or empty owned state.".to_owned(),
                    |(name, _, _)| format!("Preserved PostgreSQL directory: /var/lib/synveil/postgresql/17/preserved/{name}"),
                ),
                format!(
                    "Owned service inventory: {}",
                    service_inventory
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ];
            summary.extend(
                plan.artifact_fingerprints
                    .iter()
                    .map(|identity| format!("Bound runtime input: {identity}")),
            );
        }
        Ok(ReviewedContext {
            network_integration_id: network_id,
            purpose,
            plan,
            selection,
            nonce: nonce.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
            binding,
            summary,
        })
    }
    fn confirm(&mut self, context: ReviewedContext, external: Option<String>) -> Result<()> {
        let external = external.map(zeroize::Zeroizing::new);
        if external.as_ref().is_some_and(|supplied| {
            self.review_external
                .as_ref()
                .is_none_or(|reviewed| supplied.as_str() != reviewed.as_str())
        }) {
            return Err("external database credential changed after review");
        }
        if self.review.as_ref().is_none_or(|review| {
            review.binding != context.binding
                || review.nonce != context.nonce
                || review.plan != context.plan
                || review.selection != context.selection
                || review.purpose != context.purpose
                || review.network_integration_id != context.network_integration_id
        }) {
            return Err("full helper review and one-time consent required");
        }
        native::directory(Path::new("/etc/synveil"), 0o750)?;
        use std::os::unix::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open("/etc/synveil/host-setup.lock")
            .map_err(|_| "Host setup lock unavailable")?;
        use fs2::FileExt;
        file.try_lock_exclusive()
            .map_err(|_| "another Host setup is running")?;
        self.lock = Some(file);
        let fresh = self.build_review(
            context.selection.clone(),
            context.purpose,
            Some(context.plan.server_installation_id.clone()),
            Some(context.nonce.clone()),
        )?;
        if fresh.plan != context.plan || fresh.binding != context.binding {
            self.lock = None;
            self.review = None;
            return Err("reviewed plan is stale; inspect and review again");
        }
        let mut journal = self.journal()?.unwrap_or(Journal {
            schema_version: 1,
            actor_uid: self.actor,
            server_id: context.plan.server_installation_id.clone(),
            selection: context.selection.clone(),
            pending_network_selection: None,
            consumed_nonces: Vec::new(),
            owned_files: BTreeMap::new(),
            pending_files: BTreeMap::new(),
            runtime_intents: BTreeMap::new(),
            retained_runtimes: BTreeMap::new(),
            pending_postgres_archive: None,
            preserved_postgres_data: Vec::new(),
            postgres_port: None,
            postgres_data_identity: None,
            postgres_system_id: None,
            network_id: context.network_integration_id.clone(),
            verified_admin: None,
            library_id: None,
            last_effect: None,
            client_device_id: None,
        });
        if journal.consumed_nonces.contains(&context.nonce) || journal.consumed_nonces.len() >= 1024
        {
            return Err("consent replay rejected");
        }
        if context.purpose == HostPurpose::ReconfigureNetwork {
            journal.pending_network_selection = Some(context.selection.clone());
        }
        let external = self.review_external.take();
        match context.selection.profile {
            DeploymentProfile::PersonalHomeManaged if external.is_some() => {
                return Err("managed profile does not accept an external database URL");
            }
            DeploymentProfile::AdvancedExternal if self.config()?.is_none() => {
                ExternalDatabaseCredential::new(
                    external
                        .as_ref()
                        .ok_or("Advanced mode requires an operator database credential")?
                        .to_string(),
                )
                .map_err(|_| "invalid external database credential")?;
            }
            _ => {}
        }
        // Persist consumed consent BEFORE the first machine effect. This never
        // makes Ready true, and a crash always requires a new reviewed nonce.
        journal.consumed_nonces.push(context.nonce.clone());
        self.persist(&journal)?;
        self.review = None;
        self.authorization = Some(Authorization {
            context,
            next: 0,
            snapshot: self.snapshot()?,
            external_database_url: external,
        });
        Ok(())
    }
    fn publish(&self, path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
        let mut journal = self.journal()?.ok_or("owned Host intent required")?;
        let key = path.to_str().ok_or("invalid owned filename")?.to_owned();
        let previous = if path.exists() {
            let actual = native::read_owned(path, 16 * 1024 * 1024)?;
            if !journal.owns_file_outcome(&key, &sha256(&actual)) {
                return Err(
                    "foreign or modified service/network file; repair requires operator reconciliation",
                );
            }
            Some(actual)
        } else {
            None
        };
        journal.pending_files.insert(
            key.clone(),
            FileIntent {
                old_sha256: previous.as_ref().map(|old| sha256(old)),
                new_sha256: sha256(bytes),
            },
        );
        self.persist(&journal)?;
        native::write_owned(path, bytes, previous.as_deref(), mode)?;
        journal.owned_files.insert(key.clone(), sha256(bytes));
        journal.pending_files.remove(&key);
        self.persist(&journal)
    }
    fn execute(&mut self, binding: &str, sequence: usize, effect: PrivilegedEffect) -> Result<()> {
        let context = self
            .authorization
            .as_ref()
            .ok_or("full reviewed helper consent required")?;
        if context.context.binding != binding
            || context.next != sequence
            || context
                .context
                .plan
                .expected_privileged_effects
                .get(sequence)
                != Some(&effect)
            || context.snapshot != self.snapshot()?
        {
            self.authorization = None;
            return Err("stale, reordered or replayed Host effect rejected");
        }
        let selection = context.context.selection.clone();
        let mut journal = self.journal()?.ok_or("owned Host journal missing")?;
        let (catalog, _) = self.catalog(selection.profile, selection.mode)?;
        let result = match effect {
            PrivilegedEffect::InstallVerifiedDependencies => {
                services::install_runtimes(self, &catalog, &selection)
            }
            PrivilegedEffect::PrepareConfiguration => {
                let external = self
                    .authorization
                    .as_mut()
                    .and_then(|a| a.external_database_url.take())
                    .map(|value| ExternalDatabaseCredential::new(value.to_string()))
                    .transpose()
                    .map_err(|_| "invalid external database credential")?;
                self.store
                    .initialize_for_host(
                        NewServerConfig {
                            deployment_profile: selection.profile,
                        },
                        journal.server_id.clone(),
                        external,
                        true,
                    )
                    .map(|_| ())
                    .map_err(|_| "configuration initialization needs repair")
            }
            PrivilegedEffect::PrepareStorage => {
                let config = self.config()?.ok_or("server configuration missing")?;
                if matches!(config.storage, StorageConfiguration::PreparingLocal { .. }) {
                    self.wizard()?
                        .resume_pending_selection()
                        .map_err(|_| "pending server storage needs repair")?;
                } else {
                    let plan = self
                        .wizard()?
                        .prepare_selection(Path::new(&selection.storage_root), None)
                        .map_err(|_| "storage plan changed")?;
                    self.wizard()?
                        .confirm_selection(plan)
                        .map_err(|_| "server storage setup needs repair")?;
                }
                services::handoff_storage(self)
            }
            PrivilegedEffect::ReconcileOwnedServices => {
                services::reconcile(self, &catalog, &selection)
            }
            PrivilegedEffect::ConfigureOwnedReachability => {
                network::configure(self, &catalog, &selection)
            }
            PrivilegedEffect::StopOwnedServices => services::stop_owned(self, &catalog, &selection),
            PrivilegedEffect::RemoveOwnedSoftwarePreservingData => {
                services::remove_owned(self, &catalog, &selection)
            }
        };
        if let Err(error) = result {
            self.authorization = None;
            self.lock = None;
            return Err(error);
        }
        journal = self.journal()?.ok_or("owned Host journal missing")?;
        if effect == PrivilegedEffect::ConfigureOwnedReachability
            && journal.pending_network_selection.as_ref() == Some(&selection)
        {
            journal.selection = selection;
            journal.pending_network_selection = None;
        }
        journal.last_effect = Some(effect);
        self.persist(&journal)?;
        let snapshot = self.snapshot()?;
        let authorization = self
            .authorization
            .as_mut()
            .ok_or("Host authorization missing")?;
        authorization.next += 1;
        authorization.snapshot = snapshot;
        if authorization.next == authorization.context.plan.expected_privileged_effects.len() {
            self.authorization = None;
            self.lock = None;
        }
        Ok(())
    }
    fn remove_owned_file(&self, path: &Path) -> Result<()> {
        let mut journal = self.journal()?.ok_or("owned file intent missing")?;
        let key = path.to_str().ok_or("invalid owned file")?;
        if path.exists() {
            let bytes = native::read_owned(path, 16 * 1024 * 1024)?;
            if !journal.owns_file_outcome(key, &sha256(&bytes)) {
                return Err("modified or foreign file preserved during removal");
            }
            fs::remove_file(path).map_err(|_| "owned software removal outcome uncertain")?;
            fs::File::open(path.parent().ok_or("invalid owned path")?)
                .and_then(|file| file.sync_all())
                .map_err(|_| "owned software removal sync failed")?;
        }
        journal.owned_files.remove(key);
        journal.pending_files.remove(key);
        self.persist(&journal)
    }
    pub fn dispatch(&mut self, request: Request) -> Response {
        let result: Result<Response> = (|| match request {
            Request::Inspect => {
                let (observation, detail) = self.observe()?;
                let generation = self.config()?.map_or(1, |c| c.generation);
                Ok(Response::Inspection {
                    observation,
                    generation,
                    detail,
                    candidates: network::candidates().unwrap_or_default(),
                    selection: self.journal()?.map(|journal| {
                        journal
                            .pending_network_selection
                            .unwrap_or(journal.selection)
                    }),
                })
            }
            Request::Review {
                mut selection,
                external_database_url,
            } => {
                self.authorization = None;
                self.lock = None;
                self.review_external = None;
                let external = if selection.profile == DeploymentProfile::AdvancedExternal {
                    if let Some(url) = external_database_url {
                        if self.config()?.is_some()
                            && !self
                                .store
                                .host_database_url()
                                .map_err(|_| "existing database credential needs repair")?
                                .with_secret(|stored| stored == url)
                        {
                            return Err(
                                "setup preserves the existing database credential; credential replacement needs an explicit operator migration",
                            );
                        }
                        Some(zeroize::Zeroizing::new(url))
                    } else {
                        Some(self.store.host_database_url().map_err(|_| "Advanced setup requires the operator's database credential during review")?.with_secret(|url| zeroize::Zeroizing::new(url.to_owned())))
                    }
                } else {
                    if external_database_url.is_some() {
                        return Err("Home setup does not accept an external database URL");
                    }
                    None
                };
                selection.external_database_target = external
                    .as_ref()
                    .map(|url| external_target(url))
                    .transpose()?;
                let context = self.build_review(selection, HostPurpose::Setup, None, None)?;
                self.review_external = external;
                self.review = Some(context.clone());
                Ok(Response::Review { context })
            }
            Request::ReviewLifecycle { purpose } => {
                self.authorization = None;
                self.lock = None;
                self.review_external = None;
                let journal = self
                    .journal()?
                    .ok_or("inspect an existing owned Host before lifecycle changes")?;
                let selection = journal
                    .pending_network_selection
                    .unwrap_or(journal.selection);
                let context = self.build_review(selection, purpose, None, None)?;
                self.review = Some(context.clone());
                Ok(Response::Review { context })
            }
            Request::ReviewNetwork { mut selection } => {
                self.authorization = None;
                self.lock = None;
                self.review_external = None;
                let journal = self
                    .journal()?
                    .ok_or("inspect an existing owned Host before changing access")?;
                if selection.profile == DeploymentProfile::AdvancedExternal {
                    let external = self
                        .store
                        .host_database_url()
                        .map_err(|_| "Advanced PostgreSQL credential needs operator repair")?;
                    selection.external_database_target =
                        Some(external.with_secret(external_target)?);
                } else {
                    selection.external_database_target = None;
                }
                let context = self.build_review(
                    selection,
                    HostPurpose::ReconfigureNetwork,
                    Some(journal.server_id),
                    None,
                )?;
                self.review = Some(context.clone());
                Ok(Response::Review { context })
            }
            Request::Confirm {
                context,
                external_database_url,
            } => {
                self.confirm(*context, external_database_url)?;
                Ok(Response::Complete)
            }
            Request::Apply {
                binding,
                sequence,
                effect,
            } => {
                self.execute(&binding, sequence, effect)?;
                Ok(Response::Complete)
            }
            Request::Account {
                login,
                password,
                create,
            } => {
                account::verify_owner(self, login, password, create)?;
                Ok(Response::Complete)
            }
            Request::EnrollDesktop { login, password } => Ok(Response::Enrollment {
                token: account::enroll_desktop(self, login, password)?,
            }),
            Request::Verify => {
                let (observation, _) = self.observe()?;
                if observation.derive_state() != synveil_server_bootstrap::HostSetupState::Ready {
                    return Err("server is not Ready; inspect canonical owners");
                }
                let journal = self.journal()?.ok_or("owned Host journal missing")?;
                Ok(Response::Ready {
                    descriptor: network::descriptor(self)?,
                    library_id: journal.library_id.ok_or("initial Library missing")?,
                })
            }
            Request::Cancel => {
                self.review = None;
                self.review_external = None;
                self.authorization = None;
                self.lock = None;
                Ok(Response::Complete)
            }
        })();
        match result {
            Ok(response) => response,
            Err(error) => {
                self.authorization = None;
                self.lock = None;
                Response::Error {
                    code: error.to_owned(),
                }
            }
        }
    }
    fn observe(&self) -> Result<(OwnerObservation, String)> {
        let mut observation = OwnerObservation {
            supported_platform: crate::supported_platform().is_some(),
            ..OwnerObservation::default()
        };
        if !observation.supported_platform {
            observation.blocked = true;
            return Ok((
                observation,
                "Managed Host supports Ubuntu 24.04 and Fedora 42 x86_64".to_owned(),
            ));
        }
        let Some(journal) = self.journal()? else {
            return Ok((observation, "Host setup has not started".to_owned()));
        };
        observation.host_intent_recorded = true;
        let selection = journal
            .pending_network_selection
            .as_ref()
            .unwrap_or(&journal.selection);
        if journal.pending_network_selection.is_some() {
            observation.needs_repair = true;
        }
        observation.server_installation_id = Some(journal.server_id.clone());
        observation.canonical_origin = Some(selection.canonical_origin.clone());
        observation.reachability_mode = Some(selection.mode);
        let catalog = match self.catalog(selection.profile, selection.mode) {
            Ok((catalog, _)) => catalog,
            Err(reason) => {
                observation.blocked = true;
                return Ok((observation, reason.to_owned()));
            }
        };
        observation.dependencies_ready = services::installed(&catalog, selection).is_ok();
        let Some(config) = self.config()? else {
            return Ok((observation, "Protected configuration is pending".to_owned()));
        };
        if config.server_installation_id != journal.server_id
            || config.deployment_profile != selection.profile
        {
            return Err("server identity conflict; preserving repair required");
        }
        observation.configuration_ready = matches!(
            self.store.inspect(),
            synveil_server_config::ConfigInspection::ValidCurrent { .. }
        );
        if let StorageConfiguration::ConfiguredLocal { root, .. }
        | StorageConfiguration::PreparingLocal { root, .. } = &config.storage
            && root != &selection.storage_root
        {
            observation.needs_repair = true;
            return Ok((observation, "Canonical storage differs from the reviewed Host; a preserving migration is required".to_owned()));
        }
        observation.storage_ready =
            matches!(config.storage, StorageConfiguration::ConfiguredLocal { .. })
                && open_existing_managed_local_storage(&config).is_ok();
        services::observe(self, &catalog, &config, &mut observation)?;
        network::observe(self, &config, &mut observation)?;
        account::observe(self, &mut observation)?;
        Ok((observation, "Readiness is derived from protected owners, service state, database migrations, HTTPS and owner/Library evidence".to_owned()))
    }
}
impl CanonicalOwners for NativeOwners {
    fn inspect(&mut self) -> std::result::Result<OwnerObservation, CoordinatorError> {
        self.observe()
            .map(|(o, _)| o)
            .map_err(|_| CoordinatorError::OwnerRejected)
    }
    fn current_generation(&mut self) -> std::result::Result<u64, CoordinatorError> {
        self.config()
            .map(|c| c.map_or(1, |c| c.generation))
            .map_err(|_| CoordinatorError::OwnerRejected)
    }
    fn apply(&mut self, effect: PrivilegedEffect) -> std::result::Result<(), CoordinatorError> {
        let authorization = self
            .authorization
            .as_ref()
            .ok_or(CoordinatorError::ReviewRequired)?;
        let binding = authorization.context.binding.clone();
        let sequence = authorization.next;
        self.execute(&binding, sequence, effect)
            .map_err(|_| CoordinatorError::OwnerRejected)
    }
    fn trust_descriptor(&mut self) -> std::result::Result<String, CoordinatorError> {
        network::descriptor(self).map_err(|_| CoordinatorError::NotReady)
    }
}
