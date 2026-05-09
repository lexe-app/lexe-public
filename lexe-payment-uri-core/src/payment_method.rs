use anyhow::anyhow;
use bitcoin::address::{NetworkUnchecked, NetworkValidation};
use lexe_api_core::types::{
    invoice::Invoice, lnurl::LnurlPayRequest, offer::Offer,
};
use lexe_common::ln::{amount::Amount, network::Network};
#[cfg(test)]
use lexe_common::{ln::amount, test_utils::arbitrary};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;

use crate::{email_like::EmailLikeAddress, lnurl::Lnurl};

/// A single "payment method" -- each kind here should correspond with a single
/// linear payment flow for a user, where there are no other alternate methods.
///
/// For example, a Unified BTC QR code contains a single BIP321 URI,
/// which may contain _multiple_ discrete payment methods (an onchain address,
/// a BOLT11 invoice, a BOLT12 offer).
#[allow(clippy::large_enum_variant)]
pub enum PaymentMethod {
    Onchain(Onchain),
    Invoice(Invoice),
    Offer(OfferWithAmount),
    LnurlPayRequest(LnurlPayRequest),
}

/// "Almost" a payment method: a piece of payment data that requires further
/// resolution before it becomes a [`PaymentMethod`].
///
/// Produced by `flatten()` on the various URI types, then consumed by the
/// async resolver in the `lexe-payment-uri` crate.
#[derive(Debug)]
pub enum Resolvable {
    /// A Lightning Address or BIP353 address.
    EmailLike(EmailLikeAddress<'static>),
    /// An LNURL-pay endpoint.
    Lnurl(Lnurl<'static>),
}

impl PaymentMethod {
    pub fn is_onchain(&self) -> bool {
        matches!(self, Self::Onchain(_))
    }

    pub fn is_invoice(&self) -> bool {
        matches!(self, Self::Invoice(_))
    }

    pub fn is_offer(&self) -> bool {
        matches!(self, Self::Offer(_))
    }

    pub fn kind(&self) -> &'static str {
        match self {
            PaymentMethod::Onchain(_) => "onchain",
            PaymentMethod::Invoice(_) => "invoice",
            PaymentMethod::Offer(_) => "offer",
            PaymentMethod::LnurlPayRequest(_) => "lnurl",
        }
    }

    pub fn supports_network(&self, network: Network) -> bool {
        match self {
            Self::Onchain(o) => o.supports_network(network),
            Self::Invoice(i) => i.supports_network(network),
            Self::Offer(o) => o.supports_network(network),
            Self::LnurlPayRequest(_) => true,
        }
    }

    /// For use with `sort_by_key`, `max_by_key`, etc.
    /// Payment methods with a higher priority should be preferred over others.
    pub fn priority(&self) -> usize {
        match self {
            PaymentMethod::Invoice(_) => 40,
            PaymentMethod::Offer(_) => 30,
            PaymentMethod::LnurlPayRequest(_) => 20,
            PaymentMethod::Onchain(o) => 10 + o.relative_priority(),
        }
    }
}

/// An onchain payment method, usually parsed from a standalone BTC address or
/// BIP321 URI.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Onchain {
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_mainnet_addr_unchecked()")
    )]
    pub address: bitcoin::Address<NetworkUnchecked>,

    #[cfg_attr(
        test,
        proptest(strategy = "amount::arb::sats_amount().prop_map(Some)")
    )]
    pub amount: Option<Amount>,

    /// The recipient/payee name.
    pub label: Option<String>,

    /// The payment description.
    pub message: Option<String>,
}

impl Onchain {
    #[inline]
    pub fn supports_network(&self, network: Network) -> bool {
        self.address.is_valid_for_network(network.to_bitcoin())
    }

    /// Returns the relative priority for this onchain address. Higher = better.
    pub(crate) fn relative_priority(&self) -> usize {
        use bitcoin::AddressType::*;
        let address_type =
            match self.address.assume_checked_ref().address_type() {
                Some(x) => x,
                // Non-standard
                None => return 0,
            };
        match address_type {
            // Pay to pubkey hash.
            P2pkh => 2,
            // Pay to script hash.
            P2sh => 2,
            // Pay to witness pubkey hash.
            P2wpkh => 4,
            // Pay to witness script hash.
            P2wsh => 4,
            // Pay to taproot.
            // TODO(phlip9): can we pay to taproot yet?
            P2tr => 3,
            // Unknown standard
            _ => 1,
        }
    }
}

impl<V: NetworkValidation> From<bitcoin::Address<V>> for Onchain {
    fn from(addr: bitcoin::Address<V>) -> Self {
        let address = addr.into_unchecked().clone();
        Self {
            address,
            amount: None,
            label: None,
            message: None,
        }
    }
}

/// An offer payment method with optional amount from a BIP321 URI.
pub struct OfferWithAmount {
    pub offer: Offer,
    /// Amount from BIP321 URI.
    /// Used when the offer is amount-less to pre-fill an amount in the UI.
    pub bip321_amount: Option<Amount>,
}

impl OfferWithAmount {
    pub fn validate_amounts(&self) -> anyhow::Result<()> {
        match (self.offer.min_amount(), self.bip321_amount) {
            (Some(min_amount), Some(bip321_amount))
                if bip321_amount < min_amount =>
                Err(anyhow!(
                    "Receiver error: BIP 321 amount must be greater than or \
                     equal to minimum amount encoded in offer."
                )),
            _ => Ok(()),
        }
    }

    #[inline]
    pub fn supports_network(&self, network: Network) -> bool {
        self.offer.supports_network(network)
    }

    pub fn no_bip321_amount(offer: Offer) -> Self {
        Self {
            offer,
            bip321_amount: None,
        }
    }
}
