//! Durable, append-only execution journal for the common installer engine.

use crate::*;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const JOURNAL_SCHEMA_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_BYTES: u64 = 1024 * 1024;
pub const MAX_CHECKPOINT_RECORDS: usize = 16_384;
pub const MAX_PLAN_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalMode {
    StartNew,
    ResumeExisting,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JournalVerifiedResult {
    VerifiedSuccess,
    VerifiedNoop,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "record",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum JournalRecord {
    TransactionOpened {
        plan_id: String,
        intent: InstallationIntent,
        target_scope: TargetScope,
    },
    StageEntered {
        stage: Stage,
    },
    EffectMutationStarted {
        effect_id: String,
    },
    EffectOutcome {
        effect_id: String,
        outcome: EffectResultState,
    },
    EffectVerified {
        effect_id: String,
        result: JournalVerifiedResult,
    },
    CompensationStarted {
        effect_id: String,
    },
    CompensationVerified {
        effect_id: String,
    },
    FinalVerificationSucceeded,
    TransactionCompleted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JournalEnvelope {
    pub schema_version: u32,
    pub generation: u64,
    pub plan_fingerprint_sha256: String,
    pub previous_record_sha256: Option<String>,
    pub record: JournalRecord,
    pub record_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDisposition {
    Fresh,
    ResumeReady,
    ReconciledApplied,
    ReplanRequired,
    InspectionRequired,
    AlreadyCompleted,
    StillUnknown,
    JournalBusy,
    JournalCorrupt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalErrorCode {
    JournalMissing,
    JournalBusy,
    JournalCorrupt,
    JournalUnsupportedSchema,
    JournalPlanMismatch,
    JournalIoFailed,
    JournalDiskFull,
    JournalLimitExceeded,
    ActiveTransactionExists,
    RecoveryInspectionRequired,
}

#[derive(Debug)]
pub struct JournalError {
    pub code: JournalErrorCode,
    pub inspection_required: bool,
}

impl JournalError {
    fn new(code: JournalErrorCode) -> Self {
        Self {
            code,
            inspection_required: matches!(
                code,
                JournalErrorCode::JournalMissing
                    | JournalErrorCode::JournalCorrupt
                    | JournalErrorCode::JournalUnsupportedSchema
                    | JournalErrorCode::JournalLimitExceeded
                    | JournalErrorCode::RecoveryInspectionRequired
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalFaultPoint {
    Write,
    AfterWrite,
    FileSync,
    AfterFileSync,
    Commit,
    AfterCommit,
    CommittedObjectSync,
    AfterCommittedObjectSync,
    DirectorySync,
    AfterDirectorySync,
    DiskFull,
}

#[derive(Clone, Debug, Default)]
pub struct JournalOptions {
    pub fail_at: Option<(JournalRecordClass, JournalFaultPoint)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalRecordClass {
    TransactionStart,
    StageEntry,
    EffectMutationStart,
    EffectResult,
    EffectVerification,
    Reconciliation,
    CompensationStart,
    CompensationCompletion,
    FinalVerification,
    TransactionCompletion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalExecutionResult {
    pub disposition: RecoveryDisposition,
    pub completed: bool,
    pub error: Option<JournalErrorCode>,
}

pub fn plan_fingerprint(plan: &InstallationPlan) -> Result<String, JournalError> {
    let bytes = serde_json::to_vec(plan)
        .map_err(|_| JournalError::new(JournalErrorCode::JournalCorrupt))?;
    if bytes.len() > MAX_PLAN_BYTES {
        return Err(JournalError::new(JournalErrorCode::JournalLimitExceeded));
    }
    Ok(hex_hash(&bytes))
}

pub struct InstallationJournal {
    directory: PathBuf,
    lock: File,
    fingerprint: String,
    records: Vec<JournalEnvelope>,
    options: JournalOptions,
    plan: InstallationPlan,
}

impl std::fmt::Debug for InstallationJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallationJournal")
            .field("directory", &self.directory)
            .field("fingerprint", &self.fingerprint)
            .field("records", &self.records)
            .finish_non_exhaustive()
    }
}

impl InstallationJournal {
    pub fn open(
        root: &Path,
        plan: &InstallationPlan,
        mode: JournalMode,
    ) -> Result<Self, JournalError> {
        Self::open_with_options(root, plan, mode, JournalOptions::default())
    }

    pub fn open_with_options(
        root: &Path,
        plan: &InstallationPlan,
        mode: JournalMode,
        options: JournalOptions,
    ) -> Result<Self, JournalError> {
        validate_id(&plan.plan_id)?;
        if root.as_os_str().is_empty() {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        reject_symlink(root)?;
        match mode {
            JournalMode::StartNew => create_directory_durable(root)?,
            JournalMode::ResumeExisting if !root.exists() => {
                return Err(JournalError::new(JournalErrorCode::JournalMissing));
            }
            _ => {}
        }
        reject_symlink(root)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::metadata(root).map_err(io_error)?;
            if metadata.uid() != nix::unistd::getuid().as_raw() || metadata.mode() & 0o022 != 0 {
                return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
            }
        }
        let directory = root.join(&plan.plan_id);
        reject_symlink(&directory)?;
        if !directory.exists() && mode == JournalMode::ResumeExisting {
            return Err(JournalError::new(JournalErrorCode::JournalMissing));
        }
        if !directory.exists() {
            fs::create_dir(&directory).map_err(io_error)?;
            sync_directory(root)?;
        }
        let directory_metadata = fs::symlink_metadata(&directory).map_err(io_error)?;
        if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        let lock_path = directory.join("lock");
        reject_symlink(&lock_path)?;
        let mut lock_options = OpenOptions::new();
        lock_options
            .read(true)
            .write(true)
            .create(true)
            .truncate(false);
        secure_open(&mut lock_options);
        let lock = lock_options.open(&lock_path).map_err(io_error)?;
        if !lock.metadata().map_err(io_error)?.is_file() {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        lock.try_lock_exclusive()
            .map_err(|_| JournalError::new(JournalErrorCode::JournalBusy))?;
        let fingerprint = plan_fingerprint(plan)?;
        let records = load_records(&directory, &fingerprint, plan)?;
        if mode == JournalMode::StartNew && !records.is_empty() {
            return Err(JournalError::new(JournalErrorCode::ActiveTransactionExists));
        }
        if mode == JournalMode::ResumeExisting && records.is_empty() {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        Ok(Self {
            directory,
            lock,
            fingerprint,
            records,
            options,
            plan: plan.clone(),
        })
    }

    pub fn records(&self) -> &[JournalEnvelope] {
        &self.records
    }
    pub fn transaction_directory(&self) -> &Path {
        &self.directory
    }

    pub fn append(&mut self, record: JournalRecord) -> Result<(), JournalError> {
        reject_symlink(&self.directory)?;
        if self.records.len() >= MAX_CHECKPOINT_RECORDS {
            return Err(JournalError::new(JournalErrorCode::JournalLimitExceeded));
        }
        // Hash integrity is not semantic validity.  Refuse an impossible transition
        // before it can become durable, while recovery independently performs the
        // same validation for journals written by older or damaged implementations.
        let generation = self.records.len() as u64;
        let mut candidate = self.records.clone();
        let previous_record_sha256 = self.records.last().map(|r| r.record_sha256.clone());
        let class = record_class(&record);
        let hash_input = HashInput {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation,
            plan_fingerprint_sha256: &self.fingerprint,
            previous_record_sha256: previous_record_sha256.as_deref(),
            record: &record,
        };
        let record_sha256 = hex_hash(
            &serde_json::to_vec(&hash_input)
                .map_err(|_| JournalError::new(JournalErrorCode::JournalCorrupt))?,
        );
        let envelope = JournalEnvelope {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation,
            plan_fingerprint_sha256: self.fingerprint.clone(),
            previous_record_sha256,
            record,
            record_sha256,
        };
        candidate.push(envelope.clone());
        replay(&self.plan, &candidate)?;
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|_| JournalError::new(JournalErrorCode::JournalCorrupt))?;
        if bytes.len() as u64 > MAX_CHECKPOINT_BYTES {
            return Err(JournalError::new(JournalErrorCode::JournalLimitExceeded));
        }
        self.maybe_fail(class, JournalFaultPoint::Write)?;
        let temp = self
            .directory
            .join(format!("checkpoint-{generation:016}.tmp"));
        let final_path = self
            .directory
            .join(format!("checkpoint-{generation:016}.json"));
        reject_symlink(&temp)?;
        reject_symlink(&final_path)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        secure_open(&mut options);
        let mut file = options.open(&temp).map_err(io_error)?;
        self.maybe_fail(class, JournalFaultPoint::DiskFull)?;
        file.write_all(&bytes)
            .and_then(|_| file.flush())
            .map_err(io_error)?;
        self.maybe_fail(class, JournalFaultPoint::AfterWrite)?;
        self.maybe_fail(class, JournalFaultPoint::FileSync)?;
        file.sync_all().map_err(io_error)?;
        self.maybe_fail(class, JournalFaultPoint::AfterFileSync)?;
        self.maybe_fail(class, JournalFaultPoint::Commit)?;
        fs::hard_link(&temp, &final_path).map_err(io_error)?;
        fs::remove_file(&temp).map_err(io_error)?;
        self.maybe_fail(class, JournalFaultPoint::AfterCommit)?;
        self.maybe_fail(class, JournalFaultPoint::CommittedObjectSync)?;
        File::open(&final_path)
            .and_then(|f| f.sync_all())
            .map_err(io_error)?;
        self.maybe_fail(class, JournalFaultPoint::AfterCommittedObjectSync)?;
        self.maybe_fail(class, JournalFaultPoint::DirectorySync)?;
        sync_directory(&self.directory)?;
        self.maybe_fail(class, JournalFaultPoint::AfterDirectorySync)?;
        self.records.push(envelope);
        Ok(())
    }

    fn maybe_fail(
        &self,
        class: JournalRecordClass,
        point: JournalFaultPoint,
    ) -> Result<(), JournalError> {
        if self.options.fail_at == Some((class, point)) {
            let code = if point == JournalFaultPoint::DiskFull {
                JournalErrorCode::JournalDiskFull
            } else {
                JournalErrorCode::JournalIoFailed
            };
            Err(JournalError::new(code))
        } else {
            Ok(())
        }
    }
}

impl Drop for InstallationJournal {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}

#[derive(Serialize)]
struct HashInput<'a> {
    schema_version: u32,
    generation: u64,
    plan_fingerprint_sha256: &'a str,
    previous_record_sha256: Option<&'a str>,
    record: &'a JournalRecord,
}

fn load_records(
    directory: &Path,
    fingerprint: &str,
    plan: &InstallationPlan,
) -> Result<Vec<JournalEnvelope>, JournalError> {
    let mut paths = Vec::new();
    for item in fs::read_dir(directory).map_err(io_error)? {
        let path = item.map_err(io_error)?.path();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| JournalError::new(JournalErrorCode::JournalCorrupt))?;
        if name == "lock" {
            continue;
        }
        if name.starts_with("checkpoint-") && name.ends_with(".tmp") {
            reject_symlink(&path)?;
            if !item_type_is_regular(&path)? {
                return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
            }
            fs::remove_file(&path).map_err(io_error)?;
            continue;
        }
        if !name.starts_with("checkpoint-") || !name.ends_with(".json") {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        reject_symlink(&path)?;
        let digits = &name[11..name.len() - 5];
        let generation = digits
            .parse::<u64>()
            .map_err(|_| JournalError::new(JournalErrorCode::JournalCorrupt))?;
        paths.push((generation, path));
    }
    paths.sort_by_key(|p| p.0);
    if paths.len() > MAX_CHECKPOINT_RECORDS {
        return Err(JournalError::new(JournalErrorCode::JournalLimitExceeded));
    }
    let mut records: Vec<JournalEnvelope> = Vec::new();
    for (expected, (generation, path)) in paths.into_iter().enumerate() {
        if generation != expected as u64 {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.is_file() {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        if metadata.len() > MAX_CHECKPOINT_BYTES {
            return Err(JournalError::new(JournalErrorCode::JournalLimitExceeded));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .map_err(io_error)?
            .take(MAX_CHECKPOINT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        let envelope: JournalEnvelope = serde_json::from_slice(&bytes)
            .map_err(|_| JournalError::new(JournalErrorCode::JournalCorrupt))?;
        if envelope.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(JournalError::new(
                JournalErrorCode::JournalUnsupportedSchema,
            ));
        }
        if envelope.generation != generation {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        if envelope.plan_fingerprint_sha256 != fingerprint {
            return Err(JournalError::new(JournalErrorCode::JournalPlanMismatch));
        }
        let previous = records.last().map(|r| r.record_sha256.as_str());
        if envelope.previous_record_sha256.as_deref() != previous {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        let input = HashInput {
            schema_version: envelope.schema_version,
            generation,
            plan_fingerprint_sha256: &envelope.plan_fingerprint_sha256,
            previous_record_sha256: envelope.previous_record_sha256.as_deref(),
            record: &envelope.record,
        };
        let expected_hash = hex_hash(
            &serde_json::to_vec(&input)
                .map_err(|_| JournalError::new(JournalErrorCode::JournalCorrupt))?,
        );
        if envelope.record_sha256 != expected_hash {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        records.push(envelope);
    }
    if !records.is_empty() {
        replay(plan, &records)?;
    }
    Ok(records)
}

fn item_type_is_regular(path: &Path) -> Result<bool, JournalError> {
    Ok(fs::symlink_metadata(path)
        .map_err(io_error)?
        .file_type()
        .is_file())
}

fn validate_id(id: &str) -> Result<(), JournalError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
    {
        return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), JournalError> {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(target_os = "linux")]
                if meta.is_dir() {
                    use std::os::unix::fs::MetadataExt;
                    let trusted_owner =
                        meta.uid() == 0 || meta.uid() == nix::unistd::getuid().as_raw();
                    let protected_temporary_root = meta.uid() == 0 && meta.mode() & 0o1000 != 0;
                    if !trusted_owner || (meta.mode() & 0o022 != 0 && !protected_temporary_root) {
                        return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                    }
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                    }
                }
                let link = meta.file_type().is_symlink();
                #[cfg(target_os = "macos")]
                let link = link && !trusted_macos_root_alias(ancestor, &meta);
                if link || (ancestor != path && !meta.is_dir() && !meta.file_type().is_symlink()) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(e)),
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn trusted_macos_root_alias(path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // Preserve the OS-owned /var and /tmp aliases used by native temporary
    // directories. User-controlled links remain rejected; this is no native
    // qualification claim and does not relax Linux/Windows ancestry checks.
    let target = match path.to_str() {
        Some("/var") => Path::new("/private/var"),
        Some("/tmp") => Path::new("/private/tmp"),
        _ => return false,
    };
    metadata.uid() == 0
        && fs::canonicalize(path).is_ok_and(|actual| actual == target)
        && fs::metadata("/").is_ok_and(|root| root.uid() == 0 && root.mode() & 0o022 == 0)
}

fn secure_open(options: &mut OpenOptions) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    let _ = options;
}

fn sync_directory(path: &Path) -> Result<(), JournalError> {
    reject_symlink(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

        // Windows requires BACKUP_SEMANTICS to open a directory handle. Keep
        // OPEN_REPARSE_POINT and verify the handle itself so a path replacement
        // cannot make the durability flush follow a junction or symbolic link.
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        let directory = options.open(path).map_err(io_error)?;
        let metadata = directory.metadata().map_err(io_error)?;
        use std::os::windows::fs::MetadataExt;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        directory.sync_all().map_err(io_error)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(io_error)
    }
}
fn create_directory_durable(path: &Path) -> Result<(), JournalError> {
    let parent = path
        .parent()
        .filter(|candidate| !candidate.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                return sync_directory(parent);
            }
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    create_directory_durable(parent)?;
    match fs::create_dir(path) {
        Ok(()) => sync_directory(parent),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(io_error)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(JournalError::new(JournalErrorCode::JournalCorrupt))
            }
        }
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(all(test, windows))]
mod windows_directory_sync_tests {
    use super::create_directory_durable;

    #[test]
    fn directory_flush_uses_a_real_windows_directory_handle() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("journal-root");
        create_directory_durable(&root).expect("create and durably flush journal directory");
        create_directory_durable(&root.join("nested"))
            .expect("create and durably flush nested journal directory");
    }
}

fn io_error(error: std::io::Error) -> JournalError {
    let raw_code = error.raw_os_error();
    let platform_disk_full = {
        #[cfg(target_os = "linux")]
        {
            matches!(raw_code, Some(28 | 122)) // ENOSPC / EDQUOT
        }
        #[cfg(target_os = "macos")]
        {
            matches!(raw_code, Some(28 | 69)) // ENOSPC / EDQUOT
        }
        #[cfg(windows)]
        {
            matches!(raw_code, Some(39 | 112 | 1816)) // disk full / quota exceeded
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            false
        }
    };
    let disk_full = error.kind() == std::io::ErrorKind::StorageFull || platform_disk_full;
    JournalError::new(if disk_full {
        JournalErrorCode::JournalDiskFull
    } else {
        JournalErrorCode::JournalIoFailed
    })
}

#[cfg(all(test, target_os = "macos"))]
mod macos_path_tests {
    use super::trusted_macos_root_alias;
    use std::{fs, os::unix::fs::MetadataExt, path::Path};

    #[test]
    fn accepts_only_canonical_os_owned_temporary_root_aliases() {
        for (alias, canonical) in [
            (Path::new("/var"), Path::new("/private/var")),
            (Path::new("/tmp"), Path::new("/private/tmp")),
        ] {
            let Ok(metadata) = fs::symlink_metadata(alias) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                assert_eq!(metadata.uid(), 0);
                assert_eq!(fs::canonicalize(alias).unwrap(), canonical);
                assert!(trusted_macos_root_alias(alias, &metadata));
            }
        }
    }
}

fn hex_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn record_class(record: &JournalRecord) -> JournalRecordClass {
    match record {
        JournalRecord::TransactionOpened { .. } => JournalRecordClass::TransactionStart,
        JournalRecord::StageEntered { .. } => JournalRecordClass::StageEntry,
        JournalRecord::EffectMutationStarted { .. } => JournalRecordClass::EffectMutationStart,
        JournalRecord::EffectOutcome { .. } => JournalRecordClass::EffectResult,
        JournalRecord::EffectVerified { .. } => JournalRecordClass::EffectVerification,
        JournalRecord::CompensationStarted { .. } => JournalRecordClass::CompensationStart,
        JournalRecord::CompensationVerified { .. } => JournalRecordClass::CompensationCompletion,
        JournalRecord::FinalVerificationSucceeded => JournalRecordClass::FinalVerification,
        JournalRecord::TransactionCompleted => JournalRecordClass::TransactionCompletion,
    }
}

impl InstallerEngine {
    pub fn execute_journaled<A: InstallationAdapter>(
        &self,
        request: &InstallationRequest,
        plan: &InstallationPlan,
        adapter: &mut A,
        root: &Path,
        mode: JournalMode,
    ) -> JournalExecutionResult {
        self.execute_journaled_with_options(
            request,
            plan,
            adapter,
            root,
            mode,
            JournalOptions::default(),
        )
    }

    pub fn execute_journaled_with_options<A: InstallationAdapter>(
        &self,
        request: &InstallationRequest,
        plan: &InstallationPlan,
        adapter: &mut A,
        root: &Path,
        mode: JournalMode,
        options: JournalOptions,
    ) -> JournalExecutionResult {
        if self.validate(request, plan).is_err() {
            return journal_stop(JournalErrorCode::JournalPlanMismatch);
        }
        let mut journal = match InstallationJournal::open_with_options(root, plan, mode, options) {
            Ok(j) => j,
            Err(e) => return journal_stop(e.code),
        };
        if mode == JournalMode::StartNew
            && let Err(e) = journal.append(JournalRecord::TransactionOpened {
                plan_id: plan.plan_id.clone(),
                intent: plan.intent,
                target_scope: plan.target_scope,
            })
        {
            return journal_stop(e.code);
        }
        let state = match replay(plan, journal.records()) {
            Ok(s) => s,
            Err(e) => return journal_stop(e.code),
        };
        if state.completed {
            return JournalExecutionResult {
                disposition: RecoveryDisposition::AlreadyCompleted,
                completed: true,
                error: None,
            };
        }
        let mut disposition = if mode == JournalMode::StartNew {
            RecoveryDisposition::Fresh
        } else {
            RecoveryDisposition::ResumeReady
        };
        let mut applied: Vec<&InstallationEffect> = Vec::new();
        for effect in &plan.ordered_effects {
            match state.effects.get(&effect.effect_id).copied() {
                Some(EffectState::Verified) => {
                    if !adapter.verify_effect(effect) {
                        return JournalExecutionResult {
                            disposition: RecoveryDisposition::ReplanRequired,
                            completed: false,
                            error: Some(JournalErrorCode::RecoveryInspectionRequired),
                        };
                    }
                    applied.push(effect);
                    continue;
                }
                Some(EffectState::MutationStarted) => match adapter.reconcile_unknown(effect) {
                    ReconciliationOutcome::VerifiedApplied => {
                        if !adapter.verify_effect(effect) {
                            return inspection();
                        }
                        if let Err(e) = journal.append(JournalRecord::EffectVerified {
                            effect_id: effect.effect_id.clone(),
                            result: JournalVerifiedResult::VerifiedSuccess,
                        }) {
                            return journal_recovery_stop(e.code);
                        }
                        disposition = RecoveryDisposition::ReconciledApplied;
                        applied.push(effect);
                        continue;
                    }
                    ReconciliationOutcome::VerifiedNotApplied => {
                        return JournalExecutionResult {
                            disposition: RecoveryDisposition::ReplanRequired,
                            completed: false,
                            error: None,
                        };
                    }
                    ReconciliationOutcome::StillUnknown => {
                        return JournalExecutionResult {
                            disposition: RecoveryDisposition::StillUnknown,
                            completed: false,
                            error: Some(JournalErrorCode::RecoveryInspectionRequired),
                        };
                    }
                },
                Some(EffectState::Partial | EffectState::CompensationStarted) => {
                    return inspection();
                }
                Some(EffectState::Compensated) => {
                    return JournalExecutionResult {
                        disposition: RecoveryDisposition::ReplanRequired,
                        completed: false,
                        error: None,
                    };
                }
                Some(EffectState::FailureBeforeMutation) => {
                    return JournalExecutionResult {
                        disposition: RecoveryDisposition::ReplanRequired,
                        completed: false,
                        error: None,
                    };
                }
                None => {}
            }
            if !adapter.privilege_available(effect.privilege)
                || !adapter.inspect_preconditions(effect)
            {
                return journal_fail_with_compensation(
                    &mut journal,
                    &applied,
                    adapter,
                    RecoveryDisposition::ReplanRequired,
                );
            }
            if let Err(e) = journal.append(JournalRecord::EffectMutationStarted {
                effect_id: effect.effect_id.clone(),
            }) {
                return journal_stop(e.code);
            }
            let outcome = adapter.apply_effect(effect);
            match outcome {
                ApplyOutcome::Success | ApplyOutcome::Noop => {
                    if !adapter.verify_effect(effect) {
                        if let Err(e) = journal.append(JournalRecord::EffectOutcome {
                            effect_id: effect.effect_id.clone(),
                            outcome: EffectResultState::KnownPartialMutation,
                        }) {
                            return journal_recovery_stop(e.code);
                        }
                        return journal_fail_with_compensation(
                            &mut journal,
                            &applied,
                            adapter,
                            RecoveryDisposition::InspectionRequired,
                        );
                    }
                    let result = if outcome == ApplyOutcome::Noop {
                        JournalVerifiedResult::VerifiedNoop
                    } else {
                        JournalVerifiedResult::VerifiedSuccess
                    };
                    if let Err(e) = journal.append(JournalRecord::EffectVerified {
                        effect_id: effect.effect_id.clone(),
                        result,
                    }) {
                        return journal_recovery_stop(e.code);
                    }
                    applied.push(effect);
                }
                ApplyOutcome::FailureBeforeMutation => {
                    if let Err(e) = journal.append(JournalRecord::EffectOutcome {
                        effect_id: effect.effect_id.clone(),
                        outcome: EffectResultState::FailureBeforeMutation,
                    }) {
                        return journal_recovery_stop(e.code);
                    }
                    return journal_fail_with_compensation(
                        &mut journal,
                        &applied,
                        adapter,
                        RecoveryDisposition::ReplanRequired,
                    );
                }
                ApplyOutcome::KnownPartialMutation => {
                    if let Err(e) = journal.append(JournalRecord::EffectOutcome {
                        effect_id: effect.effect_id.clone(),
                        outcome: EffectResultState::KnownPartialMutation,
                    }) {
                        return journal_recovery_stop(e.code);
                    }
                    return journal_fail_with_compensation(
                        &mut journal,
                        &applied,
                        adapter,
                        RecoveryDisposition::InspectionRequired,
                    );
                }
                ApplyOutcome::OutcomeUnknown => match adapter.reconcile_unknown(effect) {
                    ReconciliationOutcome::VerifiedApplied if adapter.verify_effect(effect) => {
                        if let Err(e) = journal.append(JournalRecord::EffectVerified {
                            effect_id: effect.effect_id.clone(),
                            result: JournalVerifiedResult::VerifiedSuccess,
                        }) {
                            return journal_recovery_stop(e.code);
                        }
                        disposition = RecoveryDisposition::ReconciledApplied;
                        applied.push(effect);
                    }
                    ReconciliationOutcome::VerifiedNotApplied => {
                        return JournalExecutionResult {
                            disposition: RecoveryDisposition::ReplanRequired,
                            completed: false,
                            error: None,
                        };
                    }
                    _ => {
                        return journal_fail_with_compensation(
                            &mut journal,
                            &applied,
                            adapter,
                            RecoveryDisposition::InspectionRequired,
                        );
                    }
                },
            }
        }
        if !adapter.verify_installation(plan) {
            return inspection();
        }
        if !state.final_verified
            && let Err(e) = journal.append(JournalRecord::FinalVerificationSucceeded)
        {
            return journal_recovery_stop(e.code);
        }
        if let Err(e) = journal.append(JournalRecord::TransactionCompleted) {
            return journal_recovery_stop(e.code);
        }
        JournalExecutionResult {
            disposition,
            completed: true,
            error: None,
        }
    }
}

#[derive(Clone, Copy)]
enum EffectState {
    MutationStarted,
    Verified,
    FailureBeforeMutation,
    Partial,
    CompensationStarted,
    Compensated,
}
struct ReplayState {
    effects: BTreeMap<String, EffectState>,
    final_verified: bool,
    completed: bool,
}
fn replay(
    plan: &InstallationPlan,
    records: &[JournalEnvelope],
) -> Result<ReplayState, JournalError> {
    let valid: BTreeSet<&str> = plan
        .ordered_effects
        .iter()
        .map(|e| e.effect_id.as_str())
        .collect();
    let mut effects = BTreeMap::new();
    let order: BTreeMap<&str, usize> = plan
        .ordered_effects
        .iter()
        .enumerate()
        .map(|(index, effect)| (effect.effect_id.as_str(), index))
        .collect();
    let mut opened = false;
    let mut final_verified = false;
    let mut completed = false;
    for envelope in records {
        if completed {
            return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
        }
        match &envelope.record {
            JournalRecord::TransactionOpened {
                plan_id,
                intent,
                target_scope,
            } if plan_id == &plan.plan_id
                && intent == &plan.intent
                && target_scope == &plan.target_scope
                && envelope.generation == 0
                && !opened =>
            {
                opened = true
            }
            JournalRecord::TransactionOpened { .. } => {
                return Err(JournalError::new(if !opened && envelope.generation == 0 {
                    JournalErrorCode::JournalPlanMismatch
                } else {
                    JournalErrorCode::JournalCorrupt
                }));
            }
            JournalRecord::EffectMutationStarted { effect_id } => {
                require_opened(opened)?;
                ensure_effect(&valid, effect_id)?;
                if effects.contains_key(effect_id) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                let index = order[effect_id.as_str()];
                if plan.ordered_effects[..index].iter().any(|prior| {
                    !matches!(effects.get(&prior.effect_id), Some(EffectState::Verified))
                }) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                effects.insert(effect_id.clone(), EffectState::MutationStarted);
            }
            JournalRecord::EffectVerified { effect_id, .. } => {
                require_opened(opened)?;
                ensure_effect(&valid, effect_id)?;
                if !matches!(effects.get(effect_id), Some(EffectState::MutationStarted)) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                effects.insert(effect_id.clone(), EffectState::Verified);
            }
            JournalRecord::EffectOutcome { effect_id, outcome } => {
                require_opened(opened)?;
                ensure_effect(&valid, effect_id)?;
                if !matches!(effects.get(effect_id), Some(EffectState::MutationStarted)) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                effects.insert(
                    effect_id.clone(),
                    match outcome {
                        EffectResultState::FailureBeforeMutation => {
                            EffectState::FailureBeforeMutation
                        }
                        EffectResultState::KnownPartialMutation
                        | EffectResultState::OutcomeUnknown => EffectState::Partial,
                        _ => return Err(JournalError::new(JournalErrorCode::JournalCorrupt)),
                    },
                );
            }
            JournalRecord::CompensationStarted { effect_id } => {
                require_opened(opened)?;
                ensure_effect(&valid, effect_id)?;
                if !matches!(effects.get(effect_id), Some(EffectState::Verified)) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                effects.insert(effect_id.clone(), EffectState::CompensationStarted);
            }
            JournalRecord::CompensationVerified { effect_id } => {
                require_opened(opened)?;
                ensure_effect(&valid, effect_id)?;
                if !matches!(
                    effects.get(effect_id),
                    Some(EffectState::CompensationStarted)
                ) {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                effects.insert(effect_id.clone(), EffectState::Compensated);
            }
            JournalRecord::FinalVerificationSucceeded => {
                require_opened(opened)?;
                if final_verified
                    || plan.ordered_effects.iter().any(|effect| {
                        !matches!(effects.get(&effect.effect_id), Some(EffectState::Verified))
                    })
                {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                final_verified = true;
            }
            JournalRecord::TransactionCompleted => {
                require_opened(opened)?;
                if !final_verified {
                    return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
                }
                completed = true;
            }
            JournalRecord::StageEntered { .. } => require_opened(opened)?,
        }
    }
    if !opened {
        return Err(JournalError::new(JournalErrorCode::JournalCorrupt));
    }
    Ok(ReplayState {
        effects,
        final_verified,
        completed,
    })
}

/// Mirrors the Prompt007 reverse-order compensation policy while adding the
/// durable mutation boundaries required by the journaled execution path.
fn journal_fail_with_compensation<A: InstallationAdapter>(
    journal: &mut InstallationJournal,
    applied: &[&InstallationEffect],
    adapter: &mut A,
    disposition: RecoveryDisposition,
) -> JournalExecutionResult {
    for effect in crate::engine::compensation_candidates(applied, adapter) {
        if let Err(error) = journal.append(JournalRecord::CompensationStarted {
            effect_id: effect.effect_id.clone(),
        }) {
            return journal_recovery_stop(error.code);
        }
        if !adapter.compensate_effect(effect) || !adapter.verify_compensation(effect) {
            return inspection();
        }
        if let Err(error) = journal.append(JournalRecord::CompensationVerified {
            effect_id: effect.effect_id.clone(),
        }) {
            return journal_recovery_stop(error.code);
        }
    }
    JournalExecutionResult {
        disposition,
        completed: false,
        error: None,
    }
}
fn require_opened(opened: bool) -> Result<(), JournalError> {
    if opened {
        Ok(())
    } else {
        Err(JournalError::new(JournalErrorCode::JournalCorrupt))
    }
}
fn ensure_effect(valid: &BTreeSet<&str>, id: &str) -> Result<(), JournalError> {
    if valid.contains(id) {
        Ok(())
    } else {
        Err(JournalError::new(JournalErrorCode::JournalCorrupt))
    }
}
fn journal_stop(code: JournalErrorCode) -> JournalExecutionResult {
    JournalExecutionResult {
        disposition: match code {
            JournalErrorCode::JournalBusy => RecoveryDisposition::JournalBusy,
            JournalErrorCode::JournalDiskFull => RecoveryDisposition::ReplanRequired,
            JournalErrorCode::JournalCorrupt
            | JournalErrorCode::JournalUnsupportedSchema
            | JournalErrorCode::JournalLimitExceeded => RecoveryDisposition::JournalCorrupt,
            _ => RecoveryDisposition::InspectionRequired,
        },
        completed: false,
        error: Some(code),
    }
}
fn journal_recovery_stop(code: JournalErrorCode) -> JournalExecutionResult {
    JournalExecutionResult {
        disposition: RecoveryDisposition::InspectionRequired,
        completed: false,
        error: Some(code),
    }
}
fn inspection() -> JournalExecutionResult {
    JournalExecutionResult {
        disposition: RecoveryDisposition::InspectionRequired,
        completed: false,
        error: Some(JournalErrorCode::RecoveryInspectionRequired),
    }
}
