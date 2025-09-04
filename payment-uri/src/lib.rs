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
pub fn resolve_best(
    network: LxNetwork,
    payment_uri: PaymentUri,
) -> anyhow::Result<PaymentMethod> {
    // A single scanned/opened PaymentUri can contain multiple different payment
    // methods (e.g., a LN BOLT11 invoice + an onchain fallback address).
    let mut payment_methods = payment_uri.flatten();

    // Filter out all methods that aren't valid for our current network
    // (e.g., ignore all testnet addresses when we're cfg'd for mainnet).
    payment_methods.retain(|method| method.supports_network(network));
    anyhow::ensure!(
        !payment_methods.is_empty(),
        "Payment code is not valid for {network}"
    );

    // Pick the most preferable payment method.
    let best = payment_methods
        .into_iter()
        .max_by_key(|x| match x {
            PaymentMethod::Invoice(_) => 30,
            PaymentMethod::Offer(_) => 20,
            PaymentMethod::Onchain(o) => 10 + o.relative_priority(),
        })
        .expect("We just checked there's at least one method");

    Ok(best)
}
