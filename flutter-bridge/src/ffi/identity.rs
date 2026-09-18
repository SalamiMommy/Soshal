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
use zeroize::Zeroize;

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

fn row_to_profile(row: &UserRow) -> ProfileInfo {
    let mut p = empty_profile(row.pubkey.clone());
    p.name = row.name.clone().unwrap_or_default();
    p.display_name = row.display_name.clone().unwrap_or_default();
    p.picture = row.picture.clone().unwrap_or_default();
    p.banner = row.banner.clone().unwrap_or_default();
    p.about = row.about.clone().unwrap_or_default();
    p.nip05 = row.nip05.clone().unwrap_or_default();
    p.created_at = row.created_at.max(0) as u64;
    p.followers = row.follower_count.max(0) as i32;
    // NOTE: contact_pubkeys holds the row owner's own contacts, so the
    // "does *me* follow this profile" flag can't be derived from this row —
    // it is computed in `identity_get_profile` from the viewer's contacts.
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
            Some(r) => {
                let mut p = row_to_profile(&r);
                if let (Some(me), true) = (me.as_deref(), me.as_deref() != Some(r.pubkey.as_str()))
                {
                    if let Ok(Some(my_row)) = repo.get_by_pubkey(me) {
                        if let Ok(follows) =
                            serde_json::from_str::<Vec<String>>(&my_row.contact_pubkeys)
                        {
                            p.is_following =
                                follows.iter().any(|f| f.eq_ignore_ascii_case(&r.pubkey));
                        }
                    }
                }
                p
            }
            None => empty_profile(pubkey),
        };
        Ok(p)
    })
    .map(super::util::json_ok)?
}

/// Upsert a fetched kind-0 profile row into the DB.
/// If the profile's pubkey matches the active account (unlocked signer), the
/// caller must be acting as that identity — prevents a compromised Dart layer
/// from injecting forged metadata for the active user.
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
    // If the signer is unlocked and the profile belongs to the active account,
    // enforce identity: a Dart caller must not be able to overwrite the active
    // user's own cached profile with forged data without holding the key.
    if let Ok(active_pk) = super::signer::signer_pubkey() {
        if active_pk.eq_ignore_ascii_case(pubkey) {
            super::signer::require_identity(pubkey)?;
        }
    }
    super::db::with_db_result(|db| {
        let repo = UserRepo::new(db);
        let existing = repo.get_by_pubkey(&pubkey)?;
        let now = soshal_common_core::format::now_secs();
        let row = UserRow {
            pubkey: pubkey.trim().to_ascii_lowercase(),
            npub: soshal_identity_core::keys::npub_encode(pubkey).unwrap_or_default(),
            name: get("name"),
            display_name: get("display_name"),
            about: get("about"),
            picture: get("picture"),
            banner: get("banner"),
            nip05: get("nip05"),
            lud16: None,
            created_at: v
                .get("created_at")
                .and_then(|c| c.as_i64())
                .or_else(|| existing.as_ref().map(|e| e.created_at))
                .unwrap_or(now),
            updated_at: now,
            metadata_json: Some(content.to_string()),
            contact_pubkeys: existing
                .as_ref()
                .map(|e| e.contact_pubkeys.clone())
                .unwrap_or_else(|| String::from("[]")),
            relay_list: existing
                .as_ref()
                .map(|e| e.relay_list.clone())
                .unwrap_or_else(|| String::from("[]")),
            follower_count: existing.as_ref().map(|e| e.follower_count).unwrap_or(0),
        };
        repo.upsert(&row)?;
        Ok(true)
    })
}

/// Search users by name/about (FTS-ish prefix match).
#[frb(sync, serialize)]
pub fn identity_search_users(query: String, limit: i32) -> Result<String, String> {
    let limit = limit.clamp(1, 100) as i64;
    super::db::with_db_result(|db| {
        let rows = UserRepo::new(db).search(&query, limit)?;
        let profiles: Vec<ProfileInfo> = rows.iter().map(|r| row_to_profile(r)).collect();
        Ok(profiles)
    })
    .map(super::util::json_ok)?
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
    if !unlocked.eq_ignore_ascii_case(&pubkey) {
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
    let url = format!(
        "https://{domain}/.well-known/nostr.json?name={}",
        percent_encode_query(&name)
    );
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
        .map_err(super::util::to_err)?;
    let resp = client.get(&url).send().await.map_err(super::util::to_err)?;
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
    while let Some(chunk) = resp.chunk().await.map_err(super::util::to_err)? {
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
    // Resolve to a genuine public key, not just "a 64-char string". The
    // identifier must map to a valid hex pubkey (or npub form) — anything
    // else is not a real NIP-05 registration and fails verification.
    let result = entry.to_lowercase();
    let valid_pubkey = {
        let mut hex_ok = result.len() == 64 && result.chars().all(|c| c.is_ascii_hexdigit());
        if hex_ok {
            hex_ok = nostr::key::PublicKey::from_hex(&result).is_ok();
        }
        hex_ok || result.starts_with("npub1")
    };
    Ok((valid_pubkey, result))
}

fn split_nip05(nip05: &str) -> (String, String) {
    match nip05.rsplit_once('@') {
        Some((name, domain)) => (name.to_string(), domain.to_string()),
        None => (String::new(), nip05.to_string()),
    }
}

/// Percent-encode every byte outside `[A-Za-z0-9.-_]` for use in a query
/// value, so a NIP-05 name cannot inject extra query parameters.
fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Compute the WoT trust score (0..1) of `target` from `viewer`'s graph,
/// using all stored users' contact lists.
#[frb(sync, serialize)]
pub fn identity_get_trust_score(
    source_pubkey: String,
    target_pubkey: String,
) -> Result<f32, String> {
    let source_pubkey = source_pubkey.trim().to_ascii_lowercase();
    let target_pubkey = target_pubkey.trim().to_ascii_lowercase();
    let users = wot_graph_users()?;
    let mut self_contacts = Vec::new();
    let mut target_contacts = Vec::new();
    for u in &users {
        if u.pubkey.eq_ignore_ascii_case(&source_pubkey) {
            self_contacts = u.contacts.clone();
        }
        if u.pubkey.eq_ignore_ascii_case(&target_pubkey) {
            target_contacts = u.contacts.clone();
        }
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

/// WoT contact-graph snapshot, cached per DB path for `WOT_GRAPH_TTL` so
/// repeated trust/WoT calls skip rebuilding the whole graph.
static WOT_GRAPH_CACHE: std::sync::Mutex<Option<WotGraphSnapshot>> = std::sync::Mutex::new(None);
const WOT_GRAPH_TTL: std::time::Duration = std::time::Duration::from_secs(45);

struct WotGraphSnapshot {
    db_path: String,
    fetched_at: std::time::Instant,
    users: Vec<wot::WotUser>,
}

/// Load the WoT contact graph — only `pubkey` + `contact_pubkeys` columns
/// (repos have no list-all; light raw query mirrors db.rs helpers).
pub(crate) fn wot_graph_users() -> Result<Vec<wot::WotUser>, String> {
    let current_db = super::db::db_path()?;
    let mut guard = crate::ffi::util::lock(&WOT_GRAPH_CACHE);
    if let Some(snap) = guard.as_ref() {
        if snap.db_path == current_db && snap.fetched_at.elapsed() < WOT_GRAPH_TTL {
            return Ok(snap.users.clone());
        }
    }
    let users = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users",
            (),
            |r| {
                let pubkey: String = r.get(0)?;
                let contacts_raw: Option<String> = r.get(1).ok();
                let contacts = contacts_raw
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                    .unwrap_or_default();
                Ok(wot::WotUser { pubkey, contacts })
            },
        )
    })?;
    *guard = Some(WotGraphSnapshot {
        db_path: current_db,
        fetched_at: std::time::Instant::now(),
        users,
    });
    guard
        .as_ref()
        .map(|s| s.users.clone())
        .ok_or_else(|| "wot graph cache empty".to_string())
}

/// Resolve an audience vocabulary value into the reachable author set used
/// for content filtering across feature tabs:
/// - `"public"` (default) → `None`, no author filter
/// - `"friends"` / `"friends_only"` → direct follows (WoT distance 1)
/// - `"network"` → friends ∪ friends-of-friends (WoT distance 1 + 2)
/// - an author pubkey / `"author:<pubkey>"` / `"user:<pubkey>"` → that author
///
/// Returns `None` for public (unfiltered); `Some(vec)` for filtered.
/// Empty vector when the active account is absent or has no contacts — the
/// filter then matches nothing. The reachable set is derived from the cached
/// contact graph ([`wot_graph_users`], 45 s TTL) and the WoT distance
/// partition in identity-core.
pub(crate) fn resolve_audience_authors(audience: &str) -> Result<Option<Vec<String>>, String> {
    let trimmed = audience.trim();
    if let Some(author) = trimmed
        .strip_prefix("author:")
        .or_else(|| trimmed.strip_prefix("user:"))
    {
        let author = author.trim();
        return Ok(Some(if author.is_empty() {
            Vec::new()
        } else {
            vec![author.to_string()]
        }));
    }
    if trimmed.len() == 64 && trimmed.as_bytes().iter().all(|b| b.is_ascii_hexdigit()) {
        return Ok(Some(vec![trimmed.to_string()]));
    }
    let level = match trimmed {
        "friends" | "friends_only" | "following" | "follows" => 1u32,
        "network" | "friends_of_friends" | "fof" => 2,
        _ => return Ok(None),
    };
    let self_pubkey = super::db::active_pubkey()?;
    if self_pubkey.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let users = wot_graph_users()?;
    let by_distance = wot::get_wot_peers_by_distance(&self_pubkey, &users, level);
    let mut authors: Vec<String> = Vec::new();
    authors.push(self_pubkey.clone());
    for d in 1..=level {
        if let Some(set) = by_distance.get(&d) {
            authors.extend(set.iter().cloned());
        }
    }
    authors.sort_unstable();
    authors.dedup();
    Ok(Some(authors))
}

/// Drop the cached WoT graph; call when the contact graph mutates
/// (follow/unfollow) so the next call re-reads the DB.
fn wot_graph_invalidate() {
    if let Ok(mut guard) = WOT_GRAPH_CACHE.lock() {
        *guard = None;
    }
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
    let target_pubkey = target_pubkey.trim().to_ascii_lowercase();
    let viewer_pubkey = viewer_pubkey.trim().to_ascii_lowercase();
    let wot_users = wot_graph_users()?;
    let by_distance = wot::get_wot_peers_by_distance(&viewer_pubkey, &wot_users, 2);
    let status = if by_distance
        .get(&1)
        .map(|v| v.iter().any(|pk| pk.eq_ignore_ascii_case(&target_pubkey)))
        .unwrap_or(false)
    {
        "trusted"
    } else if by_distance
        .get(&2)
        .map(|v| v.iter().any(|pk| pk.eq_ignore_ascii_case(&target_pubkey)))
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
        Ok(pk) => pk.trim().to_ascii_lowercase(),
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let pubkey = pubkey.trim().to_ascii_lowercase();
    if pubkey.len() != 64 || hex::decode(&pubkey).is_err() {
        return Err("invalid pubkey: must be 64-character hex".to_string());
    }
    if pubkey.eq_ignore_ascii_case(&unlocked) {
        return Err("cannot follow yourself".to_string());
    }
    let (list, row, was_following) = super::db::with_db_result(|db| {
        let row = UserRepo::new(db).get_by_pubkey(&unlocked)?;
        let mut follows: Vec<String> = match &row {
            Some(r) => serde_json::from_str(&r.contact_pubkeys).unwrap_or_default(),
            None => Vec::new(),
        };
        let was_following = follows.iter().any(|f| f.eq_ignore_ascii_case(&pubkey));
        if !was_following {
            follows.push(pubkey.clone());
        }
        Ok((follows, row, was_following))
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
                follower_count: 0,
            },
        };
        r.contact_pubkeys = updated;
        UserRepo::new(db).upsert(&r)?;
        // Materialized follower count: a new follow adds +1 to the target.
        if !was_following && !pubkey.eq_ignore_ascii_case(&unlocked) {
            UserRepo::new(db).bump_follower_count(&pubkey, 1)?;
        }
        Ok(())
    })?;
    let mut builder = EventBuilder::new(Kind::ContactList, "");
    for f in &list {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), f.clone()]) {
            builder = builder.tag(tag);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    soshal_identity_core::wot::invalidate_wot_peers_cache();
    wot_graph_invalidate();
    publish_event(signed.clone())?;
    Ok(signed)
}

/// Unfollow `pubkey`: rebuild the signer's full NIP-02 contact list minus the
/// target, persist it, sign a kind-3 event and publish. Returns true.
#[frb(sync, serialize)]
pub fn identity_unfollow_user(pubkey: String) -> Result<bool, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk.trim().to_ascii_lowercase(),
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let pubkey = pubkey.trim().to_ascii_lowercase();
    if pubkey.len() != 64 || hex::decode(&pubkey).is_err() {
        return Err("invalid pubkey: must be 64-character hex".to_string());
    }
    let (list, row, was_following) = super::db::with_db_result(|db| {
        let row = UserRepo::new(db).get_by_pubkey(&unlocked)?;
        let mut follows: Vec<String> = match &row {
            Some(r) => serde_json::from_str(&r.contact_pubkeys).unwrap_or_default(),
            None => Vec::new(),
        };
        let was_following = follows.iter().any(|f| f.eq_ignore_ascii_case(&pubkey));
        follows.retain(|f| !f.eq_ignore_ascii_case(&pubkey));
        Ok((follows, row, was_following))
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
                follower_count: 0,
            },
        };
        r.contact_pubkeys = updated;
        UserRepo::new(db).upsert(&r)?;
        // Materialized follower count: an unfollow removes -1 from the target.
        if was_following && !pubkey.eq_ignore_ascii_case(&unlocked) {
            UserRepo::new(db).bump_follower_count(&pubkey, -1)?;
        }
        Ok(())
    })?;
    let mut builder = EventBuilder::new(Kind::ContactList, "");
    for f in &list {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), f.clone()]) {
            builder = builder.tag(tag);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    soshal_identity_core::wot::invalidate_wot_peers_cache();
    wot_graph_invalidate();
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
    soshal_content_core::custom_profile::parse_and_validate(&profile_json)?;
    let builder = EventBuilder::new(Kind::Custom(30085), profile_json);
    let signed = super::signer::sign_builder(builder)?;
    publish_event(signed.clone())?;
    let id = serde_json::from_str::<serde_json::Value>(&signed)
        .ok()
        .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    Ok(id)
}

/// Publish a NIP-09 kind-5 deletion tombstone for the active account's
/// identity events (kind 0 metadata, kind 3 contacts, kind 10002 relay
/// list). Best-effort: queues to the outbox when offline.
#[frb(serialize)]
pub async fn identity_delete_profile(pubkey: String) -> Result<String, String> {
    let unlocked = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    if unlocked != pubkey {
        return Err("pubkey does not match unlocked signer".to_string()).into();
    }
    let mut builder = EventBuilder::new(Kind::EventDeletion, "profile deleted by user");
    for kind in ["0", "3", "10002"] {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["a".to_string(), format!("{kind}:{pubkey}")])
        {
            builder = builder.tag(tag);
        }
    }
    let signed_json = super::signer::sign_builder(builder)?;
    super::sync::publish_or_enqueue("delete_profile", &signed_json).await?;
    Ok(signed_json).into()
}

/// Get the local blocked list for a user.
#[frb(sync, serialize)]
pub fn identity_get_blocked_users(pubkey: String) -> Result<Vec<String>, String> {
    super::signer::require_identity(&pubkey)?;
    super::db::with_db_result(|db| BlockRepo::new(db).list(&pubkey))
}

/// Check whether `checker_pubkey` has blocked `target_pubkey`.
#[frb(sync, serialize)]
pub fn identity_is_blocked(checker_pubkey: String, target_pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| BlockRepo::new(db).is_blocked(&checker_pubkey, &target_pubkey))
}

/// Build an in-process signer handle from an nsec; returns the derived pubkey.
/// Used for diagnostics only — the app's live signer lives in `signer.rs`.
///
/// The nsec crosses the FFI boundary as a plain `String`, so this surface must
/// never be called in production code. In release builds the function zeroizes
/// the nsec parameter and immediately returns `Err`; the call is a no-op from a
/// key-exposure perspective. The runtime guard is in addition to any build-time
/// gating done by the Dart caller (e.g. `kDebugMode`).
#[frb(sync, serialize)]
pub fn identity_in_process_signer(mut nsec: String) -> Result<String, String> {
    if !cfg!(debug_assertions) {
        // Zeroize key material before the early return so it does not linger
        // on the heap after the nsec String is dropped.
        nsec.zeroize();
        return Err("signer probe disabled in release builds".to_string());
    }
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

    #[test]
    fn test_row_to_profile_follow_state() {
        let pk = "a".repeat(64);
        let row = UserRow {
            pubkey: pk,
            npub: "npub1abc".into(),
            name: Some("Alice".into()),
            display_name: None,
            about: None,
            picture: None,
            banner: None,
            nip05: None,
            lud16: None,
            created_at: 10,
            updated_at: 10,
            metadata_json: None,
            contact_pubkeys: serde_json::json!(["b".repeat(64), "c".repeat(64)]).to_string(),
            relay_list: "[]".into(),
            follower_count: 0,
        };
        let p = row_to_profile(&row);
        assert_eq!(p.following, 2);
        assert!(!p.is_following);
        let bad = UserRow {
            contact_pubkeys: "not-json".into(),
            ..row
        };
        assert_eq!(row_to_profile(&bad).following, 0);
    }

    #[test]
    fn test_get_profile_is_following_from_viewer_contacts() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-follow", "identity");
        let keys = nostr::key::Keys::generate();
        let me = keys.public_key().to_hex();
        let target = "f".repeat(64);
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        // me follows target (my contacts) — but target does NOT follow me.
        crate::ffi::db::db_execute_raw_test(
            format!(
                "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{me}', 'npub1me', '[\"{target}\"]') ON CONFLICT(pubkey) DO UPDATE SET contact_pubkeys='[\"{target}\"]'"
            ),
        )
        .unwrap();
        crate::ffi::db::db_execute_raw_test(
            format!(
                "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{target}', 'npub1tgt', '[]') ON CONFLICT(pubkey) DO UPDATE SET contact_pubkeys='[]'"
            ),
        )
        .unwrap();
        let json = identity_get_profile(target.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["pubkey"], target);
        assert!(v["is_following"].as_bool().unwrap(), "json: {json}");
        // Target's own profile: never "following" even if listed in own contacts.
        let json = identity_get_profile(me).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(!v["is_following"].as_bool().unwrap(), "json: {json}");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_wot_status_warning_distance_two() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-wot", "identity");
        let me = "a".repeat(64);
        let friend = "b".repeat(64);
        let target = "c".repeat(64);
        let stranger = "d".repeat(64);
        for (pk, npub, contacts) in [
            (me.clone(), "npub1me", format!("[\"{friend}\"]")),
            (friend.clone(), "npub1fr", format!("[\"{target}\"]")),
            (target.clone(), "npub1tg", "[]".to_string()),
            (stranger.clone(), "npub1st", "[]".to_string()),
        ] {
            crate::ffi::db::db_execute_raw_test(format!(
                "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{pk}', '{npub}', '{contacts}') ON CONFLICT(pubkey) DO UPDATE SET contact_pubkeys='{contacts}'"
            ))
            .unwrap();
        }
        assert_eq!(
            identity_get_wot_status(target, me.clone()).unwrap(),
            "warning"
        );
        assert_eq!(
            identity_get_wot_status(friend, me.clone()).unwrap(),
            "trusted"
        );
        assert_eq!(identity_get_wot_status(stranger, me).unwrap(), "unknown");
    }

    #[test]
    fn test_store_profile_non_json_content_silent() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-store-bad", "identity");
        let pk = "e".repeat(64);
        assert!(
            identity_store_profile(format!(r#"{{"pubkey":"{pk}","content":"not json"}}"#)).unwrap()
        );
        let v: serde_json::Value =
            serde_json::from_str(&identity_get_profile(pk).unwrap()).unwrap();
        for k in [
            "name",
            "display_name",
            "about",
            "picture",
            "banner",
            "nip05",
        ] {
            assert_eq!(v[k], "", "field {k}");
        }
        let pk2 = "f".repeat(64);
        assert!(identity_store_profile(format!(r#"{{"pubkey":"{pk2}","content":""}}"#)).unwrap());
        let v: serde_json::Value =
            serde_json::from_str(&identity_get_profile(pk2).unwrap()).unwrap();
        assert_eq!(v["name"], "");
    }

    #[test]
    fn test_search_users_limit_clamp() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-search", "identity");
        for i in 0..3 {
            let pk = format!("{:064x}", i + 1);
            let name = format!("alice{i}");
            identity_store_profile(format!(
                r#"{{"pubkey":"{pk}","content":"{{\"name\":\"{name}\"}}"}}"#
            ))
            .unwrap();
        }
        for limit in [0, -5] {
            let out: Vec<serde_json::Value> =
                serde_json::from_str(&identity_search_users("alice".into(), limit).unwrap())
                    .unwrap();
            assert_eq!(out.len(), 1, "limit {limit}");
        }
        let out: Vec<serde_json::Value> =
            serde_json::from_str(&identity_search_users("alice".into(), 1000).unwrap()).unwrap();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_follow_user_idempotent_persist() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-follow-dup", "identity");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        let target = "9".repeat(64);
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{me}', 'npub1me', '[\"{target}\"]') ON CONFLICT(pubkey) DO UPDATE SET contact_pubkeys='[\"{target}\"]'"
        ))
        .unwrap();
        // No relay client in tests: publish fails, but the local follow
        // persists before the publish step — call twice, expect one entry.
        for _ in 0..2 {
            let err = identity_follow_user(target.clone()).unwrap_err();
            assert!(!err.contains("signer locked"), "{err}");
        }
        let follows: Vec<String> =
            serde_json::from_str(&identity_fetch_follows(me).unwrap()).unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0], target);
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_fetch_follows_roundtrip_and_malformed() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-fetch-follows", "identity");
        let pk = "1".repeat(64);
        let x = "2".repeat(64);
        let y = "3".repeat(64);
        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('{pk}', 'npub1x', '[\"{x}\",\"{y}\"]')"
        ))
        .unwrap();
        let follows: Vec<String> =
            serde_json::from_str(&identity_fetch_follows(pk.clone()).unwrap()).unwrap();
        assert_eq!(follows, vec![x, y]);
        crate::ffi::db::db_execute_raw_test(format!(
            "UPDATE users SET contact_pubkeys='not-json' WHERE pubkey='{pk}'"
        ))
        .unwrap();
        assert_eq!(identity_fetch_follows(pk).unwrap(), "[]");
        assert_eq!(identity_fetch_follows("9".repeat(64)).unwrap(), "[]");
    }

    #[test]
    fn test_wot_graph_users_light_load() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-wot-graph", "identity");
        let p1 = "a1".repeat(32);
        let p2 = "b2".repeat(32);
        identity_store_profile(format!(
            r#"{{"pubkey":"{p1}","content":"{{\"name\":\"Ann\"}}"}}"#
        ))
        .unwrap();
        identity_store_profile(format!(
            r#"{{"pubkey":"{p2}","content":"{{\"name\":\"Bob\"}}"}}"#
        ))
        .unwrap();
        let users = wot_graph_users().unwrap();
        assert_eq!(users.len(), 2);
        let mut pubkeys: Vec<String> = users.iter().map(|u| u.pubkey.clone()).collect();
        pubkeys.sort();
        assert_eq!(pubkeys, vec![p1, p2]);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_delete_profile_publishes_kind_five_tombstone() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("identity-delete-profile", "identity");
        let keys = soshal_nostr_core::keys::generate_keys();
        let me = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let locked_err = identity_delete_profile("5".repeat(64)).await.unwrap_err();
        assert!(locked_err.contains("does not match"), "{locked_err}");
        let signed = identity_delete_profile(me.clone()).await.unwrap();
        let event: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(event["kind"].as_i64(), Some(5));
        let addrs: Vec<String> = event["tags"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.as_array())
            .filter(|t| t.first().and_then(|x| x.as_str()) == Some("a"))
            .map(|t| t[1].as_str().unwrap().to_string())
            .collect();
        for expected in ["0", "3", "10002"] {
            let addr = format!("{expected}:{me}");
            assert!(addrs.contains(&addr), "missing a-tag {addr}: {addrs:?}");
        }
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_resolve_audience_authors() {
        let pk = "a1".repeat(32);
        assert_eq!(
            resolve_audience_authors(&pk).unwrap(),
            Some(vec![pk.clone()])
        );
        assert_eq!(
            resolve_audience_authors(&format!("author:{pk}")).unwrap(),
            Some(vec![pk.clone()])
        );
        assert_eq!(
            resolve_audience_authors(&format!("user:{pk}")).unwrap(),
            Some(vec![pk.clone()])
        );
        assert_eq!(resolve_audience_authors("author:").unwrap(), Some(vec![]));
        assert_eq!(resolve_audience_authors("public").unwrap(), None);
        assert_eq!(resolve_audience_authors("").unwrap(), None);
    }
}
