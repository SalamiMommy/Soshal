use super::{find_tag_values_map, safe_truncate, GroupEventInput, MAX_EVENTS, MAX_TAG_VALUE_LEN};
use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
pub struct ParseGroupsInput {
    pub events: Vec<GroupEventInput>,
    pub self_pubkey: String,
}

#[derive(Serialize, Deserialize)]
pub struct ParsedGroupOut {
    pub id: String,
    pub name: String,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub created_by: String,
    pub created_at: f64,
    pub is_owner: bool,
    pub audience: Option<String>,
}

fn parse_groups(input: ParseGroupsInput) -> Vec<ParsedGroupOut> {
    if input.events.len() > MAX_EVENTS {
        return Vec::new();
    }
    let mut results = Vec::new();
    for event in &input.events {
        if event.tags.len() > 100_000 {
            continue;
        }
        let [d_tag, name_tag, audience_tag_str] =
            find_tag_values_map(&event.tags, ["d", "name", "audience"]);
        if let (Some(id), Some(name)) = (d_tag, name_tag) {
            if id.len() > MAX_TAG_VALUE_LEN || name.len() > MAX_TAG_VALUE_LEN {
                continue;
            }
            let mut about: Option<String> = None;
            let mut picture: Option<String> = None;
            let mut audience: Option<String> = None;
            if event.content.len() <= 64 * 1024 {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&event.content) {
                    about = parsed
                        .get("about")
                        .and_then(|v| v.as_str())
                        .map(|s| safe_truncate(s, MAX_TAG_VALUE_LEN));
                    picture = parsed
                        .get("picture")
                        .and_then(|v| v.as_str())
                        .map(|s| safe_truncate(s, MAX_TAG_VALUE_LEN));
                    audience = parsed
                        .get("audience")
                        .and_then(|v| v.as_str())
                        .map(|s| safe_truncate(s, MAX_TAG_VALUE_LEN));
                }
            }
            if audience.is_none() {
                audience = audience_tag_str.map(|s| s.to_string());
            }
            results.push(ParsedGroupOut {
                id: id.to_string(),
                name: name.to_string(),
                about,
                picture,
                created_by: event.pubkey.clone(),
                created_at: event.created_at * 1000.0,
                is_owner: event.pubkey == input.self_pubkey,
                audience,
            });
        }
    }
    results
}

pub fn parse_groups_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<ParseGroupsInput>>(input_json, None) else {
        return "[]".to_string();
    };
    json_out(&parse_groups(input), "[]")
}
