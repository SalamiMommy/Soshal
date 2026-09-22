//! S/Kademlia DHT implementation with Cryptographic Proof-of-Work Node ID generation
//! and disjoint path lookup routing.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// S/Kademlia Node ID (256-bit hash value).
pub type NodeId = [u8; 32];

/// Cryptographic Proof-of-Work verification parameters.
///
/// Static (node-id binding) difficulty follows S/Kademlia guidance (≥21
/// bits); the dynamic component gates the final node id at 8 bits. The
/// previous 8/4-bit settings let a cheap sybil mintage fill the routing
/// table with adversarial ids — raised 2026-09.
pub const POW_STATIC_DIFFICULTY_BITS: u32 = 21;
pub const POW_DYNAMIC_DIFFICULTY_BITS: u32 = 8;

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
    if bits > 256 {
        return false;
    }
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

/// Searches for a `(static_nonce, dynamic_nonce)` pair that mints a valid
/// node id for `pubkey` within the given budgets, returning the nonces and
/// the minted id. Used for DHT node-id bootstrap and PoW tests; `None` when
/// the budgets are exhausted. Expected search cost is ~2^STATIC hashes plus
/// 2^DYNAMIC for the first static match.
pub fn find_pow_nonces(
    pubkey: &str,
    static_budget: u64,
    dynamic_budget: u64,
) -> Option<(u64, u64, NodeId)> {
    find_pow_nonces_where(pubkey, static_budget, dynamic_budget, |_| true)
}

/// [`find_pow_nonces`] variant that only accepts a minted id satisfying
/// `predicate` (e.g. targeting a specific routing bucket).
pub fn find_pow_nonces_where(
    pubkey: &str,
    static_budget: u64,
    dynamic_budget: u64,
    predicate: impl Fn(&NodeId) -> bool,
) -> Option<(u64, u64, NodeId)> {
    for static_nonce in 0..static_budget {
        let mut h = Sha256::new();
        h.update(pubkey.as_bytes());
        h.update(static_nonce.to_be_bytes());
        let static_hash: [u8; 32] = h.finalize().into();
        if !check_leading_zeros(&static_hash, POW_STATIC_DIFFICULTY_BITS) {
            continue;
        }
        for dynamic_nonce in 0..dynamic_budget {
            let mut h2 = Sha256::new();
            h2.update(static_hash);
            h2.update(dynamic_nonce.to_be_bytes());
            let node_id: [u8; 32] = h2.finalize().into();
            if check_leading_zeros(&node_id, POW_DYNAMIC_DIFFICULTY_BITS) && predicate(&node_id) {
                return Some((static_nonce, dynamic_nonce, node_id));
            }
        }
    }
    None
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
///
/// A record is only trusted once its `node_id` verifies as the PoW binding
/// of `pubkey` under `static_nonce`/`dynamic_nonce` (enforced by
/// [`SkademliaRoutingTable::add_peer`]); fabricated ids minted against
/// another peer's pubkey are refused.
#[derive(Debug, Clone)]
pub struct SkademliaPeer {
    pub node_id: NodeId,
    pub pubkey: String,
    pub address: String,
    pub reputation_score: f64,
    /// Nonce used for the static PoW binding (`H(pubkey ‖ static_nonce)`).
    pub static_nonce: u64,
    /// Nonce used for the dynamic PoW binding (`H(static_hash ‖ dynamic_nonce)`).
    pub dynamic_nonce: u64,
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
    ///
    /// Refuses self and any record whose `node_id` is not the PoW binding
    /// of `peer.pubkey` under the peer's claimed nonces — a forged
    /// (sybil / address-poisoning) insert can otherwise trivially mint
    /// ids *closer* to the target than honest peers.
    pub fn add_peer(&mut self, peer: SkademliaPeer) -> bool {
        if peer.node_id == self.self_node_id {
            return false;
        }
        // L2: verify PoW binding before routing. generate_node_id re-checks
        // both difficulty levels and returns the id only on success.
        if generate_node_id(&peer.pubkey, peer.static_nonce, peer.dynamic_nonce)
            != Some(peer.node_id)
        {
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
                .min_by(|(_, a), (_, b)| a.reputation_score.total_cmp(&b.reputation_score))
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

    /// Mints a PoW-valid peer for `pubkey` (first nonce pair found).
    fn minted_peer(pubkey: &str, rep: f64) -> SkademliaPeer {
        let (static_nonce, dynamic_nonce, node_id) =
            find_pow_nonces(pubkey, 1 << 22, 1 << 10).expect("nonce pair found in budget");
        SkademliaPeer {
            node_id,
            pubkey: pubkey.to_string(),
            address: "10.0.0.1:8000".to_string(),
            reputation_score: rep,
            static_nonce,
            dynamic_nonce,
        }
    }

    #[test]
    fn test_skademlia_routing_table() {
        let self_id = [1u8; 32];
        let mut table = SkademliaRoutingTable::new(self_id, 4);

        let peer = minted_peer("pk_test", 0.9);

        assert!(table.add_peer(peer.clone()));
        let closest = table.find_closest(&peer.node_id, 10);
        assert_eq!(closest.len(), 1);
        assert_eq!(closest[0].pubkey, "pk_test");
    }

    #[test]
    fn test_add_peer_refuses_forged_node_id() {
        let mut table = SkademliaRoutingTable::new([1u8; 32], 4);
        // A fabricated id that is NOT the PoW binding of "eve" — must be
        // refused even though its bucket looks valid. Nonces chosen so
        // generate_node_id("eve", 0, 0) can never mint [2u8; 32].
        let forged = SkademliaPeer {
            node_id: [2u8; 32],
            pubkey: "eve".to_string(),
            address: "10.0.0.9:8000".to_string(),
            reputation_score: 1.0,
            static_nonce: 0,
            dynamic_nonce: 0,
        };
        assert!(!table.add_peer(forged), "unverified node_id rejected");
        assert!(table.k_buckets.values().all(|b| b.is_empty()));
    }
}
