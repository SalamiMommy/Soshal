//! Gap-fill coverage for network-core S/Kademlia DHT and mDNS helpers
//! (pure logic; no sockets).

use soshal_network_core::mdns::{instance_name, parse_instance_pubkey};
use soshal_network_core::skademlia::{
    check_leading_zeros, find_pow_nonces, find_pow_nonces_where, generate_node_id, xor_distance,
    NodeId, SkademliaPeer, SkademliaRoutingTable, POW_DYNAMIC_DIFFICULTY_BITS,
};

/// Mints a PoW-valid peer for `pubkey` (first nonce pair found; the search
/// is deterministic so a given pubkey always maps to the same id).
fn minted(pubkey: &str, rep: f64) -> SkademliaPeer {
    let (static_nonce, dynamic_nonce, node_id) =
        find_pow_nonces(pubkey, 1 << 26, 1 << 12).expect("id minted in budget");
    SkademliaPeer {
        node_id,
        pubkey: pubkey.to_string(),
        address: "10.0.0.1:8000".to_string(),
        reputation_score: rep,
        static_nonce,
        dynamic_nonce,
    }
}

/// Mints a PoW-valid peer whose node id lands in bucket 8 (byte 0 is forced
/// to 0x00 by the 8-bit dynamic PoW, so bucket 0 is unreachable; byte 1 MSB
/// set → deepest reachable prefix) for deterministic same-bucket collision
/// tests.
fn minted_bucket0(pubkey: &str, rep: f64) -> SkademliaPeer {
    let (static_nonce, dynamic_nonce, node_id) =
        find_pow_nonces_where(pubkey, 1 << 26, 1 << 12, |id| id[1] & 0x80 != 0)
            .expect("bucket-8 id minted in budget");
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
fn node_id_pow_generation_and_rejection() {
    let (static_nonce, dynamic_nonce, id) =
        find_pow_nonces("pk-a", 1 << 26, 1 << 12).expect("a nonce pair must satisfy pow");
    assert!(check_leading_zeros(&id, POW_DYNAMIC_DIFFICULTY_BITS));
    assert_eq!(
        generate_node_id("pk-a", static_nonce, dynamic_nonce),
        Some(id),
        "deterministic"
    );
    // A *different* dynamic nonce for the same pubkey fails the dynamic pow.
    // Scan for one that fails (expected within a handful of tries) so the
    // assertion is deterministic rather than a 1/2^DYNAMIC coin flip.
    let mut saw_rejection = false;
    for k in 1u64..=1 << 12 {
        if generate_node_id("pk-a", static_nonce, dynamic_nonce.wrapping_add(k)).is_none() {
            saw_rejection = true;
            break;
        }
    }
    assert!(saw_rejection, "some other dynamic nonce must fail");
    assert!(generate_node_id("", 0, 0).is_none());
}

#[test]
fn leading_zeros_edge_cases() {
    let zero = [0u8; 32];
    assert!(check_leading_zeros(&zero, 0));
    assert!(check_leading_zeros(&zero, 256));
    let mut h = [0u8; 32];
    h[0] = 0x80;
    assert!(check_leading_zeros(&h, 0));
    assert!(!check_leading_zeros(&h, 1));
    h[0] = 0x0F;
    assert!(check_leading_zeros(&h, 4));
    assert!(!check_leading_zeros(&h, 5));
    h[0] = 0x00;
    h[1] = 0x80;
    assert!(check_leading_zeros(&h, 8));
    assert!(!check_leading_zeros(&h, 9));
    h[1] = 0x01;
    assert!(check_leading_zeros(&h, 9), "bit 9 clear");
    assert!(!check_leading_zeros(&h, 16), "bit 16 set");
}

#[test]
fn xor_distance_is_symmetric_and_unique() {
    let a: NodeId = [0xAA; 32];
    let b: NodeId = [0x55; 32];
    let d1 = xor_distance(&a, &b);
    let d2 = xor_distance(&b, &a);
    assert_eq!(d1, d2);
    assert_eq!(xor_distance(&a, &a), [0u8; 32]);
    assert_ne!(d1, [0u8; 32]);
    let c: NodeId = [0x00; 32];
    let d3 = xor_distance(&a, &c);
    assert_eq!(d3, [0xAA; 32]);
}

#[test]
fn bucket_index_maps_distance() {
    let table = SkademliaRoutingTable::new([0u8; 32], 4);
    assert_eq!(
        table.bucket_index(&[0u8; 32]),
        256,
        "self -> deepest bucket"
    );
    let mut other = [0u8; 32];
    other[0] = 0x01;
    assert_eq!(table.bucket_index(&other), 7);
    let mut other2 = [0u8; 32];
    other2[1] = 0x80;
    assert_eq!(table.bucket_index(&other2), 8);
}

#[test]
fn add_peer_rejects_self_and_displaces_low_reputation() {
    let mut table = SkademliaRoutingTable::new([0u8; 32], 1);
    // Self-id record is refused by the self guard (before PoW).
    let self_peer = SkademliaPeer {
        node_id: [0u8; 32],
        pubkey: "self".to_string(),
        address: "10.0.0.1:8000".to_string(),
        reputation_score: 1.0,
        static_nonce: 0,
        dynamic_nonce: 0,
    };
    assert!(!table.add_peer(self_peer));
    // Two distinct pubkeys minted into the SAME bucket (8) so capacity-1
    // displacement is deterministic.
    let p2 = minted_bucket0("p2", 0.1);
    let p3 = minted_bucket0("p3", 0.5);
    assert_eq!(table.bucket_index(&p2.node_id), 8);
    assert_eq!(table.bucket_index(&p3.node_id), 8);
    assert!(table.add_peer(p2.clone()), "bucket 8 first slot");
    assert!(table.add_peer(p3.clone()), "displaces 0.1 rep peer");
    let present: Vec<NodeId> = table
        .k_buckets
        .values()
        .flatten()
        .map(|p| p.node_id)
        .collect();
    assert!(present.contains(&p3.node_id), "p3 remains");
    assert!(!present.contains(&p2.node_id), "p2 displaced");

    assert!(
        !table.add_peer(minted_bucket0("p2-low", 0.2)),
        "below current rep"
    );
    // Update existing: same node id (same pubkey + nonces, so the PoW
    // binding still verifies), higher reputation.
    let p3_high = SkademliaPeer {
        node_id: p3.node_id,
        pubkey: "p3".to_string(),
        address: p3.address.clone(),
        reputation_score: 0.9,
        static_nonce: p3.static_nonce,
        dynamic_nonce: p3.dynamic_nonce,
    };
    assert!(table.add_peer(p3_high), "update existing");
    let p3_stored = table
        .k_buckets
        .values()
        .flatten()
        .find(|p| p.node_id == p3.node_id)
        .unwrap();
    assert_eq!(p3_stored.reputation_score, 0.9);
}

#[test]
fn find_closest_orders_by_distance() {
    let self_id: NodeId = [0u8; 32];
    let mut table = SkademliaRoutingTable::new(self_id, 16);
    let peers: Vec<SkademliaPeer> = (1..=8u8).map(|i| minted(&format!("pk-{i}"), 1.0)).collect();
    for p in &peers {
        assert!(table.add_peer(p.clone()), "PoW-valid peer accepted");
    }
    assert!(table.find_closest(&self_id, 0).is_empty());
    let target: NodeId = [0x01; 32];
    let closest = table.find_closest(&target, 3);
    assert_eq!(closest.len(), 3);
    // Descending XOR distance (NodeId lexicographic over the XOR bytes) —
    // byte-sum is not monotone with XOR for arbitrary ids.
    let closest_ids: Vec<NodeId> = closest.iter().map(|p| p.node_id).collect();
    let mut sorted_ids = closest_ids.clone();
    sorted_ids.sort_by_key(|a| xor_distance(a, &target));
    assert_eq!(closest_ids, sorted_ids, "ascending xor distance");
    assert_eq!(table.find_closest(&target, 100).len(), 8);
}

#[test]
fn mdns_instance_names_roundtrip() {
    let pk = "a".repeat(64);
    let name = instance_name(&pk);
    assert_eq!(name, format!("soshal-{pk}"));
    assert_eq!(parse_instance_pubkey(&name).unwrap(), pk);
    assert_eq!(parse_instance_pubkey("soshal-short"), None);
    assert_eq!(parse_instance_pubkey("other-xxxx"), None);
    assert_eq!(
        parse_instance_pubkey(&format!("soshal-{}", "z".repeat(64))),
        None
    );
    assert_eq!(
        parse_instance_pubkey(&format!("soshal-{}", "a".repeat(63))),
        None
    );
    assert_eq!(parse_instance_pubkey(""), None);
}
