//! Integration tests for soshal-common-core: URL safety, SSRF guards, UI-safe
//! helpers, MIME sniffing and formatting utilities.

use soshal_common_core::format::{
    format_duration, format_timestamp, is_valid_hex, pluralize, seconds_to_ymd, truncate,
};
use soshal_common_core::json_util::{json_in, json_out};
use soshal_common_core::mime::{detect_mime_type, sniff_mime_type};
use soshal_common_core::regex_util::{escape_regex, is_match};
use soshal_common_core::ui_safe::{is_valid_css_color, js_string_literal, short_pk, truncate_str};
use soshal_common_core::url::{
    domain, extract, is_private_ip_str, is_valid, is_valid_event_relay_url, is_valid_media_url,
    is_valid_relay_url, safe_href, sanitize_link_url,
};

#[test]
fn url_extract_and_validate() {
    let text = "see https://example.com/a?b=1 and http://other.org/x plus no link";
    let urls = extract(text);
    assert_eq!(urls.len(), 2);
    assert!(urls[0].starts_with("https://example.com"));
    assert!(is_valid("https://example.com"));
    assert!(!is_valid("not a url"));
    assert_eq!(
        domain("https://example.com/path").as_deref(),
        Some("example.com")
    );
    assert_eq!(domain("garbage"), None);
    assert_eq!(extract("").len(), 0);
}

#[test]
fn media_url_rejects_private_and_rebinding() {
    assert!(is_valid_media_url("https://example.com/img.png"));
    assert!(!is_valid_media_url(""));
    assert!(!is_valid_media_url("ftp://example.com/x"));
    assert!(!is_valid_media_url("https://localhost/x"));
    assert!(!is_valid_media_url("https://127.0.0.1/x"));
    assert!(!is_valid_media_url("https://127.1/x"));
    assert!(!is_valid_media_url("https://10.0.0.1/x"));
    assert!(!is_valid_media_url("https://192.168.1.1/x"));
    assert!(!is_valid_media_url("https://172.16.0.1/x"));
    assert!(!is_valid_media_url("https://169.254.169.254/x"));
    assert!(!is_valid_media_url("https://0.0.0.0/x"));
    assert!(!is_valid_media_url("https://0x7f000001/x"));
    assert!(!is_valid_media_url("https://1.2.3.4.nip.io/x"));
    assert!(!is_valid_media_url("https://x.10.0.0.1.xip.io/x"));
    assert!(!is_valid_media_url("https://12345.test/x"));
}

#[test]
fn sanitize_and_href_only_http() {
    assert_eq!(
        sanitize_link_url("https://ok.org/a").as_deref(),
        Some("https://ok.org/a")
    );
    assert!(sanitize_link_url("javascript:alert(1)").is_none());
    assert!(safe_href("data:text/html,x").is_none());
    assert!(safe_href("blob:https://x/y").is_none());
    assert!(safe_href("https://localhost/x").is_none());
    assert_eq!(
        safe_href("https://safe.example/x").as_deref(),
        Some("https://safe.example/x")
    );
}

#[test]
fn private_ip_detection() {
    assert!(is_private_ip_str("127.0.0.1"));

    assert!(is_private_ip_str("10.1.2.3"));
    assert!(is_private_ip_str("172.16.0.1"));
    assert!(is_private_ip_str("172.31.255.1"));
    assert!(is_private_ip_str("192.168.0.1"));
    assert!(is_private_ip_str("169.254.1.1"));
    assert!(is_private_ip_str("0.0.0.0"));
    assert!(is_private_ip_str("100.64.0.1"));
    assert!(is_private_ip_str("224.0.0.1"));
    assert!(is_private_ip_str("::1"));
    assert!(is_private_ip_str("::ffff:127.0.0.1"));
    assert!(is_private_ip_str("[::1]"));
    assert!(is_private_ip_str("fc00::1"));
    assert!(is_private_ip_str("fe80::1"));
    assert!(!is_private_ip_str("8.8.8.8"));
    assert!(!is_private_ip_str("203.0.113.5"));
    assert!(!is_private_ip_str("example.com"));
    assert!(!is_private_ip_str(""));
    assert!(!is_private_ip_str("2606:4700::1111"));
}

#[test]
fn relay_url_validation() {
    assert_eq!(is_valid_relay_url("wss://relay.example.com"), (true, false));
    assert!(is_valid_relay_url("wss://xn--tnan.example.com").1);
    assert!(is_valid_relay_url("ws://relay.example.com").0);

    assert!(!is_valid_relay_url("https://relay.example.com").0);
    assert!(!is_valid_relay_url("wss://user:pass@relay.example.com").0);
    assert!(!is_valid_relay_url("wss://localhost").0);
    assert!(!is_valid_relay_url("wss://127.0.0.1/x").0);
    assert!(!is_valid_relay_url("wss://10.0.0.2/x").0);
    assert!(!is_valid_relay_url("wss://1.2.3.4/x").0);
    assert!(!is_valid_relay_url("wss://relay").0);
    assert!(is_valid_relay_url("wss://relay.example").0);
    assert!(!is_valid_relay_url("wss://").0);
    assert!(!is_valid_relay_url("wss://relay.nip.io").0);
    assert!(!is_valid_relay_url("wss://55555.example.com").0);
    assert!(!is_valid_relay_url("wss://0x7f.example.com").0);
    assert!(is_valid_event_relay_url("wss://relay.example.com"));
    assert!(!is_valid_event_relay_url("ws://relay.example.com"));
}

#[test]
fn css_color_validation() {
    assert!(is_valid_css_color("#fff"));
    assert!(is_valid_css_color("#123456"));
    assert!(!is_valid_css_color("red"));
    assert!(is_valid_css_color("hsl(120, 50%, 50%)"));
    assert!(!is_valid_css_color("#12"));
    assert!(!is_valid_css_color(""));
    assert!(!is_valid_css_color("; background: url(https://evil.com)"));
    assert!(!is_valid_css_color("red;"));
}

#[test]
fn ui_safe_helpers() {
    assert_eq!(short_pk("abcdef1234567890", 4), "abcd");

    assert_eq!(truncate_str("hello world", 5), "hello");
    assert_eq!(truncate_str("hello", 10), "hello");
    assert!(truncate_str("ééé", 2).chars().count() <= 2);
    assert_eq!(js_string_literal("a\"b"), "\"a\\\"b\"");

    assert!(js_string_literal("a\nb").contains("\\n"));
}

#[test]
fn mime_sniff_and_detect() {
    assert_eq!(sniff_mime_type(b"\x89PNG\r\n\x1a\n", "x"), "image/png");
    assert_eq!(sniff_mime_type(b"GIF89a", "x"), "image/gif");
    assert_eq!(
        sniff_mime_type(b"....", "fallback/type"),
        "application/octet-stream"
    );
    assert_eq!(detect_mime_type("photo.jpg"), "image/jpeg");
    assert!(detect_mime_type("video.mp4").starts_with("video/"));
}

#[test]
fn format_helpers() {
    assert!(is_valid_hex("deadbeef"));
    assert!(is_valid_hex("0123456789abcdefABCDEF"));
    assert!(!is_valid_hex(""));
    assert!(!is_valid_hex("zz"));
    assert_eq!(pluralize(1, "cat", None), "cat");
    assert_eq!(pluralize(2, "cat", None), "cats");
    assert_eq!(pluralize(2, "box", Some("boxes")), "boxes");
    assert_eq!(seconds_to_ymd(0), (1970, "Jan", 1));
    assert_eq!(seconds_to_ymd(86400), (1970, "Jan", 2));
    assert_eq!(format_timestamp(100, 100), "just now");
    assert_eq!(format_timestamp(100, 130), "just now");
    assert_eq!(format_timestamp(100, 200), "1m ago");
    assert_eq!(format_timestamp(100, 100 + 2 * 3600), "2h ago");
    assert_eq!(format_timestamp(100, 100 + 2 * 86400), "2d ago");
    assert!(format_timestamp(1_000_000, 1_700_000_000).contains(","));
    assert_eq!(format_duration(0.0), "");
    assert_eq!(format_duration(90.0), "1:30");
    assert_eq!(truncate("hello world", 5), "hell…");

    assert_eq!(truncate("hi", 5), "hi");
}

#[test]
fn json_util_roundtrip() {
    let v: i64 = json_in("42", 0);
    assert_eq!(v, 42);
    let out = json_out(&"hello", "fallback");
    assert!(out.contains("hello"));
    let v2: i64 = json_in("not json", 7);
    assert_eq!(v2, 7);
}

#[test]
fn regex_util() {
    assert!(is_match(r"^\d+$", "123"));
    assert!(!is_match(r"^\d+$", "abc"));
    let escaped = escape_regex("a.b");
    assert!(is_match(&escaped, "a.b"));
    assert!(!is_match(&escaped, "axb"));
}

#[test]
fn format_json_wrappers() {
    use soshal_common_core::format::{
        format_duration_json, format_timestamp_json, pluralize_json, truncate_json,
    };

    let trunc = truncate_json(r#"{"str":"hello world","maxLen":5}"#);
    assert_eq!(trunc, "hell…");

    let plur = pluralize_json(r#"{"count":2,"singular":"item"}"#);
    assert_eq!(plur, "items");

    let ts = format_timestamp_json(r#"{"seconds":100,"nowSec":100}"#);
    assert_eq!(ts, "just now");

    let dur = format_duration_json(r#"{"seconds":90.0}"#);
    assert_eq!(dur, "1:30");

    assert_eq!(pluralize_json("garbage"), "");
    assert_eq!(format_timestamp_json("garbage"), "");
}
