use std::sync::Arc;

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header::CONTENT_LENGTH},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::Value;
use synveil_core::ErrorCode;
use tower::ServiceExt;

use super::{
    ApiError, ApiState, RequestId, StaticReadiness, SystemHealthAuthorizer, map_core_error, router,
};

struct TestHealthAuthorizer;

impl SystemHealthAuthorizer for TestHealthAuthorizer {
    fn authorize(&self, headers: &HeaderMap) -> bool {
        headers
            .get("x-test-health-access")
            .and_then(|value| value.to_str().ok())
            == Some("allow")
    }
}

fn state(ready: bool) -> ApiState {
    ApiState::from_current_platform().with_readiness(Arc::new(StaticReadiness::new(ready)))
}

fn request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .expect("test request must be valid")
}

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body must be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response must be JSON")
}

fn response_request_id(response: &Response) -> String {
    response
        .headers()
        .get("x-request-id")
        .expect("response must include a request ID")
        .to_str()
        .expect("request ID must be valid ASCII")
        .to_owned()
}

#[tokio::test]
async fn live_response_is_unconditional_and_correlated() {
    for path in ["/live", "/health/live"] {
        let response = router(state(false))
            .oneshot(request(Method::GET, path, Body::empty()))
            .await
            .expect("router must not fail");

        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response_request_id(&response);
        assert!(
            RequestId::from_headers(&HeaderMap::from_iter([(
                "x-request-id".parse().unwrap(),
                HeaderValue::from_str(&request_id).unwrap(),
            )]))
            .is_some()
        );
        assert_eq!(json_body(response).await["status"], "live");
    }
}

#[tokio::test]
async fn readiness_is_distinct_from_liveness() {
    let response = router(state(false))
        .oneshot(request(Method::GET, "/ready", Body::empty()))
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let request_id = response_request_id(&response);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "internal_dependency_unavailable");
    assert_eq!(body["error"]["request_id"], request_id);
    assert_eq!(body["error"]["retryable"], true);

    let response = router(state(true))
        .oneshot(request(Method::GET, "/ready", Body::empty()))
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "ready");
}

#[tokio::test]
async fn request_id_is_propagated_only_when_safe() {
    let mut propagated = request(Method::GET, "/missing", Body::empty());
    propagated
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("client.req-1234"));
    let response = router(state(true))
        .oneshot(propagated)
        .await
        .expect("router must not fail");
    assert_eq!(response_request_id(&response), "client.req-1234");
    assert_eq!(
        json_body(response).await["error"]["request_id"],
        "client.req-1234"
    );

    let mut unsafe_hint = request(Method::GET, "/missing", Body::empty());
    unsafe_hint
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("short"));
    let response = router(state(true))
        .oneshot(unsafe_hint)
        .await
        .expect("router must not fail");
    let request_id = response_request_id(&response);
    assert_ne!(request_id, "short");
    assert_eq!(json_body(response).await["error"]["request_id"], request_id);
}

#[tokio::test]
async fn not_found_uses_the_safe_error_envelope() {
    let mut request = request(Method::GET, "/not-a-route", Body::empty());
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("not-found-1"));
    let response = router(state(true))
        .oneshot(request)
        .await
        .expect("router must not fail");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["retryable"], false);
    assert_eq!(body["error"]["request_id"], "not-found-1");
    assert!(body["error"]["message"].as_str().unwrap().len() <= 512);
}

#[tokio::test]
async fn detailed_health_is_fail_closed_until_authorized() {
    let response = router(state(true))
        .oneshot(request(Method::GET, "/api/v1/system/health", Body::empty()))
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "authentication_failed"
    );

    let authorized_state =
        state(true).with_system_health_authorizer(Arc::new(TestHealthAuthorizer));
    let mut request = request(Method::GET, "/api/v1/system/health", Body::empty());
    request
        .headers_mut()
        .insert("x-test-health-access", HeaderValue::from_static("allow"));
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("health-1234"));
    let response = router(authorized_state)
        .oneshot(request)
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"]["liveness"], true);
    assert_eq!(body["data"]["readiness"], true);
    assert!(body["data"]["components"].is_array());
    assert_eq!(body["meta"]["request_id"], "health-1234");
    assert!(body.to_string().find("/").is_none());
}

#[tokio::test]
async fn body_limit_returns_the_public_error_envelope() {
    let mut request = request(Method::POST, "/api/v1/system/health", Body::from("12345"));
    request
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("5"));
    let response = router(state(true).with_body_limit(4))
        .oneshot(request)
        .await
        .expect("router must not fail");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "payload_too_large");
    assert_eq!(body["error"]["retryable"], false);
}

#[test]
fn core_errors_map_to_http_only_at_the_api_boundary() {
    assert_eq!(
        map_core_error(ErrorCode::NotFound).status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        map_core_error(ErrorCode::VersionConflict).status_code(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        map_core_error(ErrorCode::StorageUnavailable).status_code(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        ApiError::PayloadTooLarge.status_code(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}
