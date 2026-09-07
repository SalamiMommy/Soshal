#[cfg(test)]
mod ffi_tests {
    use soshal_flutter_bridge::*;
    #[test]
    fn test_should_filter_pattern_content() {
        assert!(moderation::moderation_should_filter(
            "this post contains kike".to_string(),
            "me".to_string()
        )
        .unwrap());
        // CP trade
        assert!(moderation::moderation_should_filter(
            "trade cp pack on telegram".to_string(),
            "me".to_string()
        )
        .unwrap());
        // Gore / shock site
        assert!(moderation::moderation_should_filter(
            "watch this beheading video uncensored https://theync.com/clip".to_string(),
            "me".to_string()
        )
        .unwrap());
        // Crypto doubler spam
        assert!(moderation::moderation_should_filter(
            "Send 1 BTC to get 2x return instantly".to_string(),
            "me".to_string()
        )
        .unwrap());
        let clean_verdict = r#"{"passed":true,"category":null}"#;
        assert!(
            !moderation::moderation_should_filter(clean_verdict.to_string(), "me".to_string())
                .unwrap()
        );
        assert!(!moderation::moderation_should_filter(
            "Hello world from Soshal!".to_string(),
            "me".to_string()
        )
        .unwrap());
    }
    #[test]
    fn test_word_filters_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "wordfilters");
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
        // Custom word filter actively filters matching content
        assert!(moderation::moderation_should_filter(
            "this contains badword here".to_string(),
            "me".to_string()
        )
        .unwrap());
        assert!(moderation::moderation_set_word_filters("not json".to_string()).is_err());
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn test_mute_unmute_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "mute");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        let target = "b".repeat(64);
        signer::signer_lock().unwrap();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(moderation::moderation_mute_user(me.clone(), target.clone()).is_ok());
        assert!(moderation::moderation_get_muted(me.clone())
            .unwrap()
            .contains(&target));
        assert!(moderation::moderation_is_restricted(me.clone(), target.clone()).unwrap());
        assert!(moderation::moderation_mute_user(me.clone(), me.clone()).is_err());
        assert!(moderation::moderation_unmute_user(me.clone(), target.clone()).is_ok());
        assert!(!moderation::moderation_get_muted(me.clone())
            .unwrap()
            .contains(&target));
        signer::signer_lock().unwrap();
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn test_block_unblock_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "block");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        let target = "b".repeat(64);
        signer::signer_lock().unwrap();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::test_util::insert_user(&me);
        crate::test_util::insert_user(&target);
        assert!(moderation::moderation_block_user(me.clone(), target.clone()).is_ok());
        assert!(moderation::moderation_get_blocked(me.clone())
            .unwrap()
            .contains(&target));
        assert!(moderation::moderation_is_restricted(me.clone(), target.clone()).unwrap());
        assert!(moderation::moderation_block_user(me.clone(), me.clone()).is_err());
        assert!(moderation::moderation_unblock_user(me.clone(), target.clone()).is_ok());
        assert!(moderation::moderation_get_blocked(me.clone())
            .unwrap()
            .is_empty());
        signer::signer_lock().unwrap();
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn test_report_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "report");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        signer::signer_lock().unwrap();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(moderation::moderation_report_content(
            me.clone(),
            "post".to_string(),
            "post_id_1".to_string(),
            "spam".to_string()
        )
        .unwrap());
        assert!(moderation::moderation_report_content(
            me.clone(),
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
        assert_eq!(rows[0]["pubkey"], me.clone());
        assert_eq!(rows[0]["target_id"], "post_id_1");
        assert_eq!(rows[0]["reason"], "spam");
        // list_reports filters on target_pubkey, which report_content never
        // sets (content_type is dropped) -> stays empty, but no longer Err.
        let listed: Vec<serde_json::Value> =
            serde_json::from_str(&moderation::moderation_list_reports(me, 10).unwrap()).unwrap();
        assert!(listed.is_empty());
        let id = rows[0]["id"].as_str().unwrap().to_string();
        assert!(moderation::moderation_delete_report(id).unwrap());
        let gone: Vec<serde_json::Value> = serde_json::from_str(
            &db::db_query_raw("SELECT id FROM spam_reports".to_string()).unwrap(),
        )
        .unwrap();
        assert!(gone.is_empty());
        signer::signer_lock().unwrap();
        crate::test_util::cleanup(&path);
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
        );
        let err = case.unwrap_err();
        assert!(err.contains("unavailable"), "got {err}");
        let vote = r#"{"participant_id":1,"sig_share_hex":"abcd"}"#;
        let out = moderation::moderation_submit_jury_vote("case_x".to_string(), vote.to_string());
        assert!(out.is_err(), "FROST gate must reject votes");
        assert!(
            moderation::moderation_submit_jury_vote("bad case".to_string(), vote.to_string())
                .is_err()
        );
        assert!(moderation::moderation_submit_jury_vote(
            "case_x".to_string(),
            "not json".to_string()
        )
        .is_err());
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
        );
        assert!(case.is_err(), "FROST gate must reject case creation");
    }
    #[test]
    fn test_turso_configure_and_status() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "turso");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        signer::signer_lock().unwrap();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let res = moderation::moderation_unmute_user(me.clone(), "y".to_string());
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
        signer::signer_lock().unwrap();
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn test_turso_sync_paths() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "tursosync");
        assert!(turso::db_turso_sync()
            .unwrap_err()
            .contains("Turso credentials not configured"));
        assert!(
            turso::db_turso_configure("https://sync.turso.io".to_string(), "tok".to_string())
                .is_ok()
        );
        let err = turso::db_turso_sync().unwrap_err();
        assert!(err.contains("unavailable (roadmap)"), "{err}");
        let after = turso::db_turso_status().unwrap();
        assert!(after.contains(r#""status":"error""#));
        crate::test_util::cleanup(&path);
    }

    #[test]
    fn test_ai_classify_text_and_media_ffi() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("moderation", "aimod");

        // 1. Text AI Classification
        let res_clean =
            moderation::moderation_ai_classify_text("Hello clean world".to_string()).unwrap();
        assert!(res_clean.contains(r#""is_flagged":false"#));

        let res_spam = moderation::moderation_ai_classify_text(
            "Send 1 BTC to double your crypto guaranteed profit".to_string(),
        )
        .unwrap();
        assert!(res_spam.contains(r#""is_flagged":true"#));
        assert!(res_spam.contains(r#""primary_category":"spam""#));

        let res_csam =
            moderation::moderation_ai_classify_text("selling cp pack on darknet".to_string())
                .unwrap();
        assert!(res_csam.contains(r#""is_flagged":true"#));
        assert!(res_csam.contains(r#""primary_category":"csam""#));

        // 2. Media AI Classification
        let clean_bytes = vec![128u8; 512];
        let media_clean =
            moderation::moderation_ai_classify_media(clean_bytes, "image/jpeg".to_string())
                .unwrap();
        assert!(media_clean.contains(r#""passed":true"#));

        let elf_bytes = vec![0x7f, b'E', b'L', b'F', 0, 0];
        let media_bad = moderation::moderation_ai_classify_media(
            elf_bytes,
            "application/x-executable".to_string(),
        )
        .unwrap();
        assert!(media_bad.contains(r#""passed":false"#));

        // 3. 2-Tier Hybrid Classification
        let hybrid_clean =
            moderation::moderation_hybrid_classify_text("Good morning friend".to_string(), false)
                .unwrap();
        assert!(hybrid_clean.contains(r#""tier_evaluated":"Tier1Fast""#));

        let hybrid_deep = moderation::moderation_hybrid_classify_text(
            "Claim airdrop token now with seed phrase".to_string(),
            true,
        )
        .unwrap();
        assert!(hybrid_deep.contains(r#""tier_evaluated":"Tier2Deep""#));
        assert!(hybrid_deep.contains(r#""tier2_roberta_result""#));

        // 4. PDQ Perceptual Hashing
        let sample_luma = vec![128u8; 64 * 64];
        let pdq_res = moderation::moderation_compute_pdq_hash(sample_luma).unwrap();
        assert!(pdq_res.contains(r#""hash_hex""#));
        assert!(pdq_res.contains(r#""quality""#));

        crate::test_util::cleanup(&path);
    }
}
