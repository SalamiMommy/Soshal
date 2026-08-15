//! Social FFI module
//! Compatibility, discovery

use flutter_rust_bridge::frb;
use soshal_network_core::discovery::{
    suggest_mutual_friends, AllUserInfo, SuggestMutualFriendsInput,
};

/// Friend suggestions from the local contact graph, delegated to
/// network-core's mutual-friend discovery (pubkeys only, best first).
#[frb(sync, serialize)]
pub fn social_friend_suggestions() -> Result<Vec<String>, String> {
    let me = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        // No signer → no identity to suggest for → stay honestly empty.
        Err(_) => return Ok(vec![]).into(),
    };
    let users = all_contact_lists()?;
    let mut self_contacts = Vec::new();
    let mut all_users = Vec::new();
    for (pubkey, contacts_json) in users {
        let contacts: Vec<String> = serde_json::from_str(&contacts_json).unwrap_or_default();
        if pubkey == me {
            self_contacts = contacts;
        } else {
            all_users.push(AllUserInfo {
                pubkey,
                contacts,
                wot_distance: 2,
            });
        }
    }
    let suggestions = suggest_mutual_friends(SuggestMutualFriendsInput {
        self_pubkey: me,
        self_contacts,
        all_users,
        limit: 100_000,
    });
    Ok(suggestions.into_iter().map(|s| s.pubkey).collect()).into()
}

/// Load every stored user's pubkey + contact list (raw query; repos have no
/// list-all query).
fn all_contact_lists() -> Result<Vec<(String, String)>, String> {
    let json = super::db::db_query_raw("SELECT pubkey, contact_pubkeys FROM users".to_string())?;
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&json).map_err(|e| format!("parse users: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        let get = |k: &str| -> Option<String> {
            r.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
        };
        if let Some(pubkey) = get("pubkey") {
            let contacts = get("contact_pubkeys").unwrap_or_else(|| "[]".to_string());
            out.push((pubkey, contacts));
        }
    }
    Ok(out)
}
