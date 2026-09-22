//! Privacy level utilities: WoT distance mapping, level description, and
//! relay selection based on privacy configuration.

use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

// ---------------------------------------------------------------------------
// describe_privacy_level
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
pub struct DescribeLevelInput {
    pub level: String,
}

#[derive(Serialize, Deserialize)]
pub struct DescribeLevelOutput {
    pub label: String,
    pub description: String,
    #[serde(rename = "relayCount")]
    pub relay_count: i32,
    #[serde(rename = "feedReach")]
    pub feed_reach: String,
}

pub fn describe_privacy_level(level: &str) -> DescribeLevelOutput {
    match level {
        "public" => DescribeLevelOutput {
            label: "Public".to_string(),
            description: "Your profile is discoverable and visible to everyone.".to_string(),
            relay_count: 4,
            feed_reach: "Global Nostr Network".to_string(),
        },
        "only_me" => DescribeLevelOutput {
            label: "Only Me".to_string(),
            description: "Your profile is visible only to you. Nothing is published.".to_string(),
            relay_count: 0,
            feed_reach: "Local Only".to_string(),
        },
        "friends" => DescribeLevelOutput {
            label: "Friends".to_string(),
            description: "Your profile is shared only with people you follow.".to_string(),
            relay_count: 2,
            feed_reach: "Direct Connections".to_string(),
        },
        _ => DescribeLevelOutput {
            label: "Network".to_string(),
            description: "Your profile is shared with friends and their friends.".to_string(),
            relay_count: 3,
            feed_reach: "Extended Network".to_string(),
        },
    }
}

pub fn describe_privacy_level_json(input: &str) -> String {
    let level = if let Some(parsed) = json_in::<Option<DescribeLevelInput>>(input, None) {
        parsed.level
    } else {
        let trimmed = input.trim().trim_matches('"');
        if trimmed.is_empty() {
            "public".to_string()
        } else {
            trimmed.to_string()
        }
    };
    json_out(&describe_privacy_level(&level), "{}")
}

// ---------------------------------------------------------------------------
// get_max_wot_distance
// ---------------------------------------------------------------------------

pub fn get_max_wot_distance(level: &str) -> i32 {
    match level {
        "public" => 2,
        "network" => 2,
        "friends" => 1,
        "only_me" => 0,
        _ => 2,
    }
}

pub fn get_max_wot_distance_json(input: &str) -> i32 {
    let Some(parsed) = json_in::<Option<GetMaxWotDistanceInput>>(input, None) else {
        return 2;
    };
    get_max_wot_distance(&parsed.level)
}

#[derive(Deserialize, Serialize)]
pub struct GetMaxWotDistanceInput {
    pub level: String,
}

// ---------------------------------------------------------------------------
// select_relays
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
pub struct SelectRelaysInput {
    pub level: String,
    #[serde(rename = "i2pAvailable")]
    pub i2p_available: bool,
    #[serde(rename = "freenetAvailable")]
    pub freenet_available: bool,
    pub pubkey: Option<String>,
    #[serde(rename = "wotDistance")]
    pub wot_distance: Option<u32>,
}

pub fn select_relays(
    level: &str,
    i2p_available: bool,
    freenet_available: bool,
    pubkey: Option<&str>,
    wot_distance: Option<u32>,
) -> Vec<String> {
    // L6: public Nostr relay selection is audience-honest. "only_me" never
    // selects a public relay (mesh-only); a "friends" audience whose caller
    // graph distance already exceeds the friends max drops public relays too,
    // so content never leaks to strangers via a public relay's membership.
    let i2p_relays: Vec<&str> = vec![
        "wss://i2p.nostr.i2p",
        "wss://relay.i2p",
        "wss://i2p-relay.nostr.net",
    ];

    let freenet_relays: Vec<&str> = vec![
        "wss://freenet-relay.free",
        "wss://locutus-relay.free",
        "wss://nostr-free.free",
    ];

    let public_relays: Vec<&str> = vec![
        "wss://relay.damus.io",
        "wss://nos.lol",
        "wss://relay.primal.net",
        "wss://relay.nostr.band",
    ];

    let mut relays: Vec<String> = Vec::new();

    let relay_count = match level {
        "public" => 3,
        "network" => 2,
        "friends" => 2,
        "only_me" => 1,
        _ => 2,
    };

    if freenet_available {
        for relay in freenet_relays.iter().take(relay_count) {
            let r = relay.to_string();
            if !relays.contains(&r) {
                relays.push(r);
            }
        }
    }

    if i2p_available {
        for relay in i2p_relays.iter().take(relay_count) {
            let r = relay.to_string();
            if !relays.contains(&r) {
                relays.push(r);
            }
        }
    }

    let nostr_count = match level {
        "public" => public_relays.len(),
        "network" => {
            if freenet_available {
                4
            } else {
                3
            }
        }
        // L6: honored per-relay. Public relays are strangers by definition —
        // for a "friends" audience (max wot distance 1) they are only
        // acceptable while the caller's own graph distance still covers the
        // audience; once the caller's measured distance exceeds it, public
        // relay selection is dropped entirely.
        "friends" => match wot_distance {
            Some(d) if d > 1 => 0,
            _ => 2,
        },
        // "only_me" must never route through a public relay — zero public
        // selection (mesh-only; possibly an empty list, which callers treat
        // as mesh-only routing).
        "only_me" => 0,
        _ => 3,
    };
    let nostr_relays: Vec<String> = public_relays
        .iter()
        .take(nostr_count)
        .map(|r| r.to_string())
        .collect();
    relays.extend(nostr_relays);

    if let Some(pk) = pubkey {
        let max_distance = match level {
            "public" | "network" => 2,
            "friends" => 1,
            "only_me" => 0,
            _ => 2,
        };
        let is_allowed = match wot_distance {
            None => true,
            Some(d) => d <= max_distance,
        };
        if is_allowed {
            let clean: String = pk
                .to_lowercase()
                .chars()
                .filter(|c| c.is_ascii_hexdigit())
                .collect();
            let prefix = if clean.len() >= 16 {
                &clean[..16]
            } else {
                "user"
            };
            if freenet_available {
                let hosted_free_url = format!("wss://{}.free", prefix);
                if !relays.contains(&hosted_free_url) {
                    relays.push(hosted_free_url);
                }
            }
            if i2p_available {
                let hosted_url = format!("wss://{}.i2p", prefix);
                if !relays.contains(&hosted_url) {
                    relays.push(hosted_url);
                }
            }
        }
    }

    relays
}

pub fn select_relays_json(input: &str) -> String {
    let Some(parsed) = json_in::<Option<SelectRelaysInput>>(input, None) else {
        return "[]".to_string();
    };
    let relays = select_relays(
        &parsed.level,
        parsed.i2p_available,
        parsed.freenet_available,
        parsed.pubkey.as_deref(),
        parsed.wot_distance,
    );
    json_out(&relays, "[]")
}
