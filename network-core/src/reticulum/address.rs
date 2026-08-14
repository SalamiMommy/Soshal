//! Reticulum destination addressing mechanics.
//!
//! Reticulum uses 16-byte (128-bit) binary destination hashes derived from
//! single/group keys or application names and aspects.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Reticulum Destination Hash (16 bytes / 128 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReticulumAddress(pub [u8; 16]);

impl ReticulumAddress {
    /// Creates a Reticulum address directly from 16 bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Derives a 16-byte Reticulum address from a Nostr public key (hex or npub).
    pub fn from_pubkey(pubkey: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"soshal.reticulum.pubkey:");
        hasher.update(pubkey.as_bytes());
        let result = hasher.finalize();
        let mut addr = [0u8; 16];
        addr.copy_from_slice(&result[..16]);
        Self(addr)
    }

    /// Derives a 16-byte Reticulum address from an app name and aspect string.
    pub fn from_aspect(app_name: &str, aspect: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(app_name.as_bytes());
        hasher.update(b".");
        hasher.update(aspect.as_bytes());
        let result = hasher.finalize();
        let mut addr = [0u8; 16];
        addr.copy_from_slice(&result[..16]);
        Self(addr)
    }

    /// Formats the Reticulum address as a 32-character hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parses a 32-character hex string into a Reticulum address.
    pub fn from_hex(hex_str: &str) -> Result<Self, String> {
        let bytes = hex::decode(hex_str.trim()).map_err(|e| format!("Invalid hex: {e}"))?;
        if bytes.len() != 16 {
            return Err(format!("Expected 16 bytes, got {}", bytes.len()));
        }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl std::fmt::Display for ReticulumAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reticulum_address_hex_roundtrip() {
        let addr = ReticulumAddress::from_pubkey("npub1test12345");
        let hex_str = addr.to_hex();
        assert_eq!(hex_str.len(), 32);
        let parsed = ReticulumAddress::from_hex(&hex_str).unwrap();
        assert_eq!(addr, parsed);
    }

    #[test]
    fn test_reticulum_address_from_aspect() {
        let addr1 = ReticulumAddress::from_aspect("soshal.app", "feed");
        let addr2 = ReticulumAddress::from_aspect("soshal.app", "feed");
        assert_eq!(addr1, addr2);
        let addr3 = ReticulumAddress::from_aspect("soshal.app", "dm");
        assert_ne!(addr1, addr3);
    }
}
