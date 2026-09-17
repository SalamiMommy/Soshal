use serde::{Deserialize, Serialize};

use crate::json_util::{json_in, json_out};

const MAX_POSTS: usize = 100_000;

use crate::linkpreview::urls::MAX_PREVIEW_URL_LENGTH;
use crate::tags::find_tag_value;

#[derive(Deserialize, Clone)]
struct StoryEventInput {
    id: String,
    pubkey: String,
    content: String,
    created_at: f64,
    tags: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct ProcessStoriesInput {
    events: Vec<StoryEventInput>,
    now_sec: f64,
    expiry_seconds: f64,
}

#[derive(Serialize, Deserialize)]
struct ProcessedStoryOut {
    id: String,
    pubkey: String,
    media: Vec<MediaItemOut>,
    text: Option<String>,
    created_at_ms: f64,
    expires_at_ms: f64,
}

#[derive(Serialize, Deserialize)]
struct MediaItemOut {
    url: String,
    #[serde(rename = "type")]
    media_type: String,
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct StoryContentParsed {
    media: Option<Vec<MediaItemParsed>>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct MediaItemParsed {
    url: String,
    #[serde(rename = "type")]
    media_type: String,
    duration: Option<f64>,
}

fn process_stories(input: ProcessStoriesInput) -> Vec<ProcessedStoryOut> {
    if input.events.len() > MAX_POSTS {
        return Vec::new();
    }
    let now_sec = if input.now_sec.is_finite() && input.now_sec >= 0.0 {
        input.now_sec
    } else {
        0.0
    };
    let expiry_seconds = if input.expiry_seconds.is_finite() && input.expiry_seconds >= 0.0 {
        input.expiry_seconds
    } else {
        86400.0
    };
    let mut results = Vec::with_capacity(input.events.len().min(MAX_POSTS));
    for event in input.events {
        if event.content.len() > 1024 * 1024 {
            continue;
        }
        let created_at = if event.created_at.is_finite() && event.created_at >= 0.0 {
            event.created_at
        } else {
            0.0
        };
        let expires_at = find_tag_value(&event.tags, "expiration")
            .or_else(|| find_tag_value(&event.tags, "expires_at"))
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);
        let actual_expiry = expires_at.unwrap_or(created_at + expiry_seconds);
        if actual_expiry < now_sec {
            continue;
        }
        let (content_media, text) = match serde_json::from_str::<StoryContentParsed>(&event.content)
        {
            Ok(parsed) => (parsed.media.unwrap_or_default(), parsed.text),
            Err(_) => (Vec::new(), None),
        };
        let mut media: Vec<MediaItemOut> = Vec::with_capacity(content_media.len());
        for m in content_media {
            if m.url.len() <= MAX_PREVIEW_URL_LENGTH
                && m.media_type.len() <= 100
                && crate::url::is_valid_media_url(&m.url)
            {
                let duration = m.duration.filter(|d| d.is_finite() && *d >= 0.0);
                media.push(MediaItemOut {
                    url: m.url,
                    media_type: m.media_type,
                    duration,
                });
            }
        }
        results.push(ProcessedStoryOut {
            id: event.id.clone(),
            pubkey: event.pubkey.clone(),
            media,
            text,
            created_at_ms: created_at * 1000.0,
            expires_at_ms: actual_expiry * 1000.0,
        });
    }
    results.sort_by(|a, b| {
        a.created_at_ms
            .partial_cmp(&b.created_at_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

pub fn filter_stories_json(input: &str) -> String {
    let parsed = json_in(
        input,
        ProcessStoriesInput {
            events: Vec::new(),
            now_sec: 0.0,
            expiry_seconds: 0.0,
        },
    );
    let out = process_stories(parsed);
    json_out(&out, "[]")
}
