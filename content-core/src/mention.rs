use regex::Regex;
use std::sync::OnceLock;

fn npub_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)nostr:npub1[ac-hj-np-z02-9]{58,82}").expect("valid npub regex")
    })
}

fn bech32_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)npub1[ac-hj-np-z02-9]{58,82}").expect("valid bech32 regex"))
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
    if !text.to_ascii_lowercase().contains("nostr:npub1") {
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
        let m_str = m.as_str();
        let pubkey_str = if m_str.len() >= 6 && m_str[..6].eq_ignore_ascii_case("nostr:") {
            &m_str[6..]
        } else {
            m_str
        };
        segments.push(MentionSegment {
            text: m_str.to_string(),
            is_mention: true,
            pubkey: Some(pubkey_str.to_string()),
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
    if text.is_empty() || !text.to_ascii_lowercase().contains("npub1") {
        return Vec::new();
    }
    let re = bech32_re();
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}
