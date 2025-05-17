//! Payment routing.

use std::{
    cmp::{max, min},
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, ensure};
use bitcoin::secp256k1;
use cfg_if::cfg_if;
use common::{
    api::user::{NodePk, Scid},
    cli::LspInfo,
    debug_panic_release_log,
    ln::{amount::Amount, invoice::LxInvoice},
    rng::SysRngDerefHack,
    time::DisplayMs,
};
use either::Either;
use lexe_std::const_assert;
use lightning::{
    blinded_path::payment::{
        BlindedPaymentPath, ForwardTlvs, PaymentConstraints,
        PaymentForwardNode, PaymentRelay, ReceiveTlvs,
    },
    ln::{channel_state::ChannelDetails, msgs::LightningError},
    routing::{
        router::{
            DefaultRouter, InFlightHtlcs, Payee, PaymentParameters, Route,
            RouteParameters, Router, MAX_PATH_LENGTH_ESTIMATE,
        },
        scoring::ProbabilisticScoringFeeParameters,
    },
    types::features::BlindedHopFeatures,
};
use lightning_invoice::{
    RouteHint, RouteHintHop, RoutingFees, DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA,
};
use rust_decimal_macros::dec;
use tracing::{debug, info};

use crate::{
    alias::{NetworkGraphType, ProbabilisticScorerType},
    logger::LexeTracingLogger,
    traits::{LexeChannelManager, LexePersister},
};

/// The default LDK [`Router`] impl with concrete Lexe types filled in.
type DefaultRouterType = DefaultRouter<
    Arc<NetworkGraphType>,
    LexeTracingLogger,
    SysRngDerefHack,
    Arc<Mutex<ProbabilisticScorerType>>,
    ProbabilisticScoringFeeParameters,
    ProbabilisticScorerType,
>;

/// The Lexe payment [`Router`] impl for both the Lexe LSP and our user nodes.
///
/// Ideally these variants would be separated into different impls, but the
/// resulting generics hell is absolutely not worth it.
pub enum LexeRouter {
    Node {
        default_router: DefaultRouterType,
        lsp_info: LspInfo,
        intercept_scids: Vec<Scid>,
    },
    Lsp {
        default_router: DefaultRouterType,
    },
}

impl LexeRouter {
    /// Create a new [`LexeRouter`] for user nodes.
    pub fn new_user_node(
        network_graph: Arc<NetworkGraphType>,
        logger: LexeTracingLogger,
        scorer: Arc<Mutex<ProbabilisticScorerType>>,
        lsp_info: LspInfo,
        intercept_scids: Vec<Scid>,
    ) -> Self {
        let default_router = DefaultRouter::new(
            network_graph,
            logger,
            SysRngDerefHack::new(),
            scorer,
            Self::default_scoring_fee_params(),
        );

        Self::Node {
            default_router,
            lsp_info,
            intercept_scids,
        }
    }

    /// Create a new [`LexeRouter`] for LSP.
    pub fn new_lsp(
        network_graph: Arc<NetworkGraphType>,
        logger: LexeTracingLogger,
        scorer: Arc<Mutex<ProbabilisticScorerType>>,
    ) -> Self {
        let default_router = DefaultRouter::new(
            network_graph,
            logger,
            SysRngDerefHack::new(),
            scorer,
            Self::default_scoring_fee_params(),
        );

        Self::Lsp { default_router }
    }

    fn default_scoring_fee_params() -> ProbabilisticScoringFeeParameters {
        ProbabilisticScoringFeeParameters::default()
    }

    fn default_router(&self) -> &DefaultRouterType {
        match self {
            LexeRouter::Node { default_router, .. } => default_router,
            LexeRouter::Lsp { default_router } => default_router,
        }
    }
}

impl Router for LexeRouter {
    // Finds a `Route` for a payment between the given `payer` and a payee (in
    // the `RouteParameters`).
    fn find_route(
        &self,
        payer: &secp256k1::PublicKey,
        route_params: &RouteParameters,
        first_hops: Option<&[&ChannelDetails]>,
        inflight_htlcs: InFlightHtlcs,
    ) -> Result<Route, LightningError> {
        // Just delegate to the default LDK impl.
        Router::find_route(
            self.default_router(),
            payer,
            route_params,
            first_hops,
            inflight_htlcs,
        )
    }

    // Create a blinded _payment_ path back to us with payment forwarding info
    // for the payer to route with. This is roughly analogous to a BOLT11
    // invoice route hint.
    fn create_blinded_payment_paths<
        T: secp256k1::Signing + secp256k1::Verification,
    >(
        &self,
        recipient: secp256k1::PublicKey,
        first_hops: Vec<ChannelDetails>,
        tlvs: ReceiveTlvs,
        _amount_msats: u64,
        secp_ctx: &secp256k1::Secp256k1<T>,
    ) -> Result<Vec<BlindedPaymentPath>, ()> {
        let result = match self {
            // Node => create a blinded path from LSP -> Node, that includes
            // - our node's magic intercept SCID
            // - the LSP's payment forwarding info
            Self::Node {
                lsp_info,
                intercept_scids,
                ..
            } => {
                // If there are multiple intercept scids, just pick the last
                // one, as it is likely the most recently generated.
                let intercept_scid =
                    intercept_scids.last().ok_or(()).inspect_err(|()| {
                        debug_panic_release_log!("No intercept SCID provided")
                    })?;

                // Build the last hop hint for the payer to route with
                let last_hop_hint =
                    LastHopHint::new(lsp_info, *intercept_scid, &first_hops);
                let offer_route_hints = last_hop_hint.offer_route_hints(&tlvs);

                BlindedPaymentPath::new(
                    &offer_route_hints,
                    recipient,
                    tlvs,
                    last_hop_hint.htlc_maximum_msat,
                    crate::constants::USER_MIN_FINAL_CLTV_EXPIRY_DELTA,
                    SysRngDerefHack::new(),
                    secp_ctx,
                )
            }
            // LSP => just a one-hop "blinded" path to us.
            Self::Lsp { .. } => BlindedPaymentPath::one_hop(
                recipient,
                tlvs,
                crate::constants::LSP_MIN_FINAL_CLTV_EXPIRY_DELTA,
                SysRngDerefHack::new(),
                secp_ctx,
            ),
        };

        result.map(|path| vec![path])
    }
}

/// Unified logic for building a last hop route hint from the LSP to a user node
/// for payers to route with. Supports both BOLT 11 invoices and BOLT 12 offers.
pub(crate) struct LastHopHint<'a> {
    lsp_info: &'a LspInfo,
    intercept_scid: Scid,
    base_fee_msat: u32,
    prop_fee_ppm: u32,
    htlc_minimum_msat: u64,
    htlc_maximum_msat: u64,
    cltv_expiry_delta: u16,
}

impl<'a> LastHopHint<'a> {
    /// Create a new last hop route hint for a payer to route a payment via the
    /// LSP to our user node.
    ///
    /// * base fee = max(base fee from channels, base fee from LSP)
    /// * prop fee = max(prop fee from channels, prop fee from LSP)
    /// * cltv delta = max(cltv delta from channels, cltv delta from LSP)
    /// * htlc min = min(htlc min from channels, htlc min from LSP)
    /// * htlc max = htlc max from LSP
    pub fn new(
        lsp_info: &'a LspInfo,
        scid: Scid,
        channels: &'a [ChannelDetails],
    ) -> Self {
        let base = Self {
            lsp_info,
            intercept_scid: scid,
            base_fee_msat: lsp_info.lsp_usernode_base_fee_msat,
            prop_fee_ppm: lsp_info.lsp_usernode_prop_fee_ppm,
            htlc_minimum_msat: lsp_info.htlc_minimum_msat,
            htlc_maximum_msat: lsp_info.htlc_maximum_msat,
            cltv_expiry_delta: lsp_info.cltv_expiry_delta,
        };

        channels.iter().fold(base, |acc, channel| {
            let fwd = &channel.counterparty.forwarding_info;

            // For the fee rates and CLTV delta to include in our route hint(s),
            // use the maximum of the values observed in our channels and the
            // LSP's configured value according to `LspInfo`, defaulting to the
            // `LspInfo` value if a value is not available from our channels.
            let base_fee_msat = max(
                acc.base_fee_msat,
                fwd.as_ref().map(|f| f.fee_base_msat).unwrap_or(0),
            );
            let prop_fee_ppm = max(
                acc.prop_fee_ppm,
                fwd.as_ref()
                    .map(|f| f.fee_proportional_millionths)
                    .unwrap_or(0),
            );
            let cltv_expiry_delta = max(
                acc.cltv_expiry_delta,
                fwd.as_ref().map(|f| f.cltv_expiry_delta).unwrap_or(0),
            );

            // Take the min HTLC minimum across all our channels and the LSP's
            // configured value, even though it's currently 1 msat everywhere.
            //
            // Rationale:
            // - If we have any channels open, we can most likely receive a
            //   value equal to the minimum of the `htlc_minimum_msat`s across
            //   our channels (unless we have absolutely 0 liquidity left).
            // - If we have no channels open, we have to use the LSP's
            //   configured value for JIT channels. This may come in play in a
            //   scerario where (1) Lexe *isn't* subsidizing channel open costs
            //   but (2) we haven't implemented Ark/Spark/etc for handling small
            //   amounts, and thus need the user's first receive to be beyond 3k
            //   sats or whatever the prevailing on-chain fee is. In this case,
            //   the JIT hint with a higher HTLC minimum would alert the sender
            //   that such a small payment is not routable.
            let htlc_minimum_msat = min(
                acc.htlc_minimum_msat,
                channel.inbound_htlc_minimum_msat.unwrap_or(u64::MAX),
            );

            // Our capacity to receive is effectively infinite, bounded only by
            // the largest HTLCs Lexe's LSP is willing to forward to us. An
            // alternative approach would set one intercept hint with the LSP's
            // HTLC maximum, with the remaining hints set to the largest
            // `inbound_capacity` amounts available in existing channels. But
            // we can't incentivize the sender to use our existing channels by
            // setting the feerate higher in the JIT hint, because this would
            // cause them to overpay fees if they actually do use the JIT hint.
            // Thus, we just uniformly use the LSP's configured HTLC maximum.
            let htlc_maximum_msat = acc.htlc_maximum_msat;

            Self {
                lsp_info: acc.lsp_info,
                intercept_scid: acc.intercept_scid,
                base_fee_msat,
                prop_fee_ppm,
                cltv_expiry_delta,
                htlc_minimum_msat,
                htlc_maximum_msat,
            }
        })
    }

    /// Return a BOLT 12 offer style last hop route hint.
    fn offer_route_hints(&self, tlvs: &ReceiveTlvs) -> Vec<PaymentForwardNode> {
        let max_cltv_expiry = tlvs
            .tlvs()
            .payment_constraints
            .max_cltv_expiry
            .saturating_add(u32::from(self.cltv_expiry_delta));

        let last_hop_hint = PaymentForwardNode {
            tlvs: ForwardTlvs {
                // Use our magic intercept SCID so LSP can JIT open channels
                short_channel_id: self.intercept_scid.0,
                payment_relay: PaymentRelay {
                    cltv_expiry_delta: self.cltv_expiry_delta,
                    // TODO(phlip9): don't charge payer Lexe fees, instead
                    // charge payee via skimmed value.
                    fee_proportional_millionths: self.prop_fee_ppm,
                    fee_base_msat: self.base_fee_msat,
                },
                payment_constraints: PaymentConstraints {
                    max_cltv_expiry,
                    htlc_minimum_msat: self.htlc_minimum_msat,
                },
                // TODO(phlip9): LDK value. do we need to get this from LSP?
                // why does LDK always set this to empty?
                features: BlindedHopFeatures::empty(),
                next_blinding_override: None,
            },
            node_id: self.lsp_info.node_pk.inner(),
            htlc_maximum_msat: self.htlc_maximum_msat,
        };
        vec![last_hop_hint]
    }

    /// Return a BOLT 11 invoice style last hop route hint.
    pub(crate) fn invoice_route_hints(&self) -> Vec<RouteHint> {
        let last_hop_hint = RouteHintHop {
            src_node_id: self.lsp_info.node_pk.inner(),
            short_channel_id: self.intercept_scid.0,
            // TODO(phlip9): don't charge payer Lexe fees, instead charge payee
            // via skimmed value.
            fees: RoutingFees {
                base_msat: self.base_fee_msat,
                proportional_millionths: self.prop_fee_ppm,
            },
            cltv_expiry_delta: self.cltv_expiry_delta,
            htlc_minimum_msat: Some(self.htlc_minimum_msat),
            htlc_maximum_msat: Some(self.htlc_maximum_msat),
        };
        vec![RouteHint(vec![last_hop_hint])]
    }
}

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
        router: &LexeRouter,
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
    num_usable_channels: Option<usize>,
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
            Some(invoice.expires_at()?.to_duration().as_secs()),
    };

    // Hard limit: Don't allow more MPP paths than our # of usable channels.
    // Default to max 5 paths if num_usable_channels isn't supplied.
    // let max_path_count = num_usable_channels
    //     .map(|num_usable| u8::try_from(num_usable).unwrap_or(255))
    //     .unwrap_or(5);
    //
    // TODO(max): Our MPP smoketests currently break if we use the code just
    // above (which is what we want). I've opened an issue for this:
    // https://github.com/lightningdevkit/rust-lightning/issues/3727
    //
    // two_shard_two_hop_max_sendable_to_existing_channel:
    // "Error: Tried to pay 490974.155 sats. The maximum amount that you can
    // route to this recipient is 392564 sats. Consider adding to your Lightning
    // balance or sending a smaller amount."
    let _ = num_usable_channels;
    let max_path_count = lightning::routing::router::DEFAULT_MAX_PATH_COUNT;

    // One week (measured in blocks). This is also LDK's default.
    let max_total_cltv_expiry_delta = 1008;

    // Allow payment paths to saturate the channel's usable capacity.
    // The default value is 2, meaning we only use up to 1/4th of a channel's
    // capacity. But users often have quite small channels of around 50k sats.
    // This means that a simple payment of 50k sats may require 4 paths or
    // more, which drastically decreases payment reliability.
    let max_channel_saturation_power_of_half = 0;

    Ok(PaymentParameters {
        payee,
        expiry_time,
        max_path_count,
        max_total_cltv_expiry_delta,
        max_channel_saturation_power_of_half,

        // Everything else uses LDK defaults.
        max_path_length: MAX_PATH_LENGTH_ESTIMATE,
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
    router: &LexeRouter,
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
