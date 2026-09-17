//! Compatibility scoring algorithm for dating profiles.

pub mod interests;
pub mod lifestyle;
pub mod metrics;

use crate::ProfileScoringFields;
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

/// Returns true if dealbreaker matches the target snake_case field without heap allocation,
/// supporting snake_case, camelCase, PascalCase, SCREAMING_SNAKE_CASE, kebab-case, etc.
fn matches_field(dealbreaker: &str, field_snake: &str) -> bool {
    dealbreaker
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .eq(field_snake
            .chars()
            .filter(|c| c.is_alphanumeric())
            .map(|c| c.to_ascii_lowercase()))
}

fn has_dealbreaker(dealbreakers: Option<&[String]>, field_snake: &str) -> bool {
    dealbreakers.is_some_and(|list| list.iter().any(|d| matches_field(d.as_str(), field_snake)))
}

fn compute_compatibility_score_inner<P: ProfileScoringFields>(
    self_p: &P,
    other_p: &P,
    distance_km: Option<f64>,
) -> u32 {
    let pref_weights = self_p.preference_weights();
    let dealbreakers = self_p.dealbreakers();
    let mut total_weighted_score = 0.0f64;
    let mut total_weight = 0.0f64;

    macro_rules! check_field {
        ($field_name:expr, $score_val:expr, $weight_val:expr) => {
            let score = $score_val;
            let weight = $weight_val.map(clamp_weight).unwrap_or(1.0);
            // Dealbreaker = hard requirement: any mismatch below perfect
            // (score < 1.0) on a declared dealbreaker field rejects outright.
            if score < 1.0 && has_dealbreaker(dealbreakers, $field_name) {
                return 0;
            }
            total_weighted_score += score * weight;
            total_weight += weight;
        };
    }
    check_field!(
        "age",
        score_age(self_p.age(), other_p.age()),
        pref_weights.and_then(|w| w.age)
    );
    check_field!(
        "height",
        score_height(self_p.height(), other_p.height()),
        pref_weights.and_then(|w| w.height)
    );
    check_field!(
        "body_type",
        score_body_type(self_p.body_type(), other_p.body_type()),
        pref_weights.and_then(|w| w.body_type)
    );
    check_field!(
        "interests",
        score_interests(self_p.interests(), other_p.interests()),
        pref_weights.and_then(|w| w.interests).or(Some(1.0))
    );
    check_field!(
        "smoking",
        score_smoking(self_p.smoking(), other_p.smoking()),
        pref_weights.and_then(|w| w.smoking)
    );
    check_field!(
        "drinking",
        score_drinking(self_p.drinking(), other_p.drinking()),
        pref_weights.and_then(|w| w.drinking)
    );
    check_field!(
        "politics",
        score_politics(self_p.politics(), other_p.politics()),
        pref_weights.and_then(|w| w.politics)
    );
    check_field!(
        "ethnicity",
        score_ethnicity(self_p.ethnicity(), other_p.ethnicity()),
        pref_weights.and_then(|w| w.ethnicity)
    );
    check_field!(
        "education",
        score_education(self_p.education(), other_p.education()),
        pref_weights.and_then(|w| w.education)
    );
    check_field!(
        "language",
        score_language(self_p.language(), other_p.language()),
        pref_weights.and_then(|w| w.language)
    );
    check_field!(
        "relationship_intent",
        score_relationship_intent(self_p.relationship_intent(), other_p.relationship_intent()),
        pref_weights.and_then(|w| w.relationship_intent)
    );
    if let Some(km) = distance_km {
        check_field!(
            "distance",
            score_distance(km),
            pref_weights.and_then(|w| w.distance)
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
    use crate::PreferenceWeights;

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

    #[test]
    fn matches_field_casing_variations() {
        assert!(matches_field("Age", "age"));
        assert!(matches_field("age", "age"));
        assert!(matches_field("AGE", "age"));
        assert!(matches_field("BodyType", "body_type"));
        assert!(matches_field("bodyType", "body_type"));
        assert!(matches_field("body_type", "body_type"));
        assert!(matches_field("BODY_TYPE", "body_type"));
        assert!(matches_field("body-type", "body_type"));
        assert!(matches_field("RelationshipIntent", "relationship_intent"));
        assert!(matches_field("relationshipIntent", "relationship_intent"));
        assert!(matches_field("RELATIONSHIP_INTENT", "relationship_intent"));
        assert!(matches_field("relationship-intent", "relationship_intent"));

        assert!(!matches_field("Age", "height"));
        assert!(!matches_field("BodyType", "body"));
        assert!(!matches_field("Body", "body_type"));
    }
}
