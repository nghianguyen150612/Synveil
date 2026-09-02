use axum::{
    Json,
    extract::{Extension, State},
    http::HeaderMap,
};
use serde::Serialize;
use synveil_platform::{HealthInfo, HealthState};

use crate::{ApiError, ApiState, RequestContext};

#[derive(Debug, Serialize)]
pub(crate) struct LiveProbe {
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReadyProbe {
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct SystemHealthResponse {
    data: SystemHealthData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct SystemHealthData {
    status: &'static str,
    liveness: bool,
    readiness: bool,
    components: Vec<ComponentHealthResponse>,
}

#[derive(Debug, Serialize)]
struct ComponentHealthResponse {
    name: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ResponseMeta {
    request_id: String,
}

pub(crate) async fn live() -> Json<LiveProbe> {
    Json(LiveProbe { status: "live" })
}

pub(crate) async fn ready(State(state): State<ApiState>) -> Result<Json<ReadyProbe>, ApiError> {
    if state.readiness_snapshot().is_ready() {
        Ok(Json(ReadyProbe { status: "ready" }))
    } else {
        Err(ApiError::ReadinessUnavailable)
    }
}

pub(crate) async fn system_health(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    headers: HeaderMap,
) -> Result<Json<SystemHealthResponse>, ApiError> {
    if !state.health_authorized(&headers) {
        return Err(ApiError::Unauthorized);
    }

    let platform_health = state.health();
    let readiness = state.readiness_snapshot().is_ready();
    let status = public_overall_status(&platform_health, readiness);
    let components = platform_health
        .components()
        .iter()
        .map(|component| ComponentHealthResponse {
            name: component.component().as_str(),
            status: component.state().as_str(),
        })
        .collect();

    Ok(Json(SystemHealthResponse {
        data: SystemHealthData {
            status,
            liveness: platform_health.liveness(),
            readiness,
            components,
        },
        meta: ResponseMeta {
            request_id: context.request_id().to_string(),
        },
    }))
}

fn public_overall_status(health: &HealthInfo, readiness: bool) -> &'static str {
    if !health.liveness() {
        return HealthState::Unavailable.as_str();
    }
    if !readiness {
        return HealthState::Degraded.as_str();
    }
    health.overall().as_str()
}
