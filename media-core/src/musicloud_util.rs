use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use soshal_nostr_core::models::{parse_audience, NostrEvent};

#[derive(Serialize)]
#[doc(hidden)]
pub struct Musicloud {
    pub id: String,
    pub url: String,
    pub pubkey: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    pub title: Option<String>,
    pub duration: Option<u64>,
    #[serde(rename = "textOverlay")]
    pub text_overlay: Option<String>,
    pub thumbnail: Option<String>,
    pub audience: String,
    pub genre: Option<String>,
    #[serde(rename = "waveformData")]
    pub waveform_data: Option<Vec<f64>>,
}

#[doc(hidden)]
pub fn parse_musiclouds(events: Vec<NostrEvent>) -> Vec<Musicloud> {
    let mut tracks: Vec<Musicloud> = Vec::new();

    for ev in events {
        let id = ev.id;
        let content = ev.content;
        let tags = &ev.tags;
        let created_at = ev.created_at as i64;
        let pubkey = ev.pubkey;

        let mut url = content.to_string();
        let mut title: Option<String> = None;
        let mut duration: Option<u64> = None;
        let text_overlay: Option<String> = None;
        let mut thumbnail: Option<String> = None;
        let mut audience = "public".to_string();
        let mut genre: Option<String> = None;
        let mut waveform_data: Option<Vec<f64>> = None;

        for tag in tags {
            if tag.len() >= 2 {
                match tag[0].as_str() {
                    "url" => url = tag[1].clone(),
                    "imeta" => {
                        if url.is_empty() || url == content {
                            url = tag[1].clone();
                        }
                    }
                    "title" => title = Some(tag[1].clone()),
                    "duration" => {
                        if let Ok(d) = tag[1].parse::<u64>() {
                            duration = Some(d);
                        }
                    }
                    "thumb" => thumbnail = Some(tag[1].clone()),
                    "audience" => {
                        audience = parse_audience(tag[1].as_str()).to_string();
                    }
                    "genre" | "g" => genre = Some(tag[1].clone()),
                    "waveform" if tag.len() >= 2 => {
                        waveform_data = tag[1]
                            .split(',')
                            .filter_map(|s| s.trim().parse::<f64>().ok())
                            .collect::<Vec<f64>>()
                            .into();
                        if let Some(ref w) = waveform_data {
                            if w.is_empty() {
                                waveform_data = None;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if id.is_empty() || url.is_empty() {
            continue;
        }

        tracks.push(Musicloud {
            id,
            url,
            pubkey,
            created_at,
            title,
            duration,
            text_overlay,
            thumbnail,
            audience,
            genre,
            waveform_data,
        });
    }

    tracks.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    tracks
}

pub fn parse_musiclouds_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct ParseMusicloudsInput {
        events: Vec<NostrEvent>,
    }

    let Some(input) = json_in::<Option<ParseMusicloudsInput>>(input, None) else {
        return "[]".to_string();
    };
    let tracks = parse_musiclouds(input.events);
    json_out(&tracks, "[]")
}
