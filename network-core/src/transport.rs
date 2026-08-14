//! Transport mode selection.
//!
//! Decides whether outgoing traffic rides the clearnet or the local i2pd
//! daemon. `Auto` prefers i2p when i2pd is up and falls back to clearnet
//! otherwise.

use serde::{Deserialize, Serialize};

/// i2pd SAM bridge port (session control + STREAM).
pub const I2P_SAM_PORT: u16 = 7656;
/// i2pd SOCKS5 proxy port (clearnet traffic over i2p).
pub const I2P_SOCKS_PORT: u16 = 4447;
/// i2pd HTTP console port.
pub const I2P_HTTP_PORT: u16 = 4444;
/// Loopback host i2pd binds.
pub const I2P_LOCAL_HOST: &str = "127.0.0.1";

/// Where application traffic rides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    /// Direct clearnet connections only.
    #[default]
    Clearnet,
    /// Prefer i2p when the local i2pd daemon is available, else clearnet.
    Auto,
    /// Force all traffic through the local i2pd daemon.
    I2p,
}

impl TransportMode {
    /// Parses a mode name as persisted/exchanged over FFI.
    pub fn parse_mode(s: &str) -> Option<Self> {
        match s {
            "clearnet" => Some(Self::Clearnet),
            "auto" => Some(Self::Auto),
            "i2p" => Some(Self::I2p),
            _ => None,
        }
    }

    /// Canonical mode name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clearnet => "clearnet",
            Self::Auto => "auto",
            Self::I2p => "i2p",
        }
    }

    /// True when relay/HTTP traffic must route through i2pd SOCKS.
    pub fn forces_i2p(self) -> bool {
        matches!(self, Self::I2p)
    }

    /// SOCKS5 socket as `String` (for reqwest/nostr-sdk proxy config) when
    /// the mode is i2p; `None` for clearnet.
    pub fn socks_url(self) -> Option<String> {
        self.forces_i2p()
            .then(|| format!("socks5://{I2P_LOCAL_HOST}:{I2P_SOCKS_PORT}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_roundtrip() {
        for mode in [
            TransportMode::Clearnet,
            TransportMode::Auto,
            TransportMode::I2p,
        ] {
            assert_eq!(TransportMode::parse_mode(mode.as_str()), Some(mode));
        }
        assert_eq!(TransportMode::parse_mode("bogus"), None);
    }

    #[test]
    fn test_socks_url() {
        assert_eq!(TransportMode::Clearnet.socks_url(), None);
        assert_eq!(
            TransportMode::I2p.socks_url(),
            Some("socks5://127.0.0.1:4447".to_string())
        );
    }
}
