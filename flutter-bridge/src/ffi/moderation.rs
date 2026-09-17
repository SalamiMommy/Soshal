//! Moderation FFI module
//!
//! Mirrors the desktop moderation commands: mutes live in the per-account
//! settings key `{pubkey}:muted_users` (JSON list), blocks in the `blocks`
//! table, content checks + glitter sanitization delegate to
//! moderation-core pure functions.

use flutter_rust_bridge::frb;
use soshal_db_core::repos::block::{BlockRepo, BlockRow};
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::spam_report::{SpamReportRepo, SpamReportRow};

const MUTED_USERS_KEY: &str = "muted_users";

use super::util::uuid_like;

fn muted_list_key(pubkey: &str) -> String {
    if pubkey.is_empty() {
        MUTED_USERS_KEY.to_string()
    } else {
        format!("{pubkey}:{MUTED_USERS_KEY}")
    }
}

fn parse_pubkey_list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Mute a user (per-account settings list; never yourself).
#[frb(sync, serialize)]
pub fn moderation_mute_user(muter_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    if target_pubkey == muter_pubkey {
        return Err("cannot mute yourself".to_string()).into();
    }
    // The claimed actor must be the unlocked identity, or a compromised Dart
    // layer could attribute mutes/blocks/reports to an arbitrary pubkey.
    super::signer::require_identity(&muter_pubkey)?;
    super::db::with_db_result(|db| {
        let repo = SettingsRepo::new(db);
        let key = muted_list_key(&muter_pubkey);
        let raw = repo.get(&key)?.unwrap_or_default();
        let mut list: Vec<String> = parse_pubkey_list(&raw);
        if !list.iter().any(|k| k == &target_pubkey) {
            list.push(target_pubkey);
        }
        repo.set(
            &key,
            &serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
        )?;
        Ok(true)
    })
}

/// Unmute a user.
#[frb(sync, serialize)]
pub fn moderation_unmute_user(muter_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    super::signer::require_identity(&muter_pubkey)?;
    super::db::with_db_result(|db| {
        let repo = SettingsRepo::new(db);
        let key = muted_list_key(&muter_pubkey);
        let raw = repo.get(&key)?.unwrap_or_default();
        let mut list: Vec<String> = parse_pubkey_list(&raw);
        list.retain(|k| k != &target_pubkey);
        repo.set(
            &key,
            &serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
        )?;
        Ok(true)
    })
}

/// Block a user (blocks table; never yourself).
#[frb(sync, serialize)]
pub fn moderation_block_user(
    blocker_pubkey: String,
    target_pubkey: String,
) -> Result<bool, String> {
    if target_pubkey == blocker_pubkey {
        return Err("cannot block yourself".to_string()).into();
    }
    super::signer::require_identity(&blocker_pubkey)?;
    let row = BlockRow {
        pubkey: blocker_pubkey,
        blocked_pubkey: target_pubkey,
        created_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        BlockRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Unblock a user.
#[frb(sync, serialize)]
pub fn moderation_unblock_user(
    blocker_pubkey: String,
    target_pubkey: String,
) -> Result<bool, String> {
    super::signer::require_identity(&blocker_pubkey)?;
    super::db::with_db_result(|db| {
        BlockRepo::new(db).delete(&blocker_pubkey, &target_pubkey)?;
        Ok(true)
    })
}

/// Get muted users for an account.
#[frb(sync, serialize)]
pub fn moderation_get_muted(user_pubkey: String) -> Result<Vec<String>, String> {
    super::signer::require_identity(&user_pubkey)?;
    super::db::with_db_result(|db| {
        let raw = SettingsRepo::new(db)
            .get(&muted_list_key(&user_pubkey))?
            .unwrap_or_default();
        Ok(parse_pubkey_list(&raw))
    })
}

/// Get blocked users for an account.
#[frb(sync, serialize)]
pub fn moderation_get_blocked(user_pubkey: String) -> Result<Vec<String>, String> {
    super::signer::require_identity(&user_pubkey)?;
    super::db::with_db_result(|db| BlockRepo::new(db).list(&user_pubkey))
}

/// Whether the user has blocked or muted `target_pubkey`.
#[frb(sync, serialize)]
pub fn moderation_is_restricted(
    actor_pubkey: String,
    target_pubkey: String,
) -> Result<bool, String> {
    let muted = moderation_get_muted(actor_pubkey.clone())?
        .iter()
        .any(|k| k == &target_pubkey);
    if muted {
        return Ok(true).into();
    }
    super::db::with_db_result(|db| BlockRepo::new(db).is_blocked(&actor_pubkey, &target_pubkey))
}

/// Report content: stored as a local record so the moderation sync loop can
/// emit the report event; returns without relay interaction.
#[frb(sync, serialize)]
pub fn moderation_report_content(
    reporter_pubkey: String,
    content_type: String,
    content_id: String,
    reason: String,
) -> Result<bool, String> {
    super::signer::require_identity(&reporter_pubkey)?;
    // Validate content_type against a fixed allowlist so the API contract is
    // honest — unknown values are rejected rather than silently discarded.
    const VALID_CONTENT_TYPES: &[&str] =
        &["post", "comment", "account", "message", "image", "video"];
    if !VALID_CONTENT_TYPES.contains(&content_type.as_str()) {
        return Err(format!(
            "unknown content_type {content_type:?}; must be one of: {}",
            VALID_CONTENT_TYPES.join(", ")
        ))
        .into();
    }
    let reason = soshal_common_core::format::truncate(&reason, 512);
    if reason.trim().is_empty() {
        return Err("reason must not be empty".to_string()).into();
    }
    let row = SpamReportRow {
        id: uuid_like(),
        pubkey: reporter_pubkey,
        target_id: Some(content_id),
        target_pubkey: None,
        reason: Some(reason),
        tags: "[]".to_string(),
        created_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        SpamReportRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// List spam reports for a target pubkey (newest first), as a JSON array
/// with `id`, `pubkey` and `reason` per item.
#[frb(sync, serialize)]
pub fn moderation_list_reports(target_pubkey: String, limit: i64) -> Result<String, String> {
    if target_pubkey.trim().is_empty() || target_pubkey.len() > 128 {
        return Err("invalid target_pubkey".to_string()).into();
    }
    let limit = limit.clamp(1, 200);
    let items = super::db::with_db_result(|db| {
        let rows = SpamReportRepo::new(db).list_by_target(&target_pubkey, limit)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "pubkey": r.pubkey,
                    "reason": r.reason,
                })
            })
            .collect::<Vec<_>>())
    })?;
    serde_json::to_string(&items).map_err(|e| format!("json encode error: {e}"))
}

/// Delete a spam report by id.
#[frb(sync, serialize)]
pub fn moderation_delete_report(report_id: String) -> Result<bool, String> {
    if report_id.trim().is_empty() || report_id.len() > 128 {
        return Err("invalid report_id".to_string()).into();
    }
    super::db::with_db_result(|db| {
        SpamReportRepo::new(db).delete(&report_id)?;
        Ok(true)
    })
}

/// Check whether a piece of content should be hidden (moderation-core
/// check_text policy + word-list semantics on the Rust side).
#[frb(sync, serialize)]
pub fn moderation_should_filter(content: String, user_pubkey: String) -> Result<bool, String> {
    drop(user_pubkey);
    let custom_filters = super::db::with_db_result(|db| {
        let raw = SettingsRepo::new(db)
            .get("moderation_word_filters")?
            .unwrap_or_default();
        Ok(parse_pubkey_list(&raw))
    })
    .unwrap_or_default();

    let verdict = soshal_moderation_core::check::check_with_custom_words(&content, &custom_filters);
    Ok(!verdict.passed).into()
}

/// Get the active word filter list (device-global moderation settings key).
#[frb(sync, serialize)]
pub fn moderation_get_word_filters() -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| {
        let raw = SettingsRepo::new(db)
            .get("moderation_word_filters")?
            .unwrap_or_default();
        Ok(parse_pubkey_list(&raw))
    })
}

/// Update the word filter list.
#[frb(sync, serialize)]
pub fn moderation_set_word_filters(filters_json: String) -> Result<bool, String> {
    let filters: Vec<String> =
        serde_json::from_str(&filters_json).map_err(|e| format!("invalid filters JSON: {e}"))?;
    if filters.len() > 1000 {
        return Err("too many filters".to_string()).into();
    }
    for filter in &filters {
        if filter.trim().is_empty() || filter.len() > 128 {
            return Err("filter words must be non-empty and <= 128 chars".to_string()).into();
        }
    }
    super::db::with_db_result(|db| {
        SettingsRepo::new(db).set(
            "moderation_word_filters",
            &serde_json::to_string(&filters).unwrap_or_else(|_| "[]".to_string()),
        )?;
        Ok(true)
    })
}

/// Classify text using the lightweight AI moderation engine (Spam, CSAM, Gore, Bigotry, Harassment).
#[frb(sync, serialize)]
pub fn moderation_ai_classify_text(content: String) -> Result<String, String> {
    Ok(soshal_moderation_core::check::check_text_ai_json(&content))
}

/// Classify raw media bytes with the AI media perceptual and chrominance analyzer.
#[frb(sync, serialize)]
pub fn moderation_ai_classify_media(
    image_bytes: Vec<u8>,
    mime_type: String,
) -> Result<String, String> {
    Ok(soshal_moderation_core::media::check_media_buffer_ai_json(
        &image_bytes,
        &mime_type,
        &[],
    ))
}

/// 2-Tier Hybrid text evaluation (Tier 1 N-Gram -> Tier 2 RoBERTa).
#[frb(sync, serialize)]
pub fn moderation_hybrid_classify_text(
    content: String,
    force_deep_scan: bool,
) -> Result<String, String> {
    Ok(soshal_moderation_core::check::check_text_hybrid_json(
        &content,
        force_deep_scan,
    ))
}

/// Compute 256-bit Meta PDQ perceptual image hash and evaluate against threat blocklist.
#[frb(sync, serialize)]
pub fn moderation_compute_pdq_hash(image_bytes: Vec<u8>) -> Result<String, String> {
    match soshal_moderation_core::media::compute_image_pdq_hash(&image_bytes) {
        Some(res) => serde_json::to_string(&res).map_err(|e| format!("json encode error: {e}")),
        None => Err("failed to decode image or extract PDQ hash".to_string()),
    }
}

/// Create a FROST threshold jury case for community moderation.
///
/// NOTE: Real FROST multi-party threshold signing is a roadmap item.
/// This function returns an explicit error rather than running a
/// non-cryptographic simulation over FFI.
#[frb(sync, serialize)]
pub fn moderation_create_jury_case(
    _case_id: String,
    _target_pubkey: String,
    _reason: String,
    _threshold: u32,
    _total_jurors: u32,
    _group_pubkey: String,
) -> Result<String, String> {
    Err("FROST jury voting unavailable (roadmap)".to_string())
}

/// Submit a juror's partial signature vote to a moderation jury case.
///
/// NOTE: Real FROST multi-party threshold signing is a roadmap item.
/// This function returns an explicit error rather than running a
/// non-cryptographic simulation over FFI.
#[frb(sync, serialize)]
pub fn moderation_submit_jury_vote(
    _case_json: String,
    _vote_share_json: String,
) -> Result<String, String> {
    Err("FROST jury voting unavailable (roadmap)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_muted_list_key() {
        assert_eq!(muted_list_key(""), "muted_users");
        assert_eq!(muted_list_key("abc"), "abc:muted_users");
    }

    #[test]
    fn test_parse_list() {
        assert_eq!(
            parse_pubkey_list(r#"["a","b"]"#),
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(parse_pubkey_list("garbage").is_empty());
    }

    #[test]
    fn test_block_self_rejected() {
        let result = moderation_block_user("pk".to_string(), "pk".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_write_ops_require_unlocked_identity() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        super::super::signer::signer_lock().unwrap(); // deterministic baseline
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        // Locked signer: gate fails before any write.
        let err = moderation_mute_user(pk.clone(), "b".repeat(64)).unwrap_err();
        assert!(err.contains("signer locked"), "got {err}");
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let path = crate::ffi::db::tmp_db("mod", "idg");
        crate::ffi::db::insert_test_user(&pk);
        crate::ffi::db::insert_test_user(&"b".repeat(64));
        // Claimed identity != unlocked identity: rejected.
        let err = moderation_mute_user("f".repeat(64), "b".repeat(64)).unwrap_err();
        assert!(err.contains("identity mismatch"), "got {err}");
        // Matching identity: passes the gate and performs the write.
        assert!(
            moderation_mute_user(pk.clone(), "b".repeat(64)).is_ok(),
            "matching identity must pass the gate"
        );
        crate::ffi::db::reset_db_global();
        let _ = std::fs::remove_file(&path);
        super::super::signer::signer_lock().unwrap();
    }
}
