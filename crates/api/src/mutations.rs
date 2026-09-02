//! Authenticated transport for one durable client-to-server logical mutation.
//!
//! The request is a strict, bounded JSON envelope around the closed core
//! mutation vocabulary. The handler performs only transport parsing and
//! response shaping; ownership, optimistic concurrency, idempotency, and
//! journal atomicity remain in the metadata service.

use std::str::FromStr;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use synveil_core::{
    ClientMutation, ClientMutationId, ClientMutationKind, ClientMutationRequest, DeviceId,
    LibraryId, LogicalName, Node, Sequence, UserId,
};
use synveil_metadata::{
    ClientMutationBackend, ClientMutationError, ClientMutationResult, ClientMutationService,
    RebaselineReason,
};

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthenticatedPrincipal, ResponseMeta},
};

/// Strict application-level bound for the mutation envelope. It is kept
/// separate from the global request limit so this route remains bounded if a
/// composition root later increases the general API body limit.
pub const CLIENT_MUTATION_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// PostgreSQL adapter for the transport-facing mutation port.
pub struct PostgresClientMutationBackend {
    service: ClientMutationService,
}

impl PostgresClientMutationBackend {
    #[must_use]
    pub fn new(pool: synveil_metadata::DatabasePool) -> Self {
        Self {
            service: ClientMutationService::new(pool),
        }
    }
}

#[async_trait]
impl ClientMutationBackend for PostgresClientMutationBackend {
    async fn submit(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        request: ClientMutationRequest,
    ) -> Result<ClientMutationResult, ClientMutationError> {
        self.service
            .submit(owner_user_id, device_id, library_id, request)
            .await
    }
}

/// Fail-closed backend for an unconfigured composition root.
pub(crate) struct UnavailableClientMutationBackend;

#[async_trait]
impl ClientMutationBackend for UnavailableClientMutationBackend {
    async fn submit(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _request: ClientMutationRequest,
    ) -> Result<ClientMutationResult, ClientMutationError> {
        Err(ClientMutationError::DependencyUnavailable)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmitMutationRequest {
    mutation_id: String,
    base_epoch: String,
    base_sequence: String,
    kind: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateDirectoryPayload {
    parent_node_id: String,
    expected_parent_revision: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameNodePayload {
    node_id: String,
    expected_revision: String,
    new_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveNodePayload {
    node_id: String,
    expected_revision: String,
    new_parent_node_id: String,
    expected_new_parent_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrashNodePayload {
    node_id: String,
    expected_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreNodePayload {
    node_id: String,
    expected_revision: String,
    expected_parent_node_id: String,
    expected_parent_revision: String,
}

#[derive(Debug, Serialize)]
struct MutationResponse {
    data: AppliedMutationData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct AppliedMutationData {
    outcome: &'static str,
    mutation_id: String,
    kind: &'static str,
    replayed: bool,
    node: MutationNodeData,
    journal_event_id: String,
    journal_sequence: String,
}

#[derive(Debug, Serialize)]
struct MutationNodeData {
    id: String,
    library_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_node_id: Option<String>,
    kind: &'static str,
    state: &'static str,
    name: String,
    revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trashed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

/// Submit one typed logical mutation. A conflict is returned through the
/// stable API error envelope with the durable conflict projection in
/// `details`; a retry returns the same projection and marks `replayed=true`.
pub(crate) async fn submit(
    State(state): State<ApiState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id)): Path<(String, String)>,
    Json(payload): Json<SubmitMutationRequest>,
) -> Result<Response, ApiError> {
    let device_id = parse_id::<DeviceId>(&device_id)?;
    let library_id = parse_id::<LibraryId>(&library_id)?;
    let request = parse_request(payload)?;
    let mutation_id = request.mutation_id();
    let kind = request.kind();
    let result = state
        .client_mutation_backend()
        .submit(principal.owner_user_id(), device_id, library_id, request)
        .await
        .map_err(map_client_mutation_error)?;

    match result {
        ClientMutationResult::Applied {
            node,
            journal_event_id,
            journal_sequence,
            replayed,
            ..
        } => {
            tracing::info!(
                device_id = %device_id,
                library_id = %library_id,
                mutation_id = %mutation_id,
                mutation_kind = kind.as_str(),
                outcome = "APPLIED",
                replayed,
                journal_sequence = %journal_sequence,
                "client mutation submitted"
            );
            let mut response = (
                StatusCode::OK,
                Json(MutationResponse {
                    data: AppliedMutationData {
                        outcome: "APPLIED",
                        mutation_id: mutation_id.to_string(),
                        kind: kind.as_str(),
                        replayed,
                        node: mutation_node_data(node),
                        journal_event_id: journal_event_id.to_string(),
                        journal_sequence: journal_sequence.to_string(),
                    },
                    meta: response_meta(&context),
                }),
            )
                .into_response();
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            Ok(response)
        }
        ClientMutationResult::Conflict {
            conflict_id,
            conflict,
            replayed,
            ..
        } => {
            tracing::info!(
                device_id = %device_id,
                library_id = %library_id,
                mutation_id = %mutation_id,
                mutation_kind = kind.as_str(),
                outcome = "CONFLICT",
                conflict_id = %conflict_id,
                conflict_reason = conflict.reason().as_str(),
                replayed,
                "client mutation conflict persisted"
            );
            Err(ApiError::MutationConflict {
                conflict_id,
                conflict: Box::new(conflict),
                replayed,
            })
        }
    }
}

fn parse_request(payload: SubmitMutationRequest) -> Result<ClientMutationRequest, ApiError> {
    let mutation_id = parse_id::<ClientMutationId>(&payload.mutation_id)?;
    let base_epoch = parse_decimal::<Sequence>(&payload.base_epoch)?;
    let base_sequence = parse_decimal::<Sequence>(&payload.base_sequence)?;
    let kind =
        ClientMutationKind::from_str(&payload.kind).map_err(|_| ApiError::InvalidMutation)?;
    let mutation = match kind {
        ClientMutationKind::CreateDirectory => {
            let payload = parse_payload::<CreateDirectoryPayload>(payload.payload)?;
            ClientMutation::create_directory(
                parse_id(&payload.parent_node_id)?,
                parse_decimal(&payload.expected_parent_revision)?,
                parse_name(payload.name)?,
            )
        }
        ClientMutationKind::RenameNode => {
            let payload = parse_payload::<RenameNodePayload>(payload.payload)?;
            ClientMutation::rename_node(
                parse_id(&payload.node_id)?,
                parse_decimal(&payload.expected_revision)?,
                parse_name(payload.new_name)?,
            )
        }
        ClientMutationKind::MoveNode => {
            let payload = parse_payload::<MoveNodePayload>(payload.payload)?;
            ClientMutation::move_node(
                parse_id(&payload.node_id)?,
                parse_decimal(&payload.expected_revision)?,
                parse_id(&payload.new_parent_node_id)?,
                parse_decimal(&payload.expected_new_parent_revision)?,
            )
        }
        ClientMutationKind::TrashNode => {
            let payload = parse_payload::<TrashNodePayload>(payload.payload)?;
            ClientMutation::trash_node(
                parse_id(&payload.node_id)?,
                parse_decimal(&payload.expected_revision)?,
            )
        }
        ClientMutationKind::RestoreNode => {
            let payload = parse_payload::<RestoreNodePayload>(payload.payload)?;
            ClientMutation::restore_node(
                parse_id(&payload.node_id)?,
                parse_decimal(&payload.expected_revision)?,
                parse_id(&payload.expected_parent_node_id)?,
                parse_decimal(&payload.expected_parent_revision)?,
            )
        }
    };
    Ok(ClientMutationRequest::new(
        mutation_id,
        base_epoch,
        base_sequence,
        mutation,
    ))
}

fn parse_payload<T>(payload: Value) -> Result<T, ApiError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(payload).map_err(|_| ApiError::InvalidMutation)
}

fn parse_name(value: String) -> Result<LogicalName, ApiError> {
    LogicalName::new(value).map_err(|_| ApiError::InvalidMutation)
}

fn parse_id<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    value.parse().map_err(|_| ApiError::InvalidMutation)
}

fn parse_decimal<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    value.parse().map_err(|_| ApiError::InvalidMutation)
}

fn mutation_node_data(node: Node) -> MutationNodeData {
    MutationNodeData {
        id: node.id().to_string(),
        library_id: node.library_id().to_string(),
        parent_node_id: node.parent_node_id().map(|id| id.to_string()),
        kind: node.kind().as_str(),
        state: node.state().as_str(),
        name: node.name().as_str().to_owned(),
        revision: node.revision().to_string(),
        current_version_id: node.current_version_id().map(|id| id.to_string()),
        trashed_at: node.trashed_at().map(|timestamp| timestamp.to_string()),
        created_at: node.created_at().to_string(),
        updated_at: node.updated_at().to_string(),
    }
}

fn response_meta(context: &RequestContext) -> ResponseMeta {
    ResponseMeta {
        request_id: context.request_id().to_string(),
    }
}

fn map_client_mutation_error(error: ClientMutationError) -> ApiError {
    match error {
        ClientMutationError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        ClientMutationError::InvalidMutation => ApiError::InvalidMutation,
        ClientMutationError::MutationIdConflict => ApiError::MutationIdConflict,
        ClientMutationError::RebaselineRequired {
            reason,
            current_epoch,
            minimum_retained_sequence,
        } => ApiError::SyncRebaselineRequired {
            reason: match reason {
                RebaselineReason::EpochMismatch => "epoch_mismatch",
                RebaselineReason::HistoryUnavailable => "history_unavailable",
            },
            current_epoch: current_epoch.to_string(),
            minimum_retained_sequence: minimum_retained_sequence.to_string(),
        },
        ClientMutationError::DependencyUnavailable | ClientMutationError::Database(_) => {
            ApiError::MutationDependencyUnavailable
        }
        ClientMutationError::InvalidPersistedData => ApiError::MutationInvalidPersistedData,
        ClientMutationError::InternalError => ApiError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::{CLIENT_MUTATION_BODY_LIMIT_BYTES, SubmitMutationRequest, parse_request};
    use serde_json::json;
    use synveil_core::{ClientMutationKind, Sequence};

    #[test]
    fn strict_request_parsing_builds_typed_mutation() {
        let request = parse_request(SubmitMutationRequest {
            mutation_id: synveil_core::ClientMutationId::new().to_string(),
            base_epoch: "1".to_owned(),
            base_sequence: "9".to_owned(),
            kind: "RENAME_NODE".to_owned(),
            payload: json!({
                "node_id": synveil_core::NodeId::new().to_string(),
                "expected_revision": "4",
                "new_name": "renamed.txt"
            }),
        })
        .expect("valid mutation request");

        assert_eq!(request.kind(), ClientMutationKind::RenameNode);
        assert_eq!(request.base_epoch(), Sequence::new(1));
        assert_eq!(request.base_sequence(), Sequence::new(9));
    }

    #[test]
    fn invalid_kind_and_unknown_payload_fields_are_rejected() {
        let base = SubmitMutationRequest {
            mutation_id: synveil_core::ClientMutationId::new().to_string(),
            base_epoch: "1".to_owned(),
            base_sequence: "9".to_owned(),
            kind: "PATCH".to_owned(),
            payload: json!({}),
        };
        assert!(parse_request(base).is_err());

        let request = SubmitMutationRequest {
            mutation_id: synveil_core::ClientMutationId::new().to_string(),
            base_epoch: "1".to_owned(),
            base_sequence: "9".to_owned(),
            kind: "TRASH_NODE".to_owned(),
            payload: json!({
                "node_id": synveil_core::NodeId::new().to_string(),
                "expected_revision": "4",
                "extra": true
            }),
        };
        assert!(parse_request(request).is_err());
    }

    #[test]
    fn mutation_body_limit_is_strictly_bounded() {
        assert_eq!(CLIENT_MUTATION_BODY_LIMIT_BYTES, 16 * 1024);
    }
}
