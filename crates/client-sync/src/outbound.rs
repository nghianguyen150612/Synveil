use std::{io::Read, sync::Arc};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use synveil_core::{
    ClientMutation, ClientMutationId, ClientMutationKind, ClientMutationRequest, LogicalName,
    OutboundIntentId, Sha256Digest, SyncConflictId, UploadSessionState,
};

use crate::{
    ClientSyncError, LocalFingerprint, LocalObjectKind, LocalReplica, LocalStateStore,
    MAX_UPLOAD_CHUNK_BYTES, ManagedRelativePath, OutboundIntent, OutboundIntentKind,
    OutboundIntentState, RemoteErrorKind, RemoteMutationOutcome, ReplicaScope, SyncRemote,
    UploadTarget,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundSubmissionOutcome {
    NoReadyIntent,
    Submitted(OutboundIntentId),
    Conflict {
        intent_id: OutboundIntentId,
        conflict_id: SyncConflictId,
    },
    BlockedByConflict(SyncConflictId),
    Blocked(OutboundIntentId),
    Offline,
    AuthRequired,
}

pub struct OutboundSubmissionEngine {
    scope: ReplicaScope,
    remote: Arc<dyn SyncRemote>,
    replica: Arc<dyn LocalReplica>,
    state: Arc<LocalStateStore>,
    #[cfg(test)]
    hooks: Option<Arc<dyn OutboundFailureInjector>>,
}

#[cfg(test)]
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutboundFailurePoint {
    AfterMutationIdPersistedBeforeHttp,
    AfterMutationHttpBeforeLocalResult,
    AfterMutationServerAppliedPersistedBeforeFeed,
    AfterUploadStagedBeforeSessionCreate,
    AfterUploadSessionCreatedBeforeLocalPersist,
    AfterUploadChunkBeforeLocalOffset,
    AfterUploadFinalChunkBeforeComplete,
    AfterUploadCompleteBeforeLocalResult,
    AfterUploadServerAppliedPersistedBeforeFeed,
}

#[cfg(test)]
pub(crate) trait OutboundFailureInjector: Send + Sync {
    fn check(&self, point: OutboundFailurePoint) -> Result<(), ClientSyncError>;
}

impl OutboundSubmissionEngine {
    pub async fn new(
        scope: ReplicaScope,
        remote: Arc<dyn SyncRemote>,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
    ) -> Result<Self, ClientSyncError> {
        replica.validate_root()?;
        if replica.scope() != scope {
            return Err(ClientSyncError::WrongScope);
        }
        let record = state
            .replica(scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if record.scope() != scope || record.root_binding_id() != replica.binding_id() {
            return Err(ClientSyncError::WrongRootBinding);
        }
        if record.server_profile_id() != replica.server_profile_id()
            || record.server_profile_id() != remote.server_profile_id()
        {
            return Err(ClientSyncError::WrongServerProfile);
        }
        Ok(Self {
            scope,
            remote,
            replica,
            state,
            #[cfg(test)]
            hooks: None,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    pub(crate) fn state(&self) -> &Arc<LocalStateStore> {
        &self.state
    }

    #[cfg(test)]
    pub(crate) async fn with_failure_injector(
        scope: ReplicaScope,
        remote: Arc<dyn SyncRemote>,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        hooks: Arc<dyn OutboundFailureInjector>,
    ) -> Result<Self, ClientSyncError> {
        let mut engine = Self::new(scope, remote, replica, state).await?;
        engine.hooks = Some(hooks);
        Ok(engine)
    }

    #[cfg(test)]
    fn check(&self, point: OutboundFailurePoint) -> Result<(), ClientSyncError> {
        if let Some(hooks) = &self.hooks {
            hooks.check(point)?;
        }
        Ok(())
    }

    pub async fn process_next_ready_intent(
        &self,
    ) -> Result<OutboundSubmissionOutcome, ClientSyncError> {
        let _writer = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        self.replica.validate_root()?;
        if !self
            .state
            .unresolved_issues(self.scope.library_id())
            .await?
            .is_empty()
            || !self
                .state
                .observation_issues(self.scope.library_id())
                .await?
                .is_empty()
        {
            return Ok(OutboundSubmissionOutcome::NoReadyIntent);
        }
        self.state
            .reconcile_server_applied_intents(self.scope.library_id())
            .await?;
        if let Some(conflict) = self
            .state
            .first_unresolved_conflict(self.scope.library_id())
            .await?
        {
            return Ok(OutboundSubmissionOutcome::BlockedByConflict(
                conflict.conflict_id(),
            ));
        }
        let Some(intent) = self
            .state
            .next_submittable_intent(self.scope.library_id())
            .await?
        else {
            return Ok(OutboundSubmissionOutcome::NoReadyIntent);
        };
        if intent.state() == OutboundIntentState::NeedsRebaseValidation {
            return Ok(OutboundSubmissionOutcome::NoReadyIntent);
        }
        let checkpoint = match self.remote.get_checkpoint(self.scope).await {
            Ok(checkpoint) => checkpoint,
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::Offline
                        | RemoteErrorKind::Unavailable
                        | RemoteErrorKind::Timeout
                        | RemoteErrorKind::RateLimited
                        | RemoteErrorKind::Internal
                ) =>
            {
                return Ok(OutboundSubmissionOutcome::Offline);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::AuthRequired
                        | RemoteErrorKind::DeviceRevoked
                        | RemoteErrorKind::Forbidden
                ) =>
            {
                return Ok(OutboundSubmissionOutcome::AuthRequired);
            }
            Err(error) => return Err(error.into()),
        };
        let replica = self
            .state
            .replica(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if checkpoint.scope() != self.scope
            || checkpoint.epoch() != replica.journal_epoch()
            || intent.base_epoch() != replica.journal_epoch()
            || intent.base_applied_sequence() != replica.applied_sequence()
            || checkpoint.acknowledged_sequence() < replica.acknowledged_sequence()
            || checkpoint.acknowledged_sequence() < intent.base_applied_sequence()
        {
            self.state.mark_intent_blocked(intent.intent_id()).await?;
            return Ok(OutboundSubmissionOutcome::Blocked(intent.intent_id()));
        }
        match intent.kind() {
            OutboundIntentKind::CreateDirectory
            | OutboundIntentKind::RenameNode
            | OutboundIntentKind::MoveNode
            | OutboundIntentKind::DeleteOrTrashNode => self.submit_namespace_intent(&intent).await,
            OutboundIntentKind::CreateFile | OutboundIntentKind::ModifyFileContent => {
                self.submit_upload_intent(&intent).await
            }
        }
    }

    async fn submit_namespace_intent(
        &self,
        intent: &OutboundIntent,
    ) -> Result<OutboundSubmissionOutcome, ClientSyncError> {
        self.revalidate_namespace(intent).await?;
        let record = match self
            .state
            .durable_mutation_request(intent.intent_id())
            .await?
        {
            Some(record) => record,
            None => {
                let (request, json) = mutation_request_for_intent(intent)?;
                self.state
                    .persist_mutation_request(intent.intent_id(), &request, &json)
                    .await?
            }
        };
        #[cfg(test)]
        self.check(OutboundFailurePoint::AfterMutationIdPersistedBeforeHttp)?;
        self.state
            .transition_outbound_intent(
                intent.intent_id(),
                &[
                    OutboundIntentState::Pending,
                    OutboundIntentState::Ready,
                    OutboundIntentState::Preparing,
                    OutboundIntentState::Submitting,
                ],
                OutboundIntentState::Submitting,
            )
            .await?;
        match self
            .remote
            .submit_client_mutation(self.scope, record.request())
            .await
        {
            Ok(RemoteMutationOutcome::Applied(applied)) => {
                #[cfg(test)]
                self.check(OutboundFailurePoint::AfterMutationHttpBeforeLocalResult)?;
                self.state
                    .record_mutation_applied(intent.intent_id(), applied)
                    .await?;
                #[cfg(test)]
                self.check(OutboundFailurePoint::AfterMutationServerAppliedPersistedBeforeFeed)?;
                Ok(OutboundSubmissionOutcome::Submitted(intent.intent_id()))
            }
            Ok(RemoteMutationOutcome::Conflict(conflict)) => {
                let durable = self
                    .state
                    .record_mutation_conflict(intent.intent_id(), record.mutation_id(), &conflict)
                    .await?;
                Ok(OutboundSubmissionOutcome::Conflict {
                    intent_id: intent.intent_id(),
                    conflict_id: durable.conflict_id(),
                })
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::Offline
                        | RemoteErrorKind::Unavailable
                        | RemoteErrorKind::Timeout
                        | RemoteErrorKind::RateLimited
                        | RemoteErrorKind::Internal
                ) =>
            {
                Ok(OutboundSubmissionOutcome::Offline)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::AuthRequired
                        | RemoteErrorKind::DeviceRevoked
                        | RemoteErrorKind::Forbidden
                ) =>
            {
                Ok(OutboundSubmissionOutcome::AuthRequired)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::RebaselineRequired
                        | RemoteErrorKind::Rejected
                        | RemoteErrorKind::Integrity
                        | RemoteErrorKind::NotFound
                ) =>
            {
                self.state.mark_intent_blocked(intent.intent_id()).await?;
                Ok(OutboundSubmissionOutcome::Blocked(intent.intent_id()))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn submit_upload_intent(
        &self,
        intent: &OutboundIntent,
    ) -> Result<OutboundSubmissionOutcome, ClientSyncError> {
        self.revalidate_upload(intent).await?;
        let fingerprint = intent
            .observed_fingerprint()
            .filter(|value| value.kind() == LocalObjectKind::File)
            .ok_or(ClientSyncError::InvalidState)?;
        let expected_length = fingerprint.length().ok_or(ClientSyncError::InvalidState)?;
        let expected_sha256 = fingerprint.sha256().ok_or(ClientSyncError::InvalidState)?;
        let mut upload = match self
            .state
            .durable_upload_session(intent.intent_id())
            .await?
        {
            Some(upload) => upload,
            None => {
                let staged = self
                    .stage_visible_file(intent, expected_length, expected_sha256)
                    .await?;
                self.state
                    .persist_staged_upload(
                        intent.intent_id(),
                        upload_operation(intent.kind()),
                        &staged,
                        expected_length,
                        expected_sha256,
                    )
                    .await?
            }
        };
        #[cfg(test)]
        self.check(OutboundFailurePoint::AfterUploadStagedBeforeSessionCreate)?;
        if upload.upload_session_id().is_none() {
            let status = match self
                .remote
                .create_upload_session(
                    self.scope,
                    intent.intent_id(),
                    &upload_target(intent)?,
                    upload.expected_length(),
                    upload.expected_sha256(),
                )
                .await
            {
                Ok(status) => status,
                Err(error)
                    if matches!(
                        error.kind(),
                        RemoteErrorKind::Offline
                            | RemoteErrorKind::Unavailable
                            | RemoteErrorKind::Timeout
                            | RemoteErrorKind::RateLimited
                            | RemoteErrorKind::Internal
                    ) =>
                {
                    return Ok(OutboundSubmissionOutcome::Offline);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        RemoteErrorKind::AuthRequired
                            | RemoteErrorKind::DeviceRevoked
                            | RemoteErrorKind::Forbidden
                    ) =>
                {
                    return Ok(OutboundSubmissionOutcome::AuthRequired);
                }
                Err(error) if error.kind() == RemoteErrorKind::Conflict => {
                    let durable = self
                        .state
                        .record_upload_conflict(intent.intent_id(), error.current_revision())
                        .await?;
                    return Ok(OutboundSubmissionOutcome::Conflict {
                        intent_id: intent.intent_id(),
                        conflict_id: durable.conflict_id(),
                    });
                }
                Err(error) => {
                    self.state.mark_intent_blocked(intent.intent_id()).await?;
                    return if error.kind() == RemoteErrorKind::Protocol {
                        Err(error.into())
                    } else {
                        Ok(OutboundSubmissionOutcome::Blocked(intent.intent_id()))
                    };
                }
            };
            validate_upload_status(
                intent,
                &status,
                upload.expected_length(),
                upload.expected_sha256(),
            )?;
            #[cfg(test)]
            self.check(OutboundFailurePoint::AfterUploadSessionCreatedBeforeLocalPersist)?;
            self.state
                .persist_upload_session_id(
                    intent.intent_id(),
                    status.session_id(),
                    status.received_bytes(),
                )
                .await?;
            upload = self
                .state
                .durable_upload_session(intent.intent_id())
                .await?
                .ok_or(ClientSyncError::InvalidState)?;
        }
        let session_id = upload
            .upload_session_id()
            .ok_or(ClientSyncError::InvalidState)?;
        let status = self
            .remote
            .get_upload_session(self.scope, session_id)
            .await?;
        if status.is_version_conflict() {
            let durable = self
                .state
                .record_upload_conflict(intent.intent_id(), None)
                .await?;
            return Ok(OutboundSubmissionOutcome::Conflict {
                intent_id: intent.intent_id(),
                conflict_id: durable.conflict_id(),
            });
        }
        if is_failed_upload_status(&status) {
            self.state.mark_intent_blocked(intent.intent_id()).await?;
            return Ok(OutboundSubmissionOutcome::Blocked(intent.intent_id()));
        }
        validate_upload_status(
            intent,
            &status,
            upload.expected_length(),
            upload.expected_sha256(),
        )?;
        if let Some(completion) = status.completion().cloned() {
            validate_completion(
                &completion,
                upload.expected_length(),
                upload.expected_sha256(),
            )?;
            self.state
                .record_upload_completed(intent.intent_id(), completion)
                .await?;
            #[cfg(test)]
            self.check(OutboundFailurePoint::AfterUploadServerAppliedPersistedBeforeFeed)?;
            return Ok(OutboundSubmissionOutcome::Submitted(intent.intent_id()));
        }
        let mut offset = status.received_bytes();
        self.state
            .update_upload_offset(intent.intent_id(), offset, "UPLOADING")
            .await?;
        while offset < upload.expected_length() {
            let chunk = self.replica.read_staged_chunk(
                upload.staging_relative_path(),
                offset,
                MAX_UPLOAD_CHUNK_BYTES,
            )?;
            let next = self
                .remote
                .append_upload_chunk(self.scope, session_id, offset, chunk)
                .await?;
            if next <= offset || next > upload.expected_length() {
                self.state.mark_intent_blocked(intent.intent_id()).await?;
                return Ok(OutboundSubmissionOutcome::Blocked(intent.intent_id()));
            }
            offset = next;
            #[cfg(test)]
            self.check(OutboundFailurePoint::AfterUploadChunkBeforeLocalOffset)?;
            self.state
                .update_upload_offset(intent.intent_id(), offset, "UPLOADING")
                .await?;
        }
        #[cfg(test)]
        self.check(OutboundFailurePoint::AfterUploadFinalChunkBeforeComplete)?;
        self.state
            .update_upload_offset(intent.intent_id(), offset, "COMPLETING")
            .await?;
        match self.remote.complete_upload(self.scope, session_id).await {
            Ok(completion) => {
                validate_completion(
                    &completion,
                    upload.expected_length(),
                    upload.expected_sha256(),
                )?;
                #[cfg(test)]
                self.check(OutboundFailurePoint::AfterUploadCompleteBeforeLocalResult)?;
                self.state
                    .record_upload_completed(intent.intent_id(), completion)
                    .await?;
                #[cfg(test)]
                self.check(OutboundFailurePoint::AfterUploadServerAppliedPersistedBeforeFeed)?;
                Ok(OutboundSubmissionOutcome::Submitted(intent.intent_id()))
            }
            Err(error) if error.kind() == RemoteErrorKind::Conflict => {
                let durable = self
                    .state
                    .record_upload_conflict(intent.intent_id(), error.current_revision())
                    .await?;
                Ok(OutboundSubmissionOutcome::Conflict {
                    intent_id: intent.intent_id(),
                    conflict_id: durable.conflict_id(),
                })
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::Offline
                        | RemoteErrorKind::Unavailable
                        | RemoteErrorKind::Timeout
                        | RemoteErrorKind::RateLimited
                        | RemoteErrorKind::Internal
                ) =>
            {
                Ok(OutboundSubmissionOutcome::Offline)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::AuthRequired
                        | RemoteErrorKind::DeviceRevoked
                        | RemoteErrorKind::Forbidden
                ) =>
            {
                Ok(OutboundSubmissionOutcome::AuthRequired)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    RemoteErrorKind::RebaselineRequired
                        | RemoteErrorKind::Rejected
                        | RemoteErrorKind::Integrity
                        | RemoteErrorKind::NotFound
                ) =>
            {
                self.state.mark_intent_blocked(intent.intent_id()).await?;
                Ok(OutboundSubmissionOutcome::Blocked(intent.intent_id()))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn revalidate_namespace(&self, intent: &OutboundIntent) -> Result<(), ClientSyncError> {
        let current = self
            .state
            .outbound_intent(intent.intent_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if &current != intent
            || !matches!(
                current.state(),
                OutboundIntentState::Pending
                    | OutboundIntentState::Ready
                    | OutboundIntentState::Preparing
                    | OutboundIntentState::Submitting
            )
        {
            return Err(ClientSyncError::InvalidState);
        }
        match intent.kind() {
            OutboundIntentKind::CreateDirectory => {
                if self.replica.inspect(intent.observed_relative_path())?
                    != Some(LocalFingerprint::directory())
                {
                    self.state.mark_intent_blocked(intent.intent_id()).await?;
                    return Err(ClientSyncError::ContentUnstable);
                }
                self.validate_parent(intent).await
            }
            OutboundIntentKind::RenameNode | OutboundIntentKind::MoveNode => {
                let fp = intent
                    .observed_fingerprint()
                    .ok_or(ClientSyncError::InvalidState)?;
                let observed = self.replica.inspect(intent.observed_relative_path())?;
                if observed.is_some() && observed != Some(fp) {
                    self.state.mark_intent_blocked(intent.intent_id()).await?;
                    return Err(ClientSyncError::ContentUnstable);
                }
                if self
                    .replica
                    .inspect(
                        intent
                            .old_relative_path()
                            .ok_or(ClientSyncError::InvalidState)?,
                    )?
                    .is_some()
                {
                    self.state.mark_intent_blocked(intent.intent_id()).await?;
                    return Err(ClientSyncError::ContentUnstable);
                }
                self.validate_node_base(intent).await?;
                self.validate_parent(intent).await
            }
            OutboundIntentKind::DeleteOrTrashNode => {
                if self
                    .replica
                    .inspect(intent.observed_relative_path())?
                    .is_some()
                {
                    self.state.mark_intent_blocked(intent.intent_id()).await?;
                    return Err(ClientSyncError::ContentUnstable);
                }
                self.validate_node_base(intent).await
            }
            _ => Err(ClientSyncError::InvalidState),
        }
    }

    async fn revalidate_upload(&self, intent: &OutboundIntent) -> Result<(), ClientSyncError> {
        let fingerprint = intent
            .observed_fingerprint()
            .ok_or(ClientSyncError::InvalidState)?;
        if self.replica.inspect(intent.observed_relative_path())? != Some(fingerprint)
            && self
                .state
                .durable_upload_session(intent.intent_id())
                .await?
                .is_none()
        {
            self.state.mark_intent_blocked(intent.intent_id()).await?;
            return Err(ClientSyncError::ContentUnstable);
        }
        match intent.kind() {
            OutboundIntentKind::CreateFile => self.validate_parent(intent).await,
            OutboundIntentKind::ModifyFileContent => {
                self.validate_node_base(intent).await?;
                self.validate_parent(intent).await
            }
            _ => Err(ClientSyncError::InvalidState),
        }
    }

    async fn validate_node_base(&self, intent: &OutboundIntent) -> Result<(), ClientSyncError> {
        let node_id = intent.node_id().ok_or(ClientSyncError::InvalidState)?;
        let node = self
            .state
            .local_node(intent.library_id(), node_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if node.revision()
            != intent
                .base_revision()
                .ok_or(ClientSyncError::InvalidState)?
            || node.current_version_id() != intent.base_current_version_id()
        {
            self.state.mark_intent_blocked(intent.intent_id()).await?;
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    async fn validate_parent(&self, intent: &OutboundIntent) -> Result<(), ClientSyncError> {
        let parent_id = intent
            .parent_node_id()
            .ok_or(ClientSyncError::InvalidState)?;
        let parent = self
            .state
            .local_node(intent.library_id(), parent_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if parent.revision()
            != intent
                .base_parent_revision()
                .ok_or(ClientSyncError::InvalidState)?
        {
            self.state.mark_intent_blocked(intent.intent_id()).await?;
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    async fn stage_visible_file(
        &self,
        intent: &OutboundIntent,
        expected_length: u64,
        expected_sha256: Sha256Digest,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        let file = self
            .replica
            .open_visible_file(intent.observed_relative_path())?;
        let stream = futures_util::stream::try_unfold(file, |mut file| async move {
            let mut buffer = vec![0_u8; 64 * 1024];
            let read = file
                .read(&mut buffer)
                .map_err(|_| crate::RemoteError::new(RemoteErrorKind::Offline))?;
            if read == 0 {
                Ok(None)
            } else {
                buffer.truncate(read);
                Ok(Some((Bytes::from(buffer), file)))
            }
        });
        let staged = self
            .replica
            .stage_content(
                *intent.intent_id().as_uuid(),
                expected_length,
                expected_sha256,
                crate::boxed_content_stream(stream),
            )
            .await?;
        if self.replica.inspect(intent.observed_relative_path())?
            != Some(LocalFingerprint::file(expected_length, expected_sha256))
        {
            self.replica.remove_owned_staging(&staged)?;
            return Err(ClientSyncError::ContentUnstable);
        }
        Ok(staged)
    }
}

#[derive(Deserialize, Serialize)]
struct MutationJson {
    mutation_id: String,
    base_epoch: String,
    base_sequence: String,
    kind: String,
    payload: serde_json::Value,
}

pub(crate) fn mutation_request_from_json(
    value: &str,
) -> Result<ClientMutationRequest, ClientSyncError> {
    let request: MutationJson =
        serde_json::from_str(value).map_err(|_| ClientSyncError::InvalidState)?;
    parse_mutation_json(request)
}

fn mutation_request_for_intent(
    intent: &OutboundIntent,
) -> Result<(ClientMutationRequest, String), ClientSyncError> {
    let mutation_id = ClientMutationId::new();
    let name = leaf_name(intent.observed_relative_path())?;
    let mutation = match intent.kind() {
        OutboundIntentKind::CreateDirectory => ClientMutation::create_directory(
            intent
                .parent_node_id()
                .ok_or(ClientSyncError::InvalidState)?,
            intent
                .base_parent_revision()
                .ok_or(ClientSyncError::InvalidState)?,
            name,
        ),
        OutboundIntentKind::RenameNode => ClientMutation::rename_node(
            intent.node_id().ok_or(ClientSyncError::InvalidState)?,
            intent
                .base_revision()
                .ok_or(ClientSyncError::InvalidState)?,
            name,
        ),
        OutboundIntentKind::MoveNode => ClientMutation::move_node(
            intent.node_id().ok_or(ClientSyncError::InvalidState)?,
            intent
                .base_revision()
                .ok_or(ClientSyncError::InvalidState)?,
            intent
                .parent_node_id()
                .ok_or(ClientSyncError::InvalidState)?,
            intent
                .base_parent_revision()
                .ok_or(ClientSyncError::InvalidState)?,
        ),
        OutboundIntentKind::DeleteOrTrashNode => ClientMutation::trash_node(
            intent.node_id().ok_or(ClientSyncError::InvalidState)?,
            intent
                .base_revision()
                .ok_or(ClientSyncError::InvalidState)?,
        ),
        _ => return Err(ClientSyncError::InvalidState),
    };
    let request = ClientMutationRequest::new(
        mutation_id,
        intent.base_epoch(),
        intent.base_applied_sequence(),
        mutation,
    );
    let json = mutation_request_to_json(&request)?;
    Ok((request, json))
}

pub(crate) fn mutation_request_to_json(
    request: &ClientMutationRequest,
) -> Result<String, ClientSyncError> {
    let payload = match request.mutation() {
        ClientMutation::CreateDirectory {
            parent_node_id,
            expected_parent_revision,
            name,
        } => serde_json::json!({
            "parent_node_id": parent_node_id.to_string(),
            "expected_parent_revision": expected_parent_revision.to_string(),
            "name": name.as_str(),
        }),
        ClientMutation::RenameNode {
            node_id,
            expected_revision,
            new_name,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
            "new_name": new_name.as_str(),
        }),
        ClientMutation::MoveNode {
            node_id,
            expected_revision,
            new_parent_node_id,
            expected_new_parent_revision,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
            "new_parent_node_id": new_parent_node_id.to_string(),
            "expected_new_parent_revision": expected_new_parent_revision.to_string(),
        }),
        ClientMutation::TrashNode {
            node_id,
            expected_revision,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
        }),
        ClientMutation::RestoreNode {
            node_id,
            expected_revision,
            expected_parent_node_id,
            expected_parent_revision,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
            "expected_parent_node_id": expected_parent_node_id.to_string(),
            "expected_parent_revision": expected_parent_revision.to_string(),
        }),
    };
    serde_json::to_string(&MutationJson {
        mutation_id: request.mutation_id().to_string(),
        base_epoch: request.base_epoch().to_string(),
        base_sequence: request.base_sequence().to_string(),
        kind: request.kind().as_str().to_owned(),
        payload,
    })
    .map_err(|_| ClientSyncError::InvalidState)
}

fn parse_mutation_json(request: MutationJson) -> Result<ClientMutationRequest, ClientSyncError> {
    let mutation_id = request
        .mutation_id
        .parse()
        .map_err(|_| ClientSyncError::InvalidState)?;
    let base_epoch = request
        .base_epoch
        .parse()
        .map_err(|_| ClientSyncError::InvalidState)?;
    let base_sequence = request
        .base_sequence
        .parse()
        .map_err(|_| ClientSyncError::InvalidState)?;
    let kind = request
        .kind
        .parse()
        .map_err(|_| ClientSyncError::InvalidState)?;
    let mutation = match kind {
        ClientMutationKind::CreateDirectory => {
            #[derive(Deserialize)]
            struct P {
                parent_node_id: String,
                expected_parent_revision: String,
                name: String,
            }
            let p: P = serde_json::from_value(request.payload)
                .map_err(|_| ClientSyncError::InvalidState)?;
            ClientMutation::create_directory(
                p.parent_node_id
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                p.expected_parent_revision
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                LogicalName::new(p.name).map_err(|_| ClientSyncError::InvalidState)?,
            )
        }
        ClientMutationKind::RenameNode => {
            #[derive(Deserialize)]
            struct P {
                node_id: String,
                expected_revision: String,
                new_name: String,
            }
            let p: P = serde_json::from_value(request.payload)
                .map_err(|_| ClientSyncError::InvalidState)?;
            ClientMutation::rename_node(
                p.node_id
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                p.expected_revision
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                LogicalName::new(p.new_name).map_err(|_| ClientSyncError::InvalidState)?,
            )
        }
        ClientMutationKind::MoveNode => {
            #[derive(Deserialize)]
            struct P {
                node_id: String,
                expected_revision: String,
                new_parent_node_id: String,
                expected_new_parent_revision: String,
            }
            let p: P = serde_json::from_value(request.payload)
                .map_err(|_| ClientSyncError::InvalidState)?;
            ClientMutation::move_node(
                p.node_id
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                p.expected_revision
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                p.new_parent_node_id
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                p.expected_new_parent_revision
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
            )
        }
        ClientMutationKind::TrashNode => {
            #[derive(Deserialize)]
            struct P {
                node_id: String,
                expected_revision: String,
            }
            let p: P = serde_json::from_value(request.payload)
                .map_err(|_| ClientSyncError::InvalidState)?;
            ClientMutation::trash_node(
                p.node_id
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
                p.expected_revision
                    .parse()
                    .map_err(|_| ClientSyncError::InvalidState)?,
            )
        }
        ClientMutationKind::RestoreNode => return Err(ClientSyncError::InvalidState),
    };
    Ok(ClientMutationRequest::new(
        mutation_id,
        base_epoch,
        base_sequence,
        mutation,
    ))
}

fn leaf_name(path: &ManagedRelativePath) -> Result<LogicalName, ClientSyncError> {
    path.as_str()
        .rsplit('/')
        .next()
        .ok_or(ClientSyncError::InvalidRelativePath)
        .and_then(|name| LogicalName::new(name).map_err(|_| ClientSyncError::InvalidRelativePath))
}

fn upload_operation(kind: OutboundIntentKind) -> &'static str {
    match kind {
        OutboundIntentKind::CreateFile => "CREATE_FILE",
        OutboundIntentKind::ModifyFileContent => "REPLACE_CONTENT",
        _ => "INVALID",
    }
}

fn upload_target(intent: &OutboundIntent) -> Result<UploadTarget, ClientSyncError> {
    match intent.kind() {
        OutboundIntentKind::CreateFile => Ok(UploadTarget::CreateFile {
            library_id: intent.library_id(),
            parent_node_id: intent
                .parent_node_id()
                .ok_or(ClientSyncError::InvalidState)?,
            name: leaf_name(intent.observed_relative_path())?,
        }),
        OutboundIntentKind::ModifyFileContent => Ok(UploadTarget::ReplaceContent {
            library_id: intent.library_id(),
            node_id: intent.node_id().ok_or(ClientSyncError::InvalidState)?,
            expected_revision: intent
                .base_revision()
                .ok_or(ClientSyncError::InvalidState)?,
        }),
        _ => Err(ClientSyncError::InvalidState),
    }
}

fn validate_upload_status(
    intent: &OutboundIntent,
    status: &crate::UploadSessionStatus,
    length: u64,
    sha256: Sha256Digest,
) -> Result<(), ClientSyncError> {
    if status.expected_length() != length
        || status.expected_sha256() != Some(sha256)
        || status.received_bytes() > length
        || !matches!(
            status.state(),
            UploadSessionState::Open
                | UploadSessionState::Verifying
                | UploadSessionState::Committing
                | UploadSessionState::Committed
        )
        || status.target() != &upload_target(intent)?
    {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    Ok(())
}

fn is_failed_upload_status(status: &crate::UploadSessionStatus) -> bool {
    matches!(
        status.state(),
        UploadSessionState::Failed | UploadSessionState::Expired | UploadSessionState::Aborted
    )
}

fn validate_completion(
    completion: &crate::UploadCompletion,
    length: u64,
    sha256: Sha256Digest,
) -> Result<(), ClientSyncError> {
    if completion.length() != length || completion.sha256() != sha256 {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs::{self, File},
        io::{Read, Seek, SeekFrom, Write},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use bytes::Bytes;
    use futures_util::StreamExt;
    use sha2::{Digest, Sha256};
    use synveil_core::{
        ChangeEventId, ClientMutation, ClientMutationId, DeviceId, FileVersionId, LibraryId,
        LogicalName, NodeId, NodeKind, NodeState, OutboundIntentId, Revision, Sequence,
        Sha256Digest, SyncBootstrap, SyncBootstrapId, UploadSessionId, UploadSessionState, UserId,
    };
    use uuid::Uuid;

    use super::{OutboundFailureInjector, OutboundFailurePoint};
    use super::{OutboundSubmissionEngine, OutboundSubmissionOutcome};
    use super::{mutation_request_from_json, mutation_request_to_json};
    use crate::OutboundIntentState;
    use crate::{
        BootstrapCompletion, BootstrapPage, ClientSyncError, ContentByteStream, LocalFingerprint,
        LocalReplica, LocalStateConfig, LocalStateStore, ManagedRelativePath, OpaqueEvidence,
        RemoteCheckpoint, RemoteContent, RemoteError, RemoteErrorKind, RemoteFeedPage,
        RemoteMutationApplied, RemoteMutationConflict, RemoteMutationOutcome, ReplicaScope,
        RootBindingId, SyncRemote, UploadCompletion, UploadSessionStatus, UploadTarget,
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("synveil-p39b-{label}-{}", Uuid::now_v7()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = crate::test_support::remove_dir_all_bounded(&self.0);
        }
    }

    #[derive(Clone)]
    struct FakeReplica {
        root: PathBuf,
        scope: ReplicaScope,
        binding_id: RootBindingId,
    }

    impl FakeReplica {
        fn new(root: PathBuf, scope: ReplicaScope, binding_id: RootBindingId) -> Self {
            Self {
                root,
                scope,
                binding_id,
            }
        }

        fn resolve(&self, relative: &ManagedRelativePath) -> PathBuf {
            self.root.join(relative.as_path())
        }
    }

    #[async_trait]
    impl LocalReplica for FakeReplica {
        fn scope(&self) -> ReplicaScope {
            self.scope
        }

        fn root_path(&self) -> &Path {
            &self.root
        }

        fn binding_id(&self) -> RootBindingId {
            self.binding_id
        }

        fn validate_root(&self) -> Result<(), ClientSyncError> {
            if self.root.is_dir() {
                Ok(())
            } else {
                Err(ClientSyncError::InvalidRoot)
            }
        }

        fn inspect(
            &self,
            relative: &ManagedRelativePath,
        ) -> Result<Option<LocalFingerprint>, ClientSyncError> {
            let path = self.resolve(relative);
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(ClientSyncError::LocalIo),
            };
            if metadata.is_dir() {
                return Ok(Some(LocalFingerprint::directory()));
            }
            if !metadata.is_file() {
                return Err(ClientSyncError::InvalidState);
            }
            let bytes = fs::read(path)?;
            Ok(Some(LocalFingerprint::file(
                bytes.len() as u64,
                Sha256Digest::from_bytes(Sha256::digest(&bytes).into()),
            )))
        }

        fn open_visible_file(
            &self,
            relative: &ManagedRelativePath,
        ) -> Result<File, ClientSyncError> {
            Ok(File::open(self.resolve(relative))?)
        }

        fn read_staged_chunk(
            &self,
            staging: &ManagedRelativePath,
            offset: u64,
            max: usize,
        ) -> Result<Bytes, ClientSyncError> {
            let mut file = File::open(self.resolve(staging))?;
            file.seek(SeekFrom::Start(offset))?;
            let mut buffer = vec![0_u8; max];
            let read = file.read(&mut buffer)?;
            if read == 0 {
                return Err(ClientSyncError::ContentIntegrityMismatch);
            }
            buffer.truncate(read);
            Ok(Bytes::from(buffer))
        }

        fn ensure_directory(&self, relative: &ManagedRelativePath) -> Result<(), ClientSyncError> {
            fs::create_dir_all(self.resolve(relative))?;
            Ok(())
        }

        fn has_portable_name_collision(
            &self,
            _relative: &ManagedRelativePath,
        ) -> Result<bool, ClientSyncError> {
            Ok(false)
        }

        fn stage_directory(
            &self,
            operation_id: Uuid,
        ) -> Result<ManagedRelativePath, ClientSyncError> {
            let relative =
                ManagedRelativePath::new(format!(".synveil/staging/{operation_id}.dir"))?;
            fs::create_dir_all(self.resolve(&relative))?;
            Ok(relative)
        }

        fn directory_staging_location(
            &self,
            operation_id: Uuid,
        ) -> Result<ManagedRelativePath, ClientSyncError> {
            ManagedRelativePath::new(format!(".synveil/staging/{operation_id}.dir"))
        }

        async fn stage_content(
            &self,
            operation_id: Uuid,
            expected_length: u64,
            expected_sha256: Sha256Digest,
            mut content: ContentByteStream,
        ) -> Result<ManagedRelativePath, ClientSyncError> {
            let relative =
                ManagedRelativePath::new(format!(".synveil/staging/{operation_id}.part"))?;
            let path = self.resolve(&relative);
            fs::create_dir_all(path.parent().ok_or(ClientSyncError::InvalidRelativePath)?)?;
            let mut file = File::create(path)?;
            let mut digest = Sha256::new();
            let mut length = 0_u64;
            while let Some(chunk) = content.next().await {
                let chunk = chunk.map_err(ClientSyncError::Remote)?;
                length += chunk.len() as u64;
                digest.update(&chunk);
                file.write_all(&chunk)?;
            }
            if length != expected_length
                || Sha256Digest::from_bytes(digest.finalize().into()) != expected_sha256
            {
                return Err(ClientSyncError::ContentIntegrityMismatch);
            }
            Ok(relative)
        }

        fn staging_location(
            &self,
            operation_id: Uuid,
        ) -> Result<ManagedRelativePath, ClientSyncError> {
            ManagedRelativePath::new(format!(".synveil/staging/{operation_id}.part"))
        }

        fn quarantine_location(
            &self,
            node_id: NodeId,
            operation_id: Uuid,
        ) -> Result<ManagedRelativePath, ClientSyncError> {
            ManagedRelativePath::new(format!(".synveil/quarantine/{node_id}/{operation_id}"))
        }

        fn expose_staged_file(
            &self,
            staging: &ManagedRelativePath,
            destination: &ManagedRelativePath,
            _operation_id: Uuid,
        ) -> Result<(), ClientSyncError> {
            let destination = self.resolve(destination);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(self.resolve(staging), destination)?;
            Ok(())
        }

        fn rename_path(
            &self,
            source: &ManagedRelativePath,
            destination: &ManagedRelativePath,
        ) -> Result<(), ClientSyncError> {
            fs::rename(self.resolve(source), self.resolve(destination))?;
            Ok(())
        }

        fn quarantine_path(
            &self,
            source: &ManagedRelativePath,
            node_id: NodeId,
            operation_id: Uuid,
        ) -> Result<ManagedRelativePath, ClientSyncError> {
            let quarantine = self.quarantine_location(node_id, operation_id)?;
            self.rename_path(source, &quarantine)?;
            Ok(quarantine)
        }

        fn restore_quarantined(
            &self,
            quarantine: &ManagedRelativePath,
            destination: &ManagedRelativePath,
        ) -> Result<(), ClientSyncError> {
            self.rename_path(quarantine, destination)
        }

        fn list_descendants(
            &self,
            _relative: &ManagedRelativePath,
        ) -> Result<Vec<ManagedRelativePath>, ClientSyncError> {
            Ok(Vec::new())
        }

        fn remove_owned_staging(
            &self,
            staging: &ManagedRelativePath,
        ) -> Result<(), ClientSyncError> {
            let path = self.resolve(staging);
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            } else if path.exists() {
                fs::remove_file(path)?;
            }
            Ok(())
        }

        fn write_operation_receipt(&self, _operation_id: Uuid) -> Result<(), ClientSyncError> {
            Ok(())
        }

        fn has_operation_receipt(&self, _operation_id: Uuid) -> Result<bool, ClientSyncError> {
            Ok(false)
        }

        fn remove_operation_receipt(&self, _operation_id: Uuid) -> Result<(), ClientSyncError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeRemote {
        inner: Arc<Mutex<FakeRemoteState>>,
    }

    #[derive(Default)]
    struct FakeRemoteState {
        mutation_attempts: usize,
        mutation_commits: usize,
        committed_mutations: BTreeMap<ClientMutationId, RemoteMutationApplied>,
        force_conflict: Option<RemoteMutationConflict>,
        force_error: Option<RemoteErrorKind>,
        fail_mutation_once_after_commit: bool,
        upload_attempts: usize,
        upload_commits: usize,
        upload_status_calls: usize,
        append_calls: usize,
        fail_create_once_after_commit: bool,
        fail_append_once_after_commit: bool,
        fail_complete_once_after_commit: bool,
        offset_mismatch_once: bool,
        sessions: BTreeMap<UploadSessionId, FakeSession>,
    }

    struct FakeSession {
        target: UploadTarget,
        expected_length: u64,
        expected_sha256: Sha256Digest,
        received: u64,
        completion: Option<UploadCompletion>,
    }

    impl FakeRemote {
        fn new(_scope: ReplicaScope) -> Self {
            Self {
                inner: Arc::new(Mutex::new(FakeRemoteState {
                    ..FakeRemoteState::default()
                })),
            }
        }

        fn with_state<R>(&self, action: impl FnOnce(&FakeRemoteState) -> R) -> R {
            action(&self.inner.lock().unwrap())
        }

        fn mutate_state(&self, action: impl FnOnce(&mut FakeRemoteState)) {
            action(&mut self.inner.lock().unwrap());
        }
    }

    struct OnceHook {
        point: OutboundFailurePoint,
        fired: Mutex<bool>,
    }

    impl OnceHook {
        fn new(point: OutboundFailurePoint) -> Self {
            Self {
                point,
                fired: Mutex::new(false),
            }
        }
    }

    impl OutboundFailureInjector for OnceHook {
        fn check(&self, point: OutboundFailurePoint) -> Result<(), ClientSyncError> {
            let mut fired = self.fired.lock().unwrap();
            if point == self.point && !*fired {
                *fired = true;
                return Err(ClientSyncError::InjectedFailure);
            }
            Ok(())
        }
    }

    async fn harness(
        label: &str,
    ) -> (
        TempDir,
        Arc<LocalStateStore>,
        Arc<FakeReplica>,
        FakeRemote,
        ReplicaScope,
        NodeId,
        NodeId,
        FileVersionId,
    ) {
        let temp = TempDir::new(label);
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let binding = RootBindingId::new();
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(temp.0.join("state.sqlite3")))
                .await
                .unwrap(),
        );
        state.bind_replica(scope, binding).await.unwrap();
        sqlx::query("UPDATE replicas SET journal_epoch = 1 WHERE library_id = ?")
            .bind(scope.library_id().to_string())
            .execute(&state.pool)
            .await
            .unwrap();
        let root = NodeId::new();
        state
            .upsert_local_node(&local_directory(
                scope,
                root,
                None,
                "",
                "root",
                Revision::new(1),
            ))
            .await
            .unwrap();
        let file = NodeId::new();
        let version = FileVersionId::new();
        let bytes = b"initial";
        state
            .upsert_local_node(&local_file(
                scope,
                file,
                Some(root),
                "file.txt",
                "file.txt",
                Revision::new(1),
                version,
                bytes,
            ))
            .await
            .unwrap();
        let replica = Arc::new(FakeReplica::new(temp.0.join("managed"), scope, binding));
        fs::create_dir_all(&replica.root).unwrap();
        fs::write(replica.root.join("file.txt"), bytes).unwrap();
        let remote = FakeRemote::new(scope);
        (temp, state, replica, remote, scope, root, file, version)
    }

    fn local_directory(
        scope: ReplicaScope,
        node_id: NodeId,
        parent: Option<NodeId>,
        path: &str,
        name: &str,
        revision: Revision,
    ) -> crate::LocalNode {
        crate::LocalNode::new(
            scope.library_id(),
            node_id,
            parent,
            ManagedRelativePath::new(path).unwrap(),
            LogicalName::new(name).unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            revision,
            None,
            None,
            None,
            None,
            None,
            Sequence::new(1),
            true,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn local_file(
        scope: ReplicaScope,
        node_id: NodeId,
        parent: Option<NodeId>,
        path: &str,
        name: &str,
        revision: Revision,
        version: FileVersionId,
        bytes: &[u8],
    ) -> crate::LocalNode {
        let sha256 = Sha256Digest::from_bytes(Sha256::digest(bytes).into());
        crate::LocalNode::new(
            scope.library_id(),
            node_id,
            parent,
            ManagedRelativePath::new(path).unwrap(),
            LogicalName::new(name).unwrap(),
            NodeKind::File,
            NodeState::Active,
            revision,
            Some(version),
            Some(bytes.len() as u64),
            Some(sha256),
            Some(bytes.len() as u64),
            Some(sha256),
            Sequence::new(1),
            true,
            None,
        )
    }

    async fn ready_rename_intent(
        state: &LocalStateStore,
        scope: ReplicaScope,
        root: NodeId,
        file: NodeId,
        version: FileVersionId,
        replica: &FakeReplica,
    ) -> crate::OutboundIntent {
        let intent = crate::OutboundIntent::new(
            scope.library_id(),
            Some(file),
            Some(root),
            crate::OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("renamed.txt").unwrap(),
            Some(ManagedRelativePath::new("file.txt").unwrap()),
            Some(LocalFingerprint::file(
                7,
                Sha256Digest::from_bytes(Sha256::digest(b"initial").into()),
            )),
            Sequence::new(1),
            Sequence::new(0),
            Some(Revision::new(1)),
            Some(version),
            Some(Revision::new(1)),
        )
        .unwrap();
        fs::rename(
            replica.root.join("file.txt"),
            replica.root.join("renamed.txt"),
        )
        .unwrap();
        let intent = state.upsert_outbound_intent(&intent).await.unwrap();
        sqlx::query("UPDATE outbound_intents SET state = 'READY' WHERE intent_id = ?")
            .bind(intent.intent_id().to_string())
            .execute(&state.pool)
            .await
            .unwrap();
        fs::rename(
            state.database_path().with_file_name("unused"),
            state.database_path().with_file_name("unused"),
        )
        .ok();
        state
            .outbound_intent(intent.intent_id())
            .await
            .unwrap()
            .unwrap()
    }

    async fn ready_upload_intent(
        state: &LocalStateStore,
        scope: ReplicaScope,
        root: NodeId,
        file: NodeId,
        version: FileVersionId,
        bytes: &[u8],
    ) -> crate::OutboundIntent {
        let intent = crate::OutboundIntent::new(
            scope.library_id(),
            Some(file),
            Some(root),
            crate::OutboundIntentKind::ModifyFileContent,
            ManagedRelativePath::new("file.txt").unwrap(),
            None,
            Some(LocalFingerprint::file(
                bytes.len() as u64,
                Sha256Digest::from_bytes(Sha256::digest(bytes).into()),
            )),
            Sequence::new(1),
            Sequence::new(0),
            Some(Revision::new(1)),
            Some(version),
            Some(Revision::new(1)),
        )
        .unwrap();
        let intent = state.upsert_outbound_intent(&intent).await.unwrap();
        sqlx::query("UPDATE outbound_intents SET state = 'READY' WHERE intent_id = ?")
            .bind(intent.intent_id().to_string())
            .execute(&state.pool)
            .await
            .unwrap();
        state
            .outbound_intent(intent.intent_id())
            .await
            .unwrap()
            .unwrap()
    }

    async fn ready_move_intent(
        state: &LocalStateStore,
        scope: ReplicaScope,
        root: NodeId,
        file: NodeId,
        version: FileVersionId,
        replica: &FakeReplica,
    ) -> crate::OutboundIntent {
        let destination = NodeId::new();
        state
            .upsert_local_node(&local_directory(
                scope,
                destination,
                Some(root),
                "dest",
                "dest",
                Revision::new(1),
            ))
            .await
            .unwrap();
        let intent = crate::OutboundIntent::new(
            scope.library_id(),
            Some(file),
            Some(destination),
            crate::OutboundIntentKind::MoveNode,
            ManagedRelativePath::new("dest/file.txt").unwrap(),
            Some(ManagedRelativePath::new("file.txt").unwrap()),
            Some(LocalFingerprint::file(
                7,
                Sha256Digest::from_bytes(Sha256::digest(b"initial").into()),
            )),
            Sequence::new(1),
            Sequence::new(0),
            Some(Revision::new(1)),
            Some(version),
            Some(Revision::new(1)),
        )
        .unwrap();
        fs::create_dir_all(replica.root.join("dest")).unwrap();
        fs::rename(
            replica.root.join("file.txt"),
            replica.root.join("dest/file.txt"),
        )
        .unwrap();
        let intent = state.upsert_outbound_intent(&intent).await.unwrap();
        sqlx::query("UPDATE outbound_intents SET state = 'READY' WHERE intent_id = ?")
            .bind(intent.intent_id().to_string())
            .execute(&state.pool)
            .await
            .unwrap();
        fs::rename(
            state.database_path().with_file_name("unused"),
            state.database_path().with_file_name("unused"),
        )
        .ok();
        state
            .outbound_intent(intent.intent_id())
            .await
            .unwrap()
            .unwrap()
    }

    async fn ready_trash_intent(
        state: &LocalStateStore,
        scope: ReplicaScope,
        file: NodeId,
        version: FileVersionId,
    ) -> crate::OutboundIntent {
        let local_node = state
            .local_node(scope.library_id(), file)
            .await
            .unwrap()
            .expect("file node must exist in local state");

        state
            .persist_observed_node(
                &local_node,
                local_node.relative_path(),
                true,
                Some(LocalFingerprint::file(
                    7,
                    Sha256Digest::from_bytes(Sha256::digest(b"initial").into()),
                )),
            )
            .await
            .unwrap();

        let replica_record = state
            .replica(scope.library_id())
            .await
            .unwrap()
            .expect("replica record must exist");

        let intent = crate::OutboundIntent::new(
            scope.library_id(),
            Some(file),
            None,
            crate::OutboundIntentKind::DeleteOrTrashNode,
            local_node.relative_path().clone(),
            None,
            None,
            replica_record.journal_epoch(),
            replica_record.applied_sequence(),
            Some(local_node.revision()),
            Some(version),
            None,
        )
        .unwrap();
        let intent = state.upsert_outbound_intent(&intent).await.unwrap();
        sqlx::query("UPDATE outbound_intents SET state = 'READY' WHERE intent_id = ?")
            .bind(intent.intent_id().to_string())
            .execute(&state.pool)
            .await
            .unwrap();
        state
            .outbound_intent(intent.intent_id())
            .await
            .unwrap()
            .unwrap()
    }

    #[async_trait]
    impl SyncRemote for FakeRemote {
        async fn get_checkpoint(
            &self,
            scope: ReplicaScope,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            Ok(RemoteCheckpoint::new(
                scope,
                Sequence::new(1),
                Sequence::new(0),
            ))
        }

        async fn fetch_changes(
            &self,
            scope: ReplicaScope,
            _limit: u32,
        ) -> Result<RemoteFeedPage, RemoteError> {
            Ok(RemoteFeedPage::new(
                scope,
                Sequence::new(1),
                Sequence::new(1),
                Sequence::new(0),
                Sequence::new(0),
                false,
                Vec::new(),
                None,
            )
            .unwrap())
        }

        async fn acknowledge_changes(
            &self,
            scope: ReplicaScope,
            _evidence: &OpaqueEvidence,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            Ok(RemoteCheckpoint::new(
                scope,
                Sequence::new(1),
                Sequence::new(0),
            ))
        }

        async fn start_rebaseline(
            &self,
            _scope: ReplicaScope,
        ) -> Result<SyncBootstrap, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn fetch_rebaseline_page(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: SyncBootstrapId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<BootstrapPage, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn complete_rebaseline(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: SyncBootstrapId,
            _evidence: &OpaqueEvidence,
        ) -> Result<BootstrapCompletion, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn download_current_content(
            &self,
            _scope: ReplicaScope,
            _node_id: NodeId,
            _version_id: FileVersionId,
        ) -> Result<RemoteContent, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn submit_client_mutation(
            &self,
            _scope: ReplicaScope,
            request: &synveil_core::ClientMutationRequest,
        ) -> Result<RemoteMutationOutcome, RemoteError> {
            let mut state = self.inner.lock().unwrap();
            state.mutation_attempts += 1;
            if let Some(kind) = state.force_error {
                return Err(RemoteError::new(kind));
            }
            if let Some(conflict) = state.force_conflict.clone() {
                return Ok(RemoteMutationOutcome::Conflict(conflict));
            }
            let applied =
                if let Some(applied) = state.committed_mutations.get(&request.mutation_id()) {
                    RemoteMutationApplied::new(
                        applied.mutation_id(),
                        applied.node_id(),
                        applied.revision(),
                        applied.journal_event_id(),
                        applied.journal_sequence(),
                        true,
                    )
                } else {
                    state.mutation_commits += 1;
                    let node_id = match request.mutation() {
                        ClientMutation::CreateDirectory { parent_node_id, .. } => *parent_node_id,
                        ClientMutation::RenameNode { node_id, .. }
                        | ClientMutation::MoveNode { node_id, .. }
                        | ClientMutation::TrashNode { node_id, .. }
                        | ClientMutation::RestoreNode { node_id, .. } => *node_id,
                    };
                    let applied = RemoteMutationApplied::new(
                        request.mutation_id(),
                        node_id,
                        Revision::new(2),
                        ChangeEventId::new(),
                        Sequence::new(1),
                        false,
                    );
                    state
                        .committed_mutations
                        .insert(request.mutation_id(), applied);
                    applied
                };
            if state.fail_mutation_once_after_commit {
                state.fail_mutation_once_after_commit = false;
                return Err(RemoteError::new(RemoteErrorKind::Timeout));
            }
            Ok(RemoteMutationOutcome::Applied(applied))
        }

        async fn create_upload_session(
            &self,
            _scope: ReplicaScope,
            idempotency_key: OutboundIntentId,
            target: &UploadTarget,
            expected_length: u64,
            expected_sha256: Sha256Digest,
        ) -> Result<UploadSessionStatus, RemoteError> {
            let mut state = self.inner.lock().unwrap();
            state.upload_attempts += 1;
            let session_id = UploadSessionId::try_from_uuid(*idempotency_key.as_uuid())
                .map_err(|_| RemoteError::new(RemoteErrorKind::Protocol))?;
            state
                .sessions
                .entry(session_id)
                .or_insert_with(|| FakeSession {
                    target: target.clone(),
                    expected_length,
                    expected_sha256,
                    received: 0,
                    completion: None,
                });
            let session = state.sessions.get(&session_id).unwrap();
            if session.target != *target
                || session.expected_length != expected_length
                || session.expected_sha256 != expected_sha256
            {
                return Err(RemoteError::new(RemoteErrorKind::Rejected));
            }
            if state.fail_create_once_after_commit {
                state.fail_create_once_after_commit = false;
                return Err(RemoteError::new(RemoteErrorKind::Timeout));
            }
            Ok(status(session_id, state.sessions.get(&session_id).unwrap()))
        }

        async fn get_upload_session(
            &self,
            _scope: ReplicaScope,
            session_id: UploadSessionId,
        ) -> Result<UploadSessionStatus, RemoteError> {
            let mut state = self.inner.lock().unwrap();
            state.upload_status_calls += 1;
            let session = state
                .sessions
                .get(&session_id)
                .ok_or_else(|| RemoteError::new(RemoteErrorKind::NotFound))?;
            Ok(status(session_id, session))
        }

        async fn append_upload_chunk(
            &self,
            _scope: ReplicaScope,
            session_id: UploadSessionId,
            expected_offset: u64,
            chunk: Bytes,
        ) -> Result<u64, RemoteError> {
            let mut state = self.inner.lock().unwrap();
            state.append_calls += 1;
            if state.offset_mismatch_once {
                state.offset_mismatch_once = false;
                return Ok(expected_offset + chunk.len() as u64 + 1);
            }
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| RemoteError::new(RemoteErrorKind::NotFound))?;
            if session.received != expected_offset {
                return Err(RemoteError::new(RemoteErrorKind::Rejected));
            }
            session.received += chunk.len() as u64;
            let received = session.received;
            if state.fail_append_once_after_commit {
                state.fail_append_once_after_commit = false;
                return Err(RemoteError::new(RemoteErrorKind::Timeout));
            }
            Ok(received)
        }

        async fn complete_upload(
            &self,
            _scope: ReplicaScope,
            session_id: UploadSessionId,
        ) -> Result<UploadCompletion, RemoteError> {
            let mut state = self.inner.lock().unwrap();
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| RemoteError::new(RemoteErrorKind::NotFound))?;
            if session.received != session.expected_length {
                return Err(RemoteError::new(RemoteErrorKind::Rejected));
            }
            let existing = session.completion;
            let session_expected_length = session.expected_length;
            let session_expected_sha256 = session.expected_sha256;
            let session_node_id = match session.target {
                UploadTarget::CreateFile { parent_node_id, .. } => parent_node_id,
                UploadTarget::ReplaceContent { node_id, .. } => node_id,
            };
            let completion = if let Some(completion) = existing {
                completion
            } else {
                state.upload_commits += 1;
                let completion = UploadCompletion::new(
                    session_id,
                    session_node_id,
                    FileVersionId::new(),
                    Revision::new(2),
                    session_expected_length,
                    session_expected_sha256,
                );
                state.sessions.get_mut(&session_id).unwrap().completion = Some(completion);
                completion
            };
            if state.fail_complete_once_after_commit {
                state.fail_complete_once_after_commit = false;
                return Err(RemoteError::new(RemoteErrorKind::Timeout));
            }
            Ok(completion)
        }
    }

    fn status(session_id: UploadSessionId, session: &FakeSession) -> UploadSessionStatus {
        UploadSessionStatus::new(
            session_id,
            session.target.clone(),
            if session.completion.is_some() {
                UploadSessionState::Committed
            } else {
                UploadSessionState::Open
            },
            session.expected_length,
            Some(session.expected_sha256),
            session.received,
            session.completion,
        )
    }

    #[tokio::test]
    async fn namespace_crash_matrix_reuses_one_mutation_id_and_one_server_commit() {
        for point in [
            OutboundFailurePoint::AfterMutationIdPersistedBeforeHttp,
            OutboundFailurePoint::AfterMutationHttpBeforeLocalResult,
            OutboundFailurePoint::AfterMutationServerAppliedPersistedBeforeFeed,
        ] {
            let (_temp, state, replica, remote, scope, root, file, version) =
                harness("namespace-crash").await;
            let intent = ready_rename_intent(&state, scope, root, file, version, &replica).await;
            let engine = OutboundSubmissionEngine::with_failure_injector(
                scope,
                Arc::new(remote.clone()),
                replica.clone(),
                state.clone(),
                Arc::new(OnceHook::new(point)),
            )
            .await
            .unwrap();
            assert!(matches!(
                engine.process_next_ready_intent().await,
                Err(ClientSyncError::InjectedFailure)
            ));
            let first_id = state
                .durable_mutation_request(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .mutation_id();
            let retry = OutboundSubmissionEngine::new(
                scope,
                Arc::new(remote.clone()),
                replica,
                state.clone(),
            )
            .await
            .unwrap();
            let retry_outcome = retry.process_next_ready_intent().await.unwrap();
            if point == OutboundFailurePoint::AfterMutationServerAppliedPersistedBeforeFeed {
                assert_eq!(retry_outcome, OutboundSubmissionOutcome::NoReadyIntent);
            } else {
                assert_eq!(
                    retry_outcome,
                    OutboundSubmissionOutcome::Submitted(intent.intent_id())
                );
            }
            assert_eq!(
                state
                    .durable_mutation_request(intent.intent_id())
                    .await
                    .unwrap()
                    .unwrap()
                    .mutation_id(),
                first_id
            );
            remote.with_state(|state| {
                assert_eq!(state.mutation_commits, 1);
                assert_eq!(state.committed_mutations.len(), 1);
            });
            let intent_after = state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(intent_after.state(), OutboundIntentState::ServerApplied);
            let before = state.replica(scope.library_id()).await.unwrap().unwrap();
            assert_eq!(before.applied_sequence(), Sequence::new(0));
            assert_eq!(before.acknowledged_sequence(), Sequence::new(0));
            state
                .upsert_local_node(&local_file(
                    scope,
                    file,
                    Some(root),
                    "renamed.txt",
                    "renamed.txt",
                    Revision::new(2),
                    FileVersionId::new(),
                    b"initial",
                ))
                .await
                .unwrap();
            assert_eq!(
                state
                    .reconcile_server_applied_intents(scope.library_id())
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                state
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .unwrap()
                    .state(),
                OutboundIntentState::Reconciled
            );
        }
    }

    #[tokio::test]
    async fn upload_crash_and_lost_response_matrix_resumes_without_duplicate_commit() {
        for point in [
            OutboundFailurePoint::AfterUploadStagedBeforeSessionCreate,
            OutboundFailurePoint::AfterUploadSessionCreatedBeforeLocalPersist,
            OutboundFailurePoint::AfterUploadChunkBeforeLocalOffset,
            OutboundFailurePoint::AfterUploadFinalChunkBeforeComplete,
            OutboundFailurePoint::AfterUploadCompleteBeforeLocalResult,
            OutboundFailurePoint::AfterUploadServerAppliedPersistedBeforeFeed,
        ] {
            let bytes = b"replacement content staged immutably";
            let (_temp, state, replica, remote, scope, root, file, version) =
                harness("upload-crash").await;
            fs::write(replica.root.join("file.txt"), bytes).unwrap();
            let intent = ready_upload_intent(&state, scope, root, file, version, bytes).await;
            let engine = OutboundSubmissionEngine::with_failure_injector(
                scope,
                Arc::new(remote.clone()),
                replica.clone(),
                state.clone(),
                Arc::new(OnceHook::new(point)),
            )
            .await
            .unwrap();
            assert!(matches!(
                engine.process_next_ready_intent().await,
                Err(ClientSyncError::InjectedFailure)
            ));
            let retry = OutboundSubmissionEngine::new(
                scope,
                Arc::new(remote.clone()),
                replica.clone(),
                state.clone(),
            )
            .await
            .unwrap();
            let retry_outcome = retry.process_next_ready_intent().await.unwrap();
            if point == OutboundFailurePoint::AfterUploadServerAppliedPersistedBeforeFeed {
                assert_eq!(retry_outcome, OutboundSubmissionOutcome::NoReadyIntent);
            } else {
                assert_eq!(
                    retry_outcome,
                    OutboundSubmissionOutcome::Submitted(intent.intent_id())
                );
            }
            remote.with_state(|state| assert_eq!(state.upload_commits, 1));
            let upload = state
                .durable_upload_session(intent.intent_id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(upload.expected_length(), bytes.len() as u64);
            assert_eq!(
                upload.expected_sha256(),
                Sha256Digest::from_bytes(Sha256::digest(bytes).into())
            );
            assert_eq!(upload.state(), "COMPLETED");
            assert_eq!(
                state
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .unwrap()
                    .state(),
                OutboundIntentState::ServerApplied
            );
            state
                .upsert_local_node(&local_file(
                    scope,
                    file,
                    Some(root),
                    "file.txt",
                    "file.txt",
                    Revision::new(2),
                    FileVersionId::new(),
                    bytes,
                ))
                .await
                .unwrap();
            assert_eq!(
                state
                    .reconcile_server_applied_intents(scope.library_id())
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                state
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .unwrap()
                    .state(),
                OutboundIntentState::Reconciled
            );
        }
    }

    #[tokio::test]
    async fn upload_instability_matrix_blocks_or_uses_immutable_staging() {
        let bytes = b"replacement content staged immutably";
        let (_temp, state, replica, remote, scope, root, file, version) =
            harness("upload-instability").await;
        fs::write(replica.root.join("file.txt"), b"different").unwrap();
        let before_staging = ready_upload_intent(&state, scope, root, file, version, bytes).await;
        let engine = OutboundSubmissionEngine::new(
            scope,
            Arc::new(remote.clone()),
            replica.clone(),
            state.clone(),
        )
        .await
        .unwrap();
        assert!(matches!(
            engine.process_next_ready_intent().await,
            Err(ClientSyncError::ContentUnstable)
        ));
        assert_eq!(
            state
                .outbound_intent(before_staging.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Blocked
        );

        let (_temp, state, replica, remote, scope, root, file, version) =
            harness("upload-immutable").await;
        fs::write(replica.root.join("file.txt"), bytes).unwrap();
        let intent = ready_upload_intent(&state, scope, root, file, version, bytes).await;
        let engine = OutboundSubmissionEngine::with_failure_injector(
            scope,
            Arc::new(remote.clone()),
            replica.clone(),
            state.clone(),
            Arc::new(OnceHook::new(
                OutboundFailurePoint::AfterUploadStagedBeforeSessionCreate,
            )),
        )
        .await
        .unwrap();
        assert!(matches!(
            engine.process_next_ready_intent().await,
            Err(ClientSyncError::InjectedFailure)
        ));
        fs::write(replica.root.join("file.txt"), b"later distinct edit").unwrap();
        let retry =
            OutboundSubmissionEngine::new(scope, Arc::new(remote), replica.clone(), state.clone())
                .await
                .unwrap();
        assert_eq!(
            retry.process_next_ready_intent().await.unwrap(),
            OutboundSubmissionOutcome::Submitted(intent.intent_id())
        );
        let upload = state
            .durable_upload_session(intent.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(upload.expected_length(), bytes.len() as u64);
        assert_eq!(
            upload.expected_sha256(),
            Sha256Digest::from_bytes(Sha256::digest(bytes).into())
        );
        assert_eq!(
            fs::read(replica.root.join("file.txt")).unwrap(),
            b"later distinct edit"
        );
    }

    #[tokio::test]
    async fn conflicts_and_offset_mismatch_stop_automatic_retry() {
        let (_temp, state, replica, remote, scope, root, file, version) = harness("conflict").await;
        let intent = ready_rename_intent(&state, scope, root, file, version, &replica).await;
        remote.mutate_state(|state| {
            state.force_conflict = Some(
                RemoteMutationConflict::new(
                    synveil_core::SyncConflictId::new(),
                    "REVISION_MISMATCH".to_owned(),
                    false,
                )
                .unwrap(),
            );
        });
        let engine = OutboundSubmissionEngine::new(scope, Arc::new(remote), replica, state.clone())
            .await
            .unwrap();
        let conflict_id = match engine.process_next_ready_intent().await.unwrap() {
            OutboundSubmissionOutcome::Conflict {
                intent_id,
                conflict_id,
            } if intent_id == intent.intent_id() => conflict_id,
            other => panic!("unexpected conflict outcome: {other:?}"),
        };
        assert_eq!(
            engine.process_next_ready_intent().await.unwrap(),
            OutboundSubmissionOutcome::BlockedByConflict(conflict_id)
        );
        assert_eq!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Conflict
        );

        let bytes = b"replacement content staged immutably";
        let (_temp, state, replica, remote, scope, root, file, version) =
            harness("offset-mismatch").await;
        fs::write(replica.root.join("file.txt"), bytes).unwrap();
        let intent = ready_upload_intent(&state, scope, root, file, version, bytes).await;
        remote.mutate_state(|state| state.offset_mismatch_once = true);
        let engine = OutboundSubmissionEngine::new(scope, Arc::new(remote), replica, state.clone())
            .await
            .unwrap();
        assert_eq!(
            engine.process_next_ready_intent().await.unwrap(),
            OutboundSubmissionOutcome::Blocked(intent.intent_id())
        );
        assert_eq!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Blocked
        );
    }

    #[tokio::test]
    async fn non_conflict_transport_and_authorization_failures_never_create_conflicts() {
        for (label, kind, expected) in [
            (
                "auth-not-conflict",
                RemoteErrorKind::AuthRequired,
                OutboundSubmissionOutcome::AuthRequired,
            ),
            (
                "forbidden-not-conflict",
                RemoteErrorKind::Forbidden,
                OutboundSubmissionOutcome::AuthRequired,
            ),
            (
                "rate-not-conflict",
                RemoteErrorKind::RateLimited,
                OutboundSubmissionOutcome::Offline,
            ),
            (
                "server-not-conflict",
                RemoteErrorKind::Internal,
                OutboundSubmissionOutcome::Offline,
            ),
            (
                "network-not-conflict",
                RemoteErrorKind::Offline,
                OutboundSubmissionOutcome::Offline,
            ),
        ] {
            let (_temp, state, replica, remote, scope, root, file, version) = harness(label).await;
            let intent = ready_rename_intent(&state, scope, root, file, version, &replica).await;
            remote.mutate_state(|state| state.force_error = Some(kind));
            let engine =
                OutboundSubmissionEngine::new(scope, Arc::new(remote), replica, state.clone())
                    .await
                    .unwrap();
            assert_eq!(engine.process_next_ready_intent().await.unwrap(), expected);
            assert!(
                state
                    .list_unresolved_conflicts(scope.library_id(), None, None)
                    .await
                    .unwrap()
                    .items()
                    .is_empty()
            );
            assert_eq!(
                state
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .unwrap()
                    .base_revision(),
                intent.base_revision()
            );
        }
    }

    #[tokio::test]
    async fn stale_move_conflict_stops_automatic_retry() {
        let (_temp, state, replica, remote, scope, root, file, version) =
            harness("conflict-move").await;
        let intent = ready_move_intent(&state, scope, root, file, version, &replica).await;
        remote.mutate_state(|state| {
            state.force_conflict = Some(
                RemoteMutationConflict::new(
                    synveil_core::SyncConflictId::new(),
                    "PARENT_CHANGED".to_owned(),
                    false,
                )
                .unwrap(),
            );
        });
        let engine = OutboundSubmissionEngine::new(scope, Arc::new(remote), replica, state.clone())
            .await
            .unwrap();
        assert!(matches!(
            engine.process_next_ready_intent().await.unwrap(),
            OutboundSubmissionOutcome::Conflict { intent_id, .. } if intent_id == intent.intent_id()
        ));
        assert_eq!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Conflict
        );
    }

    #[tokio::test]
    async fn stale_trash_conflict_stops_automatic_retry() {
        let (_temp, state, replica, remote, scope, _root, file, version) =
            harness("conflict-trash").await;
        fs::remove_file(replica.root.join("file.txt")).unwrap();
        let intent = ready_trash_intent(&state, scope, file, version).await;
        remote.mutate_state(|state| {
            state.force_conflict = Some(
                RemoteMutationConflict::new(
                    synveil_core::SyncConflictId::new(),
                    "REVISION_MISMATCH".to_owned(),
                    false,
                )
                .unwrap(),
            );
        });
        let engine = OutboundSubmissionEngine::new(scope, Arc::new(remote), replica, state.clone())
            .await
            .unwrap();
        assert!(matches!(
            engine.process_next_ready_intent().await.unwrap(),
            OutboundSubmissionOutcome::Conflict { intent_id, .. } if intent_id == intent.intent_id()
        ));
        assert_eq!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Conflict
        );
    }

    #[tokio::test]
    async fn stale_file_content_replacement_blocks_overwrite() {
        let bytes = b"new content that conflicts with server state";
        let (_temp, state, replica, remote, scope, root, file, version) =
            harness("conflict-content").await;
        fs::write(replica.root.join("file.txt"), bytes).unwrap();
        let intent = ready_upload_intent(&state, scope, root, file, version, bytes).await;
        remote.mutate_state(|state| state.offset_mismatch_once = true);
        let engine = OutboundSubmissionEngine::new(scope, Arc::new(remote), replica, state.clone())
            .await
            .unwrap();
        assert_eq!(
            engine.process_next_ready_intent().await.unwrap(),
            OutboundSubmissionOutcome::Blocked(intent.intent_id())
        );
        assert_eq!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Blocked
        );
    }

    #[test]
    fn persisted_mutation_json_round_trips_exact_identity_and_semantics() {
        let request = synveil_core::ClientMutationRequest::new(
            ClientMutationId::new(),
            Sequence::new(7),
            Sequence::new(42),
            ClientMutation::rename_node(
                NodeId::new(),
                Revision::new(3),
                LogicalName::new("renamed.txt").unwrap(),
            ),
        );

        let json = mutation_request_to_json(&request).unwrap();
        let parsed = mutation_request_from_json(&json).unwrap();

        assert_eq!(parsed, request);
        assert_eq!(parsed.mutation_id(), request.mutation_id());
        assert_eq!(parsed.fingerprint(), request.fingerprint());
        assert_eq!(mutation_request_to_json(&parsed).unwrap(), json);
    }
}
