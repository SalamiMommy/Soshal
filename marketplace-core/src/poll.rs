use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

const MAX_TAGS_ENTRIES: usize = 100_000;
const MAX_POLL_OPTIONS: usize = 100;
const POLL_EXPIRY_DEFAULT: f64 = 604800.0;

#[derive(Deserialize)]
pub(crate) struct CalendarEventInput {
    id: String,
    pubkey: String,
    content: String,
    created_at: f64,
    #[serde(default)]
    tags: Vec<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PollOptionOut {
    pub id: i64,
    pub text: String,
}

#[derive(Serialize)]
pub struct PollOut {
    pub id: String,
    pub pubkey: String,
    pub question: String,
    pub options: Vec<PollOptionOut>,
    #[serde(rename = "expiresAt")]
    pub expires_at: f64,
    pub closed: bool,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
}

#[derive(Deserialize)]
struct PollContent {
    question: Option<String>,
    options: Option<Vec<PollOptionOut>>,
}

use soshal_nostr_core::models::find_tag_value;

pub(crate) fn parse_poll_event(ev: &CalendarEventInput, now_ms: f64) -> Option<PollOut> {
    if ev.tags.len() > MAX_TAGS_ENTRIES {
        return None;
    }
    let question: String;
    let mut options: Vec<PollOptionOut> = Vec::new();
    if ev.content.len() <= 256 * 1024 {
        match serde_json::from_str::<PollContent>(&ev.content) {
            Ok(content) => {
                question = content.question.unwrap_or_default();
                if let Some(opts) = content.options {
                    options.extend(opts.into_iter().take(MAX_POLL_OPTIONS));
                }
            }
            Err(_) => {
                question = ev.content.clone();
            }
        }
    } else {
        question = String::new();
    }
    if options.is_empty() {
        for tag in &ev.tags {
            if tag.len() >= 3 && tag[0] == "poll_option" {
                let id = tag[1].parse::<i64>().unwrap_or(0);
                options.push(PollOptionOut {
                    id,
                    text: tag[2].clone(),
                });
                if options.len() >= MAX_POLL_OPTIONS {
                    break;
                }
            }
        }
    }
    let exp_tag = find_tag_value(&ev.tags, "expiration");
    let expires_at = match exp_tag {
        Some(s) if s.len() <= 32 => match s.parse::<f64>() {
            Ok(v) if v.is_finite() => v * 1000.0,
            _ => now_ms + POLL_EXPIRY_DEFAULT * 1000.0,
        },
        _ => now_ms + POLL_EXPIRY_DEFAULT * 1000.0,
    };
    let closed = now_ms > expires_at;
    Some(PollOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        question,
        options,
        expires_at,
        closed,
        created_at: ev.created_at * 1000.0,
    })
}

#[derive(Deserialize)]
struct PollInput {
    #[serde(rename = "event")]
    ev: CalendarEventInput,
    #[serde(rename = "nowMs")]
    now_ms: f64,
}

pub fn parse_poll_event_json(input: &str) -> String {
    let Some(parsed) = json_in::<Option<PollInput>>(input, None) else {
        return "null".to_string();
    };
    json_out(&parse_poll_event(&parsed.ev, parsed.now_ms), "null")
}
