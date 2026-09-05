//! Compatibility scoring algorithm for dating profiles.

pub mod interests;
pub mod lifestyle;
pub mod metrics;

use crate::ProfileScoringFields;
use interests::*;
use lifestyle::*;
use metrics::*;

pub(crate) fn compute_compatibility_score<P: ProfileScoringFields>(self_p: &P, other_p: &P) -> u32 {
    compute_compatibility_score_inner(self_p, other_p, None)
}

/// Compatibility score plus a distance dimension. `distance_km` of `None`
/// skips the distance dimension entirely (no neutral drag on geo-less pairs).
#[doc(hidden)]
pub(crate) fn compute_compatibility_score_d<P: ProfileScoringFields>(
    self_p: &P,
    other_p: &P,
    distance_km: Option<f64>,
) -> u32 {
    compute_compatibility_score_inner(self_p, other_p, distance_km)
}

fn compute_compatibility_score_inner<P: ProfileScoringFields>(
    self_p: &P,
    other_p: &P,
    distance_km: Option<f64>,
) -> u32 {
    let weights = self_p.preference_weights();
    let dealbreakers = self_p.dealbreakers().unwrap_or(&[]);
    let mut total_weighted_score = 0.0f64;
    let mut total_weight = 0.0f64;

    macro_rules! check_field {
        ($field_name:expr, $score_val:expr, $weight_val:expr) => {
            let score = $score_val;
            let weight = $weight_val.unwrap_or(1.0);
            // Dealbreaker = hard requirement: any mismatch below perfect
            // (score < 1.0) on a declared dealbreaker field rejects outright.
            if score < 1.0 && dealbreakers.iter().any(|d| d == $field_name) {
                return 0;
            }
            total_weighted_score += score * weight;
            total_weight += weight;
        };
    }
    check_field!(
        "age",
        score_age(self_p.age(), other_p.age()),
        weights.and_then(|w| w.age)
    );
    check_field!(
        "height",
        score_height(self_p.height(), other_p.height()),
        weights.and_then(|w| w.height)
    );
    check_field!(
        "bodyType",
        score_body_type(self_p.body_type(), other_p.body_type()),
        weights.and_then(|w| w.body_type)
    );
    check_field!(
        "interests",
        score_interests(self_p.interests(), other_p.interests()),
        weights.and_then(|w| w.interests).or(Some(2.0))
    );
    check_field!(
        "smoking",
        score_smoking(self_p.smoking(), other_p.smoking()),
        weights.and_then(|w| w.smoking)
    );
    check_field!(
        "drinking",
        score_drinking(self_p.drinking(), other_p.drinking()),
        weights.and_then(|w| w.drinking)
    );
    check_field!(
        "politics",
        score_politics(self_p.politics(), other_p.politics()),
        weights.and_then(|w| w.politics)
    );
    check_field!(
        "ethnicity",
        score_ethnicity(self_p.ethnicity(), other_p.ethnicity()),
        weights.and_then(|w| w.ethnicity)
    );
    check_field!(
        "education",
        score_education(self_p.education(), other_p.education()),
        weights.and_then(|w| w.education)
    );
    check_field!(
        "language",
        score_language(self_p.language(), other_p.language()),
        weights.and_then(|w| w.language)
    );
    check_field!(
        "relationshipIntent",
        score_relationship_intent(self_p.relationship_intent(), other_p.relationship_intent()),
        weights.and_then(|w| w.relationship_intent)
    );
    if let Some(km) = distance_km {
        check_field!(
            "distance",
            score_distance(km),
            weights.and_then(|w| w.distance)
        );
    }
    if total_weight == 0.0 {
        return 50;
    }
    let avg = total_weighted_score / total_weight;
    if !avg.is_finite() {
        return 50;
    }
    (avg * 100.0).round() as u32
}

/// Mutual compatibility score, distance-aware.
#[doc(hidden)]
pub fn compute_mutual_score_with_distance<P: ProfileScoringFields>(
    self_p: &P,
    other_p: &P,
    distance_km: Option<f64>,
) -> u32 {
    let score_self = compute_compatibility_score_d(self_p, other_p, distance_km);
    let score_other = compute_compatibility_score_d(other_p, self_p, distance_km);
    ((score_self + score_other) as f64 / 2.0).round() as u32
}

/// Compatibility score between two profiles (mutual).
#[doc(hidden)]
pub fn compute_mutual_score<P: ProfileScoringFields>(self_p: &P, other_p: &P) -> u32 {
    let score_self = compute_compatibility_score(self_p, other_p);
    let score_other = compute_compatibility_score(other_p, self_p);
    ((score_self + score_other) as f64 / 2.0).round() as u32
}
