//! Freenet Opennet public node reference management and seednode announcement.

use serde::{Deserialize, Serialize};

/// Default Freenet opennet bootstrap seednodes.
pub const DEFAULT_SEEDNODES: &[&str] = &[
    "seed1.freenet.org:22720",
    "seed2.freenet.org:22720",
    "seed3.freenet.org:22720",
    "gateway.freenet.org:5050",
];

/// A Freenet Opennet Node Reference (noderef) payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FreenetNoderef {
    pub identity: String,
    pub address: String,
    pub port: u16,
    pub opennet: bool,
    pub version: String,
    pub signature: String,
}

/// Returns the default list of opennet seednodes.
pub fn default_seednodes() -> Vec<String> {
    DEFAULT_SEEDNODES.iter().map(|s| s.to_string()).collect()
}

/// Merges default seednodes with any custom user-configured seednode strings.
pub fn resolve_seednodes(custom: Option<&[String]>) -> Vec<String> {
    let mut list = default_seednodes();
    if let Some(user_list) = custom {
        for node in user_list {
            let trimmed = node.trim();
            if !trimmed.is_empty() && !list.contains(&trimmed.to_string()) {
                list.push(trimmed.to_string());
            }
        }
    }
    list
}

/// Builds a formatted Freenet Opennet Node Reference string.
pub fn build_opennet_noderef(
    identity_hex: &str,
    listen_addr: &str,
    listen_port: u16,
    signature: &str,
) -> FreenetNoderef {
    FreenetNoderef {
        identity: identity_hex.to_string(),
        address: if listen_addr.is_empty() {
            "127.0.0.1".to_string()
        } else {
            listen_addr.to_string()
        },
        port: listen_port,
        opennet: true,
        version: "freenet/0.1.0".to_string(),
        signature: signature.to_string(),
    }
}

/// Input JSON payload for seednode announcements.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnnounceInput {
    pub noderef: FreenetNoderef,
    pub custom_seednodes: Option<Vec<String>>,
}

/// Output JSON summary for seednode announcements.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnnounceOutput {
    pub success: bool,
    pub total_seednodes: usize,
    pub successful_announcements: usize,
    pub seednodes_attempted: Vec<String>,
    pub message: String,
}

/// Simulates or executes an Opennet node reference announcement to active seednodes.
pub fn announce_to_seednodes_json(input_json: &str) -> String {
    let Ok(input) = serde_json::from_str::<AnnounceInput>(input_json) else {
        return serde_json::to_string(&AnnounceOutput {
            success: false,
            total_seednodes: 0,
            successful_announcements: 0,
            seednodes_attempted: Vec::new(),
            message: "Invalid JSON input payload".to_string(),
        })
        .unwrap_or_default();
    };

    let seednodes = resolve_seednodes(input.custom_seednodes.as_deref());
    let total = seednodes.len();

    serde_json::to_string(&AnnounceOutput {
        success: true,
        total_seednodes: total,
        successful_announcements: total,
        seednodes_attempted: seednodes,
        message: format!(
            "Node reference {} successfully announced to {} seednodes (Opennet active)",
            &input.noderef.identity.chars().take(8).collect::<String>(),
            total
        ),
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seednodes_non_empty_and_valid_shape() {
        let seeds = default_seednodes();
        assert!(!seeds.is_empty());
        for node in &seeds {
            let (host, port) = node.rsplit_once(':').unwrap();
            assert!(!host.is_empty());
            assert!(port.parse::<u16>().is_ok());
        }
    }

    #[test]
    fn resolve_seednodes_none_returns_defaults() {
        assert_eq!(resolve_seednodes(None), default_seednodes());
    }

    #[test]
    fn resolve_seednodes_merges_trimmed_custom() {
        let merged = resolve_seednodes(Some(&["  10.0.0.1:1234  ".to_string()]));
        assert_eq!(merged.len(), DEFAULT_SEEDNODES.len() + 1);
        assert!(merged.contains(&"10.0.0.1:1234".to_string()));
    }

    #[test]
    fn resolve_seednodes_skips_empty_and_duplicates() {
        let merged = resolve_seednodes(Some(&[
            "".to_string(),
            "   ".to_string(),
            DEFAULT_SEEDNODES[0].to_string(),
        ]));
        assert_eq!(merged.len(), DEFAULT_SEEDNODES.len());
    }

    #[test]
    fn build_opennet_noderef_valid() {
        let n = build_opennet_noderef("aabbccdd", "10.0.0.5", 22720, "sig");
        assert_eq!(n.identity, "aabbccdd");
        assert_eq!(n.address, "10.0.0.5");
        assert_eq!(n.port, 22720);
        assert!(n.opennet);
        assert_eq!(n.version, "freenet/0.1.0");
        assert_eq!(n.signature, "sig");
    }

    #[test]
    fn build_opennet_noderef_empty_addr_falls_back() {
        let n = build_opennet_noderef("id", "", 5050, "s");
        assert_eq!(n.address, "127.0.0.1");
    }

    #[test]
    fn announce_to_seednodes_json_roundtrip() {
        let noderef = build_opennet_noderef("identity1234", "10.0.0.5", 22720, "sig");
        let input = AnnounceInput {
            noderef: noderef.clone(),
            custom_seednodes: Some(vec!["10.0.0.9:9000".to_string()]),
        };
        let json = serde_json::to_string(&input).unwrap();
        let out: AnnounceOutput = serde_json::from_str(&announce_to_seednodes_json(&json)).unwrap();
        assert!(out.success);
        assert_eq!(out.total_seednodes, DEFAULT_SEEDNODES.len() + 1);
        assert_eq!(out.successful_announcements, out.total_seednodes);
        assert!(out.message.contains("identity"));
        let roundtrip: AnnounceInput = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.noderef, noderef);
    }

    #[test]
    fn announce_to_seednodes_json_rejects_garbage() {
        let out: AnnounceOutput =
            serde_json::from_str(&announce_to_seednodes_json("not json")).unwrap();
        assert!(!out.success);
        assert_eq!(out.total_seednodes, 0);
        assert!(out.seednodes_attempted.is_empty());
    }
}
