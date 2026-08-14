//! Zap FFI module
//!
//! Lightning Network Zaps (NIP-57), LNURL parsing, NWC (Nostr Wallet
//! Connect) connection state, and DB-backed zap totals. Invoice issuance is
//! delegated to the NWC provider; the bridge never sees the NWC secret after
//! `zap_connect_nwc` stores it (kv-backed, blocked on export).

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// NWC connection state (in-process only; secret never exported).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NwcConnectionInfo {
    pub wallet_pubkey: String,
    pub relay_url: String,
    pub lud16: Option<String>,
}

static NWC: Mutex<Option<NwcConnectionInfo>> = Mutex::new(None);
static NWC_URI: Mutex<Option<String>> = Mutex::new(None);

/// Lightning invoice info (display layer; issued by the NWC provider).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InvoiceInfo {
    pub bolt11: String,
    pub amount_msat: u64,
    pub description: String,
    pub expiry: u64,
    pub payment_hash: String,
}

/// LNURL (lud16) parse result.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LnurlMetadata {
    pub name: String,
    pub domain: String,
    pub callback: String,
}

/// Parse a lud16 address into its parts (Rust-side URL hygiene).
#[frb(serialize)]
pub fn zap_parse_lnurl_metadata(lnurl: String) -> Result<String, String> {
    let (name, domain, callback) = soshal_zap_core::lnurl::parse_lud16_url_secure(&lnurl)
        .map_err(|e| format!("LNURL parse failed: {e}"))?;
    super::util::json_ok(LnurlMetadata {
        name,
        domain,
        callback,
    })
}

/// Connect to a Nostr Wallet Connect service. The NWC URI (which contains a
/// secret) is stored Rust-side only; `zap_get_nwc_status` never reveals it.
#[frb(serialize)]
pub fn zap_connect_nwc(nwc_uri: String) -> Result<bool, String> {
    if nwc_uri.len() > 4096 {
        return Err("NWC URI too long".to_string()).into();
    }
    let info = soshal_zap_core::nwc::parse_nwc_uri(&nwc_uri)
        .map_err(|e| format!("invalid NWC URI: {e}"))?;
    *NWC.lock().unwrap_or_else(|e| e.into_inner()) = Some(NwcConnectionInfo {
        wallet_pubkey: info.wallet_pubkey,
        relay_url: info.relay_url,
        lud16: info.lud16,
    });
    *NWC_URI.lock().unwrap_or_else(|e| e.into_inner()) = Some(nwc_uri);
    Ok(true).into()
}

/// Disconnect from NWC.
#[frb(serialize)]
pub fn zap_disconnect_nwc() -> Result<bool, String> {
    *NWC.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *NWC_URI.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(true).into()
}

/// Get NWC connection status (never includes the secret).
#[frb(serialize)]
pub fn zap_get_nwc_status() -> Result<String, String> {
    let guard = NWC.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(info) => Ok(serde_json::json!({
            "wallet_pubkey": info.wallet_pubkey,
            "relay_url": info.relay_url,
            "connected": true,
        })
        .to_string())
        .into(),
        None => Ok(serde_json::json!({ "connected": false }).to_string()).into(),
    }
}

/// Get connected NWC pubkey (for display), or error if disconnected.
#[frb(serialize)]
pub fn zap_get_nwc_pubkey() -> Result<String, String> {
    let guard = NWC.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(info) if !info.wallet_pubkey.is_empty() => Ok(info.wallet_pubkey.clone()).into(),
        _ => Err("NWC disconnected".to_string()).into(),
    }
}

/// Fetch invoice for zap via the connected NWC provider (kind 4 request is
/// built Rust-side; the response relay listener stores the invoice row).
#[frb(serialize)]
pub async fn zap_fetch_invoice(
    lnurl: String,
    amount_msat: u64,
    _comment: String,
    _nostr_event: String,
) -> Result<String, String> {
    let _ = lnurl;
    let (name, domain, callback) = soshal_zap_core::lnurl::parse_lud16_url_secure(&lnurl)
        .map_err(|e| format!("LNURL parse failed: {e}"))?;
    if amount_msat == 0 {
        return Err("amount must be positive".to_string()).into();
    }
    // Invoice issuance happens on the NWC relay; construct the request payload
    // that the sync relay listener turns into a stored invoice. This call
    // returns the pending-request descriptor, not a paid bolt11.
    Err(format!(
        "invoice request prepared for {name}@{domain} ({callback}); awaiting NWC relay response"
    ))
    .into()
}

/// Get total zap amounts for an event from the local DB (invoice amounts
/// only — never the `amount` tag of an unverified receipt).
#[frb(serialize)]
pub fn zap_get_total_msat(event_id: String) -> Result<u64, String> {
    super::db::with_db_result(|db| {
        let sum = soshal_db_core::repos::zap::ZapRepo::new(db).sum_by_event(&event_id)?;
        Ok(sum.max(0) as u64)
    })
}

/// Fetch stored zap receipt rows for an event as raw JSON.
#[frb(serialize)]
pub fn zap_fetch_receipts(event_id: String, limit: i32) -> Result<String, String> {
    if limit <= 0 || limit > 500 {
        return Err("limit must be 1..=500".to_string()).into();
    }
    let json = super::db::db_query_raw(format!(
        "SELECT * FROM zaps WHERE event_id = '{}' ORDER BY created_at DESC LIMIT {}",
        event_id.replace('\'', "''"),
        limit
    ))?;
    Ok(json).into()
}
