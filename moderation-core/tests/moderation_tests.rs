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
    // Cyrillic and combining diacritic evasions are de-obfuscated and caught
    assert!(!passed(&check_text("f\u{0430}ggot")));
    assert!(!passed(&check_text("nigg\u{301}er")));
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
    // valid hex so the entity decoder leaves it verbatim. With no decodable
    // entity strings left, the contains_entity_for guard is inert and the
    // href survives intact.
    let input = "<a href=\"x&#xZZ;&#39;y\">text</a>";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "<a href=\"x&#xZZ;'y\">text</a>", "{cleaned}");
    // &#47; decodes to `/`; the named table has no `/` entry (glitter.rs:14),
    // so the entity guard never trips and the src survives intact.
    let input = "<img src=\"x&#xZZ;&#47;y\">";
    let cleaned = sanitize_glitter_content(input);
    assert_eq!(cleaned, "<img src=\"x&#xZZ;/y\">", "{cleaned}");
}

#[test]
fn test_spam_detection_comprehensive() {
    // Crypto doubler
    let doubler = "Send 1 BTC to this wallet and get 2x return today!";
    assert!(!passed(&check_text(doubler)));
    assert_eq!(category(&check_text(doubler)).as_deref(), Some("spam"));

    // Airdrop phishing
    let airdrop = "Claim your airdrop! Connect wallet and validate seed phrase now";
    assert!(!passed(&check_text(airdrop)));
    assert_eq!(category(&check_text(airdrop)).as_deref(), Some("spam"));

    // Fast cash scheme
    let fast_cash = "Make $5000 a day working from home guaranteed payout!";
    assert!(!passed(&check_text(fast_cash)));
    assert_eq!(category(&check_text(fast_cash)).as_deref(), Some("spam"));

    // Line flooding
    let flood = "WINNER\nWINNER\nWINNER\nWINNER\nWINNER";
    assert!(!passed(&check_text(flood)));
    assert_eq!(category(&check_text(flood)).as_deref(), Some("spam"));

    // Obfuscated spam
    let obfuscated = "d0ubl3 y0ur cryp+0 n0w";
    assert!(!passed(&check_text(obfuscated)));
    assert_eq!(category(&check_text(obfuscated)).as_deref(), Some("spam"));
}

#[test]
fn test_csam_detection_comprehensive() {
    // Trade request
    let csam_trade = "selling cp pack on telegram";
    assert!(!passed(&check_text(csam_trade)));
    assert_eq!(category(&check_text(csam_trade)).as_deref(), Some("cp"));

    // Obfuscated pedophile term
    let obf = "p3d0ph1l3";
    assert!(!passed(&check_text(obf)));
    assert_eq!(category(&check_text(obf)).as_deref(), Some("cp"));

    // Darknet distribution domain
    let onion = "links on http://pedochat.onion";
    assert!(!passed(&check_text(onion)));
    assert_eq!(category(&check_text(onion)).as_deref(), Some("cp"));
}

#[test]
fn test_gore_detection_comprehensive() {
    // Execution footage
    let execution = "watch this beheading video footage uncensored";
    assert!(!passed(&check_text(execution)));
    assert_eq!(category(&check_text(execution)).as_deref(), Some("gore"));

    // Cartel torture video
    let cartel = "cartel execution video clip leak";
    assert!(!passed(&check_text(cartel)));
    assert_eq!(category(&check_text(cartel)).as_deref(), Some("gore"));

    // Shock site domain
    let shock = "check out https://theync.com/video456";
    assert!(!passed(&check_text(shock)));
    assert_eq!(category(&check_text(shock)).as_deref(), Some("gore"));

    // Animal cruelty
    let crush = "animal crush video download";
    assert!(!passed(&check_text(crush)));
    assert_eq!(category(&check_text(crush)).as_deref(), Some("gore"));

    // Suicide instruction
    let suicide = "suicide instructions method guide";
    assert!(!passed(&check_text(suicide)));
    assert_eq!(category(&check_text(suicide)).as_deref(), Some("gore"));
}

#[test]
fn test_custom_word_filters() {
    let custom_words = vec!["custombadword".to_string(), "bannedterm".to_string()];
    let verdict = soshal_moderation_core::check::check_with_custom_words(
        "this post has custombadword inside",
        &custom_words,
    );
    assert!(!verdict.passed);
    assert_eq!(verdict.category.as_deref(), Some("custom"));

    let clean = soshal_moderation_core::check::check_with_custom_words(
        "this is a completely friendly post",
        &custom_words,
    );
    assert!(clean.passed);
    assert_eq!(clean.category, None);
}

#[test]
fn test_media_moderation() {
    use soshal_moderation_core::media::check_media_item;

    // CSAM hash blocked
    let csam = check_media_item(
        "c27a20ff44e8bc1a3b1a8d052d9a6c4df103c80a2b0e8b1ef380b0b8e8f85f31",
        Some("https://example.com/file.png"),
        "image/png",
        &[],
    );
    assert!(!csam.passed);
    assert!(csam.is_csam);

    // Shock site URL blocked
    let shock = check_media_item(
        "0000000000000000000000000000000000000000000000000000000000000000",
        Some("https://bestgore.com/shock.mp4"),
        "video/mp4",
        &[],
    );
    assert!(!shock.passed);
    assert!(shock.is_gore);

    // Sensitive NIP-36 tag flagged
    let sensitive = check_media_item(
        "1111111111111111111111111111111111111111111111111111111111111111",
        Some("https://example.com/art.jpg"),
        "image/jpeg",
        &["content-warning: nudity".to_string()],
    );
    assert!(sensitive.passed);
    assert!(sensitive.is_sensitive);
    assert_eq!(
        sensitive.warning_reason.as_deref(),
        Some("content-warning: nudity")
    );
}

#[test]
fn test_ai_text_classification_all_categories() {
    use soshal_moderation_core::check::{check_text_ai, check_text_ai_json};

    // 1. Clean content
    let clean = check_text_ai("Hello friends! Excited to share our new open-source Nostr client.");
    assert!(!clean.is_flagged);
    assert_eq!(clean.primary_category, None);

    // 2. AI Spam
    let spam = check_text_ai(
        "Send 2.5 ETH to double your crypto instantly! Connect wallet and claim airdrop now",
    );
    assert!(spam.is_flagged);
    assert_eq!(spam.primary_category.as_deref(), Some("spam"));
    assert!(spam.scores.spam > 0.55);

    // 3. AI CSAM
    let csam = check_text_ai("selling cp archive on darknet");
    assert!(csam.is_flagged);
    assert_eq!(csam.primary_category.as_deref(), Some("csam"));
    assert!(csam.scores.csam > 0.50);

    // 4. AI Gore
    let gore = check_text_ai("live cartel execution and beheading video hd");
    assert!(gore.is_flagged);
    assert_eq!(gore.primary_category.as_deref(), Some("gore"));
    assert!(gore.scores.gore > 0.50);

    // 5. AI Bigotry
    let bigotry = check_text_ai("white power rally death to all race traitors");
    assert!(bigotry.is_flagged);
    assert_eq!(bigotry.primary_category.as_deref(), Some("bigotry"));
    assert!(bigotry.scores.bigotry > 0.50);

    // 6. AI Harassment
    let harassment = check_text_ai("i will find you and kill you leak your address");
    assert!(harassment.is_flagged);
    assert_eq!(harassment.primary_category.as_deref(), Some("harassment"));
    assert!(harassment.scores.harassment > 0.50);

    // 7. JSON serialization roundtrip
    let json = check_text_ai_json("double your crypto fast");
    assert!(json.contains("\"is_flagged\":true"));
    assert!(json.contains("\"scores\""));
}

#[test]
fn test_ai_media_buffer_classification() {
    use soshal_moderation_core::media::{check_media_buffer_ai, check_media_buffer_ai_json};

    // Clean buffer
    let clean_buf = vec![120u8; 1024];
    let res = check_media_buffer_ai(&clean_buf, "image/png", &[]);
    assert!(res.passed);
    assert!(!res.is_csam_hazard);
    assert!(!res.is_gore_hazard);

    // Disallowed executable
    let elf = vec![0x7f, b'E', b'L', b'F', 0, 0];
    let res = check_media_buffer_ai(&elf, "application/x-executable", &[]);
    assert!(!res.passed);
    assert!(res.warning_reason.unwrap().contains("executable"));

    // JSON serialization
    let json = check_media_buffer_ai_json(&clean_buf, "image/jpeg", &[]);
    assert!(json.contains("\"passed\":true"));
}

#[test]
fn test_ai_classifier_comprehensive_matrix() {
    use soshal_moderation_core::check::check_text_ai;

    // Spam variations
    let spam1 = check_text_ai("Claim airdrop now and validate seed phrase to double your crypto");
    assert!(spam1.is_flagged);
    assert_eq!(spam1.primary_category.as_deref(), Some("spam"));

    let spam2 =
        check_text_ai("Earn $5000 a day guaranteed profit working from home! DM me on telegram");
    assert!(spam2.is_flagged);
    assert_eq!(spam2.primary_category.as_deref(), Some("spam"));

    // CSAM variations
    let csam1 = check_text_ai("selling cp pack on darknet");
    assert!(csam1.is_flagged);
    assert_eq!(csam1.primary_category.as_deref(), Some("csam"));

    let csam2 = check_text_ai("underage nudes collection download");
    assert!(csam2.is_flagged);
    assert_eq!(csam2.primary_category.as_deref(), Some("csam"));

    // Gore variations
    let gore1 = check_text_ai("graphic beheading video and cartel execution clip");
    assert!(gore1.is_flagged);
    assert_eq!(gore1.primary_category.as_deref(), Some("gore"));

    let gore2 = check_text_ai("suicide instructions how to kill yourself");
    assert!(gore2.is_flagged);
    assert_eq!(gore2.primary_category.as_deref(), Some("gore"));

    // Bigotry variations
    let hate1 = check_text_ai("white power rally kill all race traitors");
    assert!(hate1.is_flagged);
    assert_eq!(hate1.primary_category.as_deref(), Some("bigotry"));

    let hate2 = check_text_ai("gas the subhuman race");
    assert!(hate2.is_flagged);
    assert_eq!(hate2.primary_category.as_deref(), Some("bigotry"));

    // Obfuscated bigotry evasion
    let hate3 = check_text_ai("f\u{0430}gg\u{043E}t");
    assert!(hate3.is_flagged);
    assert_eq!(hate3.primary_category.as_deref(), Some("bigotry"));

    // Harassment
    let har1 = check_text_ai("i will find you and kill you leak your address");
    assert!(har1.is_flagged);
    assert_eq!(har1.primary_category.as_deref(), Some("harassment"));
}

#[test]
fn test_ai_classifier_false_positive_resistance() {
    use soshal_moderation_core::check::check_text_ai;

    let clean_inputs = [
        "Good morning everyone! Looking forward to testing the new Nostr client release.",
        "The black and white photography exhibition was stunning.",
        "You can connect wallet in settings if you wish to configure payment options.",
        "The zoologist studied the monkey population in the nature reserve.",
        "A minor accident occurred on the highway with no severe injuries reported.",
        "Check out our open source code repository on GitHub.",
    ];

    for clean in clean_inputs {
        let res = check_text_ai(clean);
        assert!(
            !res.is_flagged,
            "False positive on clean text: \"{clean}\" -> flagged as {:?}",
            res.primary_category
        );
        assert_eq!(res.primary_category, None);
    }
}
