//! Identity FFI module
//!
//! User profiles (DB-backed), NIP-05 verification, Web of Trust scoring,
//! follow state, and local blocklist. Publishing profile/contact-list events
//! signs with the unlocked signer (kind 0 / kind 3).

use flutter_rust_bridge::frb;
use nostr::event::EventBuilder;
use nostr::event::Kind;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::block::BlockRepo;
use soshal_db_core::repos::user::{UserRepo, UserRow};
use soshal_identity_core::wot;

/// User profile info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileInfo {
    pub pubkey: String,
    pub name: String,
    pub display_name: String,
    pub picture: String,
    pub banner: String,
    pub about: String,
    pub nip05: String,
    pub nip05_valid: bool,
    pub created_at: u64,
    pub followers: i32,
    pub following: i32,
    pub is_following: bool,
    pub wot_status: String,
}

fn empty_profile(pubkey: String) -> ProfileInfo {
    ProfileInfo {
        pubkey,
        name: String::new(),
        display_name: String::new(),
        picture: String::new(),
        banner: String::new(),
        about: String::new(),
        nip05: String::new(),
        nip05_valid: false,
        created_at: 0,
        followers: 0,
        following: 0,
        is_following: false,
        wot_status: "unknown".to_string(),
    }
}

fn row_to_profile(row: &UserRow, me: Option<&str>) -> ProfileInfo {
    let mut p = empty_profile(row.pubkey.clone());
    p.name = row.name.clone().unwrap_or_default();
    p.display_name = row.display_name.clone().unwrap_or_default();
    p.picture = row.picture.clone().unwrap_or_default();
    p.banner = row.banner.clone().unwrap_or_default();
    p.about = row.about.clone().unwrap_or_default();
    p.nip05 = row.nip05.clone().unwrap_or_default();
    p.created_at = row.created_at.max(0) as u64;
    if let Some(me) = me {
        if let Ok(follows) = serde_json::from_str::<Vec<String>>(&row.contact_pubkeys) {
            p.is_following = follows.contains(&row.pubkey) && row.pubkey != me;
        }
    }
    p.following = serde_json::from_str::<Vec<String>>(&row.contact_pubkeys)
        .map(|f| f.len() as i32)
        .unwrap_or(0);
    p
}

/// Get a user profile from the local DB (created empty on first sight).
#[frb(sync, serialize)]
pub fn identity_get_profile(pubkey: String) -> Result<String, String> {
    let me = super::signer::signer_pubkey().ok();
    super::db::with_db_result(|db| {
        let repo = UserRepo::new(db);
        let row = repo.get_by_pubkey(&pubkey)?;
        let p = match row {
            Some(r) => row_to_profile(&r, me.as_deref()),
            None => empty_profile(pubkey),
        };
        Ok(p)
    })
    .map(super::util::json_ok)?
}

/// Upsert a fetched kind-0 profile row into the DB.
#[frb(sync, serialize)]
pub fn identity_store_profile(profile: String) -> Result<bool, String> {
    let v: serde_json::Value =
        serde_json::from_str(&profile).map_err(|e| format!("invalid profile JSON: {e}"))?;
    let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
    let content_v: serde_json::Value =
        serde_json::from_str(content).unwrap_or(serde_json::Value::Null);
    let get = |k: &str| -> Option<String> {
        content_v
            .get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let pubkey = v
        .get("pubkey")
        .and_then(|p| p.as_str())
        .ok_or_else(|| "missing pubkey".to_string())?;
    let now = soshal_common_core::format::now_secs();
    let row = UserRow {
        pubkey: pubkey.to_string(),
        npub: soshal_identity_core::keys::npub_encode(pubkey).unwrap_or_default(),
        name: get("name"),
        display_name: get("display_name"),
        about: get("about"),
        picture: get("picture"),
        banner: get("banner"),
        nip05: get("nip05"),
        lud16: None,
        created_at: v.get("created_at").and_then(|c| c.as_i64()).unwrap_or(now),
        updated_at: now,
        metadata_json: Some(content.to_string()),
        contact_pubkeys: String::from("[]"),
        relay_list: String::from("[]"),
    };
    super::db::with_db_result(|db| {
        UserRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Search users by name/about (FTS-ish prefix match).
#[frb(sync, serialize)]
pub fn identity_search_users(query: String, limit: i32) -> Result<String, String> {
    let limit = limit.clamp(1, 100) as i64;
    super::db::with_db_result(|db| {
        let rows = UserRepo::new(db).search(&query, limit)?;
        let profiles: Vec<ProfileInfo> = rows.iter().map(|r| row_to_profile(r, None)).collect();
        Ok(profiles)
    })
    .map(super::util::json_ok)?
}

/// Get the active account's own profile (alias of `identity_get_profile`).
#[frb(sync, serialize)]
pub fn identity_get_self_profile(pubkey: String) -> Result<String, String> {
    identity_get_profile(pubkey)
}

/// Build and sign a kind-0 profile metadata event for the unlocked account.
/// `pubkey` is validated against the unlocked signer; returns signed JSON
/// (publish via `network_publish_event`).
#[frb(sync, serialize)]
pub fn identity_update_profile(
    pubkey: String,
    name: String,
    display_name: String,
    picture: String,
    banner: String,
    about: String,
    nip05: String,
) -> Result<String, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    if unlocked != pubkey {
        return Err("pubkey does not match unlocked signer".to_string()).into();
    }
    let profile = serde_json::json!({
        "name": name,
        "display_name": display_name,
        "picture": picture,
        "banner": banner,
        "about": about,
        "nip05": nip05,
    })
    .to_string();
    let builder = EventBuilder::new(Kind::Metadata, profile);
    super::signer::sign_builder(builder)
}

/// Verify a NIP-05 identifier against the published `.well-known` document.
#[frb(serialize)]
pub async fn identity_verify_nip05(nip05: String) -> Result<bool, String> {
    match verify_nip05_fut(&nip05).await {
        Ok((valid, _)) => Ok(valid).into(),
        Err(e) => Err(e).into(),
    }
}

async fn verify_nip05_fut(nip05: &str) -> Result<(bool, String), String> {
    // identity-core exposes the record struct; construct the URL and fetch.
    let (name, domain) = split_nip05(nip05);
    let url = format!("https://{domain}/.well-known/nostr.json?name={name}");
    if !soshal_common_core::url::is_valid_media_url(&url) {
        return Err("nip05 domain is not allowed (private or local host)".into());
    }
    let host = reqwest::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .ok_or_else(|| "nip05 url has no host".to_string())?;
    let mut pinned_addrs: Vec<std::net::SocketAddr> = Vec::new();
    match tokio::net::lookup_host((host.as_str(), 443)).await {
        Ok(addrs) => {
            for addr in addrs {
                if soshal_common_core::url::is_private_ip_str(&addr.ip().to_string()) {
                    return Err("nip05 domain resolves to an internal address".into());
                }
                pinned_addrs.push(addr);
            }
        }
        Err(_) => return Err("nip05 domain does not resolve".into()),
    }
    if pinned_addrs.is_empty() {
        return Err("nip05 domain does not resolve".into());
    }
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(addr) = super::network::i2p_socks_addr() {
        if let Ok(proxy) = reqwest::Proxy::all(format!("socks5://{addr}")) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder
        .resolve_to_addrs(&host, &pinned_addrs)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok((false, String::new()));
    }
    const MAX_NIP05_BODY: usize = 256 * 1024;
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_NIP05_BODY {
            return Err("nip05 response too large".into());
        }
    }
    let mut body = Vec::new();
    let mut resp = resp;
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > MAX_NIP05_BODY {
            return Err("nip05 response too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("invalid nip05 response: {e}"))?;
    let entry = json
        .pointer(&format!("/names/{name}"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let result = entry.to_lowercase();
    Ok((result.starts_with("npub1") || result.len() == 64, result))
}

fn split_nip05(nip05: &str) -> (String, String) {
    match nip05.rsplit_once('@') {
        Some((name, domain)) => (name.to_string(), domain.to_string()),
        None => (String::new(), nip05.to_string()),
    }
}

/// Compute the WoT trust score (0..1) of `target` from `viewer`'s graph,
/// using all stored users' contact lists.
#[frb(sync, serialize)]
pub fn identity_get_trust_score(
    source_pubkey: String,
    target_pubkey: String,
) -> Result<f32, String> {
    let users = all_users()?;
    let mut self_contacts = Vec::new();
    let mut target_contacts = Vec::new();
    let mut wot_users = Vec::new();
    for u in &users {
        let contacts: Vec<String> = serde_json::from_str(&u.contact_pubkeys).unwrap_or_default();
        if u.pubkey == source_pubkey {
            self_contacts = contacts.clone();
        }
        if u.pubkey == target_pubkey {
            target_contacts = contacts.clone();
        }
        wot_users.push(wot::WotUser {
            pubkey: u.pubkey.clone(),
            contacts: contacts.clone(),
        });
    }
    let score = wot::compute_trust_score(
        &source_pubkey,
        &target_pubkey,
        &self_contacts,
        &target_contacts,
    )
    .score;
    Ok(score as f32).into()
}

/// Load every stored user row (raw query, since repos have no list-all).
fn all_users() -> Result<Vec<UserRow>, String> {
    let json = super::db::db_query_raw(
        "SELECT pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list FROM users"
            .to_string(),
    )?;
    let rows: Vec<serde_json::Value> = match serde_json::from_str(&json) {
        Ok(r) => r,
        Err(e) => return Err(format!("parse users: {e}")),
    };
    let mut out = Vec::new();
    for r in rows {
        let get = |k: &str| -> Option<String> {
            r.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
        };
        out.push(UserRow {
            pubkey: get("pubkey").unwrap_or_default(),
            npub: get("npub").unwrap_or_default(),
            name: get("name"),
            display_name: get("display_name"),
            about: get("about"),
            picture: get("picture"),
            banner: get("banner"),
            nip05: get("nip05"),
            lud16: get("lud16"),
            created_at: r.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0),
            updated_at: r.get("updated_at").and_then(|v| v.as_i64()).unwrap_or(0),
            metadata_json: get("metadata_json"),
            contact_pubkeys: get("contact_pubkeys").unwrap_or_else(|| "[]".to_string()),
            relay_list: get("relay_list").unwrap_or_else(|| "[]".to_string()),
        });
    }
    Ok(out)
}

/// Classify the target as trusted / warning / unknown relative to the
/// viewer (WoT distance 1 = `trusted`, distance 2 = `warning`).
/// Args: (target_pubkey, viewer_pubkey) for UI parity with the desktop
/// command surface.
#[frb(sync, serialize)]
pub fn identity_get_wot_status(
    target_pubkey: String,
    viewer_pubkey: String,
) -> Result<String, String> {
    let users = all_users()?;
    let wot_users: Vec<wot::WotUser> = users
        .iter()
        .map(|u| wot::WotUser {
            pubkey: u.pubkey.clone(),
            contacts: serde_json::from_str(&u.contact_pubkeys).unwrap_or_default(),
        })
        .collect();
    let by_distance = wot::get_wot_peers_by_distance(&viewer_pubkey, &wot_users, 2);
    let status = if by_distance
        .get(&1)
        .map(|v| v.contains(&target_pubkey))
        .unwrap_or(false)
    {
        "trusted"
    } else if by_distance
        .get(&2)
        .map(|v| v.contains(&target_pubkey))
        .unwrap_or(false)
    {
        "warning"
    } else {
        "unknown"
    };
    Ok(status.to_string()).into()
}

/// Publish a signed event JSON from a sync context, via a short-lived
/// single-threaded runtime (mirrors `identity_verify_nip05`).
fn publish_event(signed: String) -> Result<i32, String> {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return Err(format!("runtime: {e}")),
    };
    runtime.block_on(super::network::network_publish_event(signed))
}

/// Follow `pubkey`: rebuild the FULL NIP-02 contact list of the unlocked
/// signer from the local `users.contact_pubkeys` (appending the target when
/// absent), persist it, sign a kind-3 event and publish. Returns the signed
/// event JSON.
#[frb(sync, serialize)]
pub fn identity_follow_user(pubkey: String) -> Result<String, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let (list, row) = super::db::with_db_result(|db| {
        let row = UserRepo::new(db).get_by_pubkey(&unlocked)?;
        let mut follows: Vec<String> = match &row {
            Some(r) => serde_json::from_str(&r.contact_pubkeys).unwrap_or_default(),
            None => Vec::new(),
        };
        if !follows.contains(&pubkey) {
            follows.push(pubkey);
        }
        Ok((follows, row))
    })?;
    let updated = serde_json::to_string(&list).map_err(|e| format!("serialize: {e}"))?;
    super::db::with_db_result(|db| {
        let mut r = match row {
            Some(r) => r,
            None => UserRow {
                pubkey: unlocked.clone(),
                npub: soshal_identity_core::keys::npub_encode(&unlocked).unwrap_or_default(),
                name: None,
                display_name: None,
                about: None,
                picture: None,
                banner: None,
                nip05: None,
                lud16: None,
                created_at: soshal_common_core::format::now_secs(),
                updated_at: soshal_common_core::format::now_secs(),
                metadata_json: None,
                contact_pubkeys: String::new(),
                relay_list: String::from("[]"),
            },
        };
        r.contact_pubkeys = updated;
        UserRepo::new(db).upsert(&r)?;
        Ok(())
    })?;
    let mut builder = EventBuilder::new(Kind::ContactList, "");
    for f in &list {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), f.clone()]) {
            builder = builder.tag(tag);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    publish_event(signed.clone())?;
    Ok(signed)
}

/// Unfollow `pubkey`: rebuild the signer's full NIP-02 contact list minus the
/// target, persist it, sign a kind-3 event and publish. Returns true.
#[frb(sync, serialize)]
pub fn identity_unfollow_user(pubkey: String) -> Result<bool, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let (list, row) = super::db::with_db_result(|db| {
        let row = UserRepo::new(db).get_by_pubkey(&unlocked)?;
        let mut follows: Vec<String> = match &row {
            Some(r) => serde_json::from_str(&r.contact_pubkeys).unwrap_or_default(),
            None => Vec::new(),
        };
        follows.retain(|f| f != &pubkey);
        Ok((follows, row))
    })?;
    let updated = serde_json::to_string(&list).map_err(|e| format!("serialize: {e}"))?;
    super::db::with_db_result(|db| {
        let mut r = match row {
            Some(r) => r,
            None => UserRow {
                pubkey: unlocked.clone(),
                npub: soshal_identity_core::keys::npub_encode(&unlocked).unwrap_or_default(),
                name: None,
                display_name: None,
                about: None,
                picture: None,
                banner: None,
                nip05: None,
                lud16: None,
                created_at: soshal_common_core::format::now_secs(),
                updated_at: soshal_common_core::format::now_secs(),
                metadata_json: None,
                contact_pubkeys: String::new(),
                relay_list: String::from("[]"),
            },
        };
        r.contact_pubkeys = updated;
        UserRepo::new(db).upsert(&r)?;
        Ok(())
    })?;
    let mut builder = EventBuilder::new(Kind::ContactList, "");
    for f in &list {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), f.clone()]) {
            builder = builder.tag(tag);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    publish_event(signed)?;
    Ok(true)
}

/// Fetch the followed pubkeys of `pubkey` from the local contact list.
/// Returns a JSON array of pubkey strings (`[]` when unknown or empty).
#[frb(sync, serialize)]
pub fn identity_fetch_follows(pubkey: String) -> Result<String, String> {
    let follows = super::db::with_db_result(|db| {
        let row = UserRepo::new(db).get_by_pubkey(&pubkey)?;
        Ok(match row {
            Some(r) => serde_json::from_str::<Vec<String>>(&r.contact_pubkeys).unwrap_or_default(),
            None => Vec::new(),
        })
    })?;
    serde_json::to_string(&follows).map_err(|e| format!("serialize: {e}"))
}

/// Publish a NIP-65 relay-list metadata event (kind 10002) for the unlocked
/// signer. Every URL must be valid `wss://`. Returns the event id.
#[frb(sync, serialize)]
pub fn identity_publish_relay_list(relay_urls: Vec<String>) -> Result<String, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let _ = unlocked;
    for url in &relay_urls {
        if !soshal_common_core::url::is_valid_event_relay_url(url) {
            return Err(format!("invalid relay url: {url}")).into();
        }
    }
    let mut builder = EventBuilder::new(Kind::RelayList, "");
    for url in &relay_urls {
        for role in ["read", "write"] {
            if let Ok(tag) =
                nostr::event::Tag::parse(vec!["r".to_string(), url.clone(), role.to_string()])
            {
                builder = builder.tag(tag);
            }
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    publish_event(signed.clone())?;
    let id = serde_json::from_str::<serde_json::Value>(&signed)
        .ok()
        .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    Ok(id)
}

/// Publish a kind-30085 custom profile event for the unlocked signer.
/// `pubkey` must match the unlocked signer. Returns the event id.
#[frb(sync, serialize)]
pub fn identity_publish_custom_profile(
    pubkey: String,
    profile_json: String,
) -> Result<String, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    if unlocked != pubkey {
        return Err("pubkey does not match unlocked signer".to_string()).into();
    }
    let builder = EventBuilder::new(Kind::Custom(30085), profile_json);
    let signed = super::signer::sign_builder(builder)?;
    publish_event(signed.clone())?;
    let id = serde_json::from_str::<serde_json::Value>(&signed)
        .ok()
        .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    Ok(id)
}

/// Get the local blocked list for a user.
#[frb(sync, serialize)]
pub fn identity_get_blocked_users(pubkey: String) -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| BlockRepo::new(db).list(&pubkey))
}

/// Check whether `checker_pubkey` has blocked `target_pubkey`.
#[frb(sync, serialize)]
pub fn identity_is_blocked(checker_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| BlockRepo::new(db).is_blocked(&checker_pubkey, &target_pubkey))
}

/// Build an in-process signer handle from an nsec; returns the derived pubkey.
/// Used for diagnostics only — the app's live signer lives in `signer.rs`.
#[frb(sync, serialize)]
pub fn identity_in_process_signer(nsec: String) -> Result<String, String> {
    use soshal_identity_core::signers::SignerHandle;
    let keys = nostr::key::Keys::parse(&nsec).map_err(|e| format!("invalid nsec: {e}"))?;
    let handle = SignerHandle::in_process(keys);
    Ok(handle.public_key_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nip05_split() {
        assert_eq!(
            split_nip05("bob@example.com"),
            ("bob".to_string(), "example.com".to_string())
        );
    }
}
