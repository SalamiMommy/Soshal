#[path = "common/mod.rs"]
mod test_util;

use soshal_flutter_bridge::*;
fn lock() -> std::sync::MutexGuard<'static, ()> {
    test_util::lock()
}
fn gen_keys() -> (String, String) {
    let keys = soshal_nostr_core::keys::generate_keys();
    (
        keys.secret_key().to_secret_hex(),
        keys.public_key().to_hex(),
    )
}
fn unlock(secret: &str) -> String {
    signer::signer_unlock(secret.to_string()).unwrap()
}
fn setup_db(name: &str) -> String {
    let path = soshal_test_util::tmp_path("dm", name)
        .to_string_lossy()
        .to_string();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
    db::db_init(path.clone()).unwrap();
    path
}
fn cleanup_db(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
}
fn create_profile_g(
    pk: &str,
    name: &str,
    age: i32,
    gender: &str,
    seeking: &str,
    interests: &str,
) -> String {
    let signed = dating::dating_create_profile(
        pk.to_string(),
        name.to_string(),
        age,
        "u123".to_string(),
        gender.to_string(),
        seeking.to_string(),
        170,
        "athletic".to_string(),
        "never".to_string(),
        "socially".to_string(),
        "serious".to_string(),
        "liberal".to_string(),
        "".to_string(),
        "bachelor's".to_string(),
        "[]".to_string(),
        100,
        "bio".to_string(),
        "[]".to_string(),
        interests.to_string(),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&signed).unwrap();
    v["id"].as_str().unwrap().to_string()
}

fn create_profile(pk: &str, name: &str, age: i32, interests: &str) -> String {
    create_profile_g(pk, name, age, "female", "male", interests)
}
#[test]
fn test_dating_profile_roundtrip() {
    let _g = lock();
    let path = setup_db("dating_profile_roundtrip");
    let (secret, alice) = gen_keys();
    let pk = unlock(&secret);
    assert_eq!(pk, alice);
    assert!(dating::dating_create_profile(
        alice.clone(),
        "alice".to_string(),
        150,
        "u123".to_string(),
        "female".to_string(),
        "male".to_string(),
        170,
        "athletic".to_string(),
        "never".to_string(),
        "socially".to_string(),
        "serious".to_string(),
        "liberal".to_string(),
        "".to_string(),
        "bachelor's".to_string(),
        "[]".to_string(),
        100,
        "bio".to_string(),
        "[]".to_string(),
        "[]".to_string()
    )
    .is_err());
    assert!(dating::dating_create_profile(
        alice.clone(),
        "alice".to_string(),
        30,
        "".to_string(),
        "female".to_string(),
        "male".to_string(),
        170,
        "athletic".to_string(),
        "never".to_string(),
        "socially".to_string(),
        "serious".to_string(),
        "liberal".to_string(),
        "".to_string(),
        "bachelor's".to_string(),
        "[]".to_string(),
        100,
        "hello".to_string(),
        "[]".to_string(),
        "[\"music\",\"books\"]".to_string()
    )
    .is_err());
    let signed = dating::dating_create_profile(
        alice.clone(),
        "alice".to_string(),
        30,
        "51.5007,-0.1246".to_string(),
        "female".to_string(),
        "male".to_string(),
        170,
        "athletic".to_string(),
        "never".to_string(),
        "socially".to_string(),
        "serious".to_string(),
        "liberal".to_string(),
        "".to_string(),
        "bachelor's".to_string(),
        "[]".to_string(),
        100,
        "hello".to_string(),
        "[]".to_string(),
        "[\"music\",\"books\"]".to_string(),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&signed).unwrap();
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 64);
    let card: serde_json::Value =
        serde_json::from_str(&dating::dating_get_profile(id).unwrap()).unwrap();
    assert_eq!(card["pubkey"], alice);
    assert_eq!(card["name"], "");
    assert_eq!(card["age"], 30);
    assert_eq!(card["bio"], "hello");
    assert_eq!(card["location"], "gcpuvpmm2");
    assert_eq!(card["height"], 170.0);
    assert_eq!(card["smoking"], "never");
    assert_eq!(card["interests"], serde_json::json!(["music", "books"]));
    let own: serde_json::Value =
        serde_json::from_str(&dating::dating_get_own_profile(alice.clone()).unwrap()).unwrap();
    assert_eq!(own["pubkey"], alice);
    assert_eq!(own["age"], 30);
    assert!(dating::dating_update_profile(
        alice.clone(),
        "u123".to_string(),
        "female".to_string(),
        "male".to_string(),
        170,
        "athletic".to_string(),
        "never".to_string(),
        "socially".to_string(),
        "serious".to_string(),
        "liberal".to_string(),
        "".to_string(),
        "bachelor's".to_string(),
        "[]".to_string(),
        100,
        "updated bio".to_string(),
        "[]".to_string(),
        "[]".to_string()
    )
    .unwrap());
    let updated: serde_json::Value =
        serde_json::from_str(&dating::dating_get_own_profile(alice.clone()).unwrap()).unwrap();
    assert_eq!(updated["age"], 30, "update must preserve age");
    assert!(dating::dating_delete_profile(alice.clone()).unwrap());
    assert!(dating::dating_get_own_profile(alice).is_err());
    cleanup_db(&path);
}
#[test]
fn test_dating_like_is_sign_only() {
    let _g = lock();
    let path = setup_db("dating_like_sign_only");
    let (secret, alice) = gen_keys();
    let pk = unlock(&secret);
    assert_eq!(pk, alice);
    let fake_id = "a".repeat(64);
    assert!(dating::dating_like(alice.clone(), fake_id.clone()).unwrap());
    assert!(dating::dating_superlike(alice.clone(), fake_id.clone()).unwrap());
    assert!(dating::dating_unlike(alice.clone(), fake_id).unwrap());
    assert!(dating::dating_like(alice.clone(), "short".to_string()).is_err());
    let likes: Vec<serde_json::Value> =
        serde_json::from_str(&dating::dating_fetch_likes(alice.clone()).unwrap()).unwrap();
    assert!(likes.is_empty());
    let matches: Vec<serde_json::Value> =
        serde_json::from_str(&dating::dating_fetch_matches(alice).unwrap()).unwrap();
    assert!(matches.is_empty());
    cleanup_db(&path);
}
#[test]
fn test_dating_pass_excludes_from_swipes() {
    let _g = lock();
    let path = setup_db("dating_pass_excludes");
    let (alice_secret, alice_pk) = gen_keys();
    let pk = unlock(&alice_secret);
    assert_eq!(pk, alice_pk);
    let _alice_id = create_profile(&alice_pk, "alice", 30, "[]");
    let (bob_secret, bob_pk) = gen_keys();
    unlock(&bob_secret);
    let bob_id = create_profile(&bob_pk, "bob", 25, "[]");
    unlock(&alice_secret);
    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&dating::dating_fetch_profiles(alice_pk.clone(), 10).unwrap())
            .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["pubkey"], bob_pk);
    assert!(dating::dating_pass(alice_pk.clone(), bob_id).unwrap());
    let profiles: Vec<serde_json::Value> =
        serde_json::from_str(&dating::dating_fetch_profiles(alice_pk, 10).unwrap()).unwrap();
    assert!(profiles.is_empty());
    cleanup_db(&path);
}
#[test]
fn test_dating_filter_profiles() {
    let _g = lock();
    let path = setup_db("dating_filter");
    let (alice_secret, alice_pk) = gen_keys();
    unlock(&alice_secret);
    let _ = create_profile_g(&alice_pk, "alice", 30, "female", "male", "[]");
    let (bob_secret, bob_pk) = gen_keys();
    unlock(&bob_secret);
    let _ = create_profile_g(&bob_pk, "bob", 25, "male", "female", "[]");
    let (carol_secret, carol_pk) = gen_keys();
    unlock(&carol_secret);
    let _ = create_profile_g(&carol_pk, "carol", 40, "male", "female", "[]");
    let (dave_secret, dave_pk) = gen_keys();
    unlock(&dave_secret);
    let _ = create_profile_g(&dave_pk, "dave", 30, "male", "female", "[\"music\"]");
    unlock(&alice_secret);
    let filtered: Vec<serde_json::Value> = serde_json::from_str(
        &dating::dating_filter_profiles(
            alice_pk.clone(),
            30,
            0,
            0,
            0,
            0,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().any(|c| c["pubkey"] == carol_pk));
    assert!(filtered.iter().any(|c| c["pubkey"] == dave_pk));
    let by_interest: Vec<serde_json::Value> = serde_json::from_str(
        &dating::dating_filter_profiles(
            alice_pk.clone(),
            0,
            0,
            0,
            0,
            0,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "[\"music\"]".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(by_interest.len(), 1);
    assert_eq!(by_interest[0]["pubkey"], dave_pk);
    let all: Vec<serde_json::Value> = serde_json::from_str(
        &dating::dating_filter_profiles(
            alice_pk,
            0,
            100,
            0,
            0,
            0,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(all.len(), 3);
    cleanup_db(&path);
}
#[test]
fn test_dating_block_unblock() {
    let _g = lock();
    let path = setup_db("dating_block");
    let (secret, alice) = gen_keys();
    unlock(&secret);
    let bob = "b".repeat(64);
    assert!(dating::dating_block_profile(alice.clone(), bob.clone()).unwrap());
    let rows: Vec<serde_json::Value> = serde_json::from_str(
        &db::db_query_raw(format!(
            "SELECT blocked_pubkey FROM blocks WHERE pubkey = '{}'",
            alice
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["blocked_pubkey"], bob);
    assert!(dating::dating_unblock_profile(alice.clone(), bob).unwrap());
    let rows: Vec<serde_json::Value> = serde_json::from_str(
        &db::db_query_raw(format!(
            "SELECT blocked_pubkey FROM blocks WHERE pubkey = '{}'",
            alice
        ))
        .unwrap(),
    )
    .unwrap();
    assert!(rows.is_empty());
    cleanup_db(&path);
}
#[test]
fn test_dating_score_and_stats() {
    let _g = lock();
    let path = setup_db("dating_score_stats");
    let (secret, alice) = gen_keys();
    unlock(&secret);
    let bob = "c".repeat(64);
    assert_eq!(
        dating::dating_calculate_score(alice.clone(), bob, "{}".to_string()).unwrap(),
        0.0
    );
    let _ = create_profile(&alice, "alice", 30, "[\"music\"]");
    unlock(&secret);
    let (bob_secret, bob_pk) = gen_keys();
    unlock(&bob_secret);
    let _ = create_profile(&bob_pk, "bob", 28, "[\"music\",\"art\"]");
    unlock(&secret);
    let score = dating::dating_calculate_score(alice.clone(), bob_pk, "{}".to_string()).unwrap();
    assert!(score >= 0.0);
    let stats: serde_json::Value =
        serde_json::from_str(&dating::dating_get_stats(alice).unwrap()).unwrap();
    assert_eq!(stats["profile_complete"], true);
    assert_eq!(stats["likes_received"], 0);
    assert_eq!(stats["profile_views"], 0);
    assert_eq!(stats["matches"], 0);
    cleanup_db(&path);
}
#[test]
fn test_marketplace_listing_roundtrip() {
    let _g = lock();
    let path = setup_db("marketplace_listing");
    let (secret, seller) = gen_keys();
    unlock(&secret);
    assert!(marketplace::marketplace_create_listing(
        seller.clone(),
        String::new(),
        "d".to_string(),
        10,
        "sats".to_string(),
        "books".to_string(),
        "new".to_string(),
        "[]".to_string(),
        false
    )
    .is_err());
    assert!(marketplace::marketplace_create_listing(
        seller.clone(),
        "rust book".to_string(),
        "hardcover".to_string(),
        0,
        "sats".to_string(),
        "books".to_string(),
        "new".to_string(),
        "[]".to_string(),
        false
    )
    .is_err());
    let signed = marketplace::marketplace_create_listing(
        seller.clone(),
        "rust book".to_string(),
        "hardcover".to_string(),
        5000,
        "sats".to_string(),
        "books".to_string(),
        "new".to_string(),
        "[\"https://x/i.png\"]".to_string(),
        true,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&signed).unwrap();
    let id = v["id"].as_str().unwrap().to_string();
    let info: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_listing(id.clone()).unwrap()).unwrap();
    assert_eq!(info["title"], "rust book");
    assert_eq!(info["price"], 5000);
    assert_eq!(info["category"], "books");
    assert_eq!(info["seller_pubkey"], seller);
    assert_eq!(info["shipping_available"], true);
    assert_eq!(info["escrow_enabled"], true);
    assert_eq!(info["status"], "active");
    assert!(marketplace::marketplace_get_content(id.clone())
        .unwrap()
        .contains("rust book"));
    let all: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_fetch_listings(10, 0).unwrap()).unwrap();
    assert_eq!(all.len(), 1);
    let seller_list: Vec<serde_json::Value> = serde_json::from_str(
        &marketplace::marketplace_fetch_seller_listings(seller.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(seller_list.len(), 1);
    let by_cat: Vec<serde_json::Value> = serde_json::from_str(
        &marketplace::marketplace_get_by_category("books".to_string(), 10).unwrap(),
    )
    .unwrap();
    assert_eq!(by_cat.len(), 1);
    let trending: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_get_trending(10).unwrap()).unwrap();
    assert_eq!(trending.len(), 1);
    let found: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_search("rust".to_string(), 10).unwrap())
            .unwrap();
    assert_eq!(found.len(), 1);
    let empty: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_search(" ".to_string(), 10).unwrap())
            .unwrap();
    assert!(empty.is_empty());
    assert!(marketplace::marketplace_review_listing(
        id.clone(),
        "rev1".to_string(),
        5,
        "great".to_string()
    )
    .unwrap());
    let reviews: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_listing_reviews(id.clone(), 10).unwrap())
            .unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0]["rating"], 5);
    assert_eq!(reviews[0]["reviewer"], "rev1");
    assert_eq!(reviews[0]["text"], "great");
    assert_eq!(
        marketplace::marketplace_listing_rating(id.clone()).unwrap(),
        5.0
    );
    assert!(marketplace::marketplace_update_listing(
        id.clone(),
        seller.clone(),
        "new title".to_string(),
        "d2".to_string(),
        6000
    )
    .unwrap());
    let updated: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_listing(id.clone()).unwrap()).unwrap();
    assert_eq!(updated["title"], "new title");
    assert_eq!(updated["price"], 6000);
    let found: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_search("new".to_string(), 10).unwrap())
            .unwrap();
    assert_eq!(found.len(), 1);
    assert!(marketplace::marketplace_delete_listing(id.clone(), "notseller".to_string()).is_err());
    assert!(marketplace::marketplace_delete_listing(id.clone(), seller).unwrap());
    let deleted: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_listing(id).unwrap()).unwrap();
    assert_eq!(deleted["status"], "active");
    let all: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_fetch_listings(10, 0).unwrap()).unwrap();
    assert!(all.is_empty());
    cleanup_db(&path);
}
#[test]
fn test_marketplace_poll_roundtrip() {
    let _g = lock();
    let path = setup_db("marketplace_poll");
    let (secret, user) = gen_keys();
    unlock(&secret);
    assert!(marketplace::marketplace_poll_create(
        user.clone(),
        "best lang?".to_string(),
        "[\"rust\"]".to_string(),
        24
    )
    .is_err());
    let created: serde_json::Value = serde_json::from_str(
        &marketplace::marketplace_poll_create(
            user.clone(),
            "best lang?".to_string(),
            "[\"rust\",\"dart\"]".to_string(),
            24,
        )
        .unwrap(),
    )
    .unwrap();
    let poll_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["question"], "best lang?");
    let poll: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_poll_get(poll_id.clone()).unwrap()).unwrap();
    assert_eq!(poll["id"], poll_id);
    assert_eq!(poll["question"], "best lang?");
    assert_eq!(poll["votes"], serde_json::json!([0, 0]));
    assert!(!marketplace::marketplace_poll_has_voted(poll_id.clone(), user.clone()).unwrap());
    assert!(marketplace::marketplace_poll_vote(poll_id.clone(), user.clone(), 0).unwrap());
    assert!(marketplace::marketplace_poll_has_voted(poll_id.clone(), user.clone()).unwrap());
    let poll: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_poll_get(poll_id.clone()).unwrap()).unwrap();
    assert_eq!(poll["votes"], serde_json::json!([1, 0]));
    assert!(marketplace::marketplace_poll_close(poll_id.clone(), "notowner".to_string()).is_err());
    assert!(marketplace::marketplace_poll_close(poll_id, user).unwrap());
    cleanup_db(&path);
}
#[test]
fn test_marketplace_order_escrow_lifecycle() {
    let _g = lock();
    let path = setup_db("marketplace_escrow");
    let (seller_secret, seller) = gen_keys();
    unlock(&seller_secret);
    let signed = marketplace::marketplace_create_listing(
        seller.clone(),
        "guitar".to_string(),
        "acoustic".to_string(),
        5000,
        "sats".to_string(),
        "music".to_string(),
        "used".to_string(),
        "[]".to_string(),
        false,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&signed).unwrap();
    let listing_id = v["id"].as_str().unwrap().to_string();
    let (buyer_secret, buyer) = gen_keys();
    unlock(&buyer_secret);
    let order_id =
        marketplace::marketplace_create_order(listing_id.clone(), buyer.clone(), seller.clone())
            .unwrap();
    let order: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_order(order_id.clone()).unwrap())
            .unwrap();
    assert_eq!(order["listing_id"], listing_id);
    assert_eq!(order["buyer_pubkey"], "");
    assert_eq!(order["seller_pubkey"], seller);
    assert_eq!(order["status"], "created");
    assert_eq!(order["amount"], 5000);
    let buyer_orders: Vec<serde_json::Value> =
        serde_json::from_str(&marketplace::marketplace_fetch_buyer_orders(buyer.clone()).unwrap())
            .unwrap();
    assert_eq!(buyer_orders.len(), 1);
    let seller_orders: Vec<serde_json::Value> = serde_json::from_str(
        &marketplace::marketplace_fetch_seller_orders(seller.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(seller_orders.len(), 1);
    assert!(
        marketplace::marketplace_create_escrow(order_id.clone(), buyer, seller.clone(), 5000)
            .is_err()
    );
    assert!(marketplace::marketplace_create_escrow(
        order_id.clone(),
        "".to_string(),
        seller.clone(),
        0
    )
    .is_err());
    let escrow_id = marketplace::marketplace_create_escrow(
        order_id.clone(),
        "".to_string(),
        seller.clone(),
        5000,
    )
    .unwrap();
    let escrow: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_escrow(escrow_id.clone()).unwrap())
            .unwrap();
    let escrow = escrow[0].clone();
    assert_eq!(escrow["status"], "created");
    let by_listing: serde_json::Value = serde_json::from_str(
        &marketplace::marketplace_get_escrow_by_listing(listing_id.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(by_listing["id"], escrow_id);
    assert!(marketplace::marketplace_delete_listing(listing_id, seller.clone()).is_err());
    assert!(marketplace::marketplace_release_escrow(escrow_id.clone(), seller.clone()).is_err());
    unlock(&seller_secret);
    assert!(marketplace::marketplace_resolve_escrow(
        escrow_id.clone(),
        "mediator".to_string(),
        seller.clone()
    )
    .unwrap());
    let escrow: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_escrow(escrow_id).unwrap()).unwrap();
    assert_eq!(escrow[0]["status"], "refunded");
    let escrow2 = marketplace::marketplace_create_escrow(
        order_id.clone(),
        "".to_string(),
        seller.clone(),
        5000,
    )
    .unwrap();
    assert!(marketplace::marketplace_dispute_escrow(
        escrow2.clone(),
        "stranger".to_string(),
        "bad".to_string()
    )
    .is_err());
    assert!(marketplace::marketplace_dispute_escrow(
        escrow2.clone(),
        seller.clone(),
        "bad item".to_string()
    )
    .unwrap());
    let escrow: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_escrow(escrow2.clone()).unwrap())
            .unwrap();
    assert_eq!(escrow[0]["status"], "disputed");
    assert!(marketplace::marketplace_release_escrow(escrow2, seller.clone()).is_err());
    let escrow3 =
        marketplace::marketplace_create_escrow(order_id, "".to_string(), seller.clone(), 5000)
            .unwrap();
    assert!(marketplace::marketplace_release_escrow(escrow3.clone(), seller.clone()).is_err());
    db::db_execute_raw(format!(
        "UPDATE escrows SET buyer_confirmed=1, seller_confirmed=1 WHERE id='{escrow3}'"
    ))
    .unwrap();
    unlock(&seller_secret);
    assert!(marketplace::marketplace_release_escrow(escrow3.clone(), seller).unwrap());
    let escrow: serde_json::Value =
        serde_json::from_str(&marketplace::marketplace_get_escrow(escrow3).unwrap()).unwrap();
    assert_eq!(escrow[0]["status"], "completed");
    assert!(marketplace::marketplace_get_escrow("nonexistent".to_string()).is_err());
    cleanup_db(&path);
}
