use super::{find_tag_values_map, GroupEventInput, MAX_EVENTS, MAX_TAG_VALUE_LEN};
use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
pub struct ParseGroupChatMessagesInput {
    pub events: Vec<GroupEventInput>,
    pub group_id: String,
    pub channel_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ParsedGroupChatMessageOut {
    pub id: String,
    pub group_id: String,
    pub channel_id: Option<String>,
    pub pubkey: String,
    pub content: String,
    pub created_at: f64,
    pub image_url: Option<String>,
    pub video_url: Option<String>,
}

fn parse_group_chat_messages(input: ParseGroupChatMessagesInput) -> Vec<ParsedGroupChatMessageOut> {
    if input.events.len() > MAX_EVENTS {
        return Vec::new();
    }
    if input.group_id.len() > MAX_TAG_VALUE_LEN {
        return Vec::new();
    }
    if let Some(ref ch) = input.channel_id {
        if ch.len() > MAX_TAG_VALUE_LEN {
            return Vec::new();
        }
    }
    let mut results = Vec::with_capacity(input.events.len().min(MAX_EVENTS));
    for event in &input.events {
        if event.tags.len() > 100_000 {
            continue;
        }
        let [d_tag, event_channel, image_url_str, video_url_str] =
            find_tag_values_map(&event.tags, ["d", "h", "image", "video"]);
        if d_tag.map(|d| d != input.group_id.as_str()).unwrap_or(true) {
            continue;
        }
        if let Some(ref filter_ch) = input.channel_id {
            if event_channel
                .map(|ec| ec != filter_ch.as_str())
                .unwrap_or(true)
            {
                continue;
            }
        }
        let image_url = image_url_str
            .filter(|s| s.len() <= MAX_TAG_VALUE_LEN)
            .map(|s| s.to_string());
        let video_url = video_url_str
            .filter(|s| s.len() <= MAX_TAG_VALUE_LEN)
            .map(|s| s.to_string());
        results.push(ParsedGroupChatMessageOut {
            id: event.id.clone(),
            group_id: input.group_id.clone(),
            channel_id: event_channel.map(|s| s.to_string()),
            pubkey: event.pubkey.clone(),
            content: event.content.clone(),
            created_at: event.created_at * 1000.0,
            image_url,
            video_url,
        });
    }
    results
}

pub fn parse_group_chat_messages_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<ParseGroupChatMessagesInput>>(input_json, None) else {
        return "[]".to_string();
    };
    json_out(&parse_group_chat_messages(input), "[]")
}
