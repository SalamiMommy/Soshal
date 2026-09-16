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
    static CACHE: OnceLock<Mutex<super::util::TtlCache<String, Vec<String>>>> = OnceLock::new();
    let me = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        // No signer → no identity to suggest for → stay honestly empty.
        Err(_) => return Ok(vec![]).into(),
    };
    let now = soshal_common_core::format::now_secs();
    let mut cache = crate::ffi::util::lock(
        CACHE.get_or_init(|| Mutex::new(super::util::TtlCache::new(SUGGESTIONS_TTL_SECS, 8))),
    );
    if let Some(cached) = cache.get(me.as_str(), now) {
        return Ok(cached.clone()).into();
    }
    let suggestions = compute_suggestions(&me)?;
    cache.insert(me, suggestions.clone(), now);
    Ok(suggestions).into()
}

fn compute_suggestions(me: &str) -> Result<Vec<String>, String> {
    let self_contacts = self_contact_list(me)?;
    let other_users = other_contact_lists(me)?;
    let mut all_users = Vec::with_capacity(other_users.len());
    for (pubkey, contacts_json) in other_users {
        let contacts: Vec<String> = serde_json::from_str(&contacts_json).unwrap_or_default();
        all_users.push(AllUserInfo {
            pubkey,
            contacts,
            wot_distance: 2,
        });
    }
    let suggestions = suggest_mutual_friends(SuggestMutualFriendsInput {
        self_pubkey: me.to_string(),
        self_contacts,
        all_users,
        limit: 100,
    });
    Ok(suggestions.into_iter().map(|s| s.pubkey).collect())
}

/// Load the active user's own contact list directly from the database.
fn self_contact_list(me: &str) -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let found: Option<String> = soshal_db_core::query::query_first(
            &conn,
            "SELECT contact_pubkeys FROM users WHERE pubkey = ?1",
            libsql::params![me],
            |r| r.get(0),
        )?;
        Ok(found
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    })
}

/// Load candidate users' pubkey + contact list, excluding `me`.
fn other_contact_lists(me: &str) -> Result<Vec<(String, String)>, String> {
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users \
             WHERE pubkey != ?1 AND contact_pubkeys IS NOT NULL \
             AND contact_pubkeys != '[]' AND contact_pubkeys != '' LIMIT 5000",
            libsql::params![me],
            |r| {
                let pubkey: String = r.get(0)?;
                let contacts: String = r.get(1)?;
                Ok((pubkey, contacts))
            },
        )
    })
}
