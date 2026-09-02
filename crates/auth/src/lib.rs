#![forbid(unsafe_code)]

//! Platform-neutral password, browser-session, and first-run authentication
//! foundations.
//!
//! HTTP transport, cookies, CSRF, and browser recovery remain
//! outside this crate. PostgreSQL-specific persistence stays in
//! `synveil-metadata`.

mod config;
mod device_credentials;
mod errors;
mod passwords;
mod service;
mod sessions;

pub use config::{DEFAULT_SESSION_TTL, MAX_SESSION_TTL, SessionConfig, SessionConfigError};
pub use device_credentials::{
    DEVICE_ENROLLMENT_TTL_SECONDS, DeviceAuthError, DeviceAuthenticationService,
    DeviceCredentialPrincipal, DeviceEnrollmentTarget, IssuedDeviceCredential,
    IssuedEnrollmentGrant,
};
pub use errors::AuthError;
pub use passwords::{
    DEFAULT_PASSWORD_MEMORY_KIB, DEFAULT_PASSWORD_OUTPUT_BYTES, DEFAULT_PASSWORD_PARALLELISM,
    DEFAULT_PASSWORD_TIME_COST, MAX_PASSWORD_BYTES, PasswordError, PasswordHasherConfig,
    PasswordParameters, PasswordVerification, PlaintextPassword, StoredPasswordHash,
};
pub use service::AuthenticationService;
pub use sessions::{
    AuthenticatedSession, BrowserSession, SESSION_TOKEN_BYTES, SessionCredential, SessionExpiry,
    SessionExpiryError, SessionId, SessionIdError, SessionPrincipal, SessionToken,
    SessionTokenError,
};
pub use synveil_core::{DeviceCredentialSecret, EnrollmentSecret};
