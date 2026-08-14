use serde_json;

use crate::json_util::json_in;

pub fn safe_json_parse(text: &str) -> Option<String> {
    if text.len() > 64 * 1024 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string(&value).ok()
}

pub fn safe_json_parse_json(input: &str) -> String {
    let s = json_in(input, serde_json::Value::Null);
    let text = s.get("text").and_then(|v| v.as_str()).unwrap_or("");
    safe_json_parse(text).unwrap_or_else(|| "null".to_string())
}
