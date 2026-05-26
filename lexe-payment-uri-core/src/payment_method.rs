use lexe_api_core::types::{
    invoice::Invoice, lnurl::LnurlPayRequest, offer::Offer,
};
use lexe_common::ln::{amount::Amount, network::Network};

use crate::{email_like::EmailLikeAddress, lnurl::Lnurl};

/// A single "payment method" -- each kind here should correspond with a single
/// linear (outbound) payment flow for a user, where there are no other
/// alternate methods.
///
/// For example, a Unified BTC QR code contains a single BIP321 URI,
/// which may contain _multiple_ discrete payment methods (an onchain address,
/// a BOLT11 invoice, a BOLT12 offer).
//
// NOTE: This is exposed in the Rust SDK, so only use stable public types here.
#[allow(clippy::large_enum_variant)]
pub enum PaymentMethod {
    Onchain {
        /// An onchain Bitcoin address.
        address: bitcoin::Address,

        /// The amount to pay to the onchain address, if specified.
        ///
        /// Parsed from a BIP321 URI or BOLT11 invoice containing the
        /// onchain address.
        amount: Option<Amount>,

        /// A label for the onchain address.
        ///
        /// Parsed from a BIP321 URI containing the onchain address.
        label: Option<String>,

        /// A message describing the transaction or its purpose.
        ///
        /// Parsed from a BIP321 URI or BOLT11 invoice containing the
        /// onchain address.
        message: Option<String>,
    },
    Invoice {
        /// A BOLT11 invoice.
        invoice: Invoice,
    },
    Offer {
        /// A BOLT12 offer.
        offer: Offer,

        /// The amount to pay to the offer, if specified.
        ///
        /// Parsed from a BIP321 URI containing the offer.
        bip321_amount: Option<Amount>,
    },
    LnurlPay {
        /// An LNURL-pay URI.
        lnurl: String,

        /// The LNURL-pay data, which includes information about
        /// the amount constraints, callback, etc. associated with the LNURL.
        pay_request: LnurlPayRequest,
    },
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
        matches!(self, Self::Onchain { .. })
    }

    pub fn is_invoice(&self) -> bool {
        matches!(self, Self::Invoice { .. })
    }

    pub fn is_offer(&self) -> bool {
        matches!(self, Self::Offer { .. })
    }

    pub fn is_lnurl_pay(&self) -> bool {
        matches!(self, Self::LnurlPay { .. })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            PaymentMethod::Onchain { .. } => "onchain",
            PaymentMethod::Invoice { .. } => "invoice",
            PaymentMethod::Offer { .. } => "offer",
            PaymentMethod::LnurlPay { .. } => "lnurl-pay",
        }
    }

    pub fn supports_network(&self, network: Network) -> bool {
        match self {
            Self::Onchain { address, .. } => address
                .as_unchecked()
                .is_valid_for_network(network.to_bitcoin()),
            Self::Invoice { invoice } => invoice.supports_network(network),
            Self::Offer { offer, .. } => offer.supports_network(network),
            Self::LnurlPay { .. } => true,
        }
    }

    /// For use with `sort_by_key`, `max_by_key`, etc.
    /// Payment methods with a higher priority should be preferred over others.
    pub fn priority(&self) -> usize {
        match self {
            PaymentMethod::Invoice { .. } => 40,
            PaymentMethod::Offer { .. } => 30,
            PaymentMethod::LnurlPay { .. } => 20,
            PaymentMethod::Onchain { address, .. } => {
                let relative_priority = match address.address_type() {
                    // Non-standard
                    None => return 0,
                    Some(address_type) => {
                        use bitcoin::AddressType::*;
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
                };
                10 + relative_priority
            }
        }
    }
}
