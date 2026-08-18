use serde::{Deserialize, Serialize};

pub const I2P_SAM_PORT: u16 = 7656;
pub const I2P_SOCKS_PORT: u16 = 4447;
pub const I2P_HTTP_PORT: u16 = 4444;
pub const I2P_LOCAL_HOST: &str = "127.0.0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    #[default]
    Default,
    Reticulum,
    Freenet,
    I2p,
    Nostr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Reticulum,
    Freenet,
    I2p,
    Nostr,
}

impl TransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reticulum => "reticulum",
            Self::Freenet => "freenet",
            Self::I2p => "i2p",
            Self::Nostr => "nostr",
        }
    }
}

impl TransportMode {
    /// Parses a mode name as persisted/exchanged over FFI. Legacy names
    /// "clearnet" -> Nostr and "auto" -> Default are accepted.
    pub fn parse_mode(s: &str) -> Option<Self> {
        match s {
            "default" => Some(Self::Default),
            "reticulum" => Some(Self::Reticulum),
            "freenet" => Some(Self::Freenet),
            "i2p" => Some(Self::I2p),
            "nostr" => Some(Self::Nostr),
            "auto" => Some(Self::Default),
            "clearnet" => Some(Self::Nostr),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Reticulum => "reticulum",
            Self::Freenet => "freenet",
            Self::I2p => "i2p",
            Self::Nostr => "nostr",
        }
    }

    /// Resolves the mode to the concrete transport to use, given the three
    /// probe results (reticulum running, freenet gateway up, i2p daemon up).
    /// Returns (kind, satisfied). "Only" modes fall back to Nostr with
    /// satisfied=false when their transport is unavailable. Default follows
    /// the chain Reticulum -> Freenet -> I2p -> Nostr.
    pub fn resolve(
        self,
        reticulum_up: bool,
        freenet_up: bool,
        i2p_up: bool,
    ) -> (TransportKind, bool) {
        match self {
            Self::Default => {
                if reticulum_up {
                    (TransportKind::Reticulum, true)
                } else if freenet_up {
                    (TransportKind::Freenet, true)
                } else if i2p_up {
                    (TransportKind::I2p, true)
                } else {
                    (TransportKind::Nostr, true)
                }
            }
            Self::Reticulum => {
                if reticulum_up {
                    (TransportKind::Reticulum, true)
                } else {
                    (TransportKind::Nostr, false)
                }
            }
            Self::Freenet => {
                if freenet_up {
                    (TransportKind::Freenet, true)
                } else {
                    (TransportKind::Nostr, false)
                }
            }
            Self::I2p => {
                if i2p_up {
                    (TransportKind::I2p, true)
                } else {
                    (TransportKind::Nostr, false)
                }
            }
            Self::Nostr => (TransportKind::Nostr, true),
        }
    }

    /// SOCKS5 socket as `String` when the mode is i2p; `None` otherwise.
    pub fn socks_url(self) -> Option<String> {
        matches!(self, Self::I2p).then(|| format!("socks5://{I2P_LOCAL_HOST}:{I2P_SOCKS_PORT}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_roundtrip() {
        for mode in [
            TransportMode::Default,
            TransportMode::Reticulum,
            TransportMode::Freenet,
            TransportMode::I2p,
            TransportMode::Nostr,
        ] {
            assert_eq!(TransportMode::parse_mode(mode.as_str()), Some(mode));
        }
        assert_eq!(TransportMode::parse_mode("bogus"), None);
    }

    #[test]
    fn test_parse_legacy_aliases() {
        assert_eq!(
            TransportMode::parse_mode("clearnet"),
            Some(TransportMode::Nostr)
        );
        assert_eq!(
            TransportMode::parse_mode("auto"),
            Some(TransportMode::Default)
        );
    }

    #[test]
    fn test_resolve_default_chain() {
        assert_eq!(
            TransportMode::Default.resolve(true, false, false),
            (TransportKind::Reticulum, true)
        );
        assert_eq!(
            TransportMode::Default.resolve(false, true, false),
            (TransportKind::Freenet, true)
        );
        assert_eq!(
            TransportMode::Default.resolve(false, false, true),
            (TransportKind::I2p, true)
        );
        assert_eq!(
            TransportMode::Default.resolve(false, false, false),
            (TransportKind::Nostr, true)
        );
    }

    #[test]
    fn test_resolve_only_modes_fall_back() {
        assert_eq!(
            TransportMode::Reticulum.resolve(false, true, true),
            (TransportKind::Nostr, false)
        );
        assert_eq!(
            TransportMode::Reticulum.resolve(true, false, false),
            (TransportKind::Reticulum, true)
        );
        assert_eq!(
            TransportMode::Freenet.resolve(false, false, true),
            (TransportKind::Nostr, false)
        );
        assert_eq!(
            TransportMode::I2p.resolve(false, false, false),
            (TransportKind::Nostr, false)
        );
        assert_eq!(
            TransportMode::Nostr.resolve(false, false, false),
            (TransportKind::Nostr, true)
        );
    }

    #[test]
    fn test_socks_url() {
        assert_eq!(TransportMode::Nostr.socks_url(), None);
        assert_eq!(TransportMode::Reticulum.socks_url(), None);
        assert_eq!(
            TransportMode::I2p.socks_url(),
            Some("socks5://127.0.0.1:4447".to_string())
        );
    }

    #[test]
    fn test_kind_as_str() {
        assert_eq!(TransportKind::Reticulum.as_str(), "reticulum");
        assert_eq!(TransportKind::Freenet.as_str(), "freenet");
        assert_eq!(TransportKind::I2p.as_str(), "i2p");
        assert_eq!(TransportKind::Nostr.as_str(), "nostr");
    }
}
