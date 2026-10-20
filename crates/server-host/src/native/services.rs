use super::*;
use std::{
    collections::BTreeSet,
    net::TcpListener,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use synveil_server_config::{DatabaseCredentialState, DatabaseEndpoint};
use synveil_server_service::{CLUSTER_MARKER_NAME, MANAGED_DATA_DIRECTORY, ManagedClusterIdentity};

const UNITS: &[(&str, &str)] = &[
    (
        "synveil-api.service",
        include_str!("../../../../deploy/systemd/synveil-api.service"),
    ),
    (
        "synveil-worker.service",
        include_str!("../../../../deploy/systemd/synveil-worker.service"),
    ),
    (
        "synveil-server-migrate.service",
        include_str!("../../../../deploy/systemd/synveil-server-migrate.service"),
    ),
    (
        "synveil-server.target",
        include_str!("../../../../deploy/systemd/synveil-server.target"),
    ),
    (
        "synveil-postgresql.service",
        include_str!("../../../../deploy/systemd/synveil-postgresql.service"),
    ),
    (
        "synveil-edge.service",
        include_str!("../../../../deploy/systemd/synveil-edge.service"),
    ),
];
const MANAGED_DROP_INS: &[(&str, &str)] = &[
    (
        "/usr/lib/systemd/system/synveil-server.target.d/10-managed-postgresql.conf",
        include_str!("../../../../deploy/server/systemd-managed/synveil-server.target.conf"),
    ),
    (
        "/usr/lib/systemd/system/synveil-server-migrate.service.d/10-managed-postgresql.conf",
        include_str!(
            "../../../../deploy/server/systemd-managed/synveil-server-migrate.service.conf"
        ),
    ),
];
fn verify_drop_ins(paths: &str, profile: DeploymentProfile) -> Result<()> {
    for path in paths.split_whitespace() {
        let (_, template) = MANAGED_DROP_INS
            .iter()
            .find(|(known, _)| *known == path)
            .ok_or("unexpected systemd drop-in requires operator reconciliation")?;
        if profile != DeploymentProfile::PersonalHomeManaged
            || native::read_owned(Path::new(path), 16 * 1024)? != template.as_bytes()
        {
            return Err("modified or wrong-profile systemd drop-in");
        }
    }
    Ok(())
}
pub fn runtime_root(kind: &str, digest: &str) -> PathBuf {
    if kind == "postgresql" {
        Path::new("/opt/synveil/postgresql/17").join(digest)
    } else {
        Path::new("/opt/synveil").join(kind).join(digest)
    }
}
pub fn units(
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<BTreeMap<String, String>> {
    let server = runtime_root(
        "server",
        catalog
            .artifacts
            .get("server")
            .ok_or("server runtime missing")?,
    );
    let mut result = BTreeMap::new();
    for (name, template) in UNITS {
        if *name == "synveil-postgresql.service"
            && selection.profile == DeploymentProfile::AdvancedExternal
        {
            continue;
        }
        if *name == "synveil-edge.service"
            && selection.mode == synveil_server_config::ReachabilityMode::AdvancedExternalHttps
        {
            continue;
        }
        let mut text = template.replace(
            "/opt/synveil/server/current/",
            &format!("{}/bin/", server.display()),
        );
        if let Some(pg) = catalog.artifacts.get("postgresql") {
            text = text.replace(
                "/opt/synveil/postgresql/17/current",
                &runtime_root("postgresql", pg).display().to_string(),
            );
        }
        if let Some(edge) = catalog.artifacts.get("caddy") {
            text = text.replace(
                "/opt/synveil/edge/current/caddy",
                &format!("{}/bin/caddy", runtime_root("caddy", edge).display()),
            );
        }
        // admin is disabled in the Caddyfile: reload through its admin endpoint
        // is unavailable. Leaf/config renewal uses a bounded service restart.
        text = text
            .lines()
            .filter(|line| !line.starts_with("ExecReload=") || *name != "synveil-edge.service")
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        if *name == "synveil-server.target"
            && selection.mode == synveil_server_config::ReachabilityMode::AdvancedExternalHttps
        {
            text = text.replace(" synveil-edge.service", "");
        }
        if *name == "synveil-worker.service" {
            text.push_str("\n[Service]\nRuntimeDirectory=synveil-worker\nRuntimeDirectoryMode=0700\nReadWritePaths=/run/synveil-worker\nEnvironment=SYNVEIL_WORKER_HEALTH_FILE=/run/synveil-worker/health.json\n");
        }
        if ["synveil-api.service", "synveil-worker.service"].contains(name) {
            let root =
                synveil_server_service::systemd_path_value(Path::new(&selection.storage_root))
                    .map_err(|_| "invalid service storage path")?;
            text.push_str(&format!("\n[Service]\nReadWritePaths={root}\n"));
        }
        if *name == "synveil-postgresql.service" {
            text = text
                .replace("/run/synveil/postgresql", "/run/synveil-postgresql")
                .replace(
                    "RuntimeDirectory=synveil/postgresql",
                    "RuntimeDirectory=synveil-postgresql",
                );
        }
        if *name == "synveil-edge.service"
            && selection.mode == synveil_server_config::ReachabilityMode::PrivateLan
        {
            text.push_str("\n[Unit]\nWants=network-online.target\nAfter=network-online.target\n");
        }
        if *name == "synveil-server-migrate.service"
            && selection.profile == DeploymentProfile::PersonalHomeManaged
        {
            text.push_str(
                "\n[Unit]\nRequires=synveil-postgresql.service\nAfter=synveil-postgresql.service\n",
            );
        }
        if *name == "synveil-server.target"
            && selection.profile == DeploymentProfile::PersonalHomeManaged
        {
            text.push_str(
                "\n[Unit]\nWants=synveil-postgresql.service\nAfter=synveil-postgresql.service\n",
            );
        }
        result.insert((*name).to_owned(), text);
    }
    Ok(result)
}
pub fn preflight(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<()> {
    native::root_path(Path::new("/usr/bin/systemctl"))?;
    if !Path::new("/run/systemd/system").is_dir() {
        return Err("Managed Host requires the native systemd system manager");
    }
    let journal = owner.journal()?;
    let mut inventory = units(catalog, selection)?;
    if let Some(journal) = &journal {
        inventory.extend(units(catalog, &journal.selection)?);
    }
    for (name, _) in inventory {
        let path = Path::new("/etc/systemd/system").join(&name);
        if path.exists() {
            let bytes = native::read_owned(&path, 64 * 1024)?;
            if !journal.as_ref().is_some_and(|j| {
                path.to_str()
                    .is_some_and(|path| j.owns_file_outcome(path, &sha256(&bytes)))
            }) {
                return Err(
                    "foreign or modified systemd service; operator reconciliation required",
                );
            }
        }
        let drop_ins = Path::new("/etc/systemd/system").join(format!("{name}.d"));
        if drop_ins.exists()
            && fs::read_dir(drop_ins)
                .map_err(|_| "unit override inspection failed")?
                .next()
                .is_some()
        {
            return Err("unexpected systemd overrides require operator review");
        }
        if let Ok(bytes) = native::systemctl(&["show", &name, "--property=DropInPaths", "--value"])
        {
            verify_drop_ins(
                std::str::from_utf8(&bytes).map_err(|_| "invalid systemd drop-in inspection")?,
                selection.profile,
            )?;
        }
    }
    Ok(())
}
pub fn install_runtimes(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<()> {
    let (_, manifests) = owner.catalog(selection.profile, selection.mode)?;
    native::directory(Path::new("/opt/synveil"), 0o755)?;
    for (kind, manifest) in manifests {
        let digest = &catalog.artifacts[&kind];
        let destination = runtime_root(&kind, digest);
        let mut journal = owner.journal()?.ok_or("runtime ownership intent missing")?;
        if destination.exists() && journal.runtime_intents.get(&kind) != Some(digest) {
            return Err("foreign runtime namespace; adoption is forbidden");
        }
        if let Some(old) = journal
            .runtime_intents
            .get(&kind)
            .filter(|old| *old != digest)
            .cloned()
        {
            if owner
                .authorization
                .as_ref()
                .is_none_or(|authorization| authorization.context.purpose != HostPurpose::Upgrade)
            {
                return Err("runtime upgrade requires a separate reviewed ownership transition");
            }
            let retained = journal.retained_runtimes.entry(kind.clone()).or_default();
            if !retained.contains(&old) {
                retained.push(old);
            }
        }
        journal.runtime_intents.insert(kind.clone(), digest.clone());
        owner.persist(&journal)?;
        let parent = destination.parent().ok_or("invalid runtime namespace")?;
        if kind == "postgresql" {
            native::directory(Path::new("/opt/synveil/postgresql"), 0o755)?;
        }
        native::directory(parent, 0o755)?;
        if destination.join("manifest.json").exists() {
            native::verify_runtime(&destination, digest, &catalog.qualified_os)?;
            continue;
        }
        native::directory(&destination, 0o755)?;
        let source = Path::new("/var/cache/synveil/host-runtime").join(digest);
        // A partial namespace may be safely resumed only with identical bytes.
        for file in &manifest.files {
            let target = destination.join(&file.path);
            let relative_parent = Path::new(&file.path)
                .parent()
                .ok_or("invalid runtime member")?;
            let mut at = destination.clone();
            for part in relative_parent.components() {
                at.push(part);
                native::directory(&at, 0o755)?;
            }
            let bytes = native::read_owned(&source.join(&file.path), file.bytes as usize)?;
            native::write_owned(
                &target,
                &bytes,
                None,
                if file.executable { 0o755 } else { 0o644 },
            )?;
        }
        native::write_owned(
            &destination.join("manifest.json"),
            &native::read_owned(&source.join("manifest.json"), 2 * 1024 * 1024)?,
            None,
            0o644,
        )?;
        native::verify_runtime(&destination, digest, &catalog.qualified_os)?;
    }
    let mut definitions = include_str!("../../../../deploy/sysusers.d/synveil.conf").to_owned();
    if selection.profile == DeploymentProfile::PersonalHomeManaged {
        definitions.push_str(include_str!(
            "../../../../deploy/sysusers.d/synveil-postgres.conf"
        ));
        definitions.push_str("\nm synveil-postgres synveil\n");
    }
    if selection.mode != synveil_server_config::ReachabilityMode::AdvancedExternalHttps {
        definitions.push_str(include_str!(
            "../../../../deploy/sysusers.d/synveil-edge.conf"
        ));
        definitions.push_str("\nm synveil-edge synveil\n");
    }
    // Fixed declarative input, never caller-controlled account names or argv.
    native::run(
        Path::new("/usr/bin/systemd-sysusers"),
        &["-".to_owned()],
        definitions.as_bytes(),
        Duration::from_secs(30),
    )?;
    let (uid, gid) = native::service_user("synveil")?;
    if selection.profile == DeploymentProfile::PersonalHomeManaged {
        native::service_user("synveil-postgres")?;
    }
    if selection.mode != synveil_server_config::ReachabilityMode::AdvancedExternalHttps {
        native::service_user("synveil-edge")?;
    }
    let parent = Path::new("/var/lib/synveil");
    native::root_path(Path::new("/var/lib"))?;
    let metadata = fs::symlink_metadata(parent).map_err(|_| "package state parent missing")?;
    if metadata.uid() == uid
        && metadata.gid() == gid
        && metadata.mode() & 0o777 == 0o750
        && metadata.is_dir()
        && !metadata.file_type().is_symlink()
    {
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(parent)
            .map_err(|_| "state parent unavailable")?;
        use std::os::fd::AsRawFd;
        let actual = directory
            .metadata()
            .map_err(|_| "state parent unavailable")?;
        if (metadata.dev(), metadata.ino()) != (actual.dev(), actual.ino()) {
            return Err("state parent changed during reviewed handoff");
        }
        nix::unistd::fchown(
            directory.as_raw_fd(),
            Some(nix::unistd::Uid::from_raw(0)),
            Some(nix::unistd::Gid::from_raw(gid)),
        )
        .map_err(|_| "state parent handoff failed")?;
        use std::os::unix::fs::PermissionsExt;
        directory
            .set_permissions(fs::Permissions::from_mode(0o755))
            .map_err(|_| "state parent permissions failed")?;
        directory
            .sync_all()
            .map_err(|_| "state parent sync failed")?;
    }
    native::root_path(parent)?;
    Ok(())
}
pub fn installed(catalog: &RuntimeCatalog, selection: &HostSelection) -> Result<()> {
    for kind in ["server", "postgresql", "caddy"] {
        if kind == "postgresql" && selection.profile == DeploymentProfile::AdvancedExternal
            || kind == "caddy"
                && selection.mode == synveil_server_config::ReachabilityMode::AdvancedExternalHttps
        {
            continue;
        }
        let digest = catalog.artifacts.get(kind).ok_or("runtime missing")?;
        native::verify_runtime(&runtime_root(kind, digest), digest, &catalog.qualified_os)?;
    }
    Ok(())
}
pub fn review_upgrade(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<Vec<String>> {
    let journal = owner
        .journal()?
        .ok_or("an owned installation is required for upgrade")?;
    let config = owner.config()?.ok_or("server configuration missing")?;
    if !matches!(config.storage, StorageConfiguration::ConfiguredLocal { .. }) {
        return Err("finish storage repair before reviewing an upgrade");
    }
    let (_, new_manifests) = owner.catalog(selection.profile, selection.mode)?;
    let mut identities = Vec::new();
    fn version(value: &str) -> Result<Vec<u32>> {
        let components: Vec<_> = value
            .split('.')
            .map(|component| {
                component
                    .parse::<u32>()
                    .map_err(|_| "numeric pinned runtime version required for upgrade")
            })
            .collect::<Result<_>>()?;
        if !(2..=4).contains(&components.len()) {
            return Err("unsupported runtime version shape");
        }
        Ok(components)
    }
    for (kind, new_manifest) in new_manifests {
        let old = journal
            .runtime_intents
            .get(&kind)
            .ok_or("old runtime ownership identity missing")?;
        let old_manifest =
            native::verify_runtime(&runtime_root(&kind, old), old, &catalog.qualified_os).or_else(
                |_| {
                    // Resume an already approved, partially assembled new namespace
                    // against its original retained runtime, never an unknown source.
                    if catalog.artifacts.get(&kind) != Some(old) {
                        return Err("old runtime identity unavailable");
                    }
                    let retained = journal
                        .retained_runtimes
                        .get(&kind)
                        .and_then(|old| old.last())
                        .ok_or("old runtime identity unavailable")?;
                    native::verify_runtime(
                        &runtime_root(&kind, retained),
                        retained,
                        &catalog.qualified_os,
                    )
                },
            )?;
        if old_manifest.kind != kind
            || old_manifest.compatibility != new_manifest.compatibility
            || old_manifest.qualified_os != new_manifest.qualified_os
            || version(&new_manifest.version)? < version(&old_manifest.version)?
        {
            return Err(
                "runtime downgrade or compatibility change is outside the reviewed upgrade boundary",
            );
        }
        if kind == "postgresql"
            && (!old_manifest.version.starts_with("17.")
                || !new_manifest.version.starts_with("17."))
        {
            return Err("PostgreSQL major upgrades are not permitted");
        }
        identities.push(format!("old:{kind}:{old}:{}", old_manifest.version));
        if let Some(retained) = journal.retained_runtimes.get(&kind) {
            for digest in retained {
                let manifest = native::verify_runtime(
                    &runtime_root(&kind, digest),
                    digest,
                    &catalog.qualified_os,
                )?;
                identities.push(format!("retained:{kind}:{digest}:{}", manifest.version));
            }
        }
    }
    Ok(identities)
}
pub fn handoff_storage(owner: &NativeOwners) -> Result<()> {
    let config = owner.config()?.ok_or("configuration missing")?;
    let StorageConfiguration::ConfiguredLocal { root, .. } = &config.storage else {
        return Err("storage not configured");
    };
    open_existing_managed_local_storage(&config).map_err(|_| "storage identity conflict")?;
    let (uid, gid) = native::service_user("synveil")?;
    // Preserve the root inode and root ownership checked by P032. Only closed
    // ObjectStore directories become writable by the service; never recurse.
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    let open_directory = |path: &Path| {
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| "storage directory unavailable")
    };
    let root_fd = open_directory(Path::new(root))?;
    let relative = |fd: &fs::File, child: &str| {
        PathBuf::from(format!("/proc/self/fd/{}/{}", fd.as_raw_fd(), child))
    };
    let objects = open_directory(&relative(&root_fd, "objects"))?;
    let layout = open_directory(&relative(&objects, "v1"))?;
    let staging = open_directory(&relative(&root_fd, "staging"))?;
    // Open the closed directory inventory before changing any ownership. Use
    // descriptors for the handoff so renamed/symlinked children cannot redirect
    // a privileged chown outside this storage root.
    for (directory, expected_uid, mode) in [
        (&root_fd, 0, 0o750),
        (&objects, uid, 0o700),
        (&layout, uid, 0o700),
        (&staging, uid, 0o700),
    ] {
        let metadata = directory
            .metadata()
            .map_err(|_| "storage directory identity unavailable")?;
        if !metadata.is_dir()
            || ![0, expected_uid].contains(&metadata.uid())
            || metadata.mode() & 0o022 != 0
        {
            return Err("storage directory ownership conflict");
        }
        nix::unistd::fchown(
            directory.as_raw_fd(),
            Some(nix::unistd::Uid::from_raw(expected_uid)),
            Some(nix::unistd::Gid::from_raw(gid)),
        )
        .map_err(|_| "storage ownership handoff failed")?;
        directory
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|_| "storage permission handoff failed")?;
        directory
            .sync_all()
            .map_err(|_| "storage directory sync failed")?;
    }
    open_existing_managed_local_storage(&config)
        .map_err(|_| "storage permission handoff failed")?;
    Ok(())
}
fn pg_command(
    catalog: &RuntimeCatalog,
    binary: &str,
    args: &[String],
    input: &[u8],
) -> Result<Vec<u8>> {
    let runtime = runtime_root(
        "postgresql",
        catalog
            .artifacts
            .get("postgresql")
            .ok_or("PostgreSQL runtime missing")?,
    );
    let program = runtime.join("bin").join(binary);
    native::root_path(&program)?;
    let mut command = vec![
        "--user".to_owned(),
        "synveil-postgres".to_owned(),
        "--".to_owned(),
        program.display().to_string(),
    ];
    command.extend_from_slice(args);
    native::run(
        Path::new("/usr/sbin/runuser"),
        &command,
        input,
        Duration::from_secs(120),
    )
}
fn system_id(catalog: &RuntimeCatalog) -> Result<String> {
    let output = pg_command(
        catalog,
        "pg_controldata",
        &[MANAGED_DATA_DIRECTORY.to_owned()],
        &[],
    )?;
    let text = std::str::from_utf8(&output).map_err(|_| "invalid PostgreSQL control data")?;
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix("Database system identifier:"))
        .map(str::trim)
        .ok_or("PostgreSQL system identity missing")?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid PostgreSQL system identity");
    }
    Ok(value.to_owned())
}
fn active_system_identity(catalog: &RuntimeCatalog, port: u16) -> Result<String> {
    let args = vec![
        "--no-psqlrc".to_owned(),
        "--set=ON_ERROR_STOP=1".to_owned(),
        "--host=/run/synveil-postgresql".to_owned(),
        format!("--port={port}"),
        "--username=synveil-bootstrap".to_owned(),
        "--dbname=postgres".to_owned(),
        "--tuples-only".to_owned(),
        "--no-align".to_owned(),
    ];
    let result = pg_command(
        catalog,
        "psql",
        &args,
        b"SELECT system_identifier::text || ':' || current_setting('port') FROM pg_control_system();\n",
    )?;
    let identity = String::from_utf8(result)
        .map_err(|_| "invalid live PostgreSQL identity")?
        .trim()
        .to_owned();
    let (system_id, actual_port) = identity
        .split_once(':')
        .ok_or("live PostgreSQL identity unavailable")?;
    if system_id.is_empty()
        || !system_id.bytes().all(|byte| byte.is_ascii_digit())
        || actual_port.parse::<u16>().ok() != Some(port)
    {
        return Err("live PostgreSQL cluster or port identity mismatch");
    }
    Ok(system_id.to_owned())
}
fn verify_persisted_port(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    journal: &Journal,
) -> Result<()> {
    let port = journal
        .postgres_port
        .ok_or("private database port missing")?;
    if unit_active("synveil-postgresql.service") {
        effective_unit(owner, "synveil-postgresql.service")?;
        if Some(active_system_identity(catalog, port)?) != journal.postgres_system_id {
            return Err("live PostgreSQL listener belongs to a different cluster");
        }
    } else if TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err() {
        return Err("persisted private PostgreSQL port is occupied by an unrelated listener");
    }
    Ok(())
}
fn archive_name(server_id: &str, index: usize) -> String {
    format!("interrupted-initdb-{server_id}-{index}")
}
fn preserved_parent() -> PathBuf {
    Path::new("/var/lib/synveil/postgresql/17/preserved").to_path_buf()
}
/// Return the exact directory identity that an explicit preserving repair may
/// archive. It never adopts a directory that was not recorded before initdb.
pub fn review_postgres_repair(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<Option<(String, u64, u64)>> {
    if selection.profile != DeploymentProfile::PersonalHomeManaged {
        return Ok(None);
    }
    let journal = owner.journal()?.ok_or("Host intent missing")?;
    let data = Path::new(MANAGED_DATA_DIRECTORY);
    let marker_path = data
        .parent()
        .ok_or("invalid cluster path")?
        .join(CLUSTER_MARKER_NAME);
    if !marker_path.exists() {
        if !data.exists()
            && journal.postgres_data_identity.is_none()
            && journal.pending_postgres_archive.is_none()
            && journal.preserved_postgres_data.is_empty()
        {
            return Ok(None);
        }
        return Err("PostgreSQL ownership marker is missing; preserve and inspect the data");
    }
    let (postgres_uid, _) = native::service_user("synveil-postgres")?;
    let marker: ManagedClusterIdentity =
        serde_json::from_slice(&native::read_owned(&marker_path, 16 * 1024)?)
            .map_err(|_| "managed PostgreSQL marker needs preserving repair")?;
    marker
        .validate()
        .map_err(|_| "managed PostgreSQL identity conflict")?;
    if marker.server_installation_id != journal.server_id
        || catalog.artifacts.get("postgresql") != Some(&marker.runtime_artifact_id)
    {
        return Err("PostgreSQL state is not an inactive interrupted initialization");
    }
    if journal.preserved_postgres_data.len() >= 16 {
        return Err("PostgreSQL preservation archive limit reached; operator recovery required");
    }
    let expected_name = archive_name(&journal.server_id, journal.preserved_postgres_data.len());
    let target_name = if let Some(pending) = &journal.pending_postgres_archive {
        if pending.archive_name != expected_name {
            return Err("PostgreSQL preservation journal needs operator repair");
        }
        pending.archive_name.clone()
    } else {
        expected_name
    };
    let preserved = preserved_parent();
    if preserved.exists() {
        native::root_path(&preserved)?;
        let metadata = fs::symlink_metadata(&preserved)
            .map_err(|_| "PostgreSQL preservation directory unavailable")?;
        if metadata.uid() != 0 || metadata.mode() & 0o7777 != 0o700 {
            return Err("PostgreSQL preservation directory needs operator repair");
        }
    } else if journal.pending_postgres_archive.is_some() {
        return Err("pending PostgreSQL preservation directory is missing");
    }
    if !journal.preserved_postgres_data.is_empty() {
        native::root_path(&preserved)?;
        for (index, entry) in journal.preserved_postgres_data.iter().enumerate() {
            if entry.archive_name != archive_name(&journal.server_id, index) {
                return Err("PostgreSQL preservation archive inventory needs operator repair");
            }
            let path = preserved.join(&entry.archive_name);
            let saved = fs::symlink_metadata(path)
                .map_err(|_| "previously preserved PostgreSQL data is missing")?;
            if !saved.is_dir()
                || saved.file_type().is_symlink()
                || (saved.dev(), saved.ino()) != (entry.device, entry.inode)
            {
                return Err("previously preserved PostgreSQL data identity changed");
            }
        }
    }
    let source_metadata = match fs::symlink_metadata(data) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("PostgreSQL data directory inspection failed"),
    };
    let archive = preserved.join(&target_name);
    let archive_metadata = match fs::symlink_metadata(&archive) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("PostgreSQL preservation archive inspection failed"),
    };
    if let Some(pending) = &journal.pending_postgres_archive {
        if journal.postgres_data_identity != Some((pending.device, pending.inode)) {
            return Err("pending PostgreSQL preservation identity changed");
        }
        let source_matches = source_metadata.as_ref().is_some_and(|metadata| {
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && [0, postgres_uid].contains(&metadata.uid())
                && metadata.mode() & 0o077 == 0
                && (metadata.dev(), metadata.ino()) == (pending.device, pending.inode)
        });
        let archive_matches = archive_metadata.as_ref().is_some_and(|metadata| {
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && [0, postgres_uid].contains(&metadata.uid())
                && metadata.mode() & 0o077 == 0
                && (metadata.dev(), metadata.ino()) == (pending.device, pending.inode)
        });
        if source_matches == archive_matches {
            return Err("PostgreSQL interrupted preservation outcome is ambiguous");
        }
        return Ok(Some((target_name, pending.device, pending.inode)));
    }
    let Some(metadata) = source_metadata.as_ref() else {
        if journal.postgres_data_identity.is_none() || !journal.preserved_postgres_data.is_empty() {
            return Ok(None);
        }
        return Err("PostgreSQL data directory is missing from its recorded identity");
    };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || ![0, postgres_uid].contains(&metadata.uid())
        || metadata.mode() & 0o077 != 0
        || archive_metadata.is_some()
    {
        return Err("PostgreSQL data directory needs preserving operator repair");
    }
    let identity = (metadata.dev(), metadata.ino());
    if fs::read_dir(data)
        .map_err(|_| "PostgreSQL data directory unavailable")?
        .next()
        .is_none()
    {
        if marker.postgres_system_identifier.is_some() || journal.postgres_system_id.is_some() {
            return Err("initialized PostgreSQL identity has an empty data directory");
        }
        if journal
            .postgres_data_identity
            .is_some_and(|old| old != identity)
        {
            return Err("empty PostgreSQL data directory identity changed");
        }
        return Ok(None);
    }
    if journal.postgres_data_identity != Some(identity) {
        return Err("nonempty PostgreSQL data directory is not owned by this Host journal");
    }
    if marker.postgres_system_identifier.is_some() || journal.postgres_system_id.is_some() {
        let actual = system_id(catalog)?;
        if marker
            .postgres_system_identifier
            .as_ref()
            .is_some_and(|expected| expected != &actual)
            || journal
                .postgres_system_id
                .as_ref()
                .is_some_and(|expected| expected != &actual)
        {
            return Err("initialized PostgreSQL identity needs preserving operator repair");
        }
        return Ok(None);
    }
    if unit_active("synveil-postgresql.service") {
        return Err("stop the owned PostgreSQL service before preserving an interrupted directory");
    }
    if archive_metadata.is_some() {
        return Err("PostgreSQL preservation target already exists");
    }
    Ok(Some((target_name, identity.0, identity.1)))
}
fn preserve_interrupted_postgres(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
    journal: &mut Journal,
) -> Result<()> {
    if owner
        .authorization
        .as_ref()
        .is_none_or(|authorization| authorization.context.purpose != HostPurpose::RepairHost)
    {
        return Err("interrupted initdb needs a separately reviewed preserving repair");
    }
    let Some((name, device, inode)) = review_postgres_repair(owner, catalog, selection)? else {
        return Ok(());
    };
    let parent = preserved_parent();
    if !parent.exists() {
        native::directory(&parent, 0o700)?;
    }
    let parent_metadata = fs::symlink_metadata(&parent)
        .map_err(|_| "PostgreSQL preservation directory unavailable")?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != 0
        || parent_metadata.mode() & 0o7777 != 0o700
    {
        return Err("PostgreSQL preservation directory needs operator repair");
    }
    let archive = parent.join(&name);
    if journal.pending_postgres_archive.is_none() {
        journal.pending_postgres_archive = Some(PendingPostgresArchive {
            archive_name: name.clone(),
            device,
            inode,
        });
        owner.persist(journal)?;
    }
    let data = Path::new(MANAGED_DATA_DIRECTORY);
    match (fs::symlink_metadata(data), fs::symlink_metadata(&archive)) {
        (Ok(source), Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            if !source.is_dir()
                || source.file_type().is_symlink()
                || (source.dev(), source.ino()) != (device, inode)
            {
                return Err("PostgreSQL data directory changed before preservation");
            }
            fs::rename(data, &archive).map_err(|_| "PostgreSQL directory preservation failed")?;
            fs::File::open(data.parent().ok_or("invalid PostgreSQL path")?)
                .and_then(|file| file.sync_all())
                .map_err(|_| "PostgreSQL preservation sync failed")?;
            fs::File::open(&parent)
                .and_then(|file| file.sync_all())
                .map_err(|_| "PostgreSQL preservation sync failed")?;
        }
        (Err(source), Ok(saved)) if source.kind() == std::io::ErrorKind::NotFound => {
            if !saved.is_dir()
                || saved.file_type().is_symlink()
                || (saved.dev(), saved.ino()) != (device, inode)
            {
                return Err("PostgreSQL preservation archive identity changed");
            }
        }
        _ => return Err("PostgreSQL preservation outcome is ambiguous"),
    }
    let saved = fs::symlink_metadata(&archive)
        .map_err(|_| "PostgreSQL preservation archive unavailable")?;
    if (saved.dev(), saved.ino()) != (device, inode) {
        return Err("PostgreSQL preservation archive identity changed");
    }
    if !journal
        .preserved_postgres_data
        .iter()
        .any(|entry| entry.archive_name == name)
    {
        journal.preserved_postgres_data.push(PreservedPostgresData {
            archive_name: name,
            device,
            inode,
        });
    }
    journal.pending_postgres_archive = None;
    journal.postgres_data_identity = None;
    journal.postgres_system_id = None;
    owner.persist(journal)
}
fn prepare_postgres(owner: &NativeOwners, catalog: &RuntimeCatalog) -> Result<()> {
    let mut journal = owner.journal()?.ok_or("Host intent missing")?;
    let (uid, gid) = native::service_user("synveil-postgres")?;
    for path in [
        "/var/lib/synveil",
        "/var/lib/synveil/postgresql",
        "/var/lib/synveil/postgresql/17",
        "/etc/synveil/postgresql",
    ] {
        native::directory(Path::new(path), 0o750)?;
    }
    for path in [
        "/var/lib/synveil/postgresql",
        "/var/lib/synveil/postgresql/17",
        "/etc/synveil/postgresql",
    ] {
        native::handoff_directory(Path::new(path), 0, gid, 0o750)?;
    }
    let data = Path::new(MANAGED_DATA_DIRECTORY);
    let marker_path = data
        .parent()
        .ok_or("invalid cluster path")?
        .join(CLUSTER_MARKER_NAME);
    let mut marker = if marker_path.exists() {
        let marker: ManagedClusterIdentity =
            serde_json::from_slice(&native::read_owned(&marker_path, 16 * 1024)?)
                .map_err(|_| "cluster marker needs repair")?;
        marker
            .validate()
            .map_err(|_| "cluster marker identity conflict")?;
        let runtime_matches = marker.runtime_artifact_id == catalog.artifacts["postgresql"]
            || (owner.authorization.as_ref().is_some_and(|authorization| {
                authorization.context.purpose == HostPurpose::Upgrade
            }) && journal
                .retained_runtimes
                .get("postgresql")
                .is_some_and(|old| old.contains(&marker.runtime_artifact_id)));
        if marker.server_installation_id != journal.server_id || !runtime_matches {
            return Err("foreign or incompatible PostgreSQL cluster");
        }
        marker
    } else {
        if data.exists()
            && fs::read_dir(data)
                .map_err(|_| "cluster unavailable")?
                .next()
                .is_some()
        {
            return Err("foreign PostgreSQL data directory; adoption is forbidden");
        }
        let marker = ManagedClusterIdentity::new(
            journal.server_id.clone(),
            catalog.artifacts["postgresql"].clone(),
        );
        owner.publish(
            &marker_path,
            &serde_json::to_vec(&marker).map_err(|_| "cluster marker encoding failed")?,
            0o600,
        )?;
        marker
    };
    if owner
        .authorization
        .as_ref()
        .is_some_and(|authorization| authorization.context.purpose == HostPurpose::RepairHost)
    {
        let selection = journal.selection.clone();
        preserve_interrupted_postgres(owner, catalog, &selection, &mut journal)?;
    }
    if !data.exists() {
        native::directory(data, 0o700)?;
    }
    let metadata = fs::symlink_metadata(data).map_err(|_| "cluster data missing")?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || ![0, uid].contains(&metadata.uid())
        || metadata.mode() & 0o077 != 0
    {
        return Err("unsafe cluster data directory");
    }
    let identity = (metadata.dev(), metadata.ino());
    if journal
        .postgres_data_identity
        .is_some_and(|previous| previous != identity)
    {
        return Err("PostgreSQL data directory identity changed");
    }
    journal.postgres_data_identity = Some(identity);
    if journal.postgres_port.is_none() {
        journal.postgres_port = (synveil_server_service::MANAGED_PORT_FIRST
            ..=synveil_server_service::MANAGED_PORT_LAST)
            .find(|port| TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, *port)).is_ok());
        if journal.postgres_port.is_none() {
            return Err("no approved private PostgreSQL port available");
        }
    }
    verify_persisted_port(owner, catalog, &journal)?;
    owner.persist(&journal)?;
    native::handoff_directory(data, uid, gid, 0o700)?;
    let version = data.join("PG_VERSION");
    if !version.exists() {
        if fs::read_dir(data)
            .map_err(|_| "cluster unavailable")?
            .next()
            .is_some()
        {
            return Err(
                "interrupted initdb needs preserving operator repair; never reinitialize nonempty data",
            );
        }
        pg_command(
            catalog,
            "initdb",
            &[
                "-D".to_owned(),
                MANAGED_DATA_DIRECTORY.to_owned(),
                "--username=synveil-bootstrap".to_owned(),
                "--auth-local=peer".to_owned(),
                "--auth-host=reject".to_owned(),
                "--encoding=UTF8".to_owned(),
                "--locale=C.UTF-8".to_owned(),
                "--data-checksums".to_owned(),
            ],
            &[],
        )?;
    }
    let version_file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(version)
        .map_err(|_| "cluster PG_VERSION unavailable")?;
    use std::io::Read;
    let mut version_bytes = Vec::new();
    version_file
        .take(16)
        .read_to_end(&mut version_bytes)
        .map_err(|_| "cluster PG_VERSION unreadable")?;
    if version_bytes != b"17\n" {
        return Err("PostgreSQL major version mismatch; automatic upgrade forbidden");
    }
    let actual = system_id(catalog)?;
    if marker
        .postgres_system_identifier
        .as_ref()
        .is_some_and(|id| id != &actual)
        || journal
            .postgres_system_id
            .as_ref()
            .is_some_and(|id| id != &actual)
    {
        return Err("PostgreSQL system identity mismatch");
    }
    marker.postgres_system_identifier = Some(actual.clone());
    marker.runtime_artifact_id = catalog.artifacts["postgresql"].clone();
    journal.postgres_system_id = Some(actual);
    owner.persist(&journal)?;
    owner.publish(
        &marker_path,
        &serde_json::to_vec(&marker).map_err(|_| "cluster marker encoding failed")?,
        0o600,
    )?;
    let port = journal
        .postgres_port
        .ok_or("private database endpoint missing")?;
    let conf = format!(
        "listen_addresses = '127.0.0.1'\nport = {port}\nunix_socket_directories = '/run/synveil-postgresql'\nunix_socket_permissions = 0700\npassword_encryption = 'scram-sha-256'\nmax_connections = 50\nlogging_collector = off\nhba_file = '/etc/synveil/postgresql/pg_hba.conf'\nident_file = '/etc/synveil/postgresql/pg_ident.conf'\n"
    );
    owner.publish(
        Path::new("/etc/synveil/postgresql/postgresql.conf"),
        conf.as_bytes(),
        0o644,
    )?;
    owner.publish(Path::new("/etc/synveil/postgresql/pg_hba.conf"), b"local all synveil-bootstrap peer map=synveil-bootstrap\nlocal all all reject\nhost synveil synveil 127.0.0.1/32 scram-sha-256\nhost all all 0.0.0.0/0 reject\nhost all all ::0/0 reject\n", 0o644)?;
    owner.publish(
        Path::new("/etc/synveil/postgresql/pg_ident.conf"),
        b"synveil-bootstrap synveil-postgres synveil-bootstrap\n",
        0o644,
    )?;
    Ok(())
}
fn provision_database(owner: &NativeOwners, catalog: &RuntimeCatalog) -> Result<()> {
    let journal = owner.journal()?.ok_or("Host intent missing")?;
    let port = journal
        .postgres_port
        .ok_or("private PostgreSQL endpoint missing")?;
    if Some(active_system_identity(catalog, port)?) != journal.postgres_system_id {
        return Err("listening PostgreSQL cluster identity differs from the reviewed Host");
    }
    let args = vec![
        "--no-psqlrc".to_owned(),
        "--set=ON_ERROR_STOP=1".to_owned(),
        "--host=/run/synveil-postgresql".to_owned(),
        format!("--port={port}"),
        "--username=synveil-bootstrap".to_owned(),
        "--dbname=postgres".to_owned(),
        "--tuples-only".to_owned(),
        "--no-align".to_owned(),
    ];
    let password = owner
        .store
        .managed_database_password()
        .map_err(|_| "protected database password unavailable")?;
    let ownership = format!("synveil:{}", journal.server_id);
    let database_owner = format!("synveil-owner-{}", journal.server_id);
    let sql = password.with_secret(|password| zeroize::Zeroizing::new(format!("SET log_statement=none;\nSET log_min_error_statement=panic;\nSET log_error_verbosity=terse;\nSET log_parameter_max_length_on_error=0;\nBEGIN;\nDO $host$ BEGIN IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='synveil') THEN IF (SELECT shobj_description(oid,'pg_authid') FROM pg_roles WHERE rolname='synveil') IS DISTINCT FROM '{ownership}' OR EXISTS(SELECT 1 FROM pg_roles WHERE rolname='synveil' AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR NOT rolcanlogin)) THEN RAISE EXCEPTION 'foreign role'; END IF; ELSE CREATE ROLE synveil LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD '{password}'; COMMENT ON ROLE synveil IS '{ownership}'; END IF; IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='{database_owner}') THEN IF (SELECT shobj_description(oid,'pg_authid') FROM pg_roles WHERE rolname='{database_owner}') IS DISTINCT FROM '{ownership}' OR EXISTS(SELECT 1 FROM pg_roles WHERE rolname='{database_owner}' AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR rolcanlogin)) THEN RAISE EXCEPTION 'foreign database owner'; END IF; ELSE CREATE ROLE \"{database_owner}\" NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION; COMMENT ON ROLE \"{database_owner}\" IS '{ownership}'; END IF; END $host$;\nCOMMIT;\nSELECT CASE WHEN EXISTS(SELECT 1 FROM pg_database WHERE datname='synveil') THEN 'existing' ELSE 'absent' END;\n")));
    let outcome = pg_command(catalog, "psql", &args, sql.as_bytes())?;
    if String::from_utf8_lossy(&outcome).lines().last() == Some("absent") {
        let create = format!(
            "CREATE DATABASE synveil OWNER \"{database_owner}\";\nCOMMENT ON DATABASE synveil IS '{ownership}';\n"
        );
        // CREATE DATABASE cannot be in a transaction. A lost response leaves
        // the bootstrap-owned database for inspection; never drop/recreate it.
        pg_command(catalog, "psql", &args, create.as_bytes())?;
    }
    let verify = format!(
        "SELECT CASE WHEN (shobj_description(oid,'pg_database')='{ownership}' OR shobj_description(oid,'pg_database') IS NULL) AND pg_get_userbyid(datdba)='{database_owner}' THEN 'owned' ELSE 'foreign' END FROM pg_database WHERE datname='synveil';\n"
    );
    if String::from_utf8_lossy(&pg_command(catalog, "psql", &args, verify.as_bytes())?).trim()
        != "owned"
    {
        return Err("database ownership ambiguous; preserving operator repair required");
    }
    pg_command(
        catalog,
        "psql",
        &args,
        format!("COMMENT ON DATABASE synveil IS '{ownership}';\n").as_bytes(),
    )?;
    let mut database_args = args;
    database_args[5] = "--dbname=synveil".to_owned();
    pg_command(catalog, "psql", &database_args, b"REVOKE CREATE ON SCHEMA public FROM PUBLIC; GRANT CONNECT ON DATABASE synveil TO synveil; GRANT USAGE, CREATE ON SCHEMA public TO synveil;\n")?;
    let config = owner.config()?.ok_or("configuration missing")?;
    if config.database.credential_state == DatabaseCredentialState::PendingManagedEndpoint {
        let (_, fingerprint) = owner
            .store
            .load_config_read_only()
            .map_err(|_| "configuration invalid")?
            .ok_or("configuration missing")?;
        owner
            .store
            .materialize_managed_database_url(
                fingerprint,
                DatabaseEndpoint {
                    host: "127.0.0.1".to_owned(),
                    port,
                },
            )
            .map_err(|_| "database credential materialization needs repair")?;
    }
    Ok(())
}
pub fn reconcile(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<()> {
    let old_selection = owner.journal()?.map(|journal| journal.selection);
    let config = owner.config()?.ok_or("server configuration missing")?;
    let (_, manifests) = owner.catalog(selection.profile, selection.mode)?;
    let artifact = |kind: &str| -> Result<synveil_server_service::RuntimeArtifactIdentity> {
        let manifest = manifests
            .get(kind)
            .ok_or("verified runtime input missing")?;
        Ok(synveil_server_service::RuntimeArtifactIdentity {
            artifact_id: catalog.artifacts[kind].clone(),
            version: manifest.version.clone(),
            source_revision: manifest.packaging_revision.clone(),
            platform: manifest.platform.clone(),
            architecture: manifest.architecture.clone(),
            sha256: catalog.artifacts[kind].clone(),
        })
    };
    let bytes = native::read_owned(Path::new("/etc/synveil/server-config.json"), 128 * 1024)?;
    let plan = synveil_server_service::prepare_plan(
        &config,
        &bytes,
        artifact("server")?,
        if selection.profile == DeploymentProfile::PersonalHomeManaged {
            Some(artifact("postgresql")?)
        } else {
            None
        },
    )
    .map_err(|_| "canonical service owner rejected the current config/storage/runtime plan")?;
    synveil_server_service::verify_plan_is_current(&plan, &config, &bytes)
        .map_err(|_| "canonical service plan is stale")?;
    preflight(owner, catalog, selection)?;
    handoff_storage(owner)?;
    if selection.profile == DeploymentProfile::PersonalHomeManaged {
        prepare_postgres(owner, catalog)?;
    }
    let new_units = units(catalog, selection)?;
    if let Some(old_selection) = old_selection {
        for name in units(catalog, &old_selection)?.keys() {
            if new_units.contains_key(name) {
                continue;
            }
            effective_unit(owner, name)?;
            if unit_active(name) {
                native::systemctl(&["stop", name])?;
                if unit_active(name) {
                    return Err("stale owned service failed to stop");
                }
            }
            native::systemctl(&["disable", name])?;
            owner.remove_owned_file(&Path::new("/etc/systemd/system").join(name))?;
        }
    }
    for (name, text) in &new_units {
        owner.publish(
            &Path::new("/etc/systemd/system").join(name),
            text.as_bytes(),
            0o644,
        )?;
    }
    if selection.mode == synveil_server_config::ReachabilityMode::AdvancedExternalHttps {
        owner.remove_owned_file(Path::new("/etc/synveil/network/Caddyfile"))?;
    }
    native::systemctl(&["daemon-reload"])?;
    if selection.profile == DeploymentProfile::PersonalHomeManaged {
        native::systemctl(&["start", "synveil-postgresql.service"])?;
        if !unit_active("synveil-postgresql.service") {
            return Err("managed PostgreSQL failed to start; inspect preserved cluster");
        }
        provision_database(owner, catalog)?;
    }
    // Run only the canonical migration gate. Advanced PostgreSQL remains
    // operator-owned; no PostgreSQL service/runtime is installed or stopped.
    native::systemctl(&["start", "synveil-server-migrate.service"])?;
    account::database_evidence(owner).map(|_| ())
}
pub fn unit_active(name: &str) -> bool {
    native::systemctl(&["is-active", "--quiet", name]).is_ok()
}
pub fn start_application() -> Result<()> {
    native::systemctl(&["restart", "synveil-api.service", "synveil-worker.service"])?;
    if !unit_active("synveil-api.service") || !unit_active("synveil-worker.service") {
        return Err("Synveil services failed to start");
    }
    Ok(())
}
pub fn enable(selection: &HostSelection) -> Result<()> {
    native::systemctl(&["enable", "synveil-server.target"])?;
    if selection.profile == DeploymentProfile::PersonalHomeManaged {
        native::systemctl(&["enable", "synveil-postgresql.service"])?;
    }
    Ok(())
}

pub fn stop_owned(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<()> {
    preflight(owner, catalog, selection)?;
    let mut names: BTreeSet<_> = units(catalog, selection)?.into_keys().collect();
    if let Some(journal) = owner.journal()? {
        names.extend(units(catalog, &journal.selection)?.into_keys());
    }
    for name in names {
        effective_unit(owner, &name)?;
        native::systemctl(&["stop", &name])?;
        if unit_active(&name) {
            return Err("owned service did not stop within its bounded drain");
        }
    }
    Ok(())
}
pub fn remove_owned(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<()> {
    let journal = owner.journal()?.ok_or("owned service intent missing")?;
    let mut inventory = units(catalog, selection)?;
    inventory.extend(units(catalog, &journal.selection)?);
    if let Some(pending) = &journal.pending_network_selection {
        inventory.extend(units(catalog, pending)?);
    }
    for name in inventory.keys() {
        if unit_active(name) {
            return Err("stop all owned services before removing software");
        }
    }
    network::remove_owned_firewall(owner)?;
    native::systemctl(&["disable", "synveil-server.target"])?;
    if selection.profile == DeploymentProfile::PersonalHomeManaged {
        native::systemctl(&["disable", "synveil-postgresql.service"])?;
    }
    for name in inventory.keys() {
        owner.remove_owned_file(&Path::new("/etc/systemd/system").join(name))?;
    }
    if selection.mode != synveil_server_config::ReachabilityMode::AdvancedExternalHttps {
        owner.remove_owned_file(Path::new("/etc/synveil/network/Caddyfile"))?;
    }
    let mut journal = owner.journal()?.ok_or("owned runtime intent missing")?;
    let mut owned_runtimes = Vec::new();
    for (kind, old) in &journal.retained_runtimes {
        owned_runtimes.extend(old.iter().map(|digest| (kind.clone(), digest.clone())));
    }
    owned_runtimes.extend(journal.runtime_intents.clone());
    for (kind, digest) in owned_runtimes {
        let cache = Path::new("/var/cache/synveil/host-runtime").join(&digest);
        let manifest = native::verify_runtime(&cache, &digest, &catalog.qualified_os)?;
        let root = runtime_root(&kind, &digest);
        if root.exists() {
            native::root_path(&root)?;
            // Exact member deletion only, with the manifest removed last.
            // Unknown files/directories stop removal; never recurse or purge.
            for file in &manifest.files {
                let path = root.join(&file.path);
                if path.exists() {
                    let bytes = native::read_owned(&path, file.bytes as usize)?;
                    if sha256(&bytes) != file.sha256 {
                        return Err("runtime was modified; preserving removal stopped");
                    }
                    fs::remove_file(&path)
                        .map_err(|_| "runtime member removal outcome uncertain")?;
                    fs::File::open(path.parent().ok_or("invalid runtime member")?)
                        .and_then(|file| file.sync_all())
                        .map_err(|_| "runtime removal sync failed")?;
                }
            }
            let manifest_path = root.join("manifest.json");
            if manifest_path.exists() {
                if sha256(&native::read_owned(&manifest_path, 2 * 1024 * 1024)?) != digest {
                    return Err("runtime manifest was modified");
                }
                fs::remove_file(manifest_path).map_err(|_| "runtime manifest removal failed")?;
            }
            let mut directories = std::collections::BTreeSet::new();
            for file in &manifest.files {
                let mut at = Path::new(&file.path).parent();
                while let Some(parent) = at {
                    if parent.as_os_str().is_empty() {
                        break;
                    }
                    directories.insert(root.join(parent));
                    at = parent.parent();
                }
            }
            let mut directories: Vec<_> = directories.into_iter().collect();
            directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
            for directory in directories {
                if directory.exists() {
                    native::root_path(&directory)?;
                    fs::remove_dir(directory).map_err(
                        |_| "unknown runtime entries preserved; operator repair required",
                    )?;
                }
            }
            fs::remove_dir(&root)
                .map_err(|_| "unknown runtime entries preserved; operator repair required")?;
            fs::File::open(root.parent().ok_or("invalid runtime namespace")?)
                .and_then(|file| file.sync_all())
                .map_err(|_| "runtime namespace sync failed")?;
        }
        if journal.runtime_intents.get(&kind) == Some(&digest) {
            journal.runtime_intents.remove(&kind);
        }
        if let Some(old) = journal.retained_runtimes.get_mut(&kind) {
            old.retain(|old| old != &digest);
        }
        owner.persist(&journal)?;
    }
    native::systemctl(&["daemon-reload"])?;
    Ok(())
}
fn effective_unit(owner: &NativeOwners, name: &str) -> Result<()> {
    let path = format!("/etc/systemd/system/{name}");
    let journal = owner.journal()?.ok_or("owned service intent missing")?;
    let unit_bytes = native::read_owned(Path::new(&path), 64 * 1024)?;
    if journal.owned_files.get(&path) != Some(&sha256(&unit_bytes)) {
        return Err("owned systemd unit modified");
    }
    let output = native::systemctl(&[
        "show",
        name,
        "--property=FragmentPath,DropInPaths,NeedDaemonReload,User,Group,ExecStart,MainPID,Result",
    ])?;
    let text = String::from_utf8(output).map_err(|_| "invalid systemd inspection")?;
    let declared = std::str::from_utf8(&unit_bytes).map_err(|_| "invalid owned systemd unit")?;
    if let Some(user) = declared.lines().find_map(|line| line.strip_prefix("User="))
        && !text.lines().any(|line| line == format!("User={user}"))
    {
        return Err("effective service user differs from the owned unit");
    }
    if let Some(group) = declared
        .lines()
        .find_map(|line| line.strip_prefix("Group="))
        && !text.lines().any(|line| line == format!("Group={group}"))
    {
        return Err("effective service group differs from the owned unit");
    }
    if let Some(program) = declared
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))
        .and_then(|line| line.split_whitespace().next())
    {
        if !text.lines().any(|line| {
            line.starts_with("ExecStart=") && line.contains(&format!("path={program} ;"))
        }) {
            return Err("effective executable differs from the owned unit");
        }
        let pid = text
            .lines()
            .find_map(|line| line.strip_prefix("MainPID="))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let expected_process = if name == "synveil-postgresql.service" {
            let runtime_id = journal
                .runtime_intents
                .get("postgresql")
                .ok_or("managed PostgreSQL runtime identity missing")?;
            runtime_root("postgresql", runtime_id).join("bin/postgres")
        } else {
            PathBuf::from(program)
        };
        if pid != 0
            && fs::read_link(format!("/proc/{pid}/exe")).ok().as_deref()
                != Some(expected_process.as_path())
        {
            return Err("running service executable differs from the owned runtime");
        }
    }
    if !text
        .lines()
        .any(|line| line == format!("FragmentPath={path}"))
        || text.lines().any(|line| line == "NeedDaemonReload=yes")
    {
        return Err("effective systemd unit differs from reviewed owned service");
    }
    let drops = text
        .lines()
        .find_map(|line| line.strip_prefix("DropInPaths="))
        .ok_or("systemd drop-in inspection missing")?;
    verify_drop_ins(drops, journal.selection.profile)?;
    Ok(())
}
pub fn observe(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    config: &ServerConfig,
    observation: &mut OwnerObservation,
) -> Result<()> {
    let journal = owner.journal()?.ok_or("Host intent missing")?;
    let selection = journal
        .pending_network_selection
        .as_ref()
        .unwrap_or(&journal.selection);
    observation.services_ready = units(catalog, selection)?
        .keys()
        .all(|name| effective_unit(owner, name).is_ok());
    if config.deployment_profile == DeploymentProfile::PersonalHomeManaged {
        observation.postgres_17_identity_matches = fs::symlink_metadata(MANAGED_DATA_DIRECTORY)
            .is_ok_and(|data| {
                journal.postgres_data_identity == Some((data.dev(), data.ino()))
                    && journal.postgres_system_id.is_some()
                    && system_id(catalog).ok() == journal.postgres_system_id
                    && verify_persisted_port(owner, catalog, &journal).is_ok()
            });
        let expected = format!(
            "listen_addresses = '127.0.0.1'\nport = {}\n",
            journal.postgres_port.unwrap_or(0)
        );
        observation.postgres_private = native::read_owned(
            Path::new("/etc/synveil/postgresql/postgresql.conf"),
            64 * 1024,
        )
        .is_ok_and(|bytes| bytes.starts_with(expected.as_bytes()))
            && effective_unit(owner, "synveil-postgresql.service").is_ok();
    }
    if let Ok((major, private, current)) = account::database_evidence(owner) {
        observation.migrations_current = current;
        if config.deployment_profile == DeploymentProfile::AdvancedExternal {
            observation.postgres_17_identity_matches = major == 17;
            observation.postgres_private = private;
        }
    }
    observation.api_live = unit_active("synveil-api.service") && network::backend_health("live");
    observation.api_ready = observation.api_live && network::backend_health("ready");
    observation.backend_loopback =
        effective_unit(owner, "synveil-api.service").is_ok() && network::backend_bound_privately();
    observation.worker_healthy =
        unit_active("synveil-worker.service") && worker_health(owner, config).is_ok();
    Ok(())
}

fn worker_health(_owner: &NativeOwners, config: &ServerConfig) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let (uid, _) = native::service_user("synveil")?;
    let path = Path::new("/run/synveil-worker/health.json");
    let meta = fs::symlink_metadata(path).map_err(|_| "worker health unavailable")?;
    if !meta.is_file()
        || meta.uid() != uid
        || meta.nlink() != 1
        || meta.mode() & 0o077 != 0
        || meta.len() > 4096
    {
        return Err("worker health identity mismatch");
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "worker health unavailable")?;
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "worker health unavailable")?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "invalid worker health")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "invalid clock")?
        .as_secs();
    let timestamp = value["observed_at"]
        .as_u64()
        .ok_or("worker health timestamp missing")?;
    let cycle_timestamp = value["cycle_observed_at"]
        .as_u64()
        .ok_or("worker cycle evidence missing")?;
    let window = value["health_window"]
        .as_u64()
        .filter(|window| (90..=86490).contains(window))
        .ok_or("invalid worker health window")?;
    let storage_id = match &config.storage {
        StorageConfiguration::ConfiguredLocal { storage_id, .. } => storage_id.as_str(),
        _ => return Err("worker storage identity missing"),
    };
    let pid = native::systemctl(&[
        "show",
        "synveil-worker.service",
        "--property=MainPID",
        "--value",
    ])?;
    if value["server_id"].as_str() != Some(&config.server_installation_id)
        || value["pid"].as_u64()
            != std::str::from_utf8(&pid)
                .ok()
                .and_then(|s| s.trim().parse().ok())
        || now < timestamp
        || now - timestamp > 90
        || value["database_ready"] != true
        || value["storage_ready"] != true
        || value["storage_id"].as_str() != Some(storage_id)
        || value["cycle_successful"] != true
        || cycle_timestamp == 0
        || now < cycle_timestamp
        || now - cycle_timestamp > window
    {
        return Err("worker is unhealthy or health evidence is stale");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_postgres_unit_waits_for_pg_ctl_readiness_before_dependents() {
        let digest = "a".repeat(64);
        let catalog = RuntimeCatalog {
            schema_version: 1,
            qualified_os: "ubuntu-24.04".to_owned(),
            artifacts: BTreeMap::from([
                ("server".to_owned(), "b".repeat(64)),
                ("postgresql".to_owned(), digest.clone()),
                ("caddy".to_owned(), "c".repeat(64)),
            ]),
        };
        let selection = HostSelection::default();
        let rendered = units(&catalog, &selection).expect("unit templates render");
        let postgres = &rendered["synveil-postgresql.service"];

        assert!(postgres.contains("Type=forking\n"));
        assert!(postgres.contains("PIDFile=/var/lib/synveil/postgresql/17/data/postmaster.pid"));
        assert!(postgres.contains(&format!(
            "ExecStart=/opt/synveil/postgresql/17/{digest}/bin/pg_ctl start"
        )));
        assert!(postgres.contains("-s -w -t 60"));
        assert!(
            postgres.contains("ExecStop=/opt/synveil/postgresql/17/")
                && postgres.contains(
                    "pg_ctl stop -D /var/lib/synveil/postgresql/17/data -s -m fast -w -t 60"
                )
        );
        assert!(!postgres.contains("Type=notify"));

        let migrate = &rendered["synveil-server-migrate.service"];
        assert!(migrate.contains("Requires=synveil-postgresql.service"));
        assert!(migrate.contains("After=synveil-postgresql.service"));
        let target = &rendered["synveil-server.target"];
        assert!(target.contains("Wants=synveil-postgresql.service"));
        assert!(target.contains("After=synveil-postgresql.service"));
    }

    #[test]
    fn private_lan_edge_waits_for_network_online_and_postgres_stays_private() {
        let catalog = RuntimeCatalog {
            schema_version: 1,
            qualified_os: "ubuntu-24.04".to_owned(),
            artifacts: BTreeMap::from([
                ("server".to_owned(), "b".repeat(64)),
                ("postgresql".to_owned(), "a".repeat(64)),
                ("caddy".to_owned(), "c".repeat(64)),
            ]),
        };
        let selection = HostSelection {
            profile: DeploymentProfile::PersonalHomeManaged,
            storage_root: "/var/lib/synveil/storage".to_owned(),
            mode: synveil_server_config::ReachabilityMode::PrivateLan,
            interface_id: Some("eth0".to_owned()),
            listener_address: "192.168.1.5".to_owned(),
            canonical_origin: "https://192.168.1.5/".to_owned(),
            external_database_target: None,
        };
        let rendered = units(&catalog, &selection).expect("LAN unit templates render");
        let edge = &rendered["synveil-edge.service"];
        assert!(edge.contains("Wants=network-online.target"));
        assert!(edge.contains("After=network-online.target"));
        let postgres = &rendered["synveil-postgresql.service"];
        assert!(postgres.contains("User=synveil-postgres"));
        assert!(postgres.contains("RestrictAddressFamilies=AF_UNIX AF_INET"));
        assert!(!postgres.contains("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6"));
        assert!(postgres.contains("ReadWritePaths=/var/lib/synveil/postgresql/17"));
    }
}
