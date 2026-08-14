//! Shared serde boilerplate for *_json wrappers.

use serde::de::DeserializeOwned;
use serde::Serialize;

pub fn json_in<T: DeserializeOwned>(input: &str, fallback: T) -> T {
    serde_json::from_str(input).unwrap_or(fallback)
}

pub fn json_in_borrow<'a, T: serde::Deserialize<'a>>(input: &'a str) -> Option<T> {
    serde_json::from_str(input).ok()
}

pub fn json_out<T: Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| fallback.to_string())
}
