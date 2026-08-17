//! Feed query helpers: event-to-JSON mapping, feed buffer slicing, and
//! reaction/reply-map aggregation.

use std::collections::HashMap;

use soshal_nostr_core::models::NostrEvent;

/// Maps a timeline event to `{id, pubkey, content, created_at}`.
pub fn timeline_entry_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
    })
}

/// Maps an event to `{id, pubkey, content, created_at, tags}`.
pub fn event_with_tags_from_event(ev: &NostrEvent) -> serde_json::Value {
    let mut v = timeline_entry_from_event(ev);
    v["tags"] = serde_json::json!(ev.tags);
    v
}

/// Media-JSON decode cache. Post rows are immutable, so the parse result for
/// a given `tags_json` never goes stale; the same posts are re-fetched across
/// feed pages/threads/refreshes, and this avoids re-parsing + re-serializing
/// per row per fetch.
static MEDIA_JSON_CACHE: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<String, Option<String>>>,
> = std::sync::OnceLock::new();

const MEDIA_JSON_CACHE_CAP: usize = 1024;

fn cached_media_json(tags_json: &str) -> Option<Option<String>> {
    let cache = MEDIA_JSON_CACHE.get_or_init(Default::default);
    cache.read().ok()?.get(tags_json).cloned()
}

fn store_media_json(tags_json: &str, parsed: &Option<String>) {
    if let Ok(mut cache) = MEDIA_JSON_CACHE.get_or_init(Default::default).write() {
        if cache.len() >= MEDIA_JSON_CACHE_CAP {
            cache.clear();
        }
        cache.insert(tags_json.to_string(), parsed.clone());
    }
}

/// Extracts a `["media", type, url, blob_hash, size]` tag from a post's
/// `tags_json` and renders it as `{"type","url","blob_hash","size"}`.
/// The media tag is the LAN blob-sharing wire format: `url` is a
/// `blob://<hash>` placeholder (peers crawl by hash), `blob_hash` is the
/// BLAKE3 manifest hash. Malformed tags yield `None` (hostile-input safe).
pub fn media_json_from_tags(tags_json: &str) -> Option<String> {
    if let Some(cached) = cached_media_json(tags_json) {
        return cached;
    }
    let parsed = parse_media_json(tags_json);
    store_media_json(tags_json, &parsed);
    parsed
}

fn parse_media_json(tags_json: &str) -> Option<String> {
    let tags: Vec<Vec<String>> = serde_json::from_str(tags_json).ok()?;
    for tag in tags {
        if tag.first().map(|s| s.as_str()) != Some("media") {
            continue;
        }
        if tag.len() < 5 {
            return None;
        }
        let media_type = tag[1].clone();
        if !matches!(media_type.as_str(), "image" | "video" | "audio") {
            return None;
        }
        let url = tag[2].clone();
        let blob_hash = tag[3].clone();
        if blob_hash.len() != 64 || !blob_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let size: u64 = tag[4].parse().unwrap_or(0);
        return Some(
            serde_json::json!({
                "type": media_type,
                "url": url,
                "blob_hash": blob_hash,
                "size": size,
            })
            .to_string(),
        );
    }
    None
}

/// Splits the feed buffer at `since_id` (exclusive) into pending events and
/// the pre-split total. Trims the remaining buffer to `max` items.
pub fn split_feed_buffer(
    events: &mut std::collections::VecDeque<serde_json::Value>,
    since_id: &str,
    max: usize,
) -> (Vec<serde_json::Value>, usize) {
    let total = events.len();
    let start = if since_id.is_empty() {
        0
    } else {
        events
            .iter()
            .position(|e| e["id"].as_str() == Some(since_id))
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    let pending = if start >= total {
        vec![]
    } else {
        events.split_off(start).into_iter().collect()
    };
    while events.len() > max {
        events.pop_front();
    }
    (pending, total)
}

#[derive(Default)]
struct ReactionCounts {
    count: u64,
    emojis: HashMap<String, u64>,
}

/// Aggregates kind-7 reaction events into a per-target map
/// `{"count": n, "emojis": {emoji: n}}`. Reaction target is the `e` tag.
pub fn aggregate_reaction_map(events: &[NostrEvent]) -> serde_json::Map<String, serde_json::Value> {
    let mut counts_map: HashMap<String, ReactionCounts> = HashMap::new();
    for ev in events {
        for tag in &ev.tags {
            if tag.first().map(|s| s.as_str()) == Some("e") {
                let Some(target) = tag.get(1) else {
                    continue;
                };
                let entry = counts_map.entry(target.clone()).or_default();
                entry.count += 1;
                let emoji = if ev.content.is_empty() {
                    "+"
                } else {
                    ev.content.as_str()
                };
                *entry.emojis.entry(emoji.to_string()).or_default() += 1;
            }
        }
    }
    let mut map = serde_json::Map::with_capacity(counts_map.len());
    for (target, counts) in counts_map {
        let mut emojis_map = serde_json::Map::with_capacity(counts.emojis.len());
        for (emoji, cnt) in counts.emojis {
            emojis_map.insert(emoji, serde_json::json!(cnt));
        }
        map.insert(
            target,
            serde_json::json!({
                "count": counts.count,
                "emojis": emojis_map
            }),
        );
    }
    map
}

/// Counts kind-1 reply events per target post, keyed by the first `e` tag.
/// Each reply counts once against its referenced post, so deep-thread replies
/// inflate their immediate parent rather than the root.
pub fn aggregate_reply_map(events: &[NostrEvent]) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for ev in events {
        if ev.kind != 1 {
            continue;
        }
        if let Some(target) = ev.find_tag("e") {
            *map.entry(target.to_string()).or_insert(0) += 1;
        }
    }
    map
}
