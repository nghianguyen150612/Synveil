//! HTTP authentication boundary for browser sessions.
//!
//! This module owns request extraction, JSON shapes, cookie presentation, and
//! middleware. Password/session policy remains in `synveil-auth`; persistence
//! remains in `synveil-metadata`.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use synveil_auth::{
    AuthError, AuthenticatedSession, AuthenticationService, PasswordHasherConfig,
    PlaintextPassword, SessionConfig, SessionExpiry, SessionPrincipal, SessionToken,
};
use synveil_core::LoginIdentifier;
use synveil_metadata::DatabasePool;

use crate::{ApiError, ApiState, RequestContext, cookies, csrf, error::map_auth_error};

pub const LOGIN_BODY_LIMIT_BYTES: usize = 16 * 1024;
pub const BOOTSTRAP_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// Safe transport-neutral view of the persistent one-time bootstrap state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapStatus {
    Open,
    Closed,
}

/// Application-facing authentication port used by the HTTP layer. The API
/// does not depend on a concrete database implementation for request tests or
/// alternate composition roots.
#[async_trait]
pub trait AuthenticationBackend: Send + Sync {
    async fn bootstrap_state(&self) -> Result<BootstrapStatus, AuthError>;

    async fn create_first_admin(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
    ) -> Result<(), AuthError>;

    async fn login(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
    ) -> Result<IssuedSession, AuthError>;

    async fn authenticate_session(
        &self,
        token: &SessionToken,
    ) -> Result<SessionPrincipal, AuthError>;

    async fn revoke_current_session(&self, token: &SessionToken) -> Result<(), AuthError>;
}

/// Safe transport result produced by a successful password login. It contains
/// the raw token only in memory long enough for the cookie writer; it is never
/// serializable and has no `Debug` implementation.
pub struct IssuedSession {
    token: SessionToken,
    principal: SessionPrincipal,
    expires_at: SessionExpiry,
}

impl IssuedSession {
    #[must_use]
    pub fn new(
        token: SessionToken,
        principal: SessionPrincipal,
        expires_at: SessionExpiry,
    ) -> Self {
        Self {
            token,
            principal,
            expires_at,
        }
    }

    #[must_use]
    pub const fn token(&self) -> &SessionToken {
        &self.token
    }

    #[must_use]
    pub const fn principal(&self) -> SessionPrincipal {
        self.principal
    }

    #[must_use]
    pub const fn expires_at(&self) -> SessionExpiry {
        self.expires_at
    }
}

/// PostgreSQL-backed adapter over the transport-neutral auth service.
pub struct PostgresAuthenticationBackend {
    pool: Arc<DatabasePool>,
    password_config: PasswordHasherConfig,
    session_config: SessionConfig,
}

impl PostgresAuthenticationBackend {
    #[must_use]
    pub fn new(
        pool: Arc<DatabasePool>,
        password_config: PasswordHasherConfig,
        session_config: SessionConfig,
    ) -> Self {
        Self {
            pool,
            password_config,
            session_config,
        }
    }
}

#[async_trait]
impl AuthenticationBackend for PostgresAuthenticationBackend {
    async fn bootstrap_state(&self) -> Result<BootstrapStatus, AuthError> {
        let state = AuthenticationService::with_session_config(
            self.pool.as_ref(),
            self.password_config,
            self.session_config,
        )
        .bootstrap_state()
        .await?;
        Ok(match state {
            synveil_metadata::BootstrapState::Open => BootstrapStatus::Open,
            synveil_metadata::BootstrapState::Closed => BootstrapStatus::Closed,
            synveil_metadata::BootstrapState::Inconsistent => {
                return Err(AuthError::BootstrapStateInvalid);
            }
        })
    }

    async fn create_first_admin(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
    ) -> Result<(), AuthError> {
        AuthenticationService::with_session_config(
            self.pool.as_ref(),
            self.password_config,
            self.session_config,
        )
        .create_first_admin(login, password, synveil_core::Timestamp::now())
        .await
        .map(|_| ())
    }

    async fn login(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
    ) -> Result<IssuedSession, AuthError> {
        let session = AuthenticationService::with_session_config(
            self.pool.as_ref(),
            self.password_config,
            self.session_config,
        )
        .authenticate(login, password)
        .await?;
        Ok(issued_session(session))
    }

    async fn authenticate_session(
        &self,
        token: &SessionToken,
    ) -> Result<SessionPrincipal, AuthError> {
        AuthenticationService::with_session_config(
            self.pool.as_ref(),
            self.password_config,
            self.session_config,
        )
        .authenticate_session(token)
        .await
    }

    async fn revoke_current_session(&self, token: &SessionToken) -> Result<(), AuthError> {
        AuthenticationService::with_session_config(
            self.pool.as_ref(),
            self.password_config,
            self.session_config,
        )
        .revoke_current_session(token)
        .await
    }
}

/// Safe fail-closed default for a composition root that has not configured a
/// PostgreSQL pool yet. Public health routes remain usable, while auth never
/// pretends that a missing database is a successful login.
pub(crate) struct UnavailableAuthenticationBackend;

#[async_trait]
impl AuthenticationBackend for UnavailableAuthenticationBackend {
    async fn bootstrap_state(&self) -> Result<BootstrapStatus, AuthError> {
        Err(unavailable_error())
    }

    async fn create_first_admin(
        &self,
        _login: LoginIdentifier,
        _password: PlaintextPassword,
    ) -> Result<(), AuthError> {
        Err(unavailable_error())
    }

    async fn login(
        &self,
        _login: LoginIdentifier,
        _password: PlaintextPassword,
    ) -> Result<IssuedSession, AuthError> {
        Err(unavailable_error())
    }

    async fn authenticate_session(
        &self,
        _token: &SessionToken,
    ) -> Result<SessionPrincipal, AuthError> {
        Err(unavailable_error())
    }

    async fn revoke_current_session(&self, _token: &SessionToken) -> Result<(), AuthError> {
        Err(unavailable_error())
    }
}

/// Request extension containing only the minimal authenticated principal and
/// the typed bearer token needed by the CSRF/revocation boundary.
#[derive(Clone)]
pub(crate) struct AuthContext {
    token: SessionToken,
    principal: SessionPrincipal,
}

impl AuthContext {
    #[must_use]
    pub(crate) const fn new(token: SessionToken, principal: SessionPrincipal) -> Self {
        Self { token, principal }
    }

    #[must_use]
    pub(crate) const fn token(&self) -> &SessionToken {
        &self.token
    }

    #[must_use]
    pub(crate) const fn principal(&self) -> SessionPrincipal {
        self.principal
    }
}

#[derive(Clone)]
pub(crate) struct LogoutContext(Option<AuthContext>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginRequest {
    login: String,
    login_key: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BootstrapAdminRequest {
    login: String,
    login_key: String,
    password: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseMeta {
    pub(crate) request_id: String,
}

#[derive(Serialize)]
pub(crate) struct AuthSessionResponse {
    data: AuthSessionData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
pub(crate) struct AuthSessionData {
    authenticated: bool,
    user_id: String,
    is_instance_admin: bool,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct CsrfResponse {
    data: CsrfData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct CsrfData {
    csrf_token: String,
}

#[derive(Serialize)]
pub(crate) struct BootstrapStatusResponse {
    data: BootstrapStatusData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct BootstrapStatusData {
    setup_required: bool,
}

pub(crate) async fn login(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let login = LoginIdentifier::new(payload.login, payload.login_key)
        .map_err(|_| ApiError::InvalidRequest)?;
    let password =
        PlaintextPassword::new(payload.password).map_err(|_| ApiError::InvalidRequest)?;
    let session = state
        .auth_backend()
        .login(login, password)
        .await
        .map_err(map_auth_error)?;

    let csrf_token = state.csrf_key().issue(session.token());
    let mut response = Json(AuthSessionResponse {
        data: session_data(session.principal(), session.expires_at()),
        meta: ResponseMeta {
            request_id: context.request_id().to_string(),
        },
    })
    .into_response();
    cookies::append_session_cookie(
        response.headers_mut(),
        session.token(),
        state.cookie_config(),
    );
    cookies::append_csrf_cookie(response.headers_mut(), &csrf_token, state.cookie_config());
    Ok(response)
}

pub(crate) async fn bootstrap_status(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
) -> Result<Json<BootstrapStatusResponse>, ApiError> {
    let bootstrap_state = state
        .auth_backend()
        .bootstrap_state()
        .await
        .map_err(map_auth_error)?;
    Ok(Json(BootstrapStatusResponse {
        data: BootstrapStatusData {
            setup_required: bootstrap_state == BootstrapStatus::Open,
        },
        meta: ResponseMeta {
            request_id: context.request_id().to_string(),
        },
    }))
}

pub(crate) async fn create_first_admin(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<BootstrapAdminRequest>,
) -> Result<Json<BootstrapStatusResponse>, ApiError> {
    let login = LoginIdentifier::new(payload.login, payload.login_key)
        .map_err(|_| ApiError::InvalidRequest)?;
    let password =
        PlaintextPassword::new(payload.password).map_err(|_| ApiError::InvalidRequest)?;
    state
        .auth_backend()
        .create_first_admin(login, password)
        .await
        .map_err(map_auth_error)?;

    Ok(Json(BootstrapStatusResponse {
        data: BootstrapStatusData {
            setup_required: false,
        },
        meta: ResponseMeta {
            request_id: context.request_id().to_string(),
        },
    }))
}

/// Anonymous setup is protected by browser provenance when those headers are
/// present. No setup secret is invented: deployment/network exposure remains
/// the documented first-run trust assumption until the one-time state closes.
pub(crate) async fn bootstrap_boundary(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(error) = csrf::validate_provenance(&state, &request) {
        return error.into_response();
    }
    next.run(request).await
}

pub(crate) async fn current_session(
    Extension(context): Extension<AuthContext>,
    Extension(request_context): Extension<RequestContext>,
) -> Json<AuthSessionResponse> {
    Json(AuthSessionResponse {
        data: current_session_data(context.principal()),
        meta: ResponseMeta {
            request_id: request_context.request_id().to_string(),
        },
    })
}

pub(crate) async fn issue_csrf(
    State(state): State<ApiState>,
    Extension(context): Extension<AuthContext>,
    Extension(request_context): Extension<RequestContext>,
) -> Response {
    let csrf_token = state.csrf_key().issue(context.token());
    let mut response = Json(CsrfResponse {
        data: CsrfData {
            csrf_token: csrf_token.clone(),
        },
        meta: ResponseMeta {
            request_id: request_context.request_id().to_string(),
        },
    })
    .into_response();
    cookies::append_csrf_cookie(response.headers_mut(), &csrf_token, state.cookie_config());
    response
}

pub(crate) async fn logout(
    State(state): State<ApiState>,
    Extension(context): Extension<LogoutContext>,
) -> Response {
    if let Some(context) = context.0 {
        match state
            .auth_backend()
            .revoke_current_session(context.token())
            .await
        {
            Ok(()) | Err(AuthError::InvalidSession | AuthError::CredentialNotFound) => {}
            Err(error) => {
                let mut response = map_auth_error(error).into_response();
                cookies::append_clear_cookies(response.headers_mut(), state.cookie_config());
                return response;
            }
        }
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    cookies::append_clear_cookies(response.headers_mut(), state.cookie_config());
    response
}

pub(crate) async fn require_authentication(
    State(state): State<ApiState>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = match session_token_from_request(&request) {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };
    let context = match resolve_auth(&state, &token).await {
        Ok(context) => context,
        Err(error) => return error.into_response(),
    };
    request.extensions_mut().insert(context);
    next.run(request).await
}

/// Enforce the browser CSRF contract for authenticated state-changing
/// requests. Safe reads remain usable without a CSRF proof.
pub(crate) async fn require_csrf_for_mutations(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(
        request.method(),
        &axum::http::Method::POST
            | &axum::http::Method::PUT
            | &axum::http::Method::PATCH
            | &axum::http::Method::DELETE
    ) {
        let Some(context) = request.extensions().get::<AuthContext>() else {
            return ApiError::Unauthorized.into_response();
        };
        if let Err(error) = csrf::validate_request(&state, &request, context) {
            return error.into_response();
        }
    }
    next.run(request).await
}

/// Logout is intentionally tolerant of an absent/expired/revoked cookie, but
/// a currently valid session still requires the same CSRF/provenance boundary
/// as every other authenticated state change.
pub(crate) async fn logout_boundary(
    State(state): State<ApiState>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(raw_token) = cookies::read_cookie(request.headers(), cookies::SESSION_COOKIE_NAME)
    else {
        request.extensions_mut().insert(LogoutContext(None));
        return next.run(request).await;
    };
    let Ok(token) = SessionToken::try_from_hex(&raw_token) else {
        request.extensions_mut().insert(LogoutContext(None));
        return next.run(request).await;
    };

    let principal = match state.auth_backend().authenticate_session(&token).await {
        Ok(principal) => principal,
        Err(
            AuthError::InvalidSession
            | AuthError::InvalidCredentials
            | AuthError::CredentialNotFound,
        ) => {
            request.extensions_mut().insert(LogoutContext(None));
            return next.run(request).await;
        }
        Err(error) => {
            let mut response = map_auth_error(error).into_response();
            cookies::append_clear_cookies(response.headers_mut(), state.cookie_config());
            return response;
        }
    };
    let context = AuthContext::new(token, principal);
    if let Err(error) = csrf::validate_request(&state, &request, &context) {
        return error.into_response();
    }
    request
        .extensions_mut()
        .insert(LogoutContext(Some(context)));
    next.run(request).await
}

fn session_token_from_request(request: &Request) -> Result<SessionToken, ApiError> {
    let raw_token = cookies::read_cookie(request.headers(), cookies::SESSION_COOKIE_NAME)
        .ok_or(ApiError::Unauthorized)?;
    SessionToken::try_from_hex(&raw_token).map_err(|_| ApiError::Unauthorized)
}

async fn resolve_auth(state: &ApiState, token: &SessionToken) -> Result<AuthContext, ApiError> {
    let principal = state
        .auth_backend()
        .authenticate_session(token)
        .await
        .map_err(map_auth_error)?;
    Ok(AuthContext::new(token.clone(), principal))
}

fn issued_session(session: AuthenticatedSession) -> IssuedSession {
    IssuedSession::new(
        session.token().clone(),
        session.principal(),
        session.expires_at(),
    )
}

fn session_data(principal: SessionPrincipal, expires_at: SessionExpiry) -> AuthSessionData {
    AuthSessionData {
        authenticated: true,
        user_id: principal.user_id().into_uuid().to_string(),
        is_instance_admin: principal.is_instance_admin(),
        session_id: principal.session_id().to_string(),
        expires_at: Some(expires_at.expires_at().to_string()),
    }
}

fn current_session_data(principal: SessionPrincipal) -> AuthSessionData {
    AuthSessionData {
        authenticated: true,
        user_id: principal.user_id().into_uuid().to_string(),
        is_instance_admin: principal.is_instance_admin(),
        session_id: principal.session_id().to_string(),
        expires_at: None,
    }
}

fn unavailable_error() -> AuthError {
    AuthError::Persistence(synveil_metadata::DatabaseError::Failure(
        synveil_metadata::DatabaseErrorKind::ConnectionUnavailable,
    ))
}
