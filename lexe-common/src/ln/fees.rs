//! Fee schedules.

use anyhow::Context;

use crate::{ln::amount::Amount, ppm, ppm::Ppm};

/// A policy that determines the extra fee a user node pays its first hop
/// (the LSP) on an outbound payment, on top of the route's fees.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FirstHopFeePolicy {
    /// Fixed fee charged per payment.
    pub base_fee: Amount,
    /// Proportional fee charged on the payment amount.
    pub prop_fee: Ppm,
    /// Minimum fee charged per payment, applied when the base and
    /// proportional fees would total less.
    pub min_fee: Amount,
}

impl FirstHopFeePolicy {
    /// The policy user nodes apply by default.
    //
    // Zero base fee for the same reasons as the LSP's zero base fees.
    pub const DEFAULT: Self = Self {
        base_fee: Amount::ZERO,
        prop_fee: ppm!(0.5%),
        min_fee: Amount::from_msat(100),
    };

    pub const ZERO: Self = Self {
        base_fee: Amount::ZERO,
        prop_fee: Ppm::ZERO,
        min_fee: Amount::ZERO,
    };

    /// The fee charged on a payment of `amount`, exclusive of fees.
    pub fn fee(&self, amount: Amount) -> anyhow::Result<Amount> {
        let fee = (self.prop_fee * amount)
            .checked_add(self.base_fee)
            .context("First-hop fee overflowed")?;
        Ok(fee.max(self.min_fee))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{msat, sat};

    #[test]
    fn fee() {
        let policy = FirstHopFeePolicy::DEFAULT;

        // Below the floor
        assert_eq!(policy.fee(msat!(100)).unwrap(), msat!(100));
        // At the crossover
        assert_eq!(policy.fee(sat!(20)).unwrap(), msat!(100));
        // Above the floor
        assert_eq!(policy.fee(sat!(1000)).unwrap(), sat!(5));

        assert_eq!(
            FirstHopFeePolicy::ZERO.fee(sat!(1000)).unwrap(),
            Amount::ZERO
        );
    }
}
