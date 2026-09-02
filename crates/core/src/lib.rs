#![forbid(unsafe_code)]

//! Platform-neutral foundation boundary for Synveil.
//!
//! This crate contains portable domain primitives only. It must remain free of
//! HTTP, database, operating-system, and storage-backend dependencies.

mod config;
mod domain;
mod errors;
mod hashes;
mod ids;
mod numbers;
mod time;
mod tokens;

pub use config::{CacheDir, ConfigDir, DataDir, RuntimeDir};
pub use domain::{
    Device, DeviceStatus, DomainError, FileVersion, Library, LibraryStatus, LogicalName,
    LoginIdentifier, MAX_LOGICAL_NAME_BYTES, Node, NodeKind, NodeState, ObjectReference, User,
    UserStatus,
};
pub use errors::{CoreError, ErrorCode, UnknownErrorCode};
pub use hashes::{Hash, HashParseError, Sha256Digest};
pub use ids::{
    BackupSetId, ChangeEventId, DedupDomainId, DeviceId, FileVersionId, IdParseError, LibraryId,
    NodeId, ObjectId, ObjectReplicaId, ShareId, SnapshotId, UploadSessionId, UserId,
};
pub use numbers::{DecimalValueError, Revision, Sequence};
pub use time::{Timestamp, TimestampParseError};
pub use tokens::{ETag, Etag, OpaqueCursor, TokenError, VersionToken};
