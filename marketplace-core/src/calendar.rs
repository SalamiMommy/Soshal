use serde::{Deserialize, Serialize};
use soshal_common_core::consts::MAX_TAG_VALUE_LEN;
use soshal_common_core::json_util::{json_in, json_out};

const MAX_TAGS_ENTRIES: usize = 100_000;

#[derive(Deserialize)]
struct CalendarEventInput {
    id: String,
    pubkey: String,
    content: String,
    created_at: f64,
    #[serde(default)]
    tags: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct CalendarContent {
    description: Option<String>,
    image: Option<String>,
    videos: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct CalendarEventOut {
    pub id: String,
    pub pubkey: String,
    #[serde(rename = "dTag")]
    pub d_tag: String,
    pub title: String,
    #[serde(rename = "startTime")]
    pub start_time: f64,
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<String>>,
    pub participants: Vec<String>,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
}

use soshal_nostr_core::models::find_tag_value;

fn parse_calendar_event(ev: &CalendarEventInput) -> Option<CalendarEventOut> {
    if ev.tags.len() > MAX_TAGS_ENTRIES {
        return None;
    }
    let d_tag = find_tag_value(&ev.tags, "d")
        .map(|s| s.to_string())
        .unwrap_or_else(|| ev.id.chars().take(12).collect::<String>());
    let title = find_tag_value(&ev.tags, "title")
        .unwrap_or("Untitled Event")
        .to_string();
    let start_str = find_tag_value(&ev.tags, "start")?;
    if start_str.len() > 32 {
        return None;
    }
    let start_time = match start_str.parse::<f64>() {
        Ok(t) if t.is_finite() => t,
        _ => return None,
    };
    let end_time = find_tag_value(&ev.tags, "end")
        .and_then(|s| {
            if s.len() > 32 {
                None
            } else {
                s.parse::<f64>().ok()
            }
        })
        .filter(|t| t.is_finite());
    let location = find_tag_value(&ev.tags, "location").map(|s| s.to_string());
    let mut description: Option<String> = None;
    let mut image: Option<String> = None;
    let mut videos: Vec<String> = Vec::new();
    if ev.content.len() <= 256 * 1024 {
        if let Ok(content) = serde_json::from_str::<CalendarContent>(&ev.content) {
            description = content.description;
            image = content.image;
            if let Some(v) = content.videos {
                videos.extend(v);
            }
        } else if !ev.content.is_empty() {
            description = Some(ev.content.clone());
        }
    }
    for tag in &ev.tags {
        if tag.len() >= 2 && tag[0] == "video" && tag[1].len() <= MAX_TAG_VALUE_LEN {
            videos.push(tag[1].clone());
        }
        if videos.len() > 1024 {
            break;
        }
    }
    let imeta_videos = soshal_content_core::linkpreview::imeta::extract_imeta_video_urls(&ev.tags);
    videos.extend(imeta_videos);
    let mut participants: Vec<String> = Vec::new();
    for tag in &ev.tags {
        if tag.len() >= 2 && tag[0] == "p" {
            participants.push(tag[1].clone());
        }
        if participants.len() > 10_000 {
            break;
        }
    }
    Some(CalendarEventOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        d_tag,
        title,
        start_time,
        end_time,
        location,
        description,
        image,
        videos: if videos.is_empty() {
            None
        } else {
            Some(videos)
        },
        participants,
        created_at: ev.created_at,
    })
}

pub fn parse_calendar_event_json(input: &str) -> String {
    let Some(ev) = json_in::<Option<CalendarEventInput>>(input, None) else {
        return "null".to_string();
    };
    json_out(&parse_calendar_event(&ev), "null")
}
