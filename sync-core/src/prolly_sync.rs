//! Pipelined Merkle-CRDT Prolly Tree Sync Protocol.
//! Traverses Prolly Tree branches over P2P sockets in $O(\log N)$ steps to pinpoint missing post/comment IDs.

use crate::prolly_tree::ProllyTree;
use serde::{Deserialize, Serialize};

/// Sync messages exchanged during Prolly Tree reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProllySyncMessage {
    /// Initial handshake exchanging top-level root hash.
    RootExchange { root_hash: String },
    /// Root hash match acknowledgement (sync complete).
    Match,
    /// Request child node hashes for differing branch.
    RequestBranch { level: u32, node_hash: String },
    /// Response with child keys and hashes.
    ResponseBranch {
        node_hash: String,
        keys: Vec<String>,
        child_hashes: Vec<String>,
    },
    /// Final delta request for specific missing post IDs.
    RequestDeltas { missing_ids: Vec<String> },
    /// Binary delta payload containing requested events (legacy: full JSON).
    ResponseDeltas { payload_json: String },
    /// bsdiff patch applied against the requester's stored base event bytes.
    /// The requester verifies the reconstructed bytes hash to `new_id` before
    /// persisting — corrupt patches are dropped, never applied.
    ResponsePatch {
        base_id: String,
        new_id: String,
        patch_b64: String,
        full_b64: Option<String>,
    },
}

/// Cap on keys accepted from a single `ResponseBranch` (peer memory bound).
const MAX_KEYS_PER_BRANCH: usize = 10_000;
/// Cap on total missing keys tracked per session (unbounded-growth bound).
const MAX_MISSING_KEYS_TOTAL: usize = 100_000;

/// Prolly Sync Session Manager for negotiating diffs with a connected peer.
#[derive(Debug)]
pub struct ProllySyncSession {
    pub local_tree: ProllyTree,
    pub peer_root_hash: Option<String>,
    pub missing_keys: Vec<String>,
    node_index: std::collections::HashMap<String, usize>,
    local_key_set: std::collections::HashSet<String>,
}

impl ProllySyncSession {
    pub fn new(local_tree: ProllyTree) -> Self {
        let node_index = local_tree
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, n)| (n.node_hash.clone(), idx))
            .collect();
        let local_key_set = local_tree
            .nodes
            .iter()
            .flat_map(|n| &n.keys)
            .cloned()
            .collect();
        Self {
            local_tree,
            peer_root_hash: None,
            missing_keys: Vec::new(),
            node_index,
            local_key_set,
        }
    }

    /// Handles incoming Prolly sync message and returns response message if needed.
    pub fn handle_message(&mut self, msg: ProllySyncMessage) -> Option<ProllySyncMessage> {
        match msg {
            ProllySyncMessage::RootExchange { root_hash } => {
                self.peer_root_hash = Some(root_hash.clone());
                if root_hash == self.local_tree.root_hash {
                    Some(ProllySyncMessage::Match)
                } else {
                    // Roots differ - request child branch at top level
                    Some(ProllySyncMessage::RequestBranch {
                        level: 0,
                        node_hash: root_hash,
                    })
                }
            }
            ProllySyncMessage::Match => None,
            ProllySyncMessage::RequestBranch { node_hash, .. } => self
                .node_index
                .get(&node_hash)
                .map(|&idx| ProllySyncMessage::ResponseBranch {
                    node_hash,
                    keys: self.local_tree.nodes[idx].keys.clone(),
                    child_hashes: self.local_tree.nodes[idx].values_or_child_hashes.clone(),
                }),
            ProllySyncMessage::ResponseBranch { keys, .. } => {
                // Reject oversized branches outright: a peer must not grow
                // our memory via unlimited fresh keys in one message.
                if keys.len() > MAX_KEYS_PER_BRANCH {
                    return None;
                }
                // Determine missing keys compared to local tree keys and already-tracked missing keys.
                // Filter out empty or oversized keys (> 128 bytes).
                let missing: Vec<String> = keys
                    .into_iter()
                    .filter(|k| !k.is_empty() && k.len() <= 128)
                    .filter(|k| !self.local_key_set.contains(k) && !self.missing_keys.contains(k))
                    .collect();

                if !missing.is_empty() {
                    // Total missing-set cap exceeded — drop, never accumulate.
                    if self.missing_keys.len() + missing.len() > MAX_MISSING_KEYS_TOTAL {
                        return None;
                    }
                    self.missing_keys.extend(missing.clone());
                    Some(ProllySyncMessage::RequestDeltas {
                        missing_ids: missing,
                    })
                } else {
                    Some(ProllySyncMessage::Match)
                }
            }
            ProllySyncMessage::RequestDeltas { .. } => {
                // Caller handles reading actual event payloads from SQLite and returning ResponseDeltas
                None
            }
            ProllySyncMessage::ResponseDeltas { .. } | ProllySyncMessage::ResponsePatch { .. } => {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prolly_sync_reconciliation() {
        let kv1 = vec![("post_1".to_string(), "data1".to_string())];
        let kv2 = vec![
            ("post_1".to_string(), "data1".to_string()),
            ("post_2".to_string(), "data2".to_string()),
        ];

        let tree1 = ProllyTree::build(&kv1);
        let tree2 = ProllyTree::build(&kv2);

        let mut session = ProllySyncSession::new(tree1);

        // Send root exchange from peer holding tree2
        let resp = session.handle_message(ProllySyncMessage::RootExchange {
            root_hash: tree2.root_hash,
        });

        assert!(matches!(
            resp,
            Some(ProllySyncMessage::RequestBranch { .. })
        ));
    }

    #[test]
    fn patch_message_serde_roundtrip() {
        let msg = ProllySyncMessage::ResponsePatch {
            base_id: "a".repeat(64),
            new_id: "b".repeat(64),
            patch_b64: "QUJD".to_string(),
            full_b64: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ProllySyncMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn response_branch_caps_oversized_messages() {
        let tree = ProllyTree::build(&[("post_1".to_string(), "data1".to_string())]);
        let mut session = ProllySyncSession::new(tree);
        let oversized = ProllySyncMessage::ResponseBranch {
            node_hash: "h".to_string(),
            keys: (0..=MAX_KEYS_PER_BRANCH).map(|i| format!("k{i}")).collect(),
            child_hashes: Vec::new(),
        };
        assert_eq!(session.handle_message(oversized), None);
        assert!(session.missing_keys.is_empty());
    }
}
