use soshal_identity_core::signers::clear_shared_secret_cache;
use soshal_identity_core::wot::{get_wot_peers_by_distance, invalidate_wot_peers_cache, WotUser};

#[test]
fn clear_shared_secret_cache_idempotent() {
    clear_shared_secret_cache();
    clear_shared_secret_cache();
}

#[test]
fn wot_cache_invalidation_serves_fresh_partitions() {
    let self_pk = "self_pk_000";
    let mut contacts: Vec<String> = (0..65).map(|i| format!("user_{i:02}")).collect();
    let mut users: Vec<WotUser> = vec![WotUser {
        pubkey: self_pk.to_string(),
        contacts: contacts.clone(),
    }];
    for contact in &contacts {
        users.push(WotUser {
            pubkey: contact.clone(),
            contacts: vec![self_pk.to_string()],
        });
    }
    let before = get_wot_peers_by_distance(self_pk, &users, 2);
    assert_eq!(before[&1].len(), 65, "all 65 users are distance 1");
    assert!(!before.contains_key(&2));

    invalidate_wot_peers_cache();
    let after = get_wot_peers_by_distance(self_pk, &users, 2);
    assert_eq!(
        after[&1].len(),
        65,
        "invalidate then recompute stays correct"
    );

    contacts.push("user_65".into());
    let mut grown_users = vec![WotUser {
        pubkey: self_pk.to_string(),
        contacts: contacts.clone(),
    }];
    for pk in contacts {
        grown_users.push(WotUser {
            pubkey: pk,
            contacts: vec![self_pk.to_string()],
        });
    }
    invalidate_wot_peers_cache();
    let grown = get_wot_peers_by_distance(self_pk, &grown_users, 2);
    assert_eq!(
        grown[&1].len(),
        66,
        "post-invalidate lookup reflects the grown graph"
    );

    invalidate_wot_peers_cache();
    invalidate_wot_peers_cache();
}
