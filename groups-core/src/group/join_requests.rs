use super::{
    find_tag_values_map, GroupEventInput, MAX_EVENTS, MAX_TAGS_PER_EVENT, MAX_TAG_VALUE_LEN,
};
use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseGroupJoinRequestsInput {
    pub events: Vec<GroupEventInput>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedGroupJoinRequestOut {
    pub pubkey: String,
    pub event_id: String,
}

fn parse_group_join_requests(input: ParseGroupJoinRequestsInput) -> Vec<ParsedGroupJoinRequestOut> {
    if input.events.len() > MAX_EVENTS {
        return Vec::new();
    }
    let mut results = Vec::new();
    for event in &input.events {
        if event.tags.len() > MAX_TAGS_PER_EVENT {
            continue;
        }
        let [req_tag, p_tag] = find_tag_values_map(&event.tags, ["request", "p"]);
        if req_tag.is_some_and(|r| r.trim().eq_ignore_ascii_case("join")) {
            if let Some(pk) = p_tag {
                // SECURITY: a forged `request=join` event must not be able to
                // impersonate another pubkey. The `p` tag must equal the
                // event's own author.
                if pk.eq_ignore_ascii_case(&event.pubkey) && pk.len() <= MAX_TAG_VALUE_LEN {
                    results.push(ParsedGroupJoinRequestOut {
                        pubkey: pk.to_string(),
                        event_id: event.id.clone(),
                    });
                }
            }
        }
    }
    results
}

pub fn parse_group_join_requests_json(input_json: &str) -> String {
    if input_json.len() > 16 * 1024 * 1024 {
        return "[]".to_string();
    }
    let Some(input) = json_in::<Option<ParseGroupJoinRequestsInput>>(input_json, None) else {
        return "[]".to_string();
    };
    json_out(&parse_group_join_requests(input), "[]")
}
