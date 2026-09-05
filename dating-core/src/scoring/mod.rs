//! Compatibility scoring algorithm for dating profiles.

pub mod interests;
pub mod lifestyle;
pub mod metrics;

use crate::{PreferenceWeights, ProfileScoringFields};
use interests::*;
use lifestyle::*;
use metrics::*;

/// Clamp an untrusted preference weight to a sane [0.0, 1.0] range.
fn clamp_weight(w: f64) -> f64 {
    if w.is_finite() {
        w.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

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
    // Weights come from untrusted profile JSON: clamp each to [0.0, 1.0] so
    // negatives become zero and huge/non-finite values can't distort or
    // saturate the weighted average.
    let weights = self_p.preference_weights().map(|w| PreferenceWeights {
        age: w.age.map(clamp_weight),
        height: w.height.map(clamp_weight),
        body_type: w.body_type.map(clamp_weight),
        interests: w.interests.map(clamp_weight),
        smoking: w.smoking.map(clamp_weight),
        drinking: w.drinking.map(clamp_weight),
        politics: w.politics.map(clamp_weight),
        ethnicity: w.ethnicity.map(clamp_weight),
        education: w.education.map(clamp_weight),
        language: w.language.map(clamp_weight),
        relationship_intent: w.relationship_intent.map(clamp_weight),
        distance: w.distance.map(clamp_weight),
    });
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
        weights.as_ref().and_then(|w| w.age)
    );
    check_field!(
        "height",
        score_height(self_p.height(), other_p.height()),
        weights.as_ref().and_then(|w| w.height)
    );
    check_field!(
        "bodyType",
        score_body_type(self_p.body_type(), other_p.body_type()),
        weights.as_ref().and_then(|w| w.body_type)
    );
    check_field!(
        "interests",
        score_interests(self_p.interests(), other_p.interests()),
        weights.as_ref().and_then(|w| w.interests).or(Some(1.0))
    );
    check_field!(
        "smoking",
        score_smoking(self_p.smoking(), other_p.smoking()),
        weights.as_ref().and_then(|w| w.smoking)
    );
    check_field!(
        "drinking",
        score_drinking(self_p.drinking(), other_p.drinking()),
        weights.as_ref().and_then(|w| w.drinking)
    );
    check_field!(
        "politics",
        score_politics(self_p.politics(), other_p.politics()),
        weights.as_ref().and_then(|w| w.politics)
    );
    check_field!(
        "ethnicity",
        score_ethnicity(self_p.ethnicity(), other_p.ethnicity()),
        weights.as_ref().and_then(|w| w.ethnicity)
    );
    check_field!(
        "education",
        score_education(self_p.education(), other_p.education()),
        weights.as_ref().and_then(|w| w.education)
    );
    check_field!(
        "language",
        score_language(self_p.language(), other_p.language()),
        weights.as_ref().and_then(|w| w.language)
    );
    check_field!(
        "relationshipIntent",
        score_relationship_intent(self_p.relationship_intent(), other_p.relationship_intent()),
        weights.as_ref().and_then(|w| w.relationship_intent)
    );
    if let Some(km) = distance_km {
        check_field!(
            "distance",
            score_distance(km),
            weights.as_ref().and_then(|w| w.distance)
        );
    }
    if total_weight == 0.0 {
        return 50;
    }
    let avg = total_weighted_score / total_weight;
    if !avg.is_finite() {
        return 50;
    }
    let avg = avg.clamp(0.0, 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    struct TestProfile {
        age: Option<f64>,
        height: Option<f64>,
        weights: PreferenceWeights,
    }

    impl ProfileScoringFields for TestProfile {
        fn preference_weights(&self) -> Option<&PreferenceWeights> {
            Some(&self.weights)
        }
        fn age(&self) -> Option<f64> {
            self.age
        }
        fn height(&self) -> Option<f64> {
            self.height
        }
        fn body_type(&self) -> Option<&str> {
            None
        }
        fn interests(&self) -> Option<&[String]> {
            None
        }
        fn smoking(&self) -> Option<&str> {
            None
        }
        fn drinking(&self) -> Option<&str> {
            None
        }
        fn politics(&self) -> Option<&str> {
            None
        }
        fn ethnicity(&self) -> Option<&str> {
            None
        }
        fn education(&self) -> Option<&str> {
            None
        }
        fn language(&self) -> Option<&[String]> {
            None
        }
        fn relationship_intent(&self) -> Option<&str> {
            None
        }
        fn dealbreakers(&self) -> Option<&[String]> {
            None
        }
    }

    fn weights(age: Option<f64>, height: Option<f64>) -> PreferenceWeights {
        PreferenceWeights {
            age,
            height,
            body_type: None,
            interests: None,
            smoking: None,
            drinking: None,
            politics: None,
            ethnicity: None,
            education: None,
            language: None,
            relationship_intent: None,
            distance: None,
        }
    }

    #[test]
    fn canceling_weights_cannot_exceed_100() {
        let me = TestProfile {
            age: Some(30.0),
            height: Some(180.0),
            weights: weights(Some(1e18), Some(-(1e18 - 256.0))),
        };
        let other = TestProfile {
            age: Some(30.0),
            height: Some(240.0),
            weights: weights(None, None),
        };
        // Near-cancelling weights leave a tiny denominator (256.0); without
        // clamping the average would explode past u32::MAX.
        assert!(compute_compatibility_score(&me, &other) <= 100);
    }

    #[test]
    fn huge_single_weight_cannot_exceed_100() {
        let me = TestProfile {
            age: Some(30.0),
            height: None,
            weights: weights(Some(1e300), None),
        };
        let other = TestProfile {
            age: Some(30.0),
            height: None,
            weights: weights(None, None),
        };
        assert!(compute_compatibility_score(&me, &other) <= 100);
    }

    #[test]
    fn negative_weights_are_clamped_not_applied() {
        let base = TestProfile {
            age: Some(30.0),
            height: Some(180.0),
            weights: weights(Some(5.0), Some(-5.0)),
        };
        let zeroed = TestProfile {
            age: Some(30.0),
            height: Some(180.0),
            weights: weights(Some(5.0), Some(0.0)),
        };
        let other = TestProfile {
            age: Some(30.0),
            height: Some(180.0),
            weights: weights(None, None),
        };
        // A negative weight behaves exactly like an explicit zero weight: it
        // is clamped to 0 rather than dragged into the average.
        assert_eq!(
            compute_compatibility_score(&base, &other),
            compute_compatibility_score(&zeroed, &other)
        );
    }
}
