use std::{fmt, str::FromStr};

const SHA256_PREFIX: &str = "sha256:";
const SHA256_HEX_LENGTH: usize = 64;

/// The canonical SHA-256 integrity representation used by Synveil.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Sha256Digest([u8; 32]);

/// The initial algorithm-qualified content hash type.
pub type Hash = Sha256Digest;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashParseError {
    InvalidAlgorithm,
    InvalidLength,
    InvalidHex,
}

impl fmt::Display for HashParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidAlgorithm => "unsupported hash algorithm",
            Self::InvalidLength => "invalid SHA-256 hash length",
            Self::InvalidHex => "invalid lowercase hexadecimal hash",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for HashParseError {}

impl Sha256Digest {
    pub fn parse(value: &str) -> Result<Self, HashParseError> {
        value.parse()
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl TryFrom<&[u8]> for Sha256Digest {
    type Error = HashParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let bytes: [u8; 32] = value
            .try_into()
            .map_err(|_| HashParseError::InvalidLength)?;
        Ok(Self::from_bytes(bytes))
    }
}

impl FromStr for Sha256Digest {
    type Err = HashParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(hex) = value.strip_prefix(SHA256_PREFIX) else {
            return Err(HashParseError::InvalidAlgorithm);
        };

        if hex.len() != SHA256_HEX_LENGTH {
            return Err(HashParseError::InvalidLength);
        }

        let mut bytes = [0_u8; 32];
        for (index, pair) in hex.as_bytes().chunks(2).enumerate() {
            if !pair
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            {
                return Err(HashParseError::InvalidHex);
            }

            bytes[index] = (hex_value(pair[0]) << 4) | hex_value(pair[1]);
        }

        Ok(Self::from_bytes(bytes))
    }
}

const fn hex_value(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => 0,
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(SHA256_PREFIX)?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{HashParseError, Sha256Digest};

    #[test]
    fn hash_round_trips_in_canonical_form() {
        let digest = Sha256Digest::from_bytes([0xab; 32]);
        let serialized = digest.to_string();

        assert_eq!(serialized, format!("sha256:{}", "ab".repeat(32)));
        assert_eq!(Sha256Digest::from_str(&serialized), Ok(digest));
    }

    #[test]
    fn hash_rejects_wrong_algorithm_length_and_case() {
        assert_eq!(
            Sha256Digest::from_str("md5:00"),
            Err(HashParseError::InvalidAlgorithm)
        );
        assert_eq!(
            Sha256Digest::from_str("sha256:00"),
            Err(HashParseError::InvalidLength)
        );
        assert_eq!(
            Sha256Digest::from_str(&format!("sha256:{}", "AB".repeat(32))),
            Err(HashParseError::InvalidHex)
        );
    }
}
