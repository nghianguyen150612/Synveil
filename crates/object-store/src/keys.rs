use std::{fmt, str::FromStr};

const MAX_OBJECT_KEY_BYTES: usize = 1_024;
const MAX_OPAQUE_TOKEN_BYTES: usize = 256;

/// Server-generated, opaque final-object key.
///
/// The validation is deliberately stricter than a platform filesystem path:
/// it rejects absolute paths, parent traversal, separators that vary by OS,
/// and user-filename characters. Adapters still treat the value as untrusted
/// at their own boundary.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectKey(String);

impl ObjectKey {
    pub fn new(value: impl Into<String>) -> Result<Self, ObjectKeyError> {
        let value = value.into();
        validate_object_key(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl FromStr for ObjectKey {
    type Err = ObjectKeyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Debug for ObjectKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ObjectKey")
            .field(&"<redacted>")
            .finish()
    }
}

/// Opaque handle for a temporary/staging write.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StagingHandle(String);

impl StagingHandle {
    pub fn new(value: impl Into<String>) -> Result<Self, OpaqueTokenError> {
        Ok(Self(validate_opaque_token(value.into())?))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for StagingHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("StagingHandle")
            .field(&"<redacted>")
            .finish()
    }
}

/// Opaque backend version/generation evidence used for conditional deletion.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectVersion(String);

impl ObjectVersion {
    pub fn new(value: impl Into<String>) -> Result<Self, OpaqueTokenError> {
        Ok(Self(validate_opaque_token(value.into())?))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ObjectVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ObjectVersion")
            .field(&"<redacted>")
            .finish()
    }
}

fn validate_object_key(value: &str) -> Result<(), ObjectKeyError> {
    if value.is_empty() {
        return Err(ObjectKeyError::Empty);
    }
    if value.len() > MAX_OBJECT_KEY_BYTES {
        return Err(ObjectKeyError::TooLong);
    }
    if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        return Err(ObjectKeyError::AbsoluteOrEmptyComponent);
    }

    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(ObjectKeyError::TraversalOrEmptyComponent);
        }
    }

    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
    {
        return Err(ObjectKeyError::InvalidCharacter);
    }

    Ok(())
}

fn validate_opaque_token(value: String) -> Result<String, OpaqueTokenError> {
    if value.is_empty() {
        return Err(OpaqueTokenError::Empty);
    }
    if value.len() > MAX_OPAQUE_TOKEN_BYTES {
        return Err(OpaqueTokenError::TooLong);
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(OpaqueTokenError::InvalidCharacter);
    }

    Ok(value)
}

/// Safe reason an object key was rejected. The rejected value is never stored
/// in the error, so formatting cannot echo untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectKeyError {
    Empty,
    TooLong,
    InvalidCharacter,
    AbsoluteOrEmptyComponent,
    TraversalOrEmptyComponent,
}

impl fmt::Display for ObjectKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "object key is empty",
            Self::TooLong => "object key is too long",
            Self::InvalidCharacter => "object key contains an invalid character",
            Self::AbsoluteOrEmptyComponent => "object key must be relative and normalized",
            Self::TraversalOrEmptyComponent => "object key traversal is not allowed",
        })
    }
}

impl std::error::Error for ObjectKeyError {}

/// Safe reason an opaque staging handle or backend version was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpaqueTokenError {
    Empty,
    TooLong,
    InvalidCharacter,
}

impl fmt::Display for OpaqueTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "opaque token is empty",
            Self::TooLong => "opaque token is too long",
            Self::InvalidCharacter => "opaque token contains an invalid character",
        })
    }
}

impl std::error::Error for OpaqueTokenError {}
