//! Payment URI resolution.
//!
//! For core types and parsing, see [`lexe_payment_uri_core`].

/// BIP353 resolution.
pub mod bip353;
/// LNURL-pay and Lightning Address resolution.
pub mod lnurl;

pub use std::cmp;

use anyhow::{Context, anyhow, ensure};
use futures::future;
use lexe_common::ln::network::Network;
pub use lexe_payment_uri_core::*;

/// Resolve a `PaymentUri` into a "best" [`PaymentMethod`] or [`ClaimMethod`].
/// Returns at least one valid pay or claim method.
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
    network: Network,
    payment_uri: PaymentUri,
) -> anyhow::Result<(Option<PaymentMethod>, Option<ClaimMethod>)> {
    // A single scanned/opened PaymentUri can contain multiple different payment
    // methods (e.g., a LN BOLT11 invoice + an onchain fallback address).
    let (pay_methods, claim_methods) =
        resolve(bip353_client, lnurl_client, network, payment_uri).await?;

    // Pick the highest priority payment method.
    let best_payment = pay_methods.into_iter().next();
    let best_claim = claim_methods.into_iter().next();

    if best_claim.is_none() && best_payment.is_none() {
        return Err(anyhow!("No valid payment or claim methods found"));
    }

    Ok((best_payment, best_claim))
}

/// Resolve the [`PaymentUri`] into its component [`PaymentMethod`]s.
/// Filter by network validity and sort by highest priority method first.
/// Ensures at least one method result.
pub async fn resolve(
    bip353_client: &bip353::Bip353Client,
    lnurl_client: &lnurl::LnurlClient,
    network: Network,
    payment_uri: PaymentUri,
) -> anyhow::Result<(Vec<PaymentMethod>, Vec<ClaimMethod>)> {
    // Split the URI into its directly-known methods and any pieces that
    // require further resolution.
    let (mut payment_methods, resolvables) = payment_uri.flatten(network);

    ensure!(
        !payment_methods.is_empty() || !resolvables.is_empty(),
        "No valid payment/claim methods found in URI"
    );

    // Resolve all `Resolvable`s and merge their methods in.
    // Error if *every* resolution fails AND we have no direct methods.
    let resolve_futs = resolvables.into_iter().map(|resolvable| async move {
        match resolvable {
            Resolvable::EmailLike(addr) => {
                let payments = resolve::email_like(
                    bip353_client,
                    lnurl_client,
                    network,
                    addr,
                )
                .await?;
                let claims = Vec::new();
                Ok((payments, claims))
            }
            Resolvable::Lnurl(lnurl) => {
                let lightning_address = None;
                lnurl_client.resolve_lnurl(lnurl, lightning_address).await
            }
        }
    });
    let resolve_results = future::join_all(resolve_futs).await;

    let mut claim_methods = Vec::new();
    let mut resolve_errors = Vec::new();
    for result in resolve_results {
        match result {
            Ok((payments, claims)) => {
                payment_methods.extend(payments);
                claim_methods.extend(claims);
            }
            Err(e) => {
                resolve_errors.push(format!("{e:#}"));
            }
        }
    }

    ensure!(
        !payment_methods.is_empty() || !claim_methods.is_empty(),
        "Failed to resolve methods: {}",
        resolve_errors.join("; "),
    );

    // Filter out all methods that aren't valid for our current network
    // (e.g., ignore all testnet addresses when we're cfg'd for mainnet).
    payment_methods.retain(|method| method.supports_network(network));
    claim_methods.retain(|method| method.supports_network(network));
    ensure!(
        !payment_methods.is_empty() || !claim_methods.is_empty(),
        "Payment code is not valid for {network}"
    );

    // Error on offers that contradict their containing BIP 321 URI.
    // Even though there might be a valid other payment method (e.g. on-chain),
    // the offer is most likely the only off-chain method and thus the one that
    // our user actually cares about.
    for method in &payment_methods {
        if let PaymentMethod::Offer {
            offer,
            bip321_amount,
        } = method
        {
            match (offer.min_amount(), bip321_amount) {
                (Some(min_amount), Some(bip321_amount))
                    if *bip321_amount < min_amount =>
                    return Err(anyhow!(
                        "Receiver error: BIP 321 amount must be greater than \
                         or equal to minimum amount encoded in offer."
                    )),
                _ => (),
            }
        }
    }

    // Sort payment methods by relative priority; highest priority first
    payment_methods.sort_unstable_by_key(|m| cmp::Reverse(m.priority()));

    Ok((payment_methods, claim_methods))
}

/// Helpers to resolve every [`Resolvable`] variant.
mod resolve {
    use super::*;

    /// Resolve an [`EmailLikeAddress`] (BIP353 / Lightning Address) into a
    /// list of [`PaymentMethod`]s.
    pub(super) async fn email_like(
        bip353_client: &bip353::Bip353Client,
        lnurl_client: &lnurl::LnurlClient,
        network: Network,
        email_like: EmailLikeAddress<'static>,
    ) -> anyhow::Result<Vec<PaymentMethod>> {
        let mut methods = Vec::with_capacity(3);
        let mut errors = Vec::with_capacity(2);

        // Try resolving BIP353 if this is a valid BIP353 address.
        if let Some(bip353_fqdn) = &email_like.bip353_fqdn {
            let bip353_result = bip353_client
                .resolve_bip353_fqdn(network, bip353_fqdn.clone())
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

        // Always try resolving Lightning Address, which uses LNURL-pay
        let mut lnurl =
            Lnurl::from_http_url(&email_like.lightning_address_url)?;
        let ln_address_result = lnurl_client
            .get_pay_request(&lnurl)
            .await
            .context("Failed to resolve Lightning Address url");
        match ln_address_result {
            Ok(pay_request) => {
                lnurl.scheme = LnurlScheme::Pay;
                methods.push(PaymentMethod::LnurlPay {
                    pay_request,
                    lnurl: lnurl.to_string(),
                    lightning_address: Some(email_like.lightning_address()),
                });
            }
            Err(e) => errors.push(format!("{e:#}")),
        }

        // Consider it a success if we resolved at least one method, since
        // receivers may support only one of BIP353 or Lightning Address.
        // Otherwise, return a combined error.
        if !methods.is_empty() {
            Ok(methods)
        } else {
            debug_assert!(!errors.is_empty());
            Err(anyhow!("{}", errors.join("; ")))
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use lexe_common::{env::DeployEnv, ln::network::Network};
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
    /// $ RUST_LOG=debug just cargo-test -p lexe-payment-uri test_resolve_best_bluematt -- --ignored --nocapture
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

        lexe_logger::init_for_testing();

        let payment_uri = PaymentUri::parse("matt@mattcorallo.com").unwrap();
        info!("Resolving best payment method for matt@mattcorallo.com");

        let bip353_client =
            bip353::Bip353Client::new(bip353::GOOGLE_DOH_ENDPOINT).unwrap();
        let lnurl_client = lnurl::LnurlClient::new(DeployEnv::Prod).unwrap();

        let (maybe_pay_method, _) = resolve_best(
            &bip353_client,
            &lnurl_client,
            Network::Mainnet,
            payment_uri,
        )
        .apply(|fut| tokio::time::timeout(RESOLVE_BEST_TIMEOUT, fut))
        .await
        .expect("Timed out")
        .unwrap();
        let payment_method = maybe_pay_method.expect("No payment method found");

        // Payment methods are Offer and Onchain, but Offer is higher priority.
        assert!(payment_method.is_offer());
        assert!(payment_method.supports_network(Network::Mainnet));

        info!("Successfully resolved BlueMatt's payment methods");
    }
}
