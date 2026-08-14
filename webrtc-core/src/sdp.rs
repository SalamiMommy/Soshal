use serde::Deserialize;

use crate::ice::{is_private_ip, is_private_ipv6, is_safe_candidate};
use soshal_common_core::json_util::{json_in, json_out};

// ─── SDP Sanitization ───────────────────────────────────────────────────────

/// Removes host candidates and (optionally) srflx/relay candidates from an
/// SDP string, and rewrites `c=IN IP4` / `c=IN IP6` lines that reference
/// private/reserved addresses to loopback equivalents.
pub fn sanitize_sdp(sdp: &str, force_relay: bool) -> String {
    let mut lines = Vec::with_capacity(sdp.lines().count());
    for raw_line in sdp.lines() {
        let trimmed_line = raw_line.trim();
        let norm_storage;
        let line = if let Some(rest) = trimmed_line.strip_prefix("c=INIP4") {
            norm_storage = format!("c=IN IP4{rest}");
            &norm_storage
        } else if let Some(rest) = trimmed_line.strip_prefix("c=INIP6") {
            norm_storage = format!("c=IN IP6{rest}");
            &norm_storage
        } else {
            trimmed_line
        };
        if line.starts_with("a=candidate:") {
            let cand_str = line.strip_prefix("a=").unwrap_or(line);
            if !is_safe_candidate(cand_str, force_relay) {
                continue;
            }
        }
        if line.starts_with("c=IN") {
            if let Some(ip4_pos) = line.find("IP4") {
                let ip_part = &line[ip4_pos + 3..];
                let ip = ip_part.trim();
                if !ip.is_empty() && is_private_ip(ip) {
                    lines.push(format!("c=IN IP4 {}", "127.0.0.1"));
                    continue;
                }
            }
            if let Some(ip6_pos) = line.find("IP6") {
                let ip_part = &line[ip6_pos + 3..];
                let ip = ip_part.trim();
                if !ip.is_empty() && is_private_ipv6(ip) {
                    lines.push(format!("c=IN IP6 {}", "::1"));
                    continue;
                }
            }
        }
        // `o=` origin lines embed a unicast address that leaks the local LAN
        // IP to the remote peer. Always rewrite the address part to a
        // non-routable placeholder regardless of range.
        if line.starts_with("o=") {
            if let Some(ip4_pos) = line.find("IN IP4") {
                let tail = &line[ip4_pos + "IN IP4".len()..];
                let addr = tail.trim();
                if !addr.is_empty() {
                    lines.push(format!(
                        "o={}{}",
                        &line[..ip4_pos + "IN IP4".len()],
                        tail.replace(addr, "0.0.0.0")
                    ));
                    continue;
                }
            }
            if let Some(ip6_pos) = line.find("IN IP6") {
                let tail = &line[ip6_pos + "IN IP6".len()..];
                let addr = tail.trim();
                if !addr.is_empty() {
                    lines.push(format!(
                        "o={}{}",
                        &line[..ip6_pos + "IN IP6".len()],
                        tail.replace(addr, "::")
                    ));
                    continue;
                }
            }
        }
        lines.push(line.to_string());
    }
    lines.join("\r\n")
}

/// JSON-in/JSON-out wrapper for sanitize_sdp.
/// Input: `{"sdp": "...", "forceRelay": true/false}`
/// Output: `{"sanitized_sdp": "..."}`
pub fn sanitize_sdp_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct Input {
        sdp: String,
        #[serde(rename = "forceRelay")]
        force_relay: bool,
    }
    let Some(input) = json_in::<Option<Input>>(input, None) else {
        return json_out(
            &serde_json::json!({"sanitized_sdp": ""}),
            r#"{"sanitized_sdp": ""}"#,
        );
    };
    let result = sanitize_sdp(&input.sdp, input.force_relay);
    json_out(
        &serde_json::json!({"sanitized_sdp": result}),
        r#"{"sanitized_sdp": ""}"#,
    )
}

// ─── Opus SDP Configuration ─────────────────────────────────────────────────

const MAX_FFI_LEN: usize = 1024 * 1024;
const MAX_PT_LEN: usize = 16;
const MAX_LINE_LEN: usize = 1024;
const MAX_LINES: usize = 4_000;

fn safe_truncate_line(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn extract_rtpmap_pt(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("a=rtpmap:")?;
    let pt_end = rest.find(|c: char| c.is_whitespace())?;
    let pt = rest.get(..pt_end)?;
    if pt.is_empty() || !pt.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if pt.len() > MAX_PT_LEN {
        return None;
    }
    Some(pt)
}

pub fn configure_opus_audio_sdp(sdp: &str, high_fidelity: bool) -> String {
    if sdp.is_empty() {
        return String::new();
    }
    if sdp.len() > MAX_FFI_LEN {
        return sdp.to_string();
    }
    let raw_lines: Vec<&str> = sdp.split("\r\n").collect();
    if raw_lines.len() > MAX_LINES {
        return sdp.to_string();
    }

    let lines: Vec<&str> = raw_lines
        .iter()
        .take(MAX_LINES)
        .map(|l| safe_truncate_line(l, MAX_LINE_LEN))
        .collect();

    let mut opus_pt: Option<String> = None;
    for line in &lines {
        if line.starts_with("a=rtpmap:")
            && line
                .as_bytes()
                .windows(10)
                .any(|w| w.eq_ignore_ascii_case(b"opus/48000"))
        {
            if let Some(pt) = extract_rtpmap_pt(line) {
                opus_pt = Some(pt.to_string());
                break;
            }
        }
    }
    let opus_pt = match opus_pt {
        Some(p) => p,
        None => return sdp.to_string(),
    };
    let fmtp_params = if high_fidelity {
        "minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1;maxaveragebitrate=128000"
    } else {
        "minptime=20;useinbandfec=1;stereo=0;sprop-stereo=0;maxaveragebitrate=24000"
    };

    let fmtp_prefix_exact = format!("a=fmtp:{}", opus_pt);
    let fmtp_prefix_space = format!("a=fmtp:{} ", opus_pt);

    let rtpmap_prefix_exact = format!("a=rtpmap:{}", opus_pt);
    let rtpmap_prefix_space = format!("a=rtpmap:{} ", opus_pt);

    let mut fmtp_modified = false;
    let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());

    for line in &lines {
        if line.starts_with(&fmtp_prefix_space) || *line == fmtp_prefix_exact {
            fmtp_modified = true;
            new_lines.push(format!("{} {}", fmtp_prefix_exact, fmtp_params));
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !fmtp_modified {
        let mut inserted = false;
        let mut result: Vec<String> = Vec::with_capacity(new_lines.len() + 1);
        for line in &new_lines {
            result.push(line.clone());
            if !inserted && (line.starts_with(&rtpmap_prefix_space) || *line == rtpmap_prefix_exact)
            {
                result.push(format!("{} {}", fmtp_prefix_exact, fmtp_params));
                inserted = true;
            }
        }
        new_lines = result;
    }
    new_lines.join("\r\n")
}

#[derive(Deserialize)]
struct ConfigureOpusInput {
    sdp: String,
    #[serde(rename = "highFidelity")]
    high_fidelity: bool,
}

/// JSON-in/JSON-out wrapper for configure_opus_audio_sdp.
/// Input: `{"sdp": "...", "highFidelity": true/false}`
/// Output: `{"sdp": "..."}`
pub fn configure_opus_audio_sdp_json(input: &str) -> String {
    let Some(input) = json_in::<Option<ConfigureOpusInput>>(input, None) else {
        return json_out(&serde_json::json!({"sdp": ""}), r#"{"sdp": ""}"#);
    };
    if input.sdp.len() > MAX_FFI_LEN {
        return json_out(&serde_json::json!({"sdp": ""}), r#"{"sdp": ""}"#);
    }
    let result = configure_opus_audio_sdp(&input.sdp, input.high_fidelity);
    json_out(&serde_json::json!({"sdp": result}), r#"{"sdp": ""}"#)
}
