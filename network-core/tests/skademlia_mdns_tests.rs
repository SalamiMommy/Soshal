//! Gap-fill coverage for network-core S/Kademlia DHT and mDNS helpers
//! (pure logic; no sockets).

use soshal_network_core::mdns::{instance_name, parse_instance_pubkey};
use soshal_network_core::skademlia::{
    check_leading_zeros, generate_node_id, xor_distance, NodeId, SkademliaPeer,
    SkademliaRoutingTable, POW_DYNAMIC_DIFFICULTY_BITS,
};

fn peer(id: u8, pubkey: &str, rep: f64) -> SkademliaPeer {
    SkademliaPeer {
        node_id: [id; 32],
        pubkey: pubkey.to_string(),
        address: format!("10.0.0.{id}:8000"),
        reputation_score: rep,
    }
}

#[test]
fn node_id_pow_generation_and_rejection() {
    let mut found = None;
    'outer: for static_nonce in 0..4_096u64 {
        for dynamic_nonce in 0..512u64 {
            if let Some(id) = generate_node_id("pk-a", static_nonce, dynamic_nonce) {
                found = Some((static_nonce, dynamic_nonce, id));
                break 'outer;
            }
        }
    }
    let (static_nonce, dynamic_nonce, id) = found.expect("a nonce pair must satisfy pow");
    assert!(check_leading_zeros(&id, POW_DYNAMIC_DIFFICULTY_BITS));
    assert_eq!(
        generate_node_id("pk-a", static_nonce, dynamic_nonce),
        Some(id),
        "deterministic"
    );
    assert_eq!(
        generate_node_id("pk-a", static_nonce, dynamic_nonce.wrapping_add(1)),
        None,
        "a different dynamic nonce fails the dynamic pow"
    );
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
    assert!(!table.add_peer(peer(0, "self", 1.0)));
    assert!(table.add_peer(peer(2, "p2", 0.1)), "bucket 6 first slot");
    assert!(table.add_peer(peer(3, "p3", 0.5)), "displaces 0.1 rep peer");
    let p3 = table
        .k_buckets
        .values()
        .flatten()
        .find(|p| p.node_id == [3u8; 32])
        .unwrap();
    assert_eq!(p3.pubkey, "p3");
    assert!(!table.add_peer(peer(2, "p2-low", 0.2)), "below current rep");
    assert!(table.add_peer(peer(3, "p3-high", 0.9)), "update existing");
    let p3 = table
        .k_buckets
        .values()
        .flatten()
        .find(|p| p.node_id == [3u8; 32])
        .unwrap();
    assert_eq!(p3.pubkey, "p3-high");
}

#[test]
fn find_closest_orders_by_distance() {
    let self_id: NodeId = [0u8; 32];
    let mut table = SkademliaRoutingTable::new(self_id, 16);
    for i in 1..=8u8 {
        assert!(table.add_peer(peer(i, &format!("pk-{i}"), 1.0)));
    }
    assert!(table.find_closest(&self_id, 0).is_empty());
    let target: NodeId = [0x01; 32];
    let closest = table.find_closest(&target, 3);
    assert_eq!(closest.len(), 3);
    let dists: Vec<usize> = closest
        .iter()
        .map(|p| {
            xor_distance(&p.node_id, &target)
                .iter()
                .fold(0usize, |acc, b| acc + *b as usize)
        })
        .collect();
    let mut sorted = dists.clone();
    sorted.sort();
    assert_eq!(dists, sorted, "ascending distance");
    let _ = dists;
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
