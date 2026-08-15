#[cfg(test)]
mod ffi_identity_tests {
    use soshal_flutter_bridge::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn unique_pubkey(tag: &str) -> String {
        let kp: KeyPairResult =
            serde_json::from_str(&auth::auth_generate_keypair().unwrap()).unwrap();
        format!("{}_{}", tag, &kp.public_key[..12])
    }

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

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    fn profile_event(pubkey: &str, name: &str) -> String {
        let content = serde_json::json!({
            "name": name,
            "display_name": format!("{name} display"),
            "about": "smoke tester",
            "picture": "https://example.com/pic.png",
            "banner": "https://example.com/banner.png",
            "nip05": format!("{name}@example.com"),
        })
        .to_string();
        serde_json::json!({
            "pubkey": pubkey,
            "content": content,
            "created_at": 1700000000,
        })
        .to_string()
    }

    #[test]
    fn test_identity_store_get_profile_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("roundtrip");
        let pubkey = unique_pubkey("rt");
        assert!(identity::identity_store_profile(profile_event(&pubkey, "alice_rt")).is_ok());
        let got = identity::identity_get_profile(pubkey.clone()).unwrap();
        let p: ProfileInfo = serde_json::from_str(&got).unwrap();
        assert_eq!(p.pubkey, pubkey);
        assert_eq!(p.name, "alice_rt");
        assert_eq!(p.display_name, "alice_rt display");
        assert_eq!(p.about, "smoke tester");
        assert_eq!(p.nip05, "alice_rt@example.com");
        assert!(!p.is_following);
        assert_eq!(p.wot_status, "unknown");
        cleanup(&path);
    }

    #[test]
    fn test_identity_get_self_profile_unknown() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("self");
        let pubkey = unique_pubkey("self");
        let got = identity::identity_get_self_profile(pubkey.clone()).unwrap();
        let p: ProfileInfo = serde_json::from_str(&got).unwrap();
        assert_eq!(p.pubkey, pubkey);
        assert!(p.name.is_empty());
        assert_eq!(p.wot_status, "unknown");
        cleanup(&path);
    }

    #[test]
    fn test_identity_update_profile() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("update");
        signer::signer_lock().unwrap();
        let err = identity::identity_update_profile(
            "aa606060606060606060606060606060606060606060606060606060606060606a".to_string(),
            "bob".to_string(),
            "bob display".to_string(),
            String::new(),
            String::new(),
            "about bob".to_string(),
            "bob@example.com".to_string(),
        );
        assert!(err.unwrap_err().contains("signer locked"));
        let kp: KeyPairResult =
            serde_json::from_str(&auth::auth_generate_keypair().unwrap()).unwrap();
        assert!(!kp.public_key.is_empty());
        let signed = identity::identity_update_profile(
            kp.public_key.clone(),
            "bob".to_string(),
            "bob display".to_string(),
            String::new(),
            String::new(),
            "about bob".to_string(),
            "bob@example.com".to_string(),
        )
        .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev["kind"], 0);
        assert_eq!(ev["pubkey"], kp.public_key);
        assert!(ev["content"].as_str().unwrap().contains("about bob"));
        signer::signer_lock().unwrap();
        cleanup(&path);
    }

    #[test]
    fn test_identity_follow_unfollow_signer_locked() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        signer::signer_lock().unwrap();
        let path = init_db("follow");
        let target = "aa".repeat(32);
        let err = identity::identity_follow_user(target.clone());
        assert!(err.unwrap_err().contains("signer locked"));
        let err = identity::identity_unfollow_user(target);
        assert!(err.unwrap_err().contains("signer locked"));
        cleanup(&path);
    }

    #[test]
    fn test_identity_block_unblock_flow() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("block");
        let me = unique_pubkey("me");
        let target = unique_pubkey("tgt");
        assert!(!identity::identity_is_blocked(me.clone(), target.clone()).unwrap());
        assert!(moderation::moderation_block_user(me.clone(), target.clone()).unwrap());
        assert!(identity::identity_is_blocked(me.clone(), target.clone()).unwrap());
        let list = identity::identity_get_blocked_users(me.clone()).unwrap();
        assert!(list.contains(&target));
        assert!(moderation::moderation_unblock_user(me.clone(), target.clone()).unwrap());
        assert!(!identity::identity_is_blocked(me.clone(), target.clone()).unwrap());
        let list = identity::identity_get_blocked_users(me).unwrap();
        assert!(!list.contains(&target));
        cleanup(&path);
    }

    #[test]
    fn test_identity_search_users() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("search");
        let pubkey = unique_pubkey("srch");
        let res = identity::identity_search_users("zzzz_no_such_user_qq".to_string(), 10);
        let rows: Vec<ProfileInfo> = serde_json::from_str(&res.unwrap()).unwrap();
        assert!(rows.is_empty());
        assert!(
            identity::identity_store_profile(profile_event(&pubkey, "alice_smoke_xyz")).is_ok()
        );
        let res = identity::identity_search_users("alice_smoke".to_string(), 10);
        let rows: Vec<ProfileInfo> = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "alice_smoke_xyz");
        cleanup(&path);
    }

    #[test]
    fn test_identity_wot_and_trust_score() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("wot");
        let viewer = unique_pubkey("view");
        let target = unique_pubkey("wtarget");
        assert_eq!(
            identity::identity_get_wot_status(target.clone(), viewer.clone()).unwrap(),
            "unknown"
        );
        let score = identity::identity_get_trust_score(viewer, target).unwrap();
        assert!((0.0..=1.0).contains(&score));
        cleanup(&path);
    }
}
