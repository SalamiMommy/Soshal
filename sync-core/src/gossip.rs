//! Gossip Integration for sync-core.
//! Connects PlumTree epidemic gossip protocol with SQLite event ingest and outbox pipelines.

use crate::SyncUpdate;
use soshal_common_core::bounded::BoundedSet;
use soshal_db_core::Database;
use soshal_network_core::plumtree::{PlumTreeMessage, PlumTreeNode};
use soshal_nostr_core::models::verify_event;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;

/// Cross-account gossip dedup set. Events are keyed per account
/// (`<account_pubkey>:<event_id>`); an event processed while one identity is
/// active must still ingest for a second identity (L7).
static SEEN_GOSSIP: LazyLock<Mutex<BoundedSet<String>>> =
    LazyLock::new(|| Mutex::new(BoundedSet::new(SEEN_GOSSIP_CAP)));

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
        let (outgoing, verified, my_pubkey) = {
            let mut pt = self.node.write().await;
            let my_pubkey = pt.self_peer_id.clone();
            // Verify before amplify: a Gossip must carry a signature-valid
            // event whose id binds the claimed message_id. Anything else is
            // dropped WITHOUT fan-out — an attacker cannot force replication
            // of unverified content by choosing an arbitrary message_id, and
            // dedup keys on the real event id, not the attacker-chosen one.
            let verified = match &msg {
                PlumTreeMessage::Gossip {
                    message_id,
                    payload_json,
                    ..
                } => match serde_json::from_str::<nostr::event::Event>(payload_json.as_ref()) {
                    Ok(event) if &event.id.to_hex() == message_id && verify_event(&event) => {
                        Some(event)
                    }
                    _ => None,
                },
                _ => None,
            };
            if matches!(&msg, PlumTreeMessage::Gossip { .. }) && verified.is_none() {
                (Vec::new(), None, my_pubkey)
            } else {
                let out = pt.handle_incoming(from_peer, msg.clone());
                (out, verified, my_pubkey)
            }
        };

        if let Some(event) = verified {
            let key = format!("{my_pubkey}:{}", event.id.to_hex());
            let fresh = {
                let mut seen = SEEN_GOSSIP.lock().unwrap_or_else(|e| e.into_inner());
                seen.insert(key)
            };
            if fresh {
                // Use bridge identity for p-tag-to-me checks: empty
                // pubkey would drop all gossip DMs (safe) but also
                // breaks own-DM relay via mesh. Read from node.
                // Ingest into SQLite database
                if let Err(e) = crate::ingest::handle(db, &my_pubkey, &event, tx) {
                    eprintln!("gossip ingest failed: {e}");
                }
            }
        }

        outgoing
    }

    /// Process a batch of incoming mesh gossip messages and update local DB / sync channels.
    /// Ingests all valid un-seen events in a single database transaction with parallel signature checks.
    pub async fn process_gossip_batch(
        &self,
        db: &Database,
        messages: &[(&str, PlumTreeMessage)],
        tx: &Sender<SyncUpdate>,
    ) -> Vec<(String, PlumTreeMessage)> {
        let mut all_outgoing = Vec::new();
        let (verified_events, my_pubkey) = {
            let mut pt = self.node.write().await;
            let my_pubkey = pt.self_peer_id.clone();
            let mut verified_events = Vec::new();
            for (from_peer, msg) in messages {
                let verified = match msg {
                    PlumTreeMessage::Gossip {
                        message_id,
                        payload_json,
                        ..
                    } => match serde_json::from_str::<nostr::event::Event>(payload_json.as_ref()) {
                        Ok(event) if &event.id.to_hex() == message_id && verify_event(&event) => {
                            Some(event)
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if matches!(msg, PlumTreeMessage::Gossip { .. }) && verified.is_none() {
                    // Unverified gossip: no plumtree state change and no
                    // fan-out (L1).
                    continue;
                }
                let out = pt.handle_incoming(from_peer, (*msg).clone());
                all_outgoing.extend(out);
                if let Some(ev) = verified {
                    verified_events.push(ev);
                }
            }
            (verified_events, my_pubkey)
        };

        let fresh_events: Vec<_> = verified_events
            .into_iter()
            .filter(|event| {
                // L7: dedup scope is per-account.
                let key = format!("{my_pubkey}:{}", event.id.to_hex());
                let mut seen = SEEN_GOSSIP.lock().unwrap_or_else(|e| e.into_inner());
                seen.insert(key)
            })
            .collect();

        if !fresh_events.is_empty() {
            if let Err(e) = crate::ingest::handle_batch(db, &my_pubkey, &fresh_events, tx) {
                eprintln!("gossip batch ingest failed: {e}");
            }
        }

        all_outgoing
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
        let keys = Keys::generate();
        let event =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "gossip relay", 1_700_001_000);
        let msg = PlumTreeMessage::Gossip {
            message_id: event.id.to_hex(),
            payload_json: serde_json::to_string(&event).unwrap().into(),
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
        let keys = Keys::generate();
        let event =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "dup prune", 1_700_001_001);
        let msg = PlumTreeMessage::Gossip {
            message_id: event.id.to_hex(),
            payload_json: serde_json::to_string(&event).unwrap().into(),
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
    async fn gossip_with_invalid_payload_not_amplified_or_ingested() {
        let db = soshal_test_util::test_db();
        let bridge = GossipSyncBridge::new("self");
        bridge.node.write().await.add_peer("peer_a");
        // L1: an unverified payload (junk, wrong message_id, or invalid
        // signature) must NOT be amplified to any peer nor ingested.
        let (tx, _rx) = channel();
        let bad = PlumTreeMessage::Gossip {
            message_id: "bad".to_string(),
            payload_json: "not an event".into(),
            round: 0,
        };
        let outgoing = bridge.process_gossip(&db, "sender", bad, &tx).await;
        assert!(outgoing.is_empty(), "no fan-out of unverified content");

        // Same content wrapped in a *valid* event but a mismatched
        // message_id is also refused.
        let keys = Keys::generate();
        let event =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "id mismatch", 1_700_001_002);
        let mismatched = PlumTreeMessage::Gossip {
            message_id: "ffff".repeat(32),
            payload_json: serde_json::to_string(&event).unwrap().into(),
            round: 0,
        };
        let outgoing = bridge.process_gossip(&db, "sender", mismatched, &tx).await;
        assert!(
            outgoing.is_empty(),
            "message_id must bind the real event id"
        );

        let count: i64 = query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM posts", (), |r| {
            r.get::<i64>(0)
        })
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn gossip_dedup_is_scoped_per_account() {
        // L7: an event ingested for account A must still ingest for account B
        // (the global dedup set is keyed by account+event, not event alone).
        let (tx, _rx) = channel();
        let keys = Keys::generate();
        let event =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "per-account", 1_700_001_003);
        let msg = PlumTreeMessage::Gossip {
            message_id: event.id.to_hex(),
            payload_json: serde_json::to_string(&event).unwrap().into(),
            round: 0,
        };

        let db_a = soshal_test_util::test_db();
        soshal_test_util::seed_user(&db_a, &keys.public_key().to_hex());
        let bridge_a = GossipSyncBridge::new("acct_a");
        bridge_a
            .process_gossip(&db_a, "sender", msg.clone(), &tx)
            .await;
        let row_a = PostRepo::new(&db_a)
            .get_by_id(&event.id.to_hex())
            .unwrap()
            .unwrap();
        assert_eq!(row_a.content, "per-account");

        // Second identity, fresh store: must NOT be deduped away just because
        // identity A already saw the event.
        let db_b = soshal_test_util::test_db();
        soshal_test_util::seed_user(&db_b, &keys.public_key().to_hex());
        let bridge_b = GossipSyncBridge::new("acct_b");
        bridge_b.process_gossip(&db_b, "sender", msg, &tx).await;
        let row_b = PostRepo::new(&db_b)
            .get_by_id(&event.id.to_hex())
            .unwrap()
            .unwrap();
        assert_eq!(row_b.content, "per-account");
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

    #[tokio::test(flavor = "multi_thread")]
    async fn gossip_batch_event_roundtrip_ingests() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
        let event1 =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "batch note 1", 1_700_000_001);
        let event2 =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "batch note 2", 1_700_000_002);
        let msg1 = PlumTreeMessage::Gossip {
            message_id: event1.id.to_hex(),
            payload_json: serde_json::to_string(&event1).unwrap().into(),
            round: 0,
        };
        let msg2 = PlumTreeMessage::Gossip {
            message_id: event2.id.to_hex(),
            payload_json: serde_json::to_string(&event2).unwrap().into(),
            round: 0,
        };

        let bridge = GossipSyncBridge::new("self");
        let (tx, _rx) = channel();
        let outgoing = bridge
            .process_gossip_batch(&db, &[("sender", msg1), ("sender", msg2)], &tx)
            .await;
        assert!(outgoing.is_empty());

        let row1 = PostRepo::new(&db)
            .get_by_id(&event1.id.to_hex())
            .unwrap()
            .unwrap();
        assert_eq!(row1.content, "batch note 1");

        let row2 = PostRepo::new(&db)
            .get_by_id(&event2.id.to_hex())
            .unwrap()
            .unwrap();
        assert_eq!(row2.content, "batch note 2");
    }
}
