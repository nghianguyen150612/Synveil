#![forbid(unsafe_code)]

//! Crash-safe, transport-neutral desktop inbound synchronization.
//!
//! The production HTTP adapter and immutable server profiles connect the
//! transport-neutral core. The observer added for Prompt 38 turns bounded
//! filesystem hints into durable local outbound *intents* only; it never
//! submits mutations, uploads content, resolves conflicts, or adds a UI.
//! It consumes bounded logical server facts through [`SyncRemote`],
//! applies them through [`LocalReplica`], and records every safety boundary in
//! a process-independent SQLite [`LocalStateStore`].

mod conflict_policy;
mod contracts;
mod engine;
mod error;
mod http_remote;
mod names;
mod observation;
mod outbound;
mod path;
mod profiles;
mod rebaseline;
#[cfg(test)]
mod rebaseline_84c;
mod rebaseline_convergence;
mod replica;
mod state;

#[cfg(test)]
mod test_support;

pub use conflict_policy::{
    ConflictCursor, ConflictPage, DEFAULT_CONFLICT_PAGE_LIMIT, MAX_CONFLICT_PAGE_LIMIT,
    SyncConflictKind, SyncConflictRecord, SyncConflictResolution, SyncConflictStatus,
};
pub use contracts::{
    BootstrapCompletion, BootstrapPage, ContentByteStream, EngineStatus, InboundChange,
    OpaqueEvidence, RebaselineHandoffConfirmation, RemoteCheckpoint, RemoteContent, RemoteError,
    RemoteErrorKind, RemoteFeedPage, RemoteMutationApplied, RemoteMutationConflict,
    RemoteMutationOutcome, ReplicaScope, SyncRemote, UploadCompletion, UploadSessionStatus,
    UploadTarget, boxed_content_stream,
};
pub use engine::{
    EngineConfig, FailureInjector, FailurePoint, InboundSyncEngine, NoopFailureInjector,
    SyncOutcome,
};
pub use error::{ClientSyncError, RecoveryClassification};
pub use http_remote::{
    ConnectionHealth, EnrollmentCredentials, HttpClientConfig, HttpEnrollmentClient, HttpSyncRemote,
};
pub use names::{NamePortability, local_collision_key, validate_logical_name};
pub use observation::{
    LocalChangeWatcher, ManualChangeSource, ManualChangeWatcher, NotifyLocalChangeWatcher,
    ObservationConfig, ObservationIssue, ObservationIssueKind, ObservationState, OutboundIntent,
    OutboundIntentKind, OutboundIntentState, OutboundObservationEngine, WatchHint, WatchHintKind,
};
pub use outbound::{OutboundSubmissionEngine, OutboundSubmissionOutcome};
pub use path::ManagedRelativePath;
pub use profiles::{
    CanonicalBaseUrl, DeviceEnrollmentRecord, LoadedDeviceCredential, ServerProfile,
    ServerProfileId,
};
pub use rebaseline::{
    RebaselineApplier, RebaselineApplyOutcome, RebaselineBoundary, RebaselineHandoffOutcome,
    RebaselineSnapshotDescriptor, RebaselineSnapshotPage, RebaselineSnapshotRemote,
    RebaselineSnapshotSource,
};
pub use rebaseline_convergence::{
    RebaselineConvergenceCoordinator, RebaselineConvergenceOutcome, RebaselineRecoveryBlockedReason,
};
pub use replica::{
    FilesystemLocalReplica, LocalFingerprint, LocalObjectKind, LocalReplica, RootBindingId,
};
pub use state::{
    BootstrapRecord, LocalApplyIssue, LocalIssueKind, LocalNode, LocalOperation,
    LocalOperationKind, LocalOperationState, LocalStateConfig, LocalStateStore, PendingAck,
    ReplicaRecord,
};

/// Current durable local schema version.
pub const LOCAL_SCHEMA_VERSION: i64 = 6;

/// Feed and snapshot pages are deliberately processed one at a time.
pub const MAX_PAGE_ITEMS: usize = 1_000;

/// Opaque server evidence is bounded before it reaches SQLite.
pub const MAX_OPAQUE_EVIDENCE_BYTES: usize = 4 * 1024;

/// Initial upper bound for one downloaded current file.
pub const MAX_DOWNLOAD_BYTES: u64 = 1 << 40;

/// Maximum byte-stream item accepted from a remote adapter. Production
/// adapters must split larger HTTP frames before yielding them to the core.
pub const MAX_CONTENT_CHUNK_BYTES: usize = 1024 * 1024;

/// Maximum upload chunk emitted by the desktop outbound submitter. The server
/// also enforces its authoritative route/application chunk limit.
pub const MAX_UPLOAD_CHUNK_BYTES: usize = 1024 * 1024;
