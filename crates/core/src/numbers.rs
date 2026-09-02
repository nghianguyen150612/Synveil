use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecimalValueError {
    Empty,
    LeadingZero,
    InvalidCharacter,
    Overflow,
}

impl fmt::Display for DecimalValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "decimal value is empty",
            Self::LeadingZero => "decimal value has leading zeroes",
            Self::InvalidCharacter => "decimal value contains an invalid character",
            Self::Overflow => "decimal value exceeds u64",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for DecimalValueError {}

fn parse_decimal(value: &str) -> Result<u64, DecimalValueError> {
    if value.is_empty() {
        return Err(DecimalValueError::Empty);
    }

    if value.len() > 1 && value.starts_with('0') {
        return Err(DecimalValueError::LeadingZero);
    }

    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DecimalValueError::InvalidCharacter);
    }

    value.parse().map_err(|_| DecimalValueError::Overflow)
}

macro_rules! decimal_value {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self::new(value)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.get()
            }
        }

        impl FromStr for $name {
            type Err = DecimalValueError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_decimal(value).map(Self::new)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

decimal_value!(
    /// A mutable resource revision represented canonically as unsigned decimal.
    Revision
);

decimal_value!(
    /// An ordered journal or manifest sequence represented canonically as unsigned decimal.
    Sequence
);

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{DecimalValueError, Revision, Sequence};

    #[test]
    fn decimal_values_round_trip_without_leading_zeroes() {
        let revision = Revision::from_str("42").unwrap();
        let sequence = Sequence::new(u64::MAX);

        assert_eq!(revision.to_string(), "42");
        assert_eq!(revision.get(), 42);
        assert_eq!(sequence.to_string(), u64::MAX.to_string());
        assert_eq!(Sequence::from_str(&sequence.to_string()), Ok(sequence));
    }

    #[test]
    fn decimal_values_reject_noncanonical_input() {
        assert_eq!(Revision::from_str(""), Err(DecimalValueError::Empty));
        assert_eq!(
            Revision::from_str("01"),
            Err(DecimalValueError::LeadingZero)
        );
        assert_eq!(
            Revision::from_str("+1"),
            Err(DecimalValueError::InvalidCharacter)
        );
        assert_eq!(
            Revision::from_str("18446744073709551616"),
            Err(DecimalValueError::Overflow)
        );
    }
}
