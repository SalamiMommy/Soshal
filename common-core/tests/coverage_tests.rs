use soshal_common_core::consts::*;
use soshal_common_core::regex_util::compile_re;
use soshal_common_core::store::normalized_store::{global_store, NormalizedStore, UserEntity};
use soshal_common_core::url::is_private_ipv6_str;

#[test]
fn consts_values() {
    assert_eq!(KIND_PROFILE, 30082);
    assert_eq!(KIND_LIVE, 30311);
    assert_eq!(KIND_STORY, 30078);
    assert_eq!(KIND_LISTING, 30402);
    assert_eq!(KIND_ORDER, 30403);
    assert_eq!(KIND_EVENT, 31923);
    assert_eq!(KIND_EVENT_RSVP, 31924);
    assert_eq!(KIND_MINIS, 31020);
    assert_eq!(MAX_TAG_VALUE_LEN, 4096);
    assert_eq!(MAX_TAGS, 2000);
    assert_eq!(MAX_CONTENT_BYTES, 65536);
}

#[test]
fn compile_re_valid_and_matches() {
    let re = compile_re(r"^soshal_\d+$");
    assert!(re.is_match("soshal_123"));
    assert!(!re.is_match("soshal_abc"));
}

#[test]
#[should_panic(expected = "invalid regex")]
fn compile_re_panics_on_invalid() {
    compile_re("[");
}

#[test]
fn is_private_ipv6_str_cases() {
    assert!(is_private_ipv6_str("::1"));
    assert!(is_private_ipv6_str("fc00::1"));
    assert!(is_private_ipv6_str("fe80::1"));
    assert!(is_private_ipv6_str("2001::1"));
    assert!(is_private_ipv6_str("[::1]"));
    assert!(is_private_ipv6_str("::ffff:127.0.0.1"));
    assert!(!is_private_ipv6_str("2606:4700::1"));
    assert!(!is_private_ipv6_str("not-an-ip"));
    assert!(!is_private_ipv6_str(""));
}

#[test]
fn normalized_store_clear_wipes_entities() {
    let store = NormalizedStore::new();
    store.upsert_user(UserEntity {
        pubkey: "pk1".into(),
        name: Some("alice".into()),
        avatar_url: None,
        nip05: None,
        updated_at: 1000,
    });
    assert!(store.get_user("pk1").is_some());
    store.clear();
    assert!(store.get_user("pk1").is_none(), "clear wipes users map");
    store.clear();
    store.clear();
}

#[test]
fn global_store_is_singleton() {
    let a = global_store();
    let b = global_store();
    assert!(std::ptr::eq(a, b), "global store must be a singleton");
}
