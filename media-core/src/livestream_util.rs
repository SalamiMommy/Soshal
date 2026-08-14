use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use soshal_nostr_core::models::NostrEvent;

#[derive(Serialize)]
#[doc(hidden)]
pub struct LiveStream {
    pub id: String,
    pub title: String,
    pub status: String,
    pub category: String,
    #[serde(rename = "startTime")]
    pub start_time: i64,
    pub pubkey: String,
    #[serde(rename = "sfuUrl")]
    pub sfu_url: String,
}

#[doc(hidden)]
pub fn parse_live_streams(
    events: Vec<NostrEvent>,
    category_filter: Option<&str>,
) -> Vec<LiveStream> {
    let mut streams: Vec<LiveStream> = Vec::new();

    for ev in events {
        let id = ev.id;
        let content = ev.content;
        let tags = &ev.tags;
        let created_at = ev.created_at as i64;
        let pubkey = ev.pubkey;

        let mut title = content.to_string();
        let mut status = "live".to_string();
        let mut category = String::new();
        let mut sfu_url = String::new();

        for tag in tags {
            if tag.len() >= 2 {
                match tag[0].as_str() {
                    "title" => title = tag[1].clone(),
                    "status" => status = tag[1].clone(),
                    "category" => category = tag[1].clone(),
                    "sfu" => sfu_url = tag[1].clone(),
                    _ => {}
                }
            }
        }

        if let Some(cat) = category_filter {
            if !cat.is_empty() && category.to_lowercase() != cat.to_lowercase() {
                continue;
            }
        }

        streams.push(LiveStream {
            id,
            title,
            status,
            category,
            start_time: created_at,
            pubkey,
            sfu_url,
        });
    }

    streams.sort_by_key(|s| std::cmp::Reverse(s.start_time));
    streams.retain(|s| s.status == "live");
    streams
}

pub fn parse_live_streams_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct ParseLiveStreamsInput {
        events: Vec<NostrEvent>,
        #[serde(rename = "category")]
        category: Option<String>,
    }

    let Some(input) = json_in::<Option<ParseLiveStreamsInput>>(input, None) else {
        return "[]".to_string();
    };
    let streams = parse_live_streams(input.events, input.category.as_deref());
    json_out(&streams, "[]")
}

#[derive(Deserialize, Serialize, Clone)]
#[doc(hidden)]
pub struct ChatMessage {
    pub id: Option<String>,
    pub pubkey: Option<String>,
    pub content: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
}

#[doc(hidden)]
pub fn merge_chat_messages(
    local: Vec<ChatMessage>,
    relay: Vec<ChatMessage>,
    _stream_id: &str,
) -> Vec<ChatMessage> {
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<ChatMessage> = Vec::new();

    for msg in local.into_iter().chain(relay) {
        let id = msg.id.clone().unwrap_or_default();
        if id.is_empty() || seen.insert(id) {
            merged.push(msg);
        }
    }

    merged.sort_by(|a, b| {
        let a_ts = a.created_at.unwrap_or(0);
        let b_ts = b.created_at.unwrap_or(0);
        a_ts.cmp(&b_ts)
    });

    merged
}

pub fn merge_chat_messages_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct MergeChatInput {
        local: Vec<ChatMessage>,
        relay: Vec<ChatMessage>,
        #[serde(rename = "streamId")]
        stream_id: String,
    }

    let Some(input) = json_in::<Option<MergeChatInput>>(input, None) else {
        return "[]".to_string();
    };
    let merged = merge_chat_messages(input.local, input.relay, &input.stream_id);
    json_out(&merged, "[]")
}
