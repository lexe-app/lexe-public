//! Payment URI resolution.
//!
//! For core types and parsing, see [`payment_uri_core`].

/// BIP353 resolution.
pub mod bip353;

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
    network: LxNetwork,
    payment_uri: PaymentUri,
    doh_endpoint: &str,
) -> anyhow::Result<PaymentMethod> {
    // A single scanned/opened PaymentUri can contain multiple different payment
    // methods (e.g., a LN BOLT11 invoice + an onchain fallback address).
    let mut payment_methods =
        resolve_payment_methods(payment_uri, doh_endpoint)
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
    payment_uri: PaymentUri,
    doh_endpoint: &str,
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
            let maybe_bip353_result = match email_like.bip353_fqdn {
                Some(bip353_fqdn) => {
                    let bip353_result =
                        bip353::resolve_bip353_fqdn(bip353_fqdn, doh_endpoint)
                            .await
                            .context("Failed to resolve BIP353 address");
                    Some(bip353_result)
                }
                None => None,
            };

            // Prefer BIP353 if available
            if let Some(Ok(bip353_methods)) = maybe_bip353_result {
                return Ok(bip353_methods);
            }

            // TODO(max): We can simplify maybe_bip353_result if we don't ever
            // use the BIP353 error here.
            // TODO(max): Also resolve as LN address.

            Vec::new()
        }

        PaymentUri::Lnurl(lnurl) => {
            // TODO(max): Implement LNURL resolution
            let _ = lnurl;
            return Err(anyhow!("LNURL resolution not supported yet"));
        }
    };

    Ok(payment_methods)
}
