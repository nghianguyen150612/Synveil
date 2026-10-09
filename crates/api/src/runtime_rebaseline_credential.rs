//! Protected runtime delivery for the persistent rebaseline HMAC key.
//!
//! Linux production services receive `rebaseline-token-key` through systemd
//! `LoadCredential=`. The runtime gets only a credential path, never the key
//! in process arguments or managed environment files.

use std::{
    env, fmt, fs,
    io::Read,
    path::{Path, PathBuf},
};

use zeroize::Zeroizing;

use crate::{RebaselineTokenKey, RebaselineTokenKeyParseError};

pub const REBASELINE_CREDENTIAL_FILE_ENV: &str = "SYNVEIL_REBASELINE_TOKEN_KEY_FILE";
pub const CREDENTIALS_DIRECTORY_ENV: &str = "CREDENTIALS_DIRECTORY";
pub const REBASELINE_CREDENTIAL_ID: &str = "rebaseline-token-key";
pub const MAX_REBASELINE_KEY_FILE_BYTES: usize = 66;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeRebaselineCredentialError {
    MissingCredential,
    AmbiguousConfiguration,
    UnreadableCredentialFile,
    OversizedCredential,
    InvalidEncoding,
    InvalidFormat,
}

impl fmt::Display for RuntimeRebaselineCredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingCredential => "rebaseline token key is missing",
            Self::AmbiguousConfiguration => "rebaseline token key is ambiguous: both credential file and plaintext environment input are set",
            Self::UnreadableCredentialFile => "rebaseline token key file is unreadable or unsafe",
            Self::OversizedCredential => "rebaseline token key exceeds its size limit",
            Self::InvalidEncoding => "rebaseline token key encoding is invalid",
            Self::InvalidFormat => "rebaseline token key must be 64 lowercase hexadecimal characters",
        })
    }
}

impl std::error::Error for RuntimeRebaselineCredentialError {}

impl From<RebaselineTokenKeyParseError> for RuntimeRebaselineCredentialError {
    fn from(_: RebaselineTokenKeyParseError) -> Self {
        Self::InvalidFormat
    }
}

pub fn credential_file_path_from_env() -> Result<Option<PathBuf>, RuntimeRebaselineCredentialError>
{
    if let Some(value) = env::var_os(REBASELINE_CREDENTIAL_FILE_ENV) {
        let value = value
            .to_str()
            .ok_or(RuntimeRebaselineCredentialError::InvalidEncoding)?;
        if !value.trim().is_empty() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
            }
            return Ok(Some(path));
        }
    }
    if let Some(value) = env::var_os(CREDENTIALS_DIRECTORY_ENV) {
        let value = value
            .to_str()
            .ok_or(RuntimeRebaselineCredentialError::InvalidEncoding)?;
        if !value.trim().is_empty() {
            let directory = PathBuf::from(value);
            if !directory.is_absolute() {
                return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
            }
            return Ok(Some(directory.join(REBASELINE_CREDENTIAL_ID)));
        }
    }
    Ok(None)
}

pub fn rebaseline_key_from_runtime() -> Result<RebaselineTokenKey, RuntimeRebaselineCredentialError>
{
    let path = credential_file_path_from_env()?;
    let plaintext = plaintext_environment_value()?;
    match (path, plaintext) {
        (Some(_), Some(_)) => Err(RuntimeRebaselineCredentialError::AmbiguousConfiguration),
        (Some(path), None) => rebaseline_key_from_file(&path),
        (None, Some(value)) => parse_key(&value),
        (None, None) => Err(RuntimeRebaselineCredentialError::MissingCredential),
    }
}

fn plaintext_environment_value()
-> Result<Option<Zeroizing<String>>, RuntimeRebaselineCredentialError> {
    let Some(value) = env::var_os("SYNVEIL_REBASELINE_TOKEN_KEY") else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|_| RuntimeRebaselineCredentialError::InvalidEncoding)?;
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Zeroizing::new(value)))
    }
}

fn rebaseline_key_from_file(
    path: &Path,
) -> Result<RebaselineTokenKey, RuntimeRebaselineCredentialError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| RuntimeRebaselineCredentialError::UnreadableCredentialFile)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
    }
    if metadata.len() > MAX_REBASELINE_KEY_FILE_BYTES as u64 {
        return Err(RuntimeRebaselineCredentialError::OversizedCredential);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options
        .open(path)
        .map_err(|_| RuntimeRebaselineCredentialError::UnreadableCredentialFile)?;
    let opened = file
        .metadata()
        .map_err(|_| RuntimeRebaselineCredentialError::UnreadableCredentialFile)?;
    if !opened.is_file() || !same_file_identity(&metadata, &opened) {
        return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
    }
    if opened.len() > MAX_REBASELINE_KEY_FILE_BYTES as u64 {
        return Err(RuntimeRebaselineCredentialError::OversizedCredential);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.nlink() != 1 {
            return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
        }
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(opened.len() as usize));
    file.by_ref()
        .take(MAX_REBASELINE_KEY_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RuntimeRebaselineCredentialError::UnreadableCredentialFile)?;
    let after = file
        .metadata()
        .map_err(|_| RuntimeRebaselineCredentialError::UnreadableCredentialFile)?;
    if bytes.len() > MAX_REBASELINE_KEY_FILE_BYTES {
        return Err(RuntimeRebaselineCredentialError::OversizedCredential);
    }
    if bytes.len() as u64 != opened.len() || after.len() != opened.len() {
        return Err(RuntimeRebaselineCredentialError::UnreadableCredentialFile);
    }
    let mut value = match String::from_utf8(std::mem::take(&mut *bytes)) {
        Ok(value) => value,
        Err(error) => {
            let _invalid_bytes = Zeroizing::new(error.into_bytes());
            return Err(RuntimeRebaselineCredentialError::InvalidEncoding);
        }
    };
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    let value = Zeroizing::new(value);
    parse_key(&value)
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
}

fn parse_key(value: &str) -> Result<RebaselineTokenKey, RuntimeRebaselineCredentialError> {
    if value.len() != 64
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(RuntimeRebaselineCredentialError::InvalidFormat);
    }
    RebaselineTokenKey::from_hex(value).map_err(Into::into)
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::{env, fs, path::PathBuf};

    use super::{
        CREDENTIALS_DIRECTORY_ENV, MAX_REBASELINE_KEY_FILE_BYTES, REBASELINE_CREDENTIAL_FILE_ENV,
        RuntimeRebaselineCredentialError, rebaseline_key_from_runtime,
    };

    fn with_clean_env(f: impl FnOnce()) {
        let _guard = crate::runtime_test_support::environment_lock();
        let original = [
            env::var_os(REBASELINE_CREDENTIAL_FILE_ENV),
            env::var_os(CREDENTIALS_DIRECTORY_ENV),
            env::var_os("SYNVEIL_REBASELINE_TOKEN_KEY"),
        ];
        unsafe {
            env::remove_var(REBASELINE_CREDENTIAL_FILE_ENV);
            env::remove_var(CREDENTIALS_DIRECTORY_ENV);
            env::remove_var("SYNVEIL_REBASELINE_TOKEN_KEY");
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        unsafe {
            for (name, value) in [
                REBASELINE_CREDENTIAL_FILE_ENV,
                CREDENTIALS_DIRECTORY_ENV,
                "SYNVEIL_REBASELINE_TOKEN_KEY",
            ]
            .into_iter()
            .zip(original)
            {
                if let Some(value) = value {
                    env::set_var(name, value);
                } else {
                    env::remove_var(name);
                }
            }
        }
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    #[test]
    fn credential_file_and_credentials_directory_load_protected_key() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("key");
            fs::write(&path, format!("{}\r\n", "ab".repeat(32))).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
            unsafe { env::set_var(REBASELINE_CREDENTIAL_FILE_ENV, &path) };
            assert!(rebaseline_key_from_runtime().is_ok());
            unsafe {
                env::remove_var(REBASELINE_CREDENTIAL_FILE_ENV);
                env::set_var(CREDENTIALS_DIRECTORY_ENV, dir.path());
            }
            fs::copy(&path, dir.path().join("rebaseline-token-key")).unwrap();
            assert!(rebaseline_key_from_runtime().is_ok());
        });
    }

    #[test]
    fn env_fallback_and_dual_source_fail_closed() {
        with_clean_env(|| {
            let key = "cd".repeat(32);
            unsafe { env::set_var("SYNVEIL_REBASELINE_TOKEN_KEY", &key) };
            assert!(rebaseline_key_from_runtime().is_ok());
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("key");
            fs::write(&path, "ef".repeat(32)).unwrap();
            unsafe { env::set_var(REBASELINE_CREDENTIAL_FILE_ENV, &path) };
            assert_eq!(
                rebaseline_key_from_runtime().unwrap_err(),
                RuntimeRebaselineCredentialError::AmbiguousConfiguration
            );
        });
    }

    #[test]
    fn missing_invalid_oversized_are_redacted() {
        with_clean_env(|| {
            assert_eq!(
                rebaseline_key_from_runtime().unwrap_err(),
                RuntimeRebaselineCredentialError::MissingCredential
            );
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("key");
            fs::write(&path, "CANARY-invalid-key").unwrap();
            unsafe { env::set_var(REBASELINE_CREDENTIAL_FILE_ENV, &path) };
            let error = rebaseline_key_from_runtime().unwrap_err();
            assert_eq!(error, RuntimeRebaselineCredentialError::InvalidFormat);
            assert!(!format!("{error:?} {error}").contains("CANARY"));

            fs::write(&path, vec![b'x'; MAX_REBASELINE_KEY_FILE_BYTES + 1]).unwrap();
            assert_eq!(
                rebaseline_key_from_runtime().unwrap_err(),
                RuntimeRebaselineCredentialError::OversizedCredential
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_hardlink_are_rejected() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("key");
            let moved = dir.path().join("moved");
            fs::write(&moved, "ef".repeat(32)).unwrap();
            unsafe { env::set_var(REBASELINE_CREDENTIAL_FILE_ENV, &path) };
            symlink(&moved, &path).unwrap();
            assert_eq!(
                rebaseline_key_from_runtime().unwrap_err(),
                RuntimeRebaselineCredentialError::UnreadableCredentialFile
            );

            fs::remove_file(&path).unwrap();
            fs::write(&moved, "ef".repeat(32)).unwrap();
            fs::hard_link(&moved, &path).unwrap();
            assert_eq!(
                rebaseline_key_from_runtime().unwrap_err(),
                RuntimeRebaselineCredentialError::UnreadableCredentialFile
            );
        });
    }

    #[test]
    fn invalid_relative_file_path_is_rejected() {
        with_clean_env(|| {
            unsafe {
                env::set_var(
                    REBASELINE_CREDENTIAL_FILE_ENV,
                    PathBuf::from("relative-key"),
                )
            };
            assert_eq!(
                rebaseline_key_from_runtime().unwrap_err(),
                RuntimeRebaselineCredentialError::UnreadableCredentialFile
            );
        });
    }
}
