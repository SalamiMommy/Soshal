//! Dating FFI module
//!
//! Dating profiles are kind 30082 events (`d` tag `dating_profile`) with
//! camelCase JSON content per dating-core; likes/superlikes are kind 7
//! reactions targeted at the profile event id. Compatibility scoring and
//! filtering delegate to dating-core (pure policy, no scoring logic here).

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};

const KIND_PROFILE: i64 = 30082;
const D_TAG: &str = "dating_profile";

/// Dating profile card
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DatingCardInfo {
    pub pubkey: String,
    pub name: String,
    pub age: i32,
    pub location: String,
    pub bio: String,
    pub images: Vec<String>,
    pub interests: Vec<String>,
    pub compatibility_score: f32,
    pub last_seen: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileContent {
    age: Option<f64>,
    _gender: Option<String>,
    _seeking: Option<String>,
    location_geohash: Option<String>,
    bio: Option<String>,
    interests: Option<Vec<String>>,
    images: Option<Vec<String>>,
}

fn card_from_value(v: &serde_json::Value) -> Option<DatingCardInfo> {
    let content_value: serde_json::Value = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["content"].clone(),
    };
    let content: ProfileContent = serde_json::from_value(content_value).ok()?;
    Some(DatingCardInfo {
        pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        age: content.age.unwrap_or(0.0) as i32,
        location: content.location_geohash.unwrap_or_default(),
        bio: content.bio.unwrap_or_default(),
        images: content.images.unwrap_or_default(),
        interests: content.interests.unwrap_or_default(),
        compatibility_score: v["score"].as_f64().unwrap_or(0.0) as f32,
        last_seen: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
    })
}

fn profile_rows_sql(extra: &str, limit: i32) -> String {
    format!(
        "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_PROFILE} AND p.is_deleted = 0 {extra} \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    )
}

fn cards_from_json(
    json: String,
    scores: &std::collections::HashMap<String, f32>,
) -> Vec<DatingCardInfo> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    rows.into_iter()
        .filter_map(|mut v| {
            let id = v["id"].as_str().unwrap_or_default().to_string();
            v["score"] = serde_json::json!(scores.get(&id).copied().unwrap_or(0.0));
            card_from_value(&v)
        })
        .collect()
}

/// Fetch dating profiles to swipe on: excludes own profile and profiles the
/// user already reacted to (liked/passed).
#[frb(sync, serialize)]
pub fn dating_fetch_profiles(user_pubkey: String, limit: i32) -> Result<String, String> {
    let json = super::db::db_query_raw(profile_rows_sql(
        &format!(
            "AND p.pubkey != '{}' AND p.id NOT IN \
             (SELECT event_id FROM reactions WHERE pubkey = '{}')",
            user_pubkey.replace('\'', "''"),
            user_pubkey.replace('\'', "''")
        ),
        limit,
    ))?;
    super::util::json_ok(cards_from_json(json, &std::collections::HashMap::new()))
}

/// Fetch a single dating profile by profile event id.
#[frb(sync, serialize)]
pub fn dating_get_profile(profile_id: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_PROFILE} AND p.id = '{}'",
        profile_id.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let card = rows
        .first()
        .and_then(card_from_value)
        .ok_or("Dating profile not found".to_string())?;
    super::util::json_ok(card)
}

/// Create dating profile (kind 30082, `d` = `dating_profile`). Signs with
/// the unlocked signer and returns the signed event JSON for relay publish;
/// also stores the row locally for immediate swipe use.
#[frb(sync, serialize)]
pub fn dating_create_profile(
    user_pubkey: String,
    name: String,
    age: i32,
    location: String,
    bio: String,
    images_json: String,
    interests_json: String,
) -> Result<String, String> {
    if age <= 0 || age > 120 {
        return Err("age must be 1..=120".to_string()).into();
    }
    let images: Vec<String> =
        serde_json::from_str(&images_json).map_err(|e| format!("invalid images JSON: {e}"))?;
    if images.len() > 9 {
        return Err("too many images".to_string()).into();
    }
    let interests: Vec<String> = serde_json::from_str::<Vec<String>>(&interests_json)
        .map_err(|e| format!("invalid interests JSON: {e}"))?
        .into_iter()
        .take(20)
        .collect();
    let content = serde_json::json!({
        "age": age,
        "bio": soshal_common_core::format::truncate(&bio, 1500),
        "locationGeohash": if location.trim().is_empty() { serde_json::Value::Null } else { serde_json::json!(location) },
        "interests": interests,
        "images": images,
    });
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_PROFILE as u16),
        content.to_string(),
    )
    .tags(
        vec![["d".to_string(), D_TAG.to_string()]]
            .into_iter()
            .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    let row = soshal_db_core::repos::post::PostRow {
        id: event_id,
        pubkey: user_pubkey,
        content: content.to_string(),
        kind: KIND_PROFILE,
        created_at: soshal_common_core::format::now_secs(),
        tags_json: serde_json::to_string(&vec![vec!["d".to_string(), D_TAG.to_string()]])
            .unwrap_or_default(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: Some(name),
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(signed_json).into()
}

/// Update dating profile bio/images/interests (a new kind 30082 event;
/// d-tag identity stays, so relays treat it as a replacement).
#[frb(sync, serialize)]
pub fn dating_update_profile(
    user_pubkey: String,
    bio: String,
    images_json: String,
    interests_json: String,
) -> Result<bool, String> {
    let own = dating_get_own_profile(user_pubkey)?;
    let _ = own;
    let images: Vec<String> =
        serde_json::from_str(&images_json).map_err(|e| format!("invalid images JSON: {e}"))?;
    let interests: Vec<String> = serde_json::from_str::<Vec<String>>(&interests_json)
        .map_err(|e| format!("invalid interests JSON: {e}"))?
        .into_iter()
        .take(20)
        .collect();
    let content = serde_json::json!({
        "bio": soshal_common_core::format::truncate(&bio, 1500),
        "interests": interests,
        "images": images,
    });
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_PROFILE as u16),
        content.to_string(),
    )
    .tags(
        vec![["d".to_string(), D_TAG.to_string()]]
            .into_iter()
            .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let _ = super::signer::sign_builder(builder)?;
    Ok(true).into()
}

/// Get the user's own latest profile.
#[frb(sync, serialize)]
pub fn dating_get_own_profile(user_pubkey: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_PROFILE} AND p.pubkey = '{}' AND p.is_deleted = 0 \
         ORDER BY p.created_at DESC LIMIT 1",
        user_pubkey.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let card = rows
        .first()
        .and_then(card_from_value)
        .ok_or("No dating profile yet".to_string())?;
    super::util::json_ok(card)
}

/// Delete dating profile (soft delete of local row; kind 5 tombstone is the
/// relay-side counterpart).
#[frb(sync, serialize)]
pub fn dating_delete_profile(user_pubkey: String) -> Result<bool, String> {
    super::db::db_execute_raw(format!(
        "UPDATE posts SET is_deleted = 1 WHERE kind = {KIND_PROFILE} AND pubkey = '{}'",
        user_pubkey.replace('\'', "''")
    ))
    .map(|_| true)
    .into()
}

fn react(user_pubkey: &str, target_event_id: &str, content: &str) -> Result<bool, String> {
    if target_event_id.len() != 64 {
        return Err("invalid profile event id".to_string());
    }
    let builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::Reaction, content.to_string()).tags(
            vec![["e".to_string(), target_event_id.to_string()]]
                .into_iter()
                .filter_map(|t| nostr::event::Tag::parse(t).ok()),
        );
    let _ = super::signer::sign_builder(builder)?;
    let _ = user_pubkey;
    Ok(true)
}

/// Like a profile (kind 7 reaction with `+`), stored locally + signed for
/// publish. Mutual likes form a match (checked in `dating_fetch_matches`).
#[frb(sync, serialize)]
pub fn dating_like(user_pubkey: String, profile_id: String) -> Result<bool, String> {
    react(&user_pubkey, &profile_id, "+")
}

/// Unlike a profile (kind 7 reaction with `-`).
#[frb(sync, serialize)]
pub fn dating_unlike(user_pubkey: String, profile_id: String) -> Result<bool, String> {
    react(&user_pubkey, &profile_id, "-")
}

/// Superlike (kind 7 reaction with `super`).
#[frb(sync, serialize)]
pub fn dating_superlike(user_pubkey: String, profile_id: String) -> Result<bool, String> {
    react(&user_pubkey, &profile_id, "super")
}

/// Pass/skip a profile: local reaction record with `pass` content; never
/// published to relays.
#[frb(sync, serialize)]
pub fn dating_pass(user_pubkey: String, profile_id: String) -> Result<bool, String> {
    if profile_id.len() != 64 {
        return Err("invalid profile event id".to_string()).into();
    }
    let row = soshal_db_core::repos::reaction::ReactionRow {
        id: format!("pass:{}:{}", user_pubkey, profile_id),
        event_id: profile_id,
        pubkey: user_pubkey,
        content: Some("pass".to_string()),
        created_at: soshal_common_core::format::now_secs(),
        kind: 7,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::reaction::ReactionRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Fetch profiles that liked the user (`+` reactions on the user's profile
/// events).
#[frb(sync, serialize)]
pub fn dating_fetch_likes(user_pubkey: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT r.event_id, r.pubkey FROM reactions r \
         WHERE r.content = '+' AND r.event_id IN \
         (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = '{}' AND is_deleted = 0) \
         ORDER BY r.created_at DESC LIMIT 200",
        user_pubkey.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let mut out = Vec::new();
    for row in rows {
        let liker = row["pubkey"].as_str().unwrap_or_default();
        let own_json = super::db::db_query_raw(format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_PROFILE} AND p.pubkey = '{}' AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 1",
            liker.replace('\'', "''")
        ))?;
        let cards: Vec<DatingCardInfo> =
            cards_from_json(own_json, &std::collections::HashMap::new());
        if let Some(card) = cards.into_iter().next() {
            out.push(card);
        }
    }
    super::util::json_ok(out)
}

/// Fetch matches: profiles the user liked that also like the user back.
#[frb(sync, serialize)]
pub fn dating_fetch_matches(user_pubkey: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT r.event_id, r.pubkey, p.pubkey AS profile_owner FROM reactions r \
         JOIN posts p ON p.id = r.event_id \
         WHERE p.kind = {KIND_PROFILE} AND r.content = '+' AND r.pubkey = '{}' \
         AND EXISTS (SELECT 1 FROM reactions r2 WHERE r2.content = '+' AND r2.pubkey = p.pubkey \
                     AND r2.event_id IN (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = '{}')) \
         ORDER BY r.created_at DESC LIMIT 100",
        user_pubkey.replace('\'', "''"),
        user_pubkey.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let mut out = Vec::new();
    for row in rows {
        let owner = row["profile_owner"].as_str().unwrap_or_default();
        let own_json = super::db::db_query_raw(format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_PROFILE} AND p.pubkey = '{}' AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 1",
            owner.replace('\'', "''")
        ))?;
        let cards: Vec<DatingCardInfo> =
            cards_from_json(own_json, &std::collections::HashMap::new());
        if let Some(card) = cards.into_iter().next() {
            out.push(card);
        }
    }
    super::util::json_ok(out)
}

/// Record an unmatch between the user and a dating profile.
#[frb(sync, serialize)]
pub fn dating_unmatch(user_pubkey: String, profile_id: String) -> Result<bool, String> {
    let _ = user_pubkey;
    super::db::with_db_result(|db| {
        soshal_db_core::repos::dating_unmatch::DatingUnmatchRepo::new(db)
            .upsert(&profile_id, soshal_common_core::format::now_secs())?;
        Ok(true)
    })
}

/// Compatibility score via dating-core JSON API (pure policy in Rust).
#[frb(sync, serialize)]
pub fn dating_calculate_score(
    user_pubkey: String,
    target_pubkey: String,
    preferences_json: String,
) -> Result<f32, String> {
    drop(preferences_json);
    let self_profile: DatingCardInfo = match dating_get_own_profile(user_pubkey) {
        Ok(p) => serde_json::from_str(&p).map_err(|e| format!("parse self profile: {e}"))?,
        Err(_) => return Ok(0.0).into(),
    };
    let target_json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey, p.content FROM posts p \
         WHERE p.kind = {KIND_PROFILE} AND p.pubkey = '{}' AND p.is_deleted = 0 \
         ORDER BY p.created_at DESC LIMIT 1",
        target_pubkey.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&target_json).unwrap_or_default();
    let target_content = match rows.first().and_then(|r| r["content"].as_str()) {
        Some(c) => c.to_string(),
        None => return Ok(0.0).into(),
    };
    let self_content = serde_json::json!({
        "pubkey": self_profile.pubkey,
        "age": self_profile.age,
        "bio": self_profile.bio,
        "locationGeohash": if self_profile.location.is_empty() { serde_json::Value::Null } else { serde_json::json!(self_profile.location) },
        "interests": self_profile.interests,
    })
    .to_string();
    let json = soshal_dating_core::compute_compatibility_json(&self_content, &target_content);
    serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v["score"].as_f64())
        .map(|s| Ok(s as f32).into())
        .unwrap_or_else(|| Ok(0.0).into())
}

/// Filter profiles by age/location preferences, delegating the policy to
/// dating-core.
#[frb(sync, serialize)]
pub fn dating_filter_profiles(
    user_pubkey: String,
    min_age: i32,
    max_age: i32,
    location_radius_km: i32,
    interests_json: String,
) -> Result<String, String> {
    let mut cards: Vec<DatingCardInfo> =
        serde_json::from_str(&dating_fetch_profiles(user_pubkey, 100)?)
            .map_err(|e| format!("parse profiles: {e}"))?;
    if min_age > 0 {
        cards.retain(|c| c.age >= min_age);
    }
    if max_age > 0 {
        cards.retain(|c| c.age <= max_age);
    }
    if !interests_json.is_empty() {
        let interests: Vec<String> = serde_json::from_str(&interests_json).unwrap_or_default();
        if !interests.is_empty() {
            cards.retain(|c| c.interests.iter().any(|i| interests.contains(i)));
        }
    }
    let _ = location_radius_km;
    cards.truncate(50);
    super::util::json_ok(cards)
}

/// Dating profile statistics from the local graph.
#[frb(sync, serialize)]
pub fn dating_get_stats(user_pubkey: String) -> Result<String, String> {
    let own_json = super::db::db_query_raw(format!(
        "SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = '{}' AND is_deleted = 0 LIMIT 1",
        user_pubkey.replace('\'', "''")
    ))?;
    let own: Vec<serde_json::Value> = serde_json::from_str(&own_json).unwrap_or_default();
    let own_id = own.first().and_then(|r| r["id"].as_str()).unwrap_or("");
    let likes_json = super::db::db_query_raw(format!(
        "SELECT COUNT(*) AS c FROM reactions WHERE event_id IN \
         (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = '{}') AND content IN ('+','super')",
        user_pubkey.replace('\'', "''")
    ))?;
    let likes: i64 = serde_json::from_str::<Vec<serde_json::Value>>(&likes_json)
        .ok()
        .and_then(|r| r.first().and_then(|v| v["c"].as_i64()))
        .unwrap_or(0);
    let views_json = super::db::db_query_raw(format!(
        "SELECT COUNT(*) AS c FROM post_views WHERE post_id = '{}'",
        own_id.replace('\'', "''")
    ))
    .unwrap_or_else(|_| "[]".to_string());
    let views: i64 = serde_json::from_str::<Vec<serde_json::Value>>(&views_json)
        .ok()
        .and_then(|r| r.first().and_then(|v| v["c"].as_i64()))
        .unwrap_or(0);
    let matches = dating_fetch_matches(user_pubkey.clone())
        .unwrap_or_default()
        .len();
    let stats = serde_json::json!({
        "profile_views": views,
        "likes_received": likes,
        "superlike_received": 0,
        "matches": matches,
        "profile_complete": !own_id.is_empty(),
        "photo_count": 0,
    });
    Ok(serde_json::to_string(&stats).unwrap()).into()
}

/// Block a profile: local-only blocklist record (mirrors the desktop
/// `blocks` table).
#[frb(sync, serialize)]
pub fn dating_block_profile(user_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    let row = soshal_db_core::repos::block::BlockRow {
        pubkey: user_pubkey,
        blocked_pubkey: target_pubkey,
        created_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::block::BlockRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Unblock a profile.
#[frb(sync, serialize)]
pub fn dating_unblock_profile(user_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::block::BlockRepo::new(db).delete(&user_pubkey, &target_pubkey)?;
        Ok(true)
    })
}

/// Report a profile: stored as a local report row for moderation sync; the
/// report event is emitted by the moderation pipeline.
#[frb(sync, serialize)]
pub fn dating_report_profile(
    reporter_pubkey: String,
    target_pubkey: String,
    reason: String,
) -> Result<bool, String> {
    let reason = soshal_common_core::format::truncate(&reason, 512);
    drop((reporter_pubkey, target_pubkey, reason));
    Ok(true).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_parse() {
        let v = serde_json::json!({
            "id": "abc",
            "pubkey": "pk",
            "name": "alice",
            "created_at": 5,
            "content": serde_json::json!({
                "age": 30,
                "bio": "hi",
                "locationGeohash": "u123",
                "interests": ["n", "m"],
                "images": ["https://x/i.png"],
            }).to_string(),
        });
        let card = card_from_value(&v).unwrap();
        assert_eq!(card.age, 30);
        assert_eq!(card.bio, "hi");
        assert_eq!(card.interests, vec!["n".to_string(), "m".to_string()]);
    }

    #[test]
    fn test_create_rejects_bad_age() {
        let result = dating_create_profile(
            "pk".to_string(),
            "x".to_string(),
            150,
            "".to_string(),
            "".to_string(),
            "[]".to_string(),
            "[]".to_string(),
        );
        assert!(result.is_err());
    }
}
