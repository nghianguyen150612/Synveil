use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::model::StorageRootClassification;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PathIdentity {
    pub device: u64,
    pub inode: u64,
    pub parent_device: u64,
    pub parent_inode: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CandidatePath {
    pub requested: PathBuf,
    pub canonical: PathBuf,
    pub parent: PathBuf,
    pub exists: bool,
    pub identity: PathIdentity,
    pub is_mount_point: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PathInspectionError {
    Invalid,
    Unavailable,
    WrongType,
    SymlinkOrRedirected,
    PermissionUnsafe,
    Unsupported,
}

impl PathInspectionError {
    pub const fn classification(self) -> StorageRootClassification {
        match self {
            Self::Invalid => StorageRootClassification::UnsupportedFilesystemState,
            Self::Unavailable => StorageRootClassification::Unavailable,
            Self::WrongType => StorageRootClassification::WrongType,
            Self::SymlinkOrRedirected => StorageRootClassification::SymlinkOrRedirected,
            Self::PermissionUnsafe => StorageRootClassification::PermissionUnsafe,
            Self::Unsupported => StorageRootClassification::UnsupportedFilesystemState,
        }
    }
}

pub(super) fn inspect_candidate(path: &Path) -> Result<CandidatePath, PathInspectionError> {
    validate_spelling(path)?;
    if !cfg!(target_os = "linux") {
        return Err(PathInspectionError::Unsupported);
    }

    let components = path.components().collect::<Vec<_>>();
    let mut current = PathBuf::from("/");
    let mut missing_leaf = false;
    for (index, component) in components.iter().enumerate().skip(1) {
        let Component::Normal(part) = component else {
            return Err(PathInspectionError::Invalid);
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(PathInspectionError::SymlinkOrRedirected);
                }
                if !metadata.is_dir() {
                    return Err(PathInspectionError::WrongType);
                }
                if unsafe_directory(&metadata) {
                    return Err(PathInspectionError::PermissionUnsafe);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if index + 1 != components.len() {
                    return Err(PathInspectionError::Unavailable);
                }
                missing_leaf = true;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(PathInspectionError::PermissionUnsafe);
            }
            Err(_) => return Err(PathInspectionError::Unavailable),
        }
    }

    let parent = path.parent().ok_or(PathInspectionError::Invalid)?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            PathInspectionError::PermissionUnsafe
        } else {
            PathInspectionError::Unavailable
        }
    })?;
    if parent_meta.file_type().is_symlink() {
        return Err(PathInspectionError::SymlinkOrRedirected);
    }
    if !parent_meta.is_dir() {
        return Err(PathInspectionError::WrongType);
    }
    if unsafe_directory(&parent_meta) {
        return Err(PathInspectionError::PermissionUnsafe);
    }
    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| PathInspectionError::Unavailable)?;

    let (canonical, exists, identity, is_mount_point) = if missing_leaf {
        let leaf = path.file_name().ok_or(PathInspectionError::Invalid)?;
        let canonical = canonical_parent.join(leaf);
        let identity = PathIdentity {
            device: 0,
            inode: 0,
            parent_device: metadata_device(&parent_meta),
            parent_inode: metadata_inode(&parent_meta),
        };
        (canonical, false, identity, false)
    } else {
        let root_meta = fs::symlink_metadata(path).map_err(|_| PathInspectionError::Unavailable)?;
        if root_meta.file_type().is_symlink() {
            return Err(PathInspectionError::SymlinkOrRedirected);
        }
        if !root_meta.is_dir() {
            return Err(PathInspectionError::WrongType);
        }
        if unsafe_directory(&root_meta) {
            return Err(PathInspectionError::PermissionUnsafe);
        }
        let canonical = fs::canonicalize(path).map_err(|_| PathInspectionError::Unavailable)?;
        let identity = PathIdentity {
            device: metadata_device(&root_meta),
            inode: metadata_inode(&root_meta),
            parent_device: metadata_device(&parent_meta),
            parent_inode: metadata_inode(&parent_meta),
        };
        let is_mount_point = is_linux_mount_point(&canonical);
        (canonical, true, identity, is_mount_point)
    };

    if canonical == Path::new("/") || path == Path::new("/") {
        return Err(PathInspectionError::Invalid);
    }
    Ok(CandidatePath {
        requested: path.to_path_buf(),
        canonical,
        parent: canonical_parent,
        exists,
        identity,
        is_mount_point,
    })
}

pub(super) fn validate_root_exclusions(
    candidate: &Path,
    additional_roots: &[PathBuf],
) -> Result<(), PathInspectionError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if home
        .as_deref()
        .and_then(canonical_if_exists)
        .is_some_and(|home| candidate == home)
    {
        return Err(PathInspectionError::Invalid);
    }

    let mut reserved = vec![
        PathBuf::from("/etc/synveil"),
        PathBuf::from("/etc/synveil/credentials"),
        PathBuf::from("/var/lib/synveil/postgresql"),
        PathBuf::from("/run"),
    ];
    if ["/tmp", "/var/tmp", "/dev/shm"]
        .iter()
        .any(|shared_root| candidate == Path::new(shared_root))
    {
        return Err(PathInspectionError::PermissionUnsafe);
    }
    if let Ok(current) = std::env::current_dir() {
        let current = canonical_if_exists(&current).unwrap_or(current);
        if candidate == current {
            return Err(PathInspectionError::Invalid);
        }
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf);
    if let Some(workspace) = workspace {
        reserved.push(workspace);
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        reserved.push(parent.to_path_buf());
    }
    reserved.extend(additional_roots.iter().cloned());

    for root in reserved {
        let root = canonical_if_exists(&root).unwrap_or(root);
        if paths_overlap(candidate, &root) {
            return Err(PathInspectionError::Invalid);
        }
    }
    Ok(())
}

pub(super) fn paths_overlap(first: &Path, second: &Path) -> bool {
    first.starts_with(second) || second.starts_with(first)
}

fn validate_spelling(path: &Path) -> Result<(), PathInspectionError> {
    if !path.is_absolute() || path.as_os_str().is_empty() || path.as_os_str().len() > 4096 {
        return Err(PathInspectionError::Invalid);
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(PathInspectionError::Invalid);
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if path.as_os_str().as_bytes().contains(&0)
            || path
                .as_os_str()
                .as_bytes()
                .split(|byte| *byte == b'/')
                .any(|component| component == b"." || component == b"..")
        {
            return Err(PathInspectionError::Invalid);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn unsafe_directory(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    let uid = unsafe { libc::geteuid() };
    let mode = metadata.mode();
    let protected_sticky_root_directory = mode & 0o1000 != 0 && metadata.uid() == 0;
    (metadata.uid() != 0 && metadata.uid() != uid)
        || ((mode & 0o022 != 0) && !protected_sticky_root_directory)
}

#[cfg(not(target_os = "linux"))]
fn unsafe_directory(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
pub(super) fn metadata_device(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.dev()
}

#[cfg(not(unix))]
pub(super) fn metadata_device(_metadata: &fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
pub(super) fn metadata_inode(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
pub(super) fn metadata_inode(_metadata: &fs::Metadata) -> u64 {
    0
}

pub(super) fn canonical_if_exists(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

#[cfg(target_os = "linux")]
fn is_linux_mount_point(path: &Path) -> bool {
    use std::{fs::File, io::Read};

    let Ok(mut file) = File::open("/proc/self/mountinfo") else {
        return false;
    };
    let mut contents = String::new();
    if file
        .by_ref()
        .take(1024 * 1024)
        .read_to_string(&mut contents)
        .is_err()
    {
        return false;
    }
    contents.lines().any(|line| {
        line.split_whitespace()
            .nth(4)
            .is_some_and(|mount| unescape_mount_path(mount) == path)
    })
}

#[cfg(not(target_os = "linux"))]
fn is_linux_mount_point(_path: &Path) -> bool {
    false
}

fn unescape_mount_path(value: &str) -> PathBuf {
    PathBuf::from(
        value
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_overlap_uses_components_not_textual_prefixes() {
        assert!(paths_overlap(
            Path::new("/data/Synveil"),
            Path::new("/data/Synveil")
        ));
        assert!(paths_overlap(
            Path::new("/data/Synveil"),
            Path::new("/data/Synveil/MyFiles")
        ));
        assert!(paths_overlap(
            Path::new("/data/Synveil/MyFiles"),
            Path::new("/data/Synveil")
        ));
        assert!(!paths_overlap(
            Path::new("/data/Synveil"),
            Path::new("/data/Synveil2")
        ));
        assert!(!paths_overlap(
            Path::new("/data/client"),
            Path::new("/data/server")
        ));
    }

    #[test]
    fn reserved_roots_and_recommended_root_use_component_boundaries() {
        assert!(validate_root_exclusions(Path::new("/var/lib/synveil/storage"), &[]).is_ok());
        for rejected in [
            "/etc/synveil",
            "/etc/synveil/credentials",
            "/var/lib/synveil/postgresql/17",
            "/run/synveil",
            "/tmp",
            "/var/tmp",
            "/dev/shm",
        ] {
            assert!(
                validate_root_exclusions(Path::new(rejected), &[]).is_err(),
                "reserved path should be rejected: {rejected}"
            );
        }
        assert!(
            validate_root_exclusions(
                Path::new("/data/Synveil2"),
                &[PathBuf::from("/data/Synveil")]
            )
            .is_ok()
        );
        assert!(
            validate_root_exclusions(
                Path::new("/data/Synveil/MyFiles"),
                &[PathBuf::from("/data/Synveil")]
            )
            .is_err()
        );
    }

    #[test]
    fn current_directory_home_and_workspace_roots_are_rejected() {
        if let Ok(current) = std::env::current_dir() {
            assert!(validate_root_exclusions(&current, &[]).is_err());
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            assert!(validate_root_exclusions(&home, &[]).is_err());
        }
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root");
        assert!(validate_root_exclusions(workspace, &[]).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn selected_path_inspection_rejects_root_relative_symlink_wrong_type_and_world_write() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        assert_eq!(
            inspect_candidate(Path::new("relative/storage")),
            Err(PathInspectionError::Invalid)
        );
        assert_eq!(
            inspect_candidate(Path::new("/")),
            Err(PathInspectionError::Invalid)
        );
        assert_eq!(
            inspect_candidate(&base.join("missing/leaf")),
            Err(PathInspectionError::Unavailable)
        );

        let file = base.join("ordinary-file");
        fs::write(&file, b"not a directory").unwrap();
        assert_eq!(
            inspect_candidate(&file),
            Err(PathInspectionError::WrongType)
        );

        let target = base.join("target");
        fs::create_dir(&target).unwrap();
        let redirect = base.join("redirect");
        symlink(&target, &redirect).unwrap();
        assert_eq!(
            inspect_candidate(&redirect),
            Err(PathInspectionError::SymlinkOrRedirected)
        );

        let broad = base.join("broad");
        fs::create_dir(&broad).unwrap();
        fs::set_permissions(&broad, fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            inspect_candidate(&broad),
            Err(PathInspectionError::PermissionUnsafe)
        );

        let missing = base.join("new-leaf");
        let inspected = inspect_candidate(&missing).unwrap();
        assert!(!inspected.exists);
        assert_eq!(inspected.canonical, missing);
        assert!(!missing.exists());
    }
}
