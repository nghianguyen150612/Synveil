use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{
        HeaderValue, StatusCode,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::from_slice;
use tracing::{Instrument, info_span};

use crate::{ApiError, ErrorResponse, RequestContext, RequestId, request_id::REQUEST_ID_HEADER};

const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Attach safe request/trace correlation and normalize all public error bodies.
pub(crate) async fn request_context(mut request: Request, next: Next) -> Response {
    let request_id = RequestId::from_headers(request.headers()).unwrap_or_default();
    let path = request.uri().path();
    let is_auth_route = path.starts_with("/api/v1/auth/");
    let is_no_store_route = is_auth_route
        || path == "/api/v1/system/bootstrap-status"
        || path == "/api/v1/bootstrap/admin"
        || path.starts_with("/api/v1/upload-sessions")
        || ((path.starts_with("/api/v1/nodes/") || path.starts_with("/api/v1/versions/"))
            && path.ends_with("/content"));
    let context = RequestContext::new(request_id.clone());
    request.extensions_mut().insert(context.clone());

    let span = info_span!(
        "http.request",
        request_id = %context.request_id(),
        trace_id = %context.trace_id(),
        method = %request.method(),
        path = %request.uri().path(),
        status_code = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
    );
    let started = std::time::Instant::now();
    let response = next.run(request).instrument(span.clone()).await;
    let mut response = normalize_error_response(response, &request_id).await;
    let latency = started.elapsed();

    span.record("status_code", tracing::field::display(response.status()));
    span.record("latency_ms", tracing::field::display(latency.as_millis()));
    tracing::info!(parent: &span, "request completed");

    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(request_id.as_str()).expect("validated request id is a header value"),
    );
    if is_no_store_route
        && !response
            .headers()
            .contains_key(axum::http::header::CACHE_CONTROL)
    {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        );
    }
    if is_auth_route {
        response
            .headers_mut()
            .insert(axum::http::header::VARY, HeaderValue::from_static("Cookie"));
    }
    response
}

async fn normalize_error_response(response: Response, request_id: &RequestId) -> Response {
    let status = response.status();
    let response = if status == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::PayloadTooLarge.into_response()
    } else {
        response
    };

    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, MAX_ERROR_BODY_BYTES).await else {
        return error_with_request_id(ApiError::Internal, request_id);
    };

    let Ok(mut envelope) = from_slice::<ErrorResponse>(&bytes) else {
        let fallback = if status.is_client_error() {
            ApiError::InvalidRequest
        } else {
            ApiError::Internal
        };
        return error_with_request_id(fallback, request_id);
    };

    envelope.error.request_id = request_id.to_string();
    let serialized = match serde_json::to_vec(&envelope) {
        Ok(serialized) => serialized,
        Err(_) => return error_with_request_id(ApiError::Internal, request_id),
    };
    parts.headers.remove(CONTENT_LENGTH);
    parts.headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    Response::from_parts(parts, Body::from(serialized))
}

fn error_with_request_id(error: ApiError, request_id: &RequestId) -> Response {
    let mut response = error.into_response();
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(request_id.as_str()).expect("validated request id is a header value"),
    );
    response
}
