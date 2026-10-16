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

/// Resolve a [`PaymentUri`] into the highest priority method of each kind.
/// Returns at least one method.
//
// phlip9: picking by static priority is pretty dumb. It's not hard to imagine
// a better strategy, like using our current liquidity/balance to decide
// onchain vs LN, or returning all methods and giving the user a choice.
pub async fn resolve_best(
    bip353_client: &bip353::Bip353Client,
    lnurl_client: &lnurl::LnurlClient,
    network: Network,
    payment_uri: PaymentUri,
) -> anyhow::Result<BestPaymentUriMethods> {
    // `resolve` sorts each kind by priority, so the first of each is the best.
    let methods =
        resolve(bip353_client, lnurl_client, network, payment_uri).await?;
    Ok(methods.into_best())
}

/// Resolve the [`PaymentUri`] into its component [`PaymentUriMethods`].
/// Filter by network validity and sort by highest priority method first.
/// Ensures at least one method result.
pub async fn resolve(
    bip353_client: &bip353::Bip353Client,
    lnurl_client: &lnurl::LnurlClient,
    network: Network,
    payment_uri: PaymentUri,
) -> anyhow::Result<PaymentUriMethods> {
    let permitted_methods = payment_uri.permitted_methods();

    // Split the URI into its directly-known methods and any pieces that
    // require further resolution.
    let (mut methods, resolvables) = payment_uri.flatten(network);

    ensure!(
        !methods.is_empty() || !resolvables.is_empty(),
        "No valid payment, claim, or auth methods found in URI"
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

    let mut resolve_errors = Vec::new();
    for result in resolve_results {
        match result {
            Ok((payment_methods, claim_methods)) => {
                methods.payment_methods.extend(payment_methods);
                methods.claim_methods.extend(claim_methods);
            }
            Err(e) => {
                resolve_errors.push(format!("{e:#}"));
            }
        }
    }

    ensure!(
        !methods.is_empty(),
        "Failed to resolve methods: {}",
        resolve_errors.join("; "),
    );

    // Drop the kinds of methods the URI doesn't permit. This waits until
    // resolvables are resolved, since e.g. an LNURL may pay or claim.
    methods.retain_permitted(permitted_methods);
    ensure!(
        !methods.is_empty(),
        "Payment code doesn't contain a {permitted_methods} method"
    );

    // Filter out all methods that aren't valid for our current network
    // (e.g., ignore all testnet addresses when we're cfg'd for mainnet).
    methods.retain_network(network);
    ensure!(
        !methods.is_empty(),
        "Payment code is not valid for {network}"
    );

    // Error on offers that contradict their containing BIP 321 URI.
    // Even though there might be a valid other payment method (e.g. on-chain),
    // the offer is most likely the only off-chain method and thus the one that
    // our user actually cares about.
    for method in &methods.payment_methods {
        if let PaymentMethod::Offer {
            offer,
            bip321_amount,
            ..
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

    // Sort by relative priority; highest priority first
    methods.sort_by_priority();

    Ok(methods)
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
            let human_bitcoin_address = email_like.human_bitcoin_address();
            let bip353_result = bip353_client
                .resolve_bip353_fqdn(
                    network,
                    bip353_fqdn.clone(),
                    human_bitcoin_address,
                )
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

    /// The bare payment code, then wrapped in each kind of Lexe link.
    fn wrap_all(s: &str) -> [PaymentUri; 3] {
        let parse = || Box::new(PaymentUri::parse(s).unwrap());
        [
            *parse(),
            PaymentUri::LexeUri(LexeUri {
                payment_uri: parse(),
            }),
            PaymentUri::LexePayUrl(LexePayUrl {
                payment_uri: parse(),
            }),
        ]
    }

    /// Resolve with real clients, for payment codes that resolve offline.
    async fn resolve_mainnet(
        payment_uri: PaymentUri,
    ) -> anyhow::Result<PaymentUriMethods> {
        let bip353_client =
            bip353::Bip353Client::new(bip353::GOOGLE_DOH_ENDPOINT)?;
        let lnurl_client = lnurl::LnurlClient::new(DeployEnv::Prod)?;
        resolve(&bip353_client, &lnurl_client, Network::Mainnet, payment_uri)
            .await
    }

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

        let best = resolve_best(
            &bip353_client,
            &lnurl_client,
            Network::Mainnet,
            payment_uri,
        )
        .apply(|fut| tokio::time::timeout(RESOLVE_BEST_TIMEOUT, fut))
        .await
        .expect("Timed out")
        .unwrap();
        let payment_method =
            best.payment_method.expect("No payment method found");

        // Payment methods are Offer and Onchain, but Offer is higher priority.
        assert!(payment_method.is_offer());
        assert!(payment_method.supports_network(Network::Mainnet));

        info!("Successfully resolved BlueMatt's payment methods");
    }

    /// `resolve` drops the methods a Lexe link doesn't permit. Runs offline,
    /// since none of these payment codes need resolving.
    #[tokio::test]
    async fn test_resolve_lexe_links() {
        let invoice = "lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap\
            4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w\
            08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2\
            zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxp\
            upsfh36ks9jez7aamsn7hpkxqpw2xyek";
        for payment_uri in wrap_all(&format!("lightning:{invoice}")) {
            let methods = resolve_mainnet(payment_uri).await.unwrap();
            assert!(methods.payment_methods[0].is_invoice());
        }

        // Only a bare LexeConnect request authenticates
        let connect = "https://lexe.app/connect?v=1\
            &post_url=https%3A%2F%2Fpaygate.com%2Flexe\
            &one_time_secret=000102030405060708090a0b0c0d0e0f\
            &scopes=receive";
        let [bare, lexe_uri, lexe_pay] = wrap_all(connect);
        let methods = resolve_mainnet(bare).await.unwrap();
        assert_eq!(methods.auth_methods.len(), 1);
        resolve_mainnet(lexe_uri).await.map(|_| ()).unwrap_err();
        resolve_mainnet(lexe_pay).await.map(|_| ()).unwrap_err();
    }

    /// An LNURL is only known to claim once resolved, so a Lexe pay url drops
    /// its claim after resolving.
    #[tokio::test]
    async fn test_resolve_lexe_links_lnurl_claim() {
        // A LUD-08 fast LNURL-withdraw, which resolves to a claim without a
        // fetch.
        let s = "lnurlw://service.com/withdraw\
            ?tag=withdrawRequest\
            &k1=k1\
            &callback=https%3A%2F%2Fservice.com%2Fwithdraw%2Fcallback\
            &defaultDescription=Refund\
            &minWithdrawable=1000\
            &maxWithdrawable=100000";

        let [bare, lexe_uri, lexe_pay] = wrap_all(s);
        for payment_uri in [bare, lexe_uri] {
            let methods = resolve_mainnet(payment_uri).await.unwrap();
            assert_eq!(methods.claim_methods.len(), 1);
        }
        resolve_mainnet(lexe_pay).await.map(|_| ()).unwrap_err();
    }
}
