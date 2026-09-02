use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenError {
    Empty,
    TooShort,
    TooLong,
    ControlCharacter,
}

impl fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "token is empty",
            Self::TooShort => "token is too short",
            Self::TooLong => "token is too long",
            Self::ControlCharacter => "token contains a control character",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for TokenError {}

fn validate_token(
    value: &str,
    minimum_length: usize,
    maximum_length: Option<usize>,
) -> Result<(), TokenError> {
    if value.is_empty() {
        return Err(TokenError::Empty);
    }

    if value.chars().count() < minimum_length {
        return Err(TokenError::TooShort);
    }

    if maximum_length.is_some_and(|maximum| value.chars().count() > maximum) {
        return Err(TokenError::TooLong);
    }

    if value.chars().any(char::is_control) {
        return Err(TokenError::ControlCharacter);
    }

    Ok(())
}

/// An opaque metadata version token. Its representation is not interpreted by core.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Etag(String);

/// Compatibility spelling for the documented `ETag` term.
#[allow(clippy::upper_case_acronyms)]
pub type ETag = Etag;

/// An opaque, server-issued cursor. Authentication and scope binding belong to adapters.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OpaqueCursor(String);

/// Compatibility name for the documented version-token concept.
pub type VersionToken = Etag;

impl Etag {
    pub fn new(value: impl Into<String>) -> Result<Self, TokenError> {
        Self::try_new(value)
    }

    pub fn try_new(value: impl Into<String>) -> Result<Self, TokenError> {
        let value = value.into();
        validate_token(&value, 3, Some(256))?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Etag {
    type Err = TokenError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_new(value)
    }
}

impl TryFrom<String> for Etag {
    type Error = TokenError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl AsRef<str> for Etag {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Etag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl OpaqueCursor {
    pub fn new(value: impl Into<String>) -> Result<Self, TokenError> {
        Self::try_new(value)
    }

    pub fn try_new(value: impl Into<String>) -> Result<Self, TokenError> {
        let value = value.into();
        validate_token(&value, 1, None)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for OpaqueCursor {
    type Err = TokenError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_new(value)
    }
}

impl TryFrom<String> for OpaqueCursor {
    type Error = TokenError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl AsRef<str> for OpaqueCursor {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for OpaqueCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Etag, OpaqueCursor, TokenError};

    #[test]
    fn etag_and_cursor_round_trip_as_opaque_strings() {
        let etag = Etag::try_new("resource-v7").unwrap();
        let cursor = OpaqueCursor::from_str("cursor.v1.opaque").unwrap();

        assert_eq!(etag.to_string(), "resource-v7");
        assert_eq!(Etag::from_str(etag.as_str()), Ok(etag));
        assert_eq!(cursor.to_string(), "cursor.v1.opaque");
        assert_eq!(OpaqueCursor::from_str(cursor.as_str()), Ok(cursor));
    }

    #[test]
    fn tokens_reject_empty_control_and_invalid_etag_lengths() {
        assert_eq!(OpaqueCursor::from_str(""), Err(TokenError::Empty));
        assert_eq!(
            OpaqueCursor::from_str("bad\nvalue"),
            Err(TokenError::ControlCharacter)
        );
        assert_eq!(Etag::from_str("ab"), Err(TokenError::TooShort));
        assert_eq!(Etag::from_str(&"a".repeat(257)), Err(TokenError::TooLong));
    }
}
