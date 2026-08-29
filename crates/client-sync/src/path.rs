use std::{fmt, path::Path};

use crate::{ClientSyncError, validate_logical_name};

/// UTF-8, slash-separated path relative to one managed root.
///
/// The empty string represents the server root Node. Components are already
/// portability-validated before they are joined.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ManagedRelativePath(String);

impl ManagedRelativePath {
    pub fn root() -> Self {
        Self(String::new())
    }

    pub fn new(value: impl Into<String>) -> Result<Self, ClientSyncError> {
        let value = value.into();
        validate(&value)?;
        Ok(Self(value))
    }

    pub fn child(&self, segment: &str) -> Result<Self, ClientSyncError> {
        if segment.is_empty() || segment.contains(['/', '\\']) || matches!(segment, "." | "..") {
            return Err(ClientSyncError::InvalidRelativePath);
        }
        let value = if self.0.is_empty() {
            segment.to_owned()
        } else {
            format!("{}/{}", self.0, segment)
        };
        Self::new(value)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        let parent = self.0.rsplit_once('/').map_or("", |(parent, _)| parent);
        Some(Self(parent.to_owned()))
    }
}

impl fmt::Debug for ManagedRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ManagedRelativePath([REDACTED])")
    }
}

fn validate(value: &str) -> Result<(), ClientSyncError> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > 32 * 1024
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.ends_with('/')
        || value.contains('\\')
    {
        return Err(ClientSyncError::InvalidRelativePath);
    }
    let parts: Vec<_> = value.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || matches!(*part, "." | ".."))
    {
        return Err(ClientSyncError::InvalidRelativePath);
    }

    if parts.first() == Some(&".synveil") {
        let valid_identity = parts.len() >= 3
            && !parts[2].is_empty()
            && parts[2]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'));
        let valid_internal = match parts.get(1) {
            Some(&"staging") => valid_identity && parts.len() == 3,
            Some(&"quarantine") => {
                valid_identity
                    && parts[3..]
                        .iter()
                        .all(|part| validate_logical_name(part).is_portable())
            }
            _ => false,
        };
        if !valid_internal {
            return Err(ClientSyncError::InvalidRelativePath);
        }
        return Ok(());
    }

    if parts
        .iter()
        .any(|part| !validate_logical_name(part).is_portable())
    {
        return Err(ClientSyncError::InvalidRelativePath);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ManagedRelativePath;

    #[test]
    fn paths_are_relative_and_cannot_traverse() {
        assert!(ManagedRelativePath::new("a/b").is_ok());
        assert!(ManagedRelativePath::new("../secret").is_err());
        assert!(ManagedRelativePath::new("a//b").is_err());
        assert!(ManagedRelativePath::new("C:\\secret").is_err());
        assert!(ManagedRelativePath::new("C:/secret").is_err());
        assert!(ManagedRelativePath::new("CON/file").is_err());
        assert!(ManagedRelativePath::new(".synveil/user-owned").is_err());
        assert!(
            ManagedRelativePath::new(".synveil/staging/018f0f31-9fba-7f74-9b03-3b65e1439831.part")
                .is_ok()
        );
        assert_eq!(
            ManagedRelativePath::root()
                .child("folder")
                .unwrap()
                .as_str(),
            "folder"
        );
    }
}
