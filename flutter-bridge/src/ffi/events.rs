//! Events FFI module
//!
//! Locally-stored calendar events (kinds 31922/31923 rows in the posts
//! table, relay-synced like all events), NIP-52 RSVPs (kind 31924) and
//! attendance check-ins (kind 9 badge awards). Check-in timing is enforced
//! by events-core policy.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_events_core::checkin::can_checkin;

const EVENT_KINDS: &str = "31922,31923";

/// Event info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventInfo {
    pub id: String,
    pub creator_pubkey: String,
    pub title: String,
    pub description: String,
    pub location: String,
    pub latitude: f64,
    pub longitude: f64,
    pub start_time: u64,
    pub end_time: u64,
    pub image: String,
    pub attendees: i32,
    pub rsvp_status: String,
    pub created_at: u64,
}

#[derive(Deserialize)]
struct EventContent {
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    location: Option<serde_json::Value>,
    #[serde(rename = "start")]
    start_time: Option<u64>,
    #[serde(rename = "end")]
    end_time: Option<u64>,
    image: Option<String>,
    #[serde(rename = "t")]
    _sample_tag: Option<String>,
}

fn event_from_value(v: &serde_json::Value) -> Option<EventInfo> {
    let content_value: serde_json::Value = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["content"].clone(),
    };
    let c: EventContent = serde_json::from_value(content_value).ok()?;
    let (lat, lon) = match c.location.as_ref() {
        Some(serde_json::Value::String(s)) => centroid_of(s),
        Some(serde_json::Value::Object(o)) => (
            o.get("lat").and_then(|x| x.as_f64()).unwrap_or(0.0),
            o.get("lng").and_then(|x| x.as_f64()).unwrap_or(0.0),
        ),
        _ => (0.0, 0.0),
    };
    Some(EventInfo {
        id: v["id"].as_str()?.to_string(),
        creator_pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        title: c.title.or(c.name).unwrap_or_default(),
        description: c.description.unwrap_or_default(),
        location: match c.location {
            Some(serde_json::Value::String(s)) => s,
            Some(serde_json::Value::Object(o)) => o
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        },
        latitude: lat,
        longitude: lon,
        start_time: c.start_time.unwrap_or(0),
        end_time: c.end_time.unwrap_or(0),
        image: c.image.unwrap_or_default(),
        attendees: v["attendees"].as_i64().unwrap_or(0) as i32,
        rsvp_status: v["rsvp"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
    })
}

fn centroid_of(s: &str) -> (f64, f64) {
    let parts: Vec<&str> = s.split(',').collect();
    match parts.as_slice() {
        [lat, lon] => (
            lat.trim().parse::<f64>().unwrap_or(0.0),
            lon.trim().parse::<f64>().unwrap_or(0.0),
        ),
        _ => (0.0, 0.0),
    }
}

fn event_rows_sql(extra: &str, limit: i32) -> String {
    String::from("SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json ")
        + "FROM posts p WHERE p.kind IN ("
        + EVENT_KINDS
        + ") AND p.is_deleted = 0 "
        + extra
        + &format!(" ORDER BY p.created_at DESC LIMIT {}", limit.clamp(1, 100))
}

fn events_from_json(json: String) -> Vec<EventInfo> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    rows.into_iter()
        .filter_map(|v| event_from_value(&v))
        .collect()
}

fn rsvp_for_user(rows: &[serde_json::Value], my_pk: &str) -> String {
    for r in rows {
        let content = r["content"].as_str().unwrap_or("");
        let owner = r["pubkey"].as_str().unwrap_or("");
        if owner == my_pk && ["accepted", "declined", "pending"].contains(&content) {
            return content.to_string();
        }
    }
    String::new()
}

fn attendees_count(event_id: &str) -> i32 {
    super::db::db_query_raw(format!(
        "SELECT COUNT(DISTINCT pubkey) AS c FROM posts WHERE kind = 31924 \
         AND content = 'accepted' AND tags_json LIKE '%\"{}\"%'",
        event_id.replace('\'', "''")
    ))
    .ok()
    .and_then(|json| {
        serde_json::from_str::<Vec<serde_json::Value>>(&json)
            .ok()
            .and_then(|r| r.first().and_then(|v| v["c"].as_i64()))
    })
    .unwrap_or(0) as i32
}

/// Fetch nearby events (distance filter computed client-side over the
/// locally stored event rows; exact haversine, not a coarse box).
#[frb(sync, serialize)]
pub fn events_fetch_nearby(
    latitude: f64,
    longitude: f64,
    radius_km: f32,
    limit: i32,
) -> Result<String, String> {
    let radius = radius_km.max(1.0).min(5000.0);
    let json = super::db::db_query_raw(event_rows_sql("", limit))?;
    let mut out: Vec<EventInfo> = events_from_json(json);
    let center = (latitude, longitude);
    out.retain(|e| {
        if e.latitude == 0.0 && e.longitude == 0.0 {
            return false;
        }
        haversine(center, (e.latitude, e.longitude)) <= radius as f64
    });
    super::util::json_ok(out)
}

fn haversine(a: (f64, f64), b: (f64, f64)) -> f64 {
    let r = 6371.0;
    let d_lat = (b.0 - a.0).to_radians();
    let d_lon = (b.1 - a.1).to_radians();
    let h = (d_lat / 2.0).sin().powi(2)
        + a.0.to_radians().cos() * b.0.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

/// Fetch events the user is involved in (created, RSVPed, or attended).
#[frb(sync, serialize)]
pub fn events_fetch_user_events(user_pubkey: String, limit: i32) -> Result<String, String> {
    let pk = user_pubkey.replace('\'', "''");
    let json = super::db::db_query_raw(event_rows_sql(
        &format!(
            "AND (p.pubkey = '{pk}' OR EXISTS (SELECT 1 FROM posts r WHERE r.pubkey = '{pk}' \
             AND (r.tags_json LIKE '%\"e\",\"' || p.id || '\"%' \
                  OR r.id = 'checkin:' || '{pk}' || ':' || p.id)))"
        ),
        limit,
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let mut out: Vec<EventInfo> = Vec::new();
    for v in rows {
        if let Some(mut e) = event_from_value(&v) {
            e.attendees = attendees_count(&e.id);
            e.rsvp_status = rsvp_for_user(&[v], &user_pubkey);
            out.push(e);
        }
    }
    super::util::json_ok(out)
}

/// Create an event (kind 31923 calendar event; content per the calendar
/// sync format). Signs with the unlocked signer and stores the row locally.
#[allow(clippy::too_many_arguments)]
#[frb(sync, serialize)]
pub fn events_create(
    creator_pubkey: String,
    title: String,
    description: String,
    location: String,
    latitude: f64,
    longitude: f64,
    start_time: u64,
    end_time: u64,
    image_url: String,
) -> Result<String, String> {
    if title.trim().is_empty() || title.len() > 300 {
        return Err("title must be 1..=300 chars".to_string()).into();
    }
    if start_time == 0 || (end_time != 0 && end_time < start_time) {
        return Err("invalid time range".to_string()).into();
    }
    if end_time != 0 && end_time.saturating_sub(start_time) > 7 * 24 * 3600 {
        return Err("events may not span more than 7 days".to_string()).into();
    }
    let d_tag = format!("{}-{}", creator_pubkey.get(..12).unwrap_or(""), start_time);
    let content = serde_json::json!({
        "name": title,
        "description": soshal_common_core::format::truncate(&description, 2000),
        "location": if location.trim().is_empty() {
            serde_json::Value::Null
        } else if latitude != 0.0 || longitude != 0.0 {
            serde_json::json!({"name": location, "lat": latitude, "lng": longitude})
        } else {
            serde_json::json!(location)
        },
        "start": start_time,
        "end": end_time,
        "image": if image_url.is_empty() { serde_json::Value::Null } else { serde_json::json!(image_url) },
    });
    let builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(31923), content.to_string())
            .tags(
                vec![
                    vec!["d".to_string(), d_tag],
                    vec!["t".to_string(), "date".to_string()],
                ]
                .into_iter()
                .filter_map(|t| nostr::event::Tag::parse(t).ok()),
            );
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    let row = soshal_db_core::repos::post::PostRow {
        id: event_id,
        pubkey: creator_pubkey,
        content: content.to_string(),
        kind: 31923,
        created_at: soshal_common_core::format::now_secs(),
        tags_json: String::new(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: Some(title),
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

fn d_tag_of(event_id: &str) -> String {
    let json = super::db::db_query_raw(format!(
        "SELECT tags_json FROM posts WHERE id = '{}'",
        event_id.replace('\'', "''")
    ))
    .unwrap_or_else(|_| "[]".to_string());
    let tags_json: Option<String> = serde_json::from_str::<Vec<serde_json::Value>>(&json)
        .ok()
        .and_then(|r| r.first().cloned())
        .and_then(|v| v["tags_json"].as_str().map(|s| s.to_string()));
    serde_json::from_str::<Vec<Vec<String>>>(&tags_json.unwrap_or_default())
        .ok()
        .and_then(|t| {
            t.into_iter()
                .find(|t| t.first().map(|k| k == "d").unwrap_or(false))
                .and_then(|t| t.get(1).cloned())
        })
        .unwrap_or_default()
}

fn valid_rsvp(status: &str) -> bool {
    matches!(status, "accepted" | "declined" | "pending")
}

/// RSVP to an event (kind 31924, content accepted/declined/pending).
#[frb(sync, serialize)]
pub fn events_rsvp(
    event_id: String,
    user_pubkey: String,
    rsvp_status: String,
) -> Result<bool, String> {
    if !valid_rsvp(&rsvp_status) {
        return Err("rsvp_status must be accepted/declined/pending".to_string()).into();
    }
    let host = match events_get_event(event_id.clone()) {
        Ok(e) => {
            serde_json::from_str::<EventInfo>(&e)
                .map_err(|e| format!("parse event: {e}"))?
                .creator_pubkey
        }
        Err(_) => return Err("event not found locally (sync relays first)".to_string()).into(),
    };
    let event_d = d_tag_of(&event_id);
    let d_tag = if event_d.is_empty() {
        format!("{}-{}", host.get(..12).unwrap_or(""), 0)
    } else {
        event_d
    };
    let builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(31924), rsvp_status.clone())
            .tags(
                vec![
                    vec!["a".to_string(), format!("31924:{host}:{d_tag}")],
                    vec!["e".to_string(), event_id.clone()],
                ]
                .into_iter()
                .filter_map(|t| nostr::event::Tag::parse(t).ok()),
            );
    let _ = super::signer::sign_builder(builder)?;
    let row = soshal_db_core::repos::post::PostRow {
        id: format!("rsvp:{}:{}", user_pubkey, rsvp_status),
        pubkey: user_pubkey,
        content: rsvp_status,
        kind: 31924,
        created_at: soshal_common_core::format::now_secs(),
        tags_json: format!(
            r#"[["a","31924:{host}:{d_tag}"],["e","{event_id}"]]"#,
            event_id = event_id.clone()
        ),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: None,
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Check in to an event. Enforced via events-core `can_checkin`: only
/// within the event window (+ buffer) is the attendance badge signed.
#[frb(sync, serialize)]
pub fn events_check_in(
    event_id: String,
    user_pubkey: String,
    latitude: f64,
    longitude: f64,
) -> Result<bool, String> {
    let event: EventInfo = serde_json::from_str(&events_get_event(event_id.clone())?)
        .map_err(|e| format!("parse event: {e}"))?;
    let now = soshal_common_core::format::now_secs() as u64;
    let window = if event.start_time == 0 {
        (now.saturating_sub(2 * 3600), now.saturating_add(2 * 3600))
    } else {
        (
            event.start_time.saturating_sub(3600),
            event.end_time.max(event.start_time),
        )
    };
    if !can_checkin(window.0, window.1, now, 0) {
        return Err("check-in outside the event window".to_string()).into();
    }
    let _ = (latitude, longitude);
    let content = format!("Claim attendance badge for event {event_id}");
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::BadgeAward, content.clone());
    for tag in [
        vec!["d".to_string(), format!("badge-{event_id}")],
        vec!["e".to_string(), event_id.clone()],
        vec!["p".to_string(), user_pubkey.clone()],
    ] {
        if let Ok(t) = nostr::event::Tag::parse(tag) {
            builder = builder.tag(t);
        }
    }
    let _ = super::signer::sign_builder(builder)?;
    let row = soshal_db_core::repos::post::PostRow {
        id: format!("checkin:{}:{}", user_pubkey, event_id),
        pubkey: user_pubkey.clone(),
        content,
        kind: 9,
        created_at: soshal_common_core::format::now_secs(),
        tags_json: format!(
            r#"[["d","badge-{event_id}"],["e","{event_id}"],["p","{user_pubkey}"]]"#,
            event_id = event_id.clone(),
            user_pubkey = user_pubkey.clone()
        ),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: None,
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Event details: includes local RSVP + attendee counts.
#[frb(sync, serialize)]
pub fn events_get_event(event_id: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json FROM posts p \
         WHERE p.kind IN ({EVENT_KINDS}) AND p.id = '{}'",
        event_id.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let v = rows.first().ok_or("Event not found".to_string())?;
    let mut info = event_from_value(v).ok_or("Event not found".to_string())?;
    info.attendees = attendees_count(&event_id);
    super::util::json_ok(info)
}

/// Attendees = distinct pubkeys with an `accepted` RSVP (kind 31924).
#[frb(sync, serialize)]
pub fn events_get_attendees(event_id: String) -> Result<Vec<String>, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT DISTINCT pubkey FROM posts WHERE kind = 31924 AND content = 'accepted' \
         AND tags_json LIKE '%\"{}\"%' ORDER BY created_at DESC LIMIT 200",
        event_id.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    Ok(rows
        .into_iter()
        .filter_map(|r| r["pubkey"].as_str().map(|s| s.to_string()))
        .collect())
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_centroid_parse() {
        assert_eq!(centroid_of("37.7749,-122.4194"), (37.7749, -122.4194));
        assert_eq!(centroid_of("not a coords"), (0.0, 0.0));
    }

    #[test]
    fn test_centroid_parse_edges() {
        assert_eq!(centroid_of(""), (0.0, 0.0));
        assert_eq!(centroid_of("37.7749"), (0.0, 0.0), "single coord");
        assert_eq!(
            centroid_of("37.7749,-122.4194,999"),
            (0.0, 0.0),
            "extra parts"
        );
        assert_eq!(centroid_of("not,a,coord"), (0.0, 0.0), "unparseable parts");
        assert_eq!(
            centroid_of(" 37.7 , -122.4 "),
            (37.7, -122.4),
            "trims whitespace"
        );
        assert_eq!(centroid_of("37.7,-122.4.5"), (37.7, 0.0), "bad lon part");
    }

    #[test]
    fn test_haversine_sf_la() {
        let d = haversine((37.7749, -122.4194), (34.0522, -118.2437));
        assert!((540.0..560.0).contains(&d), "got {d}");
    }

    #[test]
    fn test_valid_rsvp_statuses() {
        assert!(valid_rsvp("accepted"));
        assert!(valid_rsvp("declined"));
        assert!(valid_rsvp("pending"));
        assert!(!valid_rsvp("maybe"));
    }

    #[test]
    fn test_rsvp_rejects_bad_status() {
        let result = events_rsvp("a".repeat(64), "pk".to_string(), "maybe".to_string());
        assert!(result.is_err());
    }
}

// ─── Event reminders ────────────────────────────────────────────────────────
// Local reminder rows (reminders table); firing is backend-gated (no local
// notification scheduler yet), so the UI lists/manages them honestly.

/// All reminder rows, soonest first, as JSON array.
#[frb(sync, serialize)]
pub fn events_reminders_list() -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::reminder::ReminderRepo::new(db).list()?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Create/update a reminder. Returns the reminder id.
#[frb(sync, serialize)]
pub fn events_reminder_upsert(
    reminder_id: String,
    event_id: String,
    title: String,
    start_time: i64,
    minutes_before: i64,
) -> Result<String, String> {
    if minutes_before < 0 {
        return Err("minutes_before must be >= 0".to_string()).into();
    }
    let now = soshal_common_core::format::now_secs();
    let id = if reminder_id.is_empty() {
        format!("rem_{now}_{:x}", rand::random::<u32>())
    } else {
        reminder_id
    };
    let row = soshal_db_core::repos::reminder::ReminderRow {
        id: id.clone(),
        event_id,
        title,
        start_time,
        minutes_before,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::reminder::ReminderRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// Delete a reminder.
#[frb(sync, serialize)]
pub fn events_reminder_delete(reminder_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::reminder::ReminderRepo::new(db).delete(&reminder_id)?;
        Ok(true)
    })
}

/// Expiry timestamp (unix secs, 0 if none) parsed from a tags JSON array.
#[frb(sync, serialize)]
pub fn events_expiry_from_tags(tags_json: String) -> Result<i64, String> {
    Ok(soshal_events_core::event::expiry::get_expiry_from_tags_json(&tags_json))
}

/// Interest score between my interests and a peer's (JSON: score/common).
#[frb(sync, serialize)]
pub fn events_interest_score(
    my_interests_json: String,
    peer_interests_json: String,
) -> Result<String, String> {
    let score = soshal_events_core::event::interest::compute_interest_score_json(&format!(
        r#"{{"my_interests":{my_interests_json},"peer_interests":{peer_interests_json}}}"#
    ));
    Ok(score)
}
