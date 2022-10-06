use std::fmt::{self, Display};
use std::str::FromStr;

use lightning_invoice::Invoice;
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// Wraps [`lightning_invoice::Invoice`] to impl [`serde`] Serialize /
/// Deserialize using the LDK's [`FromStr`] / [`Display`] impls.
#[derive(Clone, Debug, Eq, PartialEq, SerializeDisplay, DeserializeFromStr)]
pub struct LxInvoice(pub Invoice);

impl FromStr for LxInvoice {
    type Err = lightning_invoice::ParseOrSemanticError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Invoice::from_str(s).map(Self)
    }
}

impl Display for LxInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// TODO(max): We should proptest the serde and fromstr/display impls but it's
// non-trivial to impl Arbitrary for Invoice. lightning_invoice::InvoiceBuilder
// is probably the way.
