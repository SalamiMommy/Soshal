use serde::{Deserialize, Serialize};

use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
struct InterestScoreInput {
    #[serde(rename = "myInterests")]
    my_interests: Vec<String>,
    #[serde(rename = "peerInterests")]
    peer_interests: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct InterestScoreOutput {
    pub score: f64,
    #[serde(rename = "common")]
    pub common: Vec<String>,
}

pub fn compute_interest_score(
    my_interests: &[String],
    peer_interests: &[String],
) -> InterestScoreOutput {
    if my_interests.is_empty() || peer_interests.is_empty() {
        return InterestScoreOutput {
            score: 0.0,
            common: vec![],
        };
    }
    let (common, union) =
        soshal_social_core::compatibility::interest_overlap(my_interests, peer_interests, false);
    let common_owned: Vec<String> = common.into_iter().map(|s| s.to_string()).collect();
    let score = if union == 0 {
        0.0
    } else {
        common_owned.len() as f64 / union as f64
    };
    InterestScoreOutput {
        score,
        common: common_owned,
    }
}

pub fn compute_interest_score_json(input: &str) -> String {
    let Some(input) = json_in::<Option<InterestScoreInput>>(input, None) else {
        return r#"{"score":0,"common":[]}"#.to_string();
    };
    let out = compute_interest_score(&input.my_interests, &input.peer_interests);
    json_out(&out, r#"{"score":0,"common":[]}"#)
}
