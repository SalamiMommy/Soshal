//! Zap FFI module
//!
//! Lightning Network Zaps (NIP-57), LNURL parsing, NWC (Nostr Wallet
//! Connect) connection state, and DB-backed zap totals. Invoice issuance is
//! delegated to the NWC provider. The NWC connection secret never leaves this
//! module: it is held in-memory in a `ZeroizingString` for the process
//! lifetime only (never written to disk, never exported to Dart) and a fresh
//! NWC connection must be established after an app restart. A future
//! hardening step could persist the secret in the OS keychain (sealed with
//! the device key), but that is deliberately NOT done today to keep the blast
//! radius of a disk theft to zero.

use flutter_rust_bridge::frb;
use nostr::event::{AsyncSignEvent, Event, UnsignedEvent};
use nostr::key::{AsyncGetPublicKey, PublicKey};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

/// nostr trait adapter over the in-process signer module, so NWC
/// request/response signing flows through the same key-handling path —
/// never key bytes in this module.
#[derive(Debug)]
struct BridgeSigner;

impl AsyncGetPublicKey for BridgeSigner {
    type Error = nostr::error::Error;
    fn get_public_key_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PublicKey, Self::Error>> + Send + '_>> {
        Box::pin(async move {
            let hex = super::signer::signer_pubkey().map_err(nostr::error::Error::other)?;
            PublicKey::from_hex(&hex).map_err(nostr::error::Error::other)
        })
    }
}

impl AsyncSignEvent for BridgeSigner {
    type Error = nostr::error::Error;
    fn sign_event_async(
        &self,
        unsigned: UnsignedEvent,
    ) -> Pin<Box<dyn Future<Output = Result<Event, Self::Error>> + Send + '_>> {
        Box::pin(async move {
            let json = serde_json::to_string(&unsigned).map_err(nostr::error::Error::other)?;
            let signed =
                super::signer::signer_sign_unsigned(json).map_err(nostr::error::Error::other)?;
            serde_json::from_str(&signed).map_err(nostr::error::Error::other)
        })
    }
}

/// NWC connection state (in-process only; secret never exported).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NwcConnectionInfo {
    pub wallet_pubkey: String,
    pub relay_url: String,
    pub lud16: Option<String>,
}

impl From<soshal_zap_core::NwcConnectionInfo> for NwcConnectionInfo {
    fn from(info: soshal_zap_core::NwcConnectionInfo) -> Self {
        Self {
            wallet_pubkey: info.wallet_pubkey,
            relay_url: info.relay_url,
            lud16: info.lud16,
        }
    }
}

/// A `String` that zeroizes its heap buffer on drop, preventing the NWC
/// wallet secret from lingering in freed memory.
type ZeroizingString = zeroize::Zeroizing<String>;

static NWC: Mutex<Option<NwcConnectionInfo>> = Mutex::new(None);
static NWC_URI_STATE: Mutex<Option<ZeroizingString>> = Mutex::new(None);

/// Pending payment binding: the exact invoice issued by `zap_fetch_invoice`
/// and the msat amount requested for it. `zap_send_payment` will only pay
/// this invoice — a tampered or substituted bolt11 (wrong amount, wrong
/// recipient) is rejected. Cleared on payment, disconnect, or a fresh
/// invoice fetch.
static PENDING_PAYMENT: Mutex<Option<(String, u64)>> = Mutex::new(None);

fn pending_payment() -> Option<(String, u64)> {
    crate::ffi::util::lock(&PENDING_PAYMENT).clone()
}

fn clear_pending_payment() {
    if let Ok(mut g) = PENDING_PAYMENT.lock() {
        *g = None;
    }
}

/// Shared NWC URI fixture used by unit and integration tests only.
/// Not included in release builds to avoid shipping a wallet secret in
/// the production binary.
///
/// # SECURITY — DO NOT REPLACE THE KEY
///
/// The `secret=f0f0f0…` value is a **synthetic, all-`f0` pattern** that is
/// NOT a real Nostr Wallet Connect credential. It is intentionally trivial
/// so that it cannot accidentally authenticate against any real wallet.
///
/// **Never substitute a real NWC secret here.** Doing so would:
/// 1. Commit a live wallet credential to git history (permanent, public).
/// 2. Embed it in every debug build's symbol table.
/// 3. Expose it to anyone who can read the repository.
///
/// If you need to run integration tests against a real wallet, use a
/// dedicated test-only wallet with no real funds and inject the URI via
/// an environment variable (`TEST_NWC_URI`), never via a source constant.
// nosec: test-only synthetic key — not a real credential.
#[cfg(test)]
pub const NWC_URI: &str = "nostr+walletconnect://abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789?relay=wss://relay.damus.io&secret=f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";

/// Return the stored NWC URI for in-process use.  The URI contains the wallet
/// secret and must never be returned to Dart.
/// Returns a `Zeroizing<String>` so the secret bytes are zeroed as soon as the
/// caller drops the value — no intermediate plain `String` clone lingers on the heap.
fn nwc_uri() -> Result<ZeroizingString, String> {
    let guard = crate::ffi::util::lock(&NWC_URI_STATE);
    match guard.as_ref() {
        Some(z) => Ok(ZeroizingString::new(z.as_str().to_owned())),
        None => Err("NWC not connected".to_string()),
    }
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
    if lnurl.is_empty() || lnurl.len() > 2048 {
        return Err("invalid lnurl length: max 2048 chars".to_string()).into();
    }
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
    clear_pending_payment();
    *crate::ffi::util::lock(&NWC) = Some(info.into());
    *crate::ffi::util::lock(&NWC_URI_STATE) = Some(ZeroizingString::new(nwc_uri));
    Ok(true).into()
}

/// Disconnect from NWC.
#[frb(serialize)]
pub fn zap_disconnect_nwc() -> Result<bool, String> {
    *crate::ffi::util::lock(&NWC) = None;
    *crate::ffi::util::lock(&NWC_URI_STATE) = None;
    clear_pending_payment();
    Ok(true).into()
}

#[derive(Serialize)]
struct NwcStatusConnectedDto<'a> {
    wallet_pubkey: &'a str,
    relay_url: &'a str,
    connected: bool,
}

/// Get NWC connection status (never includes the secret).
#[frb(serialize)]
pub fn zap_get_nwc_status() -> Result<String, String> {
    let guard = crate::ffi::util::lock(&NWC);
    match guard.as_ref() {
        Some(info) => serde_json::to_string(&NwcStatusConnectedDto {
            wallet_pubkey: &info.wallet_pubkey,
            relay_url: &info.relay_url,
            connected: true,
        })
        .map_err(|e| format!("serialize nwc status: {e}"))
        .into(),
        None => Ok("{\"connected\":false}".to_string()).into(),
    }
}

/// Get connected NWC pubkey (for display), or error if disconnected.
#[frb(serialize)]
pub fn zap_get_nwc_pubkey() -> Result<String, String> {
    let guard = crate::ffi::util::lock(&NWC);
    match guard.as_ref() {
        Some(info) if !info.wallet_pubkey.is_empty() => Ok(info.wallet_pubkey.clone()).into(),
        _ => Err("NWC disconnected".to_string()).into(),
    }
}

/// Fetch invoice for a zap via the connected NWC provider. The NIP-47
/// make-invoice request is built and exchanged Rust-side (NIP-44 v2
/// ciphertext, kind 23195 response); returns the serialized response with
/// the bolt11 invoice.
///
/// # Security — NWC URI and Dart FFI
///
/// `zap_connect_nwc` accepts the full NWC URI (which contains the wallet
/// secret) as a plain Dart `String`. Dart's GC heap is not zeroizable, so
/// a brief window exists where the secret resides in Dart memory before
/// crossing FFI. Rust immediately stores it in a `ZeroizingString`.
/// Callers MUST NOT log, persist, or store the URI in Dart state beyond the
/// single `zap_connect_nwc` call. A keychain-backed path is tracked for a
/// future codegen cycle.
///
/// # Amount limits
///
/// `amount_msat` must be a multiple of 1000 (whole satoshis), ≥1000 (≥1 sat),
/// and ≤`MAX_ZAP_MSAT` (1 BTC) to prevent catastrophic accidental payments.
#[frb(serialize)]
pub async fn zap_fetch_invoice(
    lnurl: String,
    amount_msat: u64,
    _comment: String,
    _nostr_event: String,
) -> Result<String, String> {
    /// Hard ceiling: 1 BTC in millisatoshis. Prevents catastrophically large
    /// payments caused by UI bugs or attacker-manipulated input.
    const MAX_ZAP_MSAT: u64 = 100_000_000_000;

    if lnurl.is_empty() || lnurl.len() > 2048 {
        return Err("invalid lnurl length: max 2048 chars".to_string()).into();
    }
    if _comment.len() > 1000 {
        return Err("comment exceeds 1000 chars".to_string()).into();
    }
    if _nostr_event.len() > 64 * 1024 {
        return Err("nostr_event exceeds 64KB".to_string()).into();
    }
    let _lud16 = soshal_zap_core::lnurl::parse_lud16_url_secure(&lnurl)
        .map_err(|e| format!("LNURL parse failed: {e}"))?;
    if amount_msat == 0 {
        return Err("amount must be positive".to_string()).into();
    }
    if amount_msat < 1000 || !amount_msat.is_multiple_of(1000) {
        return Err(
            "amount_msat must be a positive integer multiple of 1000 (whole satoshis)".to_string(),
        );
    }
    if amount_msat > MAX_ZAP_MSAT {
        return Err(format!(
            "amount_msat {amount_msat} exceeds maximum ({MAX_ZAP_MSAT} msat = 1 BTC)"
        ))
        .into();
    }
    // The URI embeds the wallet secret; nwc_uri() returns a ZeroizingString so
    // the secret is zeroed as soon as `uri` is dropped — no extra wrapper needed.
    let uri = nwc_uri()?;
    super::signer::signer_pubkey()?;
    let description = if _comment.trim().is_empty() {
        "zap".to_string()
    } else {
        _comment
    };
    let amount_sats = (amount_msat / 1000) as i64;
    let req = soshal_zap_core::nwc::make_invoice_request(amount_sats, description)
        .map_err(|e| format!("invoice request: {e}"))?;
    let resp = soshal_zap_core::nwc::nwc_send_request(BridgeSigner, &uri, req).await?;
    // Bind the exact invoice + requested amount: `zap_send_payment` will only
    // honor this bolt11, so a provider that returns an invoice for a
    // different amount (or a tampered bolt11 from Dart) cannot be paid.
    let invoice = resp
        .get("result")
        .and_then(|r| r.get("invoice"))
        .and_then(serde_json::Value::as_str)
        .filter(|i| {
            soshal_zap_core::parse_msats_from_bolt11(i).is_ok()
                && soshal_zap_core::bolt11_checksum_valid(i)
        })
        .ok_or("NWC response missing valid bolt11 invoice")?;
    if let Ok(mut g) = PENDING_PAYMENT.lock() {
        *g = Some((invoice.to_string(), amount_msat));
    }
    serde_json::to_string(&resp)
        .map_err(|e| format!("serialize: {e}"))
        .into()
}

/// Pay a BOLT-11 invoice via the connected NWC provider. Only the exact
/// invoice previously issued by `zap_fetch_invoice` (and the amount
/// requested for it) is accepted; any other bolt11 is rejected before any
/// network I/O. Returns the serialized pay_invoice response (preimage).
#[frb(serialize)]
pub async fn zap_send_payment(bolt11: String) -> Result<String, String> {
    let expected =
        pending_payment().ok_or("no invoice pending: fetch one via zap_fetch_invoice first")?;
    super::signer::signer_pubkey()?;
    if expected.0 != bolt11 {
        clear_pending_payment();
        return Err("invoice mismatch: bolt11 does not match the fetched invoice".to_string())
            .into();
    }
    let msats = soshal_zap_core::parse_msats_from_bolt11(&bolt11)
        .map_err(|e| format!("invalid bolt11 invoice: {e}"))?;
    if msats != expected.1 {
        clear_pending_payment();
        return Err("invoice amount mismatch".to_string()).into();
    }
    let uri = nwc_uri()?;
    let resp = soshal_zap_core::nwc::pay_invoice(BridgeSigner, &uri, bolt11).await?;
    clear_pending_payment();
    Ok(resp).into()
}

/// Get total zap amounts for an event from the local DB (invoice amounts
/// only — never the `amount` tag of an unverified receipt).
#[frb(serialize)]
pub fn zap_get_total_msat(event_id: String) -> Result<u64, String> {
    let event_id = event_id.trim();
    if event_id.is_empty() || event_id.len() > 128 {
        return Ok(0);
    }
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let total: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COALESCE(SUM(amount_msat), 0) FROM zaps WHERE LOWER(event_id) = LOWER(?1)",
            libsql::params![event_id],
            |r| r.get(0),
        )?
        .unwrap_or(0);
        Ok(total.max(0) as u64)
    })
}

/// Batch zap totals for many event ids: one query, one FFI roundtrip.
#[frb(sync, serialize)]
pub fn zap_fetch_totals(event_ids: Vec<String>) -> Result<String, String> {
    if event_ids.len() > 500 {
        return Err("too many event_ids: max 500".to_string()).into();
    }
    let ids_json = serde_json::to_string(&event_ids).map_err(|e| format!("serialize: {e}"))?;
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let out = soshal_db_core::block_on(async {
            let stmt = conn
                .prepare(
                    "SELECT event_id, SUM(amount_msat) FROM zaps WHERE LOWER(event_id) IN (SELECT LOWER(value) FROM json_each(?1)) GROUP BY LOWER(event_id)",
                )
                .await?;
            let mut rows = stmt.query(libsql::params![ids_json.as_str()]).await?;
            let mut map: std::collections::HashMap<String, i64> =
                std::collections::HashMap::with_capacity(event_ids.len());
            for id in &event_ids {
                map.insert(id.clone(), 0);
            }
            while let Some(row) = rows.next().await? {
                let eid: String = row.get(0)?;
                let sum: i64 = row.get(1)?;
                for id in &event_ids {
                    if id.eq_ignore_ascii_case(&eid) {
                        map.insert(id.clone(), sum);
                    }
                }
            }
            Ok::<_, libsql::Error>(map)
        })
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string()))
    })
}

#[derive(Serialize)]
struct ZapReceiptDto {
    id: String,
    event_id: Option<String>,
    recipient_pubkey: String,
    sender_pubkey: Option<String>,
    amount_msat: i64,
    bolt11: Option<String>,
    preimage: Option<String>,
    comment: Option<String>,
    created_at: i64,
    pubkey: Option<String>,
    amount: i64,
    content: Option<String>,
    zap_type: String,
}

/// Fetch stored zap receipt rows for an event as raw JSON.
#[frb(serialize)]
pub fn zap_fetch_receipts(event_id: String, limit: i32) -> Result<String, String> {
    let event_id = event_id.trim();
    if event_id.is_empty() || event_id.len() > 128 {
        return Ok("[]".to_string()).into();
    }
    if limit <= 0 || limit > 500 {
        return Err("limit must be 1..=500".to_string()).into();
    }
    let json = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let out = soshal_db_core::block_on(async {
            let stmt = conn
                .prepare(
                    "SELECT id, event_id, recipient_pubkey, sender_pubkey, amount_msat, bolt11, preimage, comment, created_at, pubkey, amount, content, zap_type FROM zaps WHERE LOWER(event_id) = LOWER(?1) ORDER BY created_at DESC LIMIT ?2",
                )
                .await?;
            let mut rows = stmt
                .query(libsql::params![event_id, limit as i64])
                .await?;
            let mut out = Vec::with_capacity((limit as usize).min(64));
            while let Some(row) = rows.next().await? {
                out.push(ZapReceiptDto {
                    id: row.get::<String>(0)?,
                    event_id: row.get::<Option<String>>(1)?,
                    recipient_pubkey: row.get::<String>(2)?,
                    sender_pubkey: row.get::<Option<String>>(3)?,
                    amount_msat: row.get::<i64>(4)?,
                    bolt11: row.get::<Option<String>>(5)?,
                    preimage: row.get::<Option<String>>(6)?,
                    comment: row.get::<Option<String>>(7)?,
                    created_at: row.get::<i64>(8)?,
                    pubkey: row.get::<Option<String>>(9)?,
                    amount: row.get::<i64>(10)?,
                    content: row.get::<Option<String>>(11)?,
                    zap_type: row.get::<String>(12)?,
                });
            }
            Ok::<_, libsql::Error>(out)
        })
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
    })?;
    Ok(json).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    static NWC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(test)]
    fn set_pending_payment_for_test(bolt11: &str, msat: u64) {
        if let Ok(mut g) = PENDING_PAYMENT.lock() {
            *g = Some((bolt11.to_string(), msat));
        }
    }

    const NWC_PUBKEY: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    const NWC_SECRET: &str = "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";

    #[test]
    fn test_parse_lnurl_metadata_valid() {
        let json = zap_parse_lnurl_metadata("alice@example.com".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "alice");
        assert_eq!(v["domain"], "example.com");
        assert_eq!(
            v["callback"],
            "https://example.com/.well-known/lnurlp/alice"
        );
        let json = zap_parse_lnurl_metadata("bob-1_x.y@sub.example.org".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "bob-1_x.y");
        assert_eq!(v["domain"], "sub.example.org");
        assert!(v["callback"].as_str().unwrap().starts_with("https://"));
    }

    #[test]
    fn test_parse_lnurl_metadata_rejects_malformed() {
        for bad in [
            String::new(),
            "not-an-address".to_string(),
            "@example.com".to_string(),
            "user@".to_string(),
            "../admin@example.com".to_string(),
            "us er@example.com".to_string(),
            format!("{}@example.com", "a".repeat(65)),
        ] {
            assert!(
                zap_parse_lnurl_metadata(bad.clone()).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn test_connect_nwc_roundtrip_status_and_pubkey() {
        let _g = crate::ffi::util::lock(&NWC_TEST_LOCK);
        let _ = zap_disconnect_nwc();
        assert!(zap_connect_nwc(NWC_URI.to_string()).unwrap());
        let status = zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], true);
        assert_eq!(v["wallet_pubkey"], NWC_PUBKEY);
        assert!(!status.contains(NWC_SECRET));
        assert_eq!(zap_get_nwc_pubkey().unwrap(), NWC_PUBKEY);
        assert!(zap_disconnect_nwc().unwrap());
        let status = zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], false);
        assert!(zap_get_nwc_pubkey().is_err());
    }

    #[test]
    fn test_disconnect_nwc_when_not_connected() {
        let _g = crate::ffi::util::lock(&NWC_TEST_LOCK);
        let _ = zap_disconnect_nwc();
        assert!(zap_disconnect_nwc().unwrap());
        assert!(zap_get_nwc_pubkey().is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_fetch_invoice_validation_before_connect() {
        let _g = crate::ffi::util::lock(&NWC_TEST_LOCK);
        let _ = zap_disconnect_nwc();
        let bad_lnurl = zap_fetch_invoice(
            "not-an-address".to_string(),
            1000,
            String::new(),
            String::new(),
        )
        .await;
        let e = bad_lnurl.err().unwrap();
        assert!(e.contains("LNURL parse failed"));
        let zero = zap_fetch_invoice(
            "bob@example.com".to_string(),
            0,
            String::new(),
            String::new(),
        )
        .await;
        let e = zero.err().unwrap();
        assert!(e.contains("amount must be positive"));
        let disconnected = zap_fetch_invoice(
            "bob@example.com".to_string(),
            1000,
            String::new(),
            String::new(),
        )
        .await;
        let e = disconnected.err().unwrap();
        assert!(e.contains("NWC not connected"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_send_payment_fails_when_disconnected() {
        let _g = crate::ffi::util::lock(&NWC_TEST_LOCK);
        let _ = zap_disconnect_nwc();
        // No invoice fetched → pending binding absent → rejected before NWC.
        let r = zap_send_payment("lnbc1fake".to_string()).await;
        let e = r.err().unwrap();
        assert!(e.contains("no invoice pending"));
    }

    #[test]
    fn test_fetch_totals_batch_db() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("totals", "zap");
        db::db_execute_raw_test(
            "INSERT INTO zaps (id, event_id, recipient_pubkey, amount, amount_msat, created_at, zap_type) \
             VALUES ('z1','ev1','pk',5,5000,1000,'public'),('z2','ev1','pk',7,7000,2000,'public'),('z3','ev2','pk',3,3000,1500,'public')"
                .to_string(),
        )
        .unwrap();
        let json = zap_fetch_totals(vec![
            "ev1".to_string(),
            "ev2".to_string(),
            "ev3".to_string(),
        ])
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ev1"], 12000);
        assert_eq!(v["ev2"], 3000);
        assert_eq!(v["ev3"], 0);
    }

    #[test]
    fn test_connect_nwc_uri_length_and_malformed() {
        // Local validation only — no network involved.
        let long = format!("nostr+walletconnect://{}", "a".repeat(4096));
        let e = zap_connect_nwc(long).err().unwrap();
        assert_eq!(e, "NWC URI too long");
        let e = zap_connect_nwc("garbage".to_string()).err().unwrap();
        assert!(e.starts_with("invalid NWC URI: "), "err: {e}");
    }

    #[test]
    fn test_get_total_msat_seeded_and_empty() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("total-msat", "zap");
        db::db_execute_raw_test(
            "INSERT INTO zaps (id, event_id, recipient_pubkey, amount, amount_msat, created_at, zap_type) \
             VALUES ('z1','ev1','pk',5,5000,1000,'public'),('z2','ev1','pk',7,7000,2000,'public'),('z3','ev2','pk',3,3000,1500,'public')"
                .to_string(),
        )
        .unwrap();
        // Sum uses the `amount_msat` column (msat), so the total is msat.
        assert_eq!(zap_get_total_msat("ev1".to_string()).unwrap(), 12000);
        assert_eq!(zap_get_total_msat("ev2".to_string()).unwrap(), 3000);
        assert_eq!(zap_get_total_msat("ev9".to_string()).unwrap(), 0);
    }

    #[test]
    fn test_fetch_receipts_validation_and_rows() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("receipts", "zap");
        let e = zap_fetch_receipts("ev1".to_string(), 0).err().unwrap();
        assert_eq!(e, "limit must be 1..=500");
        let e = zap_fetch_receipts("ev1".to_string(), 501).err().unwrap();
        assert_eq!(e, "limit must be 1..=500");
        // Empty DB → empty rows, not an error.
        assert_eq!(zap_fetch_receipts("ev1".to_string(), 1).unwrap(), "[]");
        db::db_execute_raw_test(
            "INSERT INTO zaps (id, event_id, recipient_pubkey, amount, amount_msat, created_at, zap_type) \
             VALUES ('z1','ev1','pk',5,5000,1000,'public'),('z2','ev1','pk',7,7000,2000,'public')"
                .to_string(),
        )
        .unwrap();
        let json = zap_fetch_receipts("ev1".to_string(), 2).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert!(json.contains("z1"), "json: {json}");
        assert!(json.contains("\"amount_msat\":5000"), "json: {json}");
        let one = zap_fetch_receipts("ev1".to_string(), 1).unwrap();
        assert!(one.len() < json.len(), "limit ignored: {one}");
    }

    #[test]
    fn test_connect_nwc_clears_stale_pending_payment() {
        let _g = crate::ffi::util::lock(&NWC_TEST_LOCK);
        let _ = zap_disconnect_nwc();
        assert!(zap_connect_nwc(NWC_URI.to_string()).unwrap());
        set_pending_payment_for_test("lnbc1stale", 1000);
        assert!(pending_payment().is_some());
        // Reconnect with same URI should clear the stale pending payment.
        assert!(zap_connect_nwc(NWC_URI.to_string()).unwrap());
        assert!(pending_payment().is_none());
        let _ = zap_disconnect_nwc();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_fetch_invoice_amount_upper_bound() {
        // M6: amounts above 1 BTC (100_000_000_000 msat) must be rejected
        // before any network call is made — no NWC connection needed.
        // Must be a multiple of 1000 (whole satoshis) so the lower guard passes
        // and the upper-bound guard is reached.
        let over_limit = 100_001_000_000u64; // 100_001 BTC worth, multiple of 1000
        let err = zap_fetch_invoice(
            "user@example.com".to_string(),
            over_limit,
            "".to_string(),
            "".to_string(),
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("exceeds maximum"),
            "expected upper-bound error, got: {err}"
        );

        // u64::MAX rounded down to nearest 1000 also exceeds the cap.
        let big_round = (u64::MAX / 1000) * 1000; // largest multiple of 1000
        let err = zap_fetch_invoice(
            "user@example.com".to_string(),
            big_round,
            "".to_string(),
            "".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("exceeds maximum"), "got: {err}");
    }

    #[test]
    fn test_zap_case_insensitivity_and_limits() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("zap_case_limits", "zap");
        db::db_execute_raw_test(
            "INSERT INTO zaps (id, event_id, recipient_pubkey, amount, amount_msat, created_at, zap_type) \
             VALUES ('z1','ev_lower','pk',10,10000,1000,'public')"
                .to_string(),
        )
        .unwrap();

        // Upper case query matches lower case row
        assert_eq!(zap_get_total_msat("EV_LOWER".to_string()).unwrap(), 10000);

        let totals_json = zap_fetch_totals(vec!["EV_LOWER".to_string()]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&totals_json).unwrap();
        assert_eq!(v["EV_LOWER"], 10000);

        let receipts_json = zap_fetch_receipts("EV_LOWER".to_string(), 10).unwrap();
        assert!(receipts_json.contains("z1"));

        // Oversized input limits
        assert!(zap_parse_lnurl_metadata("x".repeat(2049)).is_err());
        let too_many_ids = (0..501).map(|i| format!("ev{i}")).collect();
        assert!(zap_fetch_totals(too_many_ids).is_err());
    }
}
