use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::http::HeaderMap;
use synveil_auth::{PasswordHasherConfig, SessionConfig};
use synveil_metadata::{DatabasePool, FileMetadataBackend, FileMetadataService};
use synveil_platform::{HealthInfo, PlatformRuntime};

use crate::{
    auth::{
        AuthenticationBackend, PostgresAuthenticationBackend, UnavailableAuthenticationBackend,
    },
    cookies::CookieConfig,
    csrf::CsrfKey,
    etag::EtagKey,
    files::UnavailableFileMetadataBackend,
    uploads::{UnavailableUploadBackend, UploadBackend},
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
    file_metadata_backend: Arc<dyn FileMetadataBackend>,
    upload_backend: Arc<dyn UploadBackend>,
    csrf_key: Arc<CsrfKey>,
    etag_key: Arc<EtagKey>,
    cookie_config: CookieConfig,
    allowed_origin: Option<String>,
    body_limit_bytes: usize,
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
            file_metadata_backend: Arc::new(UnavailableFileMetadataBackend),
            upload_backend: Arc::new(UnavailableUploadBackend),
            csrf_key: Arc::new(CsrfKey::generate()),
            etag_key: Arc::new(EtagKey::generate()),
            cookie_config: CookieConfig::production(),
            allowed_origin: None,
            body_limit_bytes: crate::DEFAULT_BODY_LIMIT_BYTES,
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
            file_metadata_backend: Arc::new(UnavailableFileMetadataBackend),
            upload_backend: Arc::new(UnavailableUploadBackend),
            csrf_key: Arc::new(CsrfKey::generate()),
            etag_key: Arc::new(EtagKey::generate()),
            cookie_config: CookieConfig::production(),
            allowed_origin: None,
            body_limit_bytes: crate::DEFAULT_BODY_LIMIT_BYTES,
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
    pub fn with_auth_backend(mut self, backend: Arc<dyn AuthenticationBackend>) -> Self {
        self.auth_backend = backend;
        self
    }

    #[must_use]
    pub fn with_file_metadata_backend(mut self, backend: Arc<dyn FileMetadataBackend>) -> Self {
        self.file_metadata_backend = backend;
        self
    }

    #[must_use]
    pub fn with_upload_backend(mut self, backend: Arc<dyn UploadBackend>) -> Self {
        self.upload_backend = backend;
        self
    }

    #[must_use]
    pub fn with_postgres_auth(
        self,
        pool: Arc<DatabasePool>,
        password_config: PasswordHasherConfig,
        session_config: SessionConfig,
    ) -> Self {
        let state = self.with_auth_backend(Arc::new(PostgresAuthenticationBackend::new(
            Arc::clone(&pool),
            password_config,
            session_config,
        )));
        state.with_file_metadata_backend(Arc::new(FileMetadataService::new(pool.as_ref().clone())))
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
    pub(crate) fn auth_backend(&self) -> &Arc<dyn AuthenticationBackend> {
        &self.auth_backend
    }

    #[must_use]
    pub(crate) fn file_metadata_backend(&self) -> &Arc<dyn FileMetadataBackend> {
        &self.file_metadata_backend
    }

    #[must_use]
    pub(crate) fn upload_backend(&self) -> &Arc<dyn UploadBackend> {
        &self.upload_backend
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
