//! Dating icebreaker generation.
//!
//! Generates template icebreaker prompts from shared interests, intentions,
//! and languages between two dating profiles.

use serde::Deserialize;

use soshal_common_core::json_util::{json_in, json_out};

/// Minimal dating profile data used for icebreaker generation.
#[derive(Deserialize)]
struct DatingProfileMin {
    interests: Option<Vec<String>>,
    #[serde(rename = "relationshipIntent")]
    relationship_intent: Option<String>,
    language: Option<Vec<String>>,
}

/// Input container for generating dating icebreaker messages.
#[derive(Deserialize)]
struct GenerateIcebreakersInput {
    self_profile: DatingProfileMin,
    peer_profile: DatingProfileMin,
}

fn generate_icebreakers(input: GenerateIcebreakersInput) -> Vec<String> {
    let mut prompts = vec![
        "Hey! Nice to match with you.".to_string(),
        "What's your favorite way to spend a weekend?".to_string(),
    ];
    let self_profile = &input.self_profile;
    let peer_profile = &input.peer_profile;
    if let (Some(s_ints), Some(p_ints)) = (
        self_profile.interests.as_ref(),
        peer_profile.interests.as_ref(),
    ) {
        let mut seen = std::collections::HashSet::new();
        let mut shared: Vec<&str> = Vec::new();
        for s in s_ints {
            let s_trim = s.trim();
            if s_trim.is_empty() {
                continue;
            }
            let s_lower = s_trim.to_ascii_lowercase();
            if p_ints.iter().any(|p| p.trim().eq_ignore_ascii_case(s_trim)) && seen.insert(s_lower)
            {
                shared.push(s_trim);
            }
        }
        if !shared.is_empty() {
            let interest_str = if shared.len() >= 2 {
                format!("{} and {}", shared[0], shared[1])
            } else {
                shared[0].to_string()
            };
            prompts.insert(
                0,
                format!(
                    "I notice we both love {}! How did you get into that?",
                    interest_str
                ),
            );
        }
    }
    if let (Some(s_int), Some(p_int)) = (
        self_profile.relationship_intent.as_ref(),
        peer_profile.relationship_intent.as_ref(),
    ) {
        let s_trim = s_int.trim();
        let p_trim = p_int.trim();
        if !s_trim.is_empty() && s_trim.eq_ignore_ascii_case(p_trim) {
            prompts.push(format!(
                "It's awesome that we're both looking for {}. What does that mean to you?",
                s_trim
            ));
        }
    }
    if let (Some(s_ls), Some(p_ls)) = (
        self_profile.language.as_ref(),
        peer_profile.language.as_ref(),
    ) {
        let mut seen = std::collections::HashSet::new();
        let mut shared: Vec<&str> = Vec::new();
        for s in s_ls {
            let s_trim = s.trim();
            if s_trim.is_empty() {
                continue;
            }
            let s_lower = s_trim.to_ascii_lowercase();
            if p_ls.iter().any(|p| p.trim().eq_ignore_ascii_case(s_trim)) && seen.insert(s_lower) {
                shared.push(s_trim);
            }
        }
        if !shared.is_empty() {
            prompts.push(format!("We both speak {}!", shared[0]));
        }
    }
    prompts
}

/// Generates icebreaker prompts from two dating profiles.
///
/// Input JSON shape: `{ "self_profile": DatingProfileMin,
/// "peer_profile": DatingProfileMin }`. Output JSON shape: `string[]`.
pub fn generate_icebreakers_json(input: &str) -> String {
    if input.len() > 1024 * 1024 {
        return "[]".to_string();
    }
    let Some(parsed) = json_in::<Option<GenerateIcebreakersInput>>(input, None) else {
        return "[]".to_string();
    };
    let prompts = generate_icebreakers(parsed);
    json_out(&prompts, "[]")
}
