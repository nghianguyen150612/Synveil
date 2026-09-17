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
mod host;
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
mod runtime;
mod signals;
mod state;
mod sync_cycle;

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
#[cfg(feature = "test-support")]
pub use host::DesktopRootRecoveryGate;
pub use host::{
    DEFAULT_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL, DEFAULT_DESKTOP_SYNC_ROOT_PROBE_INTERVAL,
    DESKTOP_SYNC_HOST_READINESS, DesktopLifecycleAdapter, DesktopLifecycleEvent,
    DesktopNetworkAdapter, DesktopRootAvailability, DesktopSyncHost, DesktopSyncHostConfig,
    DesktopSyncHostConfigError, DesktopSyncHostError, DesktopSyncHostHandle,
    DesktopSyncHostLifecycle, DesktopSyncLibraryConfig, DesktopSyncLibraryRegistration,
    DesktopSyncLibrarySource, DesktopSyncRemote, LinuxLifecycleAdapter, LinuxNetworkAdapter,
    RootAvailability, WindowsLifecycleAdapter, WindowsNetworkAdapter,
};
pub use http_remote::{
    ConnectionHealth, EnrollmentCredentials, HttpClientConfig, HttpEnrollmentClient, HttpSyncRemote,
};
pub use names::{NamePortability, local_collision_key, validate_logical_name};
pub use observation::{
    LocalChangeWatcher, ManualChangeSource, ManualChangeWatcher, NotifyLocalChangeWatcher,
    ObservationConfig, ObservationIssue, ObservationIssueKind, ObservationPollResult,
    ObservationReconciliationResult, ObservationState, OutboundIntent, OutboundIntentKind,
    OutboundIntentState, OutboundObservationEngine, WatchHint, WatchHintKind,
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
pub use runtime::{
    DEFAULT_SYNC_RUNTIME_MAX_CONCURRENT_LIBRARIES, DEFAULT_SYNC_RUNTIME_POLL_INTERVAL,
    DEFAULT_SYNC_RUNTIME_RATE_LIMIT_FALLBACK, DEFAULT_SYNC_RUNTIME_TRANSIENT_BACKOFF_INITIAL,
    DEFAULT_SYNC_RUNTIME_TRANSIENT_BACKOFF_MAX, MAX_SYNC_RUNTIME_CONCURRENT_LIBRARIES,
    MAX_SYNC_RUNTIME_DELAY, MAX_SYNC_RUNTIME_LIBRARIES, MIN_SYNC_RUNTIME_POLL_INTERVAL,
    SYNC_RUNTIME_EVENT_CAPACITY, SyncCycleExecutor, SyncRuntime, SyncRuntimeConfig,
    SyncRuntimeConfigError, SyncRuntimeError, SyncRuntimeEvent, SyncRuntimeHandle,
    SyncRuntimeIdentity, SyncRuntimeLibraryPhase, SyncRuntimeLibraryStatus, SyncRuntimeOutcome,
    SyncRuntimeRegistration, SyncRuntimeUnregistration, SyncRuntimeWakeReason,
    SyncRuntimeWakeResult, SyncWakeNotifier,
};
pub use signals::{
    CredentialLifecycleController, CredentialLifecycleResult, DurableChangeNotification,
    DurableChangeResult, DurableOutboundIntentResult, OutboundIntentProducer,
};
pub use state::{
    BootstrapRecord, LocalApplyIssue, LocalIssueKind, LocalNode, LocalOperation,
    LocalOperationKind, LocalOperationState, LocalStateConfig, LocalStateStore,
    OutboundIntentUpsertResult, PendingAck, ReplicaRecord,
};
pub use sync_cycle::{
    BidirectionalSyncCycleRunner, InboundCycleOutcome, OutboundCycleOutcome, OutboundSkipReason,
    SyncCycleCoordinator, SyncCycleResult,
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
