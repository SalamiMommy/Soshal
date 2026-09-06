use super::{GroupEventInput, MAX_EVENTS, MAX_TAGS_PER_EVENT, MAX_TAG_VALUE_LEN};
use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseGroupPostsInput {
    pub events: Vec<GroupEventInput>,
    #[serde(alias = "group_id")]
    pub group_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedGroupPostOut {
    pub id: String,
    pub group_id: String,
    pub channel_id: Option<String>,
    pub pubkey: String,
    pub content: String,
    pub images: Vec<String>,
    pub videos: Vec<String>,
    pub created_at: f64,
}

fn parse_group_posts(input: ParseGroupPostsInput) -> Vec<ParsedGroupPostOut> {
    if input.events.len() > MAX_EVENTS {
        return Vec::new();
    }
    if input.group_id.len() > MAX_TAG_VALUE_LEN {
        return Vec::new();
    }
    let mut results = Vec::new();
    for event in &input.events {
        if event.tags.len() > MAX_TAGS_PER_EVENT {
            continue;
        }
        let mut d_tag: Option<&str> = None;
        let mut channel_id: Option<&str> = None;
        for tag in &event.tags {
            if tag.len() < 2 {
                continue;
            }
            match tag[0].as_str() {
                "d" if d_tag.is_none() => d_tag = Some(&tag[1]),
                "h" if channel_id.is_none() => channel_id = Some(&tag[1]),
                _ => {}
            }
        }
        if d_tag.map(|d| d != input.group_id.as_str()).unwrap_or(true) {
            continue;
        }

        let mut images: Vec<String> = Vec::new();
        let mut videos: Vec<String> = Vec::new();
        for tag in &event.tags {
            if tag.len() < 2 {
                continue;
            }
            match tag[0].as_str() {
                "image" => {
                    if tag[1].len() <= MAX_TAG_VALUE_LEN && images.len() < 1024 {
                        images.push(tag[1].clone());
                    }
                }
                "imeta" => {
                    let mut url: Option<String> = None;
                    let mut mime: Option<String> = None;
                    for entry in tag {
                        if let Some(rest) = entry.strip_prefix("url=") {
                            if rest.len() <= MAX_TAG_VALUE_LEN {
                                url = Some(rest.to_string());
                            }
                        } else if let Some(rest) = entry.strip_prefix("m=") {
                            if rest.len() <= 100 {
                                mime = Some(rest.to_string());
                            }
                        }
                    }
                    match (url, mime) {
                        (Some(u), Some(m))
                            if (m.contains("video") || m.contains("gif"))
                                && videos.len() < 1024 =>
                        {
                            videos.push(u);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        let channel_id = channel_id.map(|s| s.to_string());
        results.push(ParsedGroupPostOut {
            id: event.id.clone(),
            group_id: input.group_id.clone(),
            channel_id,
            pubkey: event.pubkey.clone(),
            content: event.content.clone(),
            images,
            videos,
            created_at: event.created_at * 1000.0,
        });
    }
    results
}

pub fn parse_group_posts_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<ParseGroupPostsInput>>(input_json, None) else {
        return "[]".to_string();
    };
    json_out(&parse_group_posts(input), "[]")
}
