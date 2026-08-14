//! Gossip Integration for sync-core.
//! Connects PlumTree epidemic gossip protocol with SQLite event ingest and outbox pipelines.

use crate::SyncUpdate;
use soshal_db_core::Database;
use soshal_network_core::plumtree::{PlumTreeMessage, PlumTreeNode};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;

/// Binds PlumTree gossip protocol with DB event store.
#[derive(Clone)]
pub struct GossipSyncBridge {
    pub node: Arc<RwLock<PlumTreeNode>>,
}

impl GossipSyncBridge {
    pub fn new(self_pubkey: &str) -> Self {
        Self {
            node: Arc::new(RwLock::new(PlumTreeNode::new(self_pubkey))),
        }
    }

    /// Process incoming mesh gossip message and update local DB / sync channels.
    pub async fn process_gossip(
        &self,
        db: &Database,
        from_peer: &str,
        msg: PlumTreeMessage,
        tx: &Sender<SyncUpdate>,
    ) -> Vec<(String, PlumTreeMessage)> {
        let outgoing = {
            let mut pt = self.node.write().await;
            pt.handle_incoming(from_peer, msg.clone())
        };

        if let PlumTreeMessage::Gossip { payload_json, .. } = msg {
            if let Ok(event) = serde_json::from_str::<nostr::event::Event>(&payload_json) {
                // Ingest into SQLite database
                let _ = crate::ingest::handle(db, "", &event, tx);
            }
        }

        outgoing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gossip_bridge_initialization() {
        let bridge = GossipSyncBridge::new("my_pubkey");
        let node = bridge.node.read().await;
        assert_eq!(node.self_peer_id, "my_pubkey");
    }
}
