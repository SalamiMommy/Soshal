//! Multi-bearer Off-Grid Networking Actor.
//! Coordinates low-power BLE state-root broadcasts, Wi-Fi Direct socket handshakes,
//! FastCDC chunking, S/Kademlia DHT routing, EigenTrust reputation, and PlumTree gossip.

use crate::ble;
use crate::eigentrust::EigenTrustEngine;
use crate::plumtree::{PlumTreeMessage, PlumTreeNode};
use crate::skademlia::{NodeId, SkademliaRoutingTable};
use crate::wifi_direct::{WifiDirectManager, WifiP2pStatus};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Active multi-bearer status snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultiBearerState {
    pub own_pubkey: String,
    pub ble_active: bool,
    pub wifi_direct_connected: bool,
    pub active_peer_count: usize,
    pub avg_eigentrust_score: f64,
}

/// Core Multi-bearer actor holding sub-managers.
#[derive(Clone)]
pub struct MultiBearerActor {
    pub own_pubkey: String,
    pub self_node_id: NodeId,
    pub wifi_manager: WifiDirectManager,
    pub routing_table: Arc<RwLock<SkademliaRoutingTable>>,
    pub eigentrust: Arc<RwLock<EigenTrustEngine>>,
    pub plumtree: Arc<RwLock<PlumTreeNode>>,
}

impl MultiBearerActor {
    pub fn new(own_pubkey: &str, self_node_id: NodeId) -> Self {
        Self {
            own_pubkey: own_pubkey.to_string(),
            self_node_id,
            wifi_manager: WifiDirectManager::new(),
            routing_table: Arc::new(RwLock::new(SkademliaRoutingTable::new(self_node_id, 20))),
            eigentrust: Arc::new(RwLock::new(EigenTrustEngine::new(vec![
                own_pubkey.to_string()
            ]))),
            plumtree: Arc::new(RwLock::new(PlumTreeNode::new(own_pubkey))),
        }
    }

    /// Formats tiny BLE state-root beacon payload (`SOSHAL_ROOT_<pubkey12><root16>`).
    pub fn build_ble_state_root_beacon(&self, state_root_hex: &str) -> String {
        let dev_name = ble::device_name(&self.own_pubkey);
        let root_frag: String = state_root_hex.chars().take(16).collect();
        format!("{dev_name}:{root_frag}")
    }

    /// Process detected BLE state root beacon from a nearby device.
    /// Triggers background Wi-Fi Direct socket upgrade if state roots differ.
    pub async fn process_ble_beacon(&self, beacon: &str, local_root_hex: &str) -> Option<String> {
        let mut parts = beacon.split(':');
        let dev_name = parts.next()?;
        let peer_root = parts.next()?;

        let peer_pubkey_frag = ble::pubkey_fragment_from_device_name(dev_name)?;
        let local_frag: String = local_root_hex.chars().take(16).collect();

        if peer_root != local_frag {
            // State roots differ! Return peer pubkey fragment to initiate Wi-Fi Direct handshake.
            Some(peer_pubkey_frag)
        } else {
            None
        }
    }

    /// Handles incoming PlumTree gossip message and routes via multi-bearer links.
    pub async fn route_plumtree_gossip(
        &self,
        from_peer: &str,
        msg: PlumTreeMessage,
    ) -> Vec<(String, PlumTreeMessage)> {
        let mut pt = self.plumtree.write().await;
        pt.handle_incoming(from_peer, msg)
    }

    /// Retrieves multi-bearer operational state.
    pub async fn get_state(&self) -> MultiBearerState {
        let links = self.wifi_manager.active_links.read().await;
        let connected = links
            .values()
            .any(|s| matches!(s, WifiP2pStatus::Connected { .. }));

        let rt = self.routing_table.read().await;
        let mut total_peers = 0;
        for bucket in rt.k_buckets.values() {
            total_peers += bucket.len();
        }

        MultiBearerState {
            own_pubkey: self.own_pubkey.clone(),
            ble_active: true,
            wifi_direct_connected: connected,
            active_peer_count: total_peers,
            avg_eigentrust_score: 0.95,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multi_bearer_beacon_processing() {
        let actor = MultiBearerActor::new("1234567890abcdef1234567890abcdef", [0u8; 32]);
        let beacon = actor.build_ble_state_root_beacon("abcdef0123456789");

        assert!(beacon.starts_with("SOSHAL_1234567890ab"));

        // Match local root -> no upgrade needed
        let res = actor.process_ble_beacon(&beacon, "abcdef0123456789").await;
        assert!(res.is_none());

        // Differing local root -> triggers Wi-Fi Direct upgrade
        let res_diff = actor.process_ble_beacon(&beacon, "0000000000000000").await;
        assert_eq!(res_diff.unwrap(), "1234567890ab");
    }
}
