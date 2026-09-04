//! Pure domain logic for Nostr Zaps, LNURL, and NWC (Nostr Wallet Connect).

pub mod lnurl;
pub mod nwc;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NwcConnectionInfo {
    pub wallet_pubkey: String,
    pub relay_url: String,
    pub secret_hex: String,
    pub lud16: Option<String>,
}

/// Manual Debug: never prints the NWC wallet secret (a signing key). A stray
/// `{:?}` in logging/error paths must not leak key material.
impl std::fmt::Debug for NwcConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NwcConnectionInfo")
            .field("wallet_pubkey", &self.wallet_pubkey)
            .field("relay_url", &self.relay_url)
            .field("secret_hex", &"<redacted>")
            .field("lud16", &self.lud16)
            .finish()
    }
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

/// Parse-level BOLT-11 amount cap (msats) — rejects absurd invoices at parse
/// time; the per-payment NWC cap (NWC_MAX_PAY_SATS) stays the caller's check.
const MAX_PARSE_BOLT11_MSATS: u64 = 1_000_000_000_000;

/// True when `bolt11` is a valid bech32 string (checksum + charset verified).
/// Used to reject checksum-forged invoices on untrusted surfaces (zap
/// receipts from relays, NWC-provided invoices). Amount parsing alone is
/// intentionally lenient: it must keep working on partial strings for display.
pub fn bolt11_checksum_valid(bolt11: &str) -> bool {
    if bolt11.is_empty() || bolt11.len() > 4096 {
        return false;
    }
    let lower = bolt11.to_ascii_lowercase();
    bech32::decode(&lower).is_ok()
}

/// Parses msats from a BOLT11 invoice string (extracts amount field).
///
/// Returns 0 when no amount is present, the input is malformed, or too long.
/// Returns an error when the amount overflows or exceeds the zap cap.
pub fn parse_msats_from_bolt11(bolt11: &str) -> Result<u64, String> {
    if bolt11.len() > 4096 || bolt11.is_empty() {
        return Ok(0);
    }

    let lower = bolt11.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("lnbcrt") {
        r
    } else if let Some(r) = lower.strip_prefix("lnbc") {
        r
    } else if let Some(r) = lower.strip_prefix("lntb") {
        r
    } else if let Some(r) = lower.strip_prefix("lnsb") {
        r
    } else {
        return Ok(0);
    };

    if rest.is_empty() {
        return Ok(0);
    }

    let mut amount: u128 = 0;
    let mut digit_len = 0usize;
    for &b in rest.as_bytes() {
        if b.is_ascii_digit() {
            amount = amount
                .checked_mul(10)
                .and_then(|a| a.checked_add((b - b'0') as u128))
                .ok_or_else(|| "BOLT-11 amount overflow".to_string())?;
            digit_len += 1;
            if digit_len > 18 {
                return Err("BOLT-11 amount exceeds range".to_string());
            }
        } else {
            break;
        }
    }
    if digit_len == 0 {
        return Ok(0);
    }

    let unit = rest.as_bytes().get(digit_len).copied();
    let msats = match unit {
        // Pico-BTC is 0.1 msat: only multiples of 10 are representable.
        // Truncating (amount/10) would accept sub-msat invoices as 0 msat
        // free passes — reject the remainder instead.
        Some(b'p') => {
            if !amount.is_multiple_of(10) {
                return Err("BOLT-11 pico amount below msat resolution".to_string());
            }
            Some(amount / 10)
        }
        Some(b'n') => amount.checked_mul(MULT_N as u128),
        Some(b'u') => amount.checked_mul(MULT_U as u128),
        Some(b'm') => amount.checked_mul(MULT_M as u128),
        _ => amount.checked_mul(MULT_DEFAULT as u128),
    }
    .ok_or_else(|| "BOLT-11 amount overflow".to_string())?;

    let msats =
        u64::try_from(msats).map_err(|_| "BOLT-11 amount exceeds u64 msat range".to_string())?;
    if msats > MAX_PARSE_BOLT11_MSATS {
        return Err(format!(
            "BOLT-11 amount exceeds zap cap ({} msat)",
            MAX_PARSE_BOLT11_MSATS
        ));
    }
    Ok(msats)
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
    let sats = parse_msats_from_bolt11(invoice).ok()? / 1000;
    if sats == 0 {
        return None;
    }
    Some(sats)
}

/// Extracts the description hash (BOLT-11 tagged field `h`) from an invoice.
///
/// NIP-57 zap receipts bind their invoice to a zap-request event via this
/// hash: it must equal SHA-256 of the canonical zap-request JSON. Returns
/// `None` when the invoice is malformed, too long, lacks an `h` field, or
/// the field does not decode to exactly 32 bytes.
pub fn bolt11_description_hash(bolt11: &str) -> Option<[u8; 32]> {
    let normalized = bolt11.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized.len() > 4096 || !normalized.contains('1') {
        return None;
    }
    let (_hrp, data) = bech32::decode(&normalized).ok()?;
    // data[0] = version (5 bits); data[1..8] = timestamp (35 bits).
    let mut i = 1 + 7;
    while i + 3 <= data.len() {
        let field_type = u16::from(data[i]);
        let field_len = usize::from(data[i + 1]) * 32 + usize::from(data[i + 2]);
        i += 3;
        let end = i.checked_add(field_len)?;
        if end > data.len() {
            return None;
        }
        if field_type == 23 {
            // Tagged field `h`: 52 5-bit chars = 260 bits; top 256 = hash.
            let mut bits: u32 = 0;
            let mut bit_len: u32 = 0;
            let mut out = [0u8; 32];
            let mut out_pos = 0usize;
            for &w in &data[i..end] {
                bits = (bits << 5) | u32::from(w);
                bit_len += 5;
                while bit_len >= 8 && out_pos < 32 {
                    bit_len -= 8;
                    out[out_pos] = (bits >> bit_len) as u8;
                    out_pos += 1;
                }
            }
            return (out_pos >= 32).then_some(out);
        }
        i = end;
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use bech32::Hrp;

    #[test]
    fn description_hash_roundtrip() {
        let hash = [0x11u8; 32];
        let mut words = Vec::new();
        let mut bits: u64 = 0;
        let mut bit_len = 0u32;
        for &b in &hash {
            bits = (bits << 8) | u64::from(b);
            bit_len += 8;
            while bit_len >= 5 {
                bit_len -= 5;
                words.push(((bits >> bit_len) & 0x1f) as u8);
            }
        }
        if bit_len > 0 {
            words.push(((bits << (5 - bit_len)) & 0x1f) as u8);
        }
        assert_eq!(words.len(), 52);
        let mut data = vec![0u8]; // version 0
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]); // 35-bit timestamp
        data.push(23); // 1 base32 word: field type 23 ('h')
        data.extend_from_slice(&[1, 20]); // 2 base32 words: field length 52 (1 * 32 + 20)
        data.extend_from_slice(&words);
        let invoice =
            bech32::encode::<bech32::Bech32>(Hrp::parse("lnbc1u").unwrap(), &data).unwrap();
        assert_eq!(bolt11_description_hash(&invoice), Some(hash));
    }

    #[test]
    fn description_hash_absent_or_garbage() {
        assert_eq!(bolt11_description_hash(""), None);
        assert_eq!(bolt11_description_hash("lnbc1u1notvalid@@@"), None);
        assert_eq!(bolt11_description_hash("plaintext"), None);
        assert_eq!(bolt11_description_hash(&"x".repeat(4097)), None);
    }
}
