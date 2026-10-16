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
}

impl FirstHopFeePolicy {
    /// The policy user nodes apply by default.
    //
    // Zero base fee for the same reasons as the LSP's zero base fees.
    pub const DEFAULT: Self = Self {
        base_fee: Amount::ZERO,
        prop_fee: ppm!(0.5%),
    };

    pub const ZERO: Self = Self {
        base_fee: Amount::ZERO,
        prop_fee: Ppm::ZERO,
    };

    /// The fee charged on a payment of `amount`, exclusive of fees.
    pub fn fee(&self, amount: Amount) -> anyhow::Result<Amount> {
        (self.prop_fee * amount)
            .checked_add(self.base_fee)
            .context("First-hop fee overflowed")
    }
}
