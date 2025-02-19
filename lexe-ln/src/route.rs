//! Routing logic.

use anyhow::{anyhow, Context};
use common::{
    api::user::NodePk,
    debug_panic_release_log,
    ln::{amount::Amount, invoice::LxInvoice},
};
use const_utils::const_assert;
use either::Either;
use lightning::{
    ln::msgs::LightningError,
    routing::router::{
        Payee, PaymentParameters, Route, RouteParameters, Router,
        DEFAULT_MAX_PATH_COUNT, DEFAULT_MAX_TOTAL_CLTV_EXPIRY_DELTA,
        MAX_PATH_LENGTH_ESTIMATE,
    },
};
use lightning_invoice::DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA;

use crate::{
    alias::RouterType,
    traits::{LexeChannelManager, LexePersister},
};

/// Finds a route for the given [`LxInvoice`].
///
/// `fallback_amount` specifies the amount we will pay if the invoice to be paid
/// is amountless, and must be [`Some`] for amountless invoices.
pub fn find_route_for_bolt11_invoice<CM, PS>(
    channel_manager: &CM,
    router: &RouterType,
    invoice: &LxInvoice,
    fallback_amount: Option<Amount>,
) -> anyhow::Result<(Route, RouteParameters)>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    if invoice.amount().is_some() && fallback_amount.is_some() {
        // Not a serious error, but better to be unambiguous.
        debug_panic_release_log!(
            "Nit: Only provide fallback amount for amountless invoices",
        );
    }

    let payment_params = build_payment_params(Either::Right(invoice))
        .context("Couldn't build payment parameters")?;

    let amount = invoice
        .amount()
        .or(fallback_amount)
        .context("Missing fallback amount for amountless invoice")?;

    find_route_from_payment_params(
        channel_manager,
        router,
        payment_params,
        amount,
    )
}

/// Get a [`PaymentParameters`] from a payee or invoice in Lexe's default way.
///
/// LDK's builder API is unergonomic and hides a lot of details, so we
/// 'unbuilderify' it to make clear how each field modifies the final result.
pub fn build_payment_params(
    payee_pk_or_invoice: Either<NodePk, &LxInvoice>,
) -> anyhow::Result<PaymentParameters> {
    let maybe_invoice = payee_pk_or_invoice.right();

    let payee = {
        let payee_pubkey = match payee_pk_or_invoice {
            Either::Left(pk) => pk,
            Either::Right(invoice) => invoice.payee_node_pk(),
        };

        let route_hints = maybe_invoice
            .map(|invoice| invoice.0.route_hints())
            .unwrap_or_default();

        let features =
            maybe_invoice.and_then(|invoice| invoice.0.features().cloned());

        const_assert!(DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA <= u32::MAX as u64);
        let final_cltv_expiry_delta = match maybe_invoice {
            Some(invoice) => invoice.min_final_cltv_expiry_delta_u32()?,
            None => u32::try_from(DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA)
                .expect("Checked in const_assert"),
        };

        Payee::Clear {
            node_id: payee_pubkey.0,
            route_hints,
            features,
            final_cltv_expiry_delta,
        }
    };

    let expiry_time = match payee_pk_or_invoice {
        Either::Left(_) => None,
        Either::Right(invoice) =>
            Some(invoice.expires_at()?.into_duration().as_secs()),
    };

    Ok(PaymentParameters {
        payee,
        expiry_time,

        // Everything else uses LDK defaults. This is checked in tests.
        max_total_cltv_expiry_delta: DEFAULT_MAX_TOTAL_CLTV_EXPIRY_DELTA,
        max_path_count: DEFAULT_MAX_PATH_COUNT,
        max_path_length: MAX_PATH_LENGTH_ESTIMATE,
        max_channel_saturation_power_of_half: 2,
        previously_failed_channels: Vec::new(),
        previously_failed_blinded_path_idxs: Vec::new(),
    })
}

/// Finds a route from our node to the payee in the given [`PaymentParameters`]
/// for the given [`Amount`].
pub fn find_route_from_payment_params<CM, PS>(
    channel_manager: &CM,
    router: &RouterType,
    payment_params: PaymentParameters,
    amount: Amount,
) -> anyhow::Result<(Route, RouteParameters)>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // TODO(max): We may want to set a fee limit at some point
    let max_total_routing_fee_msat = None;
    let route_params = RouteParameters {
        payment_params,
        final_value_msat: amount.msat(),
        max_total_routing_fee_msat,
    };

    let route = {
        let payer_pubkey = channel_manager.get_our_node_id();
        let usable_channels = channel_manager.list_usable_channels();
        let usable_channels_refs = usable_channels.iter().collect::<Vec<_>>();
        let first_hops = Some(usable_channels_refs.as_slice());
        let in_flight_htlcs = channel_manager.compute_inflight_htlcs();
        router
            .find_route(
                &payer_pubkey,
                &route_params,
                first_hops,
                in_flight_htlcs,
            )
            .map_err(|LightningError { err, action: _ }| anyhow!("{err}"))?
    };

    Ok((route, route_params))
}

#[cfg(test)]
mod test {
    use common::{rng::FastRng, root_seed::RootSeed};

    use super::*;

    /// Compares our [`build_payment_params`] constructor with the values used
    /// in LDK's [`PaymentParameters::from_node_id`]. This test exists just so
    /// we can be notified if a default value changes in LDK.
    #[test]
    fn default_vs_ldk_constructor() {
        let mut rng = FastRng::from_u64(2838113);
        let seed = RootSeed::from_rng(&mut rng);
        let node_pk = seed.derive_node_pk(&mut rng);

        let lexe_payment_params =
            build_payment_params(Either::Left(node_pk)).unwrap();

        let min_final_cltv_expiry_delta =
            u32::try_from(DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA)
                .expect("Checked in const_assert");
        let ldk_payment_params = PaymentParameters::from_node_id(
            node_pk.0,
            min_final_cltv_expiry_delta,
        );

        assert_eq!(lexe_payment_params, ldk_payment_params);
    }
}
