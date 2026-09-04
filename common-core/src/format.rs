use serde::Deserialize;

use crate::json_util::json_in;

/// Maximum representable Unix timestamp (year 9999).
#[doc(hidden)]
pub const MAX_TIMESTAMP_SECS: u64 = 253_402_300_799;

/// Returns the current Unix timestamp in seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Truncates a string to at most max_len Unicode scalar values, replacing the
/// last kept character with an ellipsis when the input is longer.
pub fn truncate(s: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let mut count = 0;
    let mut truncate_at = None;
    for (idx, _) in s.char_indices() {
        count += 1;
        if count == max_len {
            truncate_at = Some(idx);
        } else if count > max_len {
            if let Some(cut) = truncate_at {
                let mut out = String::with_capacity(cut + 3);
                out.push_str(&s[..cut]);
                out.push('…');
                return out;
            }
        }
    }
    s.to_string()
}

pub fn is_valid_hex(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn pluralize(count: usize, singular: &str, plural: Option<&str>) -> String {
    if count == 1 {
        singular.to_string()
    } else if let Some(p) = plural {
        p.to_string()
    } else {
        format!("{}s", singular)
    }
}

pub fn seconds_to_ymd(seconds: u64) -> (u32, &'static str, u32) {
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const SECS_PER_DAY: u64 = 86_400;

    if seconds >= MAX_TIMESTAMP_SECS {
        return (9999, "Dec", 31);
    }

    // Civil calendar algorithm (Howard Hinnant): O(1) without loops
    let z = (seconds / SECS_PER_DAY) as i64 + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 3]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y } as u32;

    if year > 9999 {
        return (9999, "Dec", 31);
    }

    (year, MONTH_NAMES[(m - 1) as usize], d)
}

pub fn format_timestamp(seconds: u64, now_sec: u64) -> String {
    if seconds > MAX_TIMESTAMP_SECS {
        return String::new();
    }
    let diff = now_sec.saturating_sub(seconds);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        let (y, m, d) = seconds_to_ymd(seconds);
        format!("{} {}, {}", m, d, y)
    }
}

pub fn format_duration(seconds: f64) -> String {
    if seconds <= 0.0 || !seconds.is_finite() {
        return String::new();
    }
    let total_secs = seconds as u64;
    let m = total_secs / 60;
    let s = total_secs % 60;
    format!("{}:{:02}", m, s)
}

#[derive(Deserialize)]
struct TruncateInput {
    #[serde(rename = "str")]
    str: String,
    #[serde(rename = "maxLen")]
    max_len: usize,
}

pub fn truncate_json(input: &str) -> String {
    let input = json_in(
        input,
        TruncateInput {
            str: String::new(),
            max_len: 0,
        },
    );
    truncate(&input.str, input.max_len)
}

#[derive(Deserialize)]
struct PluralizeInput {
    count: usize,
    singular: String,
    plural: Option<String>,
}

pub fn pluralize_json(input: &str) -> String {
    let input: PluralizeInput = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    pluralize(input.count, &input.singular, input.plural.as_deref())
}

pub fn format_timestamp_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct FormatTimestampInput {
        seconds: u64,
        #[serde(rename = "nowSec")]
        now_sec: u64,
    }
    let input: FormatTimestampInput = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    format_timestamp(input.seconds, input.now_sec)
}

pub fn format_duration_json(input: &str) -> String {
    let v = json_in(input, serde_json::Value::Null);
    let seconds = v.get("seconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
    format_duration(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_secs() {
        let a = now_secs();
        let b = now_secs();
        assert!(a > 0);
        assert!(b - a <= 5);
        let sys = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((sys - a).abs() <= 60);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("", 5), "");
        assert_eq!(truncate("abc", 0), "");
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("héllo", 2), "h…");
    }

    #[test]
    fn test_is_valid_hex() {
        assert!(!is_valid_hex(""));
        assert!(is_valid_hex("aBc123fF"));
        assert!(!is_valid_hex("xyz"));
        assert!(!is_valid_hex("abc "));
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize(1, "post", None), "post");
        assert_eq!(pluralize(2, "post", None), "posts");
        assert_eq!(pluralize(2, "box", Some("boxes")), "boxes");
    }

    #[test]
    fn test_seconds_to_ymd() {
        assert_eq!(seconds_to_ymd(0), (1970, "Jan", 1));
        assert_eq!(seconds_to_ymd(86_400 * 31), (1970, "Feb", 1));
        assert_eq!(seconds_to_ymd(86_400 * 366), (1971, "Jan", 2));
        assert_eq!(seconds_to_ymd(86_400 * 1127), (1973, "Feb", 1));
        assert_eq!(seconds_to_ymd(MAX_TIMESTAMP_SECS), (9999, "Dec", 31));
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(1000, 1030), "just now");
        assert_eq!(format_timestamp(1000, 1300), "5m ago");
        assert_eq!(format_timestamp(1000, 3700), "45m ago");
        assert_eq!(format_timestamp(1000, 90_000), "1d ago");
        assert_eq!(format_timestamp(0, 700_000), "Jan 1, 1970");
        assert_eq!(format_timestamp(MAX_TIMESTAMP_SECS + 1, 0), "");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0.0), "");
        assert_eq!(format_duration(65.0), "1:05");
        assert_eq!(format_duration(600.0), "10:00");
        assert_eq!(format_duration(f64::NAN), "");
    }

    #[test]
    fn test_json_wrappers() {
        assert_eq!(
            truncate_json(r#"{"str":"hello world","maxLen":5}"#),
            "hell…"
        );
        assert_eq!(truncate_json("garbage"), "");
        assert_eq!(pluralize_json(r#"{"count":2,"singular":"post"}"#), "posts");
        assert_eq!(
            format_timestamp_json(r#"{"seconds":1000,"nowSec":1300}"#),
            "5m ago"
        );
        assert_eq!(format_duration_json(r#"{"seconds":65.0}"#), "1:05");
    }
}
