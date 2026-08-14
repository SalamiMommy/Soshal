//! Pure domain logic for Nostr Zaps, LNURL, and NWC (Nostr Wallet Connect).

pub mod lnurl;
pub mod nwc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NwcConnectionInfo {
    pub wallet_pubkey: String,
    pub relay_url: String,
    pub secret_hex: String,
    pub lud16: Option<String>,
}

/// Parses an NWC connection URI (nostr+walletconnect://...).
pub fn parse_nwc_uri(uri: &str) -> Result<NwcConnectionInfo, String> {
    nwc::parse_nwc_uri(uri)
}

const MULT_M: u64 = 100_000_000;
const MULT_U: u64 = 100_000;
const MULT_N: u64 = 100;
/// BOLT-11 amounts without a multiplier letter are denominated in BTC
/// (1 BTC = 10^11 msat). Parsing them at 10^3 instead understated the amount
/// by 10^8, letting NWC/LNURL cap checks pass while the real invoice is huge.
const MULT_DEFAULT: u64 = 100_000_000_000;

/// Parses msats from a BOLT11 invoice string (extracts amount field).
///
/// Returns 0 when no amount is present, the input is malformed, too long, or
/// the multiplication overflows.
pub fn parse_msats_from_bolt11(bolt11: &str) -> u64 {
    if bolt11.len() > 4096 {
        return 0;
    }

    let bytes = bolt11.as_bytes();
    let pos = match bytes
        .windows(4)
        .position(|w| w.eq_ignore_ascii_case(b"lnbc"))
    {
        Some(p) => p + 4,
        None => return 0,
    };

    let rest = &bytes[pos..];
    let mut amount: u64 = 0;
    let mut digit_len = 0usize;
    for &b in rest.iter().take(18) {
        if b.is_ascii_digit() {
            amount = match amount
                .checked_mul(10)
                .and_then(|a| a.checked_add((b - b'0') as u64))
            {
                Some(n) => n,
                None => return 0,
            };
            digit_len += 1;
        } else {
            break;
        }
    }
    if digit_len == 0 {
        return 0;
    }

    let unit = rest.get(digit_len).copied().map(|b| b.to_ascii_lowercase());
    match unit {
        Some(b'p') => amount / 10,
        Some(b'n') => amount.checked_mul(MULT_N).unwrap_or(0),
        Some(b'u') => amount.checked_mul(MULT_U).unwrap_or(0),
        Some(b'm') => amount.checked_mul(MULT_M).unwrap_or(0),
        _ => amount.checked_mul(MULT_DEFAULT).unwrap_or(0),
    }
}

/// Per-payment cap for NWC payments (sats).
pub const NWC_MAX_PAY_SATS: u64 = 1_000_000;
/// Daily NWC spend cap (msats).
pub const NWC_DAILY_MAX_MSATS: u64 = 10_000_000;
/// Payment-confirmation token validity window (seconds).
pub const NWC_TOKEN_TTL_SECS: u64 = 300;

/// Extracts the amount (sats) from a `lnbc` bolt11 invoice. Returns `None`
/// when the amount is missing, unparseable, or rounds down to zero sats.
pub fn bolt11_amount_sats(invoice: &str) -> Option<u64> {
    let sats = parse_msats_from_bolt11(invoice) / 1000;
    if sats == 0 {
        return None;
    }
    Some(sats)
}

/// Applies a new payment against the day's spend counter, enforcing the daily
/// cap. Returns the new cumulative total on success.
pub fn apply_daily_spend(spent_msats: u64, amount_sats: u64) -> Result<u64, String> {
    let new_total = spent_msats.saturating_add(amount_sats.saturating_mul(1000));
    if new_total > NWC_DAILY_MAX_MSATS {
        return Err(format!(
            "daily NWC spend cap ({} sats) reached",
            NWC_DAILY_MAX_MSATS / 1000
        ));
    }
    Ok(new_total)
}
