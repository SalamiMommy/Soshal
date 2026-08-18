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

#[test]
fn glitter_entity_schemes_and_encoded_handlers() {
    // Entity-encoded scheme: decode_ascii_entities expands &#x73; → s, so
    // the plain-scheme regex must still fire.
    let cleaned = sanitize_glitter_content("<a href=\"java&#x73;cript:alert(1)\">y</a>");
    assert!(
        !cleaned.to_ascii_lowercase().contains("javascript"),
        "{cleaned}"
    );
    let cleaned = sanitize_glitter_content("<a href=&#106;avascript:alert(1)>y</a>");
    assert!(
        !cleaned.to_ascii_lowercase().contains("javascript"),
        "{cleaned}"
    );
    // Case-insensitive handler stripping, quoted and bare.
    let cleaned = sanitize_glitter_content("<img src=x OnError=alert(1)>");
    assert!(!cleaned.contains("onerror"), "{cleaned}");
    let cleaned = sanitize_glitter_content("<img src=x onerror='alert(1)'>");
    assert!(!cleaned.contains("onerror"), "{cleaned}");
    let cleaned = sanitize_glitter_content("<img src=x onerror=\"alert(1)\">");
    assert!(!cleaned.contains("onerror"), "{cleaned}");
}

#[test]
fn glitter_attr_value_entity_handling() {
    // Numeric entities are decoded to plain `<`/`>` inside attribute
    // values; with no scheme they are inert text and the attr survives.
    for form in ["&#60;", "&#x3c;", "&#X3C;"] {
        let input = format!("<a href=\"{form}x\">z</a>");
        let cleaned = sanitize_glitter_content(&input);
        assert!(cleaned.contains("href"), "form {form}: {cleaned}");
    }
    // A value carrying BOTH a named entity and a live numeric entity
    // (unparseable by the decoder, e.g. non-hex digits) trips the
    // entity guard and the whole suspicious attr is dropped.
    let input = "<a href=\"&lt;&#xZZ;script\">x</a>";
    let cleaned = sanitize_glitter_content(input);
    assert!(!cleaned.contains("href"), "{cleaned}");
    let input = "<img src=\"&quot;&#xZZ;x\">";
    let cleaned = sanitize_glitter_content(input);
    assert!(!cleaned.contains("src"), "{cleaned}");
    // Attribute-breaking quote entities let a handler spill out; the
    // handler regex still removes it.
    let input = "<a href=\"x&quot; onmouseover=alert(1)\">z</a>";
    let cleaned = sanitize_glitter_content(input);
    assert!(!cleaned.contains("onmouseover"), "{cleaned}");
    // Non-suspicious attributes keep entity text.
    let input = "<div data-note=\"&lt;note&gt;\">x</div>";
    let cleaned = sanitize_glitter_content(input);
    assert!(cleaned.contains("data-note"), "{cleaned}");
    // Decoded slashes/quotes in values are inert once the attr survives.
    let input = "<img src=\"a&#47;b\">";
    let cleaned = sanitize_glitter_content(input);
    assert!(cleaned.contains("src"), "{cleaned}");
}

#[test]
fn glitter_strips_remaining_control_and_format_chars() {
    let input = "a\u{0080}b\u{009f}c\u{00ad}d\u{feff}e\u{2060}f";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "abcdef", "{cleaned:?}");
    // C1 block fully removed, bidi marks kept as-is (already covered).
    let input = "\u{009c}\u{009d}";
    assert_eq!(sanitize_glitter_content(input), "");
    // NULL bytes and other C0 (non-whitespace) removed.
    assert_eq!(sanitize_glitter_content("a\u{0000}b\u{0001}c"), "abc");
    // Whitespace controls survive.
    assert_eq!(sanitize_glitter_content("x\ty\r\nz"), "x\ty\r\nz");
}

#[test]
fn glitter_drops_css_uri_and_scheme_in_all_attr_names() {
    for attr in [
        "action",
        "formaction",
        "xlink:href",
        "background",
        "poster",
        "cite",
        "icon",
    ] {
        let input = format!("<a {attr}=javascript:alert(1)>x</a>");
        let cleaned = sanitize_glitter_content(&input);
        assert!(
            !cleaned.to_ascii_lowercase().contains("javascript"),
            "attr {attr}: {cleaned}"
        );
    }
    // css_uri: url(...) wrapper collapsed, scheme removed.
    let cleaned = sanitize_glitter_content("<div style=\"background:url(blob:xyz)\">y</div>");
    assert!(!cleaned.contains("blob:"), "{cleaned}");
    let cleaned = sanitize_glitter_content("<div style=\"background:url(filesystem:x)\">y</div>");
    assert!(!cleaned.contains("filesystem:"), "{cleaned}");
}

#[test]
fn glitter_keeps_undecodable_entities_in_suspicious_attr_values() {
    // &#39; decodes to a plain `'` (named-table decimal arm); &#xZZ; is not
    // valid hex so the entity decoder strips the non-hex digits and the
    // stub `&#x;ZZ;` survives. With no entity strings left, the
    // contains_entity_for guard is inert and the href survives intact.
    let input = "<a href=\"x&#xZZ;&#39;y\">text</a>";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "<a href=\"x&#x;ZZ;'y\">text</a>", "{cleaned}");
    // &#47; decodes to `/`; the named table has no `/` entry (glitter.rs:14),
    // so the entity guard never trips and the src survives intact.
    let input = "<img src=\"x&#xZZ;&#47;y\">";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "<img src=\"x&#x;ZZ;/y\">", "{cleaned}");
}
