//! Wi-Fi Direct and Wi-Fi Aware high-speed off-grid socket controller.
//! Manages local P2P socket streams operating at 50+ Mbps with FastCDC chunk framing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Configuration for a Wi-Fi Direct / Wi-Fi Aware P2P socket connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiP2pConfig {
    pub peer_address: String,
    pub port: u16,
    pub is_group_owner: bool,
    pub passphrase: Option<String>,
}

/// Status of the Wi-Fi Direct link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiP2pStatus {
    Disconnected,
    Negotiating,
    Connected { peer_ip: String, speed_mbps: u32 },
    Failed(String),
}

/// Transport frame header for high-speed FastCDC chunk transfer over Wi-Fi Direct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkFrame {
    pub chunk_hash: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub payload_b64: String,
}

/// Active connection state manager for local off-grid sockets.
#[derive(Default, Clone)]
pub struct WifiDirectManager {
    pub active_links: Arc<RwLock<HashMap<String, WifiP2pStatus>>>,
}

impl WifiDirectManager {
    pub fn new() -> Self {
        Self {
            active_links: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Update status of a peer connection. Disconnected peers are evicted
    /// immediately to trim connection churn, and the tracked set is
    /// size-capped so abandoned peers can't grow it unboundedly (Failed
    /// states stay observable as the peer's last-known state).
    pub async fn set_status(&self, peer_id: &str, status: WifiP2pStatus) {
        const MAX_TRACKED_LINKS: usize = 256;
        let peer_id_clean = peer_id.trim().to_ascii_lowercase();
        let mut links = self.active_links.write().await;
        match status {
            WifiP2pStatus::Disconnected => {
                links.remove(&peer_id_clean);
            }
            _ => {
                if !links.contains_key(&peer_id_clean) && links.len() >= MAX_TRACKED_LINKS {
                    if let Some(oldest) = links.keys().next().cloned() {
                        links.remove(&oldest);
                    }
                }
                links.insert(peer_id_clean, status);
            }
        }
    }

    /// Retrieve status of a peer connection.
    pub async fn get_status(&self, peer_id: &str) -> WifiP2pStatus {
        let peer_id_clean = peer_id.trim().to_ascii_lowercase();
        let links = self.active_links.read().await;
        links
            .get(&peer_id_clean)
            .cloned()
            .unwrap_or(WifiP2pStatus::Disconnected)
    }

    /// Formats a FastCDC chunk for high-speed local socket transmission.
    pub fn format_chunk_frame(chunk_hash: &str, index: usize, total: usize, data: &[u8]) -> String {
        use base64::Engine;
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(data);
        let frame = ChunkFrame {
            chunk_hash: chunk_hash.trim().to_ascii_lowercase(),
            chunk_index: index,
            total_chunks: total,
            payload_b64,
        };
        serde_json::to_string(&frame).unwrap_or_default()
    }

    /// Parses an incoming chunk frame.
    pub fn parse_chunk_frame(raw: &str) -> Option<(String, usize, usize, Vec<u8>)> {
        if raw.len() > MAX_FRAME_BYTES * 2 {
            return None;
        }
        use base64::Engine;
        let frame: ChunkFrame = serde_json::from_str(raw).ok()?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(&frame.payload_b64)
            .ok()?;
        if data.len() > MAX_FRAME_BYTES {
            return None;
        }
        Some((
            frame.chunk_hash.trim().to_ascii_lowercase(),
            frame.chunk_index,
            frame.total_chunks,
            data,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wifi_direct_status_and_chunk_framing() {
        let manager = WifiDirectManager::new();
        manager
            .set_status(
                "peer1",
                WifiP2pStatus::Connected {
                    peer_ip: "192.168.49.2".to_string(),
                    speed_mbps: 65,
                },
            )
            .await;

        let status = manager.get_status("peer1").await;
        assert_eq!(
            status,
            WifiP2pStatus::Connected {
                peer_ip: "192.168.49.2".to_string(),
                speed_mbps: 65,
            }
        );

        let data = b"fastcdc chunk payload bytes";
        let frame_str = WifiDirectManager::format_chunk_frame("hash123", 0, 1, data);
        let parsed = WifiDirectManager::parse_chunk_frame(&frame_str);
        assert!(parsed.is_some());
        let (hash, idx, total, bytes) = parsed.unwrap();
        assert_eq!(hash, "hash123");
        assert_eq!(idx, 0);
        assert_eq!(total, 1);
        assert_eq!(bytes, data);
    }
}
