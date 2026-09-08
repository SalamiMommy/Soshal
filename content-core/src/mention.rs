use regex::Regex;
use std::sync::OnceLock;

fn npub_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"nostr:npub1[ac-hj-np-z02-9]{58,82}").expect("valid npub regex"))
}

fn bech32_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"npub1[ac-hj-np-z02-9]{58,82}").expect("valid bech32 regex"))
}

#[derive(Debug, PartialEq)]
pub struct MentionSegment {
    pub text: String,
    pub is_mention: bool,
    pub pubkey: Option<String>,
}

pub fn parse(text: &str) -> Vec<MentionSegment> {
    if text.is_empty() {
        return Vec::new();
    }
    if !text.contains("nostr:npub1") {
        return vec![MentionSegment {
            text: text.to_string(),
            is_mention: false,
            pubkey: None,
        }];
    }
    let re = npub_re();
    let mut segments = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text) {
        if m.start() > last {
            segments.push(MentionSegment {
                text: text[last..m.start()].to_string(),
                is_mention: false,
                pubkey: None,
            });
        }
        segments.push(MentionSegment {
            text: m.as_str().to_string(),
            is_mention: true,
            pubkey: Some(m.as_str().trim_start_matches("nostr:").to_string()),
        });
        last = m.end();
    }
    if last < text.len() {
        segments.push(MentionSegment {
            text: text[last..].to_string(),
            is_mention: false,
            pubkey: None,
        });
    }
    segments
}

pub fn extract_pubkeys(text: &str) -> Vec<String> {
    if text.is_empty() || !text.contains("npub1") {
        return Vec::new();
    }
    let re = bech32_re();
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}
