//! Routing logic.

use anyhow::{anyhow, ensure};
use cfg_if::cfg_if;
use common::{
    api::user::NodePk,
    ln::{amount::Amount, invoice::LxInvoice},
    time::DisplayMs,
};
use const_utils::const_assert;
use either::Either;
use lightning::{
    ln::{channel_state::ChannelDetails, msgs::LightningError},
    routing::router::{
        InFlightHtlcs, Payee, PaymentParameters, Route, RouteParameters,
        Router, DEFAULT_MAX_PATH_COUNT, DEFAULT_MAX_TOTAL_CLTV_EXPIRY_DELTA,
        MAX_PATH_LENGTH_ESTIMATE,
    },
};
use lightning_invoice::DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA;
use rust_decimal_macros::dec;
use tracing::{debug, info};

use crate::{
    alias::RouterType,
    traits::{LexeChannelManager, LexePersister},
};

/// Amount-agnostic context for routing to a known payee.
/// (The payee is specified in [`PaymentParameters`]).
///
/// - Can be reused for multiple routing attempts to the given payee for a
///   single payment, e.g. within [`compute_max_flow_to_recipient`].
/// - However, since this caches `usable_channels` and `in_flight_htlcs`, it
///   should not be reused for multiple payments, nor for other payees.
pub struct RoutingContext {
    payment_params: PaymentParameters,
    payer_pk: NodePk,
    usable_channels: Vec<ChannelDetails>,
    in_flight_htlcs: InFlightHtlcs,
}

impl RoutingContext {
    pub fn from_payment_params<CM, PS>(
        channel_manager: &CM,
        payment_params: PaymentParameters,
    ) -> Self
    where
        CM: LexeChannelManager<PS>,
        PS: LexePersister,
    {
        let payer_pk = NodePk(channel_manager.get_our_node_id());
        let usable_channels = channel_manager.list_usable_channels();
        let in_flight_htlcs = channel_manager.compute_inflight_htlcs();

        Self {
            payment_params,

            payer_pk,
            usable_channels,
            in_flight_htlcs,
        }
    }

    pub fn find_route(
        &self,
        router: &RouterType,
        amount: Amount,
    ) -> anyhow::Result<(Route, RouteParameters)> {
        // TODO(max): We may want to set a fee limit at some point
        let max_total_routing_fee_msat = None;
        let route_params = RouteParameters {
            payment_params: self.payment_params.clone(),
            final_value_msat: amount.msat(),
            max_total_routing_fee_msat,
        };

        let usable_channels_refs =
            self.usable_channels.iter().collect::<Vec<_>>();
        let first_hops = Some(usable_channels_refs.as_slice());

        let route = router
            .find_route(
                &self.payer_pk.0,
                &route_params,
                first_hops,
                self.in_flight_htlcs.clone(),
            )
            .map_err(|LightningError { err, action: _ }| anyhow!("{err}"))?;

        Ok((route, route_params))
    }
}

/// Get a [`PaymentParameters`] from a payee or invoice in Lexe's default way.
/// Payment parameters are amount-agnostic.
// LDK's builder API is unergonomic and hides a lot of details, so we
// 'unbuilderify' it to make clear how each field modifies the final result.
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

/// Computes an accurate estimate of the maximum amount that is sendable to
/// this recipient specified in the given [`PaymentParameters`], given a
/// `starting_amount` which failed to find a route.
///
/// The estimate here is much more accurate than `max_sendable` since the
/// recipient is known.
// - TODO(max): We should actually compute the max flow to this recipient, but
//   there are significant complications because `Route` may be multi-path and
//   each `Path` does not expose the prop and base fee for each hop, so we'd
//   have to munge around the network graph. A dumb binary search should be good
//   enough to unblock us for now.
// - TODO(max): Expose an endpoint which computes the max flow and simply
//   returns it to the caller. We can then call this endpoint while the user is
//   presented with an amountless invoice to suggest an accurate maximum amount,
//   which they can also pay using a 'send all' button.
// - TODO(max): We should build out some prop tests for this function:
//   - This function never panics
//   - satoshi precision: For all "for all network graphs, for any two distinct
//     nodes, if this function finds a `max_flow`, routing to `max_flow` + 1
//     should fail"
//  - `max_flow` to a neighbor is just sum of `next_outbound_htlc_limit` over
//    all channels with this neighbor.
pub async fn compute_max_flow_to_recipient(
    router: &RouterType,
    routing_context: &RoutingContext,
    starting_amount: Amount,
) -> anyhow::Result<Amount> {
    cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            // Use satoshi-precise values in tests for better debugging.
            const MAX_ITERATIONS: usize = 30;
        } else {
            /// Max # of binary search iterations.
            // 10 iterations allows us to search ~1M amounts to an accuracy of
            // about 1000 sat.
            //
            // Some timing samples from prod (mainnet):
            // - best_succ=64604: 17 iters, 50.052s (2025-03-18)
            // - best_succ=87681: 17 iters, 47.668s (2025-03-18)
            const MAX_ITERATIONS: usize = 10;
        }
    }
    let start = tokio::time::Instant::now();

    info!(%starting_amount, "Computing max flow");

    let one_sat = Amount::from_sats_u32(1);

    ensure!(
        starting_amount >= one_sat,
        "`starting_amount` must be non-zero for binary search"
    );

    let mut low = one_sat;
    let mut high = starting_amount.round_sat();
    let mut best_succ: Option<Amount> = None;
    let mut last_err = anyhow!("Placeholder: initial error");

    let mut iter = 1;
    loop {
        debug!(%iter, %low, %high, ?best_succ, %last_err, "Max flow iteration");

        if low == high {
            break;
        }

        let mid = (low + high) / dec!(2);

        let route_result = routing_context.find_route(router, mid);

        match route_result {
            Ok(_) => {
                // Successfully routed mid, store succ and try larger
                best_succ = Some(mid);
                low = mid.saturating_add(one_sat).min(high);
            }
            Err(e) => {
                // Could not route mid, store error and try smaller
                last_err = e;
                high = mid.saturating_sub(one_sat).max(low);
            }
        }

        if iter >= MAX_ITERATIONS {
            break;
        } else {
            iter += 1;

            // Each route found takes a few seconds.
            // Yield so that we don't starve other tasks of CPU time.
            tokio::task::yield_now().await;
        }
    }

    let max_flow_result = match best_succ {
        Some(succ) => Ok(succ.floor_sat()),
        // No route found at all
        None => Err(last_err),
    };

    let elapsed_ms = DisplayMs(start.elapsed());
    info!("Max flow result ({iter} iters) <{elapsed_ms}>: {max_flow_result:?}");

    max_flow_result
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
