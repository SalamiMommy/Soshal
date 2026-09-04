//! Events FFI module
//!
//! Locally-stored calendar events (kinds 31922/31923 rows in the posts
//! table, relay-synced like all events), NIP-52 RSVPs (kind 31924) and
//! attendance check-ins (kind 9 badge awards). Check-in timing is enforced
//! by events-core policy.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_common_core::consts::{KIND_EVENT, KIND_EVENT_RSVP};
use soshal_events_core::checkin::{can_checkin, within_checkin_radius};

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

/// Attendee counts per event, restricted to the given event ids so the query
/// hits the `(kind, content, rsvp_event_id)` index instead of scanning the
/// whole kind-31924 set. Empty slice = no events, returns an empty map.
fn attendee_counts_for_ids(ids: &[String]) -> std::collections::HashMap<String, i32> {
    let mut counts = std::collections::HashMap::new();
    if ids.is_empty() {
        return counts;
    }
    let ids_json = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
    if let Ok(json) = super::db::db_query_params(
        "SELECT rsvp_event_id, COUNT(*) FROM posts WHERE kind = ?1 \
         AND content = 'accepted' AND rsvp_event_id IN (SELECT value FROM json_each(?2)) \
         GROUP BY rsvp_event_id",
        &[KIND_EVENT_RSVP.to_string(), ids_json],
    ) {
        if let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&json) {
            for r in rows {
                if let Some(id) = r["rsvp_event_id"].as_str() {
                    counts.insert(id.to_string(), r["COUNT(*)"].as_i64().unwrap_or(0) as i32);
                }
            }
        }
    }
    counts
}

fn attendees_count(event_id: &str) -> i32 {
    if let Ok(json) = super::db::db_query_params(
        "SELECT COUNT(*) FROM posts WHERE kind = ?1 AND content = 'accepted' \
         AND rsvp_event_id = ?2",
        &[KIND_EVENT_RSVP.to_string(), event_id.to_string()],
    ) {
        if let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&json) {
            if let Some(row) = rows.first() {
                return row["COUNT(*)"].as_i64().unwrap_or(0) as i32;
            }
        }
    }
    0
}

/// Fetch nearby events (distance filter computed client-side over the
/// locally stored event rows; exact haversine, not a coarse box). A SQL-side
/// bounding-box prefilter on the denormalized event_lat/event_lng columns
/// keeps the fetch set small and fixes the old fetch-then-filter underfill.
/// `radius_km <= 0` means "anywhere": no geo filter, zero-coordinate
/// (location-less) events included.
#[frb(sync, serialize)]
pub fn events_fetch_nearby(
    latitude: f64,
    longitude: f64,
    radius_km: f32,
    limit: i32,
) -> Result<String, String> {
    if radius_km <= 0.0 {
        let json = super::db::db_query_raw(event_rows_sql("", limit))?;
        return super::util::json_ok(events_from_json(json));
    }
    let radius = radius_km.min(5000.0);
    let lat_deg = radius as f64 / 110.574;
    let lon_deg = radius as f64 / (111.320 * latitude.to_radians().cos().abs().max(0.01));
    let (lat1, lat2) = (latitude - lat_deg, latitude + lat_deg);
    let (lon1, lon2) = (longitude - lon_deg, longitude + lon_deg);
    let geo_filter = format!(
        "AND p.event_lat BETWEEN {lat1:.6} AND {lat2:.6} \
         AND p.event_lng BETWEEN {lon1:.6} AND {lon2:.6} "
    );
    let json = super::db::db_query_raw(event_rows_sql(&geo_filter, limit))?;
    let mut out: Vec<EventInfo> = events_from_json(json);
    let center = (latitude, longitude);
    out.retain(|e| {
        if e.latitude == 0.0 && e.longitude == 0.0 {
            return false;
        }
        soshal_spatial_core::distance::haversine_km(center.0, center.1, e.latitude, e.longitude)
            <= radius as f64
    });
    super::util::json_ok(out)
}

/// Fetch events the user is involved in (created, RSVPed, or attended).
#[frb(sync, serialize)]
pub fn events_fetch_user_events(user_pubkey: String, limit: i32) -> Result<String, String> {
    let filter = "AND (p.pubkey = ?1 OR EXISTS (SELECT 1 FROM posts r WHERE r.pubkey = ?1 \
                  AND (r.rsvp_event_id = p.id \
                       OR r.id = 'checkin:' || ?1 || ':' || p.id)))";
    let json = super::db::db_query_params(&event_rows_sql(filter, limit), &[user_pubkey.clone()])?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let ids: Vec<String> = rows
        .iter()
        .filter_map(|v| v["id"].as_str().map(|s| s.to_string()))
        .collect();
    let counts = attendee_counts_for_ids(&ids);
    let mut out: Vec<EventInfo> = Vec::new();
    for v in rows {
        if let Some(mut e) = event_from_value(&v) {
            e.attendees = counts.get(&e.id).copied().unwrap_or(0);
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
    // The event is signed with the unlocked signer key, so the claimed
    // creator must match it: otherwise a caller could attribute a stored row
    // to an arbitrary (victim) pubkey while the signed event carries a
    // different one.
    super::signer::require_identity(&creator_pubkey)?;
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
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_EVENT),
        content.to_string(),
    )
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
    super::db::upsert_post_row(
        event_id.clone(),
        creator_pubkey,
        content.to_string(),
        KIND_EVENT as i64,
        soshal_common_core::format::now_secs(),
        String::new(),
        Some(title),
    )?;
    super::db::db_execute_params(
        "UPDATE posts SET event_lat = ?1, event_lng = ?2 WHERE id = ?3",
        &[latitude.to_string(), longitude.to_string(), event_id],
    )?;
    Ok(signed_json).into()
}

/// Single-row fetch of the event host pubkey + d-tag (avoids the
/// events_get_event + tags_json double query).
fn event_host_and_d_tag(event_id: &str) -> Option<(String, String)> {
    let json = super::db::db_query_params(
        &format!("SELECT pubkey, tags_json FROM posts WHERE kind IN ({EVENT_KINDS}) AND id = ?1"),
        &[event_id.to_string()],
    )
    .ok()?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).ok()?;
    let row = rows.first()?;
    let host = row["pubkey"].as_str().unwrap_or("").to_string();
    let d_tag =
        serde_json::from_str::<Vec<Vec<String>>>(row["tags_json"].as_str().unwrap_or_default())
            .ok()
            .and_then(|t| {
                t.into_iter()
                    .find(|t| t.first().map(|k| k == "d").unwrap_or(false))
                    .and_then(|t| t.get(1).cloned())
            })
            .unwrap_or_default();
    Some((host, d_tag))
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
    super::signer::require_identity(&user_pubkey)?;
    if !valid_rsvp(&rsvp_status) {
        return Err("rsvp_status must be accepted/declined/pending".to_string()).into();
    }
    let (host, event_d) = match event_host_and_d_tag(&event_id) {
        Some(v) => v,
        None => return Err("event not found locally (sync relays first)".to_string()).into(),
    };
    let d_tag = if event_d.is_empty() {
        format!("{}-{}", host.get(..12).unwrap_or(""), 0)
    } else {
        event_d
    };
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_EVENT_RSVP),
        rsvp_status.clone(),
    )
    .tags(
        vec![
            vec!["a".to_string(), format!("{KIND_EVENT_RSVP}:{host}:{d_tag}")],
            vec!["e".to_string(), event_id.clone()],
        ]
        .into_iter()
        .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let signed_id = signed["id"].as_str().unwrap_or_default().to_string();
    let now = soshal_common_core::format::now_secs();
    super::db::with_db_result(|db| {
        soshal_sync_core::outbox::enqueue_outbox_item(
            db,
            &signed_id,
            "rsvp",
            &signed_json,
            None,
            now,
        )
        .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(())
    })?;
    // RSVP state row keyed by (user, EVENT): a status-only id (`rsvp:u:going`)
    // would collide across events — event B's RSVP would overwrite event A's
    // row and stale accepted rows would double-count attendees.
    let rsvp_id = format!("rsvp:{}:{}:{}", user_pubkey, event_id, rsvp_status);
    // Clear any previous status row for this (user, event) so a changed RSVP
    // cannot leave a stale row (double-counts) behind.
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let _ = soshal_db_core::block_on(async {
            let _ = conn
                .execute(
                    "DELETE FROM posts WHERE id LIKE ?1 AND id != ?2",
                    libsql::params!(format!("rsvp:{user_pubkey}:{event_id}:%"), rsvp_id.clone()),
                )
                .await;
            Ok::<(), soshal_db_core::error::DbError>(())
        });
        Ok(())
    })?;
    super::db::upsert_post_row(
        rsvp_id,
        user_pubkey,
        rsvp_status,
        KIND_EVENT_RSVP as i64,
        soshal_common_core::format::now_secs(),
        format!(r#"[["a","{KIND_EVENT_RSVP}:{host}:{d_tag}"],["e","{event_id}"]]"#),
        None,
    )
    .map(|_| true)
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
    super::signer::require_identity(&user_pubkey)?;
    let event: EventInfo = serde_json::from_str(&events_get_event(event_id.clone())?)
        .map_err(|e| format!("parse event: {e}"))?;
    let now = soshal_common_core::format::now_secs() as u64;
    let window = if event.start_time == 0 {
        (now.saturating_sub(2 * 3600), now.saturating_add(2 * 3600))
    } else {
        let end = if event.end_time > event.start_time {
            event.end_time
        } else {
            event.start_time.saturating_add(4 * 3600)
        };
        (event.start_time.saturating_sub(3600), end)
    };
    if !can_checkin(window.0, window.1, now, 0) {
        return Err("check-in outside the event window".to_string()).into();
    }
    if event.latitude != 0.0
        && event.longitude != 0.0
        && !within_checkin_radius(event.latitude, event.longitude, latitude, longitude, 500.0)
    {
        return Err("check-in too far from the event location".to_string()).into();
    }
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
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let signed_id = signed["id"].as_str().unwrap_or_default().to_string();
    let now = soshal_common_core::format::now_secs();
    super::db::with_db_result(|db| {
        soshal_sync_core::outbox::enqueue_outbox_item(
            db,
            &signed_id,
            "checkin",
            &signed_json,
            None,
            now,
        )
        .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(())
    })?;
    super::db::upsert_post_row(
        format!("checkin:{}:{}", user_pubkey, event_id),
        user_pubkey.clone(),
        content,
        9,
        soshal_common_core::format::now_secs(),
        format!(
            r#"[["d","badge-{event_id}"],["e","{event_id}"],["p","{user_pubkey}"]]"#,
            event_id = event_id.clone(),
            user_pubkey = user_pubkey.clone()
        ),
        None,
    )
    .map(|_| true)
}

/// Event details: includes local RSVP + attendee counts.
#[frb(sync, serialize)]
pub fn events_get_event(event_id: String) -> Result<String, String> {
    let json = super::db::db_query_params(
        &format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json FROM posts p \
             WHERE p.kind IN ({EVENT_KINDS}) AND p.id = ?1"
        ),
        &[event_id.clone()],
    )?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let v = rows.first().ok_or("Event not found".to_string())?;
    let mut info = event_from_value(v).ok_or("Event not found".to_string())?;
    info.attendees = attendees_count(&event_id);
    super::util::json_ok(info)
}

/// Attendees = distinct pubkeys with an `accepted` RSVP (kind 31924).
/// Uses the denormalized `rsvp_event_id` column (v011) — indexed lookup,
/// no tags_json LIKE scan.
#[frb(sync, serialize)]
pub fn events_get_attendees(event_id: String) -> Result<Vec<String>, String> {
    let json = super::db::db_query_params(
        &format!(
            "SELECT DISTINCT pubkey FROM posts WHERE kind = {KIND_EVENT_RSVP} AND content = 'accepted' \
             AND rsvp_event_id = ?1 ORDER BY created_at DESC LIMIT 200"
        ),
        &[event_id],
    )?;
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
        let d = soshal_spatial_core::distance::haversine_km(37.7749, -122.4194, 34.0522, -118.2437);
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

    #[test]
    fn test_events_create_validation() {
        assert!(events_create(
            "pk".into(),
            "x".repeat(301),
            String::new(),
            String::new(),
            0.0,
            0.0,
            100,
            200,
            String::new()
        )
        .is_err());
        assert!(events_create(
            "pk".into(),
            String::new(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            100,
            200,
            String::new()
        )
        .is_err());
        assert!(events_create(
            "pk".into(),
            "t".into(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            0,
            200,
            String::new()
        )
        .is_err());
        assert!(events_create(
            "pk".into(),
            "t".into(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            200,
            100,
            String::new()
        )
        .is_err());
        assert!(events_create(
            "pk".into(),
            "t".into(),
            String::new(),
            String::new(),
            0.0,
            0.0,
            100,
            100 + 8 * 24 * 3600,
            String::new()
        )
        .is_err());
    }

    #[test]
    fn test_events_missing_event_paths() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        super::super::db::tmp_db("events_missing", "evt");
        assert!(events_rsvp("nonexistent".into(), "pk".into(), "accepted".into()).is_err());
        assert!(events_check_in("nonexistent".into(), "pk".into(), 0.0, 0.0).is_err());
        assert!(events_get_event("nonexistent".into()).is_err());
        assert!(events_get_attendees("nonexistent".into())
            .unwrap()
            .is_empty());
        assert_eq!(events_fetch_user_events("pk".into(), 10).unwrap(), "[]");
        assert!(events_reminder_upsert("r".into(), "e".into(), "t".into(), 1, -1).is_err());
    }

    #[test]
    fn test_events_full_flow() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        super::super::db::tmp_db("events_flow", "evt");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let now = soshal_common_core::format::now_secs() as u64;
        let created = events_create(
            pk.clone(),
            "Test Event".into(),
            "desc".into(),
            "HQ".into(),
            0.0,
            0.0,
            now - 3600,
            now + 3600,
            String::new(),
        )
        .unwrap();
        let created_id: serde_json::Value = serde_json::from_str(&created).unwrap();
        let event_id = created_id["id"].as_str().unwrap().to_string();

        assert!(events_rsvp(event_id.clone(), pk.clone(), "accepted".into()).unwrap());
        assert!(events_rsvp(event_id.clone(), pk.clone(), "declined".into()).unwrap());
        // Local RSVP rows are relay-synced; the denormalized rsvp_event_id
        // column is only set by the sync ingest path, so attendee counts
        // stay empty until the RSVP event comes back through sync.
        assert!(events_get_attendees(event_id.clone()).unwrap().is_empty());

        let detail: EventInfo =
            serde_json::from_str(&events_get_event(event_id.clone()).unwrap()).unwrap();
        assert_eq!(detail.title, "Test Event");
        assert_eq!(detail.attendees, 0);

        assert!(events_check_in(event_id.clone(), pk.clone(), 0.0, 0.0).unwrap());

        let mine: Vec<serde_json::Value> =
            serde_json::from_str(&events_fetch_user_events(pk, 10).unwrap()).unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0]["id"].as_str().unwrap(), event_id);

        let anywhere: Vec<serde_json::Value> =
            serde_json::from_str(&events_fetch_nearby(0.0, 0.0, 0.0, 10).unwrap()).unwrap();
        assert_eq!(
            anywhere.len(),
            1,
            "radius 0 = anywhere includes zero-coordinate events"
        );
        assert_eq!(anywhere[0]["id"].as_str().unwrap(), event_id);

        let score = events_interest_score(r#"["nostr"]"#.into(), r#"["nostr"]"#.into()).unwrap();
        assert!(score.contains(':'));
        let scored = events_score_events(
            r#"[{"id":"e1","title":"nostr meetup","description":""}]"#.into(),
            r#"["nostr"]"#.into(),
        )
        .unwrap();
        assert!(scored.contains("e1"));

        let rid =
            events_reminder_upsert(String::new(), event_id, "remind".into(), 123, 30).unwrap();
        assert!(!rid.is_empty());
        assert!(events_reminders_list().unwrap().contains(&rid));
        assert!(events_reminder_delete(rid.clone()).unwrap());
        assert!(!events_reminders_list().unwrap().contains(&rid));
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

/// Interest score between my interests and a peer's (JSON: score/common).
#[frb(sync, serialize)]
pub fn events_interest_score(
    my_interests_json: String,
    peer_interests_json: String,
) -> Result<String, String> {
    let score = soshal_events_core::event::interest::compute_interest_score_json(&format!(
        r#"{{"myInterests":{my_interests_json},"peerInterests":{peer_interests_json}}}"#
    ));
    Ok(score)
}

/// Batch interest scoring: extract hashtags from each event's title+summary
/// and score them against my interests in ONE FFI call, returning
/// `{"<event_id>": score}`. Replaces 2×N per-event FFI round-trips in the
/// events screen.
#[frb(sync, serialize)]
pub fn events_score_events(
    events_json: String,
    my_interests_json: String,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct EventText {
        id: String,
        title: String,
        description: String,
    }
    let events: Vec<EventText> =
        serde_json::from_str(&events_json).map_err(|e| format!("invalid events JSON: {e}"))?;
    let mut out = serde_json::Map::with_capacity(events.len());
    for ev in events {
        let tags =
            soshal_content_core::hashtag::extract(&format!("{} {}", ev.title, ev.description));
        let score = if tags.is_empty() {
            0.0
        } else {
            let json = soshal_events_core::event::interest::compute_interest_score_json(&format!(
                r#"{{"myInterests":{},"peerInterests":{}}}"#,
                my_interests_json,
                serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()),
            ));
            serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| v["score"].as_f64())
                .unwrap_or(0.0)
        };
        out.insert(ev.id, serde_json::json!(score));
    }
    Ok(serde_json::to_string(&out).unwrap_or_else(|_| "{}".into()))
}
