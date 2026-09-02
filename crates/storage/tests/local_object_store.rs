use std::{
    fs, io,
    path::{Path, PathBuf},
};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use sha2::{Digest, Sha256};
use synveil_object_store::{
    ByteRange, ByteStream, CapabilityEvidence, CapabilitySupport, DeleteOutcome,
    DeleteReconciliation, IntegrityExpectation, ObjectKey, ObjectStore, ObjectStoreError,
    StorageAvailability, StorageBackendKind, StorageCapability,
};
use synveil_storage::{LocalFilesystemObjectStore, boxed_stream};
use uuid::Uuid;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "synveil-local-object-store-test-{}",
            Uuid::now_v7()
        ));
        fs::create_dir_all(&path).expect("create test root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn body(bytes: &'static [u8]) -> synveil_object_store::ByteStream {
    boxed_stream(stream::iter(vec![Ok(Bytes::from_static(bytes))]))
}

async fn collect_body(mut body: ByteStream) -> Result<Vec<u8>, ObjectStoreError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

fn object_locator_for_test(root: &Path, key: &ObjectKey) -> PathBuf {
    let digest = Sha256::digest(key.as_str().as_bytes());
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    root.join("objects")
        .join("v1")
        .join(&encoded[..2])
        .join(encoded)
}

fn collect_paths(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        output.push(path.clone());
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            collect_paths(&path, output);
        }
    }
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
        .unwrap_or_else(|error| panic!("create directory-symlink fixture: {error}"));
    Ok(())
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
        .unwrap_or_else(|error| panic!("create file-symlink fixture: {error}"));
    Ok(())
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(not(any(unix, windows)))]
fn create_directory_symlink(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "directory symlinks are not available on this target",
    ))
}

#[cfg(not(any(unix, windows)))]
fn create_file_symlink(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file symlinks are not available on this target",
    ))
}

#[tokio::test]
async fn shared_object_store_conformance_passes_for_local_filesystem() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");

    synveil_storage::object_store::conformance::run_basic_conformance(&store).await;
}

#[test]
fn initialization_creates_managed_layout_without_cleaning_unknown_root_content() {
    let root = TempRoot::new();
    let unknown = root.path().join("operator-owned-note.txt");
    fs::write(&unknown, b"must survive initialization").expect("write unknown content");

    let _store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");

    assert!(unknown.is_file());
    assert_eq!(
        fs::read(&unknown).expect("read unknown content"),
        b"must survive initialization"
    );
    assert!(root.path().join(".synveil-storage-root").is_file());
    assert!(root.path().join("objects").is_dir());
    assert!(root.path().join("objects/v1").is_dir());
    assert!(root.path().join("staging").is_dir());
}

#[test]
fn initialization_rejects_relative_file_and_redirected_roots() {
    assert!(matches!(
        LocalFilesystemObjectStore::open(Path::new("relative-object-root")),
        Err(ObjectStoreError::InvalidRequest)
    ));

    let root = TempRoot::new();
    let file_root = root.path().join("not-a-directory");
    fs::write(&file_root, b"operator data").expect("write file-root fixture");
    assert!(matches!(
        LocalFilesystemObjectStore::open(&file_root),
        Err(ObjectStoreError::StorageUnavailable)
    ));
    assert_eq!(
        fs::read(&file_root).expect("read file-root fixture"),
        b"operator data"
    );

    let target = root.path().join("redirect-target");
    fs::create_dir(&target).expect("create redirect target");
    let redirected_root = root.path().join("redirected-root");
    if create_directory_symlink(&target, &redirected_root).is_err() {
        return;
    }
    assert!(matches!(
        LocalFilesystemObjectStore::open(&redirected_root),
        Err(ObjectStoreError::StorageUnavailable)
    ));
    assert!(target.is_dir());
}

#[test]
fn initialization_rejects_an_unrecognized_root_marker_without_cleanup() {
    let root = TempRoot::new();
    let marker = root.path().join(".synveil-storage-root");
    let unknown = root.path().join("operator-owned-note.txt");
    fs::write(&marker, b"not-a-synveil-layout\n").expect("write invalid marker");
    fs::write(&unknown, b"must survive rejection").expect("write unknown content");

    assert!(matches!(
        LocalFilesystemObjectStore::open(root.path()),
        Err(ObjectStoreError::StorageUnavailable)
    ));
    assert_eq!(
        fs::read(&unknown).expect("read unknown content"),
        b"must survive rejection"
    );
    assert!(!root.path().join("objects").exists());
    assert!(!root.path().join("staging").exists());
}

#[tokio::test]
async fn configured_root_is_retained_in_canonical_resolved_form() {
    let container = TempRoot::new();
    let lexical_parent = container.path().join("lexical-parent");
    fs::create_dir(&lexical_parent).expect("create lexical parent");
    let resolved_root = container.path().join("resolved-root");
    let requested_root = lexical_parent.join("..").join("resolved-root");
    let store = LocalFilesystemObjectStore::open(&requested_root).expect("open resolved root");

    fs::remove_dir(&lexical_parent).expect("remove no-longer-needed lexical parent");
    let handle = store
        .begin_staged_write()
        .await
        .expect("canonical root remains usable");
    store.abort_staged(&handle).await.expect("abort staging");
    assert!(resolved_root.join("objects/v1").is_dir());
    assert!(resolved_root.join("staging").is_dir());
}

#[tokio::test]
async fn stale_staging_is_not_wiped_by_reopen() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let handle = store.begin_staged_write().await.expect("begin staging");
    store
        .write_staged(
            &handle,
            body(b"stale but recoverable"),
            IntegrityExpectation::none(),
        )
        .await
        .expect("write staging");
    drop(store);

    let reopened = LocalFilesystemObjectStore::open(root.path()).expect("reopen local store");
    let names = fs::read_dir(root.path().join("staging"))
        .expect("read staging")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(names.iter().any(|name| name.contains(handle.as_str())));

    reopened
        .abort_staged(&handle)
        .await
        .expect("abort stale staging");
}

#[tokio::test]
async fn append_staging_enforces_exact_offsets_and_reconciles_after_reopen() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let handle = store.begin_staged_write().await.expect("begin staging");

    assert_eq!(
        store
            .append_staged(&handle, 0, Bytes::from_static(b"abc"), 6)
            .await
            .expect("append first chunk")
            .length(),
        3
    );
    assert_eq!(
        store
            .append_staged(&handle, 0, Bytes::from_static(b"abc"), 6)
            .await,
        Err(ObjectStoreError::PreconditionFailed)
    );
    drop(store);

    let reopened = LocalFilesystemObjectStore::open(root.path()).expect("reopen local store");
    assert_eq!(
        reopened
            .staging_progress(&handle)
            .await
            .expect("inspect durable partial progress")
            .length(),
        3
    );
    reopened
        .append_staged(&handle, 3, Bytes::from_static(b"def"), 6)
        .await
        .expect("append resumed chunk");

    let expected = Sha256::digest(b"abcdef");
    let mut expected_bytes = [0_u8; 32];
    expected_bytes.copy_from_slice(&expected);
    reopened
        .finalize_staged(
            &handle,
            IntegrityExpectation::none()
                .with_length(6)
                .with_sha256(synveil_core::Sha256Digest::from_bytes(expected_bytes)),
        )
        .await
        .expect("finalize resumed staging");
    let key = ObjectKey::new("objects/v1/resumed-upload").expect("valid key");
    reopened
        .promote_temp(&handle, &key)
        .await
        .expect("promote resumed upload");
    assert_eq!(
        collect_body(
            reopened
                .get(&key)
                .await
                .expect("read promoted upload")
                .into_stream(),
        )
        .await
        .expect("collect promoted upload"),
        b"abcdef"
    );
}

#[test]
fn reopen_rejects_a_redirected_managed_staging_directory() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    drop(store);

    let staging = root.path().join("staging");
    fs::remove_dir(&staging).expect("remove empty managed staging directory");
    let outside = TempRoot::new();
    if create_directory_symlink(outside.path(), &staging).is_err() {
        fs::create_dir(&staging).expect("restore staging after unavailable symlink fixture");
        return;
    }

    assert!(matches!(
        LocalFilesystemObjectStore::open(root.path()),
        Err(ObjectStoreError::StorageUnavailable)
    ));
    assert_eq!(
        fs::read_dir(outside.path())
            .expect("read redirect target")
            .count(),
        0
    );
}

#[tokio::test]
async fn staged_write_rejects_a_redirected_upload_file_without_touching_the_target() {
    let root = TempRoot::new();
    let outside = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let handle = store.begin_staged_write().await.expect("begin staging");
    let upload = root
        .path()
        .join("staging")
        .join(format!("{}.upload", handle.as_str()));
    fs::remove_file(&upload).expect("remove upload fixture");
    let outside_file = outside.path().join("operator-data");
    fs::write(&outside_file, b"must remain unchanged").expect("write outside fixture");
    if create_file_symlink(&outside_file, &upload).is_err() {
        return;
    }

    assert_eq!(
        store
            .write_staged(
                &handle,
                body(b"attacker bytes"),
                IntegrityExpectation::none()
            )
            .await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert_eq!(
        fs::read(&outside_file).expect("read outside fixture"),
        b"must remain unchanged"
    );
}

#[test]
fn capabilities_report_the_portable_profile_and_known_limits() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let capabilities = store.capabilities();

    assert_eq!(capabilities.backend(), StorageBackendKind::LocalFilesystem);
    assert_eq!(capabilities.availability(), StorageAvailability::Available);
    assert_eq!(
        capabilities.evidence(),
        CapabilityEvidence::AdapterProbe { version: 1 }
    );
    assert!(
        capabilities
            .iter()
            .all(|(_, support)| support != CapabilitySupport::Unknown)
    );
    assert!(capabilities.supports(StorageCapability::RangeReads));
    assert!(capabilities.supports(StorageCapability::ExclusiveCreate));
    assert!(capabilities.supports(StorageCapability::AtomicPromotion));
    assert!(capabilities.supports(StorageCapability::ReadAfterWrite));
    assert!(capabilities.supports(StorageCapability::Checksumming));
    assert!(capabilities.supports(StorageCapability::ConditionalDelete));
    for capability in [
        StorageCapability::Reflink,
        StorageCapability::BlockClone,
        StorageCapability::CopyOnWriteClone,
        StorageCapability::NativeSnapshot,
        StorageCapability::Compression,
        StorageCapability::SparseFiles,
        StorageCapability::FilesystemHealth,
    ] {
        assert_eq!(
            capabilities.support(capability),
            CapabilitySupport::Unsupported,
            "capability must remain explicitly unsupported: {capability:?}"
        );
    }
    if capabilities.supports(StorageCapability::DurableFlush) {
        assert!(capabilities.supports(StorageCapability::DurableFsync));
        assert!(capabilities.supports(StorageCapability::AtomicPromotion));
    }
}

#[tokio::test]
async fn physical_paths_are_hashed_and_invalid_key_forms_cannot_escape_root() {
    for invalid in [
        "/absolute/path",
        "../outside",
        "..\\outside",
        "C:/outside",
        "C:\\outside",
        "objects/../../outside",
        "objects//outside",
        "objects/user filename",
    ] {
        assert!(
            ObjectKey::new(invalid).is_err(),
            "key should be rejected: {invalid}"
        );
    }

    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/opaque-physical-key").expect("valid object key");
    store
        .put(
            synveil_object_store::PutRequest::new(key, body(b"content"))
                .with_integrity(IntegrityExpectation::none().with_length(7)),
        )
        .await
        .expect("put object");

    let mut paths = Vec::new();
    collect_paths(root.path(), &mut paths);
    assert!(paths.iter().all(|path| path.starts_with(root.path())));
    assert!(
        paths
            .iter()
            .all(|path| !path.to_string_lossy().contains("user filename"))
    );
    assert!(
        paths
            .iter()
            .all(|path| !path.to_string_lossy().contains("opaque-physical-key"))
    );
    assert!(root.path().join("objects/v1").is_dir());
}

#[tokio::test]
async fn an_interrupted_uncommitted_object_stays_invisible_and_is_quarantined() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/interrupted-promotion").expect("valid object key");
    let object_directory = object_locator_for_test(root.path(), &key);
    fs::create_dir_all(&object_directory).expect("create incomplete object directory");
    let partial_content = object_directory.join("content");
    fs::write(&partial_content, b"partial bytes").expect("write incomplete content");

    assert!(!store.exists(&key).await.expect("incomplete exists check"));
    assert!(matches!(
        store.get(&key).await,
        Err(ObjectStoreError::NotFound)
    ));
    assert_eq!(
        store
            .put(synveil_object_store::PutRequest::new(
                key,
                body(b"must not overwrite partial state"),
            ))
            .await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert_eq!(
        fs::read(&partial_content).expect("read incomplete content"),
        b"partial bytes"
    );
    assert!(!object_directory.join("committed").exists());
    assert_eq!(
        fs::read_dir(root.path().join("staging"))
            .expect("read staging")
            .count(),
        0
    );
}

#[tokio::test]
async fn a_committed_marker_with_missing_content_is_a_storage_failure_not_absence() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/missing-committed-content").expect("valid object key");
    store
        .put(synveil_object_store::PutRequest::new(
            key.clone(),
            body(b"committed bytes"),
        ))
        .await
        .expect("put object");
    let object_directory = object_locator_for_test(root.path(), &key);
    fs::remove_file(object_directory.join("content")).expect("remove committed content fixture");

    assert_eq!(
        store.exists(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert!(matches!(
        store.metadata(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    ));
    assert_eq!(
        store.delete(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert!(object_directory.join("committed").is_file());
    assert!(object_directory.join("metadata").is_file());
}

#[tokio::test]
async fn reads_detect_same_length_content_corruption() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/corruption-fixture").expect("valid object key");
    store
        .put(synveil_object_store::PutRequest::new(
            key.clone(),
            body(b"original"),
        ))
        .await
        .expect("put object");
    let object_directory = object_locator_for_test(root.path(), &key);
    fs::write(object_directory.join("content"), b"tampered").expect("corrupt object content");

    assert!(matches!(
        store.get(&key).await,
        Err(ObjectStoreError::IntegrityMismatch)
    ));
    assert!(matches!(
        store
            .range_read(&key, ByteRange::new(0, 4).expect("valid range"))
            .await,
        Err(ObjectStoreError::IntegrityMismatch)
    ));
}

#[tokio::test]
async fn delete_refuses_unknown_entries_and_preserves_the_committed_object() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/delete-unknown-entry").expect("valid object key");
    store
        .put(synveil_object_store::PutRequest::new(
            key.clone(),
            body(b"preserve me"),
        ))
        .await
        .expect("put object");
    let object_directory = object_locator_for_test(root.path(), &key);
    let unknown = object_directory.join("operator-note");
    fs::write(&unknown, b"unknown managed-directory content").expect("write unknown entry");

    assert_eq!(
        store.delete(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert!(unknown.is_file());
    assert_eq!(
        collect_body(
            store
                .get(&key)
                .await
                .expect("committed object remains readable")
                .into_stream(),
        )
        .await
        .expect("collect preserved object"),
        b"preserve me"
    );
}

#[tokio::test]
async fn interrupted_physical_delete_tombstone_is_reconciled_before_absence() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/interrupted-delete").expect("valid object key");
    let metadata = store
        .put(synveil_object_store::PutRequest::new(
            key.clone(),
            body(b"physical bytes must be removed"),
        ))
        .await
        .expect("put deletion fixture");
    let version = metadata
        .version()
        .expect("local committed object has a version")
        .clone();
    let object_directory = object_locator_for_test(root.path(), &key);
    let deleting_directory = object_directory.with_file_name(format!(
        "{}.deleting",
        object_directory
            .file_name()
            .expect("hashed object directory has a name")
            .to_string_lossy()
    ));

    // Simulate a process stopping after the atomic live->deleting rename and
    // after content removal, but before marker/metadata/tombstone cleanup.
    fs::rename(&object_directory, &deleting_directory).expect("create deletion tombstone");
    fs::remove_file(deleting_directory.join("content"))
        .expect("simulate already removed physical content");

    match store
        .reconcile_delete(&key)
        .await
        .expect("deletion tombstone must be inspectable")
    {
        DeleteReconciliation::InProgress(Some(evidence)) => assert_eq!(evidence, metadata),
        outcome => panic!("unexpected deletion reconciliation outcome: {outcome:?}"),
    }
    assert_eq!(
        store.exists(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert_eq!(
        store.conditional_delete(&key, &version).await,
        Ok(DeleteOutcome::Deleted)
    );
    assert_eq!(
        store.reconcile_delete(&key).await,
        Ok(DeleteReconciliation::Absent)
    );
    assert!(!object_directory.exists());
    assert!(!deleting_directory.exists());
}

#[tokio::test]
async fn a_redirected_committed_content_entry_is_rejected_without_following_it() {
    let root = TempRoot::new();
    let outside = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/redirected-content").expect("valid object key");
    store
        .put(synveil_object_store::PutRequest::new(
            key.clone(),
            body(b"stored content"),
        ))
        .await
        .expect("put object");
    let object_directory = object_locator_for_test(root.path(), &key);
    let content = object_directory.join("content");
    fs::remove_file(&content).expect("remove content fixture");
    let outside_file = outside.path().join("outside-content");
    fs::write(&outside_file, b"stored content").expect("write outside fixture");
    if create_file_symlink(&outside_file, &content).is_err() {
        return;
    }

    assert_eq!(
        store.exists(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    );
    assert!(matches!(
        store.get(&key).await,
        Err(ObjectStoreError::StorageUnavailable)
    ));
    assert_eq!(
        fs::read(&outside_file).expect("read outside fixture"),
        b"stored content"
    );
}

#[tokio::test]
async fn unexpected_symlink_object_entry_is_rejected_without_following_it() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/symlink-fixture").expect("valid object key");
    let object_directory = object_locator_for_test(root.path(), &key);
    fs::create_dir_all(object_directory.parent().expect("object directory parent"))
        .expect("create object parent");
    let outside = root.path().join("outside");
    fs::create_dir(&outside).expect("create outside directory");

    if create_directory_symlink(&outside, &object_directory).is_err() {
        return;
    }

    assert_eq!(
        store.exists(&key).await,
        Err(synveil_object_store::ObjectStoreError::StorageUnavailable)
    );
    assert!(outside.is_dir());
}

#[tokio::test]
async fn unexpected_symlink_parent_entry_is_rejected_without_following_it() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/symlink-parent-fixture").expect("valid object key");
    let object_directory = object_locator_for_test(root.path(), &key);
    let prefix = object_directory.parent().expect("object prefix");
    let outside = root.path().join("outside-parent");
    fs::create_dir(&outside).expect("create outside directory");

    if create_directory_symlink(&outside, prefix).is_err() {
        return;
    }

    assert_eq!(
        store.exists(&key).await,
        Err(synveil_object_store::ObjectStoreError::StorageUnavailable)
    );
    assert!(outside.is_dir());
}

#[tokio::test]
async fn promotion_failure_does_not_follow_symlink_destination_or_leave_staging() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let key = ObjectKey::new("objects/v1/symlink-promotion-fixture").expect("valid object key");
    let object_directory = object_locator_for_test(root.path(), &key);
    fs::create_dir_all(object_directory.parent().expect("object directory parent"))
        .expect("create object parent");
    let outside = root.path().join("outside-promotion");
    fs::create_dir(&outside).expect("create outside directory");

    if create_directory_symlink(&outside, &object_directory).is_err() {
        return;
    }

    assert_eq!(
        store
            .put(synveil_object_store::PutRequest::new(
                key,
                body(b"must not escape through promotion"),
            ))
            .await,
        Err(synveil_object_store::ObjectStoreError::StorageUnavailable)
    );
    assert!(outside.is_dir());
    assert_eq!(
        fs::read_dir(root.path().join("staging"))
            .expect("read staging")
            .count(),
        0
    );
}
