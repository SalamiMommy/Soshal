//! Integration tests for webrtc-core SDP sanitization and Opus configuration.

use soshal_webrtc_core::ice::{
    ice_config, is_private_ip, is_private_ipv6, is_safe_candidate, is_safe_candidate_json,
    redact_private_ips,
};
use soshal_webrtc_core::sdp::{
    configure_opus_audio_sdp, configure_opus_audio_sdp_json, extract_candidates, extract_rtpmap_pt,
    sanitize_sdp, sanitize_sdp_json, validate_sdp,
};

#[test]
fn sanitize_sdp_rewrites_private_ip4_con_line() {
    let sdp = "v=0\r\nc=IN IP4 192.168.1.50\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("c=IN IP4 127.0.0.1"));
    assert!(!out.contains("192.168.1.50"));
}

#[test]
fn sanitize_sdp_rewrites_private_ip6_con_line() {
    let sdp = "v=0\r\nc=IN IP6 fe80::1\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("c=IN IP6 ::1"));
    assert!(!out.contains("fe80::1"));
}

#[test]
fn sanitize_sdp_keeps_public_con_line() {
    let sdp = "v=0\r\nc=IN IP4 8.8.8.8\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("c=IN IP4 8.8.8.8"));
}

#[test]
fn sanitize_sdp_fixes_typo_con_prefix() {
    let sdp = "v=0\r\nc=INIP4 10.0.0.5\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("c=IN IP4 127.0.0.1"));
}

#[test]
fn sanitize_sdp_drops_host_candidates() {
    let sdp = "v=0\r\na=candidate:1 1 UDP 2122260223 10.0.0.5 5000 typ host\r\n\
a=candidate:2 1 UDP 1686052607 203.0.113.9 5000 typ srflx raddr 8.8.8.8 rport 5000\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(!out.contains("typ host"));
    assert!(out.contains("typ srflx"));
}

#[test]
fn sanitize_sdp_force_relay_also_drops_srflx() {
    let sdp = "v=0\r\na=candidate:2 1 UDP 1686052607 203.0.113.9 5000 typ srflx\r\n\
a=candidate:3 1 UDP 1694498815 8.8.8.8 5000 typ relay\r\n";
    let out = sanitize_sdp(sdp, true);
    assert!(!out.contains("srflx"));
    assert!(out.contains("typ relay"));
}

#[test]
fn sanitize_sdp_keeps_relay_candidate_without_force() {
    let sdp = "v=0\r\na=candidate:3 1 UDP 1694498815 8.8.8.8 5000 typ relay\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("typ relay"));
}

#[test]
fn sanitize_sdp_rewrites_private_candidate_with_private_ip() {
    let sdp = "v=0\r\na=candidate:4 1 UDP 1 192.168.1.10 5000 typ host config stuff\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(!out.contains("192.168.1.10"));
}

#[test]
fn sanitize_sdp_rewrites_origin_ip4_address() {
    let sdp = "v=0\r\no=- 1234 5678 IN IP4 192.168.1.5\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("o=- 1234 5678 IN IP4 0.0.0.0"));
    assert!(!out.contains("192.168.1.5"));
}

#[test]
fn sanitize_sdp_rewrites_origin_ip6_address() {
    let sdp = "v=0\r\no=- 1 2 IN IP6 fe80::aabb\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("IN IP6 ::"));
    assert!(!out.contains("fe80::aabb"));
}

#[test]
fn sanitize_sdp_rewrites_origin_address_to_placeholder() {
    let sdp = "v=0\r\no=- 1 2 IN IP4 93.184.216.34\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("0.0.0.0"));
    assert!(!out.contains("93.184.216.34"));
}

#[test]
fn sanitize_sdp_normalizes_newlines_to_crlf() {
    let sdp = "v=0\nc=IN IP4 8.8.8.8\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n";
    let out = sanitize_sdp(sdp, false);
    assert!(out.contains("v=0\r\nc=IN IP4 8.8.8.8\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111"));
}

#[test]
fn sanitize_sdp_json_roundtrip() {
    let input = serde_json::json!({"sdp": "v=0\r\nc=IN IP4 10.0.0.9\r\n", "forceRelay": false});
    let out = sanitize_sdp_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["sanitized_sdp"].as_str().unwrap().contains("127.0.0.1"));
}

#[test]
fn sanitize_sdp_json_garbage_input_empty_result() {
    let v: serde_json::Value = serde_json::from_str(&sanitize_sdp_json("nope")).unwrap();
    assert_eq!(v["sanitized_sdp"], "");
}

#[test]
fn extract_candidates_returns_candidate_lines() {
    let sdp = "v=0\r\na=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\r\n\
m=audio 0 RTP/AVP 0\r\na=candidate:2 1 UDP 2130706431 66.154.114.51 54322 typ relay";
    let out = extract_candidates(sdp);
    assert_eq!(out.len(), 2);
    assert!(out[0].starts_with("a=candidate:1"));
    assert!(out[1].starts_with("a=candidate:2"));
}

#[test]
fn extract_candidates_none_returns_empty() {
    assert_eq!(
        extract_candidates("v=0\nm=audio 0 RTP/AVP 0"),
        Vec::<String>::new()
    );
    assert_eq!(extract_candidates(""), Vec::<String>::new());
}

#[test]
fn validate_sdp_checks_session_lines() {
    assert!(validate_sdp("v=0\r\no=- 0 0 IN IP4 127.0.0.1"));
    assert!(!validate_sdp("v=0 only"));
    assert!(!validate_sdp("o=- 0 0 IN IP4 127.0.0.1"));
    assert!(!validate_sdp(""));
}

#[test]
fn extract_rtpmap_pt_parses_payload_type() {
    assert_eq!(extract_rtpmap_pt("a=rtpmap:111 opus/48000/2"), Some("111"));
    assert_eq!(extract_rtpmap_pt("a=rtpmap:0 PCMU/8000"), Some("0"));
    assert_eq!(extract_rtpmap_pt("a=rtpmap:"), None);
    assert_eq!(extract_rtpmap_pt("not a rtpmap"), None);
    assert_eq!(extract_rtpmap_pt("a=rtpmap:abc opus/48000"), None);
    assert_eq!(extract_rtpmap_pt("a=rtpmap:1111 opus/48000"), Some("1111"));
}

#[test]
fn configure_opus_audio_sdp_high_fidelity_params() {
    let sdp = "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n\
a=fmtp:111 minptime=10;useinbandfec=1\r\n";
    let out = configure_opus_audio_sdp(sdp, true);
    assert!(
        out.contains("minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1;maxaveragebitrate=128000")
    );
}

#[test]
fn configure_opus_audio_sdp_low_fidelity_params() {
    let sdp = "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n";
    let out = configure_opus_audio_sdp(sdp, false);
    assert!(
        out.contains("minptime=20;useinbandfec=1;stereo=0;sprop-stereo=0;maxaveragebitrate=24000")
    );
}

#[test]
fn configure_opus_audio_sdp_inserts_fmtp_after_rtpmap() {
    let sdp = "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n";
    let out = configure_opus_audio_sdp(sdp, true);
    let fmtp_idx = out.find("a=fmtp:111").unwrap();
    let rtpmap_idx = out.find("a=rtpmap:111").unwrap();
    assert!(rtpmap_idx < fmtp_idx);
}

#[test]
fn configure_opus_audio_sdp_no_opus_returns_unchanged() {
    let sdp = "m=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=rtpmap:0 PCMU/8000\r\n";
    assert_eq!(configure_opus_audio_sdp(sdp, true), sdp);
}

#[test]
fn configure_opus_audio_sdp_empty_returns_empty() {
    assert_eq!(configure_opus_audio_sdp("", true), "");
}

#[test]
fn configure_opus_audio_sdp_json_roundtrip() {
    let sdp = "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n";
    let input = serde_json::json!({"sdp": sdp, "highFidelity": true});
    let out = configure_opus_audio_sdp_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["sdp"]
        .as_str()
        .unwrap()
        .contains("maxaveragebitrate=128000"));
}

#[test]
fn configure_opus_audio_sdp_json_garbage_input() {
    let v: serde_json::Value = serde_json::from_str(&configure_opus_audio_sdp_json("zzz")).unwrap();
    assert_eq!(v["sdp"], "");
}

#[test]
fn test_is_safe_candidate_json() {
    use soshal_webrtc_core::ice::is_safe_candidate_json;
    let input = serde_json::json!({
        "candidate": "candidate:1 1 UDP 2122260223 8.8.8.8 5000 typ srflx",
        "forceRelay": false
    });
    let out = is_safe_candidate_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["safe"], true);

    assert_eq!(is_safe_candidate_json("garbage"), r#"{"safe":false}"#);
}

#[test]
fn test_configure_opus_oversized_sdp_returns_empty_json() {
    let huge_sdp = "a".repeat(1024 * 1024 + 1);
    let input = serde_json::json!({"sdp": huge_sdp, "highFidelity": true});
    let out = configure_opus_audio_sdp_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sdp"], "");
}

// ─── ice module coverage ────────────────────────────────────────────────────

#[test]
fn is_private_ip_detects_private_ranges() {
    assert!(is_private_ip("192.168.1.50"));
    assert!(is_private_ip("10.0.0.1"));
    assert!(is_private_ip("172.16.0.1"));
    assert!(is_private_ip("127.0.0.1"));
    assert!(is_private_ip("169.254.10.20"));
    assert!(!is_private_ip("8.8.8.8"));
    assert!(!is_private_ip("203.0.113.9"));
    assert!(!is_private_ip("93.184.216.34"));
    assert!(!is_private_ip("not-an-ip"));
    assert!(!is_private_ip(""));
}

#[test]
fn is_private_ipv6_detects_private_ranges() {
    assert!(is_private_ipv6("fe80::1"));
    assert!(is_private_ipv6("::1"));
    assert!(is_private_ipv6("fc00::1"));
    assert!(is_private_ipv6("ff02::1"));
    assert!(!is_private_ipv6("2606:4700::1111"));
    assert!(!is_private_ipv6("garbage"));
    assert!(!is_private_ipv6(""));
}

#[test]
fn is_safe_candidate_rejects_host_and_private_ips() {
    assert!(!is_safe_candidate("", false));
    assert!(!is_safe_candidate(
        "candidate:1 1 UDP 2122260223 10.0.0.5 5000 typ host",
        false
    ));
    assert!(!is_safe_candidate(
        "candidate:2 1 UDP 1686052607 192.168.1.10 5000 typ srflx",
        false
    ));
    assert!(is_safe_candidate(
        "candidate:3 1 UDP 1694498815 8.8.8.8 5000 typ relay",
        false
    ));
    assert!(is_safe_candidate(
        "candidate:4 1 UDP 1686052607 203.0.113.9 5000 typ srflx",
        false
    ));
}

#[test]
fn is_safe_candidate_force_relay_drops_srflx_keeps_relay() {
    assert!(!is_safe_candidate(
        "candidate:1 1 UDP 1686052607 203.0.113.9 5000 typ srflx",
        true
    ));
    assert!(is_safe_candidate(
        "candidate:2 1 UDP 1694498815 8.8.8.8 5000 typ relay",
        true
    ));
    assert!(!is_safe_candidate(
        "candidate:3 1 UDP 1 8.8.8.8 5000 typ host",
        true
    ));
}

#[test]
fn is_safe_candidate_non_ip_strings_pass() {
    assert!(is_safe_candidate("garbage", false));
    assert!(is_safe_candidate(
        "a=candidate:1 1 UDP 2130706431 8.8.8.8 5000 typ srflx extra",
        false
    ));
}

#[test]
fn redact_private_ips_replaces_private_keeps_public() {
    let out = redact_private_ips("candidate:1 1 UDP 2122260223 192.168.1.10 5000 typ host");
    assert!(!out.contains("192.168.1.10"));
    assert!(out.contains("0.0.0.0"));

    let out = redact_private_ips("c=IN IP4 10.0.0.5");
    assert_eq!(out, "c=IN IP4 0.0.0.0");

    let out = redact_private_ips("c=IN IP4 8.8.8.8");
    assert_eq!(out, "c=IN IP4 8.8.8.8");
}

#[test]
fn redact_private_ips_handles_ipv6_and_mixed_text() {
    assert_eq!(redact_private_ips("c=IN IP6 fe80::1"), "c=IN IP6 0.0.0.0");
    let out = redact_private_ips("peer 10.0.0.5 relays 8.8.8.8");
    assert_eq!(out, "peer 0.0.0.0 relays 8.8.8.8");
    assert_eq!(redact_private_ips("abc123 no ip here"), "abc123 no ip here");
    assert_eq!(redact_private_ips(""), "");
}

#[test]
fn ice_config_public_keeps_stun_servers() {
    let cfg = ice_config("public", "stun:stun.l.google.com:19302");
    assert_eq!(cfg["iceTransportPolicy"], "all");
    assert_eq!(cfg["forceRelay"], false);
    assert_eq!(cfg["iceServers"][0]["urls"], "stun:stun.l.google.com:19302");
}

#[test]
fn ice_config_private_forces_relay_without_servers() {
    let cfg = ice_config("private", "");
    assert_eq!(cfg["iceTransportPolicy"], "relay");
    assert_eq!(cfg["forceRelay"], true);
    assert_eq!(cfg["iceServers"].as_array().unwrap().len(), 0);
}

// ─── error paths ────────────────────────────────────────────────────────────

#[test]
fn validate_sdp_rejects_malformed_sessions() {
    assert!(validate_sdp("v=0\r\no=- 0 0 IN IP4 127.0.0.1"));
    assert!(!validate_sdp("v=0\r\n"));
    assert!(!validate_sdp("o=- 0 0 IN IP4 127.0.0.1"));
    assert!(!validate_sdp("v=1\r\no=- 0 0 IN IP4 127.0.0.1"));
    assert!(!validate_sdp("\r\n"));
}

#[test]
fn sanitize_sdp_json_missing_fields_returns_empty() {
    let out = sanitize_sdp_json(r#"{"sdp":"v=0\r\nc=IN IP4 10.0.0.9"}"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sanitized_sdp"], "");
}

#[test]
fn sanitize_sdp_json_force_relay_flag_respected() {
    let input = serde_json::json!({
        "sdp": "v=0\r\na=candidate:1 1 UDP 1686052607 203.0.113.9 5000 typ srflx\r\n",
        "forceRelay": true
    });
    let out = sanitize_sdp_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(!v["sanitized_sdp"].as_str().unwrap().contains("srflx"));
}

#[test]
fn configure_opus_audio_sdp_json_missing_high_fidelity_returns_empty() {
    let out = configure_opus_audio_sdp_json(r#"{"sdp":"m=audio 9 UDP/TLS/RTP/SAVPF 111"}"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sdp"], "");
}

#[test]
fn is_safe_candidate_json_private_candidate_reports_unsafe() {
    let input = serde_json::json!({
        "candidate": "candidate:1 1 UDP 2122260223 192.168.1.10 5000 typ srflx",
        "forceRelay": false
    });
    let v: serde_json::Value =
        serde_json::from_str(&is_safe_candidate_json(&input.to_string())).unwrap();
    assert_eq!(v["safe"], false);
}

// ─── roundtrips ─────────────────────────────────────────────────────────────

#[test]
fn sanitize_then_extract_candidates_roundtrip() {
    let sdp = "v=0\r\na=candidate:1 1 UDP 2122260223 10.0.0.5 5000 typ host\r\n\
a=candidate:2 1 UDP 1694498815 8.8.8.8 5000 typ relay\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n";
    let sanitized = sanitize_sdp(sdp, false);
    let candidates = extract_candidates(&sanitized);
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].contains("typ relay"));
    assert!(candidates[0].contains("8.8.8.8"));
}

#[test]
fn configure_opus_keeps_rtpmap_extractable() {
    let sdp = "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n";
    let out = configure_opus_audio_sdp(sdp, false);
    let mut rtpmap_lines = out
        .lines()
        .filter(|l| l.starts_with("a=rtpmap:"))
        .collect::<Vec<_>>();
    assert_eq!(rtpmap_lines.len(), 1);
    let pt = extract_rtpmap_pt(rtpmap_lines.remove(0)).unwrap();
    assert_eq!(pt, "111");
    assert!(out.contains("a=fmtp:111 "));
}
