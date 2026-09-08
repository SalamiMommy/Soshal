use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::json_out;

const MAX_TAGS_ENTRIES: usize = 100_000;
const MAX_POLL_OPTIONS: usize = 100;
const POLL_EXPIRY_DEFAULT: f64 = 604800.0;

use std::borrow::Cow;

#[derive(Deserialize)]
pub(crate) struct CalendarEventInput<'a> {
    #[serde(borrow)]
    id: &'a str,
    #[serde(borrow)]
    pubkey: &'a str,
    content: Cow<'a, str>,
    created_at: f64,
    #[serde(borrow, default)]
    tags: Vec<Vec<&'a str>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PollOptionOut<'a> {
    pub id: i64,
    pub text: Cow<'a, str>,
}

#[derive(Serialize)]
pub struct PollOut<'a> {
    pub id: &'a str,
    pub pubkey: &'a str,
    pub question: Cow<'a, str>,
    pub options: Vec<PollOptionOut<'a>>,
    #[serde(rename = "expiresAt")]
    pub expires_at: f64,
    pub closed: bool,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
}

#[derive(Deserialize)]
struct PollContent<'a> {
    #[serde(borrow)]
    question: Option<Cow<'a, str>>,
    #[serde(borrow)]
    options: Option<Vec<PollOptionOut<'a>>>,
}

pub(crate) fn parse_poll_event<'a>(
    ev: &'a CalendarEventInput<'a>,
    now_ms: f64,
) -> Option<PollOut<'a>> {
    if ev.tags.len() > MAX_TAGS_ENTRIES {
        return None;
    }
    let question: Cow<'a, str>;
    let mut options: Vec<PollOptionOut<'a>> = Vec::new();
    if ev.content.len() <= 256 * 1024 {
        match serde_json::from_str::<PollContent>(ev.content.as_ref()) {
            Ok(content) => {
                question = content
                    .question
                    .map(|q| Cow::Owned(q.into_owned()))
                    .unwrap_or_else(|| Cow::Borrowed(""));
                if let Some(opts) = content.options {
                    options.extend(opts.into_iter().take(MAX_POLL_OPTIONS).map(|o| {
                        PollOptionOut {
                            id: o.id,
                            text: Cow::Owned(o.text.into_owned()),
                        }
                    }));
                }
            }
            Err(_) => {
                question = match &ev.content {
                    Cow::Borrowed(s) => Cow::Borrowed(*s),
                    Cow::Owned(s) => Cow::Owned(s.clone()),
                };
            }
        }
    } else {
        question = Cow::Borrowed("");
    }
    if options.is_empty() {
        for tag in &ev.tags {
            if tag.len() >= 3 && tag[0] == "poll_option" {
                let id = tag[1].parse::<i64>().unwrap_or(0);
                options.push(PollOptionOut {
                    id,
                    text: Cow::Borrowed(tag[2]),
                });
                if options.len() >= MAX_POLL_OPTIONS {
                    break;
                }
            }
        }
    }
    let exp_tag = ev
        .tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == "expiration")
        .map(|t| t[1]);
    let expires_at = match exp_tag {
        Some(s) if s.len() <= 32 => match s.parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => v * 1000.0,
            _ => now_ms + POLL_EXPIRY_DEFAULT * 1000.0,
        },
        _ => now_ms + POLL_EXPIRY_DEFAULT * 1000.0,
    };
    let closed = now_ms > expires_at;
    Some(PollOut {
        id: ev.id,
        pubkey: ev.pubkey,
        question,
        options,
        expires_at,
        closed,
        created_at: ev.created_at * 1000.0,
    })
}

#[derive(Deserialize)]
struct PollInput<'a> {
    #[serde(borrow, rename = "event")]
    ev: CalendarEventInput<'a>,
    #[serde(rename = "nowMs")]
    now_ms: f64,
}

pub fn parse_poll_event_json(input: &str) -> String {
    let Some(parsed) = serde_json::from_str::<PollInput>(input).ok() else {
        return "null".to_string();
    };
    let res = parse_poll_event(&parsed.ev, parsed.now_ms);
    json_out(&res, "null")
}
