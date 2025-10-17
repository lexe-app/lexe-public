//! [`payment_uri`] interface

use anyhow::Context;
use payment_uri::{bip353, lnurl};

use crate::ffi::types::{Network, PaymentMethod};

/// Resolve a (possible) [`PaymentUri`] string that we just
/// scanned/pasted into the best [`PaymentMethod`] for us to pay.
///
/// [`PaymentUri`]: payment_uri::PaymentUri
pub async fn resolve_best(
    network: Network,
    uri_str: String,
) -> anyhow::Result<PaymentMethod> {
    // TODO(max): The app should hold these somewhere so we can reuse them.
    let bip353_client = bip353::Bip353Client::new(bip353::GOOGLE_DOH_ENDPOINT)
        .context("Failed to build BIP353 client")?;
    let lnurl_client =
        lnurl::LnurlClient::new().context("Failed to build LNURL client")?;

    let payment_uri = payment_uri::PaymentUri::parse(&uri_str)
        .context("Unrecognized payment code")?;

    payment_uri::resolve_best(
        &bip353_client,
        &lnurl_client,
        network.into(),
        payment_uri,
    )
    .await
    .map(PaymentMethod::from)
}
