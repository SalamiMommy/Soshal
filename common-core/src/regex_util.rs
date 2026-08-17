use regex::Regex;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const CACHE_CAP: usize = 32;

static REGEX_CACHE: OnceLock<Mutex<VecDeque<(String, Regex)>>> = OnceLock::new();

/// Compiles a regex, panicking on invalid patterns (all internal patterns are
/// static literals; a panic here is a programming error, not user input).
pub fn compile_re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("invalid regex")
}

pub fn escape_regex(s: &str) -> String {
    regex::escape(s)
}

pub fn is_match(pattern: &str, text: &str) -> bool {
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let re = {
        let mut guard = cache.lock().unwrap();
        if let Some(pos) = guard.iter().position(|(p, _)| p == pattern) {
            let re = guard.remove(pos).expect("position from iter").1;
            guard.push_back((pattern.to_string(), re.clone()));
            re
        } else {
            let Ok(re) = Regex::new(pattern) else {
                return false;
            };
            if guard.len() >= CACHE_CAP {
                guard.pop_front();
            }
            guard.push_back((pattern.to_string(), re.clone()));
            re
        }
    };
    re.is_match(text)
}
