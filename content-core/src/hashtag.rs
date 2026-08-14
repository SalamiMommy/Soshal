use regex::Regex;
use std::sync::OnceLock;

fn hashtag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#(\w{1,50})").expect("valid hashtag regex"))
}

#[derive(Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    pub is_hashtag: bool,
}

pub fn split(text: &str) -> Vec<Segment> {
    let re = hashtag_re();
    let mut segments = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text) {
        if m.start() > last {
            segments.push(Segment {
                text: text[last..m.start()].to_string(),
                is_hashtag: false,
            });
        }
        segments.push(Segment {
            text: m.as_str().to_string(),
            is_hashtag: true,
        });
        last = m.end();
    }
    if last < text.len() {
        segments.push(Segment {
            text: text[last..].to_string(),
            is_hashtag: false,
        });
    }
    segments
}

pub fn extract(text: &str) -> Vec<String> {
    let re = hashtag_re();
    re.captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect()
}
