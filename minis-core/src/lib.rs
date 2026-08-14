//! Pure domain logic for Mini-Apps, Musicloud, and Custom Profile parsing.

pub mod events;
pub mod runtime;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiniManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub entry_url: String,
    pub icon_url: Option<String>,
    pub author_pubkey: String,
}

/// Parses a Mini-App JSON payload from Nostr event content.
pub fn parse_mini_manifest(content_json: &str) -> Result<MiniManifest, String> {
    serde_json::from_str::<MiniManifest>(content_json)
        .map_err(|e| format!("invalid mini manifest: {}", e))
}
