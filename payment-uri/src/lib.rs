//! Payment URI resolution.
//!
//! For core types and parsing, see [`payment_uri_core`].

/// BIP353 resolution.
pub mod bip353;
/// LNURL-pay and Lightning Address resolution.
pub mod lnurl;

use anyhow::{anyhow, ensure, Context};
use common::ln::network::LxNetwork;
pub use payment_uri_core::*;

/// Resolve a `PaymentUri` into a single, "best" [`PaymentMethod`].
//
// phlip9: this impl is currently pretty dumb and just unconditionally
// returns the first (valid) BOLT11 invoice it finds, o/w onchain. It's not
// hard to imagine a better strategy, like using our current
// liquidity/balance to decide onchain vs LN, or returning all methods and
// giving the user a choice. This'll also need to be async in the future, as
// we'll need to fetch invoices from any LNURL endpoints we come across.
pub async fn resolve_best(
    bip353_client: &bip353::Bip353Client,
    lnurl_client: &lnurl::LnurlClient,
    network: LxNetwork,
    payment_uri: PaymentUri,
) -> anyhow::Result<PaymentMethod> {
    // A single scanned/opened PaymentUri can contain multiple different payment
    // methods (e.g., a LN BOLT11 invoice + an onchain fallback address).
    let mut payment_methods =
        resolve_payment_methods(bip353_client, lnurl_client, payment_uri)
            .await
            .context("Failed to resolve payment URI into payment methods")?;

    // Filter out all methods that aren't valid for our current network
    // (e.g., ignore all testnet addresses when we're cfg'd for mainnet).
    payment_methods.retain(|method| method.supports_network(network));
    ensure!(
        !payment_methods.is_empty(),
        "Payment code is not valid for {network}"
    );

    // Pick the most preferable payment method.
    let best = payment_methods
        .into_iter()
        .max_by_key(|x| match x {
            PaymentMethod::Invoice(_) => 40,
            PaymentMethod::Offer(_) => 30,
            PaymentMethod::LnurlPayRequest(_) => 20,
            PaymentMethod::Onchain(o) => 10 + o.relative_priority(),
        })
        .expect("We just checked there's at least one method");

    Ok(best)
}

/// Resolve the [`PaymentUri`] into its component [`PaymentMethod`]s.
async fn resolve_payment_methods(
    bip353_client: &bip353::Bip353Client,
    lnurl_client: &lnurl::LnurlClient,
    payment_uri: PaymentUri,
) -> anyhow::Result<Vec<PaymentMethod>> {
    let payment_methods = match payment_uri {
        PaymentUri::Bip321Uri(bip321) => bip321.flatten(),

        PaymentUri::LightningUri(lnuri) => lnuri.flatten(),

        PaymentUri::Invoice(invoice) =>
            payment_uri_core::helpers::flatten_invoice(invoice),

        PaymentUri::Offer(offer) => vec![PaymentMethod::Offer(offer)],

        PaymentUri::Address(address) =>
            vec![PaymentMethod::Onchain(Onchain::from(address))],

        PaymentUri::EmailLikeAddress(email_like) => {
            let mut methods = Vec::with_capacity(3);
            let mut errors = Vec::with_capacity(2);

            // Try resolving BIP353 if this is a valid BIP353 address.
            if let Some(bip353_fqdn) = email_like.bip353_fqdn {
                let bip353_result = bip353_client
                    .resolve_bip353_fqdn(bip353_fqdn)
                    .await
                    .context("Failed to resolve BIP353 address");
                match bip353_result {
                    Ok(bip353_methods) => {
                        // Early return if we found any non-onchain methods,
                        // as we can pay those immediately.
                        // NOTE: Revisit if/when we support paying via ecash?
                        if bip353_methods.iter().any(|m| !m.is_onchain()) {
                            return Ok(bip353_methods);
                        } else {
                            methods.extend(bip353_methods);
                        }
                    }
                    Err(e) => errors.push(format!("{e:#}")),
                }
            }

            // Always try resolving Lightning Address
            let ln_address_result = lnurl_client
                .get_pay_request(&email_like.lightning_address_url)
                .await
                .context("Failed to resolve Lightning Address url");
            match ln_address_result {
                Ok(pay_request) =>
                    methods.push(PaymentMethod::LnurlPayRequest(pay_request)),
                Err(e) => errors.push(format!("{e:#}")),
            }

            // Consider it a success if we resolved at least one method, since
            // receivers may support only one of BIP353 or Lightning Address.
            // Otherwise, return a combined error.
            if !methods.is_empty() {
                methods
            } else {
                debug_assert!(!errors.is_empty());
                let joined_errs = errors.join("; ");
                return Err(anyhow!("{joined_errs}"));
            }
        }

        PaymentUri::Lnurl(lnurl) => {
            let pay_request = lnurl_client
                .get_pay_request(&lnurl.http_url)
                .await
                .context("Failed to resolve LNURL-pay url")?;

            vec![PaymentMethod::LnurlPayRequest(pay_request)]
        }
    };

    Ok(payment_methods)
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use common::ln::network::LxNetwork;
    use lexe_std::Apply;
    use tracing::info;

    use super::*;

    /// Live test that resolves Matt's BIP353 address using resolve_best.
    ///
    /// As of 2025-10-11, "matt@mattcorallo.com" doesn't support Lightning
    /// Address--Lightning Address resolution is expected to fail. This is a
    /// common case whenever we resolve email-like addresses that start with the
    /// `₿` prefix. This tests that Lightning Address resolution fails quickly
    /// in this case rather than always adding a delay equivalent to
    /// [`lnurl::LNURL_HTTP_TIMEOUT`].
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_resolve_best_bluematt -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_resolve_best_bluematt() {
        /// Both BIP353 pass + Lightning Address fail should happen within this.
        const RESOLVE_BEST_TIMEOUT: Duration = Duration::from_secs(5);
        lexe_std::const_assert!(
            lnurl::LNURL_HTTP_TIMEOUT.as_secs()
                > RESOLVE_BEST_TIMEOUT.as_secs()
        );

        logger::init_for_testing();

        let payment_uri = PaymentUri::parse("matt@mattcorallo.com").unwrap();
        info!("Resolving best payment method for matt@mattcorallo.com");

        let bip353_client =
            bip353::Bip353Client::new(bip353::GOOGLE_DOH_ENDPOINT).unwrap();
        let lnurl_client = lnurl::LnurlClient::new().unwrap();

        let payment_method = resolve_best(
            &bip353_client,
            &lnurl_client,
            LxNetwork::Mainnet,
            payment_uri,
        )
        .apply(|fut| tokio::time::timeout(RESOLVE_BEST_TIMEOUT, fut))
        .await
        .expect("Timed out")
        .unwrap();

        // Payment methods are Offer and Onchain, but Offer is higher priority.
        assert!(matches!(payment_method, PaymentMethod::Offer(_)));
        assert!(payment_method.supports_network(LxNetwork::Mainnet));

        info!("Successfully resolved BlueMatt's payment methods");
    }
}
