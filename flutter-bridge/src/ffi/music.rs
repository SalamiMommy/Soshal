//! Musicloud FFI module
//!
//! Music publishing/sharing on nostr: kind-31022 tracks (addressable,
//! `d` = soshal_music_<ts>), kind-1 text-note shares, comments via the
//! minis address scheme. Fetch results are signature-verified and mapped
//! through minis-core.

use flutter_rust_bridge::frb;
use soshal_minis_core::events as minis_events;

fn add_tag(builder: nostr::event::EventBuilder, tag: Vec<String>) -> nostr::event::EventBuilder {
    match nostr::event::Tag::parse(tag) {
        Ok(t) => builder.tag(t),
        Err(_) => builder,
    }
}

/// Publish a track (kind 31022). `audio_url` must be a valid https media
/// URL. Returns the event id.
#[frb(serialize)]
pub async fn music_publish(
    audio_url: String,
    title: Option<String>,
    thumbnail: Option<String>,
    hashtags: Vec<String>,
    audience: Option<String>,
) -> Result<String, String> {
    if audio_url.is_empty() || !soshal_content_core::url::is_valid_media_url(&audio_url) {
        return Err("audio_url must be a valid https media URL".into());
    }
    let aud = audience.unwrap_or_else(|| "public".into());
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(31022), String::new());
    builder = add_tag(
        builder,
        vec![
            "d".to_string(),
            format!("soshal_music_{}", nostr::types::Timestamp::now().as_secs()),
        ],
    );
    builder = add_tag(builder, vec!["url".to_string(), audio_url]);
    if let Some(t) = title {
        if !t.is_empty() {
            builder = add_tag(builder, vec!["title".to_string(), t]);
        }
    }
    if let Some(th) = thumbnail {
        if !th.is_empty() {
            builder = add_tag(builder, vec!["image".to_string(), th]);
        }
    }
    builder = add_tag(builder, vec!["audience".to_string(), aud]);
    for h in hashtags.iter().take(10) {
        let h = h.trim_start_matches('#').to_string();
        if !h.is_empty() {
            builder = add_tag(builder, vec!["t".to_string(), h]);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch tracks (kind 31022), optionally by author. Returns JSON array of
/// musicloud entries, newest first.
#[frb(serialize)]
pub async fn music_fetch(limit: u64, author: Option<String>) -> Result<String, String> {
    let mut filter = serde_json::json!({
        "kinds": [31022],
        "limit": limit.min(100),
    });
    if let Some(a) = author {
        filter["authors"] = serde_json::json!([a]);
    }
    let raw = super::network::network_query_events(filter.to_string()).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = Vec::new();
    for e in events {
        if e.verify().is_err() {
            continue;
        }
        if let Some(mapped) =
            minis_events::musicloud_from_event(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            out.push(mapped);
        }
    }
    minis_events::sort_by_created_desc(&mut out);
    super::util::json_ok(out)
}

/// Share a track as a kind-1 text note with the standard track tags.
#[frb(serialize)]
pub async fn music_share_to_feed(
    track_id: String,
    track_pubkey: String,
    message: String,
    hashtags: Vec<String>,
) -> Result<String, String> {
    let content = message.trim().to_string();
    if content.is_empty() || content.len() > 64000 {
        return Err("message must be 1-64000 chars".into());
    }
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content);
    for tag in [
        vec!["e".to_string(), track_id.clone()],
        vec!["p".to_string(), track_pubkey.clone()],
        vec!["k".to_string(), "31022".to_string()],
        vec!["a".to_string(), format!("31022:{track_pubkey}:{track_id}")],
    ] {
        builder = add_tag(builder, tag);
    }
    for h in hashtags.iter().take(10) {
        let h = h.trim_start_matches('#').to_string();
        if !h.is_empty() {
            builder = add_tag(builder, vec!["t".to_string(), h]);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Publish a comment on a track (kind 1 with `E`/`a` tags to the track).
#[frb(serialize)]
pub async fn music_comment(
    track_kind: u16,
    track_pubkey: String,
    track_d: String,
    content: String,
) -> Result<String, String> {
    let addr = minis_events::musicloud_comment_addr(track_kind, &track_pubkey, &track_d);
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content);
    for tag in [
        vec!["E".to_string(), addr.clone()],
        vec!["a".to_string(), addr],
        vec!["p".to_string(), track_pubkey],
    ] {
        builder = add_tag(builder, tag);
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch comments (kind 1 with `E` = track address) for a track.
/// Returns JSON array of mini event outputs.
#[frb(serialize)]
pub async fn music_comments(
    track_kind: u16,
    track_pubkey: String,
    track_d: String,
) -> Result<String, String> {
    let addr = minis_events::musicloud_comment_addr(track_kind, &track_pubkey, &track_d);
    let filter = serde_json::json!({
        "kinds": [1],
        "#E": [addr.clone()],
        "limit": 100,
    })
    .to_string();
    let raw = super::network::network_query_events(filter).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = Vec::new();
    for e in events {
        if e.verify().is_err() {
            continue;
        }
        if let Some(mapped) =
            minis_events::mini_event_out(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            out.push(mapped);
        }
    }
    minis_events::sort_minis_desc(&mut out);
    super::util::json_ok(out)
}
