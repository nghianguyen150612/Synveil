//! Runtime database credential helper for systemd credential delivery.
//!
//! Production Linux service receives `DATABASE_URL` via `systemd` `LoadCredential=`
//! as a per-service file under `$CREDENTIALS_DIRECTORY/database-url`. This helper
//! provides a narrow, file-based boundary that reads the credential once, validates
//! it through the canonical `DatabaseConfig::from_url`, and never logs secret
//! contents.
//!
//! `crates/core` remains free of systemd concepts; this module lives at the
//! runtime edge (`crates/api`).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use synveil_metadata::{DatabaseConfig, DatabaseConfigError};

/// Env var that points at the delivered credential file via `Environment=%d/...`.
/// Non-secret — it carries the path, not the secret.
pub const CREDENTIAL_FILE_ENV: &str = "SYNVEIL_DATABASE_CREDENTIAL_FILE";

/// Systemd-supplied credentials directory (`$CREDENTIALS_DIRECTORY`).
pub const CREDENTIALS_DIRECTORY_ENV: &str = "CREDENTIALS_DIRECTORY";

/// Credential ID inside the credentials directory for Gen-1.
pub const CREDENTIAL_ID: &str = "database-url";

/// Maximum credential file size (8 KiB) for a database URL. Bounded to avoid
/// unbounded read into memory. Chosen because a PostgreSQL URL with credentials
/// is well under 1 KiB even with long host/db names; 8 KiB provides ample
/// margin without over-engineering streaming.
pub const MAX_CREDENTIAL_FILE_SIZE: usize = 8 * 1024;

/// Database credential load failures. Messages are intentionally generic and never
/// include the credential value or file contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeDatabaseCredentialError {
    MissingCredential,
    AmbiguousConfiguration,
    UnreadableCredentialFile,
    EmptyCredential,
    OversizedCredential { limit: usize },
    InvalidDatabaseUrl(DatabaseConfigError),
}

impl std::fmt::Display for RuntimeDatabaseCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredential => write!(f, "database credential is missing"),
            Self::AmbiguousConfiguration => write!(
                f,
                "database credential is ambiguous: both credential file and DATABASE_URL are set; use only one"
            ),
            Self::UnreadableCredentialFile => {
                write!(f, "database credential file is unreadable")
            }
            Self::EmptyCredential => write!(f, "database credential is empty"),
            Self::OversizedCredential { limit } => {
                write!(
                    f,
                    "database credential exceeds maximum size ({limit} bytes)"
                )
            }
            Self::InvalidDatabaseUrl(e) => write!(f, "database configuration is invalid: {e}"),
        }
    }
}

impl std::error::Error for RuntimeDatabaseCredentialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidDatabaseUrl(e) => Some(e),
            _ => None,
        }
    }
}

impl From<DatabaseConfigError> for RuntimeDatabaseCredentialError {
    fn from(e: DatabaseConfigError) -> Self {
        Self::InvalidDatabaseUrl(e)
    }
}

/// Determine the credential file path from runtime environment, if configured.
///
/// Precedence:
/// 1. `SYNVEIL_DATABASE_CREDENTIAL_FILE` (explicit non-secret path via `%d/...`)
/// 2. `$CREDENTIALS_DIRECTORY/database-url` (systemd auto-supplied directory)
///
/// Returns `None` if neither is set (caller may fallback to `DATABASE_URL`).
pub fn credential_file_path_from_env() -> Option<PathBuf> {
    if let Ok(val) = env::var(CREDENTIAL_FILE_ENV) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    if let Ok(dir) = env::var(CREDENTIALS_DIRECTORY_ENV) {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return Some(Path::new(trimmed).join(CREDENTIAL_ID));
        }
    }
    None
}

/// Whether `DATABASE_URL` is present in the environment (non-empty after trim).
fn database_url_env_present() -> bool {
    if let Ok(val) = env::var(synveil_metadata::DATABASE_URL_ENV) {
        return !val.trim().is_empty();
    }
    false
}

/// Load the raw database URL string from the runtime source without logging it.
///
/// Policy:
/// - If a credential file path is configured (explicit or `CREDENTIALS_DIRECTORY`),
///   it is the authoritative source. If `DATABASE_URL` is also present, this is
///   ambiguous and fails (do not silently combine).
/// - Otherwise, fallback to `DATABASE_URL` env for development/manual use.
/// - File reading is bounded by `MAX_CREDENTIAL_FILE_SIZE`, rejects empty/
///   whitespace-only, and normalizes a single trailing newline/CRLF sequence
///   (common for admin-managed secret files) without rewriting interior content.
pub fn load_database_url_from_runtime_source() -> Result<String, RuntimeDatabaseCredentialError> {
    let cred_path = credential_file_path_from_env();
    let db_url_present = database_url_env_present();

    match (cred_path, db_url_present) {
        (Some(path), true) => {
            // Ambiguous dual configuration — fail closed for operational clarity.
            // We intentionally do not read either value; we surface the policy.
            // Avoid logging either value.
            let _ = path; // path class is not secret but we avoid emitting it
            Err(RuntimeDatabaseCredentialError::AmbiguousConfiguration)
        }
        (Some(path), false) => {
            // Credential-file path is authoritative.
            load_database_url_from_file(&path)
        }
        (None, true) => {
            // Development fallback: DATABASE_URL env.
            // Reuse canonical validator without logging value.
            let url = env::var(synveil_metadata::DATABASE_URL_ENV)
                .map_err(|_| RuntimeDatabaseCredentialError::MissingCredential)?;
            // Trim before validation (DatabaseConfig::from_url also trims, but we
            // want empty check without logging).
            if url.trim().is_empty() {
                return Err(RuntimeDatabaseCredentialError::EmptyCredential);
            }
            if url.len() > MAX_CREDENTIAL_FILE_SIZE {
                return Err(RuntimeDatabaseCredentialError::OversizedCredential {
                    limit: MAX_CREDENTIAL_FILE_SIZE,
                });
            }
            // Delegate validation to canonical parser; do not include URL in error.
            // We construct a DatabaseConfig to validate and then return raw URL.
            DatabaseConfig::from_url(url.clone())
                .map_err(RuntimeDatabaseCredentialError::InvalidDatabaseUrl)?;
            Ok(url)
        }
        (None, false) => Err(RuntimeDatabaseCredentialError::MissingCredential),
    }
}

/// Read database URL from a specific credential file path with bounded, safe handling.
fn load_database_url_from_file(path: &Path) -> Result<String, RuntimeDatabaseCredentialError> {
    // Bounded size check via metadata when available, to fail fast before reading.
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() as usize > MAX_CREDENTIAL_FILE_SIZE {
            return Err(RuntimeDatabaseCredentialError::OversizedCredential {
                limit: MAX_CREDENTIAL_FILE_SIZE,
            });
        }
        // If file is a directory, treat as unreadable.
        if meta.is_dir() {
            return Err(RuntimeDatabaseCredentialError::UnreadableCredentialFile);
        }
    }
    let bytes =
        fs::read(path).map_err(|_| RuntimeDatabaseCredentialError::UnreadableCredentialFile)?;

    if bytes.len() > MAX_CREDENTIAL_FILE_SIZE {
        return Err(RuntimeDatabaseCredentialError::OversizedCredential {
            limit: MAX_CREDENTIAL_FILE_SIZE,
        });
    }
    if bytes.is_empty() {
        return Err(RuntimeDatabaseCredentialError::EmptyCredential);
    }
    // Convert to String; database URLs are ASCII/UTF-8. If not valid UTF-8, treat as invalid.
    let mut url = String::from_utf8(bytes).map_err(|_| {
        RuntimeDatabaseCredentialError::InvalidDatabaseUrl(DatabaseConfigError::UnsupportedScheme)
    })?;

    // Normalize harmless trailing line ending(s): files commonly end with one `\n`
    // or `\r\n`. We strip exactly one trailing `\n` and an optional preceding `\r`,
    // not all whitespace or interior content. Repeated newlines are treated as
    // part of the trimming logic for robustness: strip trailing `\r\n`/`\n` up to
    // one CRLF pair repeated? Spec says "only harmless trailing line ending(s)" —
    // we support one `\n` or one `\r\n`. For files ending with `\n\n`, the second
    // newline would be considered not harmless; but trimming a single `\n` still
    // handles the common case. We implement trimming of a single `\n` (+ optional `\r`),
    // and also support trimming of `\r\n` where credential was written with Windows line.
    // To allow admin files ending with `\n` (Unix) and preserve interior, we trim
    // trailing `\n` and `\r` exactly as line ending normalization, but we do NOT trim
    // arbitrary spaces.
    if url.ends_with('\n') {
        // Remove one trailing `\n`, and if preceded by `\r`, also remove `\r`.
        url.pop();
        if url.ends_with('\r') {
            url.pop();
        }
    }
    // After normalization, whitespace-only must fail.
    if url.trim().is_empty() {
        return Err(RuntimeDatabaseCredentialError::EmptyCredential);
    }
    if url.len() > MAX_CREDENTIAL_FILE_SIZE {
        return Err(RuntimeDatabaseCredentialError::OversizedCredential {
            limit: MAX_CREDENTIAL_FILE_SIZE,
        });
    }
    // Validate via canonical parser without logging secret.
    DatabaseConfig::from_url(url.clone())
        .map_err(RuntimeDatabaseCredentialError::InvalidDatabaseUrl)?;
    Ok(url)
}

/// Load `DatabaseConfig` from the runtime credential source (credential file or
/// `DATABASE_URL` fallback). Never logs the URL.
///
/// This is the runtime-edge helper that the one-shot binary must call before
/// `DatabasePool::connect` or `MigrationRunner::run`.
pub fn database_config_from_runtime() -> Result<DatabaseConfig, RuntimeDatabaseCredentialError> {
    let url = load_database_url_from_runtime_source()?;
    DatabaseConfig::from_url(url).map_err(RuntimeDatabaseCredentialError::InvalidDatabaseUrl)
}

/// Test-only helper to load from an explicit path (bypasses env precedence) for
/// isolated unit tests without mutating global env.
#[cfg(test)]
pub fn load_database_url_from_file_for_test(
    path: &Path,
) -> Result<String, RuntimeDatabaseCredentialError> {
    load_database_url_from_file(path)
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::{
        CREDENTIAL_FILE_ENV, CREDENTIAL_ID, CREDENTIALS_DIRECTORY_ENV, MAX_CREDENTIAL_FILE_SIZE,
        RuntimeDatabaseCredentialError, load_database_url_from_file_for_test,
        load_database_url_from_runtime_source,
    };
    use std::{
        env, fs,
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };

    fn env_mutex() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_env_lock<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
        let _guard = env_mutex().lock().unwrap();
        let orig_cred = env::var(CREDENTIAL_FILE_ENV).ok();
        let orig_dir = env::var(CREDENTIALS_DIRECTORY_ENV).ok();
        let orig_url = env::var(synveil_metadata::DATABASE_URL_ENV).ok();
        // Clear for test
        unsafe {
            env::remove_var(CREDENTIAL_FILE_ENV);
            env::remove_var(CREDENTIALS_DIRECTORY_ENV);
            env::remove_var(synveil_metadata::DATABASE_URL_ENV);
        }
        let result = std::panic::catch_unwind(f);
        // Restore
        unsafe {
            match orig_cred {
                Some(v) => env::set_var(CREDENTIAL_FILE_ENV, v),
                None => env::remove_var(CREDENTIAL_FILE_ENV),
            }
            match orig_dir {
                Some(v) => env::set_var(CREDENTIALS_DIRECTORY_ENV, v),
                None => env::remove_var(CREDENTIALS_DIRECTORY_ENV),
            }
            match orig_url {
                Some(v) => env::set_var(synveil_metadata::DATABASE_URL_ENV, v),
                None => env::remove_var(synveil_metadata::DATABASE_URL_ENV),
            }
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn valid_credential_file_loads() {
        with_env_lock(|| {
            let dir = env::temp_dir().join(format!(
                "synveil-cred-test-{}",
                uuid::Uuid::now_v7().simple()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            fs::write(&path, "postgresql://user:pass@127.0.0.1:5432/synveil").unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let url = load_database_url_from_runtime_source().expect("valid file should load");
            assert_eq!(url, "postgresql://user:pass@127.0.0.1:5432/synveil");
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn valid_credential_via_credentials_directory() {
        with_env_lock(|| {
            let dir = env::temp_dir().join(format!(
                "synveil-cred-dir-{}",
                uuid::Uuid::now_v7().simple()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(CREDENTIAL_ID);
            fs::write(&path, "postgresql://via-dir:secret@127.0.0.1:5432/db").unwrap();
            unsafe { env::set_var(CREDENTIALS_DIRECTORY_ENV, dir.to_string_lossy().to_string()) };
            let url = load_database_url_from_runtime_source()
                .expect("CREDENTIALS_DIRECTORY should resolve");
            assert_eq!(url, "postgresql://via-dir:secret@127.0.0.1:5432/db");
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn env_fallback_when_no_credential() {
        with_env_lock(|| {
            unsafe {
                env::set_var(
                    synveil_metadata::DATABASE_URL_ENV,
                    "postgresql://dev:dev@127.0.0.1:5432/synveil",
                )
            };
            let url = load_database_url_from_runtime_source().unwrap();
            assert_eq!(url, "postgresql://dev:dev@127.0.0.1:5432/synveil");
        });
    }

    #[test]
    fn dual_source_is_ambiguous() {
        with_env_lock(|| {
            let dir =
                env::temp_dir().join(format!("synveil-dual-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            fs::write(&path, "postgresql://from-file@127.0.0.1/db").unwrap();
            unsafe {
                env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string());
                env::set_var(
                    synveil_metadata::DATABASE_URL_ENV,
                    "postgresql://from-env@127.0.0.1/db",
                );
            }
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(err, RuntimeDatabaseCredentialError::AmbiguousConfiguration);
            assert!(!err.to_string().contains("from-file"));
            assert!(!err.to_string().contains("from-env"));
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn missing_credential_fails() {
        with_env_lock(|| {
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(err, RuntimeDatabaseCredentialError::MissingCredential);
            assert_eq!(err.to_string(), "database credential is missing");
        });
    }

    #[test]
    fn unreadable_credential_file_fails() {
        with_env_lock(|| {
            let missing = PathBuf::from("/nonexistent/synveil-cred-missing-xyz/database-url");
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, missing.to_string_lossy().to_string()) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(
                err,
                RuntimeDatabaseCredentialError::UnreadableCredentialFile
            );
            assert_eq!(err.to_string(), "database credential file is unreadable");
        });
    }

    #[test]
    fn empty_credential_file_fails() {
        with_env_lock(|| {
            let dir =
                env::temp_dir().join(format!("synveil-empty-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            fs::write(&path, "").unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(err, RuntimeDatabaseCredentialError::EmptyCredential);
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn whitespace_only_credential_fails() {
        with_env_lock(|| {
            let dir = env::temp_dir().join(format!("synveil-ws-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            fs::write(&path, "   \n\t\n").unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(err, RuntimeDatabaseCredentialError::EmptyCredential);
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn oversized_credential_fails() {
        with_env_lock(|| {
            let dir = env::temp_dir().join(format!(
                "synveil-oversize-{}",
                uuid::Uuid::now_v7().simple()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            let big = "a".repeat(MAX_CREDENTIAL_FILE_SIZE + 1);
            fs::write(&path, big).unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(
                err,
                RuntimeDatabaseCredentialError::OversizedCredential {
                    limit: MAX_CREDENTIAL_FILE_SIZE
                }
            );
            assert!(err.to_string().contains("exceeds maximum size"));
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn oversized_via_env_fallback_fails() {
        with_env_lock(|| {
            let big = format!(
                "postgresql://user:pass@host/db?param={}",
                "a".repeat(MAX_CREDENTIAL_FILE_SIZE)
            );
            unsafe { env::set_var(synveil_metadata::DATABASE_URL_ENV, big) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            assert_eq!(
                err,
                RuntimeDatabaseCredentialError::OversizedCredential {
                    limit: MAX_CREDENTIAL_FILE_SIZE
                }
            );
        });
    }

    #[test]
    fn malformed_database_url_fails_via_canonical_validator() {
        with_env_lock(|| {
            let dir = env::temp_dir().join(format!(
                "synveil-malformed-{}",
                uuid::Uuid::now_v7().simple()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            fs::write(&path, "not-a-valid-url").unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            match err {
                RuntimeDatabaseCredentialError::InvalidDatabaseUrl(_) => {}
                other => panic!("expected InvalidDatabaseUrl, got {other:?}"),
            }
            // Unsupported scheme
            fs::write(&path, "sqlite://synveil.db").unwrap();
            let err2 = load_database_url_from_runtime_source().unwrap_err();
            match err2 {
                RuntimeDatabaseCredentialError::InvalidDatabaseUrl(_) => {}
                other => panic!("expected InvalidDatabaseUrl for sqlite, got {other:?}"),
            }
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn trailing_newline_is_normalized() {
        with_env_lock(|| {
            let dir = env::temp_dir().join(format!("synveil-nl-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            // Unix newline
            fs::write(&path, "postgresql://user:pass@127.0.0.1:5432/db\n").unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let url = load_database_url_from_runtime_source().expect("newline should be stripped");
            assert_eq!(url, "postgresql://user:pass@127.0.0.1:5432/db");
            // CRLF
            fs::write(&path, "postgresql://user:pass@127.0.0.1:5432/db\r\n").unwrap();
            let url2 = load_database_url_from_runtime_source().expect("CRLF should be stripped");
            assert_eq!(url2, "postgresql://user:pass@127.0.0.1:5432/db");
            // No newline (already clean)
            fs::write(&path, "postgresql://user:pass@127.0.0.1:5432/db").unwrap();
            let url3 = load_database_url_from_runtime_source().unwrap();
            assert_eq!(url3, "postgresql://user:pass@127.0.0.1:5432/db");
            let _ = fs::remove_dir_all(&dir);
        });
    }

    #[test]
    fn no_secret_logging_in_errors() {
        with_env_lock(|| {
            let dir =
                env::temp_dir().join(format!("synveil-nolog-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("database-url");
            let secret = "postgresql://user:super-secret-password@127.0.0.1:5432/db";
            fs::write(&path, secret).unwrap();
            // Trigger malformed by using the secret as part of file? Instead test that success doesn't log,
            // and that error for malformed doesn't contain secret.
            // For oversized, ensure error doesn't contain secret.
            let big_secret = format!("postgresql://user:{}@host/db", "a".repeat(9000));
            fs::write(&path, &big_secret).unwrap();
            unsafe { env::set_var(CREDENTIAL_FILE_ENV, path.to_string_lossy().to_string()) };
            let err = load_database_url_from_runtime_source().unwrap_err();
            let err_str = err.to_string();
            assert!(!err_str.contains("super-secret"));
            assert!(!err_str.contains("postgresql://"));
            // Also test via direct file helper
            let dir2 =
                env::temp_dir().join(format!("synveil-nolog2-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir2).unwrap();
            let path2 = dir2.join("database-url");
            fs::write(&path2, secret).unwrap();
            let res = load_database_url_from_file_for_test(&path2).unwrap();
            // Ensure helper itself doesn't log; we just check result is correct and doesn't leak elsewhere
            assert_eq!(res, secret);
            let _ = fs::remove_dir_all(&dir);
            let _ = fs::remove_dir_all(&dir2);
        });
    }

    #[test]
    fn credential_file_explicit_wins_over_credentials_directory() {
        with_env_lock(|| {
            let dir1 = env::temp_dir().join(format!(
                "synveil-explicit-{}",
                uuid::Uuid::now_v7().simple()
            ));
            let dir2 =
                env::temp_dir().join(format!("synveil-dir-{}", uuid::Uuid::now_v7().simple()));
            fs::create_dir_all(&dir1).unwrap();
            fs::create_dir_all(&dir2).unwrap();
            let explicit = dir1.join("explicit-url");
            fs::write(&explicit, "postgresql://explicit:pass@127.0.0.1/db").unwrap();
            let via_dir = dir2.join(CREDENTIAL_ID);
            fs::write(&via_dir, "postgresql://via-dir:pass@127.0.0.1/db").unwrap();
            unsafe {
                env::set_var(CREDENTIAL_FILE_ENV, explicit.to_string_lossy().to_string());
                env::set_var(
                    CREDENTIALS_DIRECTORY_ENV,
                    dir2.to_string_lossy().to_string(),
                );
            }
            let url = load_database_url_from_runtime_source().expect("explicit should win");
            assert_eq!(url, "postgresql://explicit:pass@127.0.0.1/db");
            let _ = fs::remove_dir_all(&dir1);
            let _ = fs::remove_dir_all(&dir2);
        });
    }

    #[test]
    fn missing_file_with_explicit_env_does_not_fallback_to_database_url() {
        with_env_lock(|| {
            let missing = PathBuf::from("/tmp/synveil-missing-explicit-12345/database-url");
            unsafe {
                env::set_var(CREDENTIAL_FILE_ENV, missing.to_string_lossy().to_string());
                env::set_var(
                    synveil_metadata::DATABASE_URL_ENV,
                    "postgresql://fallback@127.0.0.1/db",
                );
            }
            let err = load_database_url_from_runtime_source().unwrap_err();
            // Should be ambiguous (both set) per policy, not fallback
            assert_eq!(err, RuntimeDatabaseCredentialError::AmbiguousConfiguration);
        });
    }
}
