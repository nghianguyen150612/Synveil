use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::http::HeaderMap;
use synveil_auth::{PasswordHasherConfig, SessionConfig};
use synveil_core::TrashRetentionPolicy;
use synveil_metadata::{
    ClientMutationBackend, ConflictManagementBackend, DatabasePool, FileMetadataBackend,
    FileMetadataService, VersionHistoryBackend, VersionHistoryService, VersionRestoreBackend,
    VersionRestoreService,
};
use synveil_platform::{HealthInfo, PlatformRuntime};

use crate::{
    auth::{
        AuthenticationBackend, PostgresAuthenticationBackend, UnavailableAuthenticationBackend,
    },
    conflicts::{
        ConflictCursorKey, PostgresConflictManagementBackend, UnavailableConflictManagementBackend,
    },
    cookies::CookieConfig,
    csrf::CsrfKey,
    device_auth::{
        DeviceAuthenticationBackend, PostgresDeviceAuthenticationBackend,
        UnavailableDeviceAuthenticationBackend,
    },
    downloads::{DownloadBackend, UnavailableDownloadBackend},
    etag::EtagKey,
    files::UnavailableFileMetadataBackend,
    mutations::{PostgresClientMutationBackend, UnavailableClientMutationBackend},
    rebaseline::{
        PostgresRebaselineBackend, RebaselineBackend, RebaselineTokenKey,
        UnavailableRebaselineBackend,
    },
    sync::{PostgresSyncFeedBackend, SyncAckKey, SyncFeedBackend, UnavailableSyncFeedBackend},
    uploads::{UnavailableUploadBackend, UploadBackend},
    versions::{UnavailableVersionHistoryBackend, UnavailableVersionRestoreBackend},
};

/// Bounded readiness evidence consumed by the transport layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadinessSnapshot {
    ready: bool,
}

impl ReadinessSnapshot {
    #[must_use]
    pub const fn ready() -> Self {
        Self { ready: true }
    }

    #[must_use]
    pub const fn not_ready() -> Self {
        Self { ready: false }
    }

    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.ready
    }
}

/// Application-facing readiness port. Concrete dependency probes stay outside
/// the HTTP handlers and can later combine bounded database/object-store facts.
pub trait ReadinessProbe: Send + Sync {
    fn snapshot(&self) -> ReadinessSnapshot;
}

/// Readiness adapter for the existing platform runtime contract.
pub struct PlatformReadiness {
    runtime: Arc<dyn PlatformRuntime>,
}

impl PlatformReadiness {
    #[must_use]
    pub fn new(runtime: Arc<dyn PlatformRuntime>) -> Self {
        Self { runtime }
    }
}

impl ReadinessProbe for PlatformReadiness {
    fn snapshot(&self) -> ReadinessSnapshot {
        if self.runtime.health().readiness() {
            ReadinessSnapshot::ready()
        } else {
            ReadinessSnapshot::not_ready()
        }
    }
}

/// Mutable, dependency-free readiness evidence useful for composition roots and
/// service tests. It does not perform a probe or mutate application data.
pub struct StaticReadiness {
    ready: AtomicBool,
}

impl StaticReadiness {
    #[must_use]
    pub fn new(ready: bool) -> Self {
        Self {
            ready: AtomicBool::new(ready),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::Release);
    }
}

impl ReadinessProbe for StaticReadiness {
    fn snapshot(&self) -> ReadinessSnapshot {
        if self.ready.load(Ordering::Acquire) {
            ReadinessSnapshot::ready()
        } else {
            ReadinessSnapshot::not_ready()
        }
    }
}

/// Authorization hook for the restricted detailed system-health view.
///
/// Detailed system-health authorization remains a separate application hook;
/// browser authentication is supplied through `AuthenticationBackend`.
pub trait SystemHealthAuthorizer: Send + Sync {
    fn authorize(&self, headers: &HeaderMap) -> bool;
}

/// Fail-closed default until an authenticated/admin authorizer is supplied.
#[derive(Clone, Copy, Debug, Default)]
pub struct DenySystemHealth;

impl SystemHealthAuthorizer for DenySystemHealth {
    fn authorize(&self, _headers: &HeaderMap) -> bool {
        false
    }
}

/// Transport/application state passed to Axum handlers.
#[derive(Clone)]
pub struct ApiState {
    runtime: Arc<dyn PlatformRuntime>,
    readiness: Arc<dyn ReadinessProbe>,
    health_authorizer: Arc<dyn SystemHealthAuthorizer>,
    auth_backend: Arc<dyn AuthenticationBackend>,
    device_auth_backend: Arc<dyn DeviceAuthenticationBackend>,
    file_metadata_backend: Arc<dyn FileMetadataBackend>,
    client_mutation_backend: Arc<dyn ClientMutationBackend>,
    conflict_management_backend: Arc<dyn ConflictManagementBackend>,
    version_history_backend: Arc<dyn VersionHistoryBackend>,
    version_restore_backend: Arc<dyn VersionRestoreBackend>,
    download_backend: Arc<dyn DownloadBackend>,
    upload_backend: Arc<dyn UploadBackend>,
    sync_backend: Arc<dyn SyncFeedBackend>,
    rebaseline_backend: Arc<dyn RebaselineBackend>,
    csrf_key: Arc<CsrfKey>,
    etag_key: Arc<EtagKey>,
    sync_ack_key: Arc<SyncAckKey>,
    rebaseline_token_key: Arc<RebaselineTokenKey>,
    conflict_cursor_key: Arc<ConflictCursorKey>,
    cookie_config: CookieConfig,
    allowed_origin: Option<String>,
    body_limit_bytes: usize,
    trash_retention_policy: TrashRetentionPolicy,
}

impl ApiState {
    /// Construct state with an explicit platform runtime and readiness port.
    #[must_use]
    pub fn new(runtime: Box<dyn PlatformRuntime>, readiness: Arc<dyn ReadinessProbe>) -> Self {
        let runtime: Arc<dyn PlatformRuntime> = runtime.into();
        Self {
            readiness,
            runtime,
            health_authorizer: Arc::new(DenySystemHealth),
            auth_backend: Arc::new(UnavailableAuthenticationBackend),
            device_auth_backend: Arc::new(UnavailableDeviceAuthenticationBackend),
            file_metadata_backend: Arc::new(UnavailableFileMetadataBackend),
            client_mutation_backend: Arc::new(UnavailableClientMutationBackend),
            conflict_management_backend: Arc::new(UnavailableConflictManagementBackend),
            version_history_backend: Arc::new(UnavailableVersionHistoryBackend),
            version_restore_backend: Arc::new(UnavailableVersionRestoreBackend),
            download_backend: Arc::new(UnavailableDownloadBackend),
            upload_backend: Arc::new(UnavailableUploadBackend),
            sync_backend: Arc::new(UnavailableSyncFeedBackend),
            rebaseline_backend: Arc::new(UnavailableRebaselineBackend),
            csrf_key: Arc::new(CsrfKey::generate()),
            etag_key: Arc::new(EtagKey::generate()),
            sync_ack_key: Arc::new(SyncAckKey::generate()),
            rebaseline_token_key: Arc::new(RebaselineTokenKey::generate()),
            conflict_cursor_key: Arc::new(ConflictCursorKey::generate()),
            cookie_config: CookieConfig::production(),
            allowed_origin: None,
            body_limit_bytes: crate::DEFAULT_BODY_LIMIT_BYTES,
            trash_retention_policy: TrashRetentionPolicy::default(),
        }
    }

    /// Construct the minimal cross-platform runtime composition root.
    #[must_use]
    pub fn from_current_platform() -> Self {
        let runtime: Arc<dyn PlatformRuntime> = synveil_platform::current().into();
        let readiness: Arc<dyn ReadinessProbe> =
            Arc::new(PlatformReadiness::new(Arc::clone(&runtime)));
        Self {
            runtime,
            readiness,
            health_authorizer: Arc::new(DenySystemHealth),
            auth_backend: Arc::new(UnavailableAuthenticationBackend),
            device_auth_backend: Arc::new(UnavailableDeviceAuthenticationBackend),
            file_metadata_backend: Arc::new(UnavailableFileMetadataBackend),
            client_mutation_backend: Arc::new(UnavailableClientMutationBackend),
            conflict_management_backend: Arc::new(UnavailableConflictManagementBackend),
            version_history_backend: Arc::new(UnavailableVersionHistoryBackend),
            version_restore_backend: Arc::new(UnavailableVersionRestoreBackend),
            download_backend: Arc::new(UnavailableDownloadBackend),
            upload_backend: Arc::new(UnavailableUploadBackend),
            sync_backend: Arc::new(UnavailableSyncFeedBackend),
            rebaseline_backend: Arc::new(UnavailableRebaselineBackend),
            csrf_key: Arc::new(CsrfKey::generate()),
            etag_key: Arc::new(EtagKey::generate()),
            sync_ack_key: Arc::new(SyncAckKey::generate()),
            rebaseline_token_key: Arc::new(RebaselineTokenKey::generate()),
            conflict_cursor_key: Arc::new(ConflictCursorKey::generate()),
            cookie_config: CookieConfig::production(),
            allowed_origin: None,
            body_limit_bytes: crate::DEFAULT_BODY_LIMIT_BYTES,
            trash_retention_policy: TrashRetentionPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_readiness(mut self, readiness: Arc<dyn ReadinessProbe>) -> Self {
        self.readiness = readiness;
        self
    }

    #[must_use]
    pub fn with_system_health_authorizer(
        mut self,
        authorizer: Arc<dyn SystemHealthAuthorizer>,
    ) -> Self {
        self.health_authorizer = authorizer;
        self
    }

    #[must_use]
    pub fn with_body_limit(mut self, body_limit_bytes: usize) -> Self {
        self.body_limit_bytes = body_limit_bytes;
        self
    }

    #[must_use]
    pub fn with_trash_retention_policy(mut self, policy: TrashRetentionPolicy) -> Self {
        self.trash_retention_policy = policy;
        self
    }

    #[must_use]
    pub fn with_auth_backend(mut self, backend: Arc<dyn AuthenticationBackend>) -> Self {
        self.auth_backend = backend;
        self
    }

    #[must_use]
    pub fn with_device_auth_backend(
        mut self,
        backend: Arc<dyn DeviceAuthenticationBackend>,
    ) -> Self {
        self.device_auth_backend = backend;
        self
    }

    #[must_use]
    pub fn with_file_metadata_backend(mut self, backend: Arc<dyn FileMetadataBackend>) -> Self {
        self.file_metadata_backend = backend;
        self
    }

    #[must_use]
    pub fn with_client_mutation_backend(mut self, backend: Arc<dyn ClientMutationBackend>) -> Self {
        self.client_mutation_backend = backend;
        self
    }

    #[must_use]
    pub fn with_conflict_management_backend(
        mut self,
        backend: Arc<dyn ConflictManagementBackend>,
    ) -> Self {
        self.conflict_management_backend = backend;
        self
    }

    #[must_use]
    pub fn with_version_history_backend(mut self, backend: Arc<dyn VersionHistoryBackend>) -> Self {
        self.version_history_backend = backend;
        self
    }

    #[must_use]
    pub fn with_version_restore_backend(mut self, backend: Arc<dyn VersionRestoreBackend>) -> Self {
        self.version_restore_backend = backend;
        self
    }

    #[must_use]
    pub fn with_download_backend(mut self, backend: Arc<dyn DownloadBackend>) -> Self {
        self.download_backend = backend;
        self
    }

    #[must_use]
    pub fn with_upload_backend(mut self, backend: Arc<dyn UploadBackend>) -> Self {
        self.upload_backend = backend;
        self
    }

    #[must_use]
    pub fn with_sync_backend(mut self, backend: Arc<dyn SyncFeedBackend>) -> Self {
        self.sync_backend = backend;
        self
    }

    #[must_use]
    pub fn with_rebaseline_backend(mut self, backend: Arc<dyn RebaselineBackend>) -> Self {
        self.rebaseline_backend = backend;
        self
    }

    #[must_use]
    pub fn with_postgres_auth(
        self,
        pool: Arc<DatabasePool>,
        password_config: PasswordHasherConfig,
        session_config: SessionConfig,
        rebaseline_token_key: RebaselineTokenKey,
    ) -> Self {
        let conflict_cursor_key = ConflictCursorKey::from_bytes(*rebaseline_token_key.key_bytes());
        let state = self
            .with_device_auth_backend(Arc::new(PostgresDeviceAuthenticationBackend::new(
                Arc::clone(&pool),
            )))
            .with_auth_backend(Arc::new(PostgresAuthenticationBackend::new(
                Arc::clone(&pool),
                password_config,
                session_config,
            )))
            .with_sync_backend(Arc::new(PostgresSyncFeedBackend::new(
                pool.as_ref().clone(),
            )))
            .with_rebaseline_backend(Arc::new(PostgresRebaselineBackend::new(
                pool.as_ref().clone(),
            )))
            .with_client_mutation_backend(Arc::new(PostgresClientMutationBackend::new(
                pool.as_ref().clone(),
            )))
            .with_conflict_management_backend(Arc::new(PostgresConflictManagementBackend::new(
                pool.as_ref().clone(),
            )))
            .with_conflict_cursor_key(conflict_cursor_key)
            .with_rebaseline_token_key(rebaseline_token_key);
        let trash_retention_policy = state.trash_retention_policy;
        let pool = pool.as_ref().clone();
        state
            .with_file_metadata_backend(Arc::new(FileMetadataService::new_with_policy(
                pool.clone(),
                trash_retention_policy,
            )))
            .with_version_history_backend(Arc::new(VersionHistoryService::new(pool.clone())))
            .with_version_restore_backend(Arc::new(VersionRestoreService::new(pool)))
    }

    #[must_use]
    pub fn with_csrf_key(mut self, key: CsrfKey) -> Self {
        self.csrf_key = Arc::new(key);
        self
    }

    #[must_use]
    pub fn with_etag_key(mut self, key: EtagKey) -> Self {
        self.etag_key = Arc::new(key);
        self
    }

    #[must_use]
    pub fn with_sync_ack_key(mut self, key: SyncAckKey) -> Self {
        self.sync_ack_key = Arc::new(key);
        self
    }

    #[must_use]
    pub fn with_rebaseline_token_key(mut self, key: RebaselineTokenKey) -> Self {
        self.rebaseline_token_key = Arc::new(key);
        self
    }

    #[must_use]
    pub fn with_conflict_cursor_key(mut self, key: ConflictCursorKey) -> Self {
        self.conflict_cursor_key = Arc::new(key);
        self
    }

    #[must_use]
    pub fn with_cookie_config(mut self, config: CookieConfig) -> Self {
        self.cookie_config = config;
        self
    }

    #[must_use]
    pub fn with_allowed_origin(mut self, origin: impl Into<String>) -> Self {
        let origin = origin.into();
        self.allowed_origin = (!origin.trim().is_empty()).then_some(origin);
        self
    }

    #[must_use]
    pub(crate) fn body_limit_bytes(&self) -> usize {
        self.body_limit_bytes
    }

    #[must_use]
    pub(crate) const fn trash_retention_policy(&self) -> TrashRetentionPolicy {
        self.trash_retention_policy
    }

    #[must_use]
    pub(crate) fn auth_backend(&self) -> &Arc<dyn AuthenticationBackend> {
        &self.auth_backend
    }

    #[must_use]
    pub(crate) fn device_auth_backend(&self) -> &Arc<dyn DeviceAuthenticationBackend> {
        &self.device_auth_backend
    }

    #[must_use]
    pub(crate) fn file_metadata_backend(&self) -> &Arc<dyn FileMetadataBackend> {
        &self.file_metadata_backend
    }

    #[must_use]
    pub(crate) fn client_mutation_backend(&self) -> &Arc<dyn ClientMutationBackend> {
        &self.client_mutation_backend
    }

    #[must_use]
    pub(crate) fn conflict_management_backend(&self) -> &Arc<dyn ConflictManagementBackend> {
        &self.conflict_management_backend
    }

    #[must_use]
    pub(crate) fn version_history_backend(&self) -> &Arc<dyn VersionHistoryBackend> {
        &self.version_history_backend
    }

    #[must_use]
    pub(crate) fn version_restore_backend(&self) -> &Arc<dyn VersionRestoreBackend> {
        &self.version_restore_backend
    }

    #[must_use]
    pub(crate) fn download_backend(&self) -> &Arc<dyn DownloadBackend> {
        &self.download_backend
    }

    #[must_use]
    pub(crate) fn upload_backend(&self) -> &Arc<dyn UploadBackend> {
        &self.upload_backend
    }

    #[must_use]
    pub(crate) fn sync_backend(&self) -> &Arc<dyn SyncFeedBackend> {
        &self.sync_backend
    }

    #[must_use]
    pub(crate) fn rebaseline_backend(&self) -> &Arc<dyn RebaselineBackend> {
        &self.rebaseline_backend
    }

    #[must_use]
    pub(crate) fn csrf_key(&self) -> &CsrfKey {
        self.csrf_key.as_ref()
    }

    #[must_use]
    pub(crate) fn etag_key(&self) -> &EtagKey {
        self.etag_key.as_ref()
    }

    #[must_use]
    pub(crate) fn sync_ack_key(&self) -> &SyncAckKey {
        self.sync_ack_key.as_ref()
    }

    #[must_use]
    pub(crate) fn rebaseline_token_key(&self) -> &RebaselineTokenKey {
        self.rebaseline_token_key.as_ref()
    }

    #[must_use]
    pub(crate) fn conflict_cursor_key(&self) -> &ConflictCursorKey {
        self.conflict_cursor_key.as_ref()
    }

    #[must_use]
    pub(crate) const fn cookie_config(&self) -> CookieConfig {
        self.cookie_config
    }

    #[must_use]
    pub(crate) fn allowed_origin(&self) -> Option<&str> {
        self.allowed_origin.as_deref()
    }

    #[must_use]
    pub(crate) fn health(&self) -> HealthInfo {
        self.runtime.health()
    }

    #[must_use]
    pub(crate) fn readiness_snapshot(&self) -> ReadinessSnapshot {
        self.readiness.snapshot()
    }

    #[must_use]
    pub(crate) fn health_authorized(&self, headers: &HeaderMap) -> bool {
        self.health_authorizer.authorize(headers)
    }
}
