//! Device enrollment transport over the existing authentication boundary.
//!
//! Browser ownership and CSRF authorize grant creation/revocation. The one-time
//! enrollment secret alone authorizes exchange. Neither secret is a cookie,
//! neither response is cacheable, and no request/response DTO implements Debug.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use synveil_auth::{
    DeviceAuthError, DeviceAuthenticationService, DeviceCredentialPrincipal,
    DeviceEnrollmentTarget, IssuedDeviceCredential, IssuedEnrollmentGrant,
};
use synveil_core::{
    DeviceCredentialId, DeviceCredentialSecret, DeviceId, EnrollmentSecret, LogicalName, UserId,
};
use synveil_metadata::DatabasePool;

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthContext, ResponseMeta},
};

pub const DEVICE_ENROLLMENT_BODY_LIMIT_BYTES: usize = 2 * 1024;

/// An application port, not an alternate auth framework. All issuance,
/// lifecycle, hashing, transactional consumption and revocation live in the
/// existing auth/metadata layers.
#[async_trait]
pub trait DeviceAuthenticationBackend: Send + Sync {
    async fn create_grant(
        &self,
        owner: UserId,
        target: DeviceEnrollmentTarget,
    ) -> Result<IssuedEnrollmentGrant, DeviceAuthError>;
    async fn exchange(
        &self,
        token: &EnrollmentSecret,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError>;
    async fn authenticate(
        &self,
        secret: &DeviceCredentialSecret,
    ) -> Result<DeviceCredentialPrincipal, DeviceAuthError>;
    async fn revoke_credential(
        &self,
        owner: UserId,
        device: DeviceId,
        credential: DeviceCredentialId,
    ) -> Result<(), DeviceAuthError>;
    async fn revoke_all_credentials(
        &self,
        owner: UserId,
        device: DeviceId,
    ) -> Result<(), DeviceAuthError>;
}

pub struct PostgresDeviceAuthenticationBackend {
    pool: Arc<DatabasePool>,
}

impl PostgresDeviceAuthenticationBackend {
    #[must_use]
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeviceAuthenticationBackend for PostgresDeviceAuthenticationBackend {
    async fn create_grant(
        &self,
        owner: UserId,
        target: DeviceEnrollmentTarget,
    ) -> Result<IssuedEnrollmentGrant, DeviceAuthError> {
        DeviceAuthenticationService::new(&self.pool)
            .create_grant(owner, target)
            .await
    }

    async fn exchange(
        &self,
        token: &EnrollmentSecret,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError> {
        DeviceAuthenticationService::new(&self.pool)
            .exchange(token)
            .await
    }

    async fn authenticate(
        &self,
        secret: &DeviceCredentialSecret,
    ) -> Result<DeviceCredentialPrincipal, DeviceAuthError> {
        DeviceAuthenticationService::new(&self.pool)
            .authenticate(secret)
            .await
    }

    async fn revoke_credential(
        &self,
        owner: UserId,
        device: DeviceId,
        credential: DeviceCredentialId,
    ) -> Result<(), DeviceAuthError> {
        DeviceAuthenticationService::new(&self.pool)
            .revoke_credential(owner, device, credential)
            .await
    }

    async fn revoke_all_credentials(
        &self,
        owner: UserId,
        device: DeviceId,
    ) -> Result<(), DeviceAuthError> {
        DeviceAuthenticationService::new(&self.pool)
            .revoke_all_credentials(owner, device)
            .await
    }
}

pub(crate) struct UnavailableDeviceAuthenticationBackend;

#[async_trait]
impl DeviceAuthenticationBackend for UnavailableDeviceAuthenticationBackend {
    async fn create_grant(
        &self,
        _owner: UserId,
        _target: DeviceEnrollmentTarget,
    ) -> Result<IssuedEnrollmentGrant, DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
    async fn exchange(
        &self,
        _token: &EnrollmentSecret,
    ) -> Result<IssuedDeviceCredential, DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
    async fn authenticate(
        &self,
        _secret: &DeviceCredentialSecret,
    ) -> Result<DeviceCredentialPrincipal, DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
    async fn revoke_credential(
        &self,
        _owner: UserId,
        _device: DeviceId,
        _credential: DeviceCredentialId,
    ) -> Result<(), DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
    async fn revoke_all_credentials(
        &self,
        _owner: UserId,
        _device: DeviceId,
    ) -> Result<(), DeviceAuthError> {
        Err(DeviceAuthError::Unavailable)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateGrantRequest {
    target: GrantTarget,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum GrantTarget {
    Existing { device_id: String },
    New { display_name: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExchangeRequest {
    enrollment_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RevokeRequest {}

#[derive(Serialize)]
struct GrantResponse {
    data: GrantData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct GrantData {
    grant_id: String,
    owner_user_id: String,
    device_id: String,
    enrollment_token: String,
    created_at: String,
    expires_at: String,
}

#[derive(Serialize)]
struct ExchangeResponse {
    data: CredentialData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct CredentialData {
    owner_user_id: String,
    device_id: String,
    credential_id: String,
    device_credential: String,
    created_at: String,
}

pub(crate) async fn create_grant(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateGrantRequest>,
) -> Result<Response, ApiError> {
    let target = match payload.target {
        GrantTarget::Existing { device_id } => DeviceEnrollmentTarget::Existing(
            device_id.parse().map_err(|_| ApiError::InvalidRequest)?,
        ),
        GrantTarget::New { display_name } => DeviceEnrollmentTarget::New(
            LogicalName::new(display_name).map_err(|_| ApiError::InvalidRequest)?,
        ),
    };
    let grant = state
        .device_auth_backend()
        .create_grant(auth.principal().user_id(), target)
        .await
        .map_err(map_device_auth_error)?;
    Ok((
        StatusCode::CREATED,
        Json(GrantResponse {
            data: GrantData {
                grant_id: grant.grant_id.to_string(),
                owner_user_id: grant.owner_user_id.to_string(),
                device_id: grant.device_id.to_string(),
                enrollment_token: grant.token.expose_secret().to_owned(),
                created_at: grant.created_at.to_string(),
                expires_at: grant.expires_at.to_string(),
            },
            meta: ResponseMeta {
                request_id: context.request_id().to_string(),
            },
        }),
    )
        .into_response())
}

/// Once-only exchange. A transport failure is deliberately NOT replay-safe:
/// the owner must revoke all credentials/outstanding grants for this Device,
/// create a fresh grant, and enroll again. No secret can be recovered from DB.
pub(crate) async fn exchange(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<ExchangeRequest>,
) -> Result<Response, ApiError> {
    let token = EnrollmentSecret::parse(&payload.enrollment_token)
        .map_err(|_| ApiError::InvalidEnrollment)?;
    let credential = state
        .device_auth_backend()
        .exchange(&token)
        .await
        .map_err(map_device_auth_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ExchangeResponse {
            data: CredentialData {
                owner_user_id: credential.owner_user_id.to_string(),
                device_id: credential.device_id.to_string(),
                credential_id: credential.credential_id.to_string(),
                device_credential: credential.secret.expose_secret().to_owned(),
                created_at: credential.created_at.to_string(),
            },
            meta: ResponseMeta {
                request_id: context.request_id().to_string(),
            },
        }),
    )
        .into_response())
}

pub(crate) async fn revoke_credential(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Path((device, credential)): Path<(String, String)>,
    Json(_payload): Json<RevokeRequest>,
) -> Result<StatusCode, ApiError> {
    let device = device.parse().map_err(|_| ApiError::InvalidRequest)?;
    let credential = credential.parse().map_err(|_| ApiError::InvalidRequest)?;
    state
        .device_auth_backend()
        .revoke_credential(auth.principal().user_id(), device, credential)
        .await
        .map_err(map_device_auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn revoke_all_credentials(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Path(device): Path<String>,
    Json(_payload): Json<RevokeRequest>,
) -> Result<StatusCode, ApiError> {
    let device = device.parse().map_err(|_| ApiError::InvalidRequest)?;
    state
        .device_auth_backend()
        .revoke_all_credentials(auth.principal().user_id(), device)
        .await
        .map_err(map_device_auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn map_device_auth_error(error: DeviceAuthError) -> ApiError {
    match error {
        DeviceAuthError::InvalidEnrollment => ApiError::InvalidEnrollment,
        DeviceAuthError::InvalidCredential => ApiError::Unauthorized,
        DeviceAuthError::DeviceRevoked => ApiError::DeviceRevoked,
        DeviceAuthError::DeviceNotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        DeviceAuthError::Unavailable => ApiError::ReadinessUnavailable,
        DeviceAuthError::InvalidPersistedData => ApiError::Internal,
    }
}
