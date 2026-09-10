//! Dating FFI module
//!
//! Dating profiles are kind 30082 events (`d` tag `dating_profile`) with
//! camelCase JSON content per dating-core; likes/superlikes are kind 7
//! reactions targeted at the profile event id. Compatibility scoring and
//! filtering delegate to dating-core (pure policy, no scoring logic here).

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_common_core::consts::KIND_PROFILE;

const D_TAG: &str = "dating_profile";

/// Dating profile card
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DatingCardInfo {
    pub pubkey: String,
    pub name: String,
    pub age: i32,
    pub location: String,
    pub gender: String,
    pub seeking: String,
    pub height: f32,
    pub body_type: String,
    pub smoking: String,
    pub drinking: String,
    pub relationship_intent: String,
    pub politics: String,
    pub ethnicity: String,
    pub education: String,
    pub language: Vec<String>,
    pub max_distance_km: f32,
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
    gender: Option<String>,
    seeking: Option<String>,
    height: Option<f64>,
    body_type: Option<String>,
    smoking: Option<String>,
    drinking: Option<String>,
    relationship_intent: Option<String>,
    politics: Option<String>,
    ethnicity: Option<String>,
    education: Option<String>,
    language: Option<Vec<String>>,
    max_distance_km: Option<f64>,
    location_geohash: Option<String>,
    bio: Option<String>,
    interests: Option<Vec<String>>,
    images: Option<Vec<String>>,
}

fn card_from_value(v: &serde_json::Value) -> Option<DatingCardInfo> {
    let content: ProfileContent = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).ok()?,
        None => serde_json::from_value(v["content"].clone()).ok()?,
    };
    Some(DatingCardInfo {
        pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        age: content.age.unwrap_or(0.0) as i32,
        location: content
            .location_geohash
            .unwrap_or_default()
            .chars()
            .take(5)
            .collect(),
        gender: content.gender.unwrap_or_default(),
        seeking: content.seeking.unwrap_or_default(),
        height: content.height.unwrap_or(0.0) as f32,
        body_type: content.body_type.unwrap_or_default(),
        smoking: content.smoking.unwrap_or_default(),
        drinking: content.drinking.unwrap_or_default(),
        relationship_intent: content.relationship_intent.unwrap_or_default(),
        politics: content.politics.unwrap_or_default(),
        ethnicity: content.ethnicity.unwrap_or_default(),
        education: content.education.unwrap_or_default(),
        language: content.language.unwrap_or_default(),
        max_distance_km: content.max_distance_km.unwrap_or(0.0) as f32,
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
         AND p.created_at = (SELECT MAX(created_at) FROM posts p2 \
                             WHERE p2.pubkey = p.pubkey AND p2.kind = p.kind AND p2.is_deleted = 0) \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    )
}

const GENDERS: [&str; 4] = ["male", "female", "non-binary", "other"];
const SEEKING: [&str; 5] = ["male", "female", "non-binary", "other", "All"];
const BODY_TYPES: [&str; 5] = ["slim", "athletic", "average", "curvy", "muscular"];
const SMOKING: [&str; 3] = ["never", "occasionally", "regularly"];
const DRINKING: [&str; 3] = ["never", "socially", "regularly"];
const RELATIONSHIP_INTENTS: [&str; 3] = ["serious", "casual", "still figuring out"];
const POLITICS: [&str; 6] = [
    "prefer not to say",
    "liberal",
    "moderate",
    "conservative",
    "libertarian",
    "other",
];
const EDUCATION: [&str; 7] = [
    "high school",
    "some college",
    "associate",
    "trade school",
    "bachelor's",
    "master's",
    "doctorate",
];

/// Trimmed string, or `None` when empty.
fn opt_str(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn validate_enum(field: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if !value.trim().is_empty() && !allowed.contains(&value.trim()) {
        return Err(format!("invalid {field}: {value}"));
    }
    Ok(())
}

/// Resolves the location input: either `lat,lon` (encoded to a geohash at
/// precision 5) or a raw geohash string. Empty/invalid input is rejected —
/// dating profiles require a geohash.
fn resolve_location(location: &str) -> Result<String, String> {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return Err("location is required".to_string());
    }
    if let Some((lat, lon)) = trimmed.split_once(',') {
        let lat: f64 = lat
            .trim()
            .parse()
            .map_err(|_| format!("invalid latitude: {}", lat.trim()))?;
        let lon: f64 = lon
            .trim()
            .parse()
            .map_err(|_| format!("invalid longitude: {}", lon.trim()))?;
        return soshal_spatial_core::geohash::encode_geohash(lat, lon, 5)
            .ok_or_else(|| "invalid coordinates (lat -90..=90, lon -180..=180)".to_string());
    }
    if !soshal_spatial_core::geohash::is_valid_geohash(trimmed) {
        return Err(format!("invalid geohash: {trimmed}"));
    }
    Ok(trimmed.to_string())
}

/// Validates the full attribute set; shared by create and update.
#[allow(clippy::too_many_arguments)]
fn validate_attributes(
    gender: &str,
    seeking: &str,
    height_cm: i32,
    body_type: &str,
    smoking: &str,
    drinking: &str,
    relationship_intent: &str,
    politics: &str,
    education: &str,
    max_distance_km: i32,
) -> Result<(), String> {
    validate_enum("gender", gender, &GENDERS)?;
    validate_enum("seeking", seeking, &SEEKING)?;
    if height_cm > 0 && !(100..=250).contains(&height_cm) {
        return Err(format!("height must be 100..=250 cm, got {height_cm}"));
    }
    validate_enum("bodyType", body_type, &BODY_TYPES)?;
    validate_enum("smoking", smoking, &SMOKING)?;
    validate_enum("drinking", drinking, &DRINKING)?;
    validate_enum(
        "relationshipIntent",
        relationship_intent,
        &RELATIONSHIP_INTENTS,
    )?;
    validate_enum("politics", politics, &POLITICS)?;
    validate_enum("education", education, &EDUCATION)?;
    if !(0..=500).contains(&max_distance_km) {
        return Err(format!(
            "maxDistanceKm must be 0..=500, got {max_distance_km}"
        ));
    }
    Ok(())
}

/// Renders the attribute fields into content JSON (camelCase keys matching
/// dating-core). Empty strings / 0 values render as null (unset).
#[allow(clippy::too_many_arguments)]
fn attribute_content(
    location: &str,
    gender: &str,
    seeking: &str,
    height_cm: i32,
    body_type: &str,
    smoking: &str,
    drinking: &str,
    relationship_intent: &str,
    politics: &str,
    ethnicity: &str,
    education: &str,
    language: Vec<String>,
    max_distance_km: i32,
    bio: &str,
    interests: Vec<String>,
    images: Vec<String>,
) -> serde_json::Value {
    serde_json::json!({
        "locationGeohash": location,
        "gender": opt_str(gender),
        "seeking": opt_str(seeking),
        "height": (height_cm > 0).then_some(f64::from(height_cm)),
        "bodyType": opt_str(body_type),
        "smoking": opt_str(smoking),
        "drinking": opt_str(drinking),
        "relationshipIntent": opt_str(relationship_intent),
        "politics": opt_str(politics),
        "ethnicity": opt_str(ethnicity),
        "education": opt_str(education),
        "language": (!language.is_empty()).then_some(language),
        "maxDistanceKm": (max_distance_km > 0).then_some(f64::from(max_distance_km)),
        "bio": soshal_common_core::format::truncate(bio, 1500),
        "interests": interests,
        "images": images,
    })
}

fn cards_from_values(
    rows: Vec<serde_json::Value>,
    scores: &std::collections::HashMap<String, f32>,
) -> Vec<DatingCardInfo> {
    rows.into_iter()
        .filter_map(|mut v| {
            let id = v["id"].as_str().unwrap_or_default().to_string();
            v["score"] = serde_json::json!(scores.get(&id).copied().unwrap_or(0.0));
            card_from_value(&v)
        })
        .collect()
}

fn cards_by_pubkey(
    pubkeys: &[String],
    viewer: &str,
    limit: i32,
) -> Result<std::collections::HashMap<String, DatingCardInfo>, String> {
    if pubkeys.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let ids_json = serde_json::to_string(
        &pubkeys
            .iter()
            .take(limit as usize)
            .cloned()
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".into());
    let rows = super::db::db_query_json(
        &format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_PROFILE} AND p.pubkey IN (SELECT value FROM json_each(?1)) AND p.is_deleted = 0 \
             AND p.pubkey NOT IN (SELECT blocked_pubkey FROM blocks WHERE pubkey = ?2) \
             ORDER BY p.created_at DESC"
        ),
        &[ids_json, viewer.to_string()],
    )?;
    let mut map = std::collections::HashMap::new();
    for card in cards_from_values(rows, &std::collections::HashMap::new()) {
        map.entry(card.pubkey.clone()).or_insert(card);
    }
    Ok(map)
}

/// Default suggestion weights: shared interests dominate, proximity boosts.
fn suggestion_weights() -> soshal_dating_core::PreferenceWeights {
    soshal_dating_core::PreferenceWeights {
        age: None,
        height: None,
        body_type: None,
        interests: Some(2.0),
        smoking: None,
        drinking: None,
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        relationship_intent: None,
        distance: Some(1.5),
    }
}

/// Card → scoring input (shared with `dating_filter_profiles`).
fn profile_input_from_card(c: &DatingCardInfo) -> soshal_dating_core::DatingProfileInput {
    soshal_dating_core::DatingProfileInput {
        pubkey: c.pubkey.clone(),
        age: Some(f64::from(c.age)),
        gender: (!c.gender.is_empty()).then(|| c.gender.clone()),
        seeking: (!c.seeking.is_empty()).then(|| c.seeking.clone()),
        height: (c.height > 0.0).then(|| f64::from(c.height)),
        body_type: (!c.body_type.is_empty()).then(|| c.body_type.clone()),
        smoking: (!c.smoking.is_empty()).then(|| c.smoking.clone()),
        drinking: (!c.drinking.is_empty()).then(|| c.drinking.clone()),
        relationship_intent: (!c.relationship_intent.is_empty())
            .then(|| c.relationship_intent.clone()),
        politics: (!c.politics.is_empty()).then(|| c.politics.clone()),
        ethnicity: (!c.ethnicity.is_empty()).then(|| c.ethnicity.clone()),
        education: (!c.education.is_empty()).then(|| c.education.clone()),
        language: (!c.language.is_empty()).then(|| c.language.clone()),
        max_distance_km: (c.max_distance_km > 0.0).then(|| f64::from(c.max_distance_km)),
        location_geohash: (!c.location.is_empty()).then(|| c.location.clone()),
        interests: Some(c.interests.clone()),
        ..Default::default()
    }
}

/// Self profile as scoring input: own page content JSON + suggestion weights.
fn self_profile_input(user_pubkey: &str) -> soshal_dating_core::DatingProfileInput {
    let weights = suggestion_weights();
    let fallback = || soshal_dating_core::DatingProfileInput {
        pubkey: user_pubkey.to_string(),
        preference_weights: Some(weights.clone()),
        ..Default::default()
    };
    let rows = match super::db::db_query_json(
        &format!(
            "SELECT content FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1 AND is_deleted = 0 \
             ORDER BY created_at DESC LIMIT 1"
        ),
        &[user_pubkey.to_string()],
    ) {
        Ok(j) => j,
        Err(_) => return fallback(),
    };
    let Some(content) = rows.first().and_then(|r| r["content"].as_str()) else {
        return fallback();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
        return fallback();
    };
    let str_opt = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let stored_weights = v.get("preferenceWeights").and_then(|w| {
        serde_json::from_value::<soshal_dating_core::PreferenceWeights>(w.clone()).ok()
    });
    let dealbreakers = v.get("dealbreakers").and_then(|d| d.as_array()).map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    });
    soshal_dating_core::DatingProfileInput {
        pubkey: user_pubkey.to_string(),
        age: v.get("age").and_then(|a| a.as_f64()),
        gender: str_opt("gender"),
        seeking: str_opt("seeking"),
        location_geohash: str_opt("locationGeohash"),
        max_distance_km: v.get("maxDistanceKm").and_then(|m| m.as_f64()),
        interests: v.get("interests").and_then(|i| i.as_array()).map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        }),
        preference_weights: Some(stored_weights.unwrap_or(weights)),
        dealbreakers,
        ..Default::default()
    }
}

/// Rank candidate cards by shared interests + proximity (distance-aware mutual
/// compatibility) and stamp each card's % badge with the blend score.
fn rank_cards_by_interest_distance(
    user_pubkey: &str,
    mut cards: Vec<DatingCardInfo>,
) -> Vec<DatingCardInfo> {
    if cards.is_empty() {
        return cards;
    }
    let profiles: Vec<soshal_dating_core::DatingProfileInput> =
        cards.iter().map(profile_input_from_card).collect();
    let sorted =
        soshal_dating_core::sort::sort_dating_profiles(soshal_dating_core::SortProfilesInput {
            profiles,
            self_profile: self_profile_input(user_pubkey),
            self_contacts: Vec::new(),
            sort_by: None,
        });
    let rank_map: std::collections::HashMap<&str, (usize, f32)> = sorted
        .iter()
        .enumerate()
        .map(|(rank, s)| (s.pubkey.as_str(), (rank, s.compatibility_score as f32)))
        .collect();
    for card in &mut cards {
        if let Some((_, score)) = rank_map.get(card.pubkey.as_str()) {
            card.compatibility_score = *score;
        }
    }
    cards.sort_by_key(|c| {
        rank_map
            .get(c.pubkey.as_str())
            .map(|(r, _)| *r)
            .unwrap_or(usize::MAX)
    });
    cards
}

/// Fetch dating profiles to swipe on: excludes own profile and profiles the
/// user already reacted to (liked/passed) or blocked. Ranked by shared
/// interests + proximity (see [rank_cards_by_interest_distance]).
#[frb(sync, serialize)]
fn fetch_profiles_internal(
    user_pubkey: &str,
    limit: i32,
    audience: &str,
) -> Result<Vec<DatingCardInfo>, String> {
    let authors = super::identity::resolve_audience_authors(audience)?;
    let mut params: Vec<String> = vec![user_pubkey.to_string()];
    let mut audience_clause = String::new();
    if let Some(a) = &authors {
        if a.is_empty() {
            return Ok(Vec::new());
        }
        if a.len() == 1 {
            audience_clause = " AND p.pubkey = ?2".to_string();
            params.push(a[0].clone());
        } else {
            audience_clause = " AND p.pubkey IN (SELECT value FROM json_each(?2))".to_string();
            params.push(serde_json::to_string(a).map_err(|e| format!("authors: {e}"))?);
        }
    }
    let rows = super::db::db_query_json(
        &profile_rows_sql(
            &format!(
                "AND p.pubkey != ?1 AND p.id NOT IN \
                 (SELECT event_id FROM reactions WHERE pubkey = ?1) \
                 AND p.pubkey NOT IN (SELECT pubkey FROM dating_unmatches WHERE actor_pubkey = ?1) \
                 AND p.pubkey NOT IN (SELECT blocked_pubkey FROM blocks WHERE pubkey = ?1){audience_clause}",
            ),
            limit,
        ),
        &params,
    )?;
    Ok(rank_cards_by_interest_distance(
        user_pubkey,
        rows.into_iter()
            .filter_map(|v| card_from_value(&v))
            .collect(),
    ))
}

#[frb(sync, serialize)]
pub fn dating_fetch_profiles(
    user_pubkey: String,
    limit: i32,
    audience: String,
) -> Result<String, String> {
    let cards = fetch_profiles_internal(&user_pubkey, limit, &audience)?;
    super::util::json_ok(cards)
}

/// Fetch a single dating profile by profile event id.
#[frb(sync, serialize)]
pub fn dating_get_profile(profile_id: String) -> Result<String, String> {
    let rows = super::db::db_query_json(
        &format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_PROFILE} AND p.id = ?1"
        ),
        &[profile_id],
    )?;
    let card = rows
        .first()
        .and_then(card_from_value)
        .ok_or("Dating profile not found".to_string())?;
    super::util::json_ok(card)
}

/// Create dating profile (kind 30082, `d` = `dating_profile`). Requires a
/// geohash (raw geohash or `lat,lon`); validates the full attribute set.
/// Signs with the unlocked signer and returns the signed event JSON for
/// relay publish; also stores the row locally for immediate swipe use.
#[frb(sync, serialize)]
#[allow(clippy::too_many_arguments)]
pub fn dating_create_profile(
    user_pubkey: String,
    name: String,
    age: i32,
    location: String,
    gender: String,
    seeking: String,
    height_cm: i32,
    body_type: String,
    smoking: String,
    drinking: String,
    relationship_intent: String,
    politics: String,
    ethnicity: String,
    education: String,
    language_json: String,
    max_distance_km: i32,
    bio: String,
    images_json: String,
    interests_json: String,
) -> Result<String, String> {
    super::signer::require_identity(&user_pubkey)?;
    if age <= 0 || age > 120 {
        return Err("age must be 1..=120".to_string()).into();
    }
    validate_attributes(
        &gender,
        &seeking,
        height_cm,
        &body_type,
        &smoking,
        &drinking,
        &relationship_intent,
        &politics,
        &education,
        max_distance_km,
    )?;
    let geohash = resolve_location(&location)?;
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
    let language: Vec<String> = serde_json::from_str::<Vec<String>>(&language_json)
        .map_err(|e| format!("invalid language JSON: {e}"))?
        .into_iter()
        .take(10)
        .collect();
    let mut content = attribute_content(
        &geohash,
        &gender,
        &seeking,
        height_cm,
        &body_type,
        &smoking,
        &drinking,
        &relationship_intent,
        &politics,
        &ethnicity,
        &education,
        language,
        max_distance_km,
        &bio,
        interests,
        images,
    );
    content["age"] = serde_json::json!(age);
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_PROFILE),
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
    super::db::upsert_post_row(
        event_id.clone(),
        user_pubkey.clone(),
        content.to_string(),
        KIND_PROFILE as i64,
        soshal_common_core::format::now_secs(),
        serde_json::to_string(&vec![vec!["d".to_string(), D_TAG.to_string()]]).unwrap_or_default(),
        Some(name),
    )?;
    super::db::db_execute_params(
        &format!(
            "UPDATE posts SET is_deleted = 1 WHERE kind = {KIND_PROFILE} AND pubkey = ?1 AND id != ?2"
        ),
        &[user_pubkey, event_id],
    )?;
    Ok(signed_json).into()
}

/// Update dating profile (a new kind 30082 event; d-tag identity stays, so
/// relays treat it as a replacement). The full attribute set is written;
/// form fields always come from the arguments, while non-form fields in the
/// previous content (weights, dealbreakers, friends) are preserved.
#[frb(sync, serialize)]
#[allow(clippy::too_many_arguments)]
pub fn dating_update_profile(
    user_pubkey: String,
    location: String,
    gender: String,
    seeking: String,
    height_cm: i32,
    body_type: String,
    smoking: String,
    drinking: String,
    relationship_intent: String,
    politics: String,
    ethnicity: String,
    education: String,
    language_json: String,
    max_distance_km: i32,
    bio: String,
    images_json: String,
    interests_json: String,
) -> Result<bool, String> {
    validate_attributes(
        &gender,
        &seeking,
        height_cm,
        &body_type,
        &smoking,
        &drinking,
        &relationship_intent,
        &politics,
        &education,
        max_distance_km,
    )?;
    let geohash = resolve_location(&location)?;
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
    let language: Vec<String> = serde_json::from_str::<Vec<String>>(&language_json)
        .map_err(|e| format!("invalid language JSON: {e}"))?
        .into_iter()
        .take(10)
        .collect();
    let mut content = attribute_content(
        &geohash,
        &gender,
        &seeking,
        height_cm,
        &body_type,
        &smoking,
        &drinking,
        &relationship_intent,
        &politics,
        &ethnicity,
        &education,
        language,
        max_distance_km,
        &bio,
        interests,
        images,
    );
    let old_json = super::db::db_query_params(
        &format!(
            "SELECT content FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1 AND is_deleted = 0 \
             ORDER BY created_at DESC LIMIT 1"
        ),
        &[user_pubkey.clone()],
    )?;
    let old: Vec<serde_json::Value> = serde_json::from_str(&old_json).unwrap_or_default();
    if let Some(prev) = old
        .first()
        .and_then(|r| r["content"].as_str())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
    {
        if let Some(age) = prev.get("age").cloned() {
            content["age"] = age;
        }
        for key in ["preferenceWeights", "dealbreakers", "verifiedMutualFriends"] {
            if let Some(v) = prev.get(key).cloned() {
                content[key] = v;
            }
        }
    }
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_PROFILE),
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
    super::db::upsert_post_row(
        event_id.clone(),
        user_pubkey.clone(),
        content.to_string(),
        KIND_PROFILE as i64,
        soshal_common_core::format::now_secs(),
        serde_json::to_string(&vec![vec!["d".to_string(), D_TAG.to_string()]]).unwrap_or_default(),
        None,
    )?;
    super::db::db_execute_params(
        &format!(
            "UPDATE posts SET is_deleted = 1 WHERE kind = {KIND_PROFILE} AND pubkey = ?1 AND id != ?2"
        ),
        &[user_pubkey, event_id],
    )?;
    Ok(true).into()
}

/// Get the user's own latest profile.
#[frb(sync, serialize)]
pub fn dating_get_own_profile(user_pubkey: String) -> Result<String, String> {
    let rows = super::db::db_query_json(
        &format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, COALESCE(u.name,'') AS name \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_PROFILE} AND p.pubkey = ?1 AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 1"
        ),
        &[user_pubkey],
    )?;
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
    super::db::db_execute_params(
        &format!("UPDATE posts SET is_deleted = 1 WHERE kind = {KIND_PROFILE} AND pubkey = ?1"),
        &[user_pubkey],
    )
    .map(|_| true)
    .into()
}

fn react(user_pubkey: &str, profile_pubkey: &str, content: &str) -> Result<bool, String> {
    super::signer::require_identity(user_pubkey)?;
    if profile_pubkey.len() != 64 {
        return Err("invalid profile event id".to_string());
    }
    // The like/matches/stats reads join `posts p ON p.id = r.event_id`, so the
    // stored event_id must be the profile *event id*, not the author pubkey.
    // Resolve the author's newest non-deleted profile event; fail loudly if
    // none exists rather than writing a reaction that can never surface.
    let rows = super::db::db_query_json(
        &format!(
            "SELECT p.id FROM posts p WHERE p.kind = {KIND_PROFILE} \
             AND p.pubkey = ?1 AND p.is_deleted = 0 ORDER BY p.created_at DESC LIMIT 1"
        ),
        &[profile_pubkey.to_string()],
    )?;
    let profile_event_id = rows
        .first()
        .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
        .ok_or_else(|| format!("no profile event for {profile_pubkey}"))?;
    let builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::Reaction, content.to_string()).tags(
            vec![["e".to_string(), profile_event_id.clone()]]
                .into_iter()
                .filter_map(|t| nostr::event::Tag::parse(t).ok()),
        );
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::reaction::ReactionRow {
        id: format!("reaction:{user_pubkey}:{profile_pubkey}"),
        event_id: profile_event_id,
        pubkey: user_pubkey.to_string(),
        content: Some(content.to_string()),
        created_at: now,
        kind: 7,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::reaction::ReactionRepo::new(db).upsert(&row)?;
        soshal_sync_core::outbox::enqueue_outbox_item_with_seal(
            db,
            &event_id,
            "reaction",
            &signed_json,
            None,
            now,
            super::sync::outbox_seal_fn(),
        )
        .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(true)
    })
}

/// Like a profile (kind 7 reaction with `+`), stored locally + queued for
/// relay via outbox. Mutual likes form a match (checked in `dating_fetch_matches`).
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
    super::signer::require_identity(&user_pubkey)?;
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

/// Reset all profiles the user swiped "no" on (deletes local `pass` records
/// so the profiles re-enter the discover deck). Likes/matches/unmatches are
/// untouched. Returns the number of passes reset.
#[frb(sync, serialize)]
pub fn dating_reset_passes(user_pubkey: String) -> Result<i64, String> {
    let n = super::db::db_execute_params(
        "DELETE FROM reactions WHERE pubkey = ?1 AND content = 'pass'",
        &[user_pubkey],
    )?;
    Ok(n as i64).into()
}

/// Fetch profiles that liked the user (`+` reactions on the user's profile
/// events).
#[frb(sync, serialize)]
pub fn dating_fetch_likes(user_pubkey: String) -> Result<String, String> {
    let rows = super::db::db_query_json(
        &format!(
            "SELECT r.event_id, r.pubkey FROM reactions r \
             WHERE r.content = '+' AND r.event_id IN \
             (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1 AND is_deleted = 0) \
             ORDER BY r.created_at DESC LIMIT 200"
        ),
        &[user_pubkey.clone()],
    )?;
    let mut likers: Vec<String> = Vec::new();
    let mut seen_likers = std::collections::HashSet::new();
    for row in &rows {
        let pk = row["pubkey"].as_str().unwrap_or_default();
        if seen_likers.insert(pk) {
            likers.push(pk.to_string());
        }
    }
    let cards = cards_by_pubkey(&likers, &user_pubkey, 200)?;
    let mut out = Vec::new();
    for row in rows {
        if let Some(card) = cards.get(row["pubkey"].as_str().unwrap_or_default()) {
            out.push(card.clone());
        }
    }
    super::util::json_ok(out)
}

/// Fetch matches: profiles the user liked that also like the user back.
#[frb(sync, serialize)]
pub fn dating_fetch_matches(user_pubkey: String) -> Result<String, String> {
    let rows = super::db::db_query_json(
        &format!(
            "SELECT r.event_id, r.pubkey, p.pubkey AS profile_owner FROM reactions r \
             JOIN posts p ON p.id = r.event_id \
             WHERE p.kind = {KIND_PROFILE} AND r.content = '+' AND r.pubkey = ?1 \
             AND EXISTS (SELECT 1 FROM reactions r2 WHERE r2.content = '+' AND r2.pubkey = p.pubkey \
                         AND r2.event_id IN (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1)) \
             ORDER BY r.created_at DESC LIMIT 100"
        ),
        &[user_pubkey.clone()],
    )?;
    let mut owners: Vec<String> = Vec::new();
    let mut seen_owners = std::collections::HashSet::new();
    for row in &rows {
        let pk = row["profile_owner"].as_str().unwrap_or_default();
        if seen_owners.insert(pk) {
            owners.push(pk.to_string());
        }
    }
    let cards = cards_by_pubkey(&owners, &user_pubkey, 100)?;
    let mut out = Vec::new();
    for row in rows {
        if let Some(card) = cards.get(row["profile_owner"].as_str().unwrap_or_default()) {
            out.push(card.clone());
        }
    }
    super::util::json_ok(out)
}

/// Record an unmatch between the user and a dating profile.
///
/// Scoped to `user_pubkey`: the suppression entry is per-actor, so an
/// unmatch under one account never hides the profile from another account
/// swiping on the same device.
#[frb(sync, serialize)]
pub fn dating_unmatch(user_pubkey: String, profile_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::dating_unmatch::DatingUnmatchRepo::new(db).upsert(
            &user_pubkey,
            &profile_id,
            soshal_common_core::format::now_secs(),
        )?;
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
    let prefs: serde_json::Value =
        serde_json::from_str(&preferences_json).unwrap_or(serde_json::Value::Null);
    let profile_rows = |pubkey: &str| {
        super::db::db_query_params(
            &format!(
                "SELECT p.id, p.pubkey, p.content FROM posts p \
                 WHERE p.kind = {KIND_PROFILE} AND p.pubkey = ?1 AND p.is_deleted = 0 \
                 ORDER BY p.created_at DESC LIMIT 1"
            ),
            &[pubkey.to_string()],
        )
    };
    let self_rows: Vec<serde_json::Value> =
        serde_json::from_str(&profile_rows(&user_pubkey)?).unwrap_or_default();
    let self_content_raw = match self_rows.first().and_then(|r| r["content"].as_str()) {
        Some(c) => c.to_string(),
        None => return Ok(0.0).into(),
    };
    let target_rows: Vec<serde_json::Value> =
        serde_json::from_str(&profile_rows(&target_pubkey)?).unwrap_or_default();
    let target_content = match target_rows.first().and_then(|r| r["content"].as_str()) {
        Some(c) => c.to_string(),
        None => return Ok(0.0).into(),
    };
    let mut self_obj: serde_json::Value =
        serde_json::from_str(&self_content_raw).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(w) = prefs.get("preferenceWeights") {
        self_obj["preferenceWeights"] = w.clone();
    }
    if let Some(d) = prefs.get("dealbreakers") {
        self_obj["dealbreakers"] = d.clone();
    }
    let json =
        soshal_dating_core::compute_compatibility_json(&self_obj.to_string(), &target_content);
    serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v.as_f64().or_else(|| v["score"].as_f64()))
        .map(|s| Ok(s as f32).into())
        .unwrap_or_else(|| Ok(0.0).into())
}

/// Filter profiles by age/location/trait preferences, delegating the policy
/// to dating-core.
#[frb(sync, serialize)]
#[allow(clippy::too_many_arguments)]
pub fn dating_filter_profiles(
    user_pubkey: String,
    min_age: i32,
    max_age: i32,
    location_radius_km: i32,
    height_min_cm: i32,
    height_max_cm: i32,
    body_type: String,
    smoking: String,
    drinking: String,
    relationship_intent: String,
    politics: String,
    education: String,
    interests_json: String,
) -> Result<String, String> {
    let mut cards = fetch_profiles_internal(&user_pubkey, 100, "public")?;
    if !interests_json.is_empty() {
        let interests: Vec<String> = serde_json::from_str(&interests_json).unwrap_or_default();
        if !interests.is_empty() {
            cards.retain(|c| c.interests.iter().any(|i| interests.contains(i)));
        }
    }
    let own_card = dating_get_own_profile(user_pubkey.clone())
        .ok()
        .and_then(|p| serde_json::from_str::<DatingCardInfo>(&p).ok());
    let own_location = own_card
        .as_ref()
        .and_then(|c| (!c.location.is_empty()).then(|| c.location.clone()));
    let own_gender = own_card
        .as_ref()
        .and_then(|c| (!c.gender.is_empty()).then(|| c.gender.clone()));
    let own_seeking = own_card
        .as_ref()
        .and_then(|c| (!c.seeking.is_empty()).then(|| c.seeking.clone()));
    let profiles: Vec<soshal_dating_core::DatingProfileInput> =
        cards.iter().map(profile_input_from_card).collect();
    let filtered = soshal_dating_core::filter::filter_dating_profiles(
        soshal_dating_core::FilterDatingProfilesInput {
            profiles: profiles.clone(),
            own_gender,
            own_seeking,
            own_location_geohash: own_location,
            own_max_distance_km: (location_radius_km > 0).then_some(f64::from(location_radius_km)),
            self_contacts: Vec::new(),
            hide_friends: None,
            min_age: (min_age > 0).then_some(f64::from(min_age)),
            max_age: (max_age > 0).then_some(f64::from(max_age)),
            height_min_cm: (height_min_cm > 0).then_some(f64::from(height_min_cm)),
            height_max_cm: (height_max_cm > 0).then_some(f64::from(height_max_cm)),
            body_type: opt_str(&body_type),
            smoking: opt_str(&smoking),
            drinking: opt_str(&drinking),
            relationship_intent: opt_str(&relationship_intent),
            politics: opt_str(&politics),
            education: opt_str(&education),
        },
    );
    let kept: Vec<soshal_dating_core::DatingProfileInput> = filtered
        .into_iter()
        .filter(|o| o.passes)
        .map(|o| profiles[o.index].clone())
        .collect();
    let sorted =
        soshal_dating_core::sort::sort_dating_profiles(soshal_dating_core::SortProfilesInput {
            profiles: kept,
            self_profile: self_profile_input(&user_pubkey),
            self_contacts: Vec::new(),
            sort_by: None,
        });
    let mut remaining: std::collections::HashMap<String, DatingCardInfo> =
        cards.into_iter().map(|c| (c.pubkey.clone(), c)).collect();
    let mut out = Vec::with_capacity(sorted.len().min(50));
    for s in sorted {
        if let Some(mut card) = remaining.remove(&s.pubkey) {
            card.compatibility_score = s.compatibility_score as f32;
            out.push(card);
            if out.len() >= 50 {
                break;
            }
        }
    }
    super::util::json_ok(out)
}

/// Dating profile statistics from the local graph.
#[frb(sync, serialize)]
pub fn dating_get_stats(user_pubkey: String) -> Result<String, String> {
    let (own_id, photo_count, likes, superlikes, views, matches) = super::db::with_db_result(
        |db| {
            let conn = db.conn()?;

            let own_profile = soshal_db_core::query::query_first(
            &conn,
            &format!(
                "SELECT id, content FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1 AND is_deleted = 0 LIMIT 1"
            ),
            libsql::params![user_pubkey.as_str()],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )?;

            let (own_id, photo_count) = if let Some((id, content)) = own_profile {
                let count = serde_json::from_str::<serde_json::Value>(&content)
                    .ok()
                    .and_then(|c| c["images"].as_array().map(|a| a.len() as i64))
                    .unwrap_or(0);
                (id, count)
            } else {
                (String::new(), 0)
            };

            let (likes, superlikes) = soshal_db_core::query::query_first(
                &conn,
                &format!(
                    "SELECT \
                 COALESCE(SUM(CASE WHEN content = '+' THEN 1 ELSE 0 END), 0), \
                 COALESCE(SUM(CASE WHEN content = 'super' THEN 1 ELSE 0 END), 0) \
                 FROM reactions WHERE event_id IN \
                 (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1)"
                ),
                libsql::params![user_pubkey.as_str()],
                |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?)),
            )?
            .unwrap_or((0, 0));

            let views = if !own_id.is_empty() {
                soshal_db_core::query::query_first(
                    &conn,
                    "SELECT COUNT(*) FROM post_views WHERE post_id = ?1",
                    libsql::params![own_id.as_str()],
                    |r| r.get::<i64>(0),
                )?
                .unwrap_or(0)
            } else {
                0
            };

            let matches = soshal_db_core::query::query_first(
            &conn,
            &format!(
                "SELECT COUNT(*) FROM reactions r \
                 JOIN posts p ON p.id = r.event_id \
                 WHERE p.kind = {KIND_PROFILE} AND r.content = '+' AND r.pubkey = ?1 \
                 AND EXISTS (SELECT 1 FROM reactions r2 WHERE r2.content = '+' AND r2.pubkey = p.pubkey \
                             AND r2.event_id IN (SELECT id FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = ?1))"
            ),
            libsql::params![user_pubkey.as_str()],
            |r| r.get::<i64>(0),
        )?
        .unwrap_or(0);

            Ok((own_id, photo_count, likes, superlikes, views, matches))
        },
    )?;

    let stats = serde_json::json!({
        "profile_views": views,
        "likes_received": likes,
        "superlike_received": superlikes,
        "matches": matches,
        "profile_complete": !own_id.is_empty(),
        "photo_count": photo_count,
    });
    Ok(serde_json::to_string(&stats).unwrap()).into()
}

/// Block a profile: local-only blocklist record (mirrors the desktop
/// `blocks` table).
#[frb(sync, serialize)]
pub fn dating_block_profile(user_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    super::signer::require_identity(&user_pubkey)?;
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
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::spam_report::SpamReportRow {
        id: format!("rep_{now}_{:x}", rand::random::<u64>()),
        pubkey: reporter_pubkey,
        target_id: None,
        target_pubkey: Some(target_pubkey),
        reason: Some(reason),
        tags: "[\"dating\"]".to_string(),
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::spam_report::SpamReportRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn call_create(
        pk: &str,
        location: &str,
        bio: &str,
        images: &str,
        interests: &str,
    ) -> Result<String, String> {
        dating_create_profile(
            pk.to_string(),
            "alice".to_string(),
            30,
            location.to_string(),
            "female".to_string(),
            "male".to_string(),
            170,
            "athletic".to_string(),
            "never".to_string(),
            "socially".to_string(),
            "serious".to_string(),
            "liberal".to_string(),
            "caucasian".to_string(),
            "bachelor's".to_string(),
            r#"["English"]"#.to_string(),
            100,
            bio.to_string(),
            images.to_string(),
            interests.to_string(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn call_filter(
        pk: &str,
        min_age: i32,
        max_age: i32,
        radius: i32,
        h_min: i32,
        h_max: i32,
        body_type: &str,
        smoking: &str,
        drinking: &str,
        intent: &str,
        politics: &str,
        education: &str,
        interests: &str,
    ) -> Result<String, String> {
        dating_filter_profiles(
            pk.to_string(),
            min_age,
            max_age,
            radius,
            h_min,
            h_max,
            body_type.to_string(),
            smoking.to_string(),
            drinking.to_string(),
            intent.to_string(),
            politics.to_string(),
            education.to_string(),
            interests.to_string(),
        )
    }

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
            "u33dc0".to_string(),
            "".to_string(),
            "".to_string(),
            0,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "[]".to_string(),
            0,
            "".to_string(),
            "[]".to_string(),
            "[]".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_location() {
        assert_eq!(resolve_location("u33dc0").unwrap(), "u33dc0");
        let encoded = resolve_location("51.5007,-0.1246").unwrap();
        assert_eq!(encoded, "gcpuv");
        assert_eq!(resolve_location("").unwrap_err(), "location is required");
        assert!(resolve_location("not a geohash!!").is_err());
        assert!(resolve_location("91,0").is_err());
    }

    #[test]
    fn test_validate_attributes_bad_enum() {
        assert!(validate_enum("smoking", "always", &SMOKING).is_err());
        assert!(validate_enum("gender", "female", &GENDERS).is_ok());
        assert!(validate_attributes(
            "male",
            "female",
            300,
            "athletic",
            "never",
            "socially",
            "serious",
            "liberal",
            "bachelor's",
            100,
        )
        .is_err());
    }

    #[test]
    fn test_filter_profiles_radius() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_radius", "ffi");
        crate::ffi::db::insert_test_user("own");
        crate::ffi::db::insert_test_user("near");
        crate::ffi::db::insert_test_user("far");
        let insert = |id: &str, pubkey: &str, age: i64, gh: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{id}','{pubkey}','{{\"age\":{age},\"bio\":\"\",\"locationGeohash\":\"{gh}\",\"interests\":[]}}',{KIND_PROFILE},{ts},'[]','synced',0)"
            ))
        };
        assert!(insert("own1", "own", 30, "u33dc0", 300).is_ok());
        assert!(insert("near1", "near", 25, "u33dc0", 200).is_ok());
        assert!(insert("far1", "far", 25, "9q8yyk", 100).is_ok());
        let res = call_filter("own", 18, 40, 100, 0, 0, "", "", "", "", "", "", "[]").unwrap();
        let cards: Vec<DatingCardInfo> = serde_json::from_str(&res).unwrap();
        assert!(cards.iter().any(|c| c.pubkey == "near"));
        assert!(!cards.iter().any(|c| c.pubkey == "far"));
        assert!(!cards.iter().any(|c| c.pubkey == "own"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_filter_profiles_traits_and_height() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_traits", "ffi");
        crate::ffi::db::insert_test_user("own");
        crate::ffi::db::insert_test_user("tall");
        crate::ffi::db::insert_test_user("short");
        let insert = |id: &str, pubkey: &str, height: i64, smoking: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{id}','{pubkey}','{{\"age\":30,\"bio\":\"\",\"locationGeohash\":\"u33dc0\",\"interests\":[],\"height\":{height},\"smoking\":\"{smoking}\",\"drinking\":\"socially\",\"bodyType\":\"athletic\"}}',{KIND_PROFILE},{ts},'[]','synced',0)"
            ))
        };
        assert!(insert("own1", "own", 170, "never", 300).is_ok());
        assert!(insert("h1", "tall", 190, "never", 200).is_ok());
        assert!(insert("h2", "short", 160, "regularly", 100).is_ok());
        let res = call_filter(
            "own", 0, 0, 0, 175, 0, "athletic", "never", "", "", "", "", "[]",
        )
        .unwrap();
        let cards: Vec<DatingCardInfo> = serde_json::from_str(&res).unwrap();
        assert!(cards.iter().any(|c| c.pubkey == "tall"), "{res}");
        assert!(!cards.iter().any(|c| c.pubkey == "short"), "{res}");
        assert!(!cards.iter().any(|c| c.pubkey == "own"), "{res}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_calculate_score_dealbreakers() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_score", "ffi");
        crate::ffi::db::insert_test_user("selfpk");
        crate::ffi::db::insert_test_user("tgtpk");
        let insert = |id: &str, pubkey: &str, smoking: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{id}','{pubkey}','{{\"age\":30,\"bio\":\"\",\"locationGeohash\":\"u33dc0\",\"interests\":[],\"smoking\":\"{smoking}\"}}',{KIND_PROFILE},{ts},'[]','synced',0)"
            ))
        };
        assert!(insert("self1", "selfpk", "never", 300).is_ok());
        assert!(insert("tgt1", "tgtpk", "regularly", 200).is_ok());
        let blocked = dating_calculate_score(
            "selfpk".to_string(),
            "tgtpk".to_string(),
            "{\"dealbreakers\":[\"smoking\"]}".to_string(),
        )
        .unwrap();
        assert_eq!(blocked, 0.0);
        let open =
            dating_calculate_score("selfpk".to_string(), "tgtpk".to_string(), "{}".to_string())
                .unwrap();
        assert!(open > 0.0);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_create_get_own_update_delete_profile() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_crud", "dt");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::ffi::db::insert_test_user(&pk);
        super::super::db::db_execute_params(
            "UPDATE users SET name = ?1 WHERE pubkey = ?2",
            &["alice".to_string(), pk.clone()],
        )
        .unwrap();

        let err = call_create(
            &pk,
            "u33dc0",
            "hi",
            &serde_json::to_string(&vec!["u".to_string(); 10]).unwrap(),
            "[]",
        );
        assert_eq!(err.unwrap_err(), "too many images");
        let err = call_create(&pk, "u33dc0", "hi", "not-json", "[]");
        assert!(err.unwrap_err().contains("invalid images JSON"));
        let err = call_create(&pk, "u33dc0", "hi", "[]", "not-json");
        assert!(err.unwrap_err().contains("invalid interests JSON"));
        let err = call_create(&pk, "", "hi", "[]", "[]");
        assert_eq!(err.unwrap_err(), "location is required");
        let err = call_create(&pk, "nonsense!", "hi", "[]", "[]");
        assert!(err.unwrap_err().contains("invalid geohash"));

        let signed = call_create(
            &pk,
            "51.5007,-0.1246",
            "hello",
            r#"["https://x/a.png"]"#,
            r#"["music","art"]"#,
        )
        .unwrap();
        let signed_v: serde_json::Value = serde_json::from_str(&signed).unwrap();
        let event_id = signed_v["id"].as_str().unwrap().to_string();
        let rows = super::super::db::db_query_raw_test(format!(
            "SELECT content FROM posts WHERE id = '{event_id}'"
        ))
        .unwrap();
        let rows_v: Vec<serde_json::Value> = serde_json::from_str(&rows).unwrap();
        let content_v: serde_json::Value =
            serde_json::from_str(rows_v[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(content_v["locationGeohash"], "gcpuv", "{rows}");
        assert_eq!(content_v["height"], 170.0);
        assert_eq!(content_v["smoking"], "never");
        assert_eq!(content_v["maxDistanceKm"], 100.0);

        let card: DatingCardInfo =
            serde_json::from_str(&dating_get_profile(event_id).unwrap()).unwrap();
        assert_eq!(card.name, "alice");
        assert_eq!(card.age, 30);
        assert_eq!(card.location, "gcpuv");
        assert_eq!(card.height, 170.0);
        assert_eq!(card.gender, "female");
        assert_eq!(card.interests, vec!["music".to_string(), "art".to_string()]);
        assert_eq!(
            dating_get_profile("deadbeef".to_string()).unwrap_err(),
            "Dating profile not found"
        );

        let own: DatingCardInfo =
            serde_json::from_str(&dating_get_own_profile(pk.clone()).unwrap()).unwrap();
        assert_eq!(own.bio, "hello");
        assert_eq!(
            dating_get_own_profile("otherpk".to_string()).unwrap_err(),
            "No dating profile yet"
        );

        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(dating_update_profile(
            pk.clone(),
            "u33dc0".to_string(),
            "female".to_string(),
            "male".to_string(),
            170,
            "athletic".to_string(),
            "never".to_string(),
            "socially".to_string(),
            "serious".to_string(),
            "liberal".to_string(),
            "caucasian".to_string(),
            "bachelor's".to_string(),
            r#"["English"]"#.to_string(),
            100,
            "updated bio".to_string(),
            "[]".to_string(),
            r#"["sports"]"#.to_string(),
        )
        .unwrap());
        let updated: DatingCardInfo =
            serde_json::from_str(&dating_get_own_profile(pk.clone()).unwrap()).unwrap();
        assert_eq!(updated.bio, "updated bio");
        assert_eq!(updated.age, 30, "update must preserve age");
        assert_eq!(
            updated.location, "u33dc",
            "geohash truncated to precision 5"
        );
        assert_eq!(updated.interests, vec!["sports".to_string()]);
        let cnt = super::super::db::db_query_raw_test(format!(
            "SELECT COUNT(*) AS c FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = '{pk}' AND is_deleted = 0"
        ))
        .unwrap();
        assert!(cnt.contains("\"c\":1"), "{cnt}");

        assert!(dating_delete_profile(pk.clone()).unwrap());
        assert_eq!(
            dating_get_own_profile(pk).unwrap_err(),
            "No dating profile yet"
        );
        let deleted = super::super::db::db_query_raw_test(format!(
            "SELECT COUNT(*) AS c FROM posts WHERE kind = {KIND_PROFILE} AND pubkey = '{}' AND is_deleted = 1",
            keys.public_key().to_hex()
        ))
        .unwrap();
        assert!(deleted.contains("\"c\":2"), "{deleted}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_react_and_pass() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_react", "dt");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::ffi::db::insert_test_user(&pk);
        let id64 = "a".repeat(64);
        crate::ffi::db::insert_test_user(&id64);
        // react() resolves the profile pubkey → profile event id, so the
        // target needs a posts row of KIND_PROFILE before a reaction lands.
        assert!(super::super::db::db_execute_raw_test(format!(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('prof1','{id64}','{{}}',{KIND_PROFILE},100,'[]','synced',0)"
        ))
        .is_ok());

        for f in [dating_like, dating_unlike, dating_superlike] {
            let err = f(pk.clone(), "short".to_string()).unwrap_err();
            assert_eq!(err, "invalid profile event id");
        }
        assert!(dating_like(pk.clone(), id64.clone()).unwrap());
        assert!(dating_unlike(pk.clone(), id64.clone()).unwrap());
        assert!(dating_superlike(pk.clone(), id64.clone()).unwrap());

        let reactions = super::super::db::db_query_raw_test(format!(
            "SELECT content FROM reactions WHERE id = 'reaction:{pk}:{id64}'"
        ))
        .unwrap();
        let rv: Vec<serde_json::Value> = serde_json::from_str(&reactions).unwrap();
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0]["content"], "super");

        let outbox = super::super::db::db_query_raw_test(
            "SELECT payload_json FROM outbox_queue WHERE action_type = 'reaction'".to_string(),
        )
        .unwrap();
        let ov: Vec<serde_json::Value> = serde_json::from_str(&outbox).unwrap();
        assert_eq!(ov.len(), 3);
        let payloads: Vec<String> = ov
            .iter()
            .filter_map(|r| r["payload_json"].as_str())
            .map(soshal_sync_core::outbox::decompress_payload)
            .map(|p| {
                let unseal = super::super::sync::outbox_unseal_fn();
                soshal_sync_core::outbox::unseal_payload(&p, &unseal)
            })
            .filter_map(|p| serde_json::from_str::<serde_json::Value>(&p).ok())
            .filter_map(|v| v["content"].as_str().map(|s| s.to_string()))
            .collect();
        assert!(payloads.iter().any(|p| p == "+"), "got {payloads:?}");
        assert!(payloads.iter().any(|p| p == "-"), "got {payloads:?}");
        assert!(payloads.iter().any(|p| p == "super"), "got {payloads:?}");

        let err = dating_pass(pk.clone(), "short".to_string()).unwrap_err();
        assert_eq!(err, "invalid profile event id");
        assert!(dating_pass(pk.clone(), id64.clone()).unwrap());
        let pass_rows = super::super::db::db_query_raw_test(format!(
            "SELECT content FROM reactions WHERE id = 'pass:{pk}:{id64}'"
        ))
        .unwrap();
        assert!(pass_rows.contains("\"content\":\"pass\""), "{pass_rows}");
        let outbox2 = super::super::db::db_query_raw_test(
            "SELECT COUNT(*) AS c FROM outbox_queue".to_string(),
        )
        .unwrap();
        assert!(outbox2.contains("\"c\":3"), "{outbox2}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_reset_passes() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_reset_passes", "dt");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::ffi::db::insert_test_user(&pk);
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        for pubkey in [&a, &b] {
            crate::ffi::db::insert_test_user(pubkey);
            assert!(super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{pubkey}','{pubkey}','{{\"age\":30,\"bio\":\"x\",\"locationGeohash\":\"9q8yyk7qnv\",\"interests\":[\"music\"]}}',{KIND_PROFILE},100,'[]','synced',0)"
            ))
            .is_ok());
        }

        assert!(dating_pass(pk.clone(), a.clone()).unwrap());
        assert!(dating_pass(pk.clone(), b.clone()).unwrap());
        let before: Vec<serde_json::Value> = serde_json::from_str(
            &dating_fetch_profiles(pk.clone(), 100, "public".to_string()).unwrap(),
        )
        .unwrap();
        assert!(
            before.iter().all(|c| c["pubkey"] != a && c["pubkey"] != b),
            "passed profiles must be hidden before reset: {before:?}"
        );

        assert_eq!(dating_reset_passes(pk.clone()).unwrap(), 2);
        assert_eq!(dating_reset_passes(pk.clone()).unwrap(), 0);

        let after: Vec<serde_json::Value> = serde_json::from_str(
            &dating_fetch_profiles(pk.clone(), 100, "public".to_string()).unwrap(),
        )
        .unwrap();
        assert!(
            after.iter().any(|c| c["pubkey"] == a),
            "profile a must reappear: {after:?}"
        );
        assert!(
            after.iter().any(|c| c["pubkey"] == b),
            "profile b must reappear: {after:?}"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_fetch_likes_matches_unmatch() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_graph", "dt");
        crate::ffi::db::insert_test_user("me");
        crate::ffi::db::insert_test_user("likera");
        crate::ffi::db::insert_test_user("likerb");
        crate::ffi::db::insert_test_user("cand");
        crate::ffi::db::insert_test_user("candb");
        let insert_post = |id: &str, pubkey: &str, gh: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{id}','{pubkey}','{{\"age\":30,\"bio\":\"\",\"locationGeohash\":\"{gh}\",\"interests\":[]}}',{KIND_PROFILE},{ts},'[]','synced',0)"
            ))
        };
        let insert_react = |id: &str, event_id: &str, pubkey: &str, content: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO reactions (id, event_id, pubkey, content, created_at, kind) \
                 VALUES ('{id}','{event_id}','{pubkey}','{content}',{ts},7)"
            ))
        };
        assert!(insert_post("own1", "me", "u33dc0", 400).is_ok());
        assert!(insert_post("la1", "likera", "u33dc0", 300).is_ok());
        assert!(insert_post("lb1", "likerb", "u33dc0", 200).is_ok());
        assert!(insert_post("cand1", "cand", "u33dc0", 100).is_ok());
        assert!(insert_post("cand2", "candb", "u33dc0", 90).is_ok());
        assert!(insert_react("r1", "own1", "likera", "+", 300).is_ok());
        assert!(insert_react("r2", "own1", "likera", "+", 200).is_ok());
        assert!(insert_react("r3", "own1", "likerb", "+", 100).is_ok());

        let likes = dating_fetch_likes("me".to_string()).unwrap();
        let lv: Vec<DatingCardInfo> = serde_json::from_str(&likes).unwrap();
        let mut seen: Vec<String> = lv.iter().map(|c| c.pubkey.clone()).collect();
        seen.sort();
        assert_eq!(
            seen,
            vec![
                "likera".to_string(),
                "likera".to_string(),
                "likerb".to_string()
            ]
        );
        let mut uniq = seen.clone();
        uniq.dedup();
        assert_eq!(uniq, vec!["likera".to_string(), "likerb".to_string()]);

        assert!(insert_react("r4", "la1", "me", "+", 250).is_ok());
        let matches = dating_fetch_matches("me".to_string()).unwrap();
        let mv: Vec<DatingCardInfo> = serde_json::from_str(&matches).unwrap();
        assert_eq!(mv.len(), 1, "{matches}");
        assert_eq!(mv[0].pubkey, "likera");

        assert!(dating_unmatch("me".to_string(), "candb".to_string()).unwrap());
        let profiles = dating_fetch_profiles("me".to_string(), 100, "public".to_string()).unwrap();
        let pv: Vec<DatingCardInfo> = serde_json::from_str(&profiles).unwrap();
        let pubs: Vec<String> = pv.iter().map(|c| c.pubkey.clone()).collect();
        assert!(pubs.contains(&"cand".to_string()), "{pubs:?}");
        assert!(!pubs.contains(&"candb".to_string()), "{pubs:?}");
        assert!(!pubs.contains(&"likera".to_string()), "{pubs:?}");
        assert!(!pubs.contains(&"me".to_string()), "{pubs:?}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_calculate_score_and_filter() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_scorefilter", "dt");
        crate::ffi::db::insert_test_user("selfpk");
        crate::ffi::db::insert_test_user("tgtpk");
        crate::ffi::db::insert_test_user("gpk");
        crate::ffi::db::insert_test_user("cand1");
        crate::ffi::db::insert_test_user("cand2");
        crate::ffi::db::insert_test_user("far");
        for i in 0..55 {
            crate::ffi::db::insert_test_user(&format!("bulk{i}"));
        }
        let insert_post = |id: &str, pubkey: &str, gh: &str, interests: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{id}','{pubkey}','{{\"age\":30,\"bio\":\"\",\"locationGeohash\":\"{gh}\",\"interests\":{interests}}}',{KIND_PROFILE},{ts},'[]','synced',0)"
            ))
        };
        assert!(insert_post("self1", "selfpk", "u33dc0", "[\"music\"]", 400).is_ok());
        assert!(insert_post("tgt1", "tgtpk", "u33dc0", "[\"music\"]", 300).is_ok());

        let malformed = dating_calculate_score(
            "selfpk".to_string(),
            "tgtpk".to_string(),
            "{not json".to_string(),
        );
        assert!(malformed.is_ok(), "{malformed:?}");
        assert_eq!(
            dating_calculate_score("ghost".to_string(), "tgtpk".to_string(), "{}".to_string())
                .unwrap(),
            0.0
        );
        assert_eq!(
            dating_calculate_score("selfpk".to_string(), "ghost2".to_string(), "{}".to_string())
                .unwrap(),
            0.0
        );
        assert!(super::super::db::db_execute_raw_test(format!(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('gself','gpk','not-json',{KIND_PROFILE},200,'[]','synced',0)"
        ))
        .is_ok());
        let garbage =
            dating_calculate_score("gpk".to_string(), "tgtpk".to_string(), "{}".to_string());
        assert!(garbage.is_ok(), "{garbage:?}");

        assert!(insert_post("c1", "cand1", "u33dc0", "[\"music\",\"art\"]", 250).is_ok());
        assert!(insert_post("c2", "cand2", "u33dc0", "[\"sports\"]", 240).is_ok());
        assert!(insert_post("c3", "far", "9q8yyk", "[\"music\"]", 230).is_ok());
        let filtered = call_filter(
            "selfpk",
            18,
            40,
            0,
            0,
            0,
            "",
            "",
            "",
            "",
            "",
            "",
            r#"["music"]"#,
        )
        .unwrap();
        let fv: Vec<DatingCardInfo> = serde_json::from_str(&filtered).unwrap();
        let pubs: Vec<String> = fv.iter().map(|c| c.pubkey.clone()).collect();
        assert!(pubs.contains(&"cand1".to_string()), "{pubs:?}");
        assert!(!pubs.contains(&"cand2".to_string()), "{pubs:?}");
        assert!(pubs.contains(&"far".to_string()), "{pubs:?}");

        let nearby =
            call_filter("selfpk", 18, 40, 100, 0, 0, "", "", "", "", "", "", "[]").unwrap();
        let nv: Vec<DatingCardInfo> = serde_json::from_str(&nearby).unwrap();
        assert!(nv.iter().any(|c| c.pubkey == "cand1"), "{nearby}");
        assert!(!nv.iter().any(|c| c.pubkey == "far"), "{nearby}");

        for i in 0..55 {
            assert!(insert_post(
                &format!("bulk{i}"),
                &format!("bulk{i}"),
                "u33dc0",
                "[]",
                1000 + i
            )
            .is_ok());
        }
        let truncated = call_filter("selfpk", 0, 0, 0, 0, 0, "", "", "", "", "", "", "").unwrap();
        let tv: Vec<DatingCardInfo> = serde_json::from_str(&truncated).unwrap();
        assert_eq!(tv.len(), 50, "{truncated}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_stats_block_report() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK.lock().unwrap();
        let path = crate::ffi::db::tmp_db("dating_stats", "dt");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::ffi::db::insert_test_user(&me);
        crate::ffi::db::insert_test_user("likera");
        crate::ffi::db::insert_test_user("likerb");
        crate::ffi::db::insert_test_user("likerc");
        let insert_post = |id: &str, pubkey: &str, images: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
                 VALUES ('{id}','{pubkey}','{{\"age\":30,\"bio\":\"\",\"locationGeohash\":\"u33dc0\",\"interests\":[],\"images\":{images}}}',{KIND_PROFILE},{ts},'[]','synced',0)"
            ))
        };
        let insert_react = |id: &str, event_id: &str, pubkey: &str, content: &str, ts: i64| {
            super::super::db::db_execute_raw_test(format!(
                "INSERT INTO reactions (id, event_id, pubkey, content, created_at, kind) \
                 VALUES ('{id}','{event_id}','{pubkey}','{content}',{ts},7)"
            ))
        };
        assert!(insert_post("own1", &me, r#"["https://x/a.png","https://x/b.png"]"#, 400).is_ok());
        assert!(insert_post("la1", "likera", "[]", 300).is_ok());
        assert!(insert_react("r1", "own1", "likera", "+", 300).is_ok());
        assert!(insert_react("r2", "own1", "likerb", "+", 200).is_ok());
        assert!(insert_react("r3", "own1", "likerc", "super", 100).is_ok());
        assert!(insert_react("r4", "la1", &me, "+", 250).is_ok());
        assert!(super::super::db::db_execute_raw_test(
            "INSERT INTO post_views (pubkey, post_id, seen_at) VALUES ('v1','own1',300),('v2','own1',200)"
                .to_string()
        )
        .is_ok());

        let stats = dating_get_stats(me.clone()).unwrap();
        let sv: serde_json::Value = serde_json::from_str(&stats).unwrap();
        assert_eq!(sv["likes_received"], 2);
        assert_eq!(sv["superlike_received"], 1);
        assert_eq!(sv["profile_views"], 2);
        assert_eq!(sv["photo_count"], 2);
        assert_eq!(sv["matches"], 1);
        assert_eq!(sv["profile_complete"], true);

        assert!(dating_block_profile(me.clone(), "badguy".to_string()).unwrap());
        let blocks = super::super::db::db_query_raw_test(format!(
            "SELECT blocked_pubkey FROM blocks WHERE pubkey = '{me}'"
        ))
        .unwrap();
        assert!(blocks.contains("badguy"), "{blocks}");
        assert!(dating_unblock_profile(me.clone(), "badguy".to_string()).unwrap());
        let blocks2 = super::super::db::db_query_raw_test(format!(
            "SELECT COUNT(*) AS c FROM blocks WHERE pubkey = '{me}'"
        ))
        .unwrap();
        assert!(blocks2.contains("\"c\":0"), "{blocks2}");

        assert!(dating_report_profile(me.clone(), "badguy".to_string(), "x".repeat(600)).unwrap());
        let reports = super::super::db::db_query_raw_test(
            "SELECT reason, tags FROM spam_reports WHERE target_pubkey = 'badguy'".to_string(),
        )
        .unwrap();
        let rv: Vec<serde_json::Value> = serde_json::from_str(&reports).unwrap();
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0]["reason"].as_str().unwrap().chars().count(), 512);
        let tags_v: serde_json::Value =
            serde_json::from_str(rv[0]["tags"].as_str().unwrap()).unwrap();
        assert_eq!(tags_v, serde_json::json!(["dating"]));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }
}
