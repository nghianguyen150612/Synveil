use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};
use tracing::{Span, info};

use crate::{
    ApiError, ApiState, FILE_METADATA_BODY_LIMIT_BYTES, UPLOAD_JSON_BODY_LIMIT_BYTES, auth, files,
    health, middleware, uploads,
};

/// Stable product API prefix for versioned resources.
pub const API_VERSION_PREFIX: &str = "/api/v1";

/// Conservative default for the transport foundation. Feature endpoints must
/// apply stricter operation-specific limits when they are introduced.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;
pub const LOGIN_BODY_LIMIT_BYTES: usize = auth::LOGIN_BODY_LIMIT_BYTES;
pub const BOOTSTRAP_BODY_LIMIT_BYTES: usize = auth::BOOTSTRAP_BODY_LIMIT_BYTES;

/// Construct the Axum application router.
pub fn router(state: ApiState) -> Router {
    let upload_chunk_limit =
        usize::try_from(state.upload_backend().max_chunk_size()).unwrap_or(usize::MAX);
    let public_auth = Router::new()
        .route("/auth/login", post(auth::login))
        .layer(RequestBodyLimitLayer::new(LOGIN_BODY_LIMIT_BYTES));
    let bootstrap_status =
        Router::new().route("/system/bootstrap-status", get(auth::bootstrap_status));
    let bootstrap_admin = Router::new()
        .route("/bootstrap/admin", post(auth::create_first_admin))
        .layer(RequestBodyLimitLayer::new(BOOTSTRAP_BODY_LIMIT_BYTES))
        .layer(from_fn_with_state(state.clone(), auth::bootstrap_boundary))
        .with_state(state.clone());
    let protected_auth = Router::new()
        .route("/auth/session", get(auth::current_session))
        .route("/auth/csrf", get(auth::issue_csrf))
        .layer(from_fn_with_state(
            state.clone(),
            auth::require_authentication,
        ))
        .with_state(state.clone());
    let logout = Router::new()
        .route("/auth/logout", post(auth::logout))
        .layer(from_fn_with_state(state.clone(), auth::logout_boundary))
        .with_state(state.clone());
    let protected_files = Router::new()
        .route("/libraries", get(files::list_libraries))
        .route(
            "/libraries/{library_id}/nodes",
            get(files::list_children).post(files::create_directory),
        )
        .route(
            "/nodes/{node_id}",
            get(files::get_node).patch(files::update_node),
        )
        .route("/nodes/{node_id}/trash", post(files::delete_node))
        .route("/nodes/{node_id}/restore", post(files::restore_node))
        .layer(RequestBodyLimitLayer::new(FILE_METADATA_BODY_LIMIT_BYTES))
        // The authentication layer is added last so it runs first on the
        // request and installs AuthContext before the CSRF layer evaluates a
        // state-changing method.
        .layer(from_fn_with_state(
            state.clone(),
            auth::require_csrf_for_mutations,
        ))
        .layer(from_fn_with_state(
            state.clone(),
            auth::require_authentication,
        ))
        .with_state(state.clone());
    let create_upload = Router::new()
        .route("/upload-sessions", post(uploads::create_upload_session))
        .layer(RequestBodyLimitLayer::new(UPLOAD_JSON_BODY_LIMIT_BYTES));
    let upload_session_operations = Router::new()
        .route(
            "/upload-sessions/{upload_session_id}",
            get(uploads::get_upload_session).patch(uploads::append_upload_chunk),
        )
        .route(
            "/upload-sessions/{upload_session_id}/complete",
            post(uploads::complete_upload),
        )
        .route(
            "/upload-sessions/{upload_session_id}/abort",
            post(uploads::abort_upload),
        )
        // This streaming guard is derived from the authoritative application
        // configuration. The service independently counts frames so alternate
        // transports cannot bypass the same aggregate limit.
        .layer(RequestBodyLimitLayer::new(upload_chunk_limit));
    let protected_uploads = Router::new()
        .merge(create_upload)
        .merge(upload_session_operations)
        // Authentication is installed last so it supplies AuthContext before
        // CSRF evaluates upload mutations. GET status remains CSRF-free.
        .layer(from_fn_with_state(
            state.clone(),
            auth::require_csrf_for_mutations,
        ))
        .layer(from_fn_with_state(
            state.clone(),
            auth::require_authentication,
        ))
        .with_state(state.clone());
    let api = Router::new()
        .route("/system/health", get(health::system_health))
        .merge(public_auth)
        .merge(bootstrap_status)
        .merge(bootstrap_admin)
        .merge(protected_auth)
        .merge(logout);
    // Retain the conservative global limit for all existing endpoints. The
    // upload PATCH route is merged afterwards because its body is streamed and
    // bounded by the upload application's configured chunk limit.
    let api = api
        .merge(protected_files)
        .layer(RequestBodyLimitLayer::new(state.body_limit_bytes()))
        .merge(protected_uploads);

    Router::new()
        // Requested short probe names and the OpenAPI blueprint's canonical
        // aliases are intentionally unversioned deployment signals.
        .route("/live", get(health::live))
        .route("/ready", get(health::ready))
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .nest(crate::API_VERSION_PREFIX, api)
        .fallback(not_found)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let context = request.extensions().get::<crate::RequestContext>();
                    let request_id = context
                        .map(|context| context.request_id().to_string())
                        .unwrap_or_else(|| "unassigned".to_owned());
                    let trace_id = context
                        .map(|context| context.trace_id().to_string())
                        .unwrap_or_else(|| "unassigned".to_owned());
                    tracing::info_span!(
                        "http.transport",
                        request_id = %request_id,
                        trace_id = %trace_id,
                        method = %request.method(),
                        path = %request.uri().path(),
                        status_code = tracing::field::Empty,
                    )
                })
                .on_request(|request: &axum::http::Request<_>, span: &Span| {
                    info!(
                        parent: span,
                        method = %request.method(),
                        path = %request.uri().path(),
                        "request started"
                    );
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     span: &Span| {
                        info!(
                            parent: span,
                            status = %response.status(),
                            latency_ms = latency.as_millis(),
                            "request completed"
                        );
                    },
                ),
        )
        .layer(axum::middleware::from_fn(middleware::request_context))
        .with_state(state)
}

async fn not_found() -> ApiError {
    ApiError::from(synveil_core::ErrorCode::NotFound)
}
