//! Core types and logic required to permissively parse Bitcoin / lightning
//! payment addresses and URIs. For actually *resolving* a [`PaymentUri`] into a
//! [`PaymentMethod`], which frequently requires accessing a network, see the
//! [`payment-uri`] crate.
//!
//! [`PaymentUri`]: payment_uri::PaymentUri
//! [`PaymentMethod`]: payment_method::PaymentMethod
//!
//! # Permissive parsing
//!
//! This crate parses various BTC-related payment methods permissively.
//! That means we accept inputs that are not strictly well-formed.
//!
//! Other wallet parsers for comparison:
//! + [ACINQ/phoenix - Parser](https://github.com/ACINQ/phoenix/blob/master/phoenix-shared/src/commonMain/kotlin/fr.acinq.phoenix/utils/Parser.kt)
//! + [breez/breez-sdk - input_parser.rs](https://github.com/breez/breez-sdk-greenlight/blob/main/libs/sdk-common/src/input_parser.rs)
//! + [MutinyWallet/bitcoin_waila (unmaintained)](https://github.com/MutinyWallet/bitcoin-waila/blob/master/waila/src/lib.rs)

// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]

use std::fmt::{self, Display};

use lexe_api_core::types::{invoice, offer};

/// Export all public types so they are accessible via the crate root.
/// The containing modules are used only for internal organization, and are
/// intentionally private so crate users have a simple, flat namespace.
pub use crate::{
    payment_method::{Onchain, PaymentMethod},
    payment_uri::PaymentUri,
};

/// BIP321 / BIP21 parsing and formatting.
mod bip321_uri;
/// Email-like payment URIs, including Lightning Addresses and BIP353.
mod email_like;
/// "lightning:" URIs, containing a BOLT 11 invoice or BOLT12 offer.
mod lightning_uri;
/// LNURLs.
mod lnurl;
/// `PaymentMethod` and subtypes, representing a resolved payment method.
mod payment_method;
/// Top level `PaymentUri` representing a parsed payment URI or address.
mod payment_uri;
/// Low level URI building blocks: `Uri`, `UriParam`, `UriParamKey`
mod uri;

/// Private helper functions and utilities shared between modules.
mod helpers;

/// Refuse to parse any input longer than this many KiB.
const MAX_INPUT_LEN_KIB: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    TooLong,
    BadScheme,
    UnknownCode,
    EmailLikeUnsupported,
    Bip353Unsupported,
    LnurlUnsupported,
    InvalidInvoice(invoice::ParseError),
    InvalidOffer(offer::ParseError),
    InvalidBtcAddress(bitcoin::address::ParseError),
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "Payment code is too long to parse (>{MAX_INPUT_LEN_KIB} KiB)"
            ),
            Self::BadScheme => write!(f, "Unrecognized payment URI scheme"),
            Self::UnknownCode => write!(f, "Unrecognized payment code"),
            Self::EmailLikeUnsupported => write!(
                f,
                "Lightning Addresses and BIP353 are not supported yet"
            ),
            Self::Bip353Unsupported => write!(f, "BIP353 is not supported yet"),
            Self::LnurlUnsupported => write!(f, "LNURL is not supported yet"),
            Self::InvalidInvoice(err) => Display::fmt(err, f),
            Self::InvalidOffer(err) => Display::fmt(err, f),
            Self::InvalidBtcAddress(err) =>
                write!(f, "Failed to parse on-chain address: {err}"),
        }
    }
}
