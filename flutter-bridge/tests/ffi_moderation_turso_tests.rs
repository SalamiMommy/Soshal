#[cfg(test)]
mod ffi_tests {
    use soshal_flutter_bridge::*;
    use std::sync::Mutex;

    static DB_LOCK: Mutex<()> = Mutex::new(());

    fn init_db(name: &str) -> (String, std::sync::MutexGuard<'static, ()>) {
        let g = DB_LOCK.lock().unwrap();
        let path = soshal_test_util::tmp_path("mod", name)
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db::db_init(path.clone()).is_ok());
        (path, g)
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_should_filter_pattern_content() {
        assert!(moderation::moderation_should_filter(
            "this post contains kike".to_string(),
            "me".to_string()
        )
        .unwrap());
        let clean_verdict = r#"{"passed":true,"category":null}"#;
        assert!(
            !moderation::moderation_should_filter(clean_verdict.to_string(), "me".to_string())
                .unwrap()
        );
    }

    #[test]
    fn test_word_filters_roundtrip() {
        let (path, _db_guard) = init_db("wordfilters");
        assert!(moderation::moderation_set_word_filters(
            r#"["badword","spam","kike"]"#.to_string()
        )
        .is_ok());
        assert_eq!(
            moderation::moderation_get_word_filters().unwrap(),
            vec![
                "badword".to_string(),
                "spam".to_string(),
                "kike".to_string()
            ]
        );
        assert!(moderation::moderation_set_word_filters("not json".to_string()).is_err());
        cleanup(&path);
    }

    #[test]
    fn test_mute_unmute_roundtrip() {
        let (path, _db_guard) = init_db("mute");
        assert!(
            moderation::moderation_mute_user("me_pk".to_string(), "target_pk".to_string()).is_ok()
        );
        assert!(moderation::moderation_get_muted("me_pk".to_string())
            .unwrap()
            .contains(&"target_pk".to_string()));
        assert!(
            moderation::moderation_is_restricted("me_pk".to_string(), "target_pk".to_string())
                .unwrap()
        );
        assert!(
            moderation::moderation_mute_user("me_pk".to_string(), "me_pk".to_string()).is_err()
        );
        assert!(
            moderation::moderation_unmute_user("me_pk".to_string(), "target_pk".to_string())
                .is_ok()
        );
        assert!(!moderation::moderation_get_muted("me_pk".to_string())
            .unwrap()
            .contains(&"target_pk".to_string()));
        cleanup(&path);
    }

    #[test]
    fn test_block_unblock_roundtrip() {
        let (path, _db_guard) = init_db("block");
        assert!(
            moderation::moderation_block_user("me_pk".to_string(), "target_pk".to_string()).is_ok()
        );
        assert!(moderation::moderation_get_blocked("me_pk".to_string())
            .unwrap()
            .contains(&"target_pk".to_string()));
        assert!(
            moderation::moderation_is_restricted("me_pk".to_string(), "target_pk".to_string())
                .unwrap()
        );
        assert!(
            moderation::moderation_block_user("me_pk".to_string(), "me_pk".to_string()).is_err()
        );
        assert!(
            moderation::moderation_unblock_user("me_pk".to_string(), "target_pk".to_string())
                .is_ok()
        );
        assert!(moderation::moderation_get_blocked("me_pk".to_string())
            .unwrap()
            .is_empty());
        cleanup(&path);
    }

    #[test]
    fn test_report_roundtrip() {
        let (path, _db_guard) = init_db("report");
        assert!(moderation::moderation_report_content(
            "reporter_pk".to_string(),
            "post".to_string(),
            "post_id_1".to_string(),
            "spam".to_string()
        )
        .unwrap());
        assert!(moderation::moderation_report_content(
            "reporter_pk".to_string(),
            "post".to_string(),
            "post_id_2".to_string(),
            "   ".to_string()
        )
        .is_err());
        let rows: Vec<serde_json::Value> = serde_json::from_str(
            &db::db_query_raw("SELECT id, pubkey, target_id, reason FROM spam_reports".to_string())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["pubkey"], "reporter_pk");
        assert_eq!(rows[0]["target_id"], "post_id_1");
        assert_eq!(rows[0]["reason"], "spam");
        // list_reports filters on target_pubkey, which report_content never
        // sets (content_type is dropped) -> stays empty, but no longer Err.
        let listed: Vec<serde_json::Value> = serde_json::from_str(
            &moderation::moderation_list_reports("reporter_pk".to_string(), 10).unwrap(),
        )
        .unwrap();
        assert!(listed.is_empty());
        let id = rows[0]["id"].as_str().unwrap().to_string();
        assert!(moderation::moderation_delete_report(id.clone()).unwrap());
        let gone: Vec<serde_json::Value> = serde_json::from_str(
            &db::db_query_raw("SELECT id FROM spam_reports".to_string()).unwrap(),
        )
        .unwrap();
        assert!(gone.is_empty());
        cleanup(&path);
    }

    #[test]
    fn test_jury_case_create_and_partial_vote() {
        let case = moderation::moderation_create_jury_case(
            "case_x".to_string(),
            "target_pk".to_string(),
            "Spam behavior".to_string(),
            2,
            3,
            "group_pk".to_string(),
        )
        .unwrap();
        assert!(case.contains("case_x"));
        let vote = r#"{"participant_id":1,"sig_share_hex":"abcd"}"#;
        let out = moderation::moderation_submit_jury_vote(case.clone(), vote.to_string()).unwrap();
        assert!(out.contains(r#""threshold_reached":false"#));
        let again =
            moderation::moderation_submit_jury_vote(case.clone(), vote.to_string()).unwrap();
        assert!(again.contains(r#""votes_count":1"#));
        assert!(
            moderation::moderation_submit_jury_vote("bad case".to_string(), vote.to_string())
                .is_err()
        );
        assert!(moderation::moderation_submit_jury_vote(case, "not json".to_string()).is_err());
    }

    #[test]
    fn test_jury_vote_reaches_threshold() {
        let case = moderation::moderation_create_jury_case(
            "case_y".to_string(),
            "target_pk".to_string(),
            "Spam behavior".to_string(),
            1,
            3,
            "group_pk".to_string(),
        )
        .unwrap();
        let vote = r#"{"participant_id":1,"sig_share_hex":"abcd"}"#;
        let out = moderation::moderation_submit_jury_vote(case, vote.to_string()).unwrap();
        assert!(out.contains(r#""threshold_reached":true"#));
        assert!(!out.contains(r#""verdict_signature":null"#));
    }

    #[test]
    fn test_turso_configure_and_status() {
        let (path, _db_guard) = init_db("turso");
        let res = moderation::moderation_unmute_user("x".to_string(), "y".to_string());
        assert!(res.is_ok());
        let status = turso::db_turso_status().unwrap();
        assert!(status.contains(r#""configured":false"#));
        assert_eq!(
            turso::db_turso_configure("https://example.turso.io".to_string(), "tok".to_string())
                .unwrap(),
            "Turso database credentials saved"
        );
        let after = turso::db_turso_status().unwrap();
        assert!(after.contains(r#""configured":true"#));
        cleanup(&path);
    }

    #[test]
    fn test_turso_sync_paths() {
        let (path, _db_guard) = init_db("tursosync");
        assert!(turso::db_turso_sync()
            .unwrap_err()
            .contains("Turso credentials not configured"));
        assert!(
            turso::db_turso_configure("https://sync.turso.io".to_string(), "tok".to_string())
                .is_ok()
        );
        let synced = turso::db_turso_sync().unwrap();
        assert!(synced.contains("Turso sync complete: target https://sync.turso.io"));
        let after = turso::db_turso_status().unwrap();
        assert!(after.contains(r#""status":"synced""#));
        cleanup(&path);
    }
}
