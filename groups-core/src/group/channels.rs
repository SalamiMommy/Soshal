use super::{
    clamp_created_at, safe_truncate, GroupEventInput, MAX_EVENTS, MAX_TAGS_PER_EVENT,
    MAX_TAG_VALUE_LEN,
};
use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
pub struct ParseGroupChannelsInput {
    pub events: Vec<GroupEventInput>,
}

#[derive(Serialize, Deserialize)]
pub struct ParsedGroupChannelOut {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_by: String,
    pub created_at: f64,
    pub category: Option<String>,
    pub channel_type: Option<String>,
    pub position: Option<i64>,
    pub slow_mode_seconds: Option<i64>,
}

fn parse_group_channels(input: ParseGroupChannelsInput) -> Vec<ParsedGroupChannelOut> {
    if input.events.len() > MAX_EVENTS {
        return Vec::new();
    }
    let mut results = Vec::new();
    for event in &input.events {
        if event.tags.len() > MAX_TAGS_PER_EVENT {
            continue;
        }
        let mut d_tag: Option<&str> = None;
        let mut g_tag: Option<&str> = None;
        let mut name_tag: Option<&str> = None;
        let mut desc_tag: Option<&str> = None;
        let mut category_tag: Option<&str> = None;
        let mut type_tag: Option<&str> = None;
        let mut position_tag: Option<&str> = None;
        let mut slow_mode_tag: Option<&str> = None;

        for tag in &event.tags {
            if tag.len() < 2 {
                continue;
            }
            match tag[0].as_str() {
                "d" if d_tag.is_none() => d_tag = Some(&tag[1]),
                "g" if g_tag.is_none() => g_tag = Some(&tag[1]),
                "name" if name_tag.is_none() => name_tag = Some(&tag[1]),
                "description" if desc_tag.is_none() => desc_tag = Some(&tag[1]),
                "category" if category_tag.is_none() => category_tag = Some(&tag[1]),
                "type" if type_tag.is_none() => type_tag = Some(&tag[1]),
                "position" if position_tag.is_none() => position_tag = Some(&tag[1]),
                "slow_mode_seconds" if slow_mode_tag.is_none() => slow_mode_tag = Some(&tag[1]),
                _ => {}
            }
        }
        if let (Some(id), Some(gid), Some(name)) = (d_tag, g_tag, name_tag) {
            if id.len() > MAX_TAG_VALUE_LEN
                || gid.len() > MAX_TAG_VALUE_LEN
                || name.len() > MAX_TAG_VALUE_LEN
            {
                continue;
            }
            results.push(ParsedGroupChannelOut {
                id: id.to_string(),
                group_id: gid.to_string(),
                name: name.to_string(),
                description: desc_tag.map(|s| safe_truncate(s, MAX_TAG_VALUE_LEN)),
                created_by: event.pubkey.clone(),
                created_at: clamp_created_at(event.created_at) * 1000.0,
                category: category_tag.map(|s| safe_truncate(s, MAX_TAG_VALUE_LEN)),
                channel_type: type_tag.map(|s| safe_truncate(s, MAX_TAG_VALUE_LEN)),
                position: position_tag
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|n| *n >= -1_000_000_000 && *n <= 1_000_000_000),
                slow_mode_seconds: slow_mode_tag
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|n| *n >= 0 && *n <= 86_400),
            });
        }
    }
    results
}

pub fn parse_group_channels_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<ParseGroupChannelsInput>>(input_json, None) else {
        return "[]".to_string();
    };
    json_out(&parse_group_channels(input), "[]")
}
