//! Narrow browser-cookie transport helpers.
//!
//! Cookie parsing and serialization intentionally live at the API boundary.
//! The authentication crate never sees an HTTP header, cookie attribute, or
//! raw cookie presentation string.

use axum::http::{HeaderMap, HeaderValue, header::SET_COOKIE};

use synveil_auth::SessionToken;

pub const SESSION_COOKIE_NAME: &str = "synveil_session";
pub const CSRF_COOKIE_NAME: &str = "synveil_csrf";

const COOKIE_PATH: &str = "/";
const COOKIE_EXPIRY: &str = "Thu, 01 Jan 1970 00:00:00 GMT";

/// The browser-compatible SameSite policy chosen for Synveil session cookies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CookieSameSite {
    Lax,
    Strict,
}

impl CookieSameSite {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Lax => "Lax",
            Self::Strict => "Strict",
        }
    }
}

/// Explicit cookie policy. There is deliberately no Domain field: the
/// session and CSRF cookies are host-only and always use Path=/.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CookieConfig {
    secure: bool,
    same_site: CookieSameSite,
}

impl CookieConfig {
    #[must_use]
    pub const fn new(secure: bool, same_site: CookieSameSite) -> Self {
        Self { secure, same_site }
    }

    #[must_use]
    pub const fn production() -> Self {
        Self::new(true, CookieSameSite::Lax)
    }

    /// Explicit local-development policy for an isolated HTTP-only loopback
    /// deployment. A composition root must opt into this policy; production
    /// defaults remain Secure.
    #[must_use]
    pub const fn development() -> Self {
        Self::new(false, CookieSameSite::Lax)
    }

    #[must_use]
    pub const fn secure(self) -> bool {
        self.secure
    }

    #[must_use]
    pub const fn same_site(self) -> CookieSameSite {
        self.same_site
    }
}

impl Default for CookieConfig {
    fn default() -> Self {
        Self::production()
    }
}

pub(crate) fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut found = None;

    for header in headers.get_all("cookie") {
        let value = header.to_str().ok()?;
        for pair in value.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let (candidate_name, candidate_value) = pair.split_once('=')?;
            if candidate_name.trim() != name {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(candidate_value.trim().to_owned());
        }
    }

    found
}

pub(crate) fn append_session_cookie(
    headers: &mut HeaderMap,
    token: &SessionToken,
    config: CookieConfig,
) {
    append_cookie(headers, SESSION_COOKIE_NAME, &token.to_hex(), true, config);
}

pub(crate) fn append_csrf_cookie(headers: &mut HeaderMap, token: &str, config: CookieConfig) {
    append_cookie(headers, CSRF_COOKIE_NAME, token, false, config);
}

pub(crate) fn append_clear_cookies(headers: &mut HeaderMap, config: CookieConfig) {
    append_clear_cookie(headers, SESSION_COOKIE_NAME, true, config);
    append_clear_cookie(headers, CSRF_COOKIE_NAME, false, config);
}

fn append_cookie(
    headers: &mut HeaderMap,
    name: &str,
    value: &str,
    http_only: bool,
    config: CookieConfig,
) {
    let mut cookie = format!(
        "{name}={value}; Path={COOKIE_PATH}; SameSite={}",
        config.same_site.as_str()
    );
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if config.secure {
        cookie.push_str("; Secure");
    }
    headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("generated cookie attributes are valid headers"),
    );
}

fn append_clear_cookie(headers: &mut HeaderMap, name: &str, http_only: bool, config: CookieConfig) {
    let mut cookie = format!(
        "{name}=; Max-Age=0; Expires={COOKIE_EXPIRY}; Path={COOKIE_PATH}; SameSite={}",
        config.same_site.as_str()
    );
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if config.secure {
        cookie.push_str("; Secure");
    }
    headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("generated cookie attributes are valid headers"),
    );
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::{CSRF_COOKIE_NAME, CookieConfig, CookieSameSite, SESSION_COOKIE_NAME, read_cookie};
    use synveil_auth::SessionToken;

    #[test]
    fn cookie_policy_is_explicit_and_host_only() {
        assert!(CookieConfig::production().secure());
        assert!(!CookieConfig::development().secure());
        assert_eq!(CookieConfig::production().same_site(), CookieSameSite::Lax);

        let token = SessionToken::from_bytes([0x11; 32]);
        let mut headers = HeaderMap::new();
        super::append_session_cookie(&mut headers, &token, CookieConfig::production());
        let value = headers.get("set-cookie").unwrap().to_str().unwrap();
        assert!(value.contains(SESSION_COOKIE_NAME));
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("Secure"));
        assert!(!value.contains("Domain="));
    }

    #[test]
    fn cookie_reader_rejects_duplicate_cookie_names() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "synveil_csrf=one; other=x".parse().unwrap());
        headers.append("cookie", "synveil_csrf=two".parse().unwrap());
        assert_eq!(read_cookie(&headers, CSRF_COOKIE_NAME), None);
    }
}
