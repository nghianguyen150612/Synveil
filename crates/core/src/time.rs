use std::{fmt, str::FromStr, time::Duration as StdDuration};

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

    #[must_use]
    pub fn checked_add_std(self, duration: StdDuration) -> Option<Self> {
        as_time_duration(duration)
            .and_then(|duration| self.0.checked_add(duration))
            .map(Self::from_offset_datetime)
    }

    #[must_use]
    pub fn checked_sub_std(self, duration: StdDuration) -> Option<Self> {
        as_time_duration(duration)
            .and_then(|duration| self.0.checked_sub(duration))
            .map(Self::from_offset_datetime)
    }

    pub fn parse(value: &str) -> Result<Self, TimestampParseError> {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(Self::from_offset_datetime)
            .map_err(|_| TimestampParseError)
    }
}

fn as_time_duration(duration: StdDuration) -> Option<time::Duration> {
    let seconds = i64::try_from(duration.as_secs()).ok()?;
    if seconds == i64::MAX && duration.subsec_nanos() != 0 {
        return None;
    }
    Some(time::Duration::new(seconds, duration.subsec_nanos() as i32))
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
    use std::time::Duration as StdDuration;

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

    #[test]
    fn timestamp_supports_checked_standard_duration_arithmetic() {
        let timestamp = Timestamp::parse("2026-08-22T00:00:00Z").unwrap();
        let duration = StdDuration::from_secs(86_400);

        assert_eq!(
            timestamp.checked_add_std(duration),
            Timestamp::parse("2026-08-23T00:00:00Z").ok()
        );
        assert_eq!(
            timestamp.checked_sub_std(duration),
            Timestamp::parse("2026-08-21T00:00:00Z").ok()
        );
    }
}
