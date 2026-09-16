use serde_json::json;
use soshal_content_core::ast_parser::{parse_post_ast, SpanType};
use soshal_content_core::chunk::{chunk_array, chunk_array_json};
use soshal_content_core::compress::{
    compress, compress_dict, compress_json, compress_json_dict, decompress, decompress_dict,
    decompress_dict_limited, decompress_json, decompress_json_dict, decompress_limited,
    is_dict_frame, MAX_DECOMPRESS_BYTES, ZSTD_DICT_HEADER_LEN, ZSTD_DICT_ID, ZSTD_DICT_MAGIC,
};
use soshal_content_core::entities::{decode_ascii_entities, decode_html_entities};
use soshal_content_core::extension::get_extension;
use soshal_content_core::forcelayout::calculate_force_layout_json;
use soshal_content_core::format::{
    format_duration, format_duration_json, format_timestamp, format_timestamp_json, is_valid_hex,
    now_secs, pluralize, pluralize_json, seconds_to_ymd, truncate, truncate_json,
    MAX_TIMESTAMP_SECS,
};
use soshal_content_core::hashtag::{extract as extract_hashtags, split as split_hashtags};
use soshal_content_core::json_util::{json_in, json_in_borrow, json_out};
use soshal_content_core::linkpreview::html::{
    extract_favicon, parse_link_preview_html, parse_link_preview_html_json,
};
use soshal_content_core::linkpreview::imeta::{
    extract_imeta_video_urls, extract_imeta_video_urls_json,
};
use soshal_content_core::linkpreview::urls::extract_urls_json;
use soshal_content_core::mention::{extract_pubkeys, parse as parse_mentions};
use soshal_content_core::mime::{detect_mime_type, sniff_mime_type, sniff_mime_type_json};
use soshal_content_core::regex_util::{compile_re, escape_regex, is_match};
use soshal_content_core::safe_json::{safe_json_parse, safe_json_parse_json};
use soshal_content_core::sanitize::{
    sanitize_context, sanitize_details, sanitize_error_message, sanitize_log_message,
    sanitize_notif_content, scrub_sensitive_data,
};
use soshal_content_core::stories::filter_stories_json;
use soshal_content_core::tags::{find_tag_value, find_tag_values_map, parse_audience};
use soshal_content_core::ui_safe::{is_valid_css_color, js_string_literal, short_pk, truncate_str};
use soshal_content_core::url::{
    domain, extract as extract_urls, is_private_ip_str, is_private_ipv6_str, is_valid,
    is_valid_event_relay_url, is_valid_media_url, is_valid_relay_url, safe_href, sanitize_link_url,
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
    let result = parse_mentions(
        "hello nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6 end",
    );
    assert_eq!(result.len(), 3);
    assert!(result[1].is_mention);
    let pks =
        extract_pubkeys("nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6");
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

#[test]
fn ast_parser_post_ast() {
    let spans = parse_post_ast("Hello @alice check #soshal at https://soshal.app :fire:");
    assert_eq!(spans.len(), 7);
    assert_eq!(spans[0].span_type, SpanType::Text);
    assert_eq!(spans[0].text, "Hello ");
    assert_eq!(spans[1].span_type, SpanType::Mention);
    assert_eq!(spans[1].target.as_deref(), Some("@alice"));
    assert_eq!(spans[3].span_type, SpanType::Hashtag);
    assert_eq!(spans[3].target.as_deref(), Some("soshal"));
    assert_eq!(spans[5].span_type, SpanType::Link);
    assert_eq!(spans[5].target.as_deref(), Some("https://soshal.app"));
    assert_eq!(spans[6].span_type, SpanType::Emoji);
    assert_eq!(spans[6].target.as_deref(), Some("fire"));
    assert_eq!(spans[2].target, None);
    assert!(parse_post_ast("").is_empty());
    let lone = parse_post_ast("#");
    assert_eq!(lone.len(), 1);
    assert_eq!(lone[0].span_type, SpanType::Text);
    let mid = parse_post_ast(
        "tell nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6 now",
    );
    assert_eq!(mid[1].span_type, SpanType::Mention);
    assert_eq!(
        mid[1].target.as_deref(),
        Some("nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6")
    );
}

#[test]
fn compress_deflate_roundtrip() {
    let data: &[u8] = b"roundtrip payload";
    let compressed = compress(data).unwrap();
    assert_eq!(decompress(&compressed).unwrap(), data);
    assert_eq!(decompress(&compress(b"").unwrap()).unwrap(), b"");
    assert_eq!(decompress_json(&compress_json("")), "");
    assert_eq!(decompress_json("!!!not base64!!!"), "");
    assert_eq!(decompress_json("AAAA"), "");
    assert!(decompress_limited(b"garbage", 1024).is_err());
}

#[test]
fn entities_decode_rules() {
    assert_eq!(decode_html_entities("a &amp; b"), "a & b");
    assert_eq!(decode_html_entities("&#65;"), "A");
    assert_eq!(decode_html_entities("&#x41;"), "A");
    assert_eq!(decode_html_entities("&nbsp;"), "\u{00a0}");
    assert_eq!(decode_html_entities("&mdash;"), "\u{2014}");
    assert_eq!(decode_html_entities("&unknown;"), "&unknown;");
    assert_eq!(decode_html_entities("&amp"), "&amp");
    assert_eq!(decode_html_entities("&amp;lt;"), "&lt;");
    assert_eq!(decode_ascii_entities("&#65;"), "a");
    assert_eq!(decode_ascii_entities("&#x4A;"), "j");
    assert_eq!(decode_ascii_entities("&amp;"), "&amp;");
    assert_eq!(decode_ascii_entities("&#233;"), "&#233;");
    assert_eq!(decode_ascii_entities("&#0;"), "&#0;");
}

#[test]
fn hashtag_extract_rules() {
    assert_eq!(extract_hashtags("#Foo #bar"), vec!["Foo", "bar"]);
    assert_eq!(extract_hashtags("#foo-bar"), vec!["foo"]);
    assert_eq!(extract_hashtags("#café"), vec!["café"]);
    assert_eq!(extract_hashtags("#a_b"), vec!["a_b"]);
    assert_eq!(extract_hashtags("no tags"), Vec::<String>::new());
    assert_eq!(extract_hashtags("#"), Vec::<String>::new());
    let segs = split_hashtags("#a plain #b");
    assert_eq!(segs.len(), 3);
    assert!(segs[0].is_hashtag);
    assert!(!segs[1].is_hashtag);
    assert_eq!(segs[1].text, " plain ");
    assert!(segs[2].is_hashtag);
}

#[test]
fn html_sanitize_notif_content() {
    assert_eq!(
        sanitize_notif_content("<script>alert(1)</script>", 500),
        "alert(1)"
    );
    assert_eq!(
        sanitize_notif_content("<a href=\"x\" onclick=\"steal()\">link</a>", 500),
        "link"
    );
    assert_eq!(sanitize_notif_content("plain text", 500), "plain text");
    assert_eq!(sanitize_notif_content("a  b\n\t c", 500), "a b c");
    assert_eq!(sanitize_notif_content("hello world", 5), "hello");
}

#[test]
fn mention_segments_detail() {
    let segs = parse_mentions(
        "hi nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6 end",
    );
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].text, "hi ");
    assert!(!segs[0].is_mention);
    assert!(segs[1].is_mention);
    assert_eq!(
        segs[1].pubkey.as_deref(),
        Some("npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6")
    );
    assert_eq!(segs[2].text, " end");
    assert_eq!(
        extract_pubkeys("see npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6 and nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6"),
        vec![
            "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6",
            "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6"
        ]
    );
    let invalid = parse_mentions("npub1bobby");
    assert_eq!(invalid.len(), 1);
    assert!(!invalid[0].is_mention);
}

#[test]
fn tags_multi_key_lookup() {
    let tags = vec![
        vec!["t".to_string(), "nostr".to_string()],
        vec!["t".to_string(), "second".to_string()],
        vec!["e".to_string(), "abc".to_string()],
        vec!["single".to_string()],
    ];
    assert_eq!(find_tag_value(&tags, "t"), Some("nostr"));
    assert_eq!(
        find_tag_values_map(&tags, ["t", "e"]),
        [Some("nostr"), Some("abc")]
    );
    assert_eq!(
        find_tag_values_map(&tags, ["t", "missing"]),
        [Some("nostr"), None]
    );
    assert_eq!(
        find_tag_values_map(&tags, ["e", "single"]),
        [Some("abc"), None]
    );
    assert_eq!(parse_audience("network"), "network");
    assert_eq!(parse_audience("only_me"), "only_me");
    assert_eq!(parse_audience("bogus"), "public");
}

#[test]
fn extension_edge_cases() {
    assert_eq!(get_extension("https://example.com/v.mp4?x=1&y=2"), "mp4");
    assert_eq!(get_extension("https://example.com/img.png#frag"), "png");
    assert_eq!(get_extension("archive.tar.gz?x"), "gz");
    assert_eq!(get_extension("https://example.com/path/"), "");
    assert_eq!(get_extension("https://example.com/file."), "");
    assert_eq!(get_extension("https://example.com/f.abcdefghijk"), "");
}

#[test]
fn compress_constants_and_dict_errors() {
    assert_eq!(ZSTD_DICT_HEADER_LEN, 10);
    assert_eq!(MAX_DECOMPRESS_BYTES, 4 * 1024 * 1024);
    let mut bad_id = ZSTD_DICT_MAGIC.to_vec();
    bad_id.extend_from_slice(&2u32.to_le_bytes());
    bad_id.extend_from_slice(b"payload");
    assert!(decompress_dict(&bad_id).is_err());
    assert!(decompress_dict_limited(&ZSTD_DICT_MAGIC, 1024).is_err());
    let empty = compress_dict(b"").unwrap();
    assert_eq!(decompress_dict(&empty).unwrap(), b"");
    assert_eq!(decompress_json_dict("!!!"), "");
}

#[test]
fn entities_borrowed_and_more() {
    assert_eq!(decode_html_entities("no entities here"), "no entities here");
    assert!(matches!(
        decode_html_entities("plain"),
        std::borrow::Cow::Borrowed(_)
    ));
    assert_eq!(decode_html_entities("&#X41;"), "A");
    assert_eq!(
        decode_html_entities("&quot; &apos; &euro; &copy;"),
        "\" ' \u{20ac} \u{00a9}"
    );
    assert_eq!(decode_ascii_entities("plain"), "plain");
    assert_eq!(decode_ascii_entities("&#65;&#x42;"), "ab");
}

#[test]
fn chunk_array_json_edge_cases() {
    assert_eq!(chunk_array_json("garbage"), r#"{"chunks":[]}"#);
    assert_eq!(
        chunk_array_json(r#"{"arr":[1,2],"size":0}"#),
        r#"{"chunks":[]}"#
    );
    assert_eq!(
        chunk_array_json(r#"{"arr":[1,2,3],"size":10}"#),
        r#"{"chunks":[[1,2,3]]}"#
    );
}

#[test]
fn force_layout_guards() {
    let mut nodes = Vec::new();
    for i in 0..501 {
        nodes.push(json!({"id": format!("n{i}"), "label": "x"}));
    }
    let big = json!({"nodes": nodes, "edges": [], "width": 800.0, "height": 600.0}).to_string();
    assert_eq!(calculate_force_layout_json(&big), "[]");
    let bad_w = r#"{"nodes":[{"id":"a","label":"A"}],"edges":[],"width":0,"height":600}"#;
    assert_eq!(calculate_force_layout_json(bad_w), "[]");
    let long_id = json!({
        "nodes": [
            {"id": "a", "label": "A"},
            {"id": "x".repeat(201), "label": "B"}
        ],
        "edges": [],
        "width": 100,
        "height": 100
    })
    .to_string();
    let res = calculate_force_layout_json(&long_id);
    assert!(res.contains("\"id\":\"a\""));
    assert!(!res.contains("\"id\":\"x"));
    let valid = json!({
        "nodes": [{"id": "a", "label": "A", "radius": 5, "color": "#f00"}],
        "edges": [],
        "width": 100.0,
        "height": 100.0,
        "iterations": 2,
        "initial_positions": [[10.0, 10.0]]
    })
    .to_string();
    let out = calculate_force_layout_json(&valid);
    assert!(out.contains("\"color\":\"#f00\""));
    assert!(out.contains("\"radius\":5.0"));
}

#[test]
fn format_extra_rules() {
    assert!(now_secs() > 1_500_000_000);
    assert_eq!(pluralize(1, "post", None), "post");
    assert_eq!(pluralize(2, "post", None), "posts");
    assert_eq!(pluralize(2, "box", Some("boxes")), "boxes");
    assert_eq!(format_duration(65.0), "1:05");
    assert_eq!(format_duration(0.0), "");
    assert_eq!(format_duration(f64::NAN), "");
    assert_eq!(format_timestamp(1000, 1300), "5m ago");
    assert_eq!(format_timestamp(1000, 3700), "45m ago");
    assert_eq!(format_timestamp(1000, 90_000), "1d ago");
    assert_eq!(format_timestamp(0, 700_000), "Jan 1, 1970");
    assert_eq!(format_timestamp(MAX_TIMESTAMP_SECS + 1, 0), "");
    assert_eq!(seconds_to_ymd(86_400 * 366), (1971, "Jan", 2));
    assert_eq!(seconds_to_ymd(MAX_TIMESTAMP_SECS), (9999, "Dec", 31));
    assert_eq!(truncate("héllo", 2), "h…");
    assert_eq!(truncate("", 5), "");
    assert_eq!(truncate("abc", 0), "");
}

#[test]
fn format_json_wrappers() {
    assert_eq!(
        truncate_json(r#"{"str":"hello world","maxLen":5}"#),
        "hell…"
    );
    assert_eq!(truncate_json("garbage"), "");
    assert_eq!(pluralize_json(r#"{"count":2,"singular":"post"}"#), "posts");
    assert_eq!(pluralize_json("garbage"), "");
    assert_eq!(
        format_timestamp_json(r#"{"seconds":1000,"nowSec":1300}"#),
        "5m ago"
    );
    assert_eq!(format_timestamp_json("garbage"), "");
    assert_eq!(format_duration_json(r#"{"seconds":65.0}"#), "1:05");
    assert_eq!(format_duration_json("garbage"), "");
}

#[test]
fn hashtag_length_cap() {
    let long = format!("#{}", "a".repeat(60));
    assert_eq!(extract_hashtags(&long), vec!["a".repeat(50)]);
}

#[test]
fn mention_empty_and_dupes() {
    assert!(parse_mentions("").is_empty());
    let pks = extract_pubkeys("npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6 npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6");
    assert_eq!(pks.len(), 2);
}

#[test]
fn ast_parser_whitespace_and_literals() {
    let spans = parse_post_ast("hi @bob ");
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[1].text, "@bob ");
    assert_eq!(spans[1].target.as_deref(), Some("@bob"));
    let lit = parse_post_ast("**bold** _em_ `code`");
    assert!(lit.iter().all(|s| s.span_type == SpanType::Text));
    let link = parse_post_ast("ftp://x.com");
    assert_eq!(link[0].span_type, SpanType::Text);
}

#[test]
fn mime_magic_bytes() {
    let png = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x01];
    assert_eq!(sniff_mime_type(&png, ""), "image/png");
    assert_eq!(sniff_mime_type(b"GIF89a", ""), "image/gif");
    let riff = |tag: [u8; 4]| -> Vec<u8> {
        let mut v = vec![0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0];
        v.extend_from_slice(&tag);
        v
    };
    assert_eq!(sniff_mime_type(&riff(*b"WEBP"), ""), "image/webp");
    assert_eq!(sniff_mime_type(&riff(*b"WAVE"), ""), "audio/wav");
    assert_eq!(
        sniff_mime_type(&[0, 0, 0, 0x18, 0x66, 0x74, 0x79, 0x70], ""),
        "video/mp4"
    );
    assert_eq!(sniff_mime_type(&[0x1a, 0x45, 0xdf, 0xa3], ""), "video/webm");
    assert_eq!(
        sniff_mime_type(&[0, 0, 0, 0x14, 0x6d, 0x6f, 0x6f, 0x76], ""),
        "video/quicktime"
    );
    assert_eq!(sniff_mime_type(b"fLaC", ""), "audio/flac");
    assert_eq!(sniff_mime_type(b"OggS", ""), "audio/ogg");
    assert_eq!(sniff_mime_type(b"ID3\x04\x00\x00", ""), "audio/mpeg");
    assert_eq!(sniff_mime_type(&[0xff, 0xfb, 0x90], ""), "audio/mpeg");
    assert_eq!(
        sniff_mime_type(&[], "image/png"),
        "application/octet-stream"
    );
    assert_eq!(
        sniff_mime_type(b"\x01\x02\x03", "video/x-custom"),
        "application/octet-stream"
    );
    assert_eq!(
        sniff_mime_type(b"\x01\x02\x03\x04", "audio/x-custom"),
        "audio/x-custom"
    );
}

#[test]
fn mime_json_wrapper_and_detect() {
    assert_eq!(
        sniff_mime_type_json(r#"{"bytes":[255,216,255,224],"fallback":"image/jpeg"}"#),
        "image/jpeg"
    );
    assert_eq!(sniff_mime_type_json("not json"), "application/octet-stream");
    assert_eq!(detect_mime_type("a.svg"), "image/svg+xml");
    assert_eq!(detect_mime_type("b.MP3"), "audio/mpeg");
    assert_eq!(detect_mime_type("c.pdf"), "application/pdf");
    assert_eq!(detect_mime_type("d.json"), "application/json");
    assert_eq!(detect_mime_type("e.txt"), "text/plain");
    assert_eq!(detect_mime_type("f.html"), "text/html");
    assert_eq!(detect_mime_type("g.heic"), "image/heic");
    assert_eq!(detect_mime_type("h.opus"), "audio/opus");
    assert_eq!(detect_mime_type("i.unknown"), "");
    assert_eq!(detect_mime_type("noext"), "");
    assert_eq!(detect_mime_type("j."), "");
}

#[test]
fn regex_util_is_match() {
    assert!(is_match(r"\d+", "abc123"));
    assert!(!is_match(r"\d+", "abc"));
    assert!(!is_match("[invalid", "abc"));
}

#[test]
fn safe_json_json_wrapper() {
    assert_eq!(
        safe_json_parse_json(r#"{"text":"{\"a\":1}"}"#),
        r#"{"a":1}"#
    );
    assert_eq!(safe_json_parse_json(r#"{"text":"not json"}"#), "null");
    assert_eq!(safe_json_parse_json("garbage"), "null");
}

#[test]
fn json_util_direct() {
    assert_eq!(json_in("garbage", 42i64), 42);
    let v = vec![1, 2];
    assert_eq!(json_out(&v, "[]"), "[1,2]");
    let parsed = json_in_borrow::<serde_json::Value>(r#"{"a":1}"#).unwrap();
    assert_eq!(parsed["a"], 1);
    assert!(json_in_borrow::<serde_json::Value>("garbage").is_none());
}

#[test]
fn sanitize_error_message_rules() {
    let hex64 = "a".repeat(64);
    assert_eq!(
        sanitize_error_message(&format!("private_key={hex64}")),
        "[REDACTED]"
    );
    assert!(sanitize_error_message("api_key: ABCDEFGH123").contains("[REDACTED]"));
    assert!(sanitize_error_message("read /etc/passwd").contains("[REDACTED]"));
    assert!(sanitize_error_message("conn mongodb://u:p@h/db").contains("[REDACTED]"));
    assert_eq!(
        sanitize_error_message("https://x.com?key=abc"),
        "https://x.com[REDACTED]"
    );
    let long = sanitize_error_message(&"x".repeat(600));
    assert!(long.ends_with("... [truncated]"));
    assert_eq!(long.len(), 515);
}

#[test]
fn sanitize_log_message_rules() {
    let hex64 = "a".repeat(64);
    assert_eq!(sanitize_log_message(&hex64), "[REDACTED_KEY]");
    assert_eq!(
        sanitize_log_message(&format!("0x{hex64}")),
        "[REDACTED_KEY]"
    );
    assert_eq!(sanitize_log_message("short hex 1234"), "short hex 1234");
}

#[test]
fn scrub_sensitive_data_rules() {
    let hex64 = "a".repeat(64);
    assert!(scrub_sensitive_data(&format!("privkey={hex64}")).contains("[REDACTED_KEY]"));
    assert!(
        scrub_sensitive_data("Authorization: Bearer abc123xyz").contains("Bearer [REDACTED_TOKEN]")
    );
    assert!(scrub_sensitive_data("conn from 192.168.1.1").contains("[REDACTED_IP]"));
    assert!(scrub_sensitive_data("conn from 127.0.0.1").contains("[REDACTED_IP]"));
    assert!(scrub_sensitive_data("conn from 10.1.2.3").contains("[REDACTED_IP]"));
}

#[test]
fn sanitize_details_rules() {
    assert_eq!(sanitize_details(""), "");
    let b64 = "A".repeat(32);
    assert_eq!(sanitize_details(&format!("token {b64}")), "token [BASE64]");
    assert_eq!(sanitize_details("/etc/x"), "/etc/x");
}

#[test]
fn sanitize_context_rules() {
    assert_eq!(sanitize_context(""), None);
    assert_eq!(sanitize_context("null"), None);
    assert_eq!(sanitize_context("[1,2,3]"), None);
    let ctx = sanitize_context(r#"{"apiKey":"abc","password":"x","name":"bob"}"#).unwrap();
    assert!(ctx.contains("\"apiKey\":\"[REDACTED]\""));
    assert!(ctx.contains("\"password\":\"[REDACTED]\""));
    assert!(ctx.contains("\"name\":\"bob\""));
}

#[test]
fn link_preview_direct_and_guards() {
    let out = parse_link_preview_html("no meta", "https://example.com/x").unwrap();
    assert_eq!(out.title, "https://example.com/x");
    assert_eq!(out.description, "");
    assert_eq!(out.image, None);
    assert_eq!(out.domain, "example.com");
    assert_eq!(
        out.favicon.as_deref(),
        Some("https://example.com/favicon.ico")
    );
    let www = parse_link_preview_html("<title>t</title>", "https://www.example.com/p").unwrap();
    assert_eq!(www.title, "t");
    assert_eq!(www.domain, "example.com");
    let with_meta = parse_link_preview_html(
        r#"<meta name="description" content="Desc"><meta property="og:image" content="javascript:alert(1)">"#,
        "https://example.com/",
    )
    .unwrap();
    assert_eq!(with_meta.description, "Desc");
    assert_eq!(with_meta.image, None);
    assert!(
        parse_link_preview_html(&"a".repeat(5 * 1024 * 1024 + 1), "https://example.com").is_none()
    );
    assert_eq!(parse_link_preview_html_json("not json"), "null");
}

#[test]
fn favicon_variants() {
    let rel = r#"<link rel="icon" href="/static/f.png">"#;
    assert_eq!(
        extract_favicon(rel, "https://example.com/page"),
        Some("https://example.com/static/f.png".to_string())
    );
    let js = r#"<link rel="icon" href="javascript:void(0)">"#;
    assert_eq!(extract_favicon(js, "https://example.com/page"), None);
    let data = r#"<link rel="icon" href="data:image/png;base64,xx">"#;
    assert_eq!(extract_favicon(data, "https://example.com/page"), None);
    assert_eq!(
        extract_favicon("<html></html>", "https://example.com/page"),
        Some("https://example.com/favicon.ico".to_string())
    );
}

#[test]
fn imeta_json_wrapper() {
    let input = r#"{"tags":[["imeta","url=https://x/v.mp4","m=video/mp4"],["imeta","url=https://x/a.png","m=image/png"],["t","nostr"]]}"#;
    let out = extract_imeta_video_urls_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    let many = vec![vec![String::new()]; 100_001];
    let big = serde_json::to_string(&json!({"tags": many})).unwrap();
    assert_eq!(extract_imeta_video_urls_json(&big), "[]");
}

#[test]
fn linkpreview_urls_extract_fn() {
    let urls = soshal_content_core::linkpreview::urls::extract_urls(
        "go https://x.com/a and https://x.com/a",
    );
    assert_eq!(urls, vec!["https://x.com/a"]);
}

#[test]
fn stories_valid_processing() {
    let input = json!({
        "events": [
            {"id": "expired", "pubkey": "pk", "content": "{\"text\":\"old\"}", "created_at": 50, "tags": [["expiration", "60"]]},
            {"id": "a", "pubkey": "pk", "content": "{\"text\":\"hi\"}", "created_at": 100, "tags": []},
            {"id": "b", "pubkey": "pk", "content": "{\"media\":[{\"url\":\"https://x/y.mp4\",\"type\":\"video/mp4\",\"duration\":3.0}]}", "created_at": 200, "tags": []}
        ],
        "now_sec": 100,
        "expiry_seconds": 3600
    });
    let out: serde_json::Value =
        serde_json::from_str(&filter_stories_json(&input.to_string())).unwrap();
    let events = out.as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"], "a");
    assert_eq!(events[1]["id"], "b");
    assert_eq!(events[0]["text"], "hi");
    assert_eq!(events[0]["expires_at_ms"], 3_700_000.0);
    assert_eq!(events[1]["media"][0]["url"], "https://x/y.mp4");
    assert_eq!(events[1]["media"][0]["type"], "video/mp4");
    assert_eq!(events[1]["created_at_ms"], 200_000.0);

    // Story without expiration tag created at t=10 is expired when now_sec=100 and expiry_seconds=50
    let input_old = json!({
        "events": [
            {"id": "ancient_no_tag", "pubkey": "pk", "content": "{\"text\":\"old\"}", "created_at": 10, "tags": []},
            {"id": "fresh_no_tag", "pubkey": "pk", "content": "{\"text\":\"fresh\"}", "created_at": 80, "tags": []}
        ],
        "now_sec": 100,
        "expiry_seconds": 50
    });
    let out_old: serde_json::Value =
        serde_json::from_str(&filter_stories_json(&input_old.to_string())).unwrap();
    let events_old = out_old.as_array().unwrap();
    assert_eq!(events_old.len(), 1);
    assert_eq!(events_old[0]["id"], "fresh_no_tag");
}

#[test]
fn css_color_rules() {
    assert!(is_valid_css_color("#fff"));
    assert!(is_valid_css_color("#abcd"));
    assert!(is_valid_css_color("#aabbcc"));
    assert!(is_valid_css_color("#aabbccdd"));
    assert!(is_valid_css_color("hsl(120, 50%, 50%)"));
    assert!(is_valid_css_color("hsla(0, 0%, 0%, 0.5)"));
    assert!(is_valid_css_color("hsl(120deg, 50%, 50%)"));
    assert!(!is_valid_css_color("#12"));
    assert!(!is_valid_css_color("#ggg"));
    assert!(!is_valid_css_color("red"));
    assert!(!is_valid_css_color("url(http://x)"));
    assert!(!is_valid_css_color("hsl(361, 50%, 50%)"));
    assert!(!is_valid_css_color("hsl(120, 150%, 50%)"));
    assert!(!is_valid_css_color(""));
    assert!(!is_valid_css_color(&"#".repeat(65)));
}

#[test]
fn ui_safe_string_helpers() {
    assert_eq!(short_pk("", 5), "");
    assert_eq!(short_pk("abc", 0), "");
    assert_eq!(short_pk("héllo", 3), "hé");
    assert_eq!(truncate_str("hello world", 5), "hello");
    assert_eq!(truncate_str("héllo", 3), "hé");
    assert_eq!(truncate_str("x", 0), "");
    assert_eq!(truncate_str("", 5), "");
    assert_eq!(js_string_literal("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
}

#[test]
fn url_ssrf_private_ip() {
    assert!(is_private_ip_str("127.0.0.1"));
    assert!(is_private_ip_str("10.1.2.3"));
    assert!(is_private_ip_str("192.168.0.1"));
    assert!(is_private_ip_str("172.16.0.1"));
    assert!(is_private_ip_str("172.31.255.255"));
    assert!(is_private_ip_str("169.254.169.254"));
    assert!(is_private_ip_str("100.64.0.1"));
    assert!(is_private_ip_str("0.0.0.0"));
    assert!(is_private_ip_str("224.0.0.1"));
    assert!(!is_private_ip_str("8.8.8.8"));
    assert!(!is_private_ip_str("172.32.0.1"));
    assert!(!is_private_ip_str("example.com"));
    assert!(!is_private_ip_str(""));
    assert!(is_private_ip_str("[::1]"));
    assert!(is_private_ip_str("::ffff:127.0.0.1"));
    assert!(is_private_ip_str("fc00::1"));
    assert!(is_private_ip_str("fe80::1"));
    assert!(is_private_ip_str("2001::1"));
    assert!(!is_private_ip_str("2001:db8::1"));
    assert!(is_private_ip_str("2002:7f00:1::1"));
    assert!(!is_private_ip_str("2606:4700::1"));
}

#[test]
fn url_is_private_ipv6() {
    assert!(is_private_ipv6_str("::1"));
    assert!(is_private_ipv6_str("fe80::1"));
    assert!(is_private_ipv6_str("fc00::1"));
    assert!(is_private_ipv6_str("ff02::1"));
    assert!(is_private_ipv6_str("::"));
    assert!(is_private_ipv6_str("2001::1"));
    assert!(is_private_ipv6_str("::ffff:10.0.0.1"));
    assert!(!is_private_ipv6_str("2001:db8::1"));
    assert!(!is_private_ipv6_str("example.com"));
}

#[test]
fn url_sanitize_link_and_href() {
    assert_eq!(
        sanitize_link_url("https://example.com/a.png"),
        Some("https://example.com/a.png".to_string())
    );
    assert_eq!(sanitize_link_url("javascript:alert(1)"), None);
    assert_eq!(sanitize_link_url("data:text/html,x"), None);
    assert_eq!(sanitize_link_url("http://localhost/x"), None);
    assert_eq!(sanitize_link_url("https://127.0.0.1/x"), None);
    assert_eq!(sanitize_link_url("https://evil.nip.io/x"), None);
    assert_eq!(
        safe_href("https://example.com/x"),
        Some("https://example.com/x".to_string())
    );
    assert_eq!(safe_href("ftp://example.com/x"), None);
    assert_eq!(safe_href("https://10.0.0.1/x"), None);
}

#[test]
fn url_relay_edge_cases() {
    assert_eq!(is_valid_relay_url(""), (false, false));
    assert_eq!(is_valid_relay_url("https://relay.com"), (false, false));
    assert_eq!(
        is_valid_relay_url("wss://user:pass@relay.com"),
        (false, false)
    );
    assert_eq!(is_valid_relay_url("wss://relay"), (false, false));
    assert_eq!(is_valid_relay_url("wss://1.2.3.4"), (false, false));
    assert_eq!(is_valid_relay_url("wss://localhost"), (false, false));
    assert_eq!(is_valid_relay_url("wss://192.168.1.1"), (false, false));
    assert_eq!(is_valid_relay_url("wss://evil.nip.io"), (false, false));
    assert_eq!(is_valid_relay_url("ws://relay.damus.io"), (true, false));
    assert_eq!(is_valid_relay_url("wss://xn--relay-9db.com"), (true, true));
    assert_eq!(is_valid_relay_url("wss://relay.example.com"), (true, false));
    assert!(!is_valid_event_relay_url("ws://relay.damus.io"));
}

#[test]
fn url_extract_dedupe_and_cap() {
    let dupes = extract_urls("https://a.com https://a.com https://b.com");
    assert_eq!(dupes, vec!["https://a.com", "https://b.com"]);
    let many = (0..1030)
        .map(|i| format!("https://example.com/{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(extract_urls(&many).len(), 1024);
}

#[test]
fn url_domain_and_valid() {
    assert!(is_valid("https://example.com"));
    assert!(is_valid("javascript:alert(1)"));
    assert!(!is_valid("not a url"));
    assert_eq!(domain("not a url"), None);
    assert_eq!(
        domain("https://example.com/path"),
        Some("example.com".to_string())
    );
}

#[test]
fn decode_html_entities_remaining_named_and_invalid() {
    let named =
        "&ndash;&lsquo;&hellip;&trade;&pound;&yen;&cent;&sect;&deg;&plusmn;&frac12;&divide;";
    assert_eq!(
        decode_html_entities(named),
        "\u{2013}\u{2018}\u{2026}\u{2122}\u{00a3}\u{00a5}\u{00a2}\u{00a7}\u{00b0}\u{00b1}\u{00bd}\u{00f7}"
    );
    assert_eq!(decode_html_entities("&#xGG;"), "&#xGG;");
    assert_eq!(decode_html_entities("&#xD800;"), "&#xD800;");
    assert_eq!(decode_html_entities("&#x110000;"), "&#x110000;");
    assert_eq!(decode_html_entities("&#4294967296;"), "&#4294967296;");
}

#[test]
fn decode_html_entities_window_and_multibyte() {
    assert_eq!(decode_html_entities("héllo ✓"), "héllo ✓");
    let beyond = format!("&{};", "a".repeat(25));
    assert_eq!(decode_html_entities(&beyond), beyond);
    let numeric_beyond = format!("&#{};", "1".repeat(23));
    assert_eq!(decode_html_entities(&numeric_beyond), numeric_beyond);
}

#[test]
fn decode_ascii_entities_rules() {
    assert_eq!(decode_ascii_entities("&#X41;"), "a");
    assert_eq!(
        decode_ascii_entities("&#12345678901234567890;"),
        "&#1234567890123456;"
    );
    assert_eq!(decode_ascii_entities("&#65"), "a");
    assert_eq!(decode_ascii_entities("&#128;"), "&#128;");
    assert_eq!(decode_ascii_entities("&#"), "&#");
    assert_eq!(decode_ascii_entities("&#z;"), "&#z;");
}

#[test]
fn sanitize_log_message_nsec_redaction() {
    let nsec = "nsec1qwqsvf30y2pqf30y2pqf30y2pqf30y2pqf30y2pqf30y2pveerx";
    assert_eq!(sanitize_log_message(nsec), "[REDACTED_KEY]");
    let short = format!("nsec1{}", "a".repeat(39));
    assert_eq!(sanitize_log_message(&short), short);
}

#[test]
fn sanitize_notif_content_multibyte_cut() {
    assert_eq!(sanitize_notif_content("héllo", 2), "h");
    assert_eq!(sanitize_notif_content("éééé", 5), "éé");
    assert_eq!(
        sanitize_notif_content("<b>héllo</b> wörld", 100),
        "héllo wörld"
    );
}

#[test]
fn sanitize_edge_rules() {
    assert_eq!(sanitize_details(&format!("/{}", "a".repeat(100))), "/[HEX]");
    assert_eq!(
        sanitize_details(&format!("/{}", "a".repeat(63))),
        "/[BASE64]"
    );
    let a31 = "A".repeat(31);
    assert_eq!(sanitize_details(&a31), a31);
    assert_eq!(
        sanitize_details(&format!("{}==", "A".repeat(32))),
        "[BASE64]"
    );
    let hex64 = "a".repeat(64);
    assert_eq!(sanitize_error_message(&format!("sk={hex64}")), "[REDACTED]");
    assert_eq!(
        sanitize_error_message(&format!("seckey={hex64}")),
        "[REDACTED]"
    );
    assert_eq!(
        sanitize_error_message(&format!("secret_key={hex64}")),
        "[REDACTED]"
    );
    assert_eq!(sanitize_context("\"str\""), None);
    assert_eq!(sanitize_context("42"), None);
    assert_eq!(
        sanitize_context(r#"{"key":123}"#).unwrap(),
        r#"{"key":"[REDACTED]"}"#
    );
    assert_eq!(scrub_sensitive_data("10.999.1.1"), "[REDACTED_IP]");
    assert_eq!(scrub_sensitive_data("10.9999.1.1"), "10.9999.1.1");
}
