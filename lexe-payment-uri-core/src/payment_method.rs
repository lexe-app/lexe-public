use std::cmp::Reverse;

use lexe_api_core::types::{
    invoice::Invoice, lnurl::LnurlPayRequest, offer::Offer,
};
use lexe_common::ln::{amount::Amount, network::Network};
use lexe_connect::request::CredentialRequest;

use crate::{
    email_like::EmailLikeAddress,
    lnurl::{Lnurl, LnurlWithdrawRequest},
};

/// A method a [`PaymentUri`] resolves to.
///
/// [`PaymentUri`]: crate::PaymentUri
pub trait PaymentUriMethod {
    /// The method's kind as a stable string, e.g. "invoice", "lnurl-withdraw".
    fn kind(&self) -> &'static str;

    /// Whether the method is valid for `network`.
    fn supports_network(&self, network: Network) -> bool;

    /// Higher priority methods are preferred. For `sort_by_key` and friends.
    fn priority(&self) -> usize;
}

/// A single "payment method" -- each kind here should correspond with a single
/// linear (outbound) payment flow for a user, where there are no other
/// alternate methods.
///
/// For example, a Unified BTC QR code contains a single BIP321 URI,
/// which may contain _multiple_ discrete payment methods (an onchain address,
/// a BOLT11 invoice, a BOLT12 offer).
///
/// Compare with [`ClaimMethod`], which is the inbound equivalent.
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

        /// The original Human Bitcoin Address this offer was resolved from, if
        /// it originated from one. Includes ₿ prefix: "₿user@domain".
        human_bitcoin_address: Option<String>,
    },
    LnurlPay {
        /// The LNURL-pay request, which includes information about
        /// the amount constraints, callback, etc. associated with the LNURL.
        pay_request: LnurlPayRequest,

        /// The original LNURL-pay LNURL, e.g. `lnurlp://...`
        // Should NOT be an HTTP URL
        lnurl: String,

        /// The original Lightning Address (`user@domain`) this was resolved
        /// from, if it originated from one rather than a raw LNURL.
        lightning_address: Option<String>,
    },
}

/// A single "claim method" -- each kind here should correspond with a single
/// linear (inbound) payment flow for a user, where there are no other
/// alternate methods.
///
/// Compare with [`PaymentMethod`], which is the outbound equivalent.
//
// NOTE: This is exposed in the Rust SDK, so only use stable public types here.
pub enum ClaimMethod {
    LnurlWithdraw {
        /// The LNURL-withdraw LNURL, e.g. `lnurlw://...`
        // Should NOT be an HTTP URL
        lnurl: String,

        /// The LNURL-withdraw request, which includes information about
        /// the amount constraints, callback, etc. associated with the LNURL.
        withdraw_request: LnurlWithdrawRequest,
    },
    // TODO(nicole): Support BOLT12 refunds
}

/// A single "auth method": a general auth mechanism which moves no money.
/// Compare with [`PaymentMethod`] (outbound) and [`ClaimMethod`] (inbound).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthMethod {
    /// Grant a Lexe client credential to an app or service.
    LexeConnect(Box<CredentialRequest>),
}

/// Every method a payment code resolves to, by kind.
#[derive(Default)]
pub struct PaymentUriMethods {
    pub payment_methods: Vec<PaymentMethod>,
    pub claim_methods: Vec<ClaimMethod>,
    pub auth_methods: Vec<AuthMethod>,
}

/// The highest priority method of each kind, if any.
pub struct BestPaymentUriMethods {
    pub payment_method: Option<PaymentMethod>,
    pub claim_method: Option<ClaimMethod>,
    pub auth_method: Option<AuthMethod>,
}

/// "Almost" a payment/claim method: a piece of payment data that requires
/// further resolution before it becomes a [`PaymentMethod`]/[`ClaimMethod`].
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

// --- impl PaymentMethod --- //

impl PaymentMethod {
    /// Check if the payment method is an onchain address.
    pub fn is_onchain(&self) -> bool {
        matches!(self, Self::Onchain { .. })
    }

    /// Check if the payment method is a BOLT11 invoice.
    pub fn is_invoice(&self) -> bool {
        matches!(self, Self::Invoice { .. })
    }

    /// Check if the payment method is a BOLT12 offer.
    pub fn is_offer(&self) -> bool {
        matches!(self, Self::Offer { .. })
    }

    /// Check if the payment method is an LNURL-pay endpoint.
    pub fn is_lnurl_pay(&self) -> bool {
        matches!(self, Self::LnurlPay { .. })
    }
}

impl PaymentUriMethod for PaymentMethod {
    fn kind(&self) -> &'static str {
        match self {
            PaymentMethod::Onchain { .. } => "onchain",
            PaymentMethod::Invoice { .. } => "invoice",
            PaymentMethod::Offer { .. } => "offer",
            PaymentMethod::LnurlPay { .. } => "lnurl-pay",
        }
    }

    fn supports_network(&self, network: Network) -> bool {
        match self {
            Self::Onchain { address, .. } => address
                .as_unchecked()
                .is_valid_for_network(network.to_bitcoin()),
            Self::Invoice { invoice } => invoice.supports_network(network),
            Self::Offer { offer, .. } => offer.supports_network(network),
            Self::LnurlPay { .. } => true,
        }
    }

    fn priority(&self) -> usize {
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

// --- impl ClaimMethod --- //

impl ClaimMethod {
    // TODO(nicole): Introduce when more variants added
    // /// Check if the claim method is an LNURL-withdraw endpoint.
    // pub fn is_lnurl_withdraw(&self) -> bool {
    //     matches!(self, Self::LnurlWithdraw { .. })
    // }
}

impl PaymentUriMethod for ClaimMethod {
    fn kind(&self) -> &'static str {
        match self {
            ClaimMethod::LnurlWithdraw { .. } => "lnurl-withdraw",
        }
    }

    fn supports_network(&self, _network: Network) -> bool {
        match self {
            ClaimMethod::LnurlWithdraw { .. } => true,
        }
    }

    fn priority(&self) -> usize {
        match self {
            ClaimMethod::LnurlWithdraw { .. } => 0,
        }
    }
}

// --- impl AuthMethod --- //

impl PaymentUriMethod for AuthMethod {
    fn kind(&self) -> &'static str {
        match self {
            AuthMethod::LexeConnect(_) => "lexe-connect",
        }
    }

    fn supports_network(&self, _network: Network) -> bool {
        match self {
            AuthMethod::LexeConnect(_) => true,
        }
    }

    fn priority(&self) -> usize {
        match self {
            AuthMethod::LexeConnect(_) => 0,
        }
    }
}

// --- impl PaymentUriMethods --- //

impl PaymentUriMethods {
    pub fn is_empty(&self) -> bool {
        self.payment_methods.is_empty()
            && self.claim_methods.is_empty()
            && self.auth_methods.is_empty()
    }

    /// Drop the methods that aren't valid for `network`.
    pub fn retain_network(&mut self, network: Network) {
        fn retain<M: PaymentUriMethod>(methods: &mut Vec<M>, network: Network) {
            methods.retain(|m| m.supports_network(network));
        }
        retain(&mut self.payment_methods, network);
        retain(&mut self.claim_methods, network);
        retain(&mut self.auth_methods, network);
    }

    /// Sort each kind by priority, highest first.
    pub fn sort_by_priority(&mut self) {
        fn sort<M: PaymentUriMethod>(methods: &mut [M]) {
            methods.sort_unstable_by_key(|m| Reverse(m.priority()));
        }
        sort(&mut self.payment_methods);
        sort(&mut self.claim_methods);
        sort(&mut self.auth_methods);
    }

    /// The first method of each kind, which is the best after
    /// [`Self::sort_by_priority`].
    pub fn into_best(self) -> BestPaymentUriMethods {
        BestPaymentUriMethods {
            payment_method: self.payment_methods.into_iter().next(),
            claim_method: self.claim_methods.into_iter().next(),
            auth_method: self.auth_methods.into_iter().next(),
        }
    }
}
