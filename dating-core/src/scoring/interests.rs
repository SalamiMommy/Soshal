//! Interest vector & language overlap scoring.

/// Jaccard overlap of two interest lists (case-insensitive, trimmed, deduped).
/// Missing/empty inputs score the neutral 0.5.
#[doc(hidden)]
pub fn score_interests(
    self_interests: Option<&[String]>,
    other_interests: Option<&[String]>,
) -> f64 {
    match (self_interests, other_interests) {
        (Some(s), Some(o)) => {
            if s.is_empty() || o.is_empty() {
                return 0.5;
            }
            soshal_social_core::compatibility::jaccard_similarity(s, o, true)
        }
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_language(self_language: Option<&[String]>, other_language: Option<&[String]>) -> f64 {
    score_interests(self_language, other_language)
}
