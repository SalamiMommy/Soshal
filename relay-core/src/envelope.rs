//! Binary mesh envelope framing.
//!
//! Custom wire format for the mesh relay: little-endian binary, magic
//! prefixed, hostile-input capped. Carries the signed event JSON as payload
//! (the app's data format); framing itself is nostr-protocol-free.

use serde::{Deserialize, Serialize};

/// Wire magic: `b"sos1"`.
pub const MAGIC: [u8; 4] = *b"sos1";
/// Current envelope version.
pub const ENVELOPE_VERSION: u8 = 1;
/// Flood hop ceiling — envelopes at this hop count are not re-broadcast.
pub const MAX_HOP_COUNT: u8 = 6;
/// Single event payload cap (256 KiB).
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
/// Total wire size cap (magic + headers + payload).
pub const MAX_ENVELOPE_BYTES: usize = MAX_PAYLOAD_BYTES + 512;

/// A relayed event's header + payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshEnvelope {
    pub version: u8,
    pub hop_count: u8,
    pub event_id: String,
    pub kind: u16,
    pub author: String,
    pub created_at: u64,
    pub payload: Vec<u8>,
}

impl MeshEnvelope {
    /// Builds a fresh (hop 0) envelope.
    pub fn new(
        event_id: String,
        kind: u16,
        author: String,
        created_at: u64,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            version: ENVELOPE_VERSION,
            hop_count: 0,
            event_id,
            kind,
            author,
            created_at,
            payload,
        }
    }

    /// True when this envelope must not be re-broadcast (hop ceiling).
    pub fn at_hop_limit(&self) -> bool {
        self.hop_count >= MAX_HOP_COUNT
    }

    /// Increments hop_count in place, returning true if successful or false if at ceiling.
    pub fn increment_hop_in_place(&mut self) -> bool {
        if self.at_hop_limit() {
            false
        } else {
            self.hop_count += 1;
            true
        }
    }

    /// Consumes the envelope and returns it with hop_count +1, or None at the ceiling.
    pub fn into_incremented_hop(mut self) -> Option<Self> {
        if self.increment_hop_in_place() {
            Some(self)
        } else {
            None
        }
    }

    /// Returns a clone with hop_count +1, or None at the ceiling.
    pub fn increment_hop(&self) -> Option<Self> {
        self.clone().into_incremented_hop()
    }

    /// Encodes to wire bytes (little-endian, magic prefixed).
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(4 + 1 + 1 + 2 + 2 + 8 + 4 + self.payload.len());
        out.extend_from_slice(&MAGIC);
        out.push(self.version);
        out.push(self.hop_count);
        push_str(&mut out, &self.event_id);
        out.extend_from_slice(&self.kind.to_le_bytes());
        push_str(&mut out, &self.author);
        out.extend_from_slice(&self.created_at.to_le_bytes());
        let len =
            u32::try_from(self.payload.len()).map_err(|e| format!("payload too large: {e}"))?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// Decodes wire bytes, enforcing all caps. None on any malformation.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 4 + 1 + 1 + 2 + 2 + 8 + 4 {
            return None;
        }
        if data.len() > MAX_ENVELOPE_BYTES {
            return None;
        }
        if data[..4] != MAGIC {
            return None;
        }
        let version = data[4];
        let hop_count = data[5];
        if version != ENVELOPE_VERSION {
            return None;
        }
        if hop_count > MAX_HOP_COUNT {
            return None;
        }
        let mut pos = 6;
        let event_id = take_str(data, &mut pos)?;
        if data.len() < pos + 2 {
            return None;
        }
        let kind = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;
        let author = take_str(data, &mut pos)?;
        if data.len() < pos + 8 {
            return None;
        }
        let created_at = u64::from_le_bytes(data[pos..pos + 8].try_into().ok()?);
        pos += 8;
        if data.len() < pos + 4 {
            return None;
        }
        let payload_len = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
        pos += 4;
        if payload_len > MAX_PAYLOAD_BYTES {
            return None;
        }
        if data.len() < pos + payload_len {
            return None;
        }
        let payload = data[pos..pos + payload_len].to_vec();
        Some(Self {
            version,
            hop_count,
            event_id,
            kind,
            author,
            created_at,
            payload,
        })
    }
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() <= 128, "envelope string exceeds 128-byte cap");
    out.extend_from_slice(&(s.len() as u16).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Reads a u16-length-prefixed string, capping length at 128 chars.
fn take_str(data: &[u8], pos: &mut usize) -> Option<String> {
    if data.len() < *pos + 2 {
        return None;
    }
    let len = u16::from_le_bytes([data[*pos], data[*pos + 1]]) as usize;
    *pos += 2;
    if len > 128 {
        return None;
    }
    if data.len() < *pos + len {
        return None;
    }
    let s = std::str::from_utf8(&data[*pos..*pos + len])
        .ok()?
        .to_string();
    *pos += len;
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MeshEnvelope {
        MeshEnvelope::new(
            "abc123".to_string(),
            1,
            "deadbeef".to_string(),
            1700000000,
            br#"{"id":"abc123"}"#.to_vec(),
        )
    }

    #[test]
    fn test_roundtrip() {
        let env = sample();
        let bytes = env.to_bytes().unwrap();
        assert_eq!(MeshEnvelope::from_bytes(&bytes), Some(env));
    }

    #[test]
    fn test_rejects_bad_magic() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[0] = b'x';
        assert_eq!(MeshEnvelope::from_bytes(&bytes), None);
    }

    #[test]
    fn test_rejects_unknown_version() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[4] = 99;
        assert_eq!(MeshEnvelope::from_bytes(&bytes), None);
    }

    #[test]
    fn test_rejects_oversized_payload() {
        let env = MeshEnvelope::new(
            "id".to_string(),
            1,
            "author".to_string(),
            0,
            vec![0u8; MAX_PAYLOAD_BYTES + 1],
        );
        let bytes = env.to_bytes().unwrap();
        assert_eq!(MeshEnvelope::from_bytes(&bytes), None);
    }

    #[test]
    fn test_rejects_truncated() {
        let bytes = sample().to_bytes().unwrap();
        for cut in 0..bytes.len() {
            assert_eq!(MeshEnvelope::from_bytes(&bytes[..cut]), None);
        }
    }

    #[test]
    fn test_hop_increment_ceiling() {
        let env = sample();
        assert_eq!(env.increment_hop().unwrap().hop_count, 1);
        let mut at_ceiling = env;
        at_ceiling.hop_count = MAX_HOP_COUNT;
        assert!(at_ceiling.increment_hop().is_none());
        assert!(at_ceiling.at_hop_limit());
    }

    #[test]
    fn test_rejects_oversized_strings() {
        let mut bytes = sample().to_bytes().unwrap();
        let mut pos = 6;
        let _ = take_str(&bytes, &mut pos);
        bytes[pos - 2] = 0xff;
        bytes[pos - 1] = 0xff;
        assert_eq!(MeshEnvelope::from_bytes(&bytes), None);
    }
}
