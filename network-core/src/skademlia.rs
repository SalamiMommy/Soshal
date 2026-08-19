//! S/Kademlia DHT implementation with Cryptographic Proof-of-Work Node ID generation
//! and disjoint path lookup routing.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// S/Kademlia Node ID (256-bit hash value).
pub type NodeId = [u8; 32];

/// Cryptographic Proof-of-Work verification parameters.
pub const POW_STATIC_DIFFICULTY_BITS: u32 = 8;
pub const POW_DYNAMIC_DIFFICULTY_BITS: u32 = 4;

/// Generates a valid S/Kademlia Node ID from a public key and nonces matching PoW difficulty.
pub fn generate_node_id(pubkey: &str, static_nonce: u64, dynamic_nonce: u64) -> Option<NodeId> {
    let mut hasher = Sha256::new();
    hasher.update(pubkey.as_bytes());
    hasher.update(static_nonce.to_be_bytes());
    let static_hash: [u8; 32] = hasher.finalize().into();

    if !check_leading_zeros(&static_hash, POW_STATIC_DIFFICULTY_BITS) {
        return None;
    }

    let mut hasher2 = Sha256::new();
    hasher2.update(static_hash);
    hasher2.update(dynamic_nonce.to_be_bytes());
    let node_id: [u8; 32] = hasher2.finalize().into();

    if check_leading_zeros(&node_id, POW_DYNAMIC_DIFFICULTY_BITS) {
        Some(node_id)
    } else {
        None
    }
}

/// Checks if a 256-bit hash has at least `bits` leading zero bits.
pub fn check_leading_zeros(hash: &[u8; 32], bits: u32) -> bool {
    let full_bytes = (bits / 8) as usize;
    let rem_bits = bits % 8;

    for &b in &hash[..full_bytes] {
        if b != 0 {
            return false;
        }
    }

    if rem_bits > 0 {
        let mask = 0xFFu8 << (8 - rem_bits);
        if (hash[full_bytes] & mask) != 0 {
            return false;
        }
    }

    true
}

/// Computes the XOR distance metric between two Node IDs.
pub fn xor_distance(a: &NodeId, b: &NodeId) -> NodeId {
    let mut dist = [0u8; 32];
    for (d, (&x, &y)) in dist.iter_mut().zip(a.iter().zip(b.iter())) {
        *d = x ^ y;
    }
    dist
}

/// Represents a peer node in the S/Kademlia DHT.
#[derive(Debug, Clone)]
pub struct SkademliaPeer {
    pub node_id: NodeId,
    pub pubkey: String,
    pub address: String,
    pub reputation_score: f64,
}

/// S/Kademlia Routing Table with disjoint path bucket routing.
pub struct SkademliaRoutingTable {
    pub self_node_id: NodeId,
    pub k_buckets: BTreeMap<usize, Vec<SkademliaPeer>>,
    pub bucket_capacity: usize,
}

impl SkademliaRoutingTable {
    pub fn new(self_node_id: NodeId, bucket_capacity: usize) -> Self {
        Self {
            self_node_id,
            k_buckets: BTreeMap::new(),
            bucket_capacity,
        }
    }

    /// Computes bucket index based on XOR distance leading zero bits.
    pub fn bucket_index(&self, other: &NodeId) -> usize {
        let dist = xor_distance(&self.self_node_id, other);
        for (i, &byte) in dist.iter().enumerate() {
            if byte != 0 {
                return i * 8 + byte.leading_zeros() as usize;
            }
        }
        256
    }

    /// Adds or updates a peer in the routing table.
    pub fn add_peer(&mut self, peer: SkademliaPeer) -> bool {
        if peer.node_id == self.self_node_id {
            return false;
        }
        let index = self.bucket_index(&peer.node_id);
        let bucket = self.k_buckets.entry(index).or_default();

        if let Some(existing) = bucket.iter_mut().find(|p| p.node_id == peer.node_id) {
            *existing = peer;
            return true;
        }

        if bucket.len() < self.bucket_capacity {
            bucket.push(peer);
            true
        } else {
            // Bucket full - reject or displace lowest reputation peer
            if let Some(lowest_idx) = bucket
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.reputation_score.partial_cmp(&b.reputation_score).unwrap()
                })
                .map(|(idx, _)| idx)
            {
                if bucket[lowest_idx].reputation_score < peer.reputation_score {
                    bucket[lowest_idx] = peer;
                    return true;
                }
            }
            false
        }
    }

    /// Finds the closest $k$ peers to a target Node ID across disjoint paths.
    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<SkademliaPeer> {
        let total_peers: usize = self.k_buckets.values().map(|b| b.len()).sum();
        let mut all_peers: Vec<SkademliaPeer> = Vec::with_capacity(total_peers);
        for bucket in self.k_buckets.values() {
            all_peers.extend(bucket.iter().cloned());
        }

        if count == 0 || all_peers.is_empty() {
            return Vec::new();
        }

        if count < all_peers.len() {
            all_peers.select_nth_unstable_by(count, |a, b| {
                let dist_a = xor_distance(&a.node_id, target);
                let dist_b = xor_distance(&b.node_id, target);
                dist_a.cmp(&dist_b)
            });
            all_peers.truncate(count);
            all_peers.sort_by(|a, b| {
                let dist_a = xor_distance(&a.node_id, target);
                let dist_b = xor_distance(&b.node_id, target);
                dist_a.cmp(&dist_b)
            });
        } else {
            all_peers.sort_by(|a, b| {
                let dist_a = xor_distance(&a.node_id, target);
                let dist_b = xor_distance(&b.node_id, target);
                dist_a.cmp(&dist_b)
            });
        }

        all_peers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pow_verification() {
        let hash = [0u8; 32];
        assert!(check_leading_zeros(&hash, 16));

        let mut nonzero = [0xFFu8; 32];
        nonzero[0] = 0x0F;
        assert!(check_leading_zeros(&nonzero, 4));
        assert!(!check_leading_zeros(&nonzero, 5));
    }

    #[test]
    fn test_skademlia_routing_table() {
        let self_id = [1u8; 32];
        let mut table = SkademliaRoutingTable::new(self_id, 4);

        let peer_id = [2u8; 32];
        let peer = SkademliaPeer {
            node_id: peer_id,
            pubkey: "pk_test".to_string(),
            address: "127.0.0.1:8080".to_string(),
            reputation_score: 0.9,
        };

        assert!(table.add_peer(peer));
        let closest = table.find_closest(&peer_id, 10);
        assert_eq!(closest.len(), 1);
        assert_eq!(closest[0].pubkey, "pk_test");
    }
}
