//! Private, strict representations of the reviewed `/api/v1` JSON contract.
//! No wire DTO implements Debug: evidence and server-provided messages must
//! never escape into diagnostics. HTTP and serde errors are discarded.

use std::str::FromStr;

use serde::Deserialize;
use synveil_core::{
    ChangeEvent, ChangeEventId, ChangeKind, ChangeResourceKind, DeviceCredentialId,
    DeviceCredentialSecret, DeviceId, FileVersionId, LibraryId, LogicalName, LogicalSnapshotNode,
    NodeId, NodeKind, NodeState, Revision, Sequence, Sha256Digest, SyncBootstrap, SyncBootstrapId,
    SyncBootstrapState, Timestamp, UploadSessionId, UploadSessionState, UserId,
};
use zeroize::Zeroize;

use super::{EnrollmentCredentials, protocol_error};
use crate::{
    OpaqueEvidence, RebaselineBoundary, RebaselineHandoffConfirmation,
    RebaselineSnapshotDescriptor, RemoteCheckpoint, RemoteError, RemoteErrorKind, ReplicaScope,
    UploadCompletion, UploadSessionStatus, UploadTarget,
};

pub(super) fn parse<T: FromStr>(value: &str) -> Result<T, RemoteError> {
    value.parse().map_err(|_| protocol_error())
}

pub(super) fn optional<T: FromStr>(value: Option<&str>) -> Result<Option<T>, RemoteError> {
    value.map(parse).transpose()
}

pub(super) fn decimal(value: &str) -> Result<u64, RemoteError> {
    parse::<Sequence>(value).map(Sequence::get)
}

pub(super) fn node_kind(value: &str) -> Result<NodeKind, RemoteError> {
    match value {
        "FILE" => Ok(NodeKind::File),
        "DIRECTORY" => Ok(NodeKind::Directory),
        _ => Err(protocol_error()),
    }
}

pub(super) fn node_state(value: &str) -> Result<NodeState, RemoteError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        _ => Err(protocol_error()),
    }
}

pub(super) fn request_id(value: &str) -> Result<(), RemoteError> {
    if !(8..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~-".contains(&byte))
    {
        return Err(protocol_error());
    }
    Ok(())
}

pub(super) fn evidence(
    value: Option<String>,
    maximum: usize,
) -> Result<Option<OpaqueEvidence>, RemoteError> {
    value
        .map(|value| {
            if value.is_empty()
                || value.len() > maximum
                || !value.bytes().all(|byte| byte.is_ascii_graphic())
            {
                return Err(protocol_error());
            }
            OpaqueEvidence::new(value.into_bytes()).map_err(|_| protocol_error())
        })
        .transpose()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope<T> {
    data: T,
    meta: Meta,
}

impl<T> Envelope<T> {
    pub(super) fn data(self) -> Result<T, RemoteError> {
        request_id(&self.meta.request_id)?;
        Ok(self.data)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    device_id: String,
    library_id: String,
    epoch: String,
    acknowledged_sequence: String,
    created_at: String,
    updated_at: String,
    last_seen_high_watermark: Option<String>,
}

impl Checkpoint {
    pub(super) fn into_domain(self, scope: ReplicaScope) -> Result<RemoteCheckpoint, RemoteError> {
        validate_scope(scope, &self.device_id, &self.library_id)?;
        let epoch = parse::<Sequence>(&self.epoch)?;
        let acknowledged = parse::<Sequence>(&self.acknowledged_sequence)?;
        let created = parse::<Timestamp>(&self.created_at)?;
        let updated = parse::<Timestamp>(&self.updated_at)?;
        let high = optional::<Sequence>(self.last_seen_high_watermark.as_deref())?;
        if epoch.get() == 0 || created > updated || high.is_some_and(|high| high < acknowledged) {
            return Err(protocol_error());
        }
        Ok(RemoteCheckpoint::new(scope, epoch, acknowledged))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RebaselineHandoff {
    snapshot_id: String,
    library_id: String,
    checkpoint: RebaselineHandoffCheckpoint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RebaselineHandoffCheckpoint {
    epoch: String,
    sequence: String,
}

impl RebaselineHandoff {
    pub(super) fn into_domain(
        self,
        scope: ReplicaScope,
        expected_snapshot_id: synveil_core::RebaselineSnapshotId,
    ) -> Result<RebaselineHandoffConfirmation, RemoteError> {
        if parse::<synveil_core::RebaselineSnapshotId>(&self.snapshot_id)? != expected_snapshot_id
            || parse::<LibraryId>(&self.library_id)? != scope.library_id()
        {
            return Err(protocol_error());
        }
        let epoch = parse::<Sequence>(&self.checkpoint.epoch)?;
        let sequence = parse::<Sequence>(&self.checkpoint.sequence)?;
        if epoch.get() == 0 {
            return Err(protocol_error());
        }
        Ok(RebaselineHandoffConfirmation::new(
            expected_snapshot_id,
            scope.library_id(),
            RemoteCheckpoint::new(scope, epoch, sequence),
        ))
    }
}

pub(super) fn validate_scope(
    scope: ReplicaScope,
    device: &str,
    library: &str,
) -> Result<(), RemoteError> {
    if parse::<DeviceId>(device)? != scope.device_id()
        || parse::<LibraryId>(library)? != scope.library_id()
    {
        return Err(protocol_error());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Feed {
    pub device_id: String,
    pub library_id: String,
    pub epoch: String,
    pub from_sequence: String,
    pub through_sequence: String,
    pub high_watermark: String,
    pub has_more: bool,
    pub changes: Vec<Change>,
    pub ack_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    event_id: String,
    sequence: String,
    schema_version: u16,
    resource_kind: String,
    resource_id: String,
    change_kind: String,
    resource_revision: String,
    occurred_at: String,
    parent_node_id: Option<String>,
    node_kind: Option<String>,
    node_state: Option<String>,
    current_version_id: Option<String>,
}

impl Change {
    pub(super) fn into_domain(
        self,
        scope: ReplicaScope,
        epoch: Sequence,
    ) -> Result<ChangeEvent, RemoteError> {
        let revision = parse::<Revision>(&self.resource_revision)?;
        if self.schema_version != 1 {
            return Err(protocol_error());
        }
        Ok(ChangeEvent::new(
            parse::<ChangeEventId>(&self.event_id)?,
            scope.owner_user_id(),
            scope.library_id(),
            epoch,
            parse(&self.sequence)?,
            self.schema_version,
            parse::<ChangeResourceKind>(&self.resource_kind)?,
            parse::<NodeId>(&self.resource_id)?,
            parse::<ChangeKind>(&self.change_kind)?,
            parse::<Timestamp>(&self.occurred_at)?,
            revision,
            optional(self.parent_node_id.as_deref())?,
            self.node_kind.as_deref().map(node_kind).transpose()?,
            self.node_state.as_deref().map(node_state).transpose()?,
            optional(self.current_version_id.as_deref())?,
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bootstrap {
    bootstrap_id: String,
    device_id: String,
    library_id: String,
    state: String,
    generation: String,
    snapshot_epoch: String,
    snapshot_resume_sequence: String,
    manifest_item_count: String,
    created_at: String,
    expires_at: String,
    completed_at: Option<String>,
}

impl Bootstrap {
    pub(super) fn into_domain(self, scope: ReplicaScope) -> Result<SyncBootstrap, RemoteError> {
        validate_scope(scope, &self.device_id, &self.library_id)?;
        let generation = parse::<Sequence>(&self.generation)?;
        let epoch = parse::<Sequence>(&self.snapshot_epoch)?;
        let state = parse::<SyncBootstrapState>(&self.state)?;
        let created = parse::<Timestamp>(&self.created_at)?;
        let expires = parse::<Timestamp>(&self.expires_at)?;
        let completed = optional::<Timestamp>(self.completed_at.as_deref())?;
        let count = decimal(&self.manifest_item_count)?;
        if generation.get() == 0
            || epoch.get() == 0
            || created >= expires
            || completed.is_some_and(|value| value < created)
            || (state == SyncBootstrapState::Completed) != completed.is_some()
            || count > 1_000_000
        {
            return Err(protocol_error());
        }
        // terminal_node_id is intentionally absent from the public bootstrap
        // response. The engine validates the fully paged manifest itself.
        Ok(SyncBootstrap::new(
            parse::<SyncBootstrapId>(&self.bootstrap_id)?,
            scope.owner_user_id(),
            scope.device_id(),
            scope.library_id(),
            generation,
            epoch,
            parse(&self.snapshot_resume_sequence)?,
            count,
            None,
            state,
            created,
            expires,
            completed,
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotPage {
    pub bootstrap: Bootstrap,
    pub nodes: Vec<SnapshotNode>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
    pub completion_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DurableSnapshotPage {
    snapshot_id: String,
    library_id: String,
    journal_boundary: DurableJournalBoundary,
    entry_count: String,
    pub(super) entries: Vec<SnapshotNode>,
    pub(super) has_more: bool,
    pub(super) next_cursor: Option<String>,
}

/// The create/descriptor representation intentionally validates timestamps
/// even though the client persists only the immutable identity, boundary, and
/// entry count needed for local recovery.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DurableSnapshotDescriptor {
    snapshot_id: String,
    library_id: String,
    journal_boundary: DurableJournalBoundary,
    entry_count: String,
    created_at: String,
    expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableJournalBoundary {
    library_id: String,
    journal_epoch: String,
    resume_sequence: String,
}

impl DurableSnapshotPage {
    pub(super) fn descriptor(&self) -> Result<RebaselineSnapshotDescriptor, RemoteError> {
        let library_id = parse::<LibraryId>(&self.library_id)?;
        if parse::<LibraryId>(&self.journal_boundary.library_id)? != library_id {
            return Err(protocol_error());
        }
        let epoch = parse::<Sequence>(&self.journal_boundary.journal_epoch)?;
        if epoch.get() == 0 {
            return Err(protocol_error());
        }
        Ok(RebaselineSnapshotDescriptor::new(
            parse(&self.snapshot_id)?,
            library_id,
            RebaselineBoundary::new(epoch, parse(&self.journal_boundary.resume_sequence)?),
            decimal(&self.entry_count)?,
        ))
    }
}

impl DurableSnapshotDescriptor {
    pub(super) fn into_domain(
        self,
        scope: ReplicaScope,
    ) -> Result<RebaselineSnapshotDescriptor, RemoteError> {
        let library_id = parse::<LibraryId>(&self.library_id)?;
        if library_id != scope.library_id()
            || parse::<LibraryId>(&self.journal_boundary.library_id)? != library_id
        {
            return Err(protocol_error());
        }
        let epoch = parse::<Sequence>(&self.journal_boundary.journal_epoch)?;
        let created = parse::<Timestamp>(&self.created_at)?;
        let expires = parse::<Timestamp>(&self.expires_at)?;
        if epoch.get() == 0 || expires <= created {
            return Err(protocol_error());
        }
        Ok(RebaselineSnapshotDescriptor::new(
            parse(&self.snapshot_id)?,
            library_id,
            RebaselineBoundary::new(epoch, parse(&self.journal_boundary.resume_sequence)?),
            decimal(&self.entry_count)?,
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotNode {
    node_id: String,
    parent_node_id: Option<String>,
    name: String,
    kind: String,
    state: String,
    revision: String,
    current_version_id: Option<String>,
    current_content: Option<SnapshotContent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotContent {
    byte_length: String,
    sha256: String,
}

impl SnapshotNode {
    pub(super) fn into_domain(self) -> Result<LogicalSnapshotNode, RemoteError> {
        let (length, hash) = match self.current_content {
            Some(content) => (
                Some(decimal(&content.byte_length)?),
                Some(parse(&content.sha256)?),
            ),
            None => (None, None),
        };
        let revision = parse::<Revision>(&self.revision)?;
        if length.is_some_and(|length| length > crate::MAX_DOWNLOAD_BYTES) {
            return Err(protocol_error());
        }
        LogicalSnapshotNode::new(
            parse(&self.node_id)?,
            optional(self.parent_node_id.as_deref())?,
            LogicalName::new(self.name).map_err(|_| protocol_error())?,
            node_kind(&self.kind)?,
            node_state(&self.state)?,
            revision,
            optional(self.current_version_id.as_deref())?,
            length,
            hash,
        )
        .map_err(|_| protocol_error())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Completion {
    pub bootstrap: Bootstrap,
    pub checkpoint: CompletionCheckpoint,
    pub replayed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompletionCheckpoint {
    pub journal_epoch: String,
    pub acknowledged_sequence: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub revision: String,
    pub attributes: NodeAttributes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MutationApplied {
    pub outcome: String,
    pub mutation_id: String,
    pub kind: String,
    pub replayed: bool,
    pub node: MutationNode,
    pub journal_event_id: String,
    pub journal_sequence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MutationNode {
    pub id: String,
    pub library_id: String,
    pub parent_node_id: Option<String>,
    pub kind: String,
    pub state: String,
    pub name: String,
    pub revision: String,
    pub current_version_id: Option<String>,
    pub trashed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl MutationNode {
    pub(super) fn validate_projection(&self) -> bool {
        matches!(self.kind.as_str(), "FILE" | "DIRECTORY")
            && matches!(self.state.as_str(), "ACTIVE" | "TRASHED")
            && synveil_core::LogicalName::new(self.name.clone()).is_ok()
            && self
                .parent_node_id
                .as_ref()
                .is_none_or(|value| parse::<synveil_core::NodeId>(value).is_ok())
            && self
                .current_version_id
                .as_ref()
                .is_none_or(|value| parse::<synveil_core::FileVersionId>(value).is_ok())
            && self.created_at.parse::<synveil_core::Timestamp>().is_ok()
            && self.updated_at.parse::<synveil_core::Timestamp>().is_ok()
            && self
                .trashed_at
                .as_ref()
                .is_none_or(|value| value.parse::<synveil_core::Timestamp>().is_ok())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UploadSession {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub attributes: UploadSessionAttributes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UploadSessionAttributes {
    pub state: String,
    pub received_bytes: String,
    pub expected_bytes: String,
    pub expected_sha256: Option<String>,
    pub target: UploadTargetResource,
    pub completion: Option<UploadCompletionResource>,
    pub operation: String,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
    pub last_error_code: Option<String>,
    pub terminal_failure_code: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub(super) enum UploadTargetResource {
    #[serde(rename = "CREATE_FILE")]
    CreateFile {
        library_id: String,
        parent_id: String,
        node_id: String,
        name: String,
    },
    #[serde(rename = "REPLACE_CONTENT")]
    ReplaceContent {
        library_id: String,
        node_id: String,
        expected_revision: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UploadCompletionResource {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub attributes: UploadCompletionAttributes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UploadCompletionAttributes {
    pub node_id: String,
    pub file_version_id: String,
    pub node_revision: String,
    pub bytes: String,
    pub sha256: String,
    pub committed_at: String,
}

impl UploadSession {
    pub(super) fn into_domain(self) -> Result<UploadSessionStatus, RemoteError> {
        if self.resource_type != "upload_session" {
            return Err(protocol_error());
        }
        let session_id = parse::<UploadSessionId>(&self.id)?;
        let state = parse::<UploadSessionState>(&self.attributes.state)?;
        let expected_length = decimal(&self.attributes.expected_bytes)?;
        let received = decimal(&self.attributes.received_bytes)?;
        if received > expected_length {
            return Err(protocol_error());
        }
        let expected_sha256 = optional(self.attributes.expected_sha256.as_deref())?;
        let target = self.attributes.target.into_domain()?;
        let completion = self
            .attributes
            .completion
            .map(|value| value.into_domain(session_id))
            .transpose()?;
        parse::<Timestamp>(&self.attributes.created_at)?;
        parse::<Timestamp>(&self.attributes.updated_at)?;
        parse::<Timestamp>(&self.attributes.expires_at)?;
        let expected_operation = match target {
            UploadTarget::CreateFile { .. } => "CREATE_FILE",
            UploadTarget::ReplaceContent { .. } => "REPLACE_CONTENT",
        };
        if self.attributes.operation != expected_operation {
            return Err(protocol_error());
        }
        let version_conflict =
            self.attributes.terminal_failure_code.as_deref() == Some("version_conflict");
        let _ = self.attributes.last_error_code;
        Ok(UploadSessionStatus::new(
            session_id,
            target,
            state,
            expected_length,
            expected_sha256,
            received,
            completion,
        )
        .with_version_conflict(version_conflict))
    }
}

impl UploadTargetResource {
    fn into_domain(self) -> Result<UploadTarget, RemoteError> {
        match self {
            Self::CreateFile {
                library_id,
                parent_id,
                node_id,
                name,
            } => {
                parse::<NodeId>(&node_id)?;
                Ok(UploadTarget::CreateFile {
                    library_id: parse(&library_id)?,
                    parent_node_id: parse(&parent_id)?,
                    name: LogicalName::new(name).map_err(|_| protocol_error())?,
                })
            }
            Self::ReplaceContent {
                library_id,
                node_id,
                expected_revision,
            } => Ok(UploadTarget::ReplaceContent {
                library_id: parse(&library_id)?,
                node_id: parse(&node_id)?,
                expected_revision: parse(&expected_revision)?,
            }),
        }
    }
}

impl UploadCompletionResource {
    pub(super) fn into_domain(
        self,
        session_id: UploadSessionId,
    ) -> Result<UploadCompletion, RemoteError> {
        if self.resource_type != "upload_completion"
            || parse::<UploadSessionId>(&self.id)? != session_id
        {
            return Err(protocol_error());
        }
        parse::<Timestamp>(&self.attributes.committed_at)?;
        Ok(UploadCompletion::new(
            session_id,
            parse::<NodeId>(&self.attributes.node_id)?,
            parse::<FileVersionId>(&self.attributes.file_version_id)?,
            parse::<Revision>(&self.attributes.node_revision)?,
            decimal(&self.attributes.bytes)?,
            parse::<Sha256Digest>(&self.attributes.sha256)?,
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NodeAttributes {
    pub library_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub state: String,
    pub current_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub trashed_at: Option<String>,
    pub restore_deadline: Option<String>,
    pub purge_eligible: bool,
}

impl Node {
    pub(super) fn validate(&self, scope: ReplicaScope, node_id: NodeId) -> Result<(), RemoteError> {
        let attrs = &self.attributes;
        if self.resource_type != "node"
            || parse::<NodeId>(&self.id)? != node_id
            || parse::<LibraryId>(&attrs.library_id)? != scope.library_id()
            || parse::<Timestamp>(&attrs.created_at)? > parse::<Timestamp>(&attrs.updated_at)?
        {
            return Err(protocol_error());
        }
        parse::<Revision>(&self.revision)?;
        let kind = node_kind(&attrs.kind)?;
        let state = node_state(&attrs.state)?;
        let parent = optional::<NodeId>(attrs.parent_id.as_deref())?;
        let version = optional::<FileVersionId>(attrs.current_version_id.as_deref())?;
        LogicalName::new(attrs.name.as_str()).map_err(|_| protocol_error())?;
        if (parent.is_none() && (kind != NodeKind::Directory || state != NodeState::Active))
            || (kind == NodeKind::Directory && version.is_some())
        {
            return Err(protocol_error());
        }
        optional::<Timestamp>(attrs.trashed_at.as_deref())?;
        optional::<Timestamp>(attrs.restore_deadline.as_deref())?;
        let _ = attrs.purge_eligible;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Version {
    id: String,
    #[serde(rename = "type")]
    resource_type: String,
    attributes: VersionAttributes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionAttributes {
    node_id: String,
    created_at: String,
    byte_length: String,
    sha256: String,
    is_current: bool,
}

impl Version {
    pub(super) fn validate(
        self,
        node_id: NodeId,
        version_id: FileVersionId,
    ) -> Result<(u64, Sha256Digest), RemoteError> {
        if self.resource_type != "file_version"
            || parse::<FileVersionId>(&self.id)? != version_id
            || parse::<NodeId>(&self.attributes.node_id)? != node_id
        {
            return Err(protocol_error());
        }
        parse::<Timestamp>(&self.attributes.created_at)?;
        // An immutable snapshot/version can cease being current after the
        // metadata request. Download the exact requested version, never a
        // substitute current version; the engine verifies its expected hash.
        let _ = self.attributes.is_current;
        let length = decimal(&self.attributes.byte_length)?;
        if length > crate::MAX_DOWNLOAD_BYTES {
            return Err(RemoteError::new(RemoteErrorKind::BodyLimit));
        }
        Ok((length, parse(&self.attributes.sha256)?))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Enrollment {
    owner_user_id: String,
    device_id: String,
    credential_id: String,
    device_credential: String,
    created_at: String,
}

impl Enrollment {
    pub(super) fn into_domain(
        mut self,
        profile: crate::ServerProfile,
    ) -> Result<EnrollmentCredentials, RemoteError> {
        let secret = DeviceCredentialSecret::parse(&self.device_credential);
        self.device_credential.zeroize();
        Ok(EnrollmentCredentials {
            profile,
            owner_user_id: parse::<UserId>(&self.owner_user_id)?,
            device_id: parse::<DeviceId>(&self.device_id)?,
            credential_id: parse::<DeviceCredentialId>(&self.credential_id)?,
            secret: secret.map_err(|_| protocol_error())?,
            created_at: parse::<Timestamp>(&self.created_at)?,
        })
    }
}

impl Drop for Enrollment {
    fn drop(&mut self) {
        self.device_credential.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ready {
    pub status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ErrorBody {
    pub code: String,
    pub message: String,
    pub request_id: String,
    pub retryable: bool,
    pub details: Option<serde_json::Map<String, serde_json::Value>>,
}
