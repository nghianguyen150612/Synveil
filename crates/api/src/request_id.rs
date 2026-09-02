use std::{fmt, time::SystemTime};

use axum::http::{HeaderMap, HeaderValue, header::HeaderName};
use uuid::Uuid;

/// The public correlation header used by every Synveil HTTP response.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// A validated, opaque diagnostic request identifier.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RequestId(String);

impl RequestId {
    /// Parse a client-provided identifier only when it is safe to echo and log.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        headers
            .get(&REQUEST_ID_HEADER)
            .and_then(Self::from_header_value)
    }

    /// Generate a server-owned identifier when no safe client hint exists.
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_header_value(value: &HeaderValue) -> Option<Self> {
        let value = value.to_str().ok()?;
        if !(8..=128).contains(&value.len())
            // Correlation hints are logged. Reserve machine-secret prefixes,
            // including tokens accidentally pasted inside a longer hint.
            || value.contains("svd1_")
            || value.contains("sve1_")
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._~-".contains(&byte))
        {
            return None;
        }

        Some(Self(value.to_owned()))
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Correlation data attached to a request by the transport middleware.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestContext {
    request_id: RequestId,
    trace_id: RequestId,
    received_at: SystemTime,
}

impl RequestContext {
    #[must_use]
    pub fn new(request_id: RequestId) -> Self {
        Self {
            request_id,
            trace_id: RequestId::generate(),
            received_at: SystemTime::now(),
        }
    }

    #[must_use]
    pub fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    #[must_use]
    pub fn trace_id(&self) -> &RequestId {
        &self.trace_id
    }

    #[must_use]
    pub fn received_at(&self) -> SystemTime {
        self.received_at
    }
}
