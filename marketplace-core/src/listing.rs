use serde::{Deserialize, Serialize};
use soshal_common_core::consts::MAX_TAG_VALUE_LEN;
use soshal_common_core::json_util::{json_in, json_out};

const MAX_TAGS_ENTRIES: usize = 100_000;
const MAX_PRICE: f64 = 1.0e15;

#[derive(Deserialize)]
struct ListingEvent {
    id: String,
    pubkey: String,
    content: String,
    created_at: f64,
    #[serde(default)]
    tags: Vec<Vec<String>>,
}

#[derive(Serialize)]
pub struct ListingOut {
    pub id: String,
    pub pubkey: String,
    #[serde(rename = "dTag")]
    pub d_tag: String,
    pub title: String,
    pub price: f64,
    pub currency: String,
    pub condition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "locationGeohash", skip_serializing_if = "Option::is_none")]
    pub location_geohash: Option<String>,
    pub images: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<String>>,
    #[serde(rename = "contactMethods")]
    pub contact_methods: Vec<String>,
    pub tags: Vec<String>,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
    #[serde(rename = "escrowEnabled")]
    pub escrow_enabled: bool,
}

#[derive(Deserialize)]
struct ListingContent {
    description: Option<String>,
    condition: Option<String>,
    #[serde(rename = "contactMethods")]
    contact_methods: Option<Vec<String>>,
    #[serde(rename = "escrowEnabled")]
    escrow_enabled: Option<bool>,
}

fn parse_listing(ev: &ListingEvent) -> Option<ListingOut> {
    if ev.tags.len() > MAX_TAGS_ENTRIES {
        return None;
    }
    let mut d_tag: Option<&str> = None;
    let mut title: Option<&str> = None;
    let mut price_str: Option<&str> = None;
    let mut currency: Option<&str> = None;
    let mut location_geohash: Option<&str> = None;
    let mut images: Vec<String> = Vec::new();
    let mut videos: Vec<String> = Vec::new();
    let mut hashtags: Vec<String> = Vec::new();

    for tag in &ev.tags {
        if tag.len() < 2 {
            continue;
        }
        let v = &tag[1];
        if v.len() > MAX_TAG_VALUE_LEN {
            continue;
        }
        match tag[0].as_str() {
            "d" if d_tag.is_none() => d_tag = Some(v),
            "title" if title.is_none() => title = Some(v),
            "price" if price_str.is_none() => price_str = Some(v),
            "currency" if currency.is_none() => currency = Some(v),
            "location" | "g" if location_geohash.is_none() => location_geohash = Some(v),
            "image" => images.push(v.clone()),
            "video" => videos.push(v.clone()),
            "t" => hashtags.push(v.clone()),
            _ => {}
        }
        if images.len() + videos.len() + hashtags.len() > 10_000 {
            break;
        }
    }

    let price_str = price_str?;
    let price = match price_str.parse::<f64>() {
        Ok(p) if p.is_finite() && p.abs() <= MAX_PRICE => p,
        _ => return None,
    };
    let currency_str = currency.unwrap_or("USD");
    if currency_str.len() > 16 {
        return None;
    }
    let currency = currency_str.to_string();
    let d_tag = d_tag
        .map(|s| s.to_string())
        .unwrap_or_else(|| ev.id.chars().take(12).collect::<String>());
    let title = title.unwrap_or("Untitled").to_string();
    let location_geohash = location_geohash.map(|s| s.to_string());
    let mut description: Option<String> = None;
    let mut condition = "good".to_string();
    let mut contact_methods: Vec<String> = Vec::new();
    let mut escrow_enabled = false;
    if ev.content.len() <= 256 * 1024 {
        if let Ok(content) = serde_json::from_str::<ListingContent>(&ev.content) {
            description = content.description;
            if let Some(c) = content.condition {
                condition = c;
            }
            if let Some(cm) = content.contact_methods {
                contact_methods = cm.into_iter().take(64).collect();
            }
            if let Some(e) = content.escrow_enabled {
                escrow_enabled = e;
            }
        } else if !ev.content.is_empty() {
            description = Some(ev.content.clone());
        }
    }
    Some(ListingOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        d_tag,
        title,
        price,
        currency,
        condition,
        description,
        location_geohash,
        images,
        videos: if videos.is_empty() {
            None
        } else {
            Some(videos)
        },
        contact_methods,
        tags: hashtags,
        created_at: ev.created_at,
        escrow_enabled,
    })
}

pub fn parse_listing_json(input: &str) -> String {
    let Some(ev) = json_in::<Option<ListingEvent>>(input, None) else {
        return "null".to_string();
    };
    json_out(&parse_listing(&ev), "null")
}

pub fn parse_listing_value(ev: serde_json::Value) -> String {
    let Some(ev) = serde_json::from_value::<Option<ListingEvent>>(ev)
        .ok()
        .flatten()
    else {
        return "null".to_string();
    };
    json_out(&parse_listing(&ev), "null")
}
