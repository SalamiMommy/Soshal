//! Fuzzy string similarity and typo-tolerant search ranking.
//!
//! Employs `strsim` for Jaro-Winkler and normalized Damerau-Levenshtein metrics,
//! enabling typo-tolerant search across Nostr usernames, handles, and hashtags.

use rayon::prelude::*;

/// Blended similarity score between `query` and `target` in the range `[0.0, 1.0]`.
///
/// Combines Jaro-Winkler (which favors common prefixes, essential for username and
/// tag completions) with normalized Damerau-Levenshtein (edit/transposition distance).
/// Includes a prefix bonus for exact prefix matches.
pub fn fuzzy_match(query: &str, target: &str) -> f64 {
    let q = query.trim();
    let t = target.trim();

    if q.is_empty() && t.is_empty() {
        return 1.0;
    }
    if q.is_empty() || t.is_empty() {
        return 0.0;
    }

    let q_lower = q.to_lowercase();
    let t_lower = t.to_lowercase();

    if q_lower == t_lower {
        return 1.0;
    }

    let jaro = strsim::jaro_winkler(&q_lower, &t_lower);
    let damerau = strsim::normalized_damerau_levenshtein(&q_lower, &t_lower);

    let mut score = (0.6 * jaro) + (0.4 * damerau);

    // Prefix boost for autocomplete behavior
    if t_lower.starts_with(&q_lower) {
        score = (score + 0.15).min(1.0);
    }

    score.clamp(0.0, 1.0)
}

/// Ranks a slice of candidate strings against `query` returning up to `top_k` matches
/// with similarity score >= `min_score`, sorted by score descending.
pub fn fuzzy_rank_candidates<'a>(
    query: &str,
    candidates: &[&'a str],
    min_score: f64,
    top_k: usize,
) -> Vec<(&'a str, f64)> {
    if query.trim().is_empty() || candidates.is_empty() || top_k == 0 {
        return Vec::new();
    }

    let score_candidate = |&candidate: &&'a str| {
        let score = fuzzy_match(query, candidate);
        (candidate, score)
    };

    let mut scored: Vec<(&'a str, f64)> = if candidates.len() >= 64 {
        candidates
            .par_iter()
            .map(score_candidate)
            .filter(|&(_, s)| s >= min_score)
            .collect()
    } else {
        candidates
            .iter()
            .map(score_candidate)
            .filter(|&(_, s)| s >= min_score)
            .collect()
    };

    if top_k < scored.len() {
        scored.select_nth_unstable_by(top_k, |a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    scored
}

/// Suggests the closest dictionary match ("Did you mean...?") if similarity >= `min_threshold`.
pub fn suggest_did_you_mean<'a>(
    query: &str,
    dictionary: &[&'a str],
    min_threshold: f64,
) -> Option<&'a str> {
    let matches = fuzzy_rank_candidates(query, dictionary, min_threshold, 1);
    matches.first().map(|&(word, _)| word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact_and_empty() {
        assert_eq!(fuzzy_match("nostr", "nostr"), 1.0);
        assert_eq!(fuzzy_match("Nostr", "nostr"), 1.0);
        assert_eq!(fuzzy_match("", ""), 1.0);
        assert_eq!(fuzzy_match("nostr", ""), 0.0);
        assert_eq!(fuzzy_match("", "nostr"), 0.0);
    }

    #[test]
    fn test_fuzzy_match_transpositions_and_typos() {
        // Transposition in "nostr" vs "nosrt"
        let score_transposed = fuzzy_match("nosrt", "nostr");
        assert!(score_transposed > 0.85);

        // Missing letter "salamimomy" vs "salamimommy"
        let score_typo = fuzzy_match("salamimomy", "salamimommy");
        assert!(score_typo > 0.88);

        // Completely unrelated string
        let score_unrelated = fuzzy_match("zebra", "nostr");
        assert!(score_unrelated < 0.45);
    }

    #[test]
    fn test_fuzzy_match_prefix_boost() {
        let prefix_score = fuzzy_match("sala", "salamimommy");
        let non_prefix_score = fuzzy_match("momm", "salamimommy");
        assert!(prefix_score > non_prefix_score);
    }

    #[test]
    fn test_fuzzy_rank_candidates() {
        let candidates = [
            "salamimommy",
            "alice",
            "bob",
            "salami_sandwich",
            "charlie",
            "sal",
        ];
        let ranked = fuzzy_rank_candidates("salami", &candidates, 0.5, 3);
        assert!(!ranked.is_empty());
        assert_eq!(ranked[0].0, "salamimommy");
        assert_eq!(ranked[1].0, "salami_sandwich");
        assert!(ranked.len() <= 3);
    }

    #[test]
    fn test_suggest_did_you_mean() {
        let dict = ["nostr", "bitcoin", "lightning", "relay", "profile"];
        assert_eq!(suggest_did_you_mean("nostrr", &dict, 0.75), Some("nostr"));
        assert_eq!(
            suggest_did_you_mean("bitcion", &dict, 0.75),
            Some("bitcoin")
        );
        assert_eq!(
            suggest_did_you_mean("completely_unrelated", &dict, 0.75),
            None
        );
    }
}
