//! [`payment_uri`] interface

use anyhow::Context;

use crate::ffi::types::{Network, PaymentMethod};

/// Resolve a (possible) [`PaymentUri`] string that we just
/// scanned/pasted into the best [`PaymentMethod`] for us to pay.
///
/// [`PaymentUri`]: payment_uri::PaymentUri
pub fn resolve_best(
    network: Network,
    uri_str: String,
) -> anyhow::Result<PaymentMethod> {
    let payment_uri = payment_uri::PaymentUri::parse(&uri_str)
        .context("Unrecognized payment code")?;
    payment_uri::resolve_best(network.into(), payment_uri)
        .map(PaymentMethod::from)
}
