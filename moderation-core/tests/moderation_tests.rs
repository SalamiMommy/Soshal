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
