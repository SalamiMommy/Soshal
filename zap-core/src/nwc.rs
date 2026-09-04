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

    // NWC traffic carries the wallet secret + invoices: no cleartext ws://,
    // and no loopback/private/rebinding hosts (SSRF guard on the relay).
    // Every `relay=` param is validated — the nostr crate uses the first
    // one, so an unvalidated later param must never win.
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "relay" => {
                let r = v.to_string();
                if r.len() > 512 {
                    return Err("NWC relay too long".into());
                }
                if !r.starts_with("wss://") {
                    return Err("NWC relay must be wss://".into());
                }
                if !soshal_common_core::url::is_valid_relay_url(&r).0 {
                    return Err("invalid NWC relay".into());
                }
                relay_url = r;
            }
            "secret" => secret_hex = v.to_string(),
            "lud16" => lud16 = Some(v.to_string()),
            _ => {}
        }
    }
    if relay_url.is_empty() {
        return Err("missing relay in NWC URI".into());
    }
    if secret_hex.is_empty()
        || secret_hex.len() < 32
        || secret_hex.len() > 128
        || !secret_hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
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
    let msats = super::parse_msats_from_bolt11(invoice)
        .map_err(|_| "invalid bolt11 invoice".to_string())?;
    if msats > super::NWC_MAX_PAY_SATS * 1000 {
        return Err(format!(
            "payment exceeds {}-sat NWC cap",
            super::NWC_MAX_PAY_SATS
        ));
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
    let req_method = request.method.clone();
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
    let response = events
        .into_iter()
        .filter(|e| e.pubkey.to_string() == wallet_pk && e.verify().is_ok())
        .find_map(|ev| {
            nostr::nips::nip47::Response::from_event(
                &uri,
                &ev,
                nostr::nips::nip47::Nip47Ciphers::NIP44V2,
            )
            .ok()
            .filter(|r| r.result_type == req_method)
        })
        .ok_or("nwc: no response within 15s")?;
    serde_json::to_value(&response).map_err(|e| format!("serialize: {}", e))
}

#[cfg(test)]
mod tests {
    use super::{
        get_balance_request, make_invoice_request, nwc_send_request, parse_nwc_uri, pay_invoice,
        pay_invoice_request, validate_pay_invoice, NwcConnectionInfo,
    };
    use crate::NWC_MAX_PAY_SATS;
    use nostr::nips::nip47::{ErrorCode, Method, RequestParams, Response, ResponseResult};
    use soshal_nostr_core::nostr;

    fn valid_uri() -> String {
        format!(
            "nostr+walletconnect://{}?relay=wss://relay.example.com&secret={}",
            "a".repeat(64),
            "b".repeat(64)
        )
    }

    #[test]
    fn uri_parses_valid() {
        let info = parse_nwc_uri(&valid_uri()).unwrap();
        assert_eq!(info.wallet_pubkey, "a".repeat(64));
        assert_eq!(info.relay_url, "wss://relay.example.com");
        assert_eq!(info.secret_hex, "b".repeat(64));
        assert!(info.lud16.is_none());
    }

    #[test]
    fn uri_parses_with_lud16() {
        let info = parse_nwc_uri(&format!("{}&lud16=me@example.com", valid_uri())).unwrap();
        assert_eq!(info.lud16.as_deref(), Some("me@example.com"));
    }

    #[test]
    fn uri_round_trips_through_nostr_crate() {
        let parsed = nostr::nips::nip47::NostrWalletConnectUri::parse(valid_uri()).unwrap();
        assert_eq!(parsed.public_key.to_string(), "a".repeat(64));
        assert_eq!(parsed.secret.to_secret_hex(), "b".repeat(64));
        let re = nostr::nips::nip47::NostrWalletConnectUri::parse(parsed.to_string()).unwrap();
        assert_eq!(re.public_key, parsed.public_key);
        assert_eq!(re.secret, parsed.secret);
        assert_eq!(re.relays, parsed.relays);
    }

    #[test]
    fn uri_rejects_invalid() {
        let good = valid_uri();
        let pubkey = "a".repeat(64);
        let secret = "b".repeat(64);
        assert!(parse_nwc_uri("").is_err());
        assert!(parse_nwc_uri(&"x".repeat(4097)).is_err());
        assert!(parse_nwc_uri("http://example.com").is_err());
        assert!(parse_nwc_uri(&good.replace(&pubkey, "short")).is_err());
        assert!(parse_nwc_uri(&good.replace(&pubkey, &"z".repeat(64))).is_err());
        assert!(parse_nwc_uri(&good.replace("wss://", "ws://")).is_err());
        assert!(parse_nwc_uri(&good.replace("relay=wss://", "relay=")).is_err());
        assert!(parse_nwc_uri(&good.replace("relay.example.com", "127.0.0.1")).is_err());
        assert!(parse_nwc_uri(&good.replace("relay.example.com", "localhost")).is_err());
        assert!(parse_nwc_uri(&good.replace("&secret=", "&")).is_err());
        assert!(parse_nwc_uri(&good.replace(&secret, &"c".repeat(31))).is_err());
        assert!(parse_nwc_uri(&good.replace(&secret, &"c".repeat(129))).is_err());
        let oversize = format!(
            "nostr+walletconnect://{}?relay=wss://relay.example.com&secret={}",
            "a".repeat(4000),
            secret
        );
        assert!(parse_nwc_uri(&oversize).is_err());
    }

    #[test]
    fn uri_secret_length_boundaries() {
        let base = format!(
            "nostr+walletconnect://{}?relay=wss://relay.example.com&secret=",
            "a".repeat(64)
        );
        assert!(parse_nwc_uri(&format!("{base}{}", "c".repeat(32))).is_ok());
        assert!(parse_nwc_uri(&format!("{base}{}", "c".repeat(128))).is_ok());
        assert!(parse_nwc_uri(&format!("{base}{}", "c".repeat(31))).is_err());
        assert!(parse_nwc_uri(&format!("{base}{}", "c".repeat(129))).is_err());
    }

    #[test]
    fn response_envelope_parses() {
        let resp =
            Response::from_json(r#"{"result_type":"get_balance","result":{"balance":250000}}"#)
                .unwrap();
        assert_eq!(resp.result_type, Method::GetBalance);
        assert!(resp.error.is_none());
        assert_eq!(resp.to_get_balance().unwrap().balance, 250000);
    }

    #[test]
    fn error_response_envelope_handled() {
        let resp = Response::from_json(
            r#"{"result_type":"pay_invoice","error":{"code":"INSUFFICIENT_BALANCE","message":"wallet empty"}}"#,
        )
        .unwrap();
        assert_eq!(resp.result_type, Method::PayInvoice);
        assert!(resp.result.is_none());
        let err = resp.error.clone().unwrap();
        assert_eq!(err.code, ErrorCode::InsufficientBalance);
        assert_eq!(err.message, "wallet empty");
        assert!(resp.to_pay_invoice().is_err());
        assert!(Response::from_json(r#"{"result_type":"bogus_method","result":{}}"#).is_err());
    }

    #[test]
    fn bolt11_invoice_amount_is_authoritative() {
        let resp = Response::from_json(
            r#"{"result_type":"make_invoice","result":{"invoice":"lnbc10n","amount":987654321}}"#,
        )
        .unwrap();
        let result = match &resp.result {
            Some(ResponseResult::MakeInvoice(r)) => r,
            _ => panic!("expected make_invoice result"),
        };
        assert_eq!(result.amount, Some(987654321));
        assert_eq!(crate::bolt11_amount_sats(&result.invoice), Some(1));
    }

    #[test]
    fn validate_pay_invoice_uses_bolt11_amount_only() {
        assert!(validate_pay_invoice("lnbc10n").is_ok());
        assert!(validate_pay_invoice("lnbc10m").is_ok());
        assert_eq!(
            validate_pay_invoice("lnbc20m").unwrap_err(),
            format!("payment exceeds {}-sat NWC cap", NWC_MAX_PAY_SATS)
        );
        assert!(validate_pay_invoice("lnbc1").is_err());
        assert!(validate_pay_invoice("lnbc10p").is_ok());
        assert!(validate_pay_invoice("").is_err());
        assert!(validate_pay_invoice(&"x".repeat(5000)).is_err());
    }

    #[test]
    fn make_invoice_request_uses_msat_and_validates_range() {
        assert!(make_invoice_request(0, "d".into()).is_err());
        assert!(make_invoice_request(-1, "d".into()).is_err());
        assert!(make_invoice_request(21_000_000_001, "d".into()).is_err());
        let req = make_invoice_request(21_000_000_000, "d".into()).unwrap();
        assert_eq!(req.method, Method::MakeInvoice);
        match &req.params {
            RequestParams::MakeInvoice(p) => assert_eq!(p.amount, 21_000_000_000_000),
            _ => panic!("expected make_invoice params"),
        }
    }

    #[test]
    fn pay_invoice_request_carries_invoice() {
        let req = pay_invoice_request("lnbc10n".into());
        assert_eq!(req.method, Method::PayInvoice);
        match req.params {
            RequestParams::PayInvoice(p) => assert_eq!(p.invoice, "lnbc10n"),
            _ => panic!("expected pay_invoice params"),
        }
    }

    #[test]
    fn get_balance_request_method() {
        assert_eq!(get_balance_request().method, Method::GetBalance);
    }

    #[test]
    fn connection_info_derives() {
        let info = NwcConnectionInfo {
            wallet_pubkey: "pk1".into(),
            relay_url: "wss://relay".into(),
            secret_hex: "secret".into(),
            lud16: Some("user@domain".into()),
        };
        assert_eq!(info.clone(), info);
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            serde_json::from_str::<NwcConnectionInfo>(&json).unwrap(),
            info
        );
    }

    #[test]
    fn uri_rejects_oversized_relay() {
        let secret = "b".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=wss://{}&secret={}",
            "a".repeat(64),
            "r".repeat(513),
            secret
        );
        let err = parse_nwc_uri(&uri).unwrap_err();
        assert!(err.contains("NWC relay too long"), "got {err}");
    }

    #[tokio::test]
    async fn pay_invoice_rejects_empty_invoice_without_network() {
        let signer = nostr::key::Keys::generate();
        let err = pay_invoice(signer, &valid_uri(), String::new())
            .await
            .unwrap_err();
        assert!(err.contains("invalid bolt11 invoice"), "got {err}");
    }

    #[tokio::test]
    async fn nwc_send_request_rejects_missing_relay_without_network() {
        let signer = nostr::key::Keys::generate();
        let req = make_invoice_request(1000, "d".into()).unwrap();
        let uri = format!(
            "nostr+walletconnect://{}?secret={}",
            "a".repeat(64),
            "b".repeat(64)
        );
        let err = nwc_send_request(signer, &uri, req).await.unwrap_err();
        assert!(err.contains("missing relay"), "got {err}");
    }
}
