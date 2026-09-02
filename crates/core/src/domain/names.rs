use std::fmt;

use super::errors::DomainError;

/// Bounded UTF-8 storage for a user-visible logical name.
///
/// The value is deliberately kept raw. It is not a filesystem path, an object
/// key, or a normalized comparison key. The exact comparison/normalization
/// profile remains the documented open domain decision.
pub const MAX_LOGICAL_NAME_BYTES: usize = 1_024;

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogicalName(String);

impl LogicalName {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        validate_text(&value, DomainError::EmptyName, DomainError::NameTooLong)?;
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

impl fmt::Debug for LogicalName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("LogicalName").field(&self.0).finish()
    }
}

impl AsRef<str> for LogicalName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// A login identifier paired with an adapter-supplied uniqueness key.
///
/// Core preserves both values and does not choose case folding or Unicode
/// normalization. The uniqueness key is not derived from a filesystem rule.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LoginIdentifier {
    value: String,
    uniqueness_key: String,
}

impl LoginIdentifier {
    pub fn new(
        value: impl Into<String>,
        uniqueness_key: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let value = value.into();
        let uniqueness_key = uniqueness_key.into();
        validate_text(
            &value,
            DomainError::EmptyLoginIdentifier,
            DomainError::LoginIdentifierTooLong,
        )?;
        validate_text(
            &uniqueness_key,
            DomainError::EmptyLoginKey,
            DomainError::LoginKeyTooLong,
        )?;
        Ok(Self {
            value,
            uniqueness_key,
        })
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub fn uniqueness_key(&self) -> &str {
        &self.uniqueness_key
    }
}

impl fmt::Debug for LoginIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoginIdentifier")
            .field("value", &self.value)
            .field("uniqueness_key", &self.uniqueness_key)
            .finish()
    }
}

fn validate_text(
    value: &str,
    empty_error: DomainError,
    too_long_error: DomainError,
) -> Result<(), DomainError> {
    if value.is_empty() {
        return Err(empty_error);
    }
    if value.len() > MAX_LOGICAL_NAME_BYTES {
        return Err(too_long_error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LogicalName, LoginIdentifier, MAX_LOGICAL_NAME_BYTES};
    use crate::DomainError;

    #[test]
    fn logical_names_preserve_raw_unicode_without_filesystem_normalization() {
        let first = LogicalName::new("A/B").expect("logical separators are not filesystem paths");
        let second = LogicalName::new("a/b").expect("raw logical name is valid");

        assert_eq!(first.as_str(), "A/B");
        assert_ne!(first, second);
    }

    #[test]
    fn logical_names_and_login_keys_are_bounded_without_echoing_values() {
        assert_eq!(LogicalName::new(""), Err(DomainError::EmptyName));
        assert_eq!(
            LogicalName::new("x".repeat(MAX_LOGICAL_NAME_BYTES + 1)),
            Err(DomainError::NameTooLong)
        );
        assert_eq!(
            LoginIdentifier::new("alice", ""),
            Err(DomainError::EmptyLoginKey)
        );
        assert_eq!(
            LoginIdentifier::new("x".repeat(MAX_LOGICAL_NAME_BYTES + 1), "key"),
            Err(DomainError::LoginIdentifierTooLong)
        );
    }
}
