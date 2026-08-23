use std::{fmt, str::FromStr};

use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

/// A server-observed instant normalized to UTC for canonical serialization.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(OffsetDateTime);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimestampParseError;

impl fmt::Display for TimestampParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("timestamp is not valid RFC 3339")
    }
}

impl std::error::Error for TimestampParseError {}

impl Timestamp {
    #[must_use]
    pub fn now() -> Self {
        Self::now_utc()
    }

    #[must_use]
    pub fn now_utc() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    #[must_use]
    pub fn from_offset_datetime(value: OffsetDateTime) -> Self {
        Self(value.to_offset(UtcOffset::UTC))
    }

    #[must_use]
    pub const fn as_offset_datetime(&self) -> OffsetDateTime {
        self.0
    }

    pub fn parse(value: &str) -> Result<Self, TimestampParseError> {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(Self::from_offset_datetime)
            .map_err(|_| TimestampParseError)
    }
}

impl FromStr for Timestamp {
    type Err = TimestampParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0.format(&Rfc3339).map_err(|_| fmt::Error)?;
        formatter.write_str(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::Timestamp;

    #[test]
    fn timestamp_normalizes_offsets_to_utc_and_round_trips() {
        let timestamp = Timestamp::parse("2026-08-22T19:34:56.123456+07:00").unwrap();
        let serialized = timestamp.to_string();

        assert!(serialized.ends_with('Z'));
        assert_eq!(Timestamp::parse(&serialized), Ok(timestamp));
    }

    #[test]
    fn timestamp_rejects_non_rfc3339_values() {
        assert!(Timestamp::parse("not-a-timestamp").is_err());
    }
}
