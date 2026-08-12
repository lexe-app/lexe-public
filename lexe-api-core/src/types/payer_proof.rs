use std::{fmt, str::FromStr};

use lightning::offers::{
    parse::Bolt12ParseError, payer_proof::PayerProof as LdkPayerProof,
};
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// A cryptographic proof that a [`Bolt12Invoice`] was paid, disclosing a
/// payer-chosen subset of the invoice's fields to a third-party verifier.
///
/// A `PayerProof` is already cryptographically verified; verification occurs
/// during parsing.
///
/// Serialized as bech32, e.g. `lnp1...`.
///
/// [`Bolt12Invoice`]: super::bolt12_invoice::Bolt12Invoice
#[derive(SerializeDisplay, DeserializeFromStr)]
pub struct PayerProof(pub LdkPayerProof);

impl From<LdkPayerProof> for PayerProof {
    #[inline]
    fn from(value: LdkPayerProof) -> Self {
        Self(value)
    }
}

impl fmt::Display for PayerProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for PayerProof {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LdkPayerProof::from_str(s).map(Self).map_err(ParseError)
    }
}

/// Exists because [`Bolt12ParseError`] doesn't impl [`fmt::Display`].
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError(pub Bolt12ParseError);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let err = &self.0;
        write!(f, "Failed to parse payer proof: {err:?}")
    }
}

impl std::error::Error for ParseError {}
