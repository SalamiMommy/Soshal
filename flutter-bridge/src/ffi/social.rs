//! Social FFI module
//! Compatibility, discovery

use flutter_rust_bridge::frb;

/// Friend suggestions from the local contact graph: users who follow me but
/// whom I do not follow yet, sorted by WoT trust score (best first).
#[frb(sync, serialize)]
pub fn social_friend_suggestions() -> Result<Vec<String>, String> {
    let me = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        // No signer → no identity to suggest for → stay honestly empty.
        Err(_) => return Ok(vec![]).into(),
    };
    let users = all_contact_lists()?;
    let mut my_contacts = Vec::new();
    let mut followers = Vec::new();
    for (pubkey, contacts_json) in users {
        let contacts: Vec<String> = serde_json::from_str(&contacts_json).unwrap_or_default();
        if pubkey == me {
            my_contacts = contacts;
        } else if contacts.contains(&me) {
            followers.push(pubkey);
        }
    }
    let mut candidates = suggest_candidates(&me, &my_contacts, &followers);
    candidates.sort_by(|a, b| {
        let sa = super::identity::identity_get_trust_score(me.clone(), a.clone()).unwrap_or(0.0);
        let sb = super::identity::identity_get_trust_score(me.clone(), b.clone()).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(candidates).into()
}

/// Pure candidate selection: followers of `me` not already followed by `me`.
fn suggest_candidates(me: &str, my_contacts: &[String], followers: &[String]) -> Vec<String> {
    followers
        .iter()
        .filter(|p| *p != me && !my_contacts.contains(p))
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suggest_candidates_excludes_self_and_followed() {
        let my_contacts = vec!["alice".to_string()];
        let followers = vec!["alice".to_string(), "bob".to_string(), "me".to_string()];
        let mut got = suggest_candidates("me", &my_contacts, &followers);
        got.sort();
        assert_eq!(got, vec!["bob".to_string()]);
    }
}
