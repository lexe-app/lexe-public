//! [`payment_uri`] interface

use anyhow::Context;

use crate::ffi::types::{Network, PaymentMethod};

/// Resolve a (possible) [`PaymentUri`] string that we just
/// scanned/pasted into the best [`PaymentMethod`] for us to pay.
///
/// [`PaymentUri`]: payment_uri::PaymentUri
pub async fn resolve_best(
    network: Network,
    uri_str: String,
) -> anyhow::Result<PaymentMethod> {
    // TODO(max): The app should hold this somewhere so we can reuse it.
    let lnurl_client = payment_uri::lnurl::LnurlClient::new()
        .context("Failed to build LNURL client")?;

    let payment_uri = payment_uri::PaymentUri::parse(&uri_str)
        .context("Unrecognized payment code")?;

    payment_uri::resolve_best(
        &lnurl_client,
        network.into(),
        payment_uri,
        payment_uri::bip353::GOOGLE_DOH_ENDPOINT,
    )
    .await
    .map(PaymentMethod::from)
}
