//! FFI identity surface tests: profile store/query, WoT scoring, block
//! checks, signer-gated publish builders. Publish steps need a live relay
//! client, so success paths are asserted as far as `relay client not
//! initialized`.

#[cfg(test)]
mod identity_gap_tests {
    use soshal_flutter_bridge::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn init_db(name: &str) -> String {
        let path = format!(
            "{}/soshal_identity_{}_{name}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db::db_init(path.clone()).is_ok());
        path
    }

    fn unlock_signer() -> String {
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        pk
    }

    fn kind0_json(pubkey: &str, name: &str) -> String {
        format!(
            r#"{{"pubkey":"{pubkey}","content":"{{\"name\":\"{name}\",\"display_name\":\"{name} d\",\"about\":\"bio\",\"picture\":\"https://example.com/p.png\",\"banner\":\"https://example.com/b.png\",\"nip05\":\"{name}@example.com\"}}","created_at":1700000000}}"#
        )
    }

    #[test]
    fn profile_store_query_search_and_self_alias() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = init_db("profile");
        let pk = "a".repeat(64);

        let empty = identity::identity_get_profile(pk.clone()).unwrap();
        assert!(empty.contains("\"name\":\"\""));
        assert!(empty.contains("\"wot_status\":\"unknown\""));

        assert!(identity::identity_store_profile(kind0_json(&pk, "alice")).unwrap());
        let stored = identity::identity_get_profile(pk.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(v["name"], "alice");
        assert_eq!(v["display_name"], "alice d");
        assert_eq!(v["picture"], "https://example.com/p.png");
        assert_eq!(v["nip05"], "alice@example.com");
        assert_eq!(v["nip05_valid"], false);

        let self_profile = identity::identity_get_self_profile(pk.clone()).unwrap();
        assert!(self_profile.contains("\"name\":\"alice\""));

        let hits = identity::identity_search_users("ali".into(), 10).unwrap();
        assert!(hits.contains("\"name\":\"alice\""), "{hits}");

        let follows = identity::identity_fetch_follows(pk.clone()).unwrap();
        assert_eq!(follows, "[]");
        let _ = identity::identity_fetch_follows("b".repeat(64)).unwrap();

        let bad = identity::identity_store_profile(r#"{"content":"{}"}"#.into()).unwrap_err();
        assert!(bad.contains("missing pubkey"), "{bad}");
        let bad = identity::identity_store_profile("not json".into()).unwrap_err();
        assert!(bad.contains("invalid profile JSON"), "{bad}");
        let _ = db;
    }

    #[test]
    fn trust_score_and_wot_status_from_graph() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = init_db("wot");
        let me = "a".repeat(64);
        let friend = "b".repeat(64);
        let stranger = "c".repeat(64);

        let insert = |pk: &str, contacts: &str| {
            let sql = format!(
                "INSERT INTO users (pubkey, npub, contact_pubkeys, relay_list) VALUES ('{pk}', '', '{contacts}', '[]')"
            );
            db::db_execute_raw(sql).unwrap();
        };
        insert(&me, &format!("[\"{friend}\"]"));
        insert(&friend, &format!("[\"{me}\"]"));
        insert(&stranger, "[]");

        let score = identity::identity_get_trust_score(me.clone(), friend.clone()).unwrap();
        assert!(score > 0.5, "mutual follow should score high: {score}");
        let low = identity::identity_get_trust_score(me.clone(), stranger.clone()).unwrap();
        assert!(low < 0.5, "no path should score low: {low}");

        assert_eq!(
            identity::identity_get_wot_status(friend.clone(), me.clone()).unwrap(),
            "trusted"
        );
        assert_eq!(
            identity::identity_get_wot_status(stranger.clone(), me.clone()).unwrap(),
            "unknown"
        );
        let _ = db;
    }

    #[test]
    fn blocked_users_roundtrip_via_moderation() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = init_db("blocks");
        let me = "a".repeat(64);
        let target = "b".repeat(64);

        assert!(identity::identity_is_blocked(me.clone(), target.clone()).unwrap() == false);
        assert!(moderation::moderation_block_user(me.clone(), target.clone()).unwrap());
        assert!(identity::identity_is_blocked(me.clone(), target.clone()).unwrap());
        let list = identity::identity_get_blocked_users(me.clone()).unwrap();
        assert_eq!(list, vec![target.clone()]);
        assert!(moderation::moderation_unblock_user(me.clone(), target.clone()).unwrap());
        assert!(identity::identity_is_blocked(me.clone(), target.clone()).unwrap() == false);
        let _ = db;
    }

    #[test]
    fn update_profile_signer_gated_and_signs() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = init_db("update");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_lock().unwrap();

        let locked = identity::identity_update_profile(
            pk.clone(),
            "n".into(),
            "d".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        )
        .unwrap_err();
        assert!(locked.contains("signer locked"), "{locked}");

        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let other = identity::identity_update_profile(
            "f".repeat(64),
            "n".into(),
            "d".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        )
        .unwrap_err();
        assert!(other.contains("does not match"), "{other}");

        let signed = identity::identity_update_profile(
            pk.clone(),
            "alice".into(),
            "Alice".into(),
            "https://example.com/p.png".into(),
            "".into(),
            "bio".into(),
            "alice@example.com".into(),
        )
        .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev["kind"], 0);
        assert_eq!(ev["pubkey"], pk);
        assert!(ev["content"]
            .as_str()
            .unwrap()
            .contains("\"name\":\"alice\""));
        let _ = db;
    }

    #[test]
    fn publish_relay_list_and_follow_unfollow_reach_publish() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = init_db("publish");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_lock().unwrap();

        let locked = identity::identity_publish_relay_list(vec!["wss://relay.example.com".into()])
            .unwrap_err();
        assert!(locked.contains("signer locked"), "{locked}");
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        let bad_url =
            identity::identity_publish_relay_list(vec!["http://relay.example.com".into()])
                .unwrap_err();
        assert!(bad_url.contains("invalid relay url"), "{bad_url}");

        let no_client =
            identity::identity_publish_relay_list(vec!["wss://relay.example.com".into()])
                .unwrap_err();
        assert!(
            no_client.contains("relay client not initialized"),
            "{no_client}"
        );

        let target = "b".repeat(64);
        signer::signer_lock().unwrap();
        let locked = identity::identity_follow_user(target.clone()).unwrap_err();
        assert!(locked.contains("signer locked"), "{locked}");
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let no_client = identity::identity_follow_user(target.clone()).unwrap_err();
        assert!(
            no_client.contains("relay client not initialized"),
            "{no_client}"
        );
        let no_client = identity::identity_unfollow_user(target.clone()).unwrap_err();
        assert!(
            no_client.contains("relay client not initialized"),
            "{no_client}"
        );
        let _ = db;
    }

    #[test]
    fn custom_profile_pubkey_gated() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = init_db("custom");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_lock().unwrap();

        let locked =
            identity::identity_publish_custom_profile(pk.clone(), "{}".into()).unwrap_err();
        assert!(locked.contains("signer locked"), "{locked}");
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        let mismatch =
            identity::identity_publish_custom_profile("f".repeat(64), "{}".into()).unwrap_err();
        assert!(mismatch.contains("does not match"), "{mismatch}");

        let no_client = identity::identity_publish_custom_profile(pk, "{}".into()).unwrap_err();
        assert!(
            no_client.contains("relay client not initialized"),
            "{no_client}"
        );
        let _ = db;
    }

    #[test]
    fn nip05_verify_rejects_local_and_malformed() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let err = identity::identity_verify_nip05("x@localhost".into())
                .await
                .unwrap_err();
            assert!(!err.is_empty(), "{err}");
            let err = identity::identity_verify_nip05("x@".into())
                .await
                .unwrap_err();
            assert!(!err.is_empty(), "{err}");
        });
    }

    #[test]
    fn in_process_signer_valid_and_invalid() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = identity::identity_in_process_signer(keys.secret_key().to_secret_hex()).unwrap();
        assert_eq!(pk, keys.public_key().to_hex());
        let e = identity::identity_in_process_signer("junk".into()).unwrap_err();
        assert!(e.contains("invalid nsec"), "{e}");
    }
}
