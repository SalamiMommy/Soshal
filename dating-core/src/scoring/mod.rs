//! Compatibility scoring algorithm for dating profiles.

pub mod interests;
pub mod lifestyle;
pub mod metrics;

use crate::DatingProfile;
use interests::*;
use lifestyle::*;
use metrics::*;

pub(crate) fn compute_compatibility_score(self_p: &DatingProfile, other_p: &DatingProfile) -> u32 {
    let weights = self_p.preference_weights.as_ref();
    let dealbreakers = self_p.dealbreakers.as_deref().unwrap_or(&[]);
    let mut total_weighted_score = 0.0f64;
    let mut total_weight = 0.0f64;

    macro_rules! check_field {
        ($field_name:expr, $score_val:expr, $weight_val:expr) => {
            let score = $score_val;
            let weight = $weight_val.unwrap_or(1.0);
            if score == 0.0 && dealbreakers.iter().any(|d| d == $field_name) {
                return 0;
            }
            total_weighted_score += score * weight;
            total_weight += weight;
        };
    }
    check_field!(
        "age",
        score_age(self_p.age, other_p.age),
        weights.and_then(|w| w.age)
    );
    check_field!(
        "height",
        score_height(self_p.height, other_p.height),
        weights.and_then(|w| w.height)
    );
    check_field!(
        "bodyType",
        score_body_type(self_p.body_type.as_deref(), other_p.body_type.as_deref()),
        weights.and_then(|w| w.body_type)
    );
    check_field!(
        "interests",
        score_interests(self_p.interests.as_deref(), other_p.interests.as_deref()),
        weights.and_then(|w| w.interests)
    );
    check_field!(
        "smoking",
        score_smoking(self_p.smoking.as_deref(), other_p.smoking.as_deref()),
        weights.and_then(|w| w.smoking)
    );
    check_field!(
        "drinking",
        score_drinking(self_p.drinking.as_deref(), other_p.drinking.as_deref()),
        weights.and_then(|w| w.drinking)
    );
    check_field!(
        "politics",
        score_politics(self_p.politics.as_deref(), other_p.politics.as_deref()),
        weights.and_then(|w| w.politics)
    );
    check_field!(
        "ethnicity",
        score_ethnicity(self_p.ethnicity.as_deref(), other_p.ethnicity.as_deref()),
        weights.and_then(|w| w.ethnicity)
    );
    check_field!(
        "education",
        score_education(self_p.education.as_deref(), other_p.education.as_deref()),
        weights.and_then(|w| w.education)
    );
    check_field!(
        "language",
        score_language(self_p.language.as_deref(), other_p.language.as_deref()),
        weights.and_then(|w| w.language)
    );
    check_field!(
        "relationshipIntent",
        score_relationship_intent(
            self_p.relationship_intent.as_deref(),
            other_p.relationship_intent.as_deref()
        ),
        weights.and_then(|w| w.relationship_intent)
    );
    if total_weight == 0.0 {
        return 50;
    }
    let avg = total_weighted_score / total_weight;
    if !avg.is_finite() {
        return 50;
    }
    (avg * 100.0).round() as u32
}

/// Compatibility score between two profiles (mutual).
#[doc(hidden)]
pub fn compute_mutual_score(self_p: &DatingProfile, other_p: &DatingProfile) -> u32 {
    let score_self = compute_compatibility_score(self_p, other_p);
    let score_other = compute_compatibility_score(other_p, self_p);
    ((score_self + score_other) as f64 / 2.0).round() as u32
}
