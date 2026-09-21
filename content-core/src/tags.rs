//! Minimal Nostr tag helpers, free of relay/network dependencies; the
//! Flutter UI reuses them via the FFI bridge.

/// Finds the second element of the first tag whose name matches `key`.
pub fn find_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    for tag in tags {
        if tag.len() >= 2 && tag[0] == key {
            return Some(&tag[1]);
        }
    }
    None
}

/// Single-pass multi-key tag extractor. Scans `tags` once to resolve all `K` tag keys
/// in $O(N)$ time instead of $O(K \cdot N)$ sequential scans.
pub fn find_tag_values_map<'a, const K: usize>(
    tags: &'a [Vec<String>],
    keys: [&str; K],
) -> [Option<&'a str>; K] {
    let mut results = [None; K];
    let mut found_count = 0;
    for tag in tags {
        if tag.len() >= 2 {
            for (idx, key) in keys.iter().enumerate() {
                if results[idx].is_none() && &tag[0] == key {
                    results[idx] = Some(tag[1].as_str());
                    found_count += 1;
                    if found_count == K {
                        return results;
                    }
                    break;
                }
            }
        }
    }
    results
}

/// Normalizes an audience string to a known value.
pub fn parse_audience(value: &str) -> &'static str {
    match value {
        "public" => "public",
        "network" => "network",
        "friends_only" => "friends_only",
        "only_me" => "only_me",
        _ => "public",
    }
}
