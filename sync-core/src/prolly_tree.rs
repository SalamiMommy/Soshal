//! Prolly Tree (Probabilistic Merkle B-Tree / Search Tree) implementation.
//! Provides deterministic structural Merkle hashing over database key-value pairs for $O(1)$ root state comparison
//! and $O(\log N)$ state reconciliation.

use serde::{Deserialize, Serialize};

/// Target average chunk size in bytes (e.g., 4KB boundary target).
pub const TARGET_CHUNK_SIZE: u32 = 4096;
/// Modulo mask for probabilistic boundary condition (4KB -> 4095).
pub const GEAR_MASK: u32 = 0x0FFF;

/// Node in the Prolly Tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProllyNode {
    pub level: u32,
    pub node_hash: String,
    pub keys: Vec<String>,
    pub values_or_child_hashes: Vec<String>,
}

/// Prolly Tree manager for building and reconciling Merkle-CRDT indexes.
#[derive(Debug, Default, Clone)]
pub struct ProllyTree {
    pub root_hash: String,
    pub nodes: Vec<ProllyNode>,
}

impl ProllyTree {
    /// Computes Gear rolling hash on key-value byte sequence.
    pub fn gear_hash(data: &[u8]) -> u32 {
        let mut hash: u32 = 0;
        for &byte in data {
            hash = (hash << 1).wrapping_add(byte as u32);
        }
        hash
    }

    /// Determines if a key-value boundary occurs at `data`.
    pub fn is_boundary(data: &[u8]) -> bool {
        (Self::gear_hash(data) & GEAR_MASK) == 0
    }

    /// Computes deterministic Merkle hash of a node.
    pub fn compute_node_hash(level: u32, keys: &[String], children: &[String]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(level.to_be_bytes());
        for k in keys {
            hasher.update(k.as_bytes());
        }
        for c in children {
            hasher.update(c.as_bytes());
        }
        hex::encode(hasher.finalize())
    }

    /// Builds a Prolly Tree from sorted (key, value) pairs.
    pub fn build(kv_pairs: &[(String, String)]) -> Self {
        if kv_pairs.is_empty() {
            let empty_hash = Self::compute_node_hash(0, &[], &[]);
            return Self {
                root_hash: empty_hash.clone(),
                nodes: vec![ProllyNode {
                    level: 0,
                    node_hash: empty_hash,
                    keys: vec![],
                    values_or_child_hashes: vec![],
                }],
            };
        }

        let mut current_keys: Vec<String> = kv_pairs.iter().map(|(k, _)| k.clone()).collect();
        let mut current_vals: Vec<String> = kv_pairs.iter().map(|(_, v)| v.clone()).collect();
        let mut all_nodes = Vec::new();
        let mut level = 0;
        let mut kv_bytes = String::new();

        loop {
            let mut level_nodes = Vec::new();
            let mut chunk_keys = Vec::new();
            let mut chunk_vals = Vec::new();

            for (k, v) in current_keys.into_iter().zip(current_vals) {
                kv_bytes.clear();
                kv_bytes.push_str(&k);
                kv_bytes.push(':');
                kv_bytes.push_str(&v);
                let boundary = Self::is_boundary(kv_bytes.as_bytes());

                chunk_keys.push(k);
                chunk_vals.push(v);

                // Emit when a content-derived gear-gear boundary lands (the
                // first key alone can start a new chunk) or the hard cap is
                // hit. Honouring a boundary on the very first key keeps node
                // splits purely content-derived, which is what structural
                // O(log N) reconciliation relies on.
                if boundary || chunk_keys.len() >= 100 {
                    let nhash = Self::compute_node_hash(level, &chunk_keys, &chunk_vals);
                    level_nodes.push(ProllyNode {
                        level,
                        node_hash: nhash,
                        keys: std::mem::take(&mut chunk_keys),
                        values_or_child_hashes: std::mem::take(&mut chunk_vals),
                    });
                }
            }

            if !chunk_keys.is_empty() {
                let nhash = Self::compute_node_hash(level, &chunk_keys, &chunk_vals);
                level_nodes.push(ProllyNode {
                    level,
                    node_hash: nhash,
                    keys: chunk_keys,
                    values_or_child_hashes: chunk_vals,
                });
            }

            if level_nodes.len() == 1 {
                // Root reached!
                let root_hash = level_nodes[0].node_hash.clone();
                all_nodes.extend(std::mem::take(&mut level_nodes));
                return Self {
                    root_hash,
                    nodes: all_nodes,
                };
            }

            // Prepare next level up
            current_keys = level_nodes
                .iter()
                .map(|n| n.keys.first().unwrap_or(&n.node_hash).clone())
                .collect();
            current_vals = level_nodes.iter().map(|n| n.node_hash.clone()).collect();
            all_nodes.extend(std::mem::take(&mut level_nodes));
            level += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prolly_tree_deterministic_root() {
        let kv1 = vec![
            ("post_1".to_string(), "val1".to_string()),
            ("post_2".to_string(), "val2".to_string()),
            ("post_3".to_string(), "val3".to_string()),
        ];
        let kv2 = kv1.clone();

        let tree1 = ProllyTree::build(&kv1);
        let tree2 = ProllyTree::build(&kv2);

        assert_eq!(tree1.root_hash, tree2.root_hash);
        assert!(!tree1.root_hash.is_empty());
    }
}
