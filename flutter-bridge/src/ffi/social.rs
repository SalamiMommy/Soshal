//! Social FFI module
//! Compatibility, discovery

use flutter_rust_bridge::frb;
use soshal_network_core::discovery::{
    suggest_mutual_friends, AllUserInfo, SuggestMutualFriendsInput,
};

/// Friend suggestions from the local contact graph, delegated to
/// network-core's mutual-friend discovery (pubkeys only, best first).
///
/// The underlying WoT pass scans up to 5000 contact lists; the result is
/// cached per pubkey for 60 s so repeated opens don't redo the scan.
const SUGGESTIONS_TTL_SECS: i64 = 60;

#[frb(sync, serialize)]
pub fn social_friend_suggestions() -> Result<Vec<String>, String> {
    use std::sync::{Mutex, OnceLock};
    type SuggestionsCache = Option<(String, i64, Vec<String>)>;
    static CACHE: OnceLock<Mutex<SuggestionsCache>> = OnceLock::new();
    let me = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        // No signer → no identity to suggest for → stay honestly empty.
        Err(_) => return Ok(vec![]).into(),
    };
    let now = soshal_common_core::format::now_secs();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((pk, ts, cached)) = guard.as_ref() {
            if pk == &me && now.saturating_sub(*ts) < SUGGESTIONS_TTL_SECS {
                return Ok(cached.clone()).into();
            }
        }
    }
    let suggestions = compute_suggestions(&me)?;
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((me, now, suggestions.clone()));
    Ok(suggestions).into()
}

fn compute_suggestions(me: &str) -> Result<Vec<String>, String> {
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
        self_pubkey: me.to_string(),
        self_contacts,
        all_users,
        limit: 100_000,
    });
    Ok(suggestions.into_iter().map(|s| s.pubkey).collect())
}

/// Load every stored user's pubkey + contact list (raw query; repos have no
/// list-all query). Users with no contacts can't contribute to the WoT
/// suggestions and are skipped before the JSON parse.
fn all_contact_lists() -> Result<Vec<(String, String)>, String> {
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users \
             WHERE contact_pubkeys != '[]' LIMIT 5000",
            (),
            |r| {
                let pubkey: String = r.get(0)?;
                let contacts: String = r.get(1)?;
                Ok((pubkey, contacts))
            },
        )
    })
}
