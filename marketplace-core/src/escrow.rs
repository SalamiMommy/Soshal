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

#[cfg(test)]
mod tests {
    use super::*;

    struct Escrow {
        buyer_confirmed: bool,
        seller_confirmed: bool,
        arbitrator_approved: bool,
        dispute_active: bool,
        released: bool,
    }

    impl Escrow {
        fn new() -> Self {
            Escrow {
                buyer_confirmed: false,
                seller_confirmed: false,
                arbitrator_approved: false,
                dispute_active: false,
                released: false,
            }
        }

        fn confirm_buyer(&mut self) {
            self.buyer_confirmed = true;
        }

        fn confirm_seller(&mut self) {
            self.seller_confirmed = true;
        }

        fn open_dispute(&mut self) {
            self.dispute_active = true;
        }

        fn approve_arbitrator(&mut self) {
            self.arbitrator_approved = true;
        }

        fn release(&mut self) -> bool {
            if self.released
                || !can_release(
                    self.buyer_confirmed,
                    self.seller_confirmed,
                    self.arbitrator_approved,
                    self.dispute_active,
                )
            {
                return false;
            }
            self.released = true;
            true
        }
    }

    #[test]
    fn escrow_create_confirm_release_flow() {
        let mut escrow = Escrow::new();
        assert!(!escrow.release());
        escrow.confirm_buyer();
        assert!(!escrow.release());
        escrow.confirm_seller();
        assert!(escrow.release());
    }

    #[test]
    fn escrow_release_requires_buyer_confirm() {
        let mut escrow = Escrow::new();
        escrow.confirm_seller();
        assert!(!escrow.release());
        escrow.confirm_buyer();
        assert!(escrow.release());
    }

    #[test]
    fn escrow_dispute_path_requires_arbitrator() {
        let mut escrow = Escrow::new();
        escrow.confirm_buyer();
        escrow.confirm_seller();
        escrow.open_dispute();
        assert!(!escrow.release());
        escrow.approve_arbitrator();
        assert!(escrow.release());
    }

    #[test]
    fn escrow_single_party_cannot_release() {
        let mut by_buyer = Escrow::new();
        by_buyer.confirm_buyer();
        assert!(!by_buyer.release());
        let mut by_seller = Escrow::new();
        by_seller.confirm_seller();
        assert!(!by_seller.release());
    }

    #[test]
    fn escrow_double_release_rejected() {
        let mut escrow = Escrow::new();
        escrow.confirm_buyer();
        escrow.confirm_seller();
        assert!(escrow.release());
        assert!(!escrow.release());
        assert!(can_release(true, true, false, false));
    }

    #[test]
    fn escrow_status_variants_distinct() {
        assert_ne!(EscrowStatus::Pending, EscrowStatus::Funded);
        assert_ne!(EscrowStatus::Funded, EscrowStatus::Disputed);
        assert_ne!(EscrowStatus::Disputed, EscrowStatus::Released);
        assert_ne!(EscrowStatus::Released, EscrowStatus::Refunded);
    }
}
