pub mod channels;
pub mod chat;
pub mod groups;
pub mod join_requests;
pub mod posts;

pub(crate) const MAX_EVENTS: usize = 100_000;

/// Per-event tag cap (distinct from [`MAX_EVENTS`], which bounds the event
/// batch) — guards against a hostile inbound event carrying a tag flood.
pub(crate) const MAX_TAGS_PER_EVENT: usize = 100_000;
pub(crate) use soshal_common_core::consts::MAX_TAG_VALUE_LEN;

pub type GroupEventInput = soshal_nostr_core::models::NostrEvent;

pub(crate) use soshal_nostr_core::models::find_tag_values_map;

pub(crate) fn safe_truncate(s: &str, max_bytes: usize) -> String {
    soshal_common_core::ui_safe::truncate_str(s, max_bytes).to_string()
}

pub(crate) fn clamp_created_at(v: f64) -> f64 {
    if v.is_finite() && v >= 0.0 {
        v
    } else {
        0.0
    }
}

pub(crate) fn is_safe_group_media_url(u: &str) -> bool {
    if u.is_empty() || u.len() > MAX_TAG_VALUE_LEN {
        return false;
    }
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("file:")
        || lower.starts_with("vbscript:")
    {
        return false;
    }
    if lower.contains("://") || lower.starts_with("//") {
        return soshal_common_core::url::is_valid_media_url(u);
    }
    true
}
