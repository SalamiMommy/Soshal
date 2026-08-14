//! NWC (Nostr Wallet Connect) connection URI parsing, request building, and
//! request/response exchange over nostr relays.

use soshal_nostr_core::nostr;
use soshal_nostr_core::nostr_sdk;

use super::NwcConnectionInfo;

/// Parses a NWC connection URI (`nostr+walletconnect://...`).
///
/// Enforces the full guard set: scheme, hex pubkey, `wss://` only, SSRF
/// allowlist via `is_valid_relay_url`, size caps on every input field.
pub fn parse_nwc_uri(uri: &str) -> Result<NwcConnectionInfo, String> {
    if uri.is_empty() || uri.len() > 4096 {
        return Err("NWC URI too long".into());
    }
    let parsed = url::Url::parse(uri).map_err(|e| format!("invalid URI: {}", e))?;
    if parsed.scheme() != "nostr+walletconnect" {
        return Err("invalid scheme".into());
    }
    let wallet_pubkey = parsed.host_str().ok_or("missing pubkey")?.to_string();
    if wallet_pubkey.len() != 64 || !wallet_pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid pubkey in NWC URI".into());
    }
    let mut relay_url = String::new();
    let mut secret_hex = String::new();
    let mut lud16 = None;

    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "relay" => relay_url = v.to_string(),
            "secret" => secret_hex = v.to_string(),
            "lud16" => lud16 = Some(v.to_string()),
            _ => {}
        }
    }
    if relay_url.is_empty() {
        return Err("missing relay in NWC URI".into());
    }
    if relay_url.len() > 512 {
        return Err("NWC relay too long".into());
    }
    // NWC traffic carries the wallet secret + invoices: no cleartext ws://,
    // and no loopback/private/rebinding hosts (SSRF guard on the relay).
    if !relay_url.starts_with("wss://") {
        return Err("NWC relay must be wss://".into());
    }
    if !soshal_common_core::url::is_valid_relay_url(&relay_url).0 {
        return Err("invalid NWC relay".into());
    }
    if secret_hex.is_empty() || secret_hex.len() < 32 || secret_hex.len() > 128 {
        return Err("invalid secret in NWC URI".into());
    }
    Ok(NwcConnectionInfo {
        wallet_pubkey,
        relay_url,
        secret_hex,
        lud16,
    })
}

/// Builds a NIP-47 make-invoice request. Validates the amount range.
pub fn make_invoice_request(
    amount_sats: i64,
    description: String,
) -> Result<nostr::nips::nip47::Request, String> {
    if amount_sats <= 0 || amount_sats > 21_000_000_000 {
        return Err("amount out of range".into());
    }
    Ok(nostr::nips::nip47::Request::make_invoice(
        nostr::nips::nip47::MakeInvoiceRequest {
            amount: (amount_sats as u64).saturating_mul(1000),
            description: Some(description),
            description_hash: None,
            expiry: None,
        },
    ))
}

/// Builds a NIP-47 pay-invoice request.
pub fn pay_invoice_request(invoice: String) -> nostr::nips::nip47::Request {
    nostr::nips::nip47::Request::pay_invoice(nostr::nips::nip47::PayInvoiceRequest::new(invoice))
}

/// Builds a NIP-47 get-balance request.
pub fn get_balance_request() -> nostr::nips::nip47::Request {
    nostr::nips::nip47::Request::get_balance()
}

/// Validates a BOLT-11 invoice for payment: non-empty, length-capped, and
/// within the per-payment sat cap. Runs before the invoice ever leaves the
/// process.
pub fn validate_pay_invoice(invoice: &str) -> Result<(), String> {
    if invoice.is_empty() || invoice.len() > 4096 {
        return Err("invalid bolt11 invoice".into());
    }
    if let Some(sats) = super::bolt11_amount_sats(invoice) {
        if sats > super::NWC_MAX_PAY_SATS {
            return Err(format!(
                "payment exceeds {}-sat NWC cap",
                super::NWC_MAX_PAY_SATS
            ));
        }
    }
    Ok(())
}

/// Pays a BOLT-11 invoice via the connected NWC provider: builds the NIP-47
/// pay-invoice request, exchanges it over the wallet relay (NIP-44 v2
/// ciphertext, kind 23195 response), and returns the serialized response
/// (preimage confirmation). The wss:// + SSRF guard re-runs inside
/// `nwc_send_request`.
pub async fn pay_invoice<
    S: nostr::key::AsyncGetPublicKey + nostr::event::AsyncSignEvent + Send + Sync + 'static,
>(
    signer: S,
    uri_str: &str,
    invoice: String,
) -> Result<String, String> {
    validate_pay_invoice(&invoice)?;
    let resp = nwc_send_request(signer, uri_str, pay_invoice_request(invoice)).await?;
    serde_json::to_string(&resp).map_err(|e| format!("serialize: {e}"))
}

/// Sends a NIP-47 request to the wallet's relay and waits for the encrypted
/// response. Signature-verifies every returned event before parsing.
///
/// Takes a `NostrSigner` (never raw key material) so the key agent can sit
/// between the command layer and the wallet relay.
pub async fn nwc_send_request<
    S: nostr::key::AsyncGetPublicKey + nostr::event::AsyncSignEvent + Send + Sync + 'static,
>(
    signer: S,
    uri_str: &str,
    request: nostr::nips::nip47::Request,
) -> Result<serde_json::Value, String> {
    // Re-validate the URI at connect time (not just at seal time): wss://
    // only + SSRF allowlist, so a tampered relay can never receive the
    // wallet secret or invoice over cleartext or from an internal host.
    parse_nwc_uri(uri_str)?;
    let uri = nostr::nips::nip47::NostrWalletConnectUri::parse(uri_str)
        .map_err(|e| format!("nwc uri: {}", e))?;
    let event = request
        .to_event(&uri, nostr::nips::nip47::Nip47Ciphers::NIP44V2)
        .map_err(|e| format!("nwc request: {}", e))?;
    let relay = uri.relays.first().cloned().ok_or("nwc: no relay")?;
    let client = nostr_sdk::client::Client::builder()
        .authenticator(nostr_sdk::authenticator::SignerAuthenticator::new(signer))
        .build();
    client
        .add_relay(relay.clone())
        .await
        .map_err(|e| format!("nwc relay: {}", e))?;
    client.connect().await;
    client
        .send_event(&event)
        .await
        .map_err(|e| format!("nwc send: {}", e))?;
    let my_pk = event.pubkey.to_string();
    let wallet_pk = uri.public_key.to_string();
    let resp_filter = nostr::filter::Filter::new()
        .kind(nostr::event::Kind::from_u16(23195))
        .author(
            nostr::key::PublicKey::from_hex(&wallet_pk).map_err(|e| format!("wallet pk: {}", e))?,
        )
        .pubkey(
            nostr::key::PublicKey::from_hex(&my_pk)
                .map_err(|e| format!("invalid pubkey: {}", e))?,
        )
        .limit(10);
    let events = client
        .fetch_events(resp_filter)
        .timeout(std::time::Duration::from_secs(15))
        .await
        .map_err(|e| format!("nwc fetch: {}", e))?;
    let ev = events
        .into_iter()
        .find(|e| e.verify().is_ok())
        .ok_or("nwc: no response within 15s")?;
    let response = nostr::nips::nip47::Response::from_event(
        &uri,
        &ev,
        nostr::nips::nip47::Nip47Ciphers::NIP44V2,
    )
    .map_err(|e| format!("nwc response: {}", e))?;
    serde_json::to_value(&response).map_err(|e| format!("serialize: {}", e))
}
