//! PlumTree (Epidemic Broadcast Tree) Gossip Protocol.
//! Eliminates 80-90% of redundant network payload broadcasts by dynamically constructing an optimal
//! spanning tree across active peers with eager payload pushes and lazy `IHave` announcements.

use serde::{Deserialize, Serialize};
use soshal_common_core::bounded::{BoundedMap, BoundedSet};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_RECEIVED_MESSAGES: usize = 10_000;
/// Cap on outstanding graft requests; oldest evicted past it (bounds
/// attacker-spoofed IHave memory).
const MAX_PENDING_GRAFTS: usize = 4_096;
/// Cap on eager (payload-push) peers per node; new peers beyond it land in
/// the lazy set. Bounds fan-out cost under conn-churn.
const EAGER_PEER_CAP: usize = 64;
/// Cap on lazy (announce-only) peers per node.
const LAZY_PEER_CAP: usize = 256;
/// Per-peer IHave-triggered graft credit window. 16 grafts/second/peer caps
/// fake-IHave floods without hindering a healthy lazy tree.
const GRAFT_WINDOW: Duration = Duration::from_secs(1);
const GRAFT_CREDITS_PER_WINDOW: u32 = 16;

/// Types of messages in the PlumTree protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlumTreeMessage {
    /// Eager full post/event payload push. Payload shared via `Arc` so the
    /// fan-out to eager peers refcounts instead of cloning the full JSON.
    Gossip {
        message_id: String,
        payload_json: Arc<str>,
        round: u32,
    },
    /// Lazy message announcement (hash only).
    IHave { message_id: String, round: u32 },
    /// Graft request to missing payload from lazy link and promote to eager spanning tree.
    Graft { message_id: String },
    /// Prune request to demote link from eager tree to lazy announcement set.
    Prune { message_id: String },
}

/// PlumTree Node Manager for managing peer eager/lazy links per topic.
#[derive(Debug)]
pub struct PlumTreeNode {
    pub self_peer_id: String,
    pub eager_peers: HashSet<String>,
    pub lazy_peers: HashSet<String>,
    pub received_messages: BoundedSet<String>,
    pub pending_grafts: BoundedMap<String, String>,
    /// Per-peer graft credit (window start, remaining credits) for IHave
    /// floods. Cleared implicitly when the window elapses.
    graft_windows: HashMap<String, (Instant, u32)>,
}

impl PlumTreeNode {
    pub fn new(self_peer_id: &str) -> Self {
        Self {
            self_peer_id: self_peer_id.to_string(),
            eager_peers: HashSet::new(),
            lazy_peers: HashSet::new(),
            received_messages: BoundedSet::new(MAX_RECEIVED_MESSAGES),
            pending_grafts: BoundedMap::new(MAX_PENDING_GRAFTS),
            graft_windows: HashMap::new(),
        }
    }

    /// Adds a newly connected peer. Eager (payload-push) by default, up to
    /// [`EAGER_PEER_CAP`]; past that the peer joins the lazy (announce-only)
    /// set, up to [`LAZY_PEER_CAP`]. Beyond both, the peer is not admitted —
    /// membership is bounded so no single attacker can grow the fan-out
    /// without bound.
    pub fn add_peer(&mut self, peer_id: &str) {
        if peer_id == self.self_peer_id {
            return;
        }
        if self.eager_peers.len() < EAGER_PEER_CAP {
            self.eager_peers.insert(peer_id.to_string());
        } else if self.lazy_peers.len() < LAZY_PEER_CAP {
            self.lazy_peers.insert(peer_id.to_string());
        }
    }

    /// Removes a disconnected peer.
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.eager_peers.remove(peer_id);
        self.lazy_peers.remove(peer_id);
    }

    /// Process an incoming message and return outgoing messages mapped by target peer ID.
    pub fn handle_incoming(
        &mut self,
        from_peer: &str,
        msg: PlumTreeMessage,
    ) -> Vec<(String, PlumTreeMessage)> {
        let mut outgoing = Vec::new();

        match msg {
            PlumTreeMessage::Gossip {
                message_id,
                payload_json,
                round,
            } => {
                if self.received_messages.contains(&message_id) {
                    // Duplicate payload received on eager link - send Prune to sender!
                    self.eager_peers.remove(from_peer);
                    self.lazy_peers.insert(from_peer.to_string());
                    outgoing.push((
                        from_peer.to_string(),
                        PlumTreeMessage::Prune {
                            message_id: message_id.clone(),
                        },
                    ));
                } else {
                    // New payload accepted
                    self.received_messages.insert(message_id.clone());
                    self.pending_grafts.remove(&message_id);

                    // Forward eagerly to all eager peers except sender
                    for eager_peer in &self.eager_peers {
                        if eager_peer != from_peer {
                            outgoing.push((
                                eager_peer.clone(),
                                PlumTreeMessage::Gossip {
                                    message_id: message_id.clone(),
                                    payload_json: payload_json.clone(),
                                    round: round.saturating_add(1),
                                },
                            ));
                        }
                    }

                    // Forward lazily (IHave) to all lazy peers
                    for lazy_peer in &self.lazy_peers {
                        if lazy_peer != from_peer {
                            outgoing.push((
                                lazy_peer.clone(),
                                PlumTreeMessage::IHave {
                                    message_id: message_id.clone(),
                                    round: round.saturating_add(1),
                                },
                            ));
                        }
                    }
                }
            }
            PlumTreeMessage::IHave { message_id, .. } => {
                // Only trust announced ids that look like real hashes
                // (64-hex). Anything else can't be a genuine event id and is
                // dropped before it can occupy pending-graft memory.
                if !is_plausible_event_id(&message_id) {
                    return outgoing;
                }
                // Rate-limit IHave-triggered grafts per peer (16/s): a
                // fake-IHave flood must not evict legitimate grafts from the
                // pending set or spend unbounded outbound graft traffic.
                if !self.consume_graft_credit(from_peer) {
                    return outgoing;
                }
                if !self.received_messages.contains(&message_id)
                    && !self.pending_grafts.contains_key(&message_id)
                {
                    // Unseen payload announced lazily - trigger Graft to fetch full payload!
                    self.pending_grafts
                        .insert(message_id.clone(), from_peer.to_string());
                    outgoing.push((from_peer.to_string(), PlumTreeMessage::Graft { message_id }));
                }
            }
            PlumTreeMessage::Graft { message_id } => {
                // Only promote a peer that is already a member — an unknown
                // peer must not be able to inject itself into the eager tree
                // (and from there into every payload fan-out) with a spoofed
                // Graft.
                if self.eager_peers.contains(from_peer) || self.lazy_peers.contains(from_peer) {
                    // Promote peer to eager link
                    self.lazy_peers.remove(from_peer);
                    self.eager_peers.insert(from_peer.to_string());

                    // Note: caller will re-send payload for message_id if available in local DB
                    let _ = message_id;
                }
            }
            PlumTreeMessage::Prune { .. } => {
                // Demote peer to lazy link
                self.eager_peers.remove(from_peer);
                self.lazy_peers.insert(from_peer.to_string());
            }
        }

        outgoing
    }

    /// Decrements the sending peer's graft credit for the current window,
    /// returning `false` (and spending nothing) once the per-window budget is
    /// exhausted, `true` when a graft may be issued.
    fn consume_graft_credit(&mut self, peer: &str) -> bool {
        let now = Instant::now();
        let entry = self
            .graft_windows
            .entry(peer.to_string())
            .or_insert((now, GRAFT_CREDITS_PER_WINDOW));
        if now.duration_since(entry.0) >= GRAFT_WINDOW {
            *entry = (now, GRAFT_CREDITS_PER_WINDOW);
        }
        if entry.1 == 0 {
            false
        } else {
            entry.1 -= 1;
            true
        }
    }
}

/// Returns true when `id` has the shape of a real 256-bit hash: exactly
/// 64 lowercase hex chars (nostr event ids). Anything else is attacker
/// junk, unusable as an event id.
fn is_plausible_event_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plumtree_eager_lazy_propagation_and_prune() {
        let mut node = PlumTreeNode::new("self");
        node.add_peer("peer_eager");
        node.lazy_peers.insert("peer_lazy".to_string());

        let gossip = PlumTreeMessage::Gossip {
            message_id: "msg1".to_string(),
            payload_json: "{\"text\":\"hello\"}".into(),
            round: 1,
        };

        let outgoing = node.handle_incoming("sender", gossip);

        assert!(node.received_messages.contains(&"msg1".to_string()));
        assert_eq!(outgoing.len(), 2); // 1 eager gossip + 1 lazy IHave

        // Test duplicate receipt -> Prune
        let duplicate_gossip = PlumTreeMessage::Gossip {
            message_id: "msg1".to_string(),
            payload_json: "{\"text\":\"hello\"}".into(),
            round: 1,
        };

        let dup_out = node.handle_incoming("peer_eager", duplicate_gossip);
        assert_eq!(dup_out.len(), 1);
        assert!(matches!(dup_out[0].1, PlumTreeMessage::Prune { .. }));
        assert!(node.lazy_peers.contains("peer_eager"));
    }

    #[test]
    fn test_peer_management_self_guard_and_removal() {
        let mut node = PlumTreeNode::new("self");
        node.add_peer("self");
        node.add_peer("peer_a");
        assert!(!node.eager_peers.contains("self"));
        assert!(node.eager_peers.contains("peer_a"));

        node.lazy_peers.insert("peer_lazy".to_string());
        node.remove_peer("peer_a");
        node.remove_peer("peer_lazy");
        assert!(!node.eager_peers.contains("peer_a"));
        assert!(!node.lazy_peers.contains("peer_lazy"));
    }

    #[test]
    fn test_ihave_noop_graft_promote_prune_demote_and_graft_cleanup() {
        let mut node = PlumTreeNode::new("self");
        node.add_peer("peer_eager");
        node.lazy_peers.insert("peer_lazy".to_string());
        let seen = "aa".repeat(32);
        let pending = "bb".repeat(32);
        let fresh = "cc".repeat(32);

        // IHave for already-seen message -> no-op
        node.received_messages.insert(seen.clone());
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::IHave {
                message_id: seen.clone(),
                round: 1,
            },
        );
        assert!(out.is_empty());
        assert!(!node.pending_grafts.contains_key(&seen));

        // IHave for message already pending graft -> no-op
        node.pending_grafts
            .insert(pending.clone(), "peer_lazy".to_string());
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::IHave {
                message_id: pending.clone(),
                round: 1,
            },
        );
        assert!(out.is_empty());

        // Fresh IHave -> Graft + pending entry
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::IHave {
                message_id: fresh.clone(),
                round: 1,
            },
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].1, PlumTreeMessage::Graft { .. }));
        assert_eq!(
            node.pending_grafts.get(&fresh).map(String::as_str),
            Some("peer_lazy")
        );

        // Graft promotes lazy -> eager
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::Graft {
                message_id: fresh.clone(),
            },
        );
        assert!(out.is_empty());
        assert!(node.eager_peers.contains("peer_lazy"));
        assert!(!node.lazy_peers.contains("peer_lazy"));

        // Gossip arrival clears pending graft
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::Gossip {
                message_id: fresh.clone(),
                payload_json: "{}".into(),
                round: 1,
            },
        );
        assert!(!node.pending_grafts.contains_key(&fresh));
        assert!(node.received_messages.contains(&fresh));
        // sender was promoted to eager; only the remaining eager peer
        // (peer_eager) gets the forward, no IHave to lazy peers
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "peer_eager");
        assert!(matches!(
            &out[0].1,
            PlumTreeMessage::Gossip { round: 2, .. }
        ));

        // Prune demotes eager -> lazy
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::Prune {
                message_id: fresh.clone(),
            },
        );
        assert!(out.is_empty());
        assert!(!node.eager_peers.contains("peer_lazy"));
        assert!(node.lazy_peers.contains("peer_lazy"));
    }

    #[test]
    fn test_graft_from_unknown_peer_ignored() {
        let mut node = PlumTreeNode::new("self");
        node.add_peer("peer_eager");
        // A peer that is in neither set must not be able to inject itself
        // into the eager tree with a spoofed Graft.
        let out = node.handle_incoming(
            "unknown_attacker",
            PlumTreeMessage::Graft {
                message_id: "dd".repeat(32),
            },
        );
        assert!(out.is_empty());
        assert!(!node.eager_peers.contains("unknown_attacker"));
        assert!(!node.lazy_peers.contains("unknown_attacker"));
    }

    #[test]
    fn test_implausible_ihave_id_dropped() {
        let mut node = PlumTreeNode::new("self");
        node.add_peer("peer_eager");
        node.lazy_peers.insert("peer_lazy".to_string());
        let out = node.handle_incoming(
            "peer_lazy",
            PlumTreeMessage::IHave {
                message_id: "not-a-valid-hash".to_string(),
                round: 1,
            },
        );
        assert!(out.is_empty(), "junk id must not trigger a graft");
        assert!(node.pending_grafts.is_empty());
    }

    #[test]
    fn test_ihave_flood_rate_limited_per_peer() {
        let mut node = PlumTreeNode::new("self");
        node.lazy_peers.insert("peer_lazy".to_string());
        let mut grafts = 0;
        for i in 0..(GRAFT_CREDITS_PER_WINDOW + 8) {
            let id = format!("{:02x}", i & 0xff).repeat(32);
            let out = node.handle_incoming(
                "peer_lazy",
                PlumTreeMessage::IHave {
                    message_id: id.clone(),
                    round: 1,
                },
            );
            if !out.is_empty() {
                grafts += 1;
            }
            assert_eq!(
                node.pending_grafts.contains_key(&id),
                !out.is_empty(),
                "pending entry only for granted grafts"
            );
        }
        // Credited grafts granted, the rest of the window suppressed.
        assert_eq!(grafts, GRAFT_CREDITS_PER_WINDOW);
        // A different peer (its own window) is unaffected.
        let out = node.handle_incoming(
            "peer_other",
            PlumTreeMessage::IHave {
                message_id: "ff".repeat(32),
                round: 1,
            },
        );
        assert!(!out.is_empty(), "unrelated peer has its own credit");
    }

    #[test]
    fn test_peer_caps_bound_fanout() {
        let mut node = PlumTreeNode::new("self");
        for i in 0..(EAGER_PEER_CAP + LAZY_PEER_CAP + 16) {
            node.add_peer(&format!("peer-{i}"));
        }
        assert_eq!(node.eager_peers.len(), EAGER_PEER_CAP);
        assert_eq!(node.lazy_peers.len(), LAZY_PEER_CAP);
        // Beyond the combined caps a peer is admitted to neither set: total
        // membership is bounded, so excess spoofed ids buy nothing.
        assert!(!node
            .eager_peers
            .contains(&format!("peer-{}", EAGER_PEER_CAP + LAZY_PEER_CAP)));
        assert!(!node
            .lazy_peers
            .contains(&format!("peer-{}", EAGER_PEER_CAP + LAZY_PEER_CAP)));
    }
}
