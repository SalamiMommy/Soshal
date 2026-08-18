//! Dating compatibility scoring, profile filtering, and sorting.
//!
//! Domain logic for the DatingService — compatibility scoring (one-directional
//! and mutual), profile filtering by gender/seeking/distance, and sorting by
//! compatibility score.

use serde::{Deserialize, Serialize};

// Re-export json utilities from common-core
pub use soshal_common_core::json_util::{json_in, json_out};

pub mod filter;
pub mod icebreaker;
pub mod scoring;
pub mod sort;

/// Maximum number of profiles we accept in a single call.
#[doc(hidden)]
pub const MAX_PROFILES: usize = 100_000;

/// Weight configuration for dating compatibility scoring.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceWeights {
    pub age: Option<f64>,
    pub height: Option<f64>,
    pub body_type: Option<f64>,
    pub interests: Option<f64>,
    pub smoking: Option<f64>,
    pub drinking: Option<f64>,
    pub politics: Option<f64>,
    pub ethnicity: Option<f64>,
    pub education: Option<f64>,
    pub language: Option<f64>,
    pub relationship_intent: Option<f64>,
}

/// A dating profile with preferences, bio, photos and traits.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatingProfile {
    pub age: Option<f64>,
    pub gender: Option<String>,
    pub seeking: Option<String>,
    pub height: Option<f64>,
    pub body_type: Option<String>,
    pub smoking: Option<String>,
    pub drinking: Option<String>,
    pub bio: Option<String>,
    pub relationship_intent: Option<String>,
    pub location_geohash: Option<String>,
    pub max_distance_km: Option<f64>,
    pub verified_mutual_friends: Option<Vec<String>>,
    pub interests: Option<Vec<String>>,
    pub images: Option<Vec<String>>,
    pub politics: Option<String>,
    pub ethnicity: Option<String>,
    pub education: Option<String>,
    pub language: Option<Vec<String>>,
    pub preference_weights: Option<PreferenceWeights>,
    pub dealbreakers: Option<Vec<String>>,
}

/// Input container for computing dating compatibility scores.
#[derive(Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DatingProfileInput {
    pub event_id: Option<String>,
    pub pubkey: String,
    pub age: Option<f64>,
    pub gender: Option<String>,
    pub seeking: Option<String>,
    pub height: Option<f64>,
    pub body_type: Option<String>,
    pub smoking: Option<String>,
    pub drinking: Option<String>,
    pub relationship_intent: Option<String>,
    pub location_geohash: Option<String>,
    pub max_distance_km: Option<f64>,
    pub interests: Option<Vec<String>>,
    pub politics: Option<String>,
    pub ethnicity: Option<String>,
    pub education: Option<String>,
    pub language: Option<Vec<String>>,
    pub preference_weights: Option<PreferenceWeights>,
    pub dealbreakers: Option<Vec<String>>,
    pub verified_mutual_friends: Option<Vec<String>>,
    pub liked_by_me: Option<bool>,
    pub liked_me: Option<bool>,
    pub liker_total_likes: Option<u32>,
}

/// Scoring-relevant profile fields, shared by `DatingProfile` and `DatingProfileInput`.
#[doc(hidden)]
pub trait ProfileScoringFields {
    fn age(&self) -> Option<f64>;
    fn height(&self) -> Option<f64>;
    fn body_type(&self) -> Option<&str>;
    fn interests(&self) -> Option<&[String]>;
    fn smoking(&self) -> Option<&str>;
    fn drinking(&self) -> Option<&str>;
    fn politics(&self) -> Option<&str>;
    fn ethnicity(&self) -> Option<&str>;
    fn education(&self) -> Option<&str>;
    fn language(&self) -> Option<&[String]>;
    fn relationship_intent(&self) -> Option<&str>;
    fn preference_weights(&self) -> Option<&PreferenceWeights>;
    fn dealbreakers(&self) -> Option<&[String]>;
}

impl ProfileScoringFields for DatingProfile {
    fn age(&self) -> Option<f64> {
        self.age
    }
    fn height(&self) -> Option<f64> {
        self.height
    }
    fn body_type(&self) -> Option<&str> {
        self.body_type.as_deref()
    }
    fn interests(&self) -> Option<&[String]> {
        self.interests.as_deref()
    }
    fn smoking(&self) -> Option<&str> {
        self.smoking.as_deref()
    }
    fn drinking(&self) -> Option<&str> {
        self.drinking.as_deref()
    }
    fn politics(&self) -> Option<&str> {
        self.politics.as_deref()
    }
    fn ethnicity(&self) -> Option<&str> {
        self.ethnicity.as_deref()
    }
    fn education(&self) -> Option<&str> {
        self.education.as_deref()
    }
    fn language(&self) -> Option<&[String]> {
        self.language.as_deref()
    }
    fn relationship_intent(&self) -> Option<&str> {
        self.relationship_intent.as_deref()
    }
    fn preference_weights(&self) -> Option<&PreferenceWeights> {
        self.preference_weights.as_ref()
    }
    fn dealbreakers(&self) -> Option<&[String]> {
        self.dealbreakers.as_deref()
    }
}

impl ProfileScoringFields for DatingProfileInput {
    fn age(&self) -> Option<f64> {
        self.age
    }
    fn height(&self) -> Option<f64> {
        self.height
    }
    fn body_type(&self) -> Option<&str> {
        self.body_type.as_deref()
    }
    fn interests(&self) -> Option<&[String]> {
        self.interests.as_deref()
    }
    fn smoking(&self) -> Option<&str> {
        self.smoking.as_deref()
    }
    fn drinking(&self) -> Option<&str> {
        self.drinking.as_deref()
    }
    fn politics(&self) -> Option<&str> {
        self.politics.as_deref()
    }
    fn ethnicity(&self) -> Option<&str> {
        self.ethnicity.as_deref()
    }
    fn education(&self) -> Option<&str> {
        self.education.as_deref()
    }
    fn language(&self) -> Option<&[String]> {
        self.language.as_deref()
    }
    fn relationship_intent(&self) -> Option<&str> {
        self.relationship_intent.as_deref()
    }
    fn preference_weights(&self) -> Option<&PreferenceWeights> {
        self.preference_weights.as_ref()
    }
    fn dealbreakers(&self) -> Option<&[String]> {
        self.dealbreakers.as_deref()
    }
}

/// A single sorted profile result with compatibility score.
#[derive(Serialize, Deserialize)]
pub struct SortedProfileOut {
    pub event_id: Option<String>,
    pub pubkey: String,
    pub compatibility_score: u32,
    pub mutual_friends: Vec<String>,
    pub distance: u32,
    pub liked_by_me: bool,
    pub liked_me: bool,
    pub liker_total_likes: u32,
}

/// A single filtered dating profile result.
#[derive(Serialize, Deserialize)]
pub struct FilteredDatingProfileOut {
    pub index: usize,
    pub passes: bool,
    pub is_contact: bool,
    pub mutual_friends: Vec<String>,
}

/// Deserializes an optional `f64` that may arrive as a JSON number or a numeric string.
fn de_opt_f64<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(d)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected number")),
        Some(serde_json::Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .map(Some)
            .map_err(|_| serde::de::Error::custom("expected numeric string")),
        Some(_) => Err(serde::de::Error::custom("expected number or string")),
    }
}

/// Input container for sorting dating profiles by compatibility.
#[derive(Deserialize)]
pub struct SortProfilesInput {
    pub profiles: Vec<DatingProfileInput>,
    #[serde(rename = "selfProfile")]
    pub self_profile: DatingProfileInput,
    #[serde(rename = "selfContacts")]
    pub self_contacts: Vec<String>,
    #[serde(rename = "sortBy")]
    pub sort_by: Option<String>,
}

/// Input container for filtering dating profiles.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterDatingProfilesInput {
    pub profiles: Vec<DatingProfileInput>,
    pub own_gender: Option<String>,
    pub own_seeking: Option<String>,
    pub own_location_geohash: Option<String>,
    pub own_max_distance_km: Option<f64>,
    pub self_contacts: Vec<String>,
    #[serde(default)]
    pub hide_friends: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub min_age: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub max_age: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub height_min_cm: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub height_max_cm: Option<f64>,
    pub body_type: Option<String>,
    pub smoking: Option<String>,
    pub drinking: Option<String>,
    pub relationship_intent: Option<String>,
    pub politics: Option<String>,
    pub education: Option<String>,
}

// ---------------------------------------------------------------------------
// Public JSON API (replaces WASM FFI)
// ---------------------------------------------------------------------------

/// Computes one-directional dating compatibility (0–100).
/// Input: two JSON `DatingProfile` strings.
pub fn compute_compatibility_json(self_input: &str, other_input: &str) -> String {
    let Some(self_profile) = json_in::<Option<DatingProfile>>(self_input, None) else {
        return "0.0".to_string();
    };
    let Some(other_profile) = json_in::<Option<DatingProfile>>(other_input, None) else {
        return "0.0".to_string();
    };
    let score = scoring::compute_compatibility_score(&self_profile, &other_profile);
    json_out(&score, "0.0")
}

/// Computes mutual (bidirectional, averaged) compatibility score.
pub fn compute_mutual_score_json(self_input: &str, other_input: &str) -> String {
    let Some(self_profile) = json_in::<Option<DatingProfile>>(self_input, None) else {
        return "0.0".to_string();
    };
    let Some(other_profile) = json_in::<Option<DatingProfile>>(other_input, None) else {
        return "0.0".to_string();
    };
    let score = scoring::compute_mutual_score(&self_profile, &other_profile);
    json_out(&score, "0.0")
}

/// Sorts dating profiles by compatibility, liked_me, and distance.
pub fn sort_profiles_json(input: &str) -> String {
    let Some(parsed) = json_in::<Option<SortProfilesInput>>(input, None) else {
        return "[]".to_string();
    };
    let out = sort::sort_dating_profiles(parsed);
    json_out(&out, "[]")
}

/// Filters dating profiles by gender/seeking/distance.
pub fn filter_profiles_json(input: &str) -> String {
    let Some(parsed) = json_in::<Option<FilterDatingProfilesInput>>(input, None) else {
        return "[]".to_string();
    };
    let out = filter::filter_dating_profiles(parsed);
    json_out(&out, "[]")
}

/// Generates icebreaker prompts from two dating profiles.
pub fn generate_icebreakers_json(input: &str) -> String {
    icebreaker::generate_icebreakers_json(input)
}
