//! Signed metadata ETags for conditional node mutations.

use std::fmt;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use synveil_auth::SessionToken;
use synveil_core::{NodeId, Revision};

type HmacSha256 = Hmac<Sha256>;

const TAG_BYTES: usize = 32;
const TAG_HEX_BYTES: usize = TAG_BYTES * 2;

/// Per-application signing key for mutable metadata ETags. ETags remain
/// opaque to clients, while the signature prevents a client from manufacturing
/// a revision token for an arbitrary node.
#[derive(Clone)]
pub struct EtagKey([u8; 32]);

impl EtagKey {
    #[must_use]
    pub fn generate() -> Self {
        let token = SessionToken::generate();
        Self(*token.as_bytes())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn issue(&self, node_id: NodeId, revision: Revision) -> String {
        let revision = revision.to_string();
        let tag = self.sign(node_id, &revision);
        format!("\"v1.node.{node_id}.{revision}.{}\"", encode_hex(&tag))
    }

    /// Verify an exact strong ETag and recover only its signed revision.
    pub fn verify(&self, candidate: &str, node_id: NodeId) -> Option<Revision> {
        let value = candidate.strip_prefix('"')?.strip_suffix('"')?;
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() != 5
            || parts[0] != "v1"
            || parts[1] != "node"
            || parts[2] != node_id.to_string()
        {
            return None;
        }
        let revision = parts[3].parse().ok()?;
        let tag = decode_hex(parts[4])?;
        let expected = self.sign(node_id, parts[3]);
        bool::from(expected.as_slice().ct_eq(&tag)).then_some(revision)
    }

    fn sign(&self, node_id: NodeId, revision: &str) -> [u8; TAG_BYTES] {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts a 256-bit key");
        mac.update(b"synveil/etag/v1\0");
        mac.update(node_id.as_bytes());
        mac.update(revision.as_bytes());
        let bytes = mac.finalize().into_bytes();
        let mut tag = [0_u8; TAG_BYTES];
        tag.copy_from_slice(&bytes);
        tag
    }
}

impl Default for EtagKey {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Debug for EtagKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EtagKey([REDACTED])")
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != TAG_HEX_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(TAG_BYTES);
    for pair in value.as_bytes().chunks(2) {
        if pair.len() != 2 {
            return None;
        }
        let high = decode_hex_nibble(pair[0])?;
        let low = decode_hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

#[cfg(test)]
mod tests {
    use super::EtagKey;
    use synveil_core::{NodeId, Revision};

    #[test]
    fn etags_are_signed_and_node_scoped() {
        let key = EtagKey::from_bytes([0x42; 32]);
        let node = NodeId::new();
        let other = NodeId::new();
        let etag = key.issue(node, Revision::new(7));
        assert_eq!(key.verify(&etag, node), Some(Revision::new(7)));
        assert_eq!(key.verify(&etag, other), None);
        assert_eq!(key.verify(&etag.replace(".7.", ".8."), node), None);
    }
}
