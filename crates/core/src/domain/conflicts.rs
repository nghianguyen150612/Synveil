//! Platform-neutral conflict lifecycle and explicit manual-resolution identity.
//!
//! This contract deliberately describes user-selected decisions only. It has
//! no automatic policy, merge primitive, filesystem naming rule, or raw patch
//! representation.

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};

use crate::{ConflictResolutionId, Revision, SyncConflictId};

/// Version of the canonical manual-resolution fingerprint encoding.
pub const CONFLICT_RESOLUTION_FINGERPRINT_VERSION: u16 = 1;

/// Closed lifecycle for durable conflict evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ConflictLifecycle {
    Open,
    Resolved,
    Dismissed,
}

impl ConflictLifecycle {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Resolved => "RESOLVED",
            Self::Dismissed => "DISMISSED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictLifecycleParseError;

impl fmt::Display for ConflictLifecycleParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync conflict lifecycle is unknown")
    }
}

impl std::error::Error for ConflictLifecycleParseError {}

impl FromStr for ConflictLifecycle {
    type Err = ConflictLifecycleParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "OPEN" => Ok(Self::Open),
            "RESOLVED" => Ok(Self::Resolved),
            "DISMISSED" => Ok(Self::Dismissed),
            _ => Err(ConflictLifecycleParseError),
        }
    }
}

/// The only two manual decisions supported by the protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ConflictResolutionAction {
    AcceptServer,
    ApplyClientIntent,
}

impl ConflictResolutionAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptServer => "ACCEPT_SERVER",
            Self::ApplyClientIntent => "APPLY_CLIENT_INTENT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictResolutionActionParseError;

impl fmt::Display for ConflictResolutionActionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync conflict resolution action is unknown")
    }
}

impl std::error::Error for ConflictResolutionActionParseError {}

impl FromStr for ConflictResolutionAction {
    type Err = ConflictResolutionActionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ACCEPT_SERVER" => Ok(Self::AcceptServer),
            "APPLY_CLIENT_INTENT" => Ok(Self::ApplyClientIntent),
            _ => Err(ConflictResolutionActionParseError),
        }
    }
}

/// One explicit resolution decision. Optional fields are validated against
/// the durable original mutation kind by the conflict-management service.
/// Their presence is still fingerprinted here so no alternate transport can
/// silently add or remove a precondition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictResolutionRequest {
    resolution_id: ConflictResolutionId,
    action: ConflictResolutionAction,
    expected_current_revision: Option<Revision>,
    expected_current_parent_revision: Option<Revision>,
}

impl ConflictResolutionRequest {
    #[must_use]
    pub const fn new(
        resolution_id: ConflictResolutionId,
        action: ConflictResolutionAction,
        expected_current_revision: Option<Revision>,
        expected_current_parent_revision: Option<Revision>,
    ) -> Self {
        Self {
            resolution_id,
            action,
            expected_current_revision,
            expected_current_parent_revision,
        }
    }

    #[must_use]
    pub const fn resolution_id(self) -> ConflictResolutionId {
        self.resolution_id
    }

    #[must_use]
    pub const fn action(self) -> ConflictResolutionAction {
        self.action
    }

    #[must_use]
    pub const fn expected_current_revision(self) -> Option<Revision> {
        self.expected_current_revision
    }

    #[must_use]
    pub const fn expected_current_parent_revision(self) -> Option<Revision> {
        self.expected_current_parent_revision
    }

    /// Hash a deterministic typed encoding of the conflict, action, and fresh
    /// preconditions. The resolution identity and JSON representation are
    /// deliberately excluded.
    #[must_use]
    pub fn fingerprint(self, conflict_id: SyncConflictId) -> ConflictResolutionFingerprint {
        let mut canonical = Vec::with_capacity(96);
        canonical.extend_from_slice(b"synveil:conflict-resolution\0");
        canonical.extend_from_slice(&CONFLICT_RESOLUTION_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(conflict_id.as_bytes());
        canonical.push(match self.action {
            ConflictResolutionAction::AcceptServer => 1,
            ConflictResolutionAction::ApplyClientIntent => 2,
        });
        append_optional_revision(&mut canonical, self.expected_current_revision);
        append_optional_revision(&mut canonical, self.expected_current_parent_revision);

        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        ConflictResolutionFingerprint {
            version: CONFLICT_RESOLUTION_FINGERPRINT_VERSION,
            sha256,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConflictResolutionFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl ConflictResolutionFingerprint {
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

fn append_optional_revision(canonical: &mut Vec<u8>, revision: Option<Revision>) {
    match revision {
        Some(revision) => {
            canonical.push(1);
            canonical.extend_from_slice(&revision.get().to_be_bytes());
        }
        None => canonical.push(0),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        CONFLICT_RESOLUTION_FINGERPRINT_VERSION, ConflictLifecycle, ConflictResolutionAction,
        ConflictResolutionRequest,
    };
    use crate::{ConflictResolutionId, Revision, SyncConflictId};

    #[test]
    fn lifecycle_and_action_vocabularies_are_closed() {
        for lifecycle in [
            ConflictLifecycle::Open,
            ConflictLifecycle::Resolved,
            ConflictLifecycle::Dismissed,
        ] {
            assert_eq!(
                ConflictLifecycle::from_str(lifecycle.as_str()),
                Ok(lifecycle)
            );
        }
        for action in [
            ConflictResolutionAction::AcceptServer,
            ConflictResolutionAction::ApplyClientIntent,
        ] {
            assert_eq!(
                ConflictResolutionAction::from_str(action.as_str()),
                Ok(action)
            );
        }
        assert!(ConflictLifecycle::from_str("AUTO_MERGED").is_err());
        assert!(ConflictResolutionAction::from_str("LAST_WRITE_WINS").is_err());
    }

    #[test]
    fn resolution_fingerprint_is_stable_versioned_and_identity_independent() {
        let conflict_id = SyncConflictId::new();
        let left = ConflictResolutionRequest::new(
            ConflictResolutionId::new(),
            ConflictResolutionAction::ApplyClientIntent,
            Some(Revision::new(7)),
            Some(Revision::new(4)),
        );
        let right = ConflictResolutionRequest::new(
            ConflictResolutionId::new(),
            ConflictResolutionAction::ApplyClientIntent,
            Some(Revision::new(7)),
            Some(Revision::new(4)),
        );

        assert_eq!(
            left.fingerprint(conflict_id),
            right.fingerprint(conflict_id)
        );
        assert_eq!(
            left.fingerprint(conflict_id).version(),
            CONFLICT_RESOLUTION_FINGERPRINT_VERSION
        );
    }

    #[test]
    fn semantic_resolution_changes_alter_the_fingerprint() {
        let conflict_id = SyncConflictId::new();
        let base = ConflictResolutionRequest::new(
            ConflictResolutionId::new(),
            ConflictResolutionAction::ApplyClientIntent,
            Some(Revision::new(7)),
            None,
        );
        let changed_revision = ConflictResolutionRequest::new(
            base.resolution_id(),
            base.action(),
            Some(Revision::new(8)),
            None,
        );
        let changed_action = ConflictResolutionRequest::new(
            base.resolution_id(),
            ConflictResolutionAction::AcceptServer,
            None,
            None,
        );

        assert_ne!(
            base.fingerprint(conflict_id),
            changed_revision.fingerprint(conflict_id)
        );
        assert_ne!(
            base.fingerprint(conflict_id),
            changed_action.fingerprint(conflict_id)
        );
        assert_ne!(
            base.fingerprint(conflict_id),
            base.fingerprint(SyncConflictId::new())
        );
    }
}
