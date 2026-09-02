//! Session-bound signed double-submit CSRF tokens.

use std::fmt;

use axum::{extract::Request, http::header::HeaderName};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use synveil_auth::{SESSION_TOKEN_BYTES, SessionToken};

use crate::{ApiError, ApiState, auth::AuthContext, cookies};

type HmacSha256 = Hmac<Sha256>;

const CSRF_NONCE_BYTES: usize = 32;
const CSRF_TAG_BYTES: usize = 32;
const CSRF_RAW_BYTES: usize = CSRF_NONCE_BYTES + CSRF_TAG_BYTES;
const CSRF_HEX_BYTES: usize = CSRF_RAW_BYTES * 2;

pub const CSRF_HEADER_NAME: HeaderName = HeaderName::from_static("x-csrf-token");

/// Per-application signing key for session-bound CSRF proofs.
#[derive(Clone)]
pub struct CsrfKey([u8; SESSION_TOKEN_BYTES]);

impl CsrfKey {
    #[must_use]
    pub fn generate() -> Self {
        let token = SessionToken::generate();
        Self(*token.as_bytes())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; SESSION_TOKEN_BYTES]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn issue(&self, session_token: &SessionToken) -> String {
        let nonce_token = SessionToken::generate();
        let nonce = &nonce_token.as_bytes()[..CSRF_NONCE_BYTES];
        let tag = self.sign(session_token, nonce);
        let mut raw = [0_u8; CSRF_RAW_BYTES];
        raw[..CSRF_NONCE_BYTES].copy_from_slice(nonce);
        raw[CSRF_NONCE_BYTES..].copy_from_slice(&tag);
        encode_hex(&raw)
    }

    #[must_use]
    pub fn verify(&self, session_token: &SessionToken, candidate: &str) -> bool {
        if candidate.len() != CSRF_HEX_BYTES {
            return false;
        }
        let Some(raw) = decode_hex(candidate) else {
            return false;
        };
        let expected = self.sign(session_token, &raw[..CSRF_NONCE_BYTES]);
        expected.as_slice().ct_eq(&raw[CSRF_NONCE_BYTES..]).into()
    }

    fn sign(&self, session_token: &SessionToken, nonce: &[u8]) -> [u8; CSRF_TAG_BYTES] {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts a 256-bit key");
        mac.update(b"synveil/csrf/v1\0");
        mac.update(session_token.as_bytes());
        mac.update(nonce);
        let bytes = mac.finalize().into_bytes();
        let mut tag = [0_u8; CSRF_TAG_BYTES];
        tag.copy_from_slice(&bytes);
        tag
    }
}

impl Default for CsrfKey {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Debug for CsrfKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CsrfKey([REDACTED])")
    }
}

/// Validate request provenance and the session-bound header/cookie pair.
pub(crate) fn validate_request(
    state: &ApiState,
    request: &Request,
    auth: &AuthContext,
) -> Result<(), ApiError> {
    validate_provenance(state, request)?;

    let cookie = cookies::read_cookie(request.headers(), cookies::CSRF_COOKIE_NAME)
        .ok_or(ApiError::Forbidden)?;
    let header = request
        .headers()
        .get(CSRF_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Forbidden)?;

    if cookie.as_bytes().ct_eq(header.as_bytes()).unwrap_u8() != 1
        || !state.csrf_key().verify(auth.token(), header)
    {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

pub(crate) fn validate_provenance(state: &ApiState, request: &Request) -> Result<(), ApiError> {
    if let Some(fetch_site) = request.headers().get("sec-fetch-site") {
        let fetch_site = fetch_site.to_str().map_err(|_| ApiError::Forbidden)?.trim();
        if !fetch_site.eq_ignore_ascii_case("same-origin") {
            return Err(ApiError::Forbidden);
        }
    }

    let Some(origin) = request.headers().get("origin") else {
        return Ok(());
    };
    let origin = origin.to_str().map_err(|_| ApiError::Forbidden)?.trim();
    if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
        return Err(ApiError::Forbidden);
    }

    if let Some(allowed_origin) = state.allowed_origin() {
        if origin != allowed_origin {
            return Err(ApiError::Forbidden);
        }
        return Ok(());
    }

    // Without an explicitly configured public origin, an Origin header is
    // accepted only with the browser's same-origin fetch provenance. This
    // keeps the default CORS posture fail-closed for deployments that have not
    // yet supplied their canonical external origin.
    if request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("same-origin"))
    {
        return Ok(());
    }

    Err(ApiError::Forbidden)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        if pair.len() != 2 {
            return None;
        }
        let high = decode_hex_nibble(pair[0])?;
        let low = decode_hex_nibble(pair[1])?;
        decoded.push((high << 4) | low);
    }
    (decoded.len() == CSRF_RAW_BYTES).then_some(decoded)
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

#[cfg(test)]
mod tests {
    use super::CsrfKey;
    use synveil_auth::SessionToken;

    #[test]
    fn csrf_proof_is_random_session_bound_and_non_reusable_across_sessions() {
        let key = CsrfKey::from_bytes([0x42; 32]);
        let first = SessionToken::from_bytes([0x11; 32]);
        let second = SessionToken::from_bytes([0x12; 32]);
        let proof = key.issue(&first);

        assert_eq!(proof.len(), 128);
        assert!(key.verify(&first, &proof));
        assert!(!key.verify(&second, &proof));
        assert!(!key.verify(&first, &proof[..proof.len() - 2]));
        assert_ne!(proof, key.issue(&first));
    }
}
