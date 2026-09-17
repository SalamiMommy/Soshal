use serde::Deserialize;

use soshal_common_core::json_util::json_in_borrow;

#[derive(Deserialize)]
struct ExpiryInputBorrow<'a> {
    #[serde(borrow)]
    tags: Vec<Vec<&'a str>>,
}

pub fn get_expiry_from_tags(tags: &[Vec<String>]) -> i64 {
    soshal_content_core::tags::find_tag_value(tags, "expiration")
        .or_else(|| soshal_content_core::tags::find_tag_value(tags, "expires_at"))
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
}

pub fn get_expiry_from_tags_json(input: &str) -> i64 {
    let Some(input) = json_in_borrow::<ExpiryInputBorrow>(input) else {
        return 0;
    };
    if input.tags.len() > soshal_common_core::consts::MAX_TAGS {
        return 0;
    }
    for tag in input.tags.iter().take(soshal_common_core::consts::MAX_TAGS) {
        if tag.len() >= 2 && (tag[0] == "expiration" || tag[0] == "expires_at") {
            if let Ok(v) = tag[1].parse::<i64>() {
                return v;
            }
        }
    }
    0
}
