use soshal_content_core::entities::decode_ascii_entities;

fn contains_entity_for(s: &str, c: char) -> bool {
    let lower = c.to_ascii_lowercase();
    let upper = c.to_ascii_uppercase();
    if !s.contains("&#") {
        return false;
    }
    let named: &[(&str, char)] = match c {
        '<' => &[("&lt;", '<')],
        '>' => &[("&gt;", '>')],
        '"' => &[("&quot;", '"')],
        '\'' => &[("&apos;", '\''), ("&#39;", '\'')],
        '/' => &[],
        _ => &[],
    };
    for (ent, _) in named {
        if s.contains(ent) {
            return true;
        }
    }
    let _ = (lower, upper);
    match c {
        '<' => s.contains("&#60;") || s.contains("&#x3c;") || s.contains("&#X3C;"),
        '>' => s.contains("&#62;") || s.contains("&#x3e;") || s.contains("&#X3E;"),
        '"' => s.contains("&#34;") || s.contains("&#x22;") || s.contains("&#X22;"),
        '\'' => s.contains("&#39;") || s.contains("&#x27;") || s.contains("&#X27;"),
        '/' => s.contains("&#47;") || s.contains("&#x2f;") || s.contains("&#X2F;"),
        _ => {
            let dec = format!("&#{};", c as u32);
            if s.contains(&dec) {
                return true;
            }
            let hex_lower = format!("&#x{:x};", c as u32);
            if s.contains(&hex_lower) {
                return true;
            }
            let hex_upper = format!("&#X{:X};", c as u32);
            s.contains(&hex_upper)
        }
    }
}

struct GlitterRegexes {
    script: regex::Regex,
    style: regex::Regex,
    danger_tags: regex::Regex,
    on_handler: regex::Regex,
    css_uri: regex::Regex,
    dangerous_uri: regex::Regex,
    entity_scheme: regex::Regex,
    attr_value: regex::Regex,
}

fn get_glitter_regexes() -> &'static GlitterRegexes {
    static RE: std::sync::OnceLock<GlitterRegexes> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        // catch_unwind: RegexBuilder::build can panic on hostile patterns;
        // degrade to a never-matching regex instead of crashing moderation.
        let build_size_limited = |pat: &str| -> regex::Regex {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::regex_util::build_regex(pat)
            }))
            .unwrap_or_else(|_| regex::Regex::new(r"^$").expect("fallback regex must compile"))
        };
        GlitterRegexes {
            script: build_size_limited(r"(?is)<script\b[^>]*>[\s\S]*?</script\s*>"),
            style: build_size_limited(r"(?is)<style\b[^>]*>[\s\S]*?</style\s*>"),
            danger_tags: build_size_limited(r#"(?is)</?(?:iframe|object|embed|form|base|meta|link)\b[^>]*>"#),
            on_handler: build_size_limited(r#"(?is)\bon[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#),
            css_uri: build_size_limited(r#"(?is)(?:url\s*\(\s*)(?:javascript|vbscript|data|blob|filesystem|view-source|jar|resource|chrome|chrome-extension|about|edge|opera|safariextz|android-app|intent|file)\s*:"#),
            dangerous_uri: build_size_limited(r#"(?is)\b(?:href|src|action|formaction|xlink:href|background|poster|cite|icon)\s*=\s*(?:"\s*(?:javascript|vbscript|data|blob|filesystem|view-source|jar|resource|chrome|chrome-extension|about|edge|opera|safariextz|android-app|intent|file)\s*:[^"]*"|'\s*(?:javascript|vbscript|data|blob|filesystem|view-source|jar|resource|chrome|chrome-extension|about|edge|opera|safariextz|android-app|intent|file)\s*:[^']*'|(?:javascript|vbscript|data|blob|filesystem|view-source|jar|resource|chrome|chrome-extension|about|edge|opera|safariextz|android-app|intent|file)\s*:[^\s>]+)"#),
            entity_scheme: build_size_limited(r"(?i)(?:java|jar|resource|chrome|about|edge|opera|intent|android-app|file)&#x?[0-9a-f]+;|&#x?[0-9a-f]+;?(?:script|jar|resource|chrome|about|edge|opera|intent|android-app|file)\s*:"),
            attr_value: build_size_limited(r#"(?is)\b([a-z][a-z0-9:_-]*)\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))"#),
        }
    })
}

/// Removes C0/C1 control characters and zero-width joiners/bidi marks that can
/// smuggle past regex-based sanitization (e.g. split scheme tokens or hide
/// dangerous content from the attribute value walker). Whitespace
/// (tab/newline/CR) is preserved so normal formatting survives.
fn strip_control_chars(s: &str) -> String {
    s.chars()
        .filter(|c| {
            let u = *c as u32;
            if u < 0x20 {
                matches!(c, '\t' | '\n' | '\r')
            } else {
                !(0x7f..=0x9f).contains(&u)
                    && !matches!(
                        c,
                        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{00ad}'
                    )
            }
        })
        .collect()
}

pub fn sanitize_glitter_content(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let bounded = if content.len() > crate::check::MAX_MODERATION_INPUT_LEN {
        &content[..content.floor_char_boundary(crate::check::MAX_MODERATION_INPUT_LEN)]
    } else {
        content
    };
    let s = strip_control_chars(bounded);
    let s = decode_ascii_entities(&s);
    let re = get_glitter_regexes();

    let s = re.script.replace_all(&s, "");
    let s = re.style.replace_all(&s, "");
    let s = re.danger_tags.replace_all(&s, "");
    let s = re.on_handler.replace_all(&s, "");
    let s = re.css_uri.replace_all(&s, "url(");
    let s = re.dangerous_uri.replace_all(&s, "");
    let s = re.entity_scheme.replace_all(&s, "");

    let mut result = String::with_capacity(s.len());
    let mut last = 0;
    for mat in re.attr_value.find_iter(&s) {
        result.push_str(&s[last..mat.start()]);
        let value = mat
            .as_str()
            .split_once('=')
            .map(|x| x.1)
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        let suspicious_attr = matches!(
            mat.as_str()
                .split('=')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "href"
                | "src"
                | "action"
                | "formaction"
                | "xlink:href"
                | "background"
                | "poster"
                | "cite"
                | "icon"
        );
        if suspicious_attr
            && (contains_entity_for(value, '<')
                || contains_entity_for(value, '>')
                || contains_entity_for(value, '"')
                || contains_entity_for(value, '\'')
                || contains_entity_for(value, '/'))
        {
        } else {
            result.push_str(mat.as_str());
        }
        last = mat.end();
    }
    result.push_str(&s[last..]);

    result.replace('\0', "")
}
