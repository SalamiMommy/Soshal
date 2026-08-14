#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum EscrowStatus {
    Pending,
    Funded,
    Disputed,
    Released,
    Refunded,
}

/// Escrow release policy: without a dispute, BOTH buyer and seller must
/// confirm. Either side alone cannot release (or steal) escrowed funds. With
/// a dispute, only the arbitrator's approval releases the funds.
pub fn can_release(
    buyer_confirmed: bool,
    seller_confirmed: bool,
    arbitrator_approved: bool,
    dispute_active: bool,
) -> bool {
    if dispute_active {
        return arbitrator_approved;
    }
    buyer_confirmed && seller_confirmed
}
