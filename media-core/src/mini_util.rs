use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use soshal_nostr_core::models::{parse_audience, NostrEvent};

#[derive(Serialize)]
#[doc(hidden)]
pub struct Mini {
    pub id: String,
    pub url: String,
    pub pubkey: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "textOverlay")]
    pub text_overlay: Option<String>,
    pub thumbnail: Option<String>,
    pub audience: String,
}

#[doc(hidden)]
pub fn parse_minis(events: Vec<NostrEvent>) -> Vec<Mini> {
    let mut minis: Vec<Mini> = Vec::new();

    for ev in events {
        let id = ev.id;
        let content = ev.content;
        let tags = &ev.tags;
        let created_at = ev.created_at as i64;
        let pubkey = ev.pubkey;

        let mut url = content.to_string();
        let mut text_overlay: Option<String> = None;
        let mut thumbnail: Option<String> = None;
        let mut audience = "public".to_string();

        for tag in tags {
            if tag.len() >= 2 {
                match tag[0].as_str() {
                    "url" => url = tag[1].clone(),
                    "imeta" => {
                        if url.is_empty() || url == content {
                            url = tag[1].clone();
                        }
                    }
                    "title" => text_overlay = Some(tag[1].clone()),
                    "thumb" => thumbnail = Some(tag[1].clone()),
                    "audience" => {
                        audience = parse_audience(tag[1].as_str()).to_string();
                    }
                    _ => {}
                }
            }
        }

        if id.is_empty() || url.is_empty() {
            continue;
        }

        minis.push(Mini {
            id,
            url,
            pubkey,
            created_at,
            text_overlay,
            thumbnail,
            audience,
        });
    }

    minis.sort_by_key(|m| std::cmp::Reverse(m.created_at));
    minis
}

pub fn parse_minis_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct ParseMinisInput {
        events: Vec<NostrEvent>,
    }

    let Some(input) = json_in::<Option<ParseMinisInput>>(input, None) else {
        return "[]".to_string();
    };
    let minis = parse_minis(input.events);
    json_out(&minis, "[]")
}
