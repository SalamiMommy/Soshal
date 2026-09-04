//! Gossip Integration for sync-core.
//! Connects PlumTree epidemic gossip protocol with SQLite event ingest and outbox pipelines.

use crate::SyncUpdate;
use soshal_db_core::Database;
use soshal_network_core::plumtree::{PlumTreeMessage, PlumTreeNode};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;

static SEEN_GOSSIP: LazyLock<Mutex<(VecDeque<String>, HashSet<String>)>> =
    LazyLock::new(|| Mutex::new((VecDeque::new(), HashSet::new())));

const SEEN_GOSSIP_CAP: usize = 10_000;

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
            if let Ok(event) = serde_json::from_str::<nostr::event::Event>(payload_json.as_ref()) {
                let key = event.id.to_hex();
                let fresh = {
                    let mut seen = SEEN_GOSSIP.lock().unwrap_or_else(|e| e.into_inner());
                    if seen.1.insert(key.clone()) {
                        seen.0.push_back(key);
                        if seen.0.len() > SEEN_GOSSIP_CAP {
                            if let Some(oldest) = seen.0.pop_front() {
                                seen.1.remove(&oldest);
                            }
                        }
                        true
                    } else {
                        false
                    }
                };
                if fresh {
                    // Use bridge identity for p-tag-to-me checks: empty
                    // pubkey would drop all gossip DMs (safe) but also
                    // breaks own-DM relay via mesh. Read from node.
                    let my_pubkey = self.node.read().await.self_peer_id.clone();
                    // Ingest into SQLite database
                    let _ = crate::ingest::handle(db, &my_pubkey, &event, tx);
                }
            }
        }

        outgoing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::event::Kind;
    use nostr::key::Keys;
    use soshal_db_core::query::query_first;
    use soshal_db_core::repos::post::PostRepo;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_gossip_bridge_initialization() {
        let bridge = GossipSyncBridge::new("my_pubkey");
        let node = bridge.node.read().await;
        assert_eq!(node.self_peer_id, "my_pubkey");
    }

    fn channel() -> (Sender<SyncUpdate>, tokio::sync::mpsc::Receiver<SyncUpdate>) {
        tokio::sync::mpsc::channel(16)
    }

    #[test]
    fn message_encode_decode_roundtrip() {
        let messages = vec![
            PlumTreeMessage::Gossip {
                message_id: "m1".to_string(),
                payload_json: r#"{"content":"hi"}"#.into(),
                round: 3,
            },
            PlumTreeMessage::IHave {
                message_id: "m1".to_string(),
                round: 3,
            },
            PlumTreeMessage::Graft {
                message_id: "m1".to_string(),
            },
            PlumTreeMessage::Prune {
                message_id: "m1".to_string(),
            },
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let decoded: PlumTreeMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
        }
    }

    #[test]
    fn invalid_message_json_rejected() {
        assert!(serde_json::from_str::<PlumTreeMessage>("not json").is_err());
        assert!(serde_json::from_str::<PlumTreeMessage>(r#"{"Gossip":{}}"#).is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn gossip_relays_to_eager_and_lazy_peers() {
        let db = soshal_test_util::test_db();
        let bridge = GossipSyncBridge::new("self");
        bridge.node.write().await.add_peer("eager_a");
        bridge.node.write().await.add_peer("eager_b");
        bridge
            .node
            .write()
            .await
            .lazy_peers
            .insert("lazy_c".to_string());
        let msg = PlumTreeMessage::Gossip {
            message_id: "m1".to_string(),
            payload_json: "{}".into(),
            round: 0,
        };
        let (tx, _rx) = channel();
        let outgoing = bridge.process_gossip(&db, "sender", msg, &tx).await;

        assert_eq!(outgoing.len(), 3);
        assert!(outgoing
            .iter()
            .any(|(p, m)| p == "eager_a" && matches!(m, PlumTreeMessage::Gossip { .. })));
        assert!(outgoing
            .iter()
            .any(|(p, m)| p == "eager_b" && matches!(m, PlumTreeMessage::Gossip { .. })));
        assert!(outgoing
            .iter()
            .any(|(p, m)| p == "lazy_c" && matches!(m, PlumTreeMessage::IHave { .. })));
        assert!(!outgoing.iter().any(|(p, _)| p == "sender"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn duplicate_gossip_prunes_sender() {
        let db = soshal_test_util::test_db();
        let bridge = GossipSyncBridge::new("self");
        bridge.node.write().await.add_peer("peer_a");
        bridge.node.write().await.add_peer("peer_b");
        let msg = PlumTreeMessage::Gossip {
            message_id: "m1".to_string(),
            payload_json: "{}".into(),
            round: 0,
        };
        let (tx, _rx) = channel();
        let first = bridge.process_gossip(&db, "peer_a", msg.clone(), &tx).await;
        assert!(first
            .iter()
            .any(|(p, m)| p == "peer_b" && matches!(m, PlumTreeMessage::Gossip { .. })));

        let second = bridge.process_gossip(&db, "peer_a", msg, &tx).await;
        assert!(second
            .iter()
            .any(|(p, m)| p == "peer_a" && matches!(m, PlumTreeMessage::Prune { .. })));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn gossip_with_invalid_payload_not_ingested() {
        let db = soshal_test_util::test_db();
        let bridge = GossipSyncBridge::new("self");
        bridge.node.write().await.add_peer("peer_a");
        let msg = PlumTreeMessage::Gossip {
            message_id: "bad".to_string(),
            payload_json: "not an event".into(),
            round: 0,
        };
        let (tx, _rx) = channel();
        let outgoing = bridge.process_gossip(&db, "sender", msg, &tx).await;
        assert!(outgoing
            .iter()
            .any(|(p, m)| p == "peer_a" && matches!(m, PlumTreeMessage::Gossip { .. })));

        let count: i64 = query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM posts", (), |r| {
            r.get::<i64>(0)
        })
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn gossip_event_roundtrip_ingests() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
        let event =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "mesh roundtrip", 1_700_000_000);
        let msg = PlumTreeMessage::Gossip {
            message_id: event.id.to_hex(),
            payload_json: serde_json::to_string(&event).unwrap().into(),
            round: 0,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: PlumTreeMessage = serde_json::from_str(&json).unwrap();

        let bridge = GossipSyncBridge::new("self");
        let (tx, _rx) = channel();
        let outgoing = bridge.process_gossip(&db, "sender", decoded, &tx).await;
        assert!(outgoing.is_empty());

        let row = PostRepo::new(&db)
            .get_by_id(&event.id.to_hex())
            .unwrap()
            .unwrap();
        assert_eq!(row.content, "mesh roundtrip");
    }
}
