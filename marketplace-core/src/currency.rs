use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// High-precision financial amount with 128-bit decimal representation,
/// eliminating IEEE-754 float rounding inaccuracies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketplaceAmount {
    pub amount: Decimal,
    pub currency: String,
}

impl MarketplaceAmount {
    pub fn new(amount: Decimal, currency: &str) -> Self {
        Self {
            amount,
            currency: currency.trim().to_ascii_uppercase(),
        }
    }
}

/// Calculate marketplace/escrow fee based on fee basis points (1 bp = 0.01% = 0.0001).
/// Rounds according to standard banker's rounding (`MidpointNearestEven`).
pub fn calculate_escrow_fee(amount: Decimal, fee_basis_points: u32) -> Result<Decimal, String> {
    if amount < Decimal::ZERO {
        return Err("Amount cannot be negative".to_string());
    }
    let bps = Decimal::from(fee_basis_points);
    let divisor = Decimal::from(10_000);

    let fee = (amount * bps) / divisor;
    Ok(fee.round_dp(2))
}

/// Convert an amount from one currency to another using an exact exchange rate.
pub fn convert_currency(
    amount: Decimal,
    exchange_rate: Decimal,
    scale: u32,
) -> Result<Decimal, String> {
    if amount < Decimal::ZERO {
        return Err("Amount cannot be negative".to_string());
    }
    if exchange_rate <= Decimal::ZERO {
        return Err("Exchange rate must be strictly positive".to_string());
    }

    let converted = amount * exchange_rate;
    Ok(converted.round_dp(scale))
}

/// Convert Bitcoin satoshis (`1 BTC = 100,000,000 sats`) to fiat at the given BTC fiat price.
pub fn sats_to_fiat(sats: u64, btc_price_fiat: Decimal, scale: u32) -> Result<Decimal, String> {
    if btc_price_fiat <= Decimal::ZERO {
        return Err("BTC price must be strictly positive".to_string());
    }
    let sats_dec = Decimal::from(sats);
    let sats_per_btc = Decimal::from(100_000_000);

    let btc_amount = sats_dec / sats_per_btc;
    let fiat = btc_amount * btc_price_fiat;
    Ok(fiat.round_dp(scale))
}

/// Convert a fiat amount to Bitcoin satoshis at the given BTC fiat price.
pub fn fiat_to_sats(fiat: Decimal, btc_price_fiat: Decimal) -> Result<u64, String> {
    if fiat < Decimal::ZERO {
        return Err("Fiat amount cannot be negative".to_string());
    }
    if btc_price_fiat <= Decimal::ZERO {
        return Err("BTC price must be strictly positive".to_string());
    }

    let sats_per_btc = Decimal::from(100_000_000);
    let sats_dec = (fiat / btc_price_fiat) * sats_per_btc;
    let rounded = sats_dec.round_dp(0);

    rounded
        .to_u64()
        .ok_or_else(|| "Sats value out of u64 range".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_float_rounding_absence() {
        // In IEEE-754: 0.1 + 0.2 != 0.3
        // In Decimal: 0.1 + 0.2 == 0.3 exactly
        let d1 = Decimal::from_str("0.1").unwrap();
        let d2 = Decimal::from_str("0.2").unwrap();
        let expected = Decimal::from_str("0.3").unwrap();
        assert_eq!(d1 + d2, expected);
    }

    #[test]
    fn test_calculate_escrow_fee() {
        // $100.00 with 250 bps (2.5%) fee = $2.50
        let amount = Decimal::from_str("100.00").unwrap();
        let fee = calculate_escrow_fee(amount, 250).unwrap();
        assert_eq!(fee, Decimal::from_str("2.50").unwrap());

        // Negative amount rejected
        let neg = Decimal::from_str("-50.00").unwrap();
        assert!(calculate_escrow_fee(neg, 250).is_err());
    }

    #[test]
    fn test_sats_and_fiat_conversions() {
        // BTC at $100,000.00
        let btc_price = Decimal::from_str("100000.00").unwrap();

        // 100,000 sats (0.001 BTC) at $100,000/BTC = $100.00
        let fiat = sats_to_fiat(100_000, btc_price, 2).unwrap();
        assert_eq!(fiat, Decimal::from_str("100.00").unwrap());

        // Reverse: $100.00 at $100,000/BTC = 100,000 sats
        let sats = fiat_to_sats(fiat, btc_price).unwrap();
        assert_eq!(sats, 100_000);

        // Reject zero or negative rate
        assert!(sats_to_fiat(1000, Decimal::ZERO, 2).is_err());
        assert!(fiat_to_sats(Decimal::ONE, Decimal::ZERO).is_err());
    }

    #[test]
    fn test_currency_conversion() {
        // 50 USD at 0.92 EUR/USD = 46.00 EUR
        let usd = Decimal::from(50);
        let rate = Decimal::from_str("0.92").unwrap();
        let eur = convert_currency(usd, rate, 2).unwrap();
        assert_eq!(eur, Decimal::from_str("46.00").unwrap());
    }
}
