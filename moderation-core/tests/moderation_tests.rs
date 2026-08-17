//! Integration tests for soshal-moderation-core: hate-speech flagging and
//! HTML/glitter sanitization.

use soshal_moderation_core::check::check_text;
use soshal_moderation_core::glitter::sanitize_glitter_content;

fn passed(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json).unwrap()["passed"]
        .as_bool()
        .unwrap()
}

fn category(json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(json).unwrap()["category"]
        .as_str()
        .map(|s| s.to_string())
}

#[test]
fn check_text_passes_clean() {
    assert!(passed(&check_text("hello world")));
    assert!(passed(&check_text("")));
    assert!(passed(&check_text("   ")));
    assert!(passed(&check_text("здравствуйте мир")));
}

#[test]
fn check_text_flags_racist_slurs() {
    let flagged = vec![
        "you are a nigger",
        "that coon is here",
        "he's such a spic",
        "dirty chink",
        "gook go home",
        "wetback crossing",
        "kike banker",
        "white power rally",
        "monkey black man",
        "black monkey",
    ];
    for text in flagged {
        assert!(!passed(&check_text(text)), "should flag: {text}");
        assert_eq!(category(&check_text(text)).as_deref(), Some("racist"));
    }
}

#[test]
fn check_text_leetspeak_obfuscation() {
    assert!(!passed(&check_text("n1gg3r")));
    assert!(!passed(&check_text("NIGGER")));
}

#[test]
fn check_text_does_not_flag_innocuous_monkey() {
    assert!(passed(&check_text("monkeys in the zoo")));
    assert!(passed(&check_text("black and white photography")));
}

#[test]
fn check_text_fails_closed_on_oversize() {
    let big = "x".repeat(256 * 1024 + 1);
    assert!(!passed(&check_text(&big)));
    assert_eq!(category(&check_text(&big)).as_deref(), Some("oversize"));
}

#[test]
fn glitter_strips_scripts_and_handlers() {
    assert_eq!(sanitize_glitter_content(""), "");
    let clean = sanitize_glitter_content("<script>alert(1)</script>hello");
    assert!(!clean.contains("<script"));
    assert!(!clean.contains("alert(1)"));
    let clean2 = sanitize_glitter_content("<img src=x onerror=alert(1)>");
    assert!(!clean2.contains("onerror"));
    assert!(!clean2.is_empty());
}

#[test]
fn glitter_strips_encoded_entities() {
    let clean = sanitize_glitter_content("&lt;script&gt;alert(1)&lt;/script&gt;");
    assert!(!clean.contains("<script"));
    let clean2 = sanitize_glitter_content("<style>body{}</style>text");
    assert!(!clean2.contains("<style"));
    assert!(clean2.contains("text"));
    let clean3 = sanitize_glitter_content("<a href=\"javascript:alert(1)\">x</a>");
    assert!(!clean3.contains("javascript:"));
}

#[test]
fn glitter_keeps_plain_glitter() {
    let s = "✨ hello world ✨ https://example.com/x";
    assert_eq!(sanitize_glitter_content(s), s);
}

#[test]
fn check_text_flags_homophobic_transphobic_hate_spam() {
    assert_eq!(
        category(&check_text("you faggot")).as_deref(),
        Some("homophobic")
    );
    assert_eq!(
        category(&check_text("tranny go away")).as_deref(),
        Some("transphobic")
    );
    assert_eq!(
        category(&check_text("hitler was right")).as_deref(),
        Some("hate")
    );
    assert_eq!(
        category(&check_text("buy now free money earn crypto")).as_deref(),
        Some("spam")
    );
}

#[test]
fn glitter_strips_control_characters() {
    let input = "hello\u{200b}\u{200c}world\u{0000}";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "helloworld");
}

#[test]
fn glitter_strips_bidi_and_c1_controls() {
    let input = "a\u{202e}b\u{202d}c\u{0085}d\u{009f}e";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "a\u{202e}b\u{202d}cde");
    assert!(!cleaned.contains('\u{0085}'));
    assert!(!cleaned.contains('\u{009f}'));
}

#[test]
fn glitter_keeps_whitespace_controls() {
    let input = "a\tb\nc\rd";
    assert_eq!(sanitize_glitter_content(input), input);
}

#[test]
fn glitter_strips_obfuscated_schemes() {
    let cases = [
        "<a href='data:text/html,<script>alert(1)</script>'>y</a>",
        "<form action=javascript:alert(1)>z</form>",
        "<a href=javascript:alert(1)>w</a>",
        "<div style=\"background:url(javascript:alert(1))\">x</div>",
        "<div style=\"background:url(data:text/html,x)\">y</div>",
    ];
    for input in cases {
        let cleaned = sanitize_glitter_content(input);
        assert!(
            !cleaned.to_ascii_lowercase().contains("javascript")
                && !cleaned.to_ascii_lowercase().contains("data:"),
            "input {input:?} leaked: {cleaned:?}"
        );
    }
}

#[test]
fn glitter_drops_entities_in_suspicious_attrs() {
    let cleaned = sanitize_glitter_content("<img src=\"x&quot; onerror=1\">");
    assert!(!cleaned.contains("onerror"), "{cleaned}");
}

#[test]
fn glitter_keeps_safe_attrs_and_css() {
    let input = "<a href=\"https://example.com\" class=\"btn\">ok</a><img src=\"x.jpg\" width=10>";
    assert_eq!(sanitize_glitter_content(input), input);
}

#[test]
fn glitter_strips_dangerous_tags() {
    let cases = [
        "<iframe src=x></iframe>",
        "<object data=x></object>",
        "<embed src=x>",
        "<meta http-equiv=refresh>",
        "<link rel=stylesheet href=x>",
        "<base href=x>",
    ];
    for input in cases {
        let cleaned = sanitize_glitter_content(input);
        assert!(!cleaned.contains("iframe"), "{input}");
        assert!(!cleaned.contains("object"), "{input}");
        assert!(!cleaned.contains("embed"), "{input}");
        assert!(!cleaned.contains("meta"), "{input}");
        assert!(!cleaned.contains("link"), "{input}");
        assert!(!cleaned.contains("base"), "{input}");
    }
}

#[test]
fn glitter_css_uri_schemes_stripped() {
    let cases = [
        "<div style=\"background:url(javascript:alert(1))\">x</div>",
        "<div style=\"background:url(data:text/html,x)\">y</div>",
        "<div style=\"background:url(blob:xyz)\">z</div>",
    ];
    for input in cases {
        let cleaned = sanitize_glitter_content(input);
        assert!(!cleaned.contains("javascript"), "{input}: {cleaned}");
        assert!(!cleaned.contains("data:"), "{input}: {cleaned}");
        assert!(!cleaned.contains("blob:"), "{input}: {cleaned}");
    }
}

#[test]
fn glitter_handles_malformed_attrs() {
    let input = "<a href= onclick=alert(1)>x</a>";
    let cleaned = sanitize_glitter_content(input);
    assert!(!cleaned.contains("onclick"), "{cleaned}");
}

#[test]
fn glitter_keeps_plain_url_and_entities_in_text() {
    let input = "use &amp; more https://example.com/a?b=1&amp;c=2";
    let cleaned = sanitize_glitter_content(input);
    assert!(cleaned.contains("https://example.com/a?b=1"), "{cleaned}");
    assert!(!cleaned.contains("onerror"), "{cleaned}");
}

#[test]
fn check_text_adversarial_cases() {
    assert!(!passed(&check_text("you faggot")));
    assert_eq!(
        category(&check_text("you faggot")).as_deref(),
        Some("homophobic")
    );
    assert!(!passed(&check_text("FAGGOT")));
    assert!(!passed(&check_text("TrAnNy")));
    assert!(passed(&check_text("f\u{0430}ggot")));
    assert!(passed(&check_text("nigg\u{301}er")));
    assert!(passed(&check_text("faggotry")));
    assert!(passed(&check_text("unfaggot")));
    assert!(passed(&check_text("hello world")));
    assert_eq!(category(&check_text("hello world")).as_deref(), None);
    assert!(passed(&check_text("")));
    assert!(passed(&check_text("   ")));
    assert!(passed(&check_text("\t\n ")));
}
