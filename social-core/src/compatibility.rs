//! Interest-overlap scoring. Single Jaccard kernel shared by the dating,
//! events, and social compatibility scoring paths.

use std::collections::HashSet;

/// Computes the common interest items and the union size after trimming and
/// deduplicating both sides. `case_insensitive` normalizes comparisons (used
/// by dating scoring); the common items are returned in original case.
pub fn interest_overlap_details<'a>(
    a: &'a [String],
    b: &'a [String],
    case_insensitive: bool,
) -> (Vec<&'a str>, usize, usize, usize) {
    use std::borrow::Cow;
    let norm = |s: &'a str| -> &'a str { s.trim() };
    let key = |t: &'a str| -> Cow<'a, str> {
        if case_insensitive && t.chars().any(|c| c.is_uppercase()) {
            Cow::Owned(t.to_ascii_lowercase())
        } else {
            Cow::Borrowed(t)
        }
    };
    // Build deduplicated key sets.
    let mut seen_a: HashSet<Cow<'a, str>> = HashSet::with_capacity(a.len());
    let mut seen_b: HashSet<Cow<'a, str>> = HashSet::with_capacity(b.len());
    for s in a {
        let t = norm(s);
        if !t.is_empty() {
            seen_a.insert(key(t));
        }
    }
    for s in b {
        let t = norm(s);
        if !t.is_empty() {
            seen_b.insert(key(t));
        }
    }
    let len_a = seen_a.len();
    let len_b = seen_b.len();
    // Collect common items (deduplicated by removing from seen_b when found).
    let mut common: Vec<&'a str> = Vec::with_capacity(len_a.min(len_b));
    for s in a {
        if seen_b.is_empty() {
            break;
        }
        let t = norm(s);
        if !t.is_empty() && seen_b.remove(&key(t)) {
            common.push(t);
        }
    }
    // |A ∪ B| = |A| + |B| − |A ∩ B|; |A ∩ B| = common.len() (already deduplicated).
    let union = len_a + len_b - common.len();
    (common, union, len_a, len_b)
}

pub fn interest_overlap<'a>(
    a: &'a [String],
    b: &'a [String],
    case_insensitive: bool,
) -> (Vec<&'a str>, usize) {
    let (common, union, _, _) = interest_overlap_details(a, b, case_insensitive);
    (common, union)
}

/// Compatibility as common-over-max on deduplicated interests.
pub fn basic_compatibility(a_interest: &[String], b_interest: &[String]) -> f64 {
    if a_interest.is_empty() || b_interest.is_empty() {
        return 0.5;
    }
    if a_interest == b_interest {
        return 1.0;
    }
    let (common, _, len_a, len_b) = interest_overlap_details(a_interest, b_interest, false);
    let max = len_a.max(len_b);
    if max == 0 {
        return 0.5;
    }
    common.len() as f64 / max as f64
}

/// Jaccard similarity (common / union) on deduplicated, optionally
/// case-insensitive interests.
pub fn jaccard_similarity(
    a_interest: &[String],
    b_interest: &[String],
    case_insensitive: bool,
) -> f64 {
    if a_interest.is_empty() || b_interest.is_empty() {
        return 0.5;
    }
    if a_interest == b_interest {
        return 1.0;
    }
    let (common, union) = interest_overlap(a_interest, b_interest, case_insensitive);
    if union == 0 {
        return 0.0;
    }
    common.len() as f64 / union as f64
}
