//! Platform-neutral contracts for one client-to-server logical mutation.
//!
//! The protocol is intentionally closed.  A client can submit only one of the
//! explicitly named logical operations below; arbitrary JSON patch documents,
//! file bytes, object-store identities, and conflict-resolution instructions
//! are not part of this contract.

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};

use crate::{ClientMutationId, LogicalName, NodeId, Revision, Sequence};

/// Version of the canonical semantic fingerprint encoding.
pub const CLIENT_MUTATION_FINGERPRINT_VERSION: u16 = 1;

/// The supported client mutation vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ClientMutationKind {
    CreateDirectory,
    RenameNode,
    MoveNode,
    TrashNode,
    RestoreNode,
}

impl ClientMutationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateDirectory => "CREATE_DIRECTORY",
            Self::RenameNode => "RENAME_NODE",
            Self::MoveNode => "MOVE_NODE",
            Self::TrashNode => "TRASH_NODE",
            Self::RestoreNode => "RESTORE_NODE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientMutationKindParseError;

impl fmt::Display for ClientMutationKindParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("client mutation kind is unknown")
    }
}

impl std::error::Error for ClientMutationKindParseError {}

impl FromStr for ClientMutationKind {
    type Err = ClientMutationKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CREATE_DIRECTORY" => Ok(Self::CreateDirectory),
            "RENAME_NODE" => Ok(Self::RenameNode),
            "MOVE_NODE" => Ok(Self::MoveNode),
            "TRASH_NODE" => Ok(Self::TrashNode),
            "RESTORE_NODE" => Ok(Self::RestoreNode),
            _ => Err(ClientMutationKindParseError),
        }
    }
}

/// One typed semantic operation.  The expected revisions are part of the
/// operation rather than implicit transport headers so they participate in
/// idempotency and cannot be silently dropped by an alternate transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientMutation {
    CreateDirectory {
        parent_node_id: NodeId,
        expected_parent_revision: Revision,
        name: LogicalName,
    },
    RenameNode {
        node_id: NodeId,
        expected_revision: Revision,
        new_name: LogicalName,
    },
    MoveNode {
        node_id: NodeId,
        expected_revision: Revision,
        new_parent_node_id: NodeId,
        expected_new_parent_revision: Revision,
    },
    TrashNode {
        node_id: NodeId,
        expected_revision: Revision,
    },
    RestoreNode {
        node_id: NodeId,
        expected_revision: Revision,
        expected_parent_node_id: NodeId,
        expected_parent_revision: Revision,
    },
}

impl ClientMutation {
    #[must_use]
    pub fn create_directory(
        parent_node_id: NodeId,
        expected_parent_revision: Revision,
        name: LogicalName,
    ) -> Self {
        Self::CreateDirectory {
            parent_node_id,
            expected_parent_revision,
            name,
        }
    }

    #[must_use]
    pub const fn rename_node(
        node_id: NodeId,
        expected_revision: Revision,
        new_name: LogicalName,
    ) -> Self {
        Self::RenameNode {
            node_id,
            expected_revision,
            new_name,
        }
    }

    #[must_use]
    pub const fn move_node(
        node_id: NodeId,
        expected_revision: Revision,
        new_parent_node_id: NodeId,
        expected_new_parent_revision: Revision,
    ) -> Self {
        Self::MoveNode {
            node_id,
            expected_revision,
            new_parent_node_id,
            expected_new_parent_revision,
        }
    }

    #[must_use]
    pub const fn trash_node(node_id: NodeId, expected_revision: Revision) -> Self {
        Self::TrashNode {
            node_id,
            expected_revision,
        }
    }

    #[must_use]
    pub const fn restore_node(
        node_id: NodeId,
        expected_revision: Revision,
        expected_parent_node_id: NodeId,
        expected_parent_revision: Revision,
    ) -> Self {
        Self::RestoreNode {
            node_id,
            expected_revision,
            expected_parent_node_id,
            expected_parent_revision,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ClientMutationKind {
        match self {
            Self::CreateDirectory { .. } => ClientMutationKind::CreateDirectory,
            Self::RenameNode { .. } => ClientMutationKind::RenameNode,
            Self::MoveNode { .. } => ClientMutationKind::MoveNode,
            Self::TrashNode { .. } => ClientMutationKind::TrashNode,
            Self::RestoreNode { .. } => ClientMutationKind::RestoreNode,
        }
    }

    /// The primary logical resource used for observability.  A create has no
    /// resource identity until the server allocates its Node ID, so it returns
    /// the explicitly preconditioned parent instead.
    #[must_use]
    pub const fn resource_id(&self) -> NodeId {
        match self {
            Self::CreateDirectory { parent_node_id, .. }
            | Self::MoveNode {
                new_parent_node_id: parent_node_id,
                ..
            } => *parent_node_id,
            Self::RenameNode { node_id, .. }
            | Self::TrashNode { node_id, .. }
            | Self::RestoreNode { node_id, .. } => *node_id,
        }
    }
}

/// One complete client mutation envelope.  Base sync context is included in
/// the fingerprint because it is a semantic precondition, while the durable
/// identity is kept separate so a caller can never change an operation's ID
/// by changing its JSON field ordering or representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientMutationRequest {
    mutation_id: ClientMutationId,
    base_epoch: Sequence,
    base_sequence: Sequence,
    mutation: ClientMutation,
}

impl ClientMutationRequest {
    #[must_use]
    pub const fn new(
        mutation_id: ClientMutationId,
        base_epoch: Sequence,
        base_sequence: Sequence,
        mutation: ClientMutation,
    ) -> Self {
        Self {
            mutation_id,
            base_epoch,
            base_sequence,
            mutation,
        }
    }

    #[must_use]
    pub const fn mutation_id(&self) -> ClientMutationId {
        self.mutation_id
    }

    #[must_use]
    pub const fn base_epoch(&self) -> Sequence {
        self.base_epoch
    }

    #[must_use]
    pub const fn base_sequence(&self) -> Sequence {
        self.base_sequence
    }

    #[must_use]
    pub const fn mutation(&self) -> &ClientMutation {
        &self.mutation
    }

    #[must_use]
    pub const fn kind(&self) -> ClientMutationKind {
        self.mutation.kind()
    }

    #[must_use]
    pub const fn resource_id(&self) -> NodeId {
        self.mutation.resource_id()
    }

    /// Calculate the versioned SHA-256 digest over a deterministic binary
    /// encoding of semantic fields.  It deliberately excludes JSON syntax and
    /// field order, and never includes a raw request document.
    #[must_use]
    pub fn fingerprint(&self) -> ClientMutationFingerprint {
        let mut canonical = Vec::with_capacity(160);
        canonical.extend_from_slice(b"synveil:client-mutation\0");
        canonical.extend_from_slice(&CLIENT_MUTATION_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(&self.base_epoch.get().to_be_bytes());
        canonical.extend_from_slice(&self.base_sequence.get().to_be_bytes());

        match &self.mutation {
            ClientMutation::CreateDirectory {
                parent_node_id,
                expected_parent_revision,
                name,
            } => {
                canonical.push(1);
                append_id(&mut canonical, parent_node_id);
                append_revision(&mut canonical, *expected_parent_revision);
                append_name(&mut canonical, name);
            }
            ClientMutation::RenameNode {
                node_id,
                expected_revision,
                new_name,
            } => {
                canonical.push(2);
                append_id(&mut canonical, node_id);
                append_revision(&mut canonical, *expected_revision);
                append_name(&mut canonical, new_name);
            }
            ClientMutation::MoveNode {
                node_id,
                expected_revision,
                new_parent_node_id,
                expected_new_parent_revision,
            } => {
                canonical.push(3);
                append_id(&mut canonical, node_id);
                append_revision(&mut canonical, *expected_revision);
                append_id(&mut canonical, new_parent_node_id);
                append_revision(&mut canonical, *expected_new_parent_revision);
            }
            ClientMutation::TrashNode {
                node_id,
                expected_revision,
            } => {
                canonical.push(4);
                append_id(&mut canonical, node_id);
                append_revision(&mut canonical, *expected_revision);
            }
            ClientMutation::RestoreNode {
                node_id,
                expected_revision,
                expected_parent_node_id,
                expected_parent_revision,
            } => {
                canonical.push(5);
                append_id(&mut canonical, node_id);
                append_revision(&mut canonical, *expected_revision);
                append_id(&mut canonical, expected_parent_node_id);
                append_revision(&mut canonical, *expected_parent_revision);
            }
        }

        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        ClientMutationFingerprint {
            version: CLIENT_MUTATION_FINGERPRINT_VERSION,
            sha256,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClientMutationFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl ClientMutationFingerprint {
    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

fn append_id(canonical: &mut Vec<u8>, id: &NodeId) {
    canonical.extend_from_slice(id.as_bytes());
}

fn append_revision(canonical: &mut Vec<u8>, revision: Revision) {
    canonical.extend_from_slice(&revision.get().to_be_bytes());
}

fn append_name(canonical: &mut Vec<u8>, name: &LogicalName) {
    let bytes = name.as_str().as_bytes();
    canonical.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    canonical.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        CLIENT_MUTATION_FINGERPRINT_VERSION, ClientMutation, ClientMutationKind,
        ClientMutationRequest,
    };
    use crate::{ClientMutationId, LogicalName, NodeId, Revision, Sequence};

    fn request(name: &str) -> ClientMutationRequest {
        ClientMutationRequest::new(
            ClientMutationId::new(),
            Sequence::new(1),
            Sequence::new(9),
            ClientMutation::rename_node(
                NodeId::new(),
                Revision::new(4),
                LogicalName::new(name).expect("valid logical name"),
            ),
        )
    }

    #[test]
    fn mutation_kind_is_closed_and_canonical() {
        for kind in [
            ClientMutationKind::CreateDirectory,
            ClientMutationKind::RenameNode,
            ClientMutationKind::MoveNode,
            ClientMutationKind::TrashNode,
            ClientMutationKind::RestoreNode,
        ] {
            assert_eq!(ClientMutationKind::from_str(kind.as_str()), Ok(kind));
        }
        assert!(ClientMutationKind::from_str("PATCH").is_err());
    }

    #[test]
    fn canonical_fingerprint_is_stable_and_versioned() {
        let mutation_id = ClientMutationId::new();
        let left = request("old.txt");
        let right = ClientMutationRequest::new(
            mutation_id,
            left.base_epoch(),
            left.base_sequence(),
            left.mutation().clone(),
        );
        assert_eq!(left.fingerprint(), right.fingerprint());
        assert_eq!(
            left.fingerprint().version(),
            CLIENT_MUTATION_FINGERPRINT_VERSION
        );
        assert_eq!(left.fingerprint().sha256(), right.fingerprint().sha256());
    }

    #[test]
    fn semantic_change_alters_fingerprint() {
        assert_ne!(
            request("old.txt").fingerprint(),
            request("new.txt").fingerprint()
        );
    }
}
