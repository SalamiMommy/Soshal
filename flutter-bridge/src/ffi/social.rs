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
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users",
            (),
            |r| {
                let pubkey: String = r.get(0)?;
                let contacts: String = r.get(1)?;
                Ok((pubkey, contacts))
            },
        )
    })
}
