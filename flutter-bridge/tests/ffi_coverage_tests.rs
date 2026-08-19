#[path = "common/mod.rs"]
mod test_util;

#[cfg(test)]
mod ffi_coverage_tests {
    use soshal_flutter_bridge::*;
    fn unlock_signer() -> String {
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        let secret = keys.secret_key().to_secret_hex();
        signer::signer_unlock(secret).unwrap();
        pk
    }
    #[test]
    fn auth_public_key_from_nsec_valid_and_invalid() {
        let _g = crate::test_util::lock();
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = auth::auth_public_key_from_nsec(keys.secret_key().to_secret_hex()).unwrap();
        assert_eq!(pk, keys.public_key().to_hex());
        let e = auth::auth_public_key_from_nsec("not-a-secret".into()).unwrap_err();
        assert!(!e.is_empty());
    }
    #[test]
    fn identity_in_process_signer_valid_and_invalid() {
        let _g = crate::test_util::lock();
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = identity::identity_in_process_signer(keys.secret_key().to_secret_hex()).unwrap();
        assert_eq!(pk, keys.public_key().to_hex());
        let e = identity::identity_in_process_signer("junk".into()).unwrap_err();
        assert!(e.contains("invalid nsec"), "got {e}");
    }
    #[test]
    fn util_truncate_and_thread_affinity() {
        let _g = crate::test_util::lock();
        let truncated = util::util_truncate("hello world".into(), 5).unwrap();
        assert!(truncated.starts_with("hell"), "got {truncated}");
        assert!(
            truncated.ends_with('…'),
            "ellipsis suffix expected, got {truncated}"
        );
        assert_eq!(util::util_truncate("short".into(), 100).unwrap(), "short");
        assert!(util::util_apply_thread_affinity(true).unwrap());
        assert!(util::util_apply_thread_affinity(false).unwrap());
    }
    #[test]
    fn media_telemetry_and_raster_and_sync_running() {
        let _g = crate::test_util::lock();
        assert!(media::media_update_scroll_telemetry(12.5, 0, 3).unwrap());
        assert!(raster::raster_signal_impeller_frame_ready(1, 0).unwrap());
        let _ = sync::sync_running().unwrap();
    }
    #[test]
    fn network_status_probes_report_false_without_daemons() {
        let _g = crate::test_util::lock();
        assert!(!network::network_i2p_status().unwrap());
        assert!(!network::network_freenet_status().unwrap());
    }
    #[test]
    fn reticulum_pure_status_and_announce() {
        let _g = crate::test_util::lock();
        let pubkey = "ab".repeat(32);
        let status = network::reticulum_get_status(pubkey.clone()).unwrap();
        assert!(status.contains('{'), "got {status}");
        let announce = network::reticulum_create_announce(pubkey, Some("soshal".into())).unwrap();
        assert!(announce.contains('{'), "got {announce}");
    }
    #[test]
    fn reticulum_send_packet_rejects_bad_destination() {
        let _g = crate::test_util::lock();
        let pubkey = "cd".repeat(32);
        let e =
            network::reticulum_send_packet(pubkey, "not-an-addr".into(), "{}".into()).unwrap_err();
        assert!(e.contains("Invalid destination"), "got {e}");
    }
    #[test]
    fn reticulum_binds_local_interfaces() {
        let _g = crate::test_util::lock();
        let a = "ee".repeat(32);
        let b = "ff".repeat(32);
        let c = "11".repeat(32);
        assert!(network::reticulum_start_transport(a, "127.0.0.1:0".into()).is_ok());
        assert!(network::reticulum_start_auto_interface(b, true, 0, 1000).is_ok());
        assert!(network::reticulum_start_tcp_server(c, 0, 4).is_ok());
    }
    #[test]
    fn i2p_fns_fail_without_sam_bridge() {
        let _g = crate::test_util::lock();
        assert!(network::i2p_connect("127.0.0.1".into(), 1).is_err());
        assert!(network::i2p_create_session("127.0.0.1".into(), 1, "s".into(), None).is_err());
        assert!(network::i2p_generate_destination("127.0.0.1".into(), 1).is_err());
        assert!(network::i2p_connect_to_destination(
            "127.0.0.1".into(),
            1,
            "s".into(),
            "dest".into()
        )
        .is_err());
        assert!(network::i2p_start_session(None).is_err());
    }
    #[test]
    fn events_interest_scores() {
        let _g = crate::test_util::lock();
        let out = events::events_interest_score(
            r#"["nostr","rust"]"#.into(),
            r#"["nostr","python"]"#.into(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            (v["score"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-9,
            "got {out}"
        );
        assert_eq!(v["common"][0], "nostr");
        let bad = events::events_interest_score("nope".into(), r#"[]"#.into()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&bad).unwrap();
        assert_eq!(v["score"], 0);
    }
    #[test]
    fn events_score_events_roundtrip() {
        let _g = crate::test_util::lock();
        let events = r#"[{"id":"ev1","title":"Rust meetup","description":"talk"}]"#;
        let out = events::events_score_events(events.into(), r#"["rust"]"#.into()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("ev1").is_some(), "got {out}");
        let e = events::events_score_events("not-json".into(), r#"[]"#.into()).unwrap_err();
        assert!(!e.is_empty());
    }
    #[test]
    fn db_save_and_get_custom_profile_nodes() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "profile_nodes");
        let pk = "pk_profile".to_string();
        assert_eq!(db::db_get_custom_profile_nodes(pk.clone()).unwrap(), "[]");
        let profile = r#"{"name":"alice","bio":"hi"}"#;
        assert!(db::db_save_custom_profile(pk.clone(), profile.into()).unwrap());
        let got = db::db_get_custom_profile_nodes(pk).unwrap();
        assert_eq!(got, profile);
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn db_delete_posts_trending_escrows_geohash() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "db_misc_cov");
        assert_eq!(db::db_delete_all_posts().unwrap(), 0);
        let trending = db::db_get_trending_hashtags(10).unwrap();
        assert_eq!(trending, "[]");
        let escrows = db::db_get_escrows_by_participant("pk".into()).unwrap();
        assert_eq!(escrows, "[]");
        assert_eq!(db::db_purge_stale_geohash_peers(3600).unwrap(), 0);
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn events_ffi_db_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "events_ffi");
        let pk = unlock_signer();
        let empty_nearby = events::events_fetch_nearby(37.0, -122.0, 10.0, 5).unwrap();
        assert_eq!(empty_nearby, "[]");
        let empty_user = events::events_fetch_user_events(pk.clone(), 5).unwrap();
        assert_eq!(empty_user, "[]");
        let now = soshal_common_core::format::now_secs() as u64;
        let created = events::events_create(
            pk.clone(),
            "Test Event".into(),
            "desc".into(),
            "HQ".into(),
            37.5,
            -122.4,
            now,
            now + 3600,
            String::new(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&created).unwrap();
        let event_id = v["id"].as_str().unwrap().to_string();
        assert!(!event_id.is_empty());
        let nearby = events::events_fetch_nearby(37.5, -122.4, 5000.0, 5).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&nearby).unwrap();
        assert_eq!(
            arr.as_array().unwrap().len(),
            1,
            "object-location event within radius"
        );
        let mine = events::events_fetch_user_events(pk.clone(), 5).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&mine).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 1);
        assert!(events::events_check_in(event_id.clone(), pk, 37.5, -122.4).unwrap());
        let attendees = events::events_get_attendees(event_id).unwrap();
        assert!(attendees.is_empty(), "check-in is not an RSVP");
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn events_create_validation_errors() {
        let _g = crate::test_util::lock();
        let e = events::events_create(
            "pk".into(),
            " ".into(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            100,
            200,
            String::new(),
        )
        .unwrap_err();
        assert!(e.contains("title"), "got {e}");
        let e = events::events_create(
            "pk".into(),
            "t".into(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            0,
            0,
            String::new(),
        )
        .unwrap_err();
        assert_eq!(e, "invalid time range");
        let e = events::events_create(
            "pk".into(),
            "t".into(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            100,
            100 + 8 * 24 * 3600,
            String::new(),
        )
        .unwrap_err();
        assert!(e.contains("7 days"), "got {e}");
    }
    #[test]
    fn events_reminders_ffi_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "reminders_ffi");
        let e = events::events_reminder_upsert(
            String::new(),
            "ev1".into(),
            "party".into(),
            100_000,
            -1,
        )
        .unwrap_err();
        assert_eq!(e, "minutes_before must be >= 0");
        let id = events::events_reminder_upsert(
            String::new(),
            "ev1".into(),
            "party".into(),
            100_000,
            60,
        )
        .unwrap();
        assert!(!id.is_empty());
        let list = events::events_reminders_list().unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert!(events::events_reminder_delete(id).unwrap());
        let list = events::events_reminders_list().unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert!(v.as_array().unwrap().is_empty());
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn messaging_ffi_store_and_fetch() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "messaging_cov");
        let pk = unlock_signer();
        assert!(messaging::messaging_store_dm(
            "dm1".into(),
            "peer_a".into(),
            pk.clone(),
            "sealed content".into(),
            1000,
            "[]".into(),
        )
        .unwrap());
        let dms = messaging::messaging_fetch_dms("peer_a".into(), 50).unwrap();
        let v: serde_json::Value = serde_json::from_str(&dms).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        let conversations = messaging::messaging_fetch_conversations(pk).unwrap();
        assert!(
            conversations.contains(&"peer_a".to_string()),
            "got {conversations:?}"
        );
        assert!(messaging::messaging_store_dms(
            r#"[{"id":"dm2","sender":"peer_b","recipient":"me","content":"x","created_at":2000}]"#
                .into()
        )
        .unwrap());
        let e = messaging::messaging_store_dms("junk".into()).unwrap_err();
        assert!(e.contains("invalid DMs JSON"), "got {e}");
        assert!(messaging::messaging_store_dms("[]".into()).unwrap());
        let e = messaging::messaging_send_group_dm(String::new(), "g1".into(), "[]".into())
            .unwrap_err();
        assert_eq!(e, "message must not be empty");
        let out = messaging::messaging_send_group_dm(
            "hello".into(),
            "g1".into(),
            format!(r#"["{}"]"#, "ab".repeat(32)),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["id"].is_string(), "signed event expected");
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn identity_publish_paths_err_without_relay_client() {
        let _g = crate::test_util::lock();
        let pk = unlock_signer();
        let other = "ab".repeat(32);
        let e =
            identity::identity_publish_custom_profile(other, r#"{"name":"x"}"#.into()).unwrap_err();
        assert!(e.contains("does not match"), "got {e}");
        let e =
            identity::identity_publish_custom_profile(pk, r#"{"name":"x"}"#.into()).unwrap_err();
        assert!(
            e.contains("relay client not initialized") || e.contains("publish failed"),
            "got {e}"
        );
        let e =
            identity::identity_publish_relay_list(vec!["wss://relay.example".into()]).unwrap_err();
        assert!(
            e.contains("relay client not initialized") || e.contains("publish failed"),
            "got {e}"
        );
    }
    #[test]
    fn identity_fetch_follows_empty() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "identity_cov");
        let got = identity::identity_fetch_follows("pk".into()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert!(v.is_array(), "got {got}");
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn sync_outbox_ffi_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "sync_cov");
        let id = sync::sync_enqueue_outbox("post".into(), r#"{"a":1}"#.into(), None).unwrap();
        assert!(!id.is_empty());
        let summary = sync::sync_get_outbox_summary().unwrap();
        let v: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert!(
            v["pending_count"].as_i64().unwrap_or(0) >= 1,
            "got {summary}"
        );
        let gc =
            sync::sync_run_epoch_garbage_collection("posts".into(), "{}".into(), 3600).unwrap();
        let v: serde_json::Value = serde_json::from_str(&gc).unwrap();
        assert!(v.is_object(), "got {gc}");
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn bookmarks_resolve_posts() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "bookmarks_cov");
        let out = bookmarks::bookmarks_resolve_posts("[]".into()).unwrap();
        assert_eq!(out, "{}");
        let e = bookmarks::bookmarks_resolve_posts("junk".into()).unwrap_err();
        assert!(e.contains("invalid ids JSON"), "got {e}");
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn dating_report_profile_stores_report() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "dating_cov");
        assert!(
            dating::dating_report_profile("reporter".into(), "target".into(), "spam".into(),)
                .unwrap()
        );
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn search_index_posts_batch() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("coverage", "search_cov");
        let e = search::search_index_posts("junk".into()).unwrap_err();
        assert!(e.contains("invalid rows JSON"), "got {e}");
        assert!(search::search_index_posts(
            r#"[{"id":"p1","pubkey":"pk","content":"hello","kind":1}]"#.into()
        )
        .unwrap());
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn p2p_mdns_start_stop_and_quic() {
        let _g = crate::test_util::lock();
        let e = p2p::p2p_mdns_advertise_start("short".into(), 9876, None).unwrap_err();
        assert_eq!(e, "invalid pubkey");
        let pk = "ab".repeat(32);
        assert!(p2p::p2p_mdns_advertise_start(pk, 9876, None).unwrap());
        assert!(p2p::p2p_mdns_advertise_stop().unwrap());
        assert!(p2p::p2p_mdns_browse_start().unwrap());
        let peers = p2p::p2p_mdns_browse_drain().unwrap();
        assert!(peers.is_empty());
        assert!(p2p::p2p_mdns_browse_stop().unwrap());
        unlock_signer();
        let port = p2p::p2p_quic_server_start(String::new()).unwrap();
        assert!(port > 0);
        assert_eq!(p2p::p2p_quic_server_port().unwrap(), port);
        assert!(p2p::p2p_quic_server_stop().unwrap());
    }
}
