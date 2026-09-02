//! Durable client-to-server logical mutation submission.
//!
//! This module is the application boundary for one typed mutation request. It
//! deliberately delegates the PostgreSQL transaction and journal ordering to
//! [`DomainRepository`]; there is no second metadata mutation path here.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use synveil_core::{
    ChangeEventId, ClientMutationKind, ClientMutationRequest, DeviceId, LibraryId, LogicalName,
    Node, NodeId, NodeState, Revision, Sequence, SyncConflictId, UserId,
};

use crate::{DatabaseError, DatabasePool, DomainRepository, RebaselineReason};

/// Conflict reasons emitted by the optimistic-concurrency contract. The
/// vocabulary is closed so clients can make safe, explicit decisions later;
/// this phase never resolves a conflict automatically.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MutationConflictReason {
    RevisionMismatch,
    NodeStateChanged,
    ParentChanged,
    NameOccupied,
    DestinationChanged,
    ResourcePurged,
}

impl MutationConflictReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RevisionMismatch => "REVISION_MISMATCH",
            Self::NodeStateChanged => "NODE_STATE_CHANGED",
            Self::ParentChanged => "PARENT_CHANGED",
            Self::NameOccupied => "NAME_OCCUPIED",
            Self::DestinationChanged => "DESTINATION_CHANGED",
            Self::ResourcePurged => "RESOURCE_PURGED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationConflictReasonParseError;

impl fmt::Display for MutationConflictReasonParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("client mutation conflict reason is unknown")
    }
}

impl std::error::Error for MutationConflictReasonParseError {}

impl FromStr for MutationConflictReason {
    type Err = MutationConflictReasonParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "REVISION_MISMATCH" => Ok(Self::RevisionMismatch),
            "NODE_STATE_CHANGED" => Ok(Self::NodeStateChanged),
            "PARENT_CHANGED" => Ok(Self::ParentChanged),
            "NAME_OCCUPIED" => Ok(Self::NameOccupied),
            "DESTINATION_CHANGED" => Ok(Self::DestinationChanged),
            "RESOURCE_PURGED" => Ok(Self::ResourcePurged),
            _ => Err(MutationConflictReasonParseError),
        }
    }
}

/// Minimal safe state returned for a persisted optimistic-concurrency
/// conflict. All fields are logical metadata; no object-store or filesystem
/// identity is represented.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationConflict {
    reason: MutationConflictReason,
    resource_id: NodeId,
    expected_revision: Option<Revision>,
    current_revision: Option<Revision>,
    current_state: Option<NodeState>,
    current_parent_id: Option<NodeId>,
    current_name: Option<LogicalName>,
    server_epoch: Sequence,
    server_sequence: Sequence,
}

impl MutationConflict {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        reason: MutationConflictReason,
        resource_id: NodeId,
        expected_revision: Option<Revision>,
        current_revision: Option<Revision>,
        current_state: Option<NodeState>,
        current_parent_id: Option<NodeId>,
        current_name: Option<LogicalName>,
        server_epoch: Sequence,
        server_sequence: Sequence,
    ) -> Self {
        Self {
            reason,
            resource_id,
            expected_revision,
            current_revision,
            current_state,
            current_parent_id,
            current_name,
            server_epoch,
            server_sequence,
        }
    }

    #[must_use]
    pub const fn reason(&self) -> MutationConflictReason {
        self.reason
    }

    #[must_use]
    pub const fn resource_id(&self) -> NodeId {
        self.resource_id
    }

    #[must_use]
    pub const fn expected_revision(&self) -> Option<Revision> {
        self.expected_revision
    }

    #[must_use]
    pub const fn current_revision(&self) -> Option<Revision> {
        self.current_revision
    }

    #[must_use]
    pub const fn current_state(&self) -> Option<NodeState> {
        self.current_state
    }

    #[must_use]
    pub const fn current_parent_id(&self) -> Option<NodeId> {
        self.current_parent_id
    }

    #[must_use]
    pub const fn current_name(&self) -> Option<&LogicalName> {
        self.current_name.as_ref()
    }

    #[must_use]
    pub const fn server_epoch(&self) -> Sequence {
        self.server_epoch
    }

    #[must_use]
    pub const fn server_sequence(&self) -> Sequence {
        self.server_sequence
    }
}

/// Deterministic terminal result stored for one client mutation identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientMutationResult {
    Applied {
        mutation_id: synveil_core::ClientMutationId,
        kind: ClientMutationKind,
        node: Node,
        journal_event_id: ChangeEventId,
        journal_sequence: Sequence,
        replayed: bool,
    },
    Conflict {
        mutation_id: synveil_core::ClientMutationId,
        kind: ClientMutationKind,
        conflict_id: SyncConflictId,
        conflict: MutationConflict,
        replayed: bool,
    },
}

impl ClientMutationResult {
    #[must_use]
    pub const fn mutation_id(&self) -> synveil_core::ClientMutationId {
        match self {
            Self::Applied { mutation_id, .. } | Self::Conflict { mutation_id, .. } => *mutation_id,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ClientMutationKind {
        match self {
            Self::Applied { kind, .. } | Self::Conflict { kind, .. } => *kind,
        }
    }

    #[must_use]
    pub const fn replayed(&self) -> bool {
        match self {
            Self::Applied { replayed, .. } | Self::Conflict { replayed, .. } => *replayed,
        }
    }
}

/// Stable transport-neutral failures. A resource conflict is a successful
/// durable decision and is therefore represented by `ClientMutationResult`,
/// not by this error enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientMutationError {
    NotFound,
    InvalidMutation,
    MutationIdConflict,
    RebaselineRequired {
        reason: RebaselineReason,
        current_epoch: Sequence,
        minimum_retained_sequence: Sequence,
    },
    DependencyUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
    InternalError,
}

impl fmt::Display for ClientMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "client mutation scope was not found",
            Self::InvalidMutation => "client mutation is invalid",
            Self::MutationIdConflict => "client mutation identity conflicts",
            Self::RebaselineRequired { .. } => {
                "client mutation requires synchronization rebaseline"
            }
            Self::DependencyUnavailable => "client mutation dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "client mutation persisted data is invalid",
            Self::InternalError => "client mutation internal error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ClientMutationError {}

/// Application-facing port for one authenticated owner/device/library
/// mutation request.
#[async_trait]
pub trait ClientMutationBackend: Send + Sync {
    async fn submit(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        request: ClientMutationRequest,
    ) -> Result<ClientMutationResult, ClientMutationError>;
}

/// PostgreSQL-backed client mutation application service.
#[derive(Clone)]
pub struct ClientMutationService {
    pool: DatabasePool,
}

impl ClientMutationService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    pub async fn submit(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        request: ClientMutationRequest,
    ) -> Result<ClientMutationResult, ClientMutationError> {
        DomainRepository::new(&self.pool)
            .submit_client_mutation(owner_user_id, device_id, library_id, request)
            .await
    }
}

#[async_trait]
impl ClientMutationBackend for ClientMutationService {
    async fn submit(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        request: ClientMutationRequest,
    ) -> Result<ClientMutationResult, ClientMutationError> {
        self.submit(owner_user_id, device_id, library_id, request)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientMutationResult, MutationConflict, MutationConflictReason};
    use synveil_core::{
        ClientMutationId, ClientMutationKind, LogicalName, NodeId, NodeState, Revision, Sequence,
        SyncConflictId,
    };

    #[test]
    fn conflict_reason_vocabulary_is_closed_and_round_trips() {
        for reason in [
            MutationConflictReason::RevisionMismatch,
            MutationConflictReason::NodeStateChanged,
            MutationConflictReason::ParentChanged,
            MutationConflictReason::NameOccupied,
            MutationConflictReason::DestinationChanged,
            MutationConflictReason::ResourcePurged,
        ] {
            assert_eq!(reason.as_str().parse(), Ok(reason));
        }
        assert!("LAST_WRITE_WINS".parse::<MutationConflictReason>().is_err());
    }

    #[test]
    fn conflict_projection_contains_only_safe_logical_facts() {
        let resource_id = NodeId::new();
        let conflict = MutationConflict::new(
            MutationConflictReason::RevisionMismatch,
            resource_id,
            Some(Revision::new(3)),
            Some(Revision::new(4)),
            Some(NodeState::Active),
            None,
            Some(LogicalName::new("renamed").expect("valid logical name")),
            Sequence::new(1),
            Sequence::new(9),
        );
        assert_eq!(conflict.resource_id(), resource_id);
        assert_eq!(conflict.expected_revision(), Some(Revision::new(3)));
        assert_eq!(conflict.current_revision(), Some(Revision::new(4)));
        assert_eq!(
            conflict.current_name().map(|name| name.as_str()),
            Some("renamed")
        );
        assert_eq!(conflict.server_sequence(), Sequence::new(9));
    }

    #[test]
    fn result_replay_marker_is_explicit_for_safe_recovery() {
        let mutation_id = ClientMutationId::new();
        let resource_id = NodeId::new();
        let conflict = MutationConflict::new(
            MutationConflictReason::ResourcePurged,
            resource_id,
            Some(Revision::new(1)),
            Some(Revision::new(2)),
            None,
            None,
            None,
            Sequence::new(1),
            Sequence::new(2),
        );
        let result = ClientMutationResult::Conflict {
            mutation_id,
            kind: ClientMutationKind::TrashNode,
            conflict_id: SyncConflictId::new(),
            conflict,
            replayed: true,
        };
        assert_eq!(result.mutation_id(), mutation_id);
        assert_eq!(result.kind(), ClientMutationKind::TrashNode);
        assert!(result.replayed());
    }
}
