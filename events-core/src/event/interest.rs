use serde::{Deserialize, Serialize};

use soshal_common_core::json_util::json_out;

#[derive(Deserialize)]
struct InterestScoreInput<'a> {
    #[serde(borrow, rename = "myInterests")]
    my_interests: Vec<&'a str>,
    #[serde(borrow, rename = "peerInterests")]
    peer_interests: Vec<&'a str>,
}

#[derive(Serialize)]
pub struct InterestScoreOutput<'a> {
    pub score: f64,
    #[serde(rename = "common")]
    pub common: Vec<&'a str>,
}

pub fn compute_interest_score<'a>(
    my_interests: &'a [String],
    peer_interests: &'a [String],
) -> InterestScoreOutput<'a> {
    if my_interests.is_empty() || peer_interests.is_empty() {
        return InterestScoreOutput {
            score: 0.0,
            common: vec![],
        };
    }
    let (common, union) =
        soshal_social_core::compatibility::interest_overlap(my_interests, peer_interests, false);
    let score = if union == 0 {
        0.0
    } else {
        common.len() as f64 / union as f64
    };
    InterestScoreOutput { score, common }
}

pub fn compute_interest_score_json(input: &str) -> String {
    let Some(input) = soshal_common_core::json_util::json_in_borrow::<InterestScoreInput>(input)
    else {
        return r#"{"score":0,"common":[]}"#.to_string();
    };
    if input.my_interests.is_empty() || input.peer_interests.is_empty() {
        return r#"{"score":0,"common":[]}"#.to_string();
    }
    let my_owned: Vec<String> = input.my_interests.iter().map(|s| s.to_string()).collect();
    let peer_owned: Vec<String> = input.peer_interests.iter().map(|s| s.to_string()).collect();
    let out = compute_interest_score(&my_owned, &peer_owned);
    json_out(&out, r#"{"score":0,"common":[]}"#)
}
