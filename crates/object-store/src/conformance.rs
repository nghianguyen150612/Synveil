//! Shared backend conformance assertions.
//!
//! This module is intentionally hidden from normal product documentation. It
//! is compiled as a small, backend-neutral test utility so every adapter can
//! exercise the same staged-write, immutable-read, and delete semantics.

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use sha2::{Digest, Sha256};
use synveil_core::Sha256Digest;

use crate::{
    ByteRange, ByteStream, DeleteOutcome, IntegrityExpectation, ObjectKey, ObjectStore,
    ObjectStoreError, ObjectVersion, PutRequest, boxed_stream,
};

fn digest(bytes: &[u8]) -> Sha256Digest {
    let computed = Sha256::digest(bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&computed);
    Sha256Digest::from_bytes(output)
}

fn chunks(parts: &[&[u8]]) -> ByteStream {
    let chunks = parts
        .iter()
        .map(|part| Ok(Bytes::copy_from_slice(part)))
        .collect::<Vec<_>>();
    boxed_stream(stream::iter(chunks))
}

async fn collect_body(mut body: ByteStream) -> Result<Vec<u8>, ObjectStoreError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

/// Run the common v1 object-store behavior suite against one backend.
///
/// The function panics on a failed assertion and is intended for integration
/// and unit tests. The caller should supply a fresh store so the fixed opaque
/// keys cannot collide with unrelated fixtures.
pub async fn run_basic_conformance<S>(store: &S)
where
    S: ObjectStore + ?Sized,
{
    let primary_key =
        ObjectKey::new("objects/v1/conformance-primary").expect("conformance key must be valid");
    let primary_content = b"hello object store";
    let primary_digest = digest(primary_content);

    let primary_metadata = store
        .put(
            PutRequest::new(
                primary_key.clone(),
                chunks(&[b"hello ", b"object ", b"store"]),
            )
            .with_integrity(
                IntegrityExpectation::none()
                    .with_length(primary_content.len() as u64)
                    .with_sha256(primary_digest),
            ),
        )
        .await
        .expect("streamed put must succeed");

    assert_eq!(primary_metadata.key(), &primary_key);
    assert_eq!(primary_metadata.length(), primary_content.len() as u64);
    assert_eq!(primary_metadata.sha256(), Some(&primary_digest));
    let primary_version = primary_metadata
        .version()
        .expect("local/object-store versions are required")
        .clone();
    let head = store
        .metadata(&primary_key)
        .await
        .expect("metadata must succeed");
    assert_eq!(head.key(), &primary_key);
    assert_eq!(head.length(), primary_content.len() as u64);
    assert_eq!(head.sha256(), Some(&primary_digest));
    assert_eq!(head.version(), Some(&primary_version));
    assert!(
        store
            .exists(&primary_key)
            .await
            .expect("exists must succeed")
    );

    assert_eq!(
        store
            .put(PutRequest::new(
                primary_key.clone(),
                chunks(&[b"replacement"]),
            ))
            .await,
        Err(ObjectStoreError::AlreadyExists)
    );

    let read = store
        .get(&primary_key)
        .await
        .expect("full read must succeed");
    assert_eq!(read.range(), None);
    assert_eq!(
        collect_body(read.into_stream()).await.expect("read body"),
        primary_content
    );

    let exact_range = ByteRange::new(6, 12).expect("range shape must be valid");
    let range = store
        .range_read(&primary_key, exact_range)
        .await
        .expect("range read must succeed");
    assert_eq!(range.range(), Some(exact_range));
    assert_eq!(
        collect_body(range.into_stream()).await.expect("range body"),
        b"object"
    );
    let final_byte = ByteRange::new(
        primary_content.len() as u64 - 1,
        primary_content.len() as u64,
    )
    .expect("final-byte range must be valid");
    assert_eq!(
        collect_body(
            store
                .range_read(&primary_key, final_byte)
                .await
                .expect("final-byte range read must succeed")
                .into_stream(),
        )
        .await
        .expect("final-byte range body"),
        b"e"
    );
    assert!(matches!(
        store
            .range_read(
                &primary_key,
                ByteRange::new(0, primary_content.len() as u64 + 1)
                    .expect("range shape must be valid"),
            )
            .await,
        Err(ObjectStoreError::InvalidRange)
    ));

    let staging_one = store
        .begin_staged_write()
        .await
        .expect("first staging handle");
    let staging_two = store
        .begin_staged_write()
        .await
        .expect("second staging handle");
    assert_ne!(staging_one, staging_two);

    let unwritten_staging = store
        .begin_staged_write()
        .await
        .expect("unwritten staging handle");
    let unwritten_key =
        ObjectKey::new("objects/v1/conformance-unwritten").expect("conformance key must be valid");
    assert_eq!(
        store.promote_temp(&unwritten_staging, &unwritten_key).await,
        Err(ObjectStoreError::StagingConflict)
    );
    assert!(!store.exists(&unwritten_key).await.expect("exists check"));
    store
        .abort_staged(&unwritten_staging)
        .await
        .expect("abort unwritten staging");

    let rejected_staging = store
        .begin_staged_write()
        .await
        .expect("integrity-rejected staging handle");
    assert_eq!(
        store
            .write_staged(
                &rejected_staging,
                chunks(&[b"integrity mismatch"]),
                IntegrityExpectation::none().with_length(1),
            )
            .await,
        Err(ObjectStoreError::IntegrityMismatch)
    );
    store
        .abort_staged(&rejected_staging)
        .await
        .expect("abort integrity-rejected staging");

    let mut large_content = vec![0_u8; 256 * 1024 + 17];
    for (index, byte) in large_content.iter_mut().enumerate() {
        *byte = (index % 251) as u8;
    }
    let large_digest = digest(&large_content);
    let split_at = large_content.len() / 3;
    let split_at_two = split_at * 2;
    let promoted_key =
        ObjectKey::new("objects/v1/conformance-promoted").expect("conformance key must be valid");
    let staged_metadata = store
        .write_staged(
            &staging_one,
            chunks(&[
                &large_content[..split_at],
                &large_content[split_at..split_at_two],
                &large_content[split_at_two..],
            ]),
            IntegrityExpectation::none()
                .with_length(large_content.len() as u64)
                .with_sha256(large_digest),
        )
        .await
        .expect("large staged write must succeed");
    assert_eq!(staged_metadata.length(), large_content.len() as u64);
    assert_eq!(staged_metadata.sha256(), Some(&large_digest));
    assert!(!store.exists(&promoted_key).await.expect("exists check"));
    assert_eq!(
        store.metadata(&promoted_key).await,
        Err(ObjectStoreError::NotFound)
    );
    let receipt = store
        .promote_temp(&staging_one, &promoted_key)
        .await
        .expect("staged promotion must succeed");
    assert_eq!(receipt.metadata().key(), &promoted_key);
    assert_eq!(receipt.metadata().length(), large_content.len() as u64);
    assert_eq!(receipt.metadata().sha256(), Some(&large_digest));
    let promoted_read = store.get(&promoted_key).await.expect("promoted read");
    assert_eq!(
        collect_body(promoted_read.into_stream())
            .await
            .expect("promoted body"),
        large_content
    );

    let second_promoted_key = ObjectKey::new("objects/v1/conformance-promoted-two")
        .expect("conformance key must be valid");
    store
        .write_staged(
            &staging_two,
            chunks(&[b"independent staging bytes"]),
            IntegrityExpectation::none(),
        )
        .await
        .expect("second staged write must succeed");
    store
        .promote_temp(&staging_two, &second_promoted_key)
        .await
        .expect("second staged promotion must succeed");
    assert_eq!(
        collect_body(
            store
                .get(&second_promoted_key)
                .await
                .expect("second promoted read")
                .into_stream(),
        )
        .await
        .expect("second promoted body"),
        b"independent staging bytes"
    );
    assert_eq!(
        store
            .delete(&second_promoted_key)
            .await
            .expect("delete committed object"),
        DeleteOutcome::Deleted
    );
    assert_eq!(
        store
            .delete(&second_promoted_key)
            .await
            .expect("repeat committed-object delete"),
        DeleteOutcome::AlreadyAbsent
    );

    let conflict_staging = store
        .begin_staged_write()
        .await
        .expect("conflict staging handle");
    store
        .write_staged(
            &conflict_staging,
            chunks(&[b"must not replace the committed object"]),
            IntegrityExpectation::none(),
        )
        .await
        .expect("conflict staged write");
    assert_eq!(
        store.promote_temp(&conflict_staging, &primary_key).await,
        Err(ObjectStoreError::AlreadyExists)
    );
    assert_eq!(
        collect_body(
            store
                .get(&primary_key)
                .await
                .expect("existing object must remain readable")
                .into_stream(),
        )
        .await
        .expect("existing object body"),
        primary_content
    );
    store
        .abort_staged(&conflict_staging)
        .await
        .expect("abort must succeed");
    store
        .abort_staged(&conflict_staging)
        .await
        .expect("abort must be idempotent");

    let zero_key =
        ObjectKey::new("objects/v1/conformance-zero").expect("conformance key must be valid");
    let zero_metadata = store
        .put(
            PutRequest::new(zero_key.clone(), chunks(&[])).with_integrity(
                IntegrityExpectation::none()
                    .with_length(0)
                    .with_sha256(digest(&[])),
            ),
        )
        .await
        .expect("zero-byte put must succeed");
    assert_eq!(zero_metadata.length(), 0);
    assert_eq!(
        collect_body(
            store
                .get(&zero_key)
                .await
                .expect("zero-byte read")
                .into_stream(),
        )
        .await
        .expect("zero-byte body"),
        Vec::<u8>::new()
    );

    let wrong_version = ObjectVersion::new("wrong-version").expect("opaque version");
    assert_eq!(
        store.conditional_delete(&primary_key, &wrong_version).await,
        Err(ObjectStoreError::PreconditionFailed)
    );
    assert_eq!(
        store
            .conditional_delete(&primary_key, &primary_version)
            .await
            .expect("conditional delete"),
        DeleteOutcome::Deleted
    );
    assert_eq!(
        store.delete(&primary_key).await.expect("idempotent delete"),
        DeleteOutcome::AlreadyAbsent
    );
    assert!(
        !store
            .exists(&primary_key)
            .await
            .expect("post-delete exists")
    );
}
