use std::fmt;

use crate::{LocalIssueKind, ObservationIssueKind, RemoteError};

/// Deterministic startup classification for an interrupted local operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryClassification {
    NotStarted,
    FilesystemCompletedDatabasePending,
    DatabaseCompletedAckPending,
    SafeToRetry,
    Ambiguous,
}

/// Stable failures surfaced by the inbound engine.
///
/// Variants intentionally carry no absolute paths, logical names, content,
/// credentials, or opaque server evidence.
#[derive(Debug)]
pub enum ClientSyncError {
    Database,
    LocalIo,
    InvalidState,
    InvalidRelativePath,
    InvalidRoot,
    RootUnavailable,
    WrongRootBinding,
    InvalidServerUrl,
    InvalidServerProfile,
    WrongServerProfile,
    SecureStoreUnavailable,
    AuthenticationRequired,
    CredentialReplacementRequired,
    RootRedirected,
    ConcurrentWriter,
    InvalidRemoteResponse,
    RebaselineInProgress,
    RebaselinePendingHandoff,
    HandoffSnapshotUnavailable,
    HandoffCheckpointConflict,
    HandoffResponseMismatch,
    HandoffTransport,
    CandidateCorrupt,
    CandidateIncomplete,
    WrongScope,
    WrongEpoch,
    SequenceGap,
    SequenceRegression,
    UnsupportedSchemaVersion,
    ResourceLimit,
    ContentIntegrityMismatch,
    ContentUnstable,
    ConflictNotFound,
    ConflictStale,
    ConflictAlreadyResolved,
    ResolutionNotApplicable,
    InjectedFailure,
    LocalIssue(LocalIssueKind),
    ObservationIssue(ObservationIssueKind),
    Remote(RemoteError),
}

impl ClientSyncError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Database => "LOCAL_DATABASE_UNAVAILABLE",
            Self::LocalIo => "LOCAL_IO_UNAVAILABLE",
            Self::InvalidState => "LOCAL_STATE_INVALID",
            Self::InvalidRelativePath => "LOCAL_PATH_INVALID",
            Self::InvalidRoot => "LOCAL_ROOT_INVALID",
            Self::RootUnavailable => "LOCAL_ROOT_UNAVAILABLE",
            Self::WrongRootBinding => "LOCAL_ROOT_BINDING_MISMATCH",
            Self::InvalidServerUrl => "SERVER_PROFILE_URL_INVALID",
            Self::InvalidServerProfile => "SERVER_PROFILE_INVALID",
            Self::WrongServerProfile => "SERVER_PROFILE_BINDING_MISMATCH",
            Self::SecureStoreUnavailable => "SECURE_CREDENTIAL_STORE_UNAVAILABLE",
            Self::AuthenticationRequired => "AUTH_REQUIRED",
            Self::CredentialReplacementRequired => "EXPLICIT_CREDENTIAL_REPLACEMENT_REQUIRED",
            Self::RootRedirected => "LOCAL_ROOT_REDIRECTED",
            Self::ConcurrentWriter => "LOCAL_REPLICA_ALREADY_OPEN",
            Self::InvalidRemoteResponse => "REMOTE_RESPONSE_INVALID",
            Self::RebaselineInProgress => "REBASELINE_IN_PROGRESS",
            Self::RebaselinePendingHandoff => "REBASELINE_PENDING_HANDOFF",
            Self::HandoffSnapshotUnavailable => "REBASELINE_HANDOFF_SNAPSHOT_UNAVAILABLE",
            Self::HandoffCheckpointConflict => "REBASELINE_HANDOFF_CHECKPOINT_CONFLICT",
            Self::HandoffResponseMismatch => "REBASELINE_HANDOFF_RESPONSE_MISMATCH",
            Self::HandoffTransport => "REBASELINE_HANDOFF_TRANSPORT_ERROR",
            Self::CandidateCorrupt => "REBASELINE_CANDIDATE_CORRUPT",
            Self::CandidateIncomplete => "REBASELINE_CANDIDATE_INCOMPLETE",
            Self::WrongScope => "REMOTE_SCOPE_MISMATCH",
            Self::WrongEpoch => "REMOTE_EPOCH_MISMATCH",
            Self::SequenceGap => "REMOTE_SEQUENCE_GAP",
            Self::SequenceRegression => "REMOTE_SEQUENCE_REGRESSION",
            Self::UnsupportedSchemaVersion => "REMOTE_SCHEMA_UNSUPPORTED",
            Self::ResourceLimit => "CLIENT_RESOURCE_LIMIT",
            Self::ContentIntegrityMismatch => "CONTENT_INTEGRITY_MISMATCH",
            Self::ContentUnstable => "LOCAL_CONTENT_UNSTABLE",
            Self::ConflictNotFound => "SYNC_CONFLICT_NOT_FOUND",
            Self::ConflictStale => "SYNC_CONFLICT_STALE",
            Self::ConflictAlreadyResolved => "SYNC_CONFLICT_ALREADY_RESOLVED",
            Self::ResolutionNotApplicable => "SYNC_CONFLICT_RESOLUTION_NOT_APPLICABLE",
            Self::InjectedFailure => "INJECTED_FAILURE",
            Self::LocalIssue(kind) => kind.as_str(),
            Self::ObservationIssue(kind) => kind.as_str(),
            Self::Remote(error) => error.code(),
        }
    }
}

impl fmt::Display for ClientSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for ClientSyncError {}

impl From<sqlx::Error> for ClientSyncError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

impl From<std::io::Error> for ClientSyncError {
    fn from(_: std::io::Error) -> Self {
        Self::LocalIo
    }
}

impl From<RemoteError> for ClientSyncError {
    fn from(error: RemoteError) -> Self {
        Self::Remote(error)
    }
}
