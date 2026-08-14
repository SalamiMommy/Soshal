use serde_json::json;
use soshal_content_core::chunk::{chunk_array, chunk_array_json};
use soshal_content_core::compress::{
    compress, compress_dict, compress_json, compress_json_dict, decompress_dict,
    decompress_dict_limited, decompress_json, decompress_json_dict, decompress_limited,
    is_dict_frame, ZSTD_DICT_ID, ZSTD_DICT_MAGIC,
};
use soshal_content_core::entities::decode_html_entities;
use soshal_content_core::extension::get_extension;
use soshal_content_core::forcelayout::calculate_force_layout_json;
use soshal_content_core::format::{format_timestamp, is_valid_hex, seconds_to_ymd, truncate};
use soshal_content_core::hashtag::{extract as extract_hashtags, split as split_hashtags};
use soshal_content_core::linkpreview::html::{extract_favicon, parse_link_preview_html_json};
use soshal_content_core::linkpreview::imeta::extract_imeta_video_urls;
use soshal_content_core::linkpreview::urls::extract_urls_json;
use soshal_content_core::mention::{extract_pubkeys, parse as parse_mentions};
use soshal_content_core::mime::{detect_mime_type, sniff_mime_type};
use soshal_content_core::regex_util::{compile_re, escape_regex};
use soshal_content_core::safe_json::safe_json_parse;
use soshal_content_core::sanitize::{
    sanitize_context, sanitize_details, sanitize_error_message, sanitize_log_message,
    scrub_sensitive_data,
};
use soshal_content_core::stories::filter_stories_json;
use soshal_content_core::tags::{find_tag_value, parse_audience};
use soshal_content_core::ui_safe::{is_valid_css_color, js_string_literal, short_pk};
use soshal_content_core::url::{
    domain, extract as extract_urls, is_valid, is_valid_event_relay_url, is_valid_media_url,
    is_valid_relay_url,
};

#[test]
fn chunk_array_basic() {
    let arr: Vec<serde_json::Value> = vec![json!(1), json!(2), json!(3), json!(4), json!(5)];
    let chunks = chunk_array(arr, 2);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], vec![json!(1), json!(2)]);
    assert_eq!(chunks[2], vec![json!(5)]);
}

#[test]
fn chunk_array_zero_size() {
    let arr: Vec<serde_json::Value> = vec![json!(1)];
    let chunks = chunk_array(arr, 0);
    assert!(chunks.is_empty());
}

#[test]
fn chunk_array_json_basic() {
    let result = chunk_array_json(r#"{"arr":[1,2,3,4,5],"size":2}"#);
    assert_eq!(result, r#"{"chunks":[[1,2],[3,4],[5]]}"#);
}

#[test]
fn compress_roundtrip() {
    let original = "hello world this is test data";
    let compressed = compress_json(original);
    let decompressed = decompress_json(&compressed);
    assert_eq!(original, decompressed);
}

#[test]
fn decompress_cap_enforced() {
    let big = "a".repeat(8 * 1024 * 1024);
    let compressed = compress(big.as_bytes()).unwrap();
    assert!(decompress_limited(&compressed, 1024).is_err());
    assert!(decompress_limited(&compressed, 9 * 1024 * 1024).is_ok());
}

#[test]
fn dict_roundtrip() {
    let payload = "{\"id\":\"note1abc\",\"pubkey\":\"npub1def\",\"kind\":1,\"content\":\"\
                    zip zap zop fed the feed with structured socia payloads #nostr\"}";
    let compressed = compress_dict(payload.as_bytes()).unwrap();
    assert!(is_dict_frame(&compressed));
    assert_eq!(&compressed[..6], &ZSTD_DICT_MAGIC);
    let id = u32::from_le_bytes(compressed[6..10].try_into().unwrap());
    assert_eq!(id, ZSTD_DICT_ID);
    let plain = decompress_dict(&compressed).unwrap();
    assert_eq!(String::from_utf8(plain).unwrap(), payload);
}

#[test]
fn dict_beats_deflate_on_repetitive_json() {
    let mut payload = String::new();
    for i in 0..200 {
        payload.push_str(&format!(
            "{{\"id\":\"note{i:03x}\",\"pubkey\":\"npub1deadbeefcafe\",\"kind\":1,\
                 \"tags\":[[\"t\",\"nostr\"]],\"content\":\"SOCIAL FEED post body text\"}}"
        ));
    }
    let dict_len = compress_dict(payload.as_bytes()).unwrap().len();
    let deflate_len = compress(payload.as_bytes()).unwrap().len();
    assert!(
        dict_len < deflate_len,
        "dict {} should beat deflate {} on repetitive JSON",
        dict_len,
        deflate_len
    );
}

#[test]
fn dict_rejects_non_dict_frame() {
    assert!(!is_dict_frame(&[]));
    assert!(!is_dict_frame(b"plain"));
    assert!(decompress_dict(b"not a frame").is_err());
}

#[test]
fn dict_cap_enforced() {
    let big = "repetitive social post content ".repeat(400_000);
    let compressed = compress_dict(big.as_bytes()).unwrap();
    assert!(decompress_dict_limited(&compressed, 4096).is_err());
    assert!(decompress_dict_limited(&compressed, 64 * 1024 * 1024).is_ok());
}

#[test]
fn dict_json_wrapper_roundtrip() {
    let original = "{\"kind\":1,\"content\":\"dictionary compressed feed payload\"}";
    let encoded = compress_json_dict(original);
    assert!(!encoded.is_empty());
    let restored = decompress_json_dict(&encoded);
    assert_eq!(original, restored);
    assert_eq!(decompress_json_dict(&compress_json(original)), original);
}

#[test]
fn decode_html_entities_basic() {
    assert_eq!(decode_html_entities("&amp;"), "&");
    assert_eq!(decode_html_entities("&lt;div&gt;"), "<div>");
}

#[test]
fn get_extension_basic() {
    assert_eq!(get_extension("https://example.com/image.jpg"), "jpg");
    assert_eq!(get_extension("https://example.com/file.PNG"), "png");
}

#[test]
fn test_force_layout_basic() {
    let input = r#"{"nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"edges":[{"source":"a","target":"b"}],"width":800,"height":600}"#;
    let result = calculate_force_layout_json(input);
    assert!(result.contains(r#""id":"a""#));
}

#[test]
fn format_tests() {
    assert_eq!(truncate("hello world", 5), "hell…");
    assert_eq!(truncate("hi", 10), "hi");
    assert_eq!(format_timestamp(100, 100), "just now");
    assert_eq!(seconds_to_ymd(0), (1970, "Jan", 1));
}

#[test]
fn hex_validation() {
    assert!(is_valid_hex("0123456789abcdefABCDEF"));
    assert!(!is_valid_hex("xyz"));
}

#[test]
fn extract_hashtags_test() {
    let tags = extract_hashtags("hello #world #foo");
    assert_eq!(tags, vec!["world", "foo"]);
    let split_res = split_hashtags("hello #world");
    assert_eq!(split_res.len(), 2);
}

#[test]
fn parse_mentions_test() {
    let result = parse_mentions("hello nostr:npub1pu3v3pzj4j6 end");
    assert_eq!(result.len(), 3);
    assert!(result[1].is_mention);
    let pks = extract_pubkeys("nostr:npub1pu3v3pzj4j6");
    assert!(!pks.is_empty());
}

#[test]
fn mime_sniff_test() {
    let bytes = vec![0xff, 0xd8, 0xff, 0xe0];
    assert_eq!(sniff_mime_type(&bytes, "image/jpeg"), "image/jpeg");
    assert_eq!(detect_mime_type("photo.jpg"), "image/jpeg");
}

#[test]
fn regex_util_test() {
    assert_eq!(escape_regex("hello.world"), r"hello\.world");
    assert!(compile_re(r"\d+").is_match("123"));
}

#[test]
fn safe_json_test() {
    let result = safe_json_parse(r#"{"a":1,"b":"hello"}"#);
    assert_eq!(result, Some(r#"{"a":1,"b":"hello"}"#.to_string()));
}

#[test]
fn sanitize_test() {
    let msg = "nsec1=abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    let result = sanitize_error_message(msg);
    assert!(result.contains("[REDACTED]"));
    assert_eq!(sanitize_log_message("user profile"), "user profile");
    let nsec = "nsec1qwqsvf30y2pqf30y2pqf30y2pqf30y2pqf30y2pqf30y2pveerx";
    assert!(scrub_sensitive_data(nsec).contains("[REDACTED_NSEC]"));
    let ctx = r#"{"userId":"123","password":"secret"}"#;
    assert!(sanitize_context(ctx).unwrap().contains("[REDACTED]"));
    let details = format!("/{}", "a".repeat(64));
    assert_eq!(sanitize_details(&details), "/[HEX]");
}

#[test]
fn link_preview_test() {
    let html = r#"<html><head><meta property="og:title" content="Test Page"></head></html>"#;
    let input = serde_json::json!({"html": html, "url": "https://example.com"});
    let res = parse_link_preview_html_json(&input.to_string());
    assert!(res.contains("Test Page"));
    let icon = extract_favicon(html, "https://example.com/page");
    assert!(icon.is_some());
    let tags = vec![vec![
        "imeta".to_string(),
        "url=https://example.com/v.mp4".to_string(),
        "m=video/mp4".to_string(),
    ]];
    let v_urls = extract_imeta_video_urls(&tags);
    assert_eq!(v_urls.len(), 1);
    let urls_res = extract_urls_json(r#"{"text":"check https://example.com"}"#);
    assert!(urls_res.contains("https://example.com"));
}

#[test]
fn stories_test() {
    let result = filter_stories_json("not json");
    assert_eq!(result, "[]");
}

#[test]
fn tags_test() {
    let tags = vec![vec!["t".into(), "nostr".into()]];
    assert_eq!(find_tag_value(&tags, "t"), Some("nostr"));
    assert_eq!(parse_audience("friends_only"), "friends_only");
}

#[test]
fn ui_safe_test() {
    assert!(is_valid_css_color("#fff"));
    assert_eq!(short_pk("abc", 12), "abc");
    assert_eq!(js_string_literal("abc"), "\"abc\"");
}

#[test]
fn url_test() {
    assert!(is_valid("https://example.com"));
    assert_eq!(
        domain("https://example.com/path").as_deref(),
        Some("example.com")
    );
    assert!(is_valid_media_url("https://example.com/image.jpg"));
    assert!(!is_valid_media_url("http://localhost:8080"));
    assert!(is_valid_event_relay_url("wss://relay.damus.io"));
    assert!(!is_valid_event_relay_url("ws://relay.damus.io"));
    let (v_relay, _) = is_valid_relay_url("wss://relay.damus.io");
    assert!(v_relay);
    let extracted = extract_urls("check https://example.com/path");
    assert_eq!(extracted.len(), 1);
}

#[test]
fn test_fts5_constants_and_safe_json_invalid() {
    use soshal_content_core::fts5::{MAX_FTS5_TERMS, MAX_FTS5_TERM_LEN};
    assert_eq!(MAX_FTS5_TERMS, 32);
    assert_eq!(MAX_FTS5_TERM_LEN, 64);

    assert_eq!(safe_json_parse("not json"), None);
}
