pub mod channels;
pub mod chat;
pub mod groups;
pub mod join_requests;
pub mod posts;

pub(crate) const MAX_EVENTS: usize = 100_000;
pub(crate) const MAX_TAG_VALUE_LEN: usize = 4096;

pub type GroupEventInput = soshal_nostr_core::models::NostrEvent;

pub(crate) use soshal_nostr_core::models::find_tag_values_map;

#[allow(dead_code)]
pub(crate) fn find_tag_value(tags: &[Vec<String>], key: &str) -> Option<String> {
    soshal_nostr_core::models::find_tag_value(tags, key).map(|s| s.to_string())
}

pub(crate) fn safe_truncate(s: &str, max_bytes: usize) -> String {
    soshal_common_core::ui_safe::truncate_str(s, max_bytes).to_string()
}
