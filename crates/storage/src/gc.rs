//! Transport-neutral physical Object garbage-collection execution service.
//!
//! The service coordinates the durable PostgreSQL execution/action boundary
//! with one or more backend-neutral `ObjectStore` adapters. It never accepts a
//! user path or public storage key, never buffers object bytes, and performs at
//! most one fenced replica deletion per call.

use std::{collections::BTreeMap, fmt, sync::Arc};

use synveil_core::{
    GcWorkerRetryPolicy, ObjectGcOperationId, ObjectGcPolicy, ObjectReplicaId, Timestamp,
};
use synveil_metadata::{
    DatabasePool, ObjectGcExecutionMetadataBackend, ObjectGcExecutionMetadataError,
    ObjectGcExecutionState, ObjectGcLease, ObjectGcOperation, ObjectGcReplicaAction,
    ObjectGcReplicaDirective, ObjectGcReplicaObservation, PostgresObjectGcExecutionRepository,
};
use synveil_object_store::{
    CapabilitySupport, DeleteOutcome, DeleteReconciliation, ObjectKey, ObjectMetadata, ObjectStore,
    ObjectStoreError, ObjectVersion, StorageAvailability, StorageBackendKind, StorageCapability,
};

/// Safe result of one bounded execution call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcStepOutcome {
    ReplicaDeleted,
    ReplicaAlreadyAbsent,
    ReplicaReconciledAbsent,
    ReplicaStillPresent,
    ReplicaDeleteInProgress,
    ReconciliationRequired,
    EvidenceMismatch,
    Deferred,
    NeedsAttention,
    ReadyToComplete,
    Completed,
}

/// Operation/lease projection returned after one bounded call. The renewed
/// lease must replace the caller's prior copy before a later action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectGcExecutionStep {
    operation: ObjectGcOperation,
    lease: ObjectGcLease,
    replica_id: Option<ObjectReplicaId>,
    outcome: ObjectGcStepOutcome,
}

impl ObjectGcExecutionStep {
    #[must_use]
    pub const fn operation(self) -> ObjectGcOperation {
        self.operation
    }

    #[must_use]
    pub const fn lease(self) -> ObjectGcLease {
        self.lease
    }

    #[must_use]
    pub const fn replica_id(self) -> Option<ObjectReplicaId> {
        self.replica_id
    }

    #[must_use]
    pub const fn outcome(self) -> ObjectGcStepOutcome {
        self.outcome
    }
}

/// Closed configuration failures for the internal service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcExecutionConfigurationError {
    NoObjectStores,
    UnknownBackend,
    DuplicateBackendKind,
}

impl fmt::Display for ObjectGcExecutionConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NoObjectStores => "physical GC requires at least one object store",
            Self::UnknownBackend => "physical GC object store backend is unknown",
            Self::DuplicateBackendKind => "physical GC backend mapping is ambiguous",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ObjectGcExecutionConfigurationError {}

/// Stable application failures. Values contain no SQL, credentials, host
/// paths, storage keys, backend versions, or checksums.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcExecutionError {
    CandidateNotFound,
    ReadyRequired,
    StaleLease,
    LeaseExpired,
    ReferenceExists,
    HoldExists,
    OperationNotFound,
    InvalidState,
    BackendUnavailable,
    EvidenceMismatch,
    ReconciliationRequired,
    NeedsAttention,
    DatabaseUnavailable,
    InvalidPersistedData,
}

impl fmt::Display for ObjectGcExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::CandidateNotFound => "physical GC candidate was not found",
            Self::ReadyRequired => "physical GC candidate is not ready",
            Self::StaleLease => "physical GC lease is stale",
            Self::LeaseExpired => "physical GC lease has expired",
            Self::ReferenceExists => "physical GC was cancelled by a committed reference",
            Self::HoldExists => "physical GC was blocked by an active hold",
            Self::OperationNotFound => "physical GC operation was not found",
            Self::InvalidState => "physical GC resource is in an invalid state",
            Self::BackendUnavailable => "physical GC backend is unavailable",
            Self::EvidenceMismatch => "physical GC replica evidence does not match",
            Self::ReconciliationRequired => "physical GC replica requires reconciliation",
            Self::NeedsAttention => "physical GC operation needs operator attention",
            Self::DatabaseUnavailable => "physical GC metadata is unavailable",
            Self::InvalidPersistedData => "physical GC persisted metadata is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ObjectGcExecutionError {}

/// Internal physical-GC application service. Backend routing is closed over
/// the configured adapter kinds; one kind cannot silently map to two stores.
pub struct ObjectGcExecutionService {
    metadata: Arc<dyn ObjectGcExecutionMetadataBackend>,
    object_stores: BTreeMap<&'static str, Arc<dyn ObjectStore>>,
}

impl ObjectGcExecutionService {
    /// Construct the production PostgreSQL service with one ObjectStore.
    pub fn new(
        pool: DatabasePool,
        policy: ObjectGcPolicy,
        object_store: Arc<dyn ObjectStore>,
    ) -> Result<Self, ObjectGcExecutionConfigurationError> {
        Self::with_metadata_backend(
            Arc::new(PostgresObjectGcExecutionRepository::new(pool, policy)),
            vec![object_store],
        )
    }

    /// Construct a service with a closed set of configured backend adapters.
    pub fn with_object_stores(
        pool: DatabasePool,
        policy: ObjectGcPolicy,
        object_stores: Vec<Arc<dyn ObjectStore>>,
    ) -> Result<Self, ObjectGcExecutionConfigurationError> {
        Self::with_metadata_backend(
            Arc::new(PostgresObjectGcExecutionRepository::new(pool, policy)),
            object_stores,
        )
    }

    /// Test/composition constructor for a reviewed metadata adapter.
    pub fn with_metadata_backend(
        metadata: Arc<dyn ObjectGcExecutionMetadataBackend>,
        object_stores: Vec<Arc<dyn ObjectStore>>,
    ) -> Result<Self, ObjectGcExecutionConfigurationError> {
        if object_stores.is_empty() {
            return Err(ObjectGcExecutionConfigurationError::NoObjectStores);
        }
        let mut mapped = BTreeMap::new();
        for object_store in object_stores {
            let backend_kind = backend_kind(object_store.capabilities().backend())
                .ok_or(ObjectGcExecutionConfigurationError::UnknownBackend)?;
            if mapped.insert(backend_kind, object_store).is_some() {
                return Err(ObjectGcExecutionConfigurationError::DuplicateBackendKind);
            }
        }
        Ok(Self {
            metadata,
            object_stores: mapped,
        })
    }

    /// Create the durable operation and per-replica action rows before any
    /// external ObjectStore side effect.
    pub async fn start_gc_execution(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionError> {
        self.metadata
            .start_gc_execution(lease)
            .await
            .map_err(map_metadata_error)
    }

    /// Rebind an incomplete operation to a newly reclaimed READY lease after
    /// revalidating the canonical candidate/object/reference/hold boundary.
    pub async fn resume_gc_execution(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionError> {
        self.metadata
            .resume_gc_execution(lease)
            .await
            .map_err(map_metadata_error)
    }

    /// Delete or reconcile exactly one deterministic replica. The lease is
    /// renewed first, and the metadata port repeats final reference/hold/
    /// generation validation in the short action-fence transaction.
    pub async fn delete_next_replica(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        self.step(operation_id, lease, true, None).await
    }

    /// Worker-specific bounded step that persists retry scheduling with the
    /// PostgreSQL time authority. The established Prompt 28 method above
    /// preserves its direct/replay behavior for existing callers.
    pub async fn delete_next_replica_with_retry_policy(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
        retry_policy: GcWorkerRetryPolicy,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        self.step(operation_id, lease, true, Some(retry_policy))
            .await
    }

    /// Reconcile exactly one previously fenced action without initiating a new
    /// delete. If the key is still present, the action becomes retryable.
    pub async fn reconcile_replica(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        self.step(operation_id, lease, false, None).await
    }

    /// Worker-specific reconciliation step with durable retry scheduling.
    pub async fn reconcile_replica_with_retry_policy(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
        retry_policy: GcWorkerRetryPolicy,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        self.step(operation_id, lease, false, Some(retry_policy))
            .await
    }

    /// Final short transaction: revalidate again, require every action and
    /// physical replica to be absent, remove candidate/Object metadata, and
    /// mark the durable operation completed.
    pub async fn complete_gc_execution(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        let existing = self
            .metadata
            .load_gc_execution(operation_id)
            .await
            .map_err(map_metadata_error)?;
        if existing.state() == ObjectGcExecutionState::Completed {
            return Ok(ObjectGcExecutionStep {
                operation: existing,
                lease,
                replica_id: None,
                outcome: ObjectGcStepOutcome::Completed,
            });
        }
        let renewed = self
            .metadata
            .renew_execution_lease(lease)
            .await
            .map_err(map_metadata_error)?;
        let operation = self
            .metadata
            .complete_gc_execution(operation_id, renewed)
            .await
            .map_err(map_metadata_error)?;
        Ok(ObjectGcExecutionStep {
            operation,
            lease: renewed,
            replica_id: None,
            outcome: ObjectGcStepOutcome::Completed,
        })
    }

    pub async fn load_gc_execution(
        &self,
        operation_id: ObjectGcOperationId,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionError> {
        self.metadata
            .load_gc_execution(operation_id)
            .await
            .map_err(map_metadata_error)
    }

    /// Persist a terminal intervention state for a proven permanent worker
    /// composition failure. The call cannot touch storage and remains fenced
    /// by the current candidate lease and generation.
    pub async fn mark_gc_execution_needs_attention(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
        error_code: &'static str,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionError> {
        self.metadata
            .mark_gc_execution_needs_attention(operation_id, lease, error_code)
            .await
            .map_err(map_metadata_error)
    }

    async fn step(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
        allow_delete: bool,
        retry_policy: Option<GcWorkerRetryPolicy>,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        let existing = self
            .metadata
            .load_gc_execution(operation_id)
            .await
            .map_err(map_metadata_error)?;
        if existing.state() == ObjectGcExecutionState::Completed {
            return Ok(ObjectGcExecutionStep {
                operation: existing,
                lease,
                replica_id: None,
                outcome: ObjectGcStepOutcome::Completed,
            });
        }
        let renewed = self
            .metadata
            .renew_execution_lease(lease)
            .await
            .map_err(map_metadata_error)?;
        match self
            .metadata
            .fence_next_replica(operation_id, renewed)
            .await
            .map_err(map_metadata_error)?
        {
            ObjectGcReplicaDirective::NoRemaining(operation) => {
                let outcome = if operation.state() == ObjectGcExecutionState::Completed {
                    ObjectGcStepOutcome::Completed
                } else {
                    ObjectGcStepOutcome::ReadyToComplete
                };
                Ok(ObjectGcExecutionStep {
                    operation,
                    lease: renewed,
                    replica_id: None,
                    outcome,
                })
            }
            ObjectGcReplicaDirective::Deferred(operation) => Ok(ObjectGcExecutionStep {
                operation,
                lease: renewed,
                replica_id: None,
                outcome: ObjectGcStepOutcome::Deferred,
            }),
            ObjectGcReplicaDirective::Reconcile(action) => {
                self.inspect_and_persist(action, renewed, None, retry_policy)
                    .await
            }
            ObjectGcReplicaDirective::Delete(action) if !allow_delete => {
                self.inspect_and_persist(action, renewed, None, retry_policy)
                    .await
            }
            ObjectGcReplicaDirective::Delete(action) => {
                self.delete_and_persist(action, renewed, retry_policy).await
            }
        }
    }

    async fn delete_and_persist(
        &self,
        action: ObjectGcReplicaAction,
        lease: ObjectGcLease,
        retry_policy: Option<GcWorkerRetryPolicy>,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        let store = self.store_for(&action)?;
        let capabilities = store.capabilities();
        if capabilities.availability() != StorageAvailability::Available {
            return self
                .persist_observation(
                    action,
                    lease,
                    ObjectGcReplicaObservation::Unknown,
                    retry_policy,
                )
                .await;
        }
        let key = match ObjectKey::new(action.storage_key().to_owned()) {
            Ok(key) => key,
            Err(_) => {
                return self
                    .persist_observation(
                        action,
                        lease,
                        ObjectGcReplicaObservation::Mismatch,
                        retry_policy,
                    )
                    .await;
            }
        };

        let before = match store.reconcile_delete(&key).await {
            Ok(value) => value,
            Err(_) => {
                return self
                    .persist_observation(
                        action,
                        lease,
                        ObjectGcReplicaObservation::Unknown,
                        retry_policy,
                    )
                    .await;
            }
        };
        let delete_in_progress = matches!(&before, DeleteReconciliation::InProgress(_));
        match before {
            DeleteReconciliation::Absent => {
                return self
                    .persist_observation(
                        action,
                        lease,
                        ObjectGcReplicaObservation::AlreadyAbsent,
                        retry_policy,
                    )
                    .await;
            }
            DeleteReconciliation::Present(metadata) => {
                if !metadata_matches(&action, &metadata) {
                    return self
                        .persist_observation(
                            action,
                            lease,
                            ObjectGcReplicaObservation::Mismatch,
                            retry_policy,
                        )
                        .await;
                }
            }
            DeleteReconciliation::InProgress(Some(metadata)) => {
                if !metadata_matches(&action, &metadata) {
                    return self
                        .persist_observation(
                            action,
                            lease,
                            ObjectGcReplicaObservation::Mismatch,
                            retry_policy,
                        )
                        .await;
                }
            }
            DeleteReconciliation::InProgress(None) => {}
        }

        // Head/reconciliation can be a slow provider call. Renew, then repeat
        // the exact PostgreSQL action-generation and reference/hold proof as
        // the last step before invoking the external mutation.
        let deletion_lease = self
            .metadata
            .renew_execution_lease(lease)
            .await
            .map_err(map_metadata_error)?;
        self.metadata
            .authorize_replica_delete(&action, deletion_lease)
            .await
            .map_err(map_metadata_error)?;

        if Timestamp::now() >= deletion_lease.lease_expires_at() {
            let observation = if delete_in_progress {
                ObjectGcReplicaObservation::DeleteInProgress
            } else {
                ObjectGcReplicaObservation::StillPresent
            };
            return self
                .persist_observation(action, deletion_lease, observation, retry_policy)
                .await;
        }

        let delete_result = match capabilities.support(StorageCapability::ConditionalDelete) {
            CapabilitySupport::Supported => {
                let Some(version) = action.expected_backend_version() else {
                    return self
                        .persist_observation(
                            action,
                            lease,
                            ObjectGcReplicaObservation::Mismatch,
                            retry_policy,
                        )
                        .await;
                };
                let Ok(version) = ObjectVersion::new(version.to_owned()) else {
                    return self
                        .persist_observation(
                            action,
                            lease,
                            ObjectGcReplicaObservation::Mismatch,
                            retry_policy,
                        )
                        .await;
                };
                store.conditional_delete(&key, &version).await
            }
            CapabilitySupport::Unsupported => store.delete(&key).await,
            CapabilitySupport::Unknown => {
                return self
                    .persist_observation(
                        action,
                        lease,
                        ObjectGcReplicaObservation::Unknown,
                        retry_policy,
                    )
                    .await;
            }
        };
        self.inspect_and_persist(action, deletion_lease, Some(delete_result), retry_policy)
            .await
    }

    async fn inspect_and_persist(
        &self,
        action: ObjectGcReplicaAction,
        lease: ObjectGcLease,
        delete_result: Option<Result<DeleteOutcome, ObjectStoreError>>,
        retry_policy: Option<GcWorkerRetryPolicy>,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        let store = self.store_for(&action)?;
        let key = match ObjectKey::new(action.storage_key().to_owned()) {
            Ok(key) => key,
            Err(_) => {
                return self
                    .persist_observation(
                        action,
                        lease,
                        ObjectGcReplicaObservation::Mismatch,
                        retry_policy,
                    )
                    .await;
            }
        };
        let observation = match store.reconcile_delete(&key).await {
            Ok(DeleteReconciliation::Absent) => match delete_result {
                Some(Ok(DeleteOutcome::Deleted)) => ObjectGcReplicaObservation::Deleted,
                Some(Ok(DeleteOutcome::AlreadyAbsent)) => ObjectGcReplicaObservation::AlreadyAbsent,
                Some(Err(_)) | None => ObjectGcReplicaObservation::ReconciledAbsent,
            },
            Ok(DeleteReconciliation::Present(metadata)) => {
                if metadata_matches(&action, &metadata) {
                    ObjectGcReplicaObservation::StillPresent
                } else {
                    ObjectGcReplicaObservation::Mismatch
                }
            }
            Ok(DeleteReconciliation::InProgress(_)) => ObjectGcReplicaObservation::DeleteInProgress,
            Err(_) => ObjectGcReplicaObservation::Unknown,
        };
        self.persist_observation(action, lease, observation, retry_policy)
            .await
    }

    async fn persist_observation(
        &self,
        action: ObjectGcReplicaAction,
        lease: ObjectGcLease,
        observation: ObjectGcReplicaObservation,
        retry_policy: Option<GcWorkerRetryPolicy>,
    ) -> Result<ObjectGcExecutionStep, ObjectGcExecutionError> {
        let operation = self
            .metadata
            .record_replica_observation(&action, lease, observation, retry_policy)
            .await
            .map_err(map_metadata_error)?;
        let outcome = if operation.state() == ObjectGcExecutionState::NeedsAttention {
            ObjectGcStepOutcome::NeedsAttention
        } else {
            match observation {
                ObjectGcReplicaObservation::Deleted => ObjectGcStepOutcome::ReplicaDeleted,
                ObjectGcReplicaObservation::AlreadyAbsent => {
                    ObjectGcStepOutcome::ReplicaAlreadyAbsent
                }
                ObjectGcReplicaObservation::ReconciledAbsent => {
                    ObjectGcStepOutcome::ReplicaReconciledAbsent
                }
                ObjectGcReplicaObservation::StillPresent => {
                    ObjectGcStepOutcome::ReplicaStillPresent
                }
                ObjectGcReplicaObservation::DeleteInProgress => {
                    ObjectGcStepOutcome::ReplicaDeleteInProgress
                }
                ObjectGcReplicaObservation::Unknown => ObjectGcStepOutcome::ReconciliationRequired,
                ObjectGcReplicaObservation::Mismatch => ObjectGcStepOutcome::EvidenceMismatch,
            }
        };
        Ok(ObjectGcExecutionStep {
            operation,
            lease,
            replica_id: Some(action.replica_id()),
            outcome,
        })
    }

    fn store_for(
        &self,
        action: &ObjectGcReplicaAction,
    ) -> Result<Arc<dyn ObjectStore>, ObjectGcExecutionError> {
        self.object_stores
            .get(action.backend_kind())
            .cloned()
            .ok_or(ObjectGcExecutionError::BackendUnavailable)
    }
}

fn metadata_matches(action: &ObjectGcReplicaAction, metadata: &ObjectMetadata) -> bool {
    if metadata.key().as_str() != action.storage_key()
        || metadata.length() != action.expected_stored_length()
        || metadata.sha256().copied() != Some(action.expected_stored_sha256())
    {
        return false;
    }
    match action.expected_backend_version() {
        Some(expected) => {
            ObjectVersion::new(expected.to_owned()).ok().as_ref() == metadata.version()
        }
        None => true,
    }
}

fn backend_kind(kind: StorageBackendKind) -> Option<&'static str> {
    match kind {
        StorageBackendKind::LocalFilesystem => Some("LOCAL_FILESYSTEM"),
        StorageBackendKind::ObjectStore => Some("OBJECT_STORE"),
        StorageBackendKind::Unknown => None,
    }
}

fn map_metadata_error(error: ObjectGcExecutionMetadataError) -> ObjectGcExecutionError {
    match error {
        ObjectGcExecutionMetadataError::CandidateNotFound => {
            ObjectGcExecutionError::CandidateNotFound
        }
        ObjectGcExecutionMetadataError::ReadyRequired => ObjectGcExecutionError::ReadyRequired,
        ObjectGcExecutionMetadataError::StaleLease => ObjectGcExecutionError::StaleLease,
        ObjectGcExecutionMetadataError::LeaseExpired => ObjectGcExecutionError::LeaseExpired,
        ObjectGcExecutionMetadataError::ReferenceExists => ObjectGcExecutionError::ReferenceExists,
        ObjectGcExecutionMetadataError::HoldExists => ObjectGcExecutionError::HoldExists,
        ObjectGcExecutionMetadataError::OperationNotFound => {
            ObjectGcExecutionError::OperationNotFound
        }
        ObjectGcExecutionMetadataError::InvalidState => ObjectGcExecutionError::InvalidState,
        ObjectGcExecutionMetadataError::ReplicaMetadataMismatch => {
            ObjectGcExecutionError::EvidenceMismatch
        }
        ObjectGcExecutionMetadataError::StaleAction => ObjectGcExecutionError::InvalidState,
        ObjectGcExecutionMetadataError::NeedsAttention => ObjectGcExecutionError::NeedsAttention,
        ObjectGcExecutionMetadataError::Database(_) => ObjectGcExecutionError::DatabaseUnavailable,
        ObjectGcExecutionMetadataError::InvalidPersistedData => {
            ObjectGcExecutionError::InvalidPersistedData
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectGcExecutionConfigurationError, ObjectGcExecutionError, ObjectGcStepOutcome};

    #[test]
    fn application_errors_and_outcomes_are_closed_and_safe() {
        assert_eq!(
            ObjectGcExecutionError::EvidenceMismatch.to_string(),
            "physical GC replica evidence does not match"
        );
        assert!(
            !ObjectGcExecutionError::DatabaseUnavailable
                .to_string()
                .contains("postgres")
        );
        assert_eq!(
            ObjectGcExecutionConfigurationError::DuplicateBackendKind.to_string(),
            "physical GC backend mapping is ambiguous"
        );
        assert_ne!(
            ObjectGcStepOutcome::ReconciliationRequired,
            ObjectGcStepOutcome::Completed
        );
    }
}
