//! Durable-change and runtime-signal integration boundaries.
//!
//! Producers in this module follow one ordering rule: complete the existing
//! durable operation, release the per-library writer guard, and only then
//! deliver a best-effort runtime hint. A stopped runtime or an unregistered
//! library therefore changes latency, never local synchronization
//! correctness.

use std::{collections::BTreeSet, sync::Arc};

use synveil_core::LibraryId;
use synveil_platform::SecretStore;

use crate::{
    ClientSyncError, DeviceEnrollmentRecord, EnrollmentCredentials, LocalStateStore,
    MAX_SYNC_RUNTIME_LIBRARIES, OutboundIntent, OutboundIntentUpsertResult, ServerProfileId,
    SyncRuntimeIdentity, SyncRuntimeWakeReason, SyncRuntimeWakeResult, SyncWakeNotifier,
};

/// The durable half of a producer operation that may be followed by a wake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableChangeResult {
    NoChange,
    Committed,
}

/// Non-ambiguous result for a durable operation plus its optional wake.
///
/// `wake_result == None` means either that no durable work was created, that a
/// multi-batch reconciliation has not reached its single wake boundary, or
/// that the caller intentionally did not attach a notifier. In the latter two
/// cases startup and periodic safety paths remain the correctness fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableChangeNotification {
    durable_result: DurableChangeResult,
    wake_result: Option<SyncRuntimeWakeResult>,
}

impl DurableChangeNotification {
    #[must_use]
    pub const fn durable_result(self) -> DurableChangeResult {
        self.durable_result
    }

    #[must_use]
    pub const fn wake_result(self) -> Option<SyncRuntimeWakeResult> {
        self.wake_result
    }

    #[must_use]
    pub const fn durable_changed(self) -> bool {
        matches!(self.durable_result, DurableChangeResult::Committed)
    }

    #[must_use]
    pub const fn wake_was_accepted(self) -> bool {
        match self.wake_result {
            Some(result) => result.was_accepted(),
            None => false,
        }
    }

    pub(crate) const fn no_change() -> Self {
        Self {
            durable_result: DurableChangeResult::NoChange,
            wake_result: None,
        }
    }

    pub(crate) const fn committed(wake_result: Option<SyncRuntimeWakeResult>) -> Self {
        Self {
            durable_result: DurableChangeResult::Committed,
            wake_result,
        }
    }
}

/// Result returned by [`OutboundIntentProducer`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableOutboundIntentResult {
    intent: OutboundIntent,
    notification: DurableChangeNotification,
}

impl DurableOutboundIntentResult {
    #[must_use]
    pub const fn intent(&self) -> &OutboundIntent {
        &self.intent
    }

    #[must_use]
    pub const fn notification(&self) -> DurableChangeNotification {
        self.notification
    }

    #[must_use]
    pub fn into_intent(self) -> OutboundIntent {
        self.intent
    }
}

/// Canonical local outbound-intent producer for callers that need a runtime
/// wake after the durable operation.
///
/// This wrapper deliberately leaves [`LocalStateStore`] unaware of the
/// scheduler. It serializes the intent mutation with the same per-library
/// writer guard used by observation and releases that guard before invoking
/// the notifier.
pub struct OutboundIntentProducer {
    state: Arc<LocalStateStore>,
    notifier: Arc<dyn SyncWakeNotifier>,
}

impl OutboundIntentProducer {
    #[must_use]
    pub fn new(state: Arc<LocalStateStore>, notifier: Arc<dyn SyncWakeNotifier>) -> Self {
        Self { state, notifier }
    }

    /// Identity of the runtime targeted by this producer, when its notifier
    /// is one of the canonical runtime handles.
    #[must_use]
    pub fn runtime_identity(&self) -> Option<SyncRuntimeIdentity> {
        self.notifier.runtime_identity()
    }

    /// Persist one intent, then signal `LocalChange` exactly when the
    /// idempotent operation inserted or updated durable active work.
    pub async fn upsert(
        &self,
        intent: &OutboundIntent,
    ) -> Result<DurableOutboundIntentResult, ClientSyncError> {
        let _writer = self.state.lock_replica_writer(intent.library_id()).await;
        let persisted = self
            .state
            .upsert_outbound_intent_with_result(intent)
            .await?;
        drop(_writer);

        let notification = self.notify_if_changed(intent.library_id(), &persisted);
        Ok(DurableOutboundIntentResult {
            intent: persisted.into_intent(),
            notification,
        })
    }

    fn notify_if_changed(
        &self,
        library_id: LibraryId,
        persisted: &OutboundIntentUpsertResult,
    ) -> DurableChangeNotification {
        if !persisted.changed() {
            return DurableChangeNotification::no_change();
        }
        DurableChangeNotification::committed(Some(
            self.notifier
                .wake_library(library_id, SyncRuntimeWakeReason::LocalChange),
        ))
    }
}

/// Credential lifecycle adapter for an embedding controller.
///
/// Credential persistence remains owned by the existing profile-bound
/// `LocalStateStore`/`SecretStore` contract. The adapter only adds the
/// post-success scheduling hint for the explicitly supplied libraries; it
/// never carries secret material in the wake message. Forget/logout wakes only
/// after the durable tombstone and secure-store cleanup have both succeeded.
pub struct CredentialLifecycleController {
    state: Arc<LocalStateStore>,
    secret_store: Arc<dyn SecretStore>,
    notifier: Arc<dyn SyncWakeNotifier>,
}

impl CredentialLifecycleController {
    #[must_use]
    pub fn new(
        state: Arc<LocalStateStore>,
        secret_store: Arc<dyn SecretStore>,
        notifier: Arc<dyn SyncWakeNotifier>,
    ) -> Self {
        Self {
            state,
            secret_store,
            notifier,
        }
    }

    /// Identity of the runtime that receives post-persistence credential
    /// hints, when the supplied notifier is canonical.
    #[must_use]
    pub fn runtime_identity(&self) -> Option<SyncRuntimeIdentity> {
        self.notifier.runtime_identity()
    }

    /// Store a newly usable enrollment, then send `CredentialChanged` to each
    /// affected registered library. A persistence/validation error returns
    /// before any wake is emitted.
    pub async fn store_enrollment(
        &self,
        enrollment: &EnrollmentCredentials,
        affected_libraries: &[LibraryId],
    ) -> Result<CredentialLifecycleResult, ClientSyncError> {
        Self::validate_library_list(affected_libraries)?;
        let record = self
            .state
            .store_enrollment(enrollment, self.secret_store.as_ref())
            .await?;
        Ok(CredentialLifecycleResult {
            record,
            wake_results: self.wake_libraries(affected_libraries)?,
        })
    }

    /// Replace a usable enrollment, then send `CredentialChanged` only after
    /// the existing credential transaction and secure-store verification have
    /// succeeded.
    pub async fn replace_enrollment(
        &self,
        enrollment: &EnrollmentCredentials,
        affected_libraries: &[LibraryId],
    ) -> Result<CredentialLifecycleResult, ClientSyncError> {
        Self::validate_library_list(affected_libraries)?;
        let record = self
            .state
            .replace_enrollment(enrollment, self.secret_store.as_ref())
            .await?;
        Ok(CredentialLifecycleResult {
            record,
            wake_results: self.wake_libraries(affected_libraries)?,
        })
    }

    /// Forget/remove the profile-bound credential and then send
    /// `CredentialChanged` to each affected library. The state layer writes a
    /// durable forgotten marker before deleting the secure-store value, so a
    /// failed deletion returns before an unauthenticated wake is published.
    pub async fn forget_device_credential(
        &self,
        profile_id: ServerProfileId,
        affected_libraries: &[LibraryId],
    ) -> Result<CredentialForgetResult, ClientSyncError> {
        Self::validate_library_list(affected_libraries)?;
        self.state
            .forget_device_credential(profile_id, self.secret_store.as_ref())
            .await?;
        Ok(CredentialForgetResult {
            wake_results: self.wake_libraries(affected_libraries)?,
        })
    }

    fn wake_libraries(
        &self,
        affected_libraries: &[LibraryId],
    ) -> Result<Vec<(LibraryId, SyncRuntimeWakeResult)>, ClientSyncError> {
        let unique: BTreeSet<_> = affected_libraries.iter().copied().collect();
        Ok(unique
            .into_iter()
            .map(|library_id| {
                (
                    library_id,
                    self.notifier
                        .wake_library(library_id, SyncRuntimeWakeReason::CredentialChanged),
                )
            })
            .collect())
    }

    fn validate_library_list(affected_libraries: &[LibraryId]) -> Result<(), ClientSyncError> {
        if affected_libraries.len() > MAX_SYNC_RUNTIME_LIBRARIES {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(())
    }
}

/// Durable result of a successful credential transition plus its scheduling
/// statuses. Wake failures remain visible without changing the success of the
/// credential persistence operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialLifecycleResult {
    record: DeviceEnrollmentRecord,
    wake_results: Vec<(LibraryId, SyncRuntimeWakeResult)>,
}

impl CredentialLifecycleResult {
    #[must_use]
    pub const fn record(&self) -> DeviceEnrollmentRecord {
        self.record
    }

    #[must_use]
    pub fn wake_results(&self) -> &[(LibraryId, SyncRuntimeWakeResult)] {
        &self.wake_results
    }
}

/// Result of a successful local credential removal plus its scheduling
/// statuses. The type intentionally carries no credential or SecretStore
/// value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialForgetResult {
    wake_results: Vec<(LibraryId, SyncRuntimeWakeResult)>,
}

impl CredentialForgetResult {
    #[must_use]
    pub fn wake_results(&self) -> &[(LibraryId, SyncRuntimeWakeResult)] {
        &self.wake_results
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use synveil_core::{
        DeviceCredentialId, DeviceCredentialSecret, DeviceId, LibraryId, LogicalName, NodeId,
        NodeKind, NodeState, Revision, Sequence, Timestamp, UserId,
    };
    use synveil_platform::{SecretName, SecretStoreError, SecretStoreState, SecretValue};

    use super::*;
    use crate::{
        LocalFingerprint, LocalNode, LocalStateConfig, ManagedRelativePath, ReplicaScope,
        RootBindingId, ServerProfile,
    };

    struct RecordingNotifier {
        result: SyncRuntimeWakeResult,
        calls: Mutex<Vec<(LibraryId, SyncRuntimeWakeReason)>>,
    }

    impl RecordingNotifier {
        fn new(result: SyncRuntimeWakeResult) -> Arc<Self> {
            Arc::new(Self {
                result,
                calls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<(LibraryId, SyncRuntimeWakeReason)> {
            self.calls.lock().expect("notifier lock").clone()
        }
    }

    impl SyncWakeNotifier for RecordingNotifier {
        fn wake_library(
            &self,
            library_id: LibraryId,
            reason: SyncRuntimeWakeReason,
        ) -> SyncRuntimeWakeResult {
            self.calls
                .lock()
                .expect("notifier lock")
                .push((library_id, reason));
            self.result
        }
    }

    struct CommitObservingNotifier {
        state: Arc<LocalStateStore>,
        intent_id: synveil_core::OutboundIntentId,
        intent_visible_at_wake: AtomicBool,
        calls: Mutex<Vec<(LibraryId, SyncRuntimeWakeReason)>>,
    }

    impl CommitObservingNotifier {
        fn new(
            state: Arc<LocalStateStore>,
            intent_id: synveil_core::OutboundIntentId,
        ) -> Arc<Self> {
            Arc::new(Self {
                state,
                intent_id,
                intent_visible_at_wake: AtomicBool::new(false),
                calls: Mutex::new(Vec::new()),
            })
        }

        fn intent_visible_at_wake(&self) -> bool {
            self.intent_visible_at_wake.load(Ordering::Acquire)
        }
    }

    impl SyncWakeNotifier for CommitObservingNotifier {
        fn wake_library(
            &self,
            library_id: LibraryId,
            reason: SyncRuntimeWakeReason,
        ) -> SyncRuntimeWakeResult {
            self.calls
                .lock()
                .expect("notifier lock")
                .push((library_id, reason));

            // The producer must release its per-library writer guard before
            // this callback. A second task both acquires that guard and reads
            // the just-written row; if either ordering rule regresses, the
            // bounded receive below fails deterministically rather than using
            // timing after the producer returns.
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let state = Arc::clone(&self.state);
            let intent_id = self.intent_id;
            let task = tokio::spawn(async move {
                let _writer = state.lock_replica_writer(library_id).await;
                let visible = state
                    .outbound_intent(intent_id)
                    .await
                    .ok()
                    .flatten()
                    .is_some();
                let _ = sender.send(visible);
            });
            let visible = receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap_or(false);
            if !visible {
                task.abort();
            }
            self.intent_visible_at_wake
                .store(visible, Ordering::Release);
            SyncRuntimeWakeResult::Queued
        }
    }

    #[derive(Default)]
    struct TestSecretStore {
        values: Mutex<BTreeMap<SecretName, SecretValue>>,
        fail_put: AtomicBool,
    }

    impl SecretStore for TestSecretStore {
        fn state(&self) -> SecretStoreState {
            SecretStoreState::Available
        }

        fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
            if self.fail_put.load(Ordering::Acquire) {
                return Err(SecretStoreError::Unavailable);
            }
            self.values
                .lock()
                .expect("secret store lock")
                .insert(name.clone(), SecretValue::new(value));
            Ok(())
        }

        fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .expect("secret store lock")
                .get(name)
                .map(|value| SecretValue::new(value.as_bytes())))
        }

        fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .expect("secret store lock")
                .remove(name)
                .is_some())
        }
    }

    async fn state_fixture(label: &str) -> (PathBuf, Arc<LocalStateStore>, ReplicaScope, NodeId) {
        let directory = std::env::temp_dir().join(format!(
            "synveil-runtime-signals-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&directory).expect("fixture directory");
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                .await
                .expect("state store"),
        );
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        state
            .bind_replica(scope, RootBindingId::new())
            .await
            .expect("replica binding");
        let root_id = NodeId::new();
        state
            .upsert_local_node(&LocalNode::new(
                scope.library_id(),
                root_id,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").expect("root name"),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(0),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                true,
                None,
            ))
            .await
            .expect("root node");
        (directory, state, scope, root_id)
    }

    fn create_directory_intent(scope: ReplicaScope, root_id: NodeId, name: &str) -> OutboundIntent {
        OutboundIntent::new(
            scope.library_id(),
            None,
            Some(root_id),
            crate::OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new(name).expect("relative path"),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(0),
            Sequence::new(0),
            None,
            None,
            Some(Revision::new(0)),
        )
        .expect("intent shape")
    }

    async fn close_fixture(directory: PathBuf, state: Arc<LocalStateStore>) {
        state.close_pool().await;
        drop(state);
        fs::remove_dir_all(directory).expect("fixture cleanup");
    }

    #[tokio::test]
    async fn durable_intent_commits_before_one_wake_and_exact_noop_is_not_woken() {
        let (directory, state, scope, root_id) = state_fixture("ordering").await;
        let notifier = RecordingNotifier::new(SyncRuntimeWakeResult::Queued);
        let producer = OutboundIntentProducer::new(
            state.clone(),
            notifier.clone() as Arc<dyn SyncWakeNotifier>,
        );
        let intent = create_directory_intent(scope, root_id, "one");

        let first = producer.upsert(&intent).await.expect("intent persistence");
        assert_eq!(
            first.notification().durable_result(),
            DurableChangeResult::Committed
        );
        assert_eq!(
            first.notification().wake_result(),
            Some(SyncRuntimeWakeResult::Queued)
        );
        assert_eq!(notifier.calls().len(), 1);
        assert_eq!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .expect("intent read")
                .expect("durable intent")
                .intent_id(),
            intent.intent_id()
        );

        let duplicate = producer
            .upsert(&intent)
            .await
            .expect("idempotent persistence");
        assert_eq!(
            duplicate.notification().durable_result(),
            DurableChangeResult::NoChange
        );
        assert_eq!(duplicate.notification().wake_result(), None);
        assert_eq!(notifier.calls().len(), 1);
        close_fixture(directory, state).await;
    }

    #[tokio::test]
    async fn stopped_wake_is_reported_without_rolling_back_durable_intent() {
        let (directory, state, scope, root_id) = state_fixture("stopped").await;
        let notifier = RecordingNotifier::new(SyncRuntimeWakeResult::RuntimeStopped);
        let producer = OutboundIntentProducer::new(
            state.clone(),
            notifier.clone() as Arc<dyn SyncWakeNotifier>,
        );
        let intent = create_directory_intent(scope, root_id, "survives");

        let result = producer.upsert(&intent).await.expect("durable persistence");
        assert!(result.notification().durable_changed());
        assert_eq!(
            result.notification().wake_result(),
            Some(SyncRuntimeWakeResult::RuntimeStopped)
        );
        assert!(!result.notification().wake_was_accepted());
        assert!(
            state
                .outbound_intent(intent.intent_id())
                .await
                .expect("intent read")
                .is_some()
        );
        assert_eq!(notifier.calls().len(), 1);
        close_fixture(directory, state).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_and_writer_release_are_observable_before_wake_callback_returns() {
        let (directory, state, scope, root_id) = state_fixture("ordering-callback").await;
        let intent = create_directory_intent(scope, root_id, "callback-order");
        let notifier = CommitObservingNotifier::new(state.clone(), intent.intent_id());
        let producer = OutboundIntentProducer::new(
            state.clone(),
            notifier.clone() as Arc<dyn SyncWakeNotifier>,
        );

        let result = producer.upsert(&intent).await.expect("intent persistence");
        assert_eq!(
            result.notification().wake_result(),
            Some(SyncRuntimeWakeResult::Queued)
        );
        assert!(notifier.intent_visible_at_wake());
        close_fixture(directory, state).await;
    }

    async fn credential_fixture(
        label: &str,
    ) -> (
        PathBuf,
        Arc<LocalStateStore>,
        ServerProfile,
        EnrollmentCredentials,
        ReplicaScope,
    ) {
        let directory = std::env::temp_dir().join(format!(
            "synveil-runtime-credentials-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&directory).expect("credential fixture directory");
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                .await
                .expect("credential state store"),
        );
        let profile = ServerProfile::new(
            crate::CanonicalBaseUrl::parse("https://credentials.example").expect("base URL"),
            "Synthetic credential profile",
        )
        .expect("profile");
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let enrollment = EnrollmentCredentials::for_test(
            profile.clone(),
            scope.owner_user_id(),
            scope.device_id(),
            DeviceCredentialId::new(),
            DeviceCredentialSecret::from_bytes([0x5a; 32]),
            Timestamp::parse("2026-09-12T00:00:00Z").expect("credential timestamp"),
        );
        (directory, state, profile, enrollment, scope)
    }

    #[tokio::test]
    async fn usable_credential_wakes_after_persistence_and_failed_update_does_not() {
        let (directory, state, profile, enrollment, scope) = credential_fixture("lifecycle").await;
        let profile_id = profile.profile_id();
        state
            .save_server_profile(&profile)
            .await
            .expect("profile persistence");
        let secret_store = Arc::new(TestSecretStore::default());
        let notifier = RecordingNotifier::new(SyncRuntimeWakeResult::RuntimeStopped);
        let controller = CredentialLifecycleController::new(
            state.clone(),
            secret_store.clone() as Arc<dyn SecretStore>,
            notifier.clone() as Arc<dyn SyncWakeNotifier>,
        );

        let stored = controller
            .store_enrollment(&enrollment, &[scope.library_id(), scope.library_id()])
            .await
            .expect("usable credential persistence");
        assert_eq!(stored.wake_results().len(), 1);
        assert_eq!(
            stored.wake_results()[0].1,
            SyncRuntimeWakeResult::RuntimeStopped
        );
        assert_eq!(
            notifier.calls(),
            vec![(scope.library_id(), SyncRuntimeWakeReason::CredentialChanged)]
        );
        assert_eq!(
            state
                .profile_enrollment(profile_id)
                .await
                .unwrap()
                .unwrap()
                .credential_id(),
            enrollment.credential_id()
        );

        let failed_notifier = RecordingNotifier::new(SyncRuntimeWakeResult::Queued);
        let failed_controller = CredentialLifecycleController::new(
            state.clone(),
            secret_store.clone() as Arc<dyn SecretStore>,
            failed_notifier.clone() as Arc<dyn SyncWakeNotifier>,
        );
        secret_store.fail_put.store(true, Ordering::Release);
        assert!(
            failed_controller
                .replace_enrollment(
                    &EnrollmentCredentials::for_test(
                        profile.clone(),
                        scope.owner_user_id(),
                        scope.device_id(),
                        DeviceCredentialId::new(),
                        DeviceCredentialSecret::from_bytes([0x6b; 32]),
                        Timestamp::parse("2026-09-12T00:00:01Z").expect("credential timestamp"),
                    ),
                    &[scope.library_id()]
                )
                .await
                .is_err()
        );
        assert!(failed_notifier.calls().is_empty());

        controller
            .forget_device_credential(profile_id, &[scope.library_id()])
            .await
            .expect("credential removal");
        assert_eq!(notifier.calls().len(), 2);
        assert_eq!(
            notifier.calls()[1],
            (scope.library_id(), SyncRuntimeWakeReason::CredentialChanged)
        );
        close_fixture(directory, state).await;
    }
}
