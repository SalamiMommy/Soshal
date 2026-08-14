use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct RawMessageReaction {
    pub emoji: String,
    #[serde(rename = "reactorPubkey")]
    pub reactor_pubkey: String,
}

#[derive(Deserialize)]
pub struct AggregateReactionsInput {
    pub reactions: Vec<RawMessageReaction>,
    #[serde(rename = "selfPubkey")]
    pub self_pubkey: String,
}

#[derive(Serialize)]
pub struct MessageReactionsSummaryOut {
    pub emoji: String,
    pub count: usize,
    #[serde(rename = "hasReacted")]
    pub has_reacted: bool,
}

pub fn aggregate_message_reactions(
    input: AggregateReactionsInput,
) -> Vec<MessageReactionsSummaryOut> {
    if input.reactions.len() > 100_000 {
        return Vec::new();
    }
    let mut emoji_map: HashMap<&str, (usize, bool)> = HashMap::new();
    for r in &input.reactions {
        if r.emoji.len() > 64 {
            continue;
        }
        let has_reacted = r.reactor_pubkey == input.self_pubkey;
        let entry = emoji_map.entry(r.emoji.as_str()).or_insert((0, false));
        entry.0 += 1;
        if has_reacted {
            entry.1 = true;
        }
    }

    let mut summary: Vec<MessageReactionsSummaryOut> = emoji_map
        .into_iter()
        .map(|(emoji, (count, has_reacted))| MessageReactionsSummaryOut {
            emoji: emoji.to_string(),
            count,
            has_reacted,
        })
        .collect();

    summary.sort_by(|a, b| a.emoji.cmp(&b.emoji));
    summary
}

pub fn aggregate_message_reactions_json(input: &str) -> String {
    let Some(input) = json_in::<Option<AggregateReactionsInput>>(input, None) else {
        return "[]".to_string();
    };
    json_out(&aggregate_message_reactions(input), "[]")
}
