// NIP-84 dating profiles
pub const KIND_PROFILE: u16 = 30082;
// Live / story streaming
pub const KIND_LIVE: u16 = 30311;
pub const KIND_STORY: u16 = 30078;
// Marketplace
pub const KIND_LISTING: u16 = 30402;
pub const KIND_ORDER: u16 = 30403;
// Events RSVP
pub const KIND_EVENT: u16 = 31923;
pub const KIND_EVENT_RSVP: u16 = 31924;
// Minis registry (kind-31020, NIP-…)
pub const KIND_MINIS: u16 = 31020;

pub const MAX_TAG_VALUE_LEN: usize = 4096;
pub const MAX_TAGS: usize = 2_000;
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;
