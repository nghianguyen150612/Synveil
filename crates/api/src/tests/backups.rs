use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Method, StatusCode},
};
use serde_json::Value;
use synveil_core::{
    BackupMaintenanceRun, BackupMaintenanceRunId, BackupMaintenanceRunRequest,
    BackupMaintenanceRunState, BackupManifestContent, BackupOperationKind, BackupPruneExecution,
    BackupPruneExecutionId, BackupPrunePlan, BackupPrunePlanId, BackupRestoreExecution,
    BackupRestoreExecutionId, BackupRestorePlan, BackupRestorePlanId, BackupSet, BackupSetId,
    BackupSnapshot, BackupSnapshotExpiryExecutionId, BackupSnapshotExpiryPlanId,
    BackupSnapshotNode, BackupSnapshotRetentionPolicyConfig, BackupSnapshotRetentionPolicyRequest,
    BackupSnapshotRetentionPolicyRevision, BackupSnapshotRetentionPolicyRevisionId,
    BackupSnapshotRetentionPolicyRevisionNumber, FileVersionId, LibraryId, LogicalName, NodeId,
    NodeKind, NodeState, Revision, Sequence, Sha256Digest, SnapshotId, SnapshotState, Timestamp,
    UserId,
};
use synveil_metadata::{
    BackupError, BackupMaintenanceRunPagePosition, BackupOperationDetail, BackupOperationId,
    BackupOperationPagePosition, BackupOperationSummary, BackupReadBackend,
    BackupSnapshotPagePosition,
};
use tower::ServiceExt;

use super::{
    ApiState, CookieConfig, TestAuthenticationBackend, authenticated_request, json_body,
    login_cookies, router, state,
};

struct FixtureData {
    sets: Vec<BackupSet>,
    backup_set: BackupSet,
    secondary_set: BackupSet,
    snapshots: Vec<BackupSnapshot>,
    nodes: Vec<BackupSnapshotNode>,
    policy: BackupSnapshotRetentionPolicyRevision,
    runs: Vec<BackupMaintenanceRun>,
}

struct TestBackupReadBackend {
    sets: Vec<BackupSet>,
    snapshots: Vec<BackupSnapshot>,
    nodes: Vec<BackupSnapshotNode>,
    current_policy: Mutex<Option<BackupSnapshotRetentionPolicyRevision>>,
    runs: Vec<BackupMaintenanceRun>,
    read_calls: AtomicUsize,
}

impl TestBackupReadBackend {
    fn new(_owner_user_id: UserId, data: FixtureData) -> Self {
        Self {
            sets: data.sets,
            snapshots: data.snapshots,
            nodes: data.nodes,
            current_policy: Mutex::new(Some(data.policy)),
            runs: data.runs,
            read_calls: AtomicUsize::new(0),
        }
    }

    fn record_read(&self) {
        self.read_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn scope_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<(), BackupError> {
        if self
            .sets
            .iter()
            .any(|set| set.owner_user_id() == owner_user_id && set.id() == backup_set_id)
        {
            Ok(())
        } else {
            Err(BackupError::NotFound)
        }
    }
}

#[async_trait]
impl BackupReadBackend for TestBackupReadBackend {
    async fn list_backup_sets(
        &self,
        owner_user_id: UserId,
        after: Option<BackupSetId>,
        limit: u32,
    ) -> Result<(Vec<BackupSet>, bool), BackupError> {
        self.record_read();
        let mut sets: Vec<_> = self
            .sets
            .iter()
            .filter(|set| set.owner_user_id() == owner_user_id)
            .filter(|set| after.is_none_or(|after| set.id() > after))
            .cloned()
            .collect();
        sets.sort_by_key(BackupSet::id);
        let has_more = sets.len() > limit as usize;
        sets.truncate(limit as usize);
        Ok((sets, has_more))
    }

    async fn get_backup_set(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSet, BackupError> {
        self.record_read();
        self.sets
            .iter()
            .find(|set| set.owner_user_id() == owner_user_id && set.id() == backup_set_id)
            .cloned()
            .ok_or(BackupError::NotFound)
    }

    async fn list_backup_snapshots(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        state: Option<SnapshotState>,
        after: Option<BackupSnapshotPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshot>, bool), BackupError> {
        self.record_read();
        self.scope_set(owner_user_id, backup_set_id)?;
        let mut snapshots: Vec<_> = self
            .snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.owner_user_id() == owner_user_id
                    && snapshot.backup_set_id() == backup_set_id
                    && state.is_none_or(|state| snapshot.state() == state)
            })
            .filter(|snapshot| {
                after.is_none_or(|after| {
                    let sort_at = snapshot.committed_at().unwrap_or(snapshot.created_at());
                    sort_at < after.sort_at()
                        || (sort_at == after.sort_at() && snapshot.id() < after.snapshot_id())
                })
            })
            .cloned()
            .collect();
        snapshots.sort_by(|left, right| {
            let left_at = left.committed_at().unwrap_or(left.created_at());
            let right_at = right.committed_at().unwrap_or(right.created_at());
            right_at
                .cmp(&left_at)
                .then_with(|| right.id().cmp(&left.id()))
        });
        let has_more = snapshots.len() > limit as usize;
        snapshots.truncate(limit as usize);
        Ok((snapshots, has_more))
    }

    async fn get_backup_snapshot(
        &self,
        owner_user_id: UserId,
        snapshot_id: SnapshotId,
    ) -> Result<BackupSnapshot, BackupError> {
        self.record_read();
        self.snapshots
            .iter()
            .find(|snapshot| {
                snapshot.owner_user_id() == owner_user_id && snapshot.id() == snapshot_id
            })
            .cloned()
            .ok_or(BackupError::NotFound)
    }

    async fn list_backup_snapshot_nodes(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        parent_node_id: Option<NodeId>,
        after: Option<NodeId>,
        limit: u32,
    ) -> Result<(Vec<BackupSnapshotNode>, bool), BackupError> {
        self.record_read();
        self.scope_set(owner_user_id, backup_set_id)?;
        if !self.snapshots.iter().any(|snapshot| {
            snapshot.owner_user_id() == owner_user_id
                && snapshot.backup_set_id() == backup_set_id
                && snapshot.id() == snapshot_id
        }) {
            return Err(BackupError::NotFound);
        }
        let mut nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| node.parent_node_id() == parent_node_id)
            .filter(|node| after.is_none_or(|after| node.node_id() > after))
            .cloned()
            .collect();
        nodes.sort_by_key(BackupSnapshotNode::node_id);
        let has_more = nodes.len() > limit as usize;
        nodes.truncate(limit as usize);
        Ok((nodes, has_more))
    }

    async fn get_current_snapshot_retention_policy(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSnapshotRetentionPolicyRevision, BackupError> {
        self.record_read();
        self.scope_set(owner_user_id, backup_set_id)?;
        self.current_policy
            .lock()
            .expect("test policy lock")
            .clone()
            .filter(|policy| policy.backup_set_id() == backup_set_id)
            .ok_or(BackupError::ExpiryPreflight(
                synveil_core::BackupSnapshotExpiryPreflightIssue::RetentionPolicyNotConfigured,
            ))
    }

    async fn list_backup_maintenance_runs(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        after: Option<BackupMaintenanceRunPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupMaintenanceRun>, bool), BackupError> {
        self.record_read();
        self.scope_set(owner_user_id, backup_set_id)?;
        let mut runs: Vec<_> = self
            .runs
            .iter()
            .filter(|run| {
                run.owner_user_id() == owner_user_id && run.backup_set_id() == backup_set_id
            })
            .filter(|run| {
                after.is_none_or(|after| {
                    run.created_at() < after.created_at()
                        || (run.created_at() == after.created_at() && run.id() < after.run_id())
                })
            })
            .cloned()
            .collect();
        runs.sort_by(|left, right| {
            right
                .created_at()
                .cmp(&left.created_at())
                .then_with(|| right.id().cmp(&left.id()))
        });
        let has_more = runs.len() > limit as usize;
        runs.truncate(limit as usize);
        Ok((runs, has_more))
    }

    async fn get_backup_maintenance_run(
        &self,
        owner_user_id: UserId,
        run_id: BackupMaintenanceRunId,
    ) -> Result<BackupMaintenanceRun, BackupError> {
        self.record_read();
        self.runs
            .iter()
            .find(|run| run.owner_user_id() == owner_user_id && run.id() == run_id)
            .cloned()
            .ok_or(BackupError::NotFound)
    }

    async fn get_restore_plan(
        &self,
        _owner_user_id: UserId,
        _plan_id: BackupRestorePlanId,
    ) -> Result<BackupRestorePlan, BackupError> {
        Err(BackupError::NotFound)
    }

    async fn get_restore_execution(
        &self,
        _owner_user_id: UserId,
        _execution_id: BackupRestoreExecutionId,
    ) -> Result<BackupRestoreExecution, BackupError> {
        Err(BackupError::NotFound)
    }

    async fn get_prune_plan(
        &self,
        _owner_user_id: UserId,
        _plan_id: BackupPrunePlanId,
    ) -> Result<BackupPrunePlan, BackupError> {
        Err(BackupError::NotFound)
    }

    async fn get_prune_execution(
        &self,
        _owner_user_id: UserId,
        _execution_id: BackupPruneExecutionId,
    ) -> Result<BackupPruneExecution, BackupError> {
        Err(BackupError::NotFound)
    }

    async fn list_backup_operations(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        kind: Option<BackupOperationKind>,
        after: Option<BackupOperationPagePosition>,
        limit: u32,
    ) -> Result<(Vec<BackupOperationSummary>, bool), BackupError> {
        self.record_read();
        self.scope_set(owner_user_id, backup_set_id)?;
        if kind.is_some_and(|kind| kind != BackupOperationKind::Maintenance) {
            return Ok((Vec::new(), false));
        }
        let mut operations = self
            .runs
            .iter()
            .filter(|run| {
                run.owner_user_id() == owner_user_id && run.backup_set_id() == backup_set_id
            })
            .map(|run| BackupOperationDetail::Maintenance(run.clone()).summary())
            .collect::<Result<Vec<_>, _>>()?;
        operations.sort_by(|left, right| {
            right
                .created_at()
                .cmp(&left.created_at())
                .then_with(|| {
                    left.operation_kind()
                        .rank()
                        .cmp(&right.operation_kind().rank())
                })
                .then_with(|| {
                    right
                        .operation_id()
                        .into_uuid()
                        .cmp(&left.operation_id().into_uuid())
                })
        });
        if let Some(after) = after {
            operations.retain(|operation| {
                operation.created_at() < after.created_at()
                    || (operation.created_at() == after.created_at()
                        && operation.operation_id().into_uuid() < after.operation_id().into_uuid())
            });
        }
        let has_more = operations.len() > limit as usize;
        operations.truncate(limit as usize);
        Ok((operations, has_more))
    }

    async fn get_backup_operation(
        &self,
        owner_user_id: UserId,
        kind: BackupOperationKind,
        operation_id: BackupOperationId,
    ) -> Result<BackupOperationDetail, BackupError> {
        self.record_read();
        let BackupOperationId::Maintenance(run_id) = operation_id else {
            return Err(BackupError::NotFound);
        };
        if kind != BackupOperationKind::Maintenance {
            return Err(BackupError::NotFound);
        }
        self.runs
            .iter()
            .find(|run| run.owner_user_id() == owner_user_id && run.id() == run_id)
            .cloned()
            .map(BackupOperationDetail::Maintenance)
            .ok_or(BackupError::NotFound)
    }
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("fixture timestamp must be valid")
}

fn fixture_data(owner_user_id: UserId, foreign_owner_user_id: Option<UserId>) -> FixtureData {
    let library_id = LibraryId::new();
    let backup_set = BackupSet::new(
        BackupSetId::new(),
        owner_user_id,
        LogicalName::new("primary-backup").expect("valid backup set name"),
        library_id,
        Some(30),
        timestamp("2026-08-20T00:00:00Z"),
    )
    .expect("valid backup set");
    let secondary_set = BackupSet::new(
        BackupSetId::new(),
        owner_user_id,
        LogicalName::new("secondary-backup").expect("valid backup set name"),
        library_id,
        None,
        timestamp("2026-08-21T00:00:00Z"),
    )
    .expect("valid backup set");
    let foreign_set = foreign_owner_user_id.map(|foreign_owner_user_id| {
        BackupSet::new(
            BackupSetId::new(),
            foreign_owner_user_id,
            LogicalName::new("foreign-backup").expect("valid backup set name"),
            LibraryId::new(),
            Some(7),
            timestamp("2026-08-22T00:00:00Z"),
        )
        .expect("valid foreign backup set")
    });

    let root_id = NodeId::new();
    let docs_id = NodeId::new();
    let report_id = NodeId::new();
    let notes_id = NodeId::new();
    let node_time = timestamp("2026-08-29T00:00:00Z");
    let root = BackupSnapshotNode::new(
        root_id,
        None,
        LogicalName::new("root").expect("valid root name"),
        NodeKind::Directory,
        NodeState::Active,
        Revision::new(1),
        None,
        node_time,
        node_time,
    )
    .expect("valid root node");
    let docs = BackupSnapshotNode::new(
        docs_id,
        Some(root_id),
        LogicalName::new("docs").expect("valid directory name"),
        NodeKind::Directory,
        NodeState::Active,
        Revision::new(2),
        None,
        node_time,
        node_time,
    )
    .expect("valid directory node");
    let report = BackupSnapshotNode::new(
        report_id,
        Some(docs_id),
        LogicalName::new("report.pdf").expect("valid file name"),
        NodeKind::File,
        NodeState::Active,
        Revision::new(9),
        Some(BackupManifestContent::new(
            FileVersionId::new(),
            42,
            Sha256Digest::from_bytes([0xab; 32]),
        )),
        node_time,
        node_time,
    )
    .expect("valid report node");
    let notes = BackupSnapshotNode::new(
        notes_id,
        Some(root_id),
        LogicalName::new("notes.txt").expect("valid file name"),
        NodeKind::File,
        NodeState::Active,
        Revision::new(3),
        Some(BackupManifestContent::new(
            FileVersionId::new(),
            12,
            Sha256Digest::from_bytes([0xcd; 32]),
        )),
        node_time,
        node_time,
    )
    .expect("valid notes node");

    let latest = BackupSnapshot::new(
        SnapshotId::new(),
        backup_set.id(),
        owner_user_id,
        library_id,
        Sequence::new(10),
        Sequence::new(20),
        4,
        Some(notes_id),
        2,
        SnapshotState::Completed,
        timestamp("2026-08-29T00:00:00Z"),
        Some(timestamp("2026-08-30T00:00:00Z")),
        None,
    );
    let same_timestamp = BackupSnapshot::new(
        SnapshotId::new(),
        backup_set.id(),
        owner_user_id,
        library_id,
        Sequence::new(11),
        Sequence::new(21),
        4,
        Some(notes_id),
        2,
        SnapshotState::Completed,
        timestamp("2026-08-28T00:00:00Z"),
        Some(timestamp("2026-08-30T00:00:00Z")),
        None,
    );
    let expired = BackupSnapshot::new(
        SnapshotId::new(),
        backup_set.id(),
        owner_user_id,
        library_id,
        Sequence::new(12),
        Sequence::new(22),
        4,
        Some(notes_id),
        2,
        SnapshotState::Expired,
        timestamp("2026-08-27T00:00:00Z"),
        Some(timestamp("2026-08-27T00:01:00Z")),
        Some(timestamp("2026-08-31T00:00:00Z")),
    );
    let failed = BackupSnapshot::new(
        SnapshotId::new(),
        backup_set.id(),
        owner_user_id,
        library_id,
        Sequence::new(13),
        Sequence::new(23),
        0,
        None,
        0,
        SnapshotState::Failed,
        timestamp("2026-08-26T00:00:00Z"),
        None,
        None,
    );

    let config =
        BackupSnapshotRetentionPolicyConfig::new(3, 86_400).expect("valid retention policy config");
    let policy_request = BackupSnapshotRetentionPolicyRequest::new(backup_set.id(), config);
    let policy = BackupSnapshotRetentionPolicyRevision::new(
        BackupSnapshotRetentionPolicyRevisionId::new(),
        owner_user_id,
        backup_set.id(),
        BackupSnapshotRetentionPolicyRevisionNumber::new(1).expect("valid policy revision number"),
        "policy-op-0001".to_owned(),
        policy_request.fingerprint(),
        config,
        timestamp("2026-08-20T00:01:00Z"),
    )
    .expect("valid retention policy revision");

    let maintenance_request = BackupMaintenanceRunRequest::new(backup_set.id());
    let policy_number = policy.revision_number();
    let run_created_at = timestamp("2026-08-22T00:00:00Z");
    let run_captured_created_at = timestamp("2026-08-22T01:00:00Z");
    let run_planned_created_at = timestamp("2026-08-22T02:00:00Z");
    let run_completed_created_at = timestamp("2026-08-22T03:00:00Z");
    let run_stale_created_at = timestamp("2026-08-22T04:00:00Z");
    let run_snapshot_at = timestamp("2026-08-23T00:00:00Z");
    let run_expiry_at = timestamp("2026-08-24T00:00:00Z");
    let run_completed_at = timestamp("2026-08-25T00:00:00Z");
    let run_stale_at = timestamp("2026-08-26T00:00:00Z");
    let run = |state: BackupMaintenanceRunState,
               created_at: Timestamp,
               captured_snapshot_id: Option<SnapshotId>,
               expiry_plan_id: Option<BackupSnapshotExpiryPlanId>,
               expiry_execution_id: Option<BackupSnapshotExpiryExecutionId>,
               snapshot_captured_at: Option<Timestamp>,
               expiry_planned_at: Option<Timestamp>,
               maintenance_completed_at: Option<Timestamp>,
               stale_at: Option<Timestamp>,
               suffix: &str| {
        BackupMaintenanceRun::new(
            BackupMaintenanceRunId::new(),
            owner_user_id,
            format!("maintenance-op-{suffix}"),
            maintenance_request.fingerprint(),
            backup_set.id(),
            policy.id(),
            policy_number,
            format!("capture-op-{suffix}"),
            format!("expiry-plan-op-{suffix}"),
            state,
            captured_snapshot_id,
            expiry_plan_id,
            expiry_execution_id,
            snapshot_captured_at,
            expiry_planned_at,
            maintenance_completed_at,
            stale_at,
            created_at,
        )
        .unwrap_or_else(|error| panic!("valid maintenance run {state:?}: {error:?}"))
    };
    let runs = vec![
        run(
            BackupMaintenanceRunState::Created,
            run_created_at,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            "created",
        ),
        run(
            BackupMaintenanceRunState::SnapshotCaptured,
            run_captured_created_at,
            Some(latest.id()),
            None,
            None,
            Some(run_snapshot_at),
            None,
            None,
            None,
            "captured",
        ),
        run(
            BackupMaintenanceRunState::ExpiryPlanned,
            run_planned_created_at,
            Some(latest.id()),
            Some(BackupSnapshotExpiryPlanId::new()),
            None,
            Some(run_snapshot_at),
            Some(run_expiry_at),
            None,
            None,
            "planned",
        ),
        run(
            BackupMaintenanceRunState::Completed,
            run_completed_created_at,
            Some(latest.id()),
            Some(BackupSnapshotExpiryPlanId::new()),
            Some(BackupSnapshotExpiryExecutionId::new()),
            Some(run_snapshot_at),
            Some(run_expiry_at),
            Some(run_completed_at),
            None,
            "completed",
        ),
        run(
            BackupMaintenanceRunState::Stale,
            run_stale_created_at,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(run_stale_at),
            "stale",
        ),
    ];

    let mut sets = vec![backup_set.clone(), secondary_set.clone()];
    if let Some(foreign_set) = foreign_set {
        sets.push(foreign_set);
    }
    FixtureData {
        sets,
        backup_set,
        secondary_set,
        snapshots: vec![latest, same_timestamp, expired, failed],
        nodes: vec![root, docs, report, notes],
        policy,
        runs,
    }
}

fn test_state(
    auth_backend: Arc<TestAuthenticationBackend>,
    backup_backend: Arc<TestBackupReadBackend>,
) -> ApiState {
    state(true)
        .with_auth_backend(auth_backend)
        .with_backup_read_backend(backup_backend)
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example")
}

async fn get_json(state: &ApiState, path: &str, session: &str, csrf: &str) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(authenticated_request(Method::GET, path, session, csrf))
        .await
        .expect("backup read request must not fail");
    let status = response.status();
    (status, json_body(response).await)
}

fn assert_no_physical_identity(value: &Value) {
    const FORBIDDEN_KEYS: [&str; 12] = [
        "ObjectId",
        "ObjectReplicaId",
        "object_id",
        "objectId",
        "object_replica_id",
        "objectReplicaId",
        "storage_key",
        "storageKey",
        "backend_locator",
        "dedup_domain_id",
        "retention_pin_id",
        "gc_candidate_id",
    ];
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                assert!(
                    !FORBIDDEN_KEYS.contains(&key.as_str()),
                    "forbidden key: {key}"
                );
                assert_no_physical_identity(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_physical_identity),
        _ => {}
    }
}

#[tokio::test]
async fn backup_read_routes_require_authentication_without_csrf() {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth_backend.user_id, None);
    let backup_backend = Arc::new(TestBackupReadBackend::new(auth_backend.user_id, data));
    let api_state = test_state(auth_backend, backup_backend);
    for path in [
        "/api/v1/backups/sets",
        "/api/v1/backups/sets/00000000-0000-7000-8000-000000000000/operations",
        "/api/v1/backups/snapshots/00000000-0000-7000-8000-000000000000",
        "/api/v1/backups/maintenance-runs/00000000-0000-7000-8000-000000000000",
        "/api/v1/backups/operations/maintenance/00000000-0000-7000-8000-000000000000",
    ] {
        let response = router(api_state.clone())
            .oneshot(super::request(Method::GET, path, Body::empty()))
            .await
            .expect("unauthenticated request must not fail");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            json_body(response).await["error"]["code"],
            "authentication_failed"
        );
    }
}

#[tokio::test]
async fn backup_set_list_is_owner_scoped_and_keyset_paginated() {
    let auth_a = Arc::new(TestAuthenticationBackend::new());
    let auth_b = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth_a.user_id, Some(auth_b.user_id));
    let backup_backend = Arc::new(TestBackupReadBackend::new(auth_a.user_id, data));
    let api_state = test_state(auth_a.clone(), backup_backend.clone());
    let (session, csrf) = login_cookies(&api_state).await;

    let (status, first) =
        get_json(&api_state, "/api/v1/backups/sets?limit=1", &session, &csrf).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["data"].as_array().expect("set data").len(), 1);
    assert_eq!(first["page"]["has_more"], true);
    let first_id = first["data"][0]["backup_set_id"]
        .as_str()
        .expect("set ID")
        .to_owned();
    assert!(first["page"]["next_cursor"].as_str().is_some());
    assert_no_physical_identity(&first);

    let next = first["page"]["next_cursor"].as_str().expect("next cursor");
    let path = format!("/api/v1/backups/sets?limit=1&cursor={next}");
    let (status, second) = get_json(&api_state, &path, &session, &csrf).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["data"].as_array().expect("set data").len(), 1);
    assert_ne!(second["data"][0]["backup_set_id"], first_id);
    assert_no_physical_identity(&second);

    let api_state_b = test_state(auth_b, backup_backend);
    let (session_b, csrf_b) = login_cookies(&api_state_b).await;
    let (status, body) = get_json(&api_state_b, "/api/v1/backups/sets", &session_b, &csrf_b).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().expect("set data").len(), 1);
    assert_eq!(body["data"][0]["name"], "foreign-backup");
}

#[tokio::test]
async fn backup_set_get_conceals_foreign_and_rejects_malformed_ids() {
    let auth_a = Arc::new(TestAuthenticationBackend::new());
    let auth_b = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth_a.user_id, Some(auth_b.user_id));
    let backup_set_id = data.backup_set.id();
    let backend = Arc::new(TestBackupReadBackend::new(auth_a.user_id, data));
    let state_a = test_state(auth_a.clone(), backend.clone());
    let (session_a, csrf_a) = login_cookies(&state_a).await;
    let path = format!("/api/v1/backups/sets/{backup_set_id}");
    let (status, body) = get_json(&state_a, &path, &session_a, &csrf_a).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["backup_set_id"], backup_set_id.to_string());

    let state_b = test_state(auth_b, backend);
    let (session_b, csrf_b) = login_cookies(&state_b).await;
    let (status, body) = get_json(&state_b, &path, &session_b, &csrf_b).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    let (status, body) = get_json(
        &state_a,
        "/api/v1/backups/sets/not-a-backup-set-id",
        &session_a,
        &csrf_a,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn snapshot_list_filters_orders_and_paginates_equal_timestamps() {
    let auth = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth.user_id, None);
    let set_id = data.backup_set.id();
    let backend = Arc::new(TestBackupReadBackend::new(auth.user_id, data));
    let api_state = test_state(auth, backend);
    let (session, csrf) = login_cookies(&api_state).await;

    let mut cursor = None;
    let mut ids = Vec::new();
    loop {
        let path = cursor.as_ref().map_or_else(
            || format!("/api/v1/backups/sets/{set_id}/snapshots?limit=1"),
            |cursor| format!("/api/v1/backups/sets/{set_id}/snapshots?limit=1&cursor={cursor}"),
        );
        let (status, body) = get_json(&api_state, &path, &session, &csrf).await;
        assert_eq!(status, StatusCode::OK);
        let page = body["data"].as_array().expect("snapshot data");
        if page.is_empty() {
            break;
        }
        ids.push(
            page[0]["snapshot_id"]
                .as_str()
                .expect("snapshot ID")
                .to_owned(),
        );
        assert_no_physical_identity(&body);
        if !body["page"]["has_more"].as_bool().expect("has_more") {
            break;
        }
        cursor = Some(
            body["page"]["next_cursor"]
                .as_str()
                .expect("next cursor")
                .to_owned(),
        );
    }
    assert_eq!(ids.len(), 4);
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());

    let (status, completed) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/snapshots?state=COMPLETED"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        completed["data"].as_array().expect("snapshot data").len(),
        2
    );
    assert!(
        completed["data"]
            .as_array()
            .expect("snapshot data")
            .iter()
            .all(|snapshot| snapshot["state"] == "COMPLETED")
    );

    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/snapshots?state=TOTALLY_INVALID"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/snapshots?limit=0"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_limit");
    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/snapshots?limit=1001"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_limit");

    let (status, _) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/snapshots?limit=1000"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get_json(
        &api_state,
        "/api/v1/backups/snapshots/not-a-snapshot-id",
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/snapshots?cursor=bad"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_cursor");
}

#[tokio::test]
async fn snapshot_get_tree_and_content_metadata_keep_expired_history_readable() {
    let auth_a = Arc::new(TestAuthenticationBackend::new());
    let auth_b = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth_a.user_id, Some(auth_b.user_id));
    let latest = data.snapshots[0].clone();
    let expired = data.snapshots[2].clone();
    let root_id = data.nodes[0].node_id();
    let docs_id = data.nodes[1].node_id();
    let backend = Arc::new(TestBackupReadBackend::new(auth_a.user_id, data));
    let state_a = test_state(auth_a.clone(), backend.clone());
    let (session_a, csrf_a) = login_cookies(&state_a).await;

    for snapshot in [&latest, &expired] {
        let path = format!("/api/v1/backups/snapshots/{}", snapshot.id());
        let (status, body) = get_json(&state_a, &path, &session_a, &csrf_a).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["state"], snapshot.state().as_str());
        assert_no_physical_identity(&body);
    }

    let (status, root_page) = get_json(
        &state_a,
        &format!("/api/v1/backups/snapshots/{}/nodes?limit=1", latest.id()),
        &session_a,
        &csrf_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(root_page["data"].as_array().expect("root data").len(), 1);
    assert_eq!(
        root_page["data"][0]["snapshot_node_id"],
        root_id.to_string()
    );
    assert_eq!(root_page["data"][0]["kind"], "DIRECTORY");
    assert!(
        root_page["data"][0]
            .get("parent_snapshot_node_id")
            .is_none()
    );

    let (status, root_children) = get_json(
        &state_a,
        &format!(
            "/api/v1/backups/snapshots/{}/nodes?parent_id={root_id}&limit=1",
            latest.id()
        ),
        &session_a,
        &csrf_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        root_children["page"]["has_more"]
            .as_bool()
            .expect("has_more")
    );
    assert_no_physical_identity(&root_children);
    let cursor = root_children["page"]["next_cursor"]
        .as_str()
        .expect("root child cursor");
    let (status, second_child_page) = get_json(
        &state_a,
        &format!(
            "/api/v1/backups/snapshots/{}/nodes?parent_id={root_id}&limit=1&cursor={cursor}",
            latest.id()
        ),
        &session_a,
        &csrf_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(
        root_children["data"][0]["snapshot_node_id"],
        second_child_page["data"][0]["snapshot_node_id"]
    );

    let (status, report_page) = get_json(
        &state_a,
        &format!(
            "/api/v1/backups/snapshots/{}/nodes?parent_id={docs_id}",
            latest.id()
        ),
        &session_a,
        &csrf_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let report = &report_page["data"][0];
    assert_eq!(report["name"], "report.pdf");
    assert_eq!(report["file_version_id"].as_str().map(str::len), Some(36));
    assert_eq!(report["byte_length"], "42");
    assert_eq!(report["sha256"].as_str().map(|value| value.len()), Some(71));
    assert_no_physical_identity(&report_page);

    let state_b = test_state(auth_b, backend);
    let (session_b, csrf_b) = login_cookies(&state_b).await;
    for path in [
        format!("/api/v1/backups/snapshots/{}", latest.id()),
        format!("/api/v1/backups/snapshots/{}/nodes", latest.id()),
        format!(
            "/api/v1/backups/snapshots/{}/nodes?parent_id={root_id}",
            latest.id()
        ),
    ] {
        let (status, body) = get_json(&state_b, &path, &session_b, &csrf_b).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }
}

#[tokio::test]
async fn restore_routes_require_authentication() {
    let auth = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth.user_id, None);
    let backup_backend = Arc::new(TestBackupReadBackend::new(auth.user_id, data));
    let api_state = test_state(auth, backup_backend);
    let snapshot_id = "00000000-0000-7000-8000-000000000000";
    let plan_id = "00000000-0000-7000-8000-000000000001";
    let exec_id = "00000000-0000-7000-8000-000000000002";

    // POST create-plan requires auth
    let response = router(api_state.clone())
        .oneshot(super::json_request(
            Method::POST,
            &format!("/api/v1/backups/snapshots/{snapshot_id}/restore-plans"),
            r#"{"target_library_id":"00000000-0000-4000-8000-000000000000","target_parent_node_id":"00000000-0000-8000-000000000000","destination_name":"Recovered"}"#,
        ))
        .await
        .expect("request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "private, no-store");

    // GET plan requires auth
    let response = router(api_state.clone())
        .oneshot(super::request(
            Method::GET,
            &format!("/api/v1/backups/restore-plans/{plan_id}"),
            Body::empty(),
        ))
        .await
        .expect("request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "private, no-store");

    // POST execute requires auth
    let response = router(api_state.clone())
        .oneshot(super::request(
            Method::POST,
            &format!("/api/v1/backups/restore-plans/{plan_id}/execute"),
            Body::empty(),
        ))
        .await
        .expect("request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "private, no-store");

    // GET execution requires auth
    let response = router(api_state.clone())
        .oneshot(super::request(
            Method::GET,
            &format!("/api/v1/backups/restore-executions/{exec_id}"),
            Body::empty(),
        ))
        .await
        .expect("request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
}

#[tokio::test]
async fn restore_plan_get_conceals_foreign_and_rejects_malformed_ids() {
    let auth_a = Arc::new(TestAuthenticationBackend::new());
    let auth_b = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth_a.user_id, Some(auth_b.user_id));
    let backup_backend = Arc::new(TestBackupReadBackend::new(auth_a.user_id, data));
    let api_state = test_state(auth_a.clone(), backup_backend);
    let (session, csrf) = login_cookies(&api_state).await;

    // GET restore-plan with malformed ID -> 400
    let response = router(api_state.clone())
        .oneshot(super::authenticated_request(
            Method::GET,
            "/api/v1/backups/restore-plans/not-a-uuid",
            &session,
            &csrf,
        ))
        .await
        .expect("request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // GET restore-execution with malformed ID -> 400
    let response = router(api_state.clone())
        .oneshot(super::authenticated_request(
            Method::GET,
            "/api/v1/backups/restore-executions/not-a-uuid",
            &session,
            &csrf,
        ))
        .await
        .expect("request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn retention_policy_reads_current_revision_and_conceals_no_policy() {
    let auth = Arc::new(TestAuthenticationBackend::new());
    let owner_user_id = auth.user_id;
    let data = fixture_data(auth.user_id, None);
    let set_id = data.backup_set.id();
    let secondary_set_id = data.secondary_set.id();
    let backend = Arc::new(TestBackupReadBackend::new(auth.user_id, data));
    let api_state = test_state(auth, backend.clone());
    let (session, csrf) = login_cookies(&api_state).await;

    let path = format!("/api/v1/backups/sets/{set_id}/retention-policy");
    let (status, body) = get_json(&api_state, &path, &session, &csrf).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["revision_number"], "1");
    assert_eq!(body["data"]["keep_latest_completed"], "3");
    assert_eq!(body["data"]["expire_after_seconds"], "86400");
    assert_no_physical_identity(&body);

    let newer_config =
        BackupSnapshotRetentionPolicyConfig::new(5, 172_800).expect("valid newer policy config");
    let newer_request = BackupSnapshotRetentionPolicyRequest::new(set_id, newer_config);
    let newer_policy = BackupSnapshotRetentionPolicyRevision::new(
        BackupSnapshotRetentionPolicyRevisionId::new(),
        owner_user_id,
        set_id,
        BackupSnapshotRetentionPolicyRevisionNumber::new(2)
            .expect("valid newer policy revision number"),
        "policy-op-0002".to_owned(),
        newer_request.fingerprint(),
        newer_config,
        timestamp("2026-08-31T00:00:00Z"),
    )
    .expect("valid newer policy revision");
    *backend.current_policy.lock().expect("test policy lock") = Some(newer_policy);
    let (status, body) = get_json(&api_state, &path, &session, &csrf).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["revision_number"], "2");
    assert_eq!(body["data"]["keep_latest_completed"], "5");

    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{secondary_set_id}/retention-policy"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn maintenance_history_is_paginated_and_get_is_observational() {
    let auth = Arc::new(TestAuthenticationBackend::new());
    let auth_b = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth.user_id, Some(auth_b.user_id));
    let set_id = data.backup_set.id();
    let run_ids: Vec<_> = data.runs.iter().map(BackupMaintenanceRun::id).collect();
    let backend = Arc::new(TestBackupReadBackend::new(auth.user_id, data));
    let before_runs = backend.runs.clone();
    let api_state = test_state(auth, backend.clone());
    let (session, csrf) = login_cookies(&api_state).await;

    let (status, first) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/maintenance-runs?limit=2"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["data"].as_array().expect("run data").len(), 2);
    assert!(first["page"]["has_more"].as_bool().expect("has_more"));
    assert_no_physical_identity(&first);
    let cursor = first["page"]["next_cursor"]
        .as_str()
        .expect("maintenance cursor");
    let (status, second) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/maintenance-runs?limit=2&cursor={cursor}"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["data"].as_array().expect("run data").len(), 2);

    let created_id = run_ids[0];
    let snapshot_captured_id = run_ids[1];
    let expiry_planned_id = run_ids[2];
    let completed_id = run_ids[3];
    let stale_id = run_ids[4];
    for (run_id, expected_state) in [
        (created_id, "CREATED"),
        (snapshot_captured_id, "SNAPSHOT_CAPTURED"),
        (expiry_planned_id, "EXPIRY_PLANNED"),
        (completed_id, "COMPLETED"),
        (stale_id, "STALE"),
    ] {
        let (status, body) = get_json(
            &api_state,
            &format!("/api/v1/backups/maintenance-runs/{run_id}"),
            &session,
            &csrf,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["state"], expected_state);
        assert_no_physical_identity(&body);
    }
    assert_eq!(backend.runs, before_runs);

    let (status, body) = get_json(
        &api_state,
        "/api/v1/backups/maintenance-runs/not-a-run-id",
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/maintenance-runs?cursor=bad"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_cursor");
}

#[tokio::test]
async fn backup_operation_feed_is_bounded_scoped_and_truthful_for_maintenance() {
    let auth_a = Arc::new(TestAuthenticationBackend::new());
    let auth_b = Arc::new(TestAuthenticationBackend::new());
    let data = fixture_data(auth_a.user_id, Some(auth_b.user_id));
    let set_id = data.backup_set.id();
    let secondary_set_id = data.secondary_set.id();
    let run_ids: Vec<_> = data.runs.iter().map(BackupMaintenanceRun::id).collect();
    let backend = Arc::new(TestBackupReadBackend::new(auth_a.user_id, data));
    let api_state = test_state(auth_a.clone(), backend.clone());
    let (session, csrf) = login_cookies(&api_state).await;

    let mut cursor = None;
    let mut first_cursor = None;
    let mut seen = Vec::new();
    loop {
        let path = cursor.as_ref().map_or_else(
            || format!("/api/v1/backups/sets/{set_id}/operations?limit=2"),
            |cursor| format!("/api/v1/backups/sets/{set_id}/operations?limit=2&cursor={cursor}"),
        );
        let (status, body) = get_json(&api_state, &path, &session, &csrf).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["data"].as_array().expect("operation data").len() <= 2);
        assert!(!body["data"].as_array().expect("operation data").is_empty());
        assert_eq!(body["data"][0]["operation_kind"], "MAINTENANCE");
        assert!(body["data"][0].get("progress_percent").is_none());
        assert!(body["data"][0].get("eta").is_none());
        assert_no_physical_identity(&body);
        seen.extend(
            body["data"]
                .as_array()
                .expect("operation data")
                .iter()
                .map(|operation| operation["operation_id"].as_str().unwrap().to_owned()),
        );
        if !body["page"]["has_more"].as_bool().expect("has_more") {
            break;
        }
        let next_cursor = body["page"]["next_cursor"]
            .as_str()
            .expect("operation cursor")
            .to_owned();
        if first_cursor.is_none() {
            first_cursor = Some(next_cursor.clone());
        }
        cursor = Some(next_cursor);
    }
    assert_eq!(seen.len(), run_ids.len());
    assert_eq!(
        seen.iter().collect::<std::collections::HashSet<_>>().len(),
        seen.len()
    );

    let expected = [
        ("CREATED", "AWAITING_ADVANCE", 0, 3, false, "ADVANCE"),
        (
            "SNAPSHOT_CAPTURED",
            "SNAPSHOT_CAPTURED",
            1,
            3,
            false,
            "ADVANCE",
        ),
        ("EXPIRY_PLANNED", "EXPIRY_PLANNED", 2, 3, false, "ADVANCE"),
        ("COMPLETED", "COMPLETED", 3, 3, true, "NONE"),
        ("STALE", "STALE", 0, 3, true, "CREATE_NEW_RUN"),
    ];
    for (run_id, (state, phase, completed, total, terminal, next_action)) in
        run_ids.iter().zip(expected)
    {
        let (status, body) = get_json(
            &api_state,
            &format!("/api/v1/backups/operations/maintenance/{run_id}"),
            &session,
            &csrf,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["operation_kind"], "MAINTENANCE");
        assert_eq!(body["data"]["state"], state);
        assert_eq!(body["data"]["phase"], phase);
        assert_eq!(body["data"]["progress"]["completed_steps"], completed);
        assert_eq!(body["data"]["progress"]["total_steps"], total);
        assert_eq!(body["data"]["terminal"], terminal);
        assert_eq!(body["data"]["next_action"], next_action);
        assert_eq!(body["data"]["operation_id"], run_id.to_string());
        assert_no_physical_identity(&body);
    }

    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/sets/{set_id}/operations?kind=RESTORE"),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].as_array().expect("filtered data").is_empty());

    for (path, code) in [
        (
            format!("/api/v1/backups/sets/{set_id}/operations?kind=UNKNOWN"),
            "invalid_request",
        ),
        (
            format!("/api/v1/backups/sets/{set_id}/operations?limit=0"),
            "invalid_limit",
        ),
        (
            format!("/api/v1/backups/sets/{set_id}/operations?cursor=bad"),
            "invalid_cursor",
        ),
        (
            format!(
                "/api/v1/backups/sets/{secondary_set_id}/operations?cursor={}",
                first_cursor.as_deref().expect("first operation cursor")
            ),
            "invalid_cursor",
        ),
    ] {
        let (status, body) = get_json(&api_state, &path, &session, &csrf).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], code);
    }

    let foreign_state = test_state(auth_b, backend.clone());
    let (foreign_session, foreign_csrf) = login_cookies(&foreign_state).await;
    let (status, body) = get_json(
        &foreign_state,
        &format!("/api/v1/backups/sets/{set_id}/operations"),
        &foreign_session,
        &foreign_csrf,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    let (status, body) = get_json(
        &api_state,
        &format!("/api/v1/backups/operations/restore/{}", run_ids[0]),
        &session,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}
