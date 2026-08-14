//! Integration tests for soshal-zap-core: lud16/LNURL parsing, NWC URI
//! validation, BOLT-11 amount extraction, spend caps and NIP-47 requests.

use soshal_nostr_core::nostr;
#[allow(deprecated)]
use soshal_zap_core::lnurl::{host_of, parse_lud16_url, parse_lud16_url_secure};
use soshal_zap_core::nwc::{
    get_balance_request, make_invoice_request, parse_nwc_uri, pay_invoice_request,
};
use soshal_zap_core::{
    apply_daily_spend, bolt11_amount_sats, parse_msats_from_bolt11, NWC_DAILY_MAX_MSATS,
};

#[test]
fn lud16_secure_accepts_valid() {
    let (user, domain, url) = parse_lud16_url_secure("alice@example.com").unwrap();
    assert_eq!(user, "alice");
    assert_eq!(domain, "example.com");
    assert_eq!(url, "https://example.com/.well-known/lnurlp/alice");
    let (u2, _, _) = parse_lud16_url_secure("bob-1_x.y@sub.domain.org").unwrap();
    assert_eq!(u2, "bob-1_x.y");
}

#[test]
fn lud16_secure_rejects_path_traversal() {
    assert!(parse_lud16_url_secure("../admin@domain.com").is_err());
    assert!(parse_lud16_url_secure("a/b@domain.com").is_err());
    assert!(parse_lud16_url_secure("a?b@domain.com").is_err());
    assert!(parse_lud16_url_secure("a#b@domain.com").is_err());
    assert!(parse_lud16_url_secure("sp ace@domain.com").is_err());
}

#[test]
fn lud16_secure_rejects_structural_badness() {
    assert!(parse_lud16_url_secure("").is_err());
    assert!(parse_lud16_url_secure("no-at-sign").is_err());
    assert!(parse_lud16_url_secure("@domain.com").is_err());
    assert!(parse_lud16_url_secure("user@").is_err());
    assert!(parse_lud16_url_secure("user@domain.com@evil.org").is_err());
    let long_user = "u".repeat(65);
    assert!(parse_lud16_url_secure(&format!("{long_user}@domain.com")).is_err());
}

#[test]
#[allow(deprecated)]
fn lud16_legacy_does_not_validate_allowlist() {
    let (user, _, url) = parse_lud16_url("../admin@domain.com").unwrap();
    assert_eq!(user, "../admin");
    assert_eq!(url, "https://domain.com/.well-known/lnurlp/../admin");
    assert!(parse_lud16_url("").is_err());
}

#[test]
fn host_of_extracts_host() {
    assert_eq!(host_of("https://example.com/path").unwrap(), "example.com");
    assert_eq!(host_of("wss://relay.nostr.com").unwrap(), "relay.nostr.com");
    assert!(host_of("not a url").is_err());
    assert!(host_of("https://").is_err());
}

#[test]
fn nwc_uri_parses_valid() {
    let info = parse_nwc_uri(
        "nostr+walletconnect://\
         0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef?\
         relay=wss://relay.getalby.com&secret=\
         abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd",
    )
    .unwrap();
    assert_eq!(info.wallet_pubkey.len(), 64);
    assert_eq!(info.relay_url, "wss://relay.getalby.com");
    assert_eq!(info.secret_hex.len(), 64);
    assert!(info.lud16.is_none());
}

#[test]
fn nwc_uri_with_lud16() {
    let info = parse_nwc_uri(&format!(
        "nostr+walletconnect://{}?relay=wss://relay.example.com&secret={}&lud16=me@example.com",
        "a".repeat(64),
        "b".repeat(64)
    ))
    .unwrap();
    assert_eq!(info.lud16.as_deref(), Some("me@example.com"));
}

#[test]
fn nwc_uri_rejects_bad_inputs() {
    let pubkey = "a".repeat(64);
    let secret = "b".repeat(64);
    let good =
        format!("nostr+walletconnect://{pubkey}?relay=wss://relay.example.com&secret={secret}");
    assert!(parse_nwc_uri("").is_err());
    assert!(parse_nwc_uri(&format!("{good}&secret=1234567890123456789012345678901")).is_err());
    assert!(parse_nwc_uri("http://not-walletconnect").is_err());
    assert!(parse_nwc_uri(&good.replace(&pubkey, "short")).is_err());
    assert!(parse_nwc_uri(&good.replace(&pubkey, &"z".repeat(64))).is_err());
    assert!(parse_nwc_uri(&good.replace("wss://", "ws://")).is_err());
    assert!(parse_nwc_uri(&good.replace("relay.example.com", "127.0.0.1")).is_err());
    assert!(parse_nwc_uri(&good.replace("relay.example.com", "localhost")).is_err());
    assert!(parse_nwc_uri(&good.replace("&secret=", "&")).is_err());
    assert!(parse_nwc_uri(&good.replace("relay=wss://", "relay=")).is_err());
}

#[test]
fn bolt11_msat_parsing_units() {
    assert_eq!(parse_msats_from_bolt11("lnbc10n"), 1000);
    assert_eq!(parse_msats_from_bolt11("LNBC123m"), 123 * 100_000_000);
    assert_eq!(parse_msats_from_bolt11("lnbc1u"), 100_000);
    assert_eq!(parse_msats_from_bolt11("lnbc5p"), 0);
    assert_eq!(parse_msats_from_bolt11("lnbc1"), 100_000_000_000);
    assert_eq!(bolt11_amount_sats("lnbc10n"), Some(1));
}

#[test]
fn bolt11_rejects_malformed() {
    assert_eq!(parse_msats_from_bolt11(""), 0);
    assert_eq!(parse_msats_from_bolt11("no prefix"), 0);
    assert_eq!(parse_msats_from_bolt11("lnbc"), 0);
    assert_eq!(parse_msats_from_bolt11("lnbcabc"), 0);
    assert_eq!(parse_msats_from_bolt11(&"x".repeat(5000)), 0);
    assert_eq!(
        parse_msats_from_bolt11(&format!("lnbc{}", "9".repeat(20))),
        0
    );
    assert_eq!(bolt11_amount_sats("lnbc1p"), None);
}

#[test]
fn bolt11_overflow_safe() {
    assert_eq!(parse_msats_from_bolt11("lnbc999999999999999999m"), 0);
    assert_eq!(
        parse_msats_from_bolt11(&format!("lnbc{}m", "1")),
        100_000_000
    );
}

#[test]
fn daily_spend_cap_enforced() {
    assert_eq!(apply_daily_spend(0, 100).unwrap(), 100_000);
    assert_eq!(
        apply_daily_spend(NWC_DAILY_MAX_MSATS - 1000, 1).unwrap(),
        NWC_DAILY_MAX_MSATS
    );
    assert!(apply_daily_spend(NWC_DAILY_MAX_MSATS, 1).is_err());
    assert!(apply_daily_spend(NWC_DAILY_MAX_MSATS, 0).is_ok());
    assert_eq!(
        apply_daily_spend(u64::MAX, u64::MAX).unwrap_err(),
        "daily NWC spend cap (10000 sats) reached"
    );
}

#[test]
fn make_invoice_requests() {
    assert!(make_invoice_request(0, "desc".into()).is_err());
    assert!(make_invoice_request(-5, "desc".into()).is_err());
    assert!(make_invoice_request(21_000_000_001, "desc".into()).is_err());
    assert!(make_invoice_request(1000, "desc".into()).is_ok());
    let method = |r: nostr::nips::nip47::Request| r.method;
    use nostr::nips::nip47::Method as M;
    assert_eq!(method(pay_invoice_request("lnbc10n".into())), M::PayInvoice);
    assert_eq!(method(get_balance_request()), M::GetBalance);
}

#[test]
fn nwc_connection_info_derives() {
    use soshal_zap_core::NwcConnectionInfo;
    let info = NwcConnectionInfo {
        wallet_pubkey: "pk1".into(),
        relay_url: "wss://relay".into(),
        secret_hex: "secret".into(),
        lud16: Some("user@domain".into()),
    };
    assert_eq!(info.clone(), info);
    let json_str = serde_json::to_string(&info).unwrap();
    let deserialized: NwcConnectionInfo = serde_json::from_str(&json_str).unwrap();
    assert_eq!(info, deserialized);
}
