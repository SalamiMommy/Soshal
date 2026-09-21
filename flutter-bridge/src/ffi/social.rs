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

/// Friend suggestions from the Web of Trust contact graph (kind-3 follows),
/// cached per pubkey for 60 s. Returns suggested pubkeys.
#[frb(sync, serialize)]
pub fn social_friend_suggestions() -> Result<Vec<String>, String> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<super::util::TtlCache<String, Vec<String>>>> = OnceLock::new();
    let me = match super::signer::signer_pubkey() {
        Ok(pk) => pk.trim().to_ascii_lowercase(),
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
    let me = me.trim().to_ascii_lowercase();
    let self_contacts = self_contact_list(&me)?;
    let other_users = other_contact_lists(&me)?;
    let blocked_set: std::collections::HashSet<String> =
        super::db::with_db_result(|db| soshal_db_core::repos::block::BlockRepo::new(db).list(&me))
            .unwrap_or_default()
            .into_iter()
            .map(|pk| pk.trim().to_ascii_lowercase())
            .filter(|pk| !pk.is_empty())
            .collect();
    let mut all_users = Vec::with_capacity(other_users.len());
    for (pubkey, contacts_json) in other_users {
        let pubkey_lower = pubkey.trim().to_ascii_lowercase();
        if pubkey_lower.is_empty() || pubkey_lower == me || blocked_set.contains(&pubkey_lower) {
            continue;
        }
        if contacts_json.len() > 1024 * 1024 {
            continue;
        }
        let parsed_contacts: Vec<String> = serde_json::from_str(&contacts_json).unwrap_or_default();
        let contacts: Vec<String> = parsed_contacts
            .into_iter()
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty() && c.len() <= 128)
            .collect();
        all_users.push(AllUserInfo {
            pubkey,
            contacts,
            wot_distance: 2,
        });
    }
    let suggestions = suggest_mutual_friends(SuggestMutualFriendsInput {
        self_pubkey: me.clone(),
        self_contacts,
        all_users,
        limit: 100,
    });
    Ok(suggestions
        .into_iter()
        .map(|s| s.pubkey)
        .filter(|pk| {
            let pk_lower = pk.trim().to_ascii_lowercase();
            !pk_lower.is_empty() && pk_lower != me && !blocked_set.contains(&pk_lower)
        })
        .collect())
}

/// Load the active user's own contact list directly from the database.
fn self_contact_list(me: &str) -> Result<Vec<String>, String> {
    let me = me.trim().to_ascii_lowercase();
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let found: Option<String> = soshal_db_core::query::query_first(
            &conn,
            "SELECT contact_pubkeys FROM users WHERE LOWER(pubkey) = ?1",
            libsql::params![me.as_str()],
            |r| r.get(0),
        )?;
        Ok(found
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .map(|list| {
                list.into_iter()
                    .map(|c| c.trim().to_ascii_lowercase())
                    .filter(|c| !c.is_empty() && c.len() <= 128)
                    .collect()
            })
            .unwrap_or_default())
    })
}

/// Load candidate users' pubkey + contact list, excluding `me`.
fn other_contact_lists(me: &str) -> Result<Vec<(String, String)>, String> {
    let me = me.trim().to_ascii_lowercase();
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users \
             WHERE LOWER(pubkey) != ?1 AND contact_pubkeys IS NOT NULL \
             AND contact_pubkeys != '[]' AND contact_pubkeys != '' LIMIT 5000",
            libsql::params![me.as_str()],
            |r| {
                let pubkey: String = r.get(0)?;
                let contacts: String = r.get(1)?;
                Ok((pubkey, contacts))
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_friend_suggestions_empty_when_locked() {
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _ = super::super::signer::signer_lock();
        let res = social_friend_suggestions().unwrap();
        assert!(res.is_empty());
    }

    #[test]
    fn test_compute_suggestions_filters_self_and_blocked_case_insensitively() {
        let _d = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("social_filter", "social");
        let me = "a".repeat(64);
        let friend = "b".repeat(64);
        let candidate_blocked = "c".repeat(64);
        let candidate_ok = "d".repeat(64);

        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{}', 'npub1{}', '[\"{}\"]')",
            me.to_lowercase(),
            me.to_lowercase(),
            friend.to_uppercase(),
        ))
        .unwrap();

        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{}', 'npub1{}', '[\"{}\"]')",
            candidate_blocked.to_uppercase(),
            candidate_blocked.to_lowercase(),
            friend.to_lowercase(),
        ))
        .unwrap();

        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{}', 'npub1{}', '[\"{}\"]')",
            candidate_ok.to_uppercase(),
            candidate_ok.to_lowercase(),
            friend.to_lowercase(),
        ))
        .unwrap();

        // Block candidate_blocked with lowercase pubkey
        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO blocks (pubkey, blocked_pubkey, created_at) VALUES ('{}', '{}', 12345)",
            me.to_lowercase(),
            candidate_blocked.to_lowercase(),
        ))
        .unwrap();

        let suggestions = compute_suggestions(&me).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].eq_ignore_ascii_case(&candidate_ok));
    }

    #[test]
    fn test_compute_suggestions_oversized_contacts_ignored() {
        let _d = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("social_oversized", "social");
        let me = "e".repeat(64);
        let huge_candidate = "f".repeat(64);
        let huge_contacts = format!("[\"{}\"]", "x".repeat(1024 * 1024 + 10));

        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{me}', 'npub1{me}', '[]')"
        ))
        .unwrap();

        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{huge_candidate}', 'npub1{huge_candidate}', '{huge_contacts}')"
        ))
        .unwrap();

        let suggestions = compute_suggestions(&me).unwrap();
        assert!(suggestions.is_empty());
    }
}
