use std::time::Duration;

use lexe_api::models::lsp::ExternalChannel;
use lexe_common::api::user::{NodePk, Scid};
use lightning::{
    io,
    routing::{
        gossip::NodeId,
        router::{CandidateRouteHop, Path},
        scoring::{
            ChannelUsage, ProbabilisticScoringFeeParameters, ScoreLookUp,
            ScoreUpdate,
        },
    },
    util::ser::{Writeable, Writer},
};

use crate::{alias::ProbabilisticScorerType, route};

/// Lexe's payment route scorer for both the LSP and user nodes.
///
/// For the LSP this is just a [`ProbabilisticScorer`].
///
/// User nodes additionally store the latest observed LSP external channel
/// liquidity to improve outbound payment reliability. We observed frequent
/// payment failures from user nodes routing payment attempts over heavily
/// skewed LSP channels. The scorer state for these channels also recovered
/// quite slowly even if the channel was rebalanced.
///
/// Instead, we can just have the user nodes periodically fetch the actual,
/// authoritative LSP external channel liquidity _directly from the LSP_ and
/// use that when routing.
///
/// [`ProbabilisticScorer`]: lightning::routing::scoring::ProbabilisticScorer
pub struct LexeScorer {
    inner: ProbabilisticScorerType,
    /// `Some` on user nodes.
    lsp: Option<Lsp>,
}

/// A user node's LSP, whose outbound hops are limited by its liquidity.
struct Lsp {
    /// For comparing against `CandidateRouteHop::source`.
    node_id: NodeId,
    /// For comparing against `RouteHop::pubkey`, via `eq_fast_unstable`.
    node_pk: NodePk,
    liquidity: LspLiquidity,
}

/// The LSP's outbound liquidity over each of its live announced channels.
/// Local path events correct it until the next update replaces it.
pub struct LspLiquidity {
    /// Dense, so route-finding can scan it quickly.
    scids: Vec<Scid>,
    /// `peers[i]` and `limits_msat[i]` belong to `scids[i]`.
    peers: Vec<NodeId>,
    limits_msat: Vec<u64>,
}

// --- impl LexeScorer --- //

impl LexeScorer {
    /// Create a new [`LexeScorer`] for user nodes.
    pub fn new_user_node(
        inner: ProbabilisticScorerType,
        lsp_node_pk: &NodePk,
        lsp_liquidity: LspLiquidity,
    ) -> Self {
        let lsp = Lsp {
            node_id: NodeId::from_pubkey(&lsp_node_pk.0),
            node_pk: *lsp_node_pk,
            liquidity: lsp_liquidity,
        };
        Self {
            inner,
            lsp: Some(lsp),
        }
    }

    /// Create a new [`LexeScorer`] for the LSP.
    pub fn new_lsp(inner: ProbabilisticScorerType) -> Self {
        Self { inner, lsp: None }
    }

    /// Replaces the LSP's liquidity, discarding local corrections.
    pub fn set_lsp_liquidity(&mut self, lsp_liquidity: LspLiquidity) {
        if let Some(lsp) = &mut self.lsp {
            lsp.liquidity = lsp_liquidity;
        }
    }
}

impl ScoreLookUp for LexeScorer {
    type ScoreParams = ProbabilisticScoringFeeParameters;

    fn channel_penalty_msat(
        &self,
        candidate: &CandidateRouteHop,
        usage: ChannelUsage,
        params: &Self::ScoreParams,
    ) -> u64 {
        self.inner.channel_penalty_msat(candidate, usage, params)
    }

    // Beware that LDK requires a consistent value from `channel_liquidity_msat`
    // during a route search. In our case, the router holds a lock on the
    // scorer during routing.
    //
    // NOTE(phlip9): this trait method comes from our LDK fork, not upstream
    fn channel_liquidity_msat(
        &self,
        candidate: &CandidateRouteHop,
    ) -> Option<u64> {
        if let Some(lsp) = &self.lsp
            && let CandidateRouteHop::PublicHop(hop) = candidate
            && hop.info.source() == &lsp.node_id
        {
            return Some(lsp.liquidity.limit_msat(hop.short_channel_id));
        }
        None
    }
}

impl ScoreUpdate for LexeScorer {
    fn payment_path_failed(
        &mut self,
        path: &Path,
        short_channel_id: u64,
        duration_since_epoch: Duration,
    ) {
        self.inner.payment_path_failed(
            path,
            short_channel_id,
            duration_since_epoch,
        );

        // Payment failed at a hop leaving the LSP. LDK forwards non-strictly,
        // so none of the LSP's channels to this peer could carry the amount.
        if let Some(lsp) = &mut self.lsp
            && let Some((scid, peer, amount_msat)) = lsp.outbound_hop(path)
            && scid == short_channel_id
        {
            let limit_msat = amount_msat.saturating_sub(1);
            lsp.liquidity.cap_peer(&peer, limit_msat);
        }
    }

    fn payment_path_successful(
        &mut self,
        path: &Path,
        duration_since_epoch: Duration,
    ) {
        self.inner
            .payment_path_successful(path, duration_since_epoch);

        // The amount moved to the LSP's peer.
        if let Some(lsp) = &mut self.lsp
            && let Some((_scid, peer, amount_msat)) = lsp.outbound_hop(path)
        {
            lsp.liquidity.debit_peer(&peer, amount_msat);
        }
    }

    fn probe_failed(
        &mut self,
        path: &Path,
        short_channel_id: u64,
        duration_since_epoch: Duration,
    ) {
        self.inner
            .probe_failed(path, short_channel_id, duration_since_epoch);
    }

    // Also used for payments the destination rejected. Those HTLCs were
    // failed back, so the LSP's liquidity is unchanged.
    fn probe_successful(
        &mut self,
        path: &Path,
        duration_since_epoch: Duration,
    ) {
        self.inner.probe_successful(path, duration_since_epoch);
    }

    fn time_passed(&mut self, duration_since_epoch: Duration) {
        self.inner.time_passed(duration_since_epoch);
    }
}

/// Only the [`ProbabilisticScorer`] is serialized.
///
/// [`ProbabilisticScorer`]: lightning::routing::scoring::ProbabilisticScorer
impl Writeable for LexeScorer {
    fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
        self.inner.write(writer)
    }
}

// --- impl Lsp --- //

impl Lsp {
    /// The `(scid, peer, amount_msat)` of the hop leaving the LSP, if `path`
    /// starts at the LSP (always the case for user nodes).
    fn outbound_hop(&self, path: &Path) -> Option<(u64, NodeId, u64)> {
        let (first_hop, rest) = path.hops.split_first()?;
        if !first_hop.pubkey.eq_fast_unstable(&self.node_pk.0) {
            return None;
        }
        let hop = rest.first()?;
        let peer = NodeId::from_pubkey(&hop.pubkey);
        let amount_msat = route::hop_amount_msat(path, 1)?;
        Some((hop.short_channel_id, peer, amount_msat))
    }
}

// --- impl LspLiquidity --- //

impl LspLiquidity {
    pub const fn empty() -> Self {
        Self {
            scids: Vec::new(),
            peers: Vec::new(),
            limits_msat: Vec::new(),
        }
    }

    pub fn from_external_channels(channels: &[ExternalChannel]) -> Self {
        let scids = channels
            .iter()
            .map(|channel| channel.scid)
            .collect::<Vec<_>>();
        let peers = channels
            .iter()
            .map(|channel| NodeId::from_pubkey(&channel.counterparty_node_id.0))
            .collect::<Vec<_>>();
        let limits_msat = channels
            .iter()
            .map(|channel| channel.next_outbound_htlc_limit.msat())
            .collect::<Vec<_>>();
        Self {
            scids,
            peers,
            limits_msat,
        }
    }

    /// The LSP's outbound limit over `scid`. Zero if the channel is closed or
    /// its peer is disconnected.
    fn limit_msat(&self, scid: u64) -> u64 {
        self.scids
            .iter()
            .position(|channel_scid| channel_scid.0 == scid)
            .map_or(0, |idx| self.limits_msat[idx])
    }

    /// Lowers the limit of each channel to `peer` to at most `limit_msat`.
    fn cap_peer(&mut self, peer: &NodeId, limit_msat: u64) {
        let peer_limits_msat = self
            .peers
            .iter()
            .zip(&mut self.limits_msat)
            .filter(|(channel_peer, _)| *channel_peer == peer);
        for (_, channel_limit_msat) in peer_limits_msat {
            *channel_limit_msat = (*channel_limit_msat).min(limit_msat);
        }
    }

    /// Debits `amount_msat` from the channel to `peer` which LDK's non-strict
    /// forwarding would pick: the one with the least liquidity that fits.
    fn debit_peer(&mut self, peer: &NodeId, amount_msat: u64) {
        let picked_limit_msat = self
            .peers
            .iter()
            .zip(&mut self.limits_msat)
            .filter(|(channel_peer, limit_msat)| {
                *channel_peer == peer && **limit_msat >= amount_msat
            })
            .min_by_key(|(_, limit_msat)| **limit_msat);
        if let Some((_, limit_msat)) = picked_limit_msat {
            *limit_msat -= amount_msat;
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use bitcoin::{constants::ChainHash, secp256k1::PublicKey};
    use lexe_common::{ln::amount::Amount, root_seed::RootSeed};
    use lightning::{
        ln::msgs::UnsignedChannelUpdate,
        routing::{
            router::{
                self, PaymentParameters, Route, RouteHop, RouteParameters,
            },
            scoring::ProbabilisticScorer,
        },
        types::features::{
            Bolt11InvoiceFeatures, ChannelFeatures, NodeFeatures,
        },
    };

    use super::*;
    use crate::{
        alias::NetworkGraphType, constants::LEXE_SCORER_PARAMS,
        logger::LexeTracingLogger,
    };

    const AMOUNT_MSAT: u64 = 100_000;

    // user --1-> LSP --2-> X --4-> payee
    //               \--6-> X      /
    //               \--3-> Y --5--/
    //
    // By fee: LSP -> X over 2 < over 6 < LSP -> Y.
    const USER_LSP: u64 = 1;
    const LSP_X: u64 = 2;
    const LSP_X2: u64 = 6;
    const LSP_Y: u64 = 3;
    const X_PAYEE: u64 = 4;
    const Y_PAYEE: u64 = 5;

    struct Harness {
        graph: Arc<NetworkGraphType>,
        scorer: LexeScorer,
    }

    impl Harness {
        fn new(lsp_channels: &[(u64, PublicKey, u64)]) -> Self {
            let logger = LexeTracingLogger::new();
            let graph = Arc::new(NetworkGraphType::new(
                bitcoin::Network::Regtest,
                logger.clone(),
            ));
            // Only the LSP's outbound hops are narrow.
            add_channel(&graph, USER_LSP, user(), lsp(), 0, 10_000_000);
            add_channel(&graph, LSP_X, lsp(), x(), 1_000, 1_000_000);
            add_channel(&graph, LSP_X2, lsp(), x(), 2_000, 1_000_000);
            add_channel(&graph, LSP_Y, lsp(), y(), 5_000, 1_000_000);
            add_channel(&graph, X_PAYEE, x(), payee(), 100, 10_000_000);
            add_channel(&graph, Y_PAYEE, y(), payee(), 100, 10_000_000);

            let inner = ProbabilisticScorer::new(
                LEXE_SCORER_PARAMS,
                graph.clone(),
                logger,
            );
            let scorer = LexeScorer::new_user_node(
                inner,
                &NodePk(lsp()),
                lsp_liquidity(lsp_channels),
            );

            Self { graph, scorer }
        }

        fn set_lsp_channels(&mut self, lsp_channels: &[(u64, PublicKey, u64)]) {
            self.scorer.set_lsp_liquidity(lsp_liquidity(lsp_channels));
        }

        fn limit_msat(&self, scid: u64) -> u64 {
            let lsp = self.scorer.lsp.as_ref().unwrap();
            lsp.liquidity.limit_msat(scid)
        }

        fn find_route(&self) -> Result<Route, &'static str> {
            let payment_params = PaymentParameters::from_node_id(payee(), 40);
            self.find_route_with(payment_params, AMOUNT_MSAT)
        }

        /// Finds a multi-path route, saturating channels if needed.
        fn find_mpp_route(
            &self,
            amount_msat: u64,
        ) -> Result<Route, &'static str> {
            let mut features = Bolt11InvoiceFeatures::empty();
            features.set_variable_length_onion_required();
            features.set_payment_secret_required();
            features.set_basic_mpp_optional();
            let mut payment_params =
                PaymentParameters::from_node_id(payee(), 40)
                    .with_bolt11_features(features)
                    .unwrap();
            payment_params.max_channel_saturation_power_of_half = 0;
            self.find_route_with(payment_params, amount_msat)
        }

        fn find_route_with(
            &self,
            payment_params: PaymentParameters,
            amount_msat: u64,
        ) -> Result<Route, &'static str> {
            let route_params = RouteParameters::from_payment_params_and_value(
                payment_params,
                amount_msat,
            );
            router::find_route(
                &user(),
                &route_params,
                &self.graph,
                None,
                LexeTracingLogger::new(),
                &self.scorer,
                &ProbabilisticScoringFeeParameters::default(),
                &[0; 32],
            )
        }

        /// The scid of the LSP outbound hop in the found route.
        fn routed_lsp_scid(&self) -> Option<u64> {
            let route = self.find_route().ok()?;
            assert_eq!(route.paths.len(), 1);
            Some(route.paths[0].hops[1].short_channel_id)
        }
    }

    #[test]
    fn routes_with_lsp_liquidity() {
        // Plenty of liquidity to X.
        let mut h =
            Harness::new(&[(LSP_X, x(), 1_000_000), (LSP_Y, y(), 1_000_000)]);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_X));

        // LSP -> X is drained.
        h.set_lsp_channels(&[(LSP_X, x(), 50_000), (LSP_Y, y(), 1_000_000)]);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_Y));

        // LSP -> X over 2 is closed, but over 6 is live.
        h.set_lsp_channels(&[
            (LSP_X2, x(), 1_000_000),
            (LSP_Y, y(), 1_000_000),
        ]);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_X2));

        // X is disconnected.
        h.set_lsp_channels(&[(LSP_Y, y(), 1_000_000)]);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_Y));

        // LSP -> X over 2 is drained, but over 6 has liquidity.
        h.set_lsp_channels(&[
            (LSP_X, x(), 0),
            (LSP_X2, x(), 1_000_000),
            (LSP_Y, y(), 1_000_000),
        ]);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_X2));

        // Everything is drained.
        h.set_lsp_channels(&[(LSP_X, x(), 0), (LSP_Y, y(), 0)]);
        assert_eq!(h.routed_lsp_scid(), None);
    }

    #[test]
    fn path_events_correct_lsp_liquidity() {
        let mut h = Harness::new(&[
            (LSP_X, x(), 1_000_000),
            (LSP_X2, x(), 300_000),
            (LSP_Y, y(), 1_000_000),
        ]);
        let path = h.find_route().unwrap().paths.remove(0);
        assert_eq!(path.hops[1].short_channel_id, LSP_X);
        let now = Duration::from_secs(1_000_000);

        // Amount sent over LSP -> X, incl. X's fee.
        let amount_msat = route::hop_amount_msat(&path, 1).unwrap();
        assert!(amount_msat > AMOUNT_MSAT);

        // Success: LDK forwards over X's channel with the least liquidity that
        // fits, i.e. over 6, not the routed 2.
        h.scorer.payment_path_successful(&path, now);
        assert_eq!(h.limit_msat(LSP_X), 1_000_000);
        assert_eq!(h.limit_msat(LSP_X2), 300_000 - amount_msat);

        // Destination rejected the payment: no change.
        h.scorer.probe_successful(&path, now);
        assert_eq!(h.limit_msat(LSP_X), 1_000_000);
        assert_eq!(h.limit_msat(LSP_X2), 300_000 - amount_msat);

        // Failed downstream of X: no change.
        h.scorer.payment_path_failed(&path, X_PAYEE, now);
        assert_eq!(h.limit_msat(LSP_X), 1_000_000);
        assert_eq!(h.limit_msat(LSP_X2), 300_000 - amount_msat);

        // Failed leaving the LSP: no channel to X could carry the amount.
        h.scorer.payment_path_failed(&path, LSP_X, now);
        assert_eq!(h.limit_msat(LSP_X), amount_msat - 1);
        assert_eq!(h.limit_msat(LSP_X2), amount_msat - 1);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_Y));

        // Only X is affected.
        assert_eq!(h.limit_msat(LSP_Y), 1_000_000);

        // The next update replaces local corrections.
        h.set_lsp_channels(&[(LSP_X, x(), 1_000_000), (LSP_Y, y(), 1_000_000)]);
        assert_eq!(h.routed_lsp_scid(), Some(LSP_X));
    }

    #[test]
    fn ignores_paths_without_lsp_outbound_hop() {
        let mut h =
            Harness::new(&[(LSP_X, x(), 1_000_000), (LSP_Y, y(), 1_000_000)]);
        let now = Duration::from_secs(1_000_000);

        // Paying the LSP directly: no hop leaves the LSP.
        let path = Path {
            hops: vec![route_hop(lsp(), USER_LSP, AMOUNT_MSAT)],
            blinded_tail: None,
        };
        assert_eq!(route::hop_amount_msat(&path, 2), None);
        let lsp = h.scorer.lsp.as_ref().unwrap();
        assert_eq!(lsp.outbound_hop(&path), None);

        h.scorer.payment_path_successful(&path, now);
        h.scorer.payment_path_failed(&path, USER_LSP, now);
        h.scorer.probe_successful(&path, now);
        assert_eq!(h.limit_msat(LSP_X), 1_000_000);
        assert_eq!(h.limit_msat(LSP_Y), 1_000_000);
    }

    #[test]
    fn splits_across_partially_drained_lsp_peers() {
        // X is cheaper but only has 600k, so the rest must go via Y.
        let mut h =
            Harness::new(&[(LSP_X, x(), 600_000), (LSP_Y, y(), 1_000_000)]);
        let route = h.find_mpp_route(1_500_000).unwrap();
        assert_eq!(route.get_total_amount(), 1_500_000);

        let mut lsp_hop_amounts = route
            .paths
            .iter()
            .map(|path| {
                let scid = path.hops[1].short_channel_id;
                (scid, route::hop_amount_msat(path, 1).unwrap())
            })
            .collect::<Vec<_>>();
        lsp_hop_amounts.sort();
        let [(LSP_X, x_msat), (LSP_Y, y_msat)] = lsp_hop_amounts[..] else {
            panic!(
                "Expected one part via each of X and Y: {lsp_hop_amounts:?}"
            );
        };
        assert!(x_msat <= 600_000);
        assert!(y_msat <= 1_000_000);

        // Not enough liquidity across the LSP's peers.
        h.set_lsp_channels(&[(LSP_X, x(), 300_000), (LSP_Y, y(), 1_000_000)]);
        assert!(h.find_mpp_route(1_500_000).is_err());
    }

    #[test]
    fn respects_each_channel_to_a_peer() {
        // X has 1M over 2 but nothing over 6, so the rest of a 1.5M payment
        // must go via Y, even though 6 is cheaper.
        let h = Harness::new(&[
            (LSP_X, x(), 1_000_000),
            (LSP_X2, x(), 0),
            (LSP_Y, y(), 1_000_000),
        ]);
        let route = h.find_mpp_route(1_500_000).unwrap();
        assert_eq!(route.get_total_amount(), 1_500_000);
        let lsp_scids = route
            .paths
            .iter()
            .map(|path| path.hops[1].short_channel_id)
            .collect::<Vec<_>>();
        assert!(lsp_scids.contains(&LSP_Y), "{lsp_scids:?}");
        assert!(!lsp_scids.contains(&LSP_X2), "{lsp_scids:?}");
    }

    /// Builds [`LspLiquidity`] from `(scid, peer, limit_msat)`s.
    fn lsp_liquidity(channels: &[(u64, PublicKey, u64)]) -> LspLiquidity {
        let channels = channels
            .iter()
            .map(|&(scid, peer, limit_msat)| ExternalChannel {
                scid: Scid(scid),
                counterparty_node_id: NodePk(peer),
                next_outbound_htlc_limit: Amount::from_msat(limit_msat),
            })
            .collect::<Vec<_>>();
        LspLiquidity::from_external_channels(&channels)
    }

    fn user() -> PublicKey {
        node_pk(0)
    }
    fn lsp() -> PublicKey {
        node_pk(1)
    }
    fn x() -> PublicKey {
        node_pk(2)
    }
    fn y() -> PublicKey {
        node_pk(3)
    }
    fn payee() -> PublicKey {
        node_pk(4)
    }

    fn node_pk(id: u64) -> PublicKey {
        RootSeed::from_u64(id).derive_node_pk().0
    }

    fn route_hop(pubkey: PublicKey, scid: u64, fee_msat: u64) -> RouteHop {
        RouteHop {
            pubkey,
            node_features: NodeFeatures::empty(),
            short_channel_id: scid,
            channel_features: ChannelFeatures::empty(),
            fee_msat,
            cltv_expiry_delta: 40,
            maybe_announced_channel: true,
        }
    }

    /// Adds a channel with the same policy in both directions.
    fn add_channel(
        graph: &NetworkGraphType,
        scid: u64,
        node_1: PublicKey,
        node_2: PublicKey,
        fee_base_msat: u32,
        htlc_maximum_msat: u64,
    ) {
        // LDK rejects timestamps too far from the current time.
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;
        graph
            .add_channel_from_partial_announcement(
                scid,
                None,
                u64::from(timestamp),
                ChannelFeatures::empty(),
                NodeId::from_pubkey(&node_1),
                NodeId::from_pubkey(&node_2),
            )
            .unwrap();

        for channel_flags in [0, 1] {
            graph
                .update_channel_unsigned(&UnsignedChannelUpdate {
                    chain_hash: ChainHash::using_genesis_block(
                        bitcoin::Network::Regtest,
                    ),
                    short_channel_id: scid,
                    timestamp,
                    message_flags: 1,
                    channel_flags,
                    cltv_expiry_delta: 40,
                    htlc_minimum_msat: 1,
                    htlc_maximum_msat,
                    fee_base_msat,
                    fee_proportional_millionths: 0,
                    excess_data: Vec::new(),
                })
                .unwrap();
        }
    }
}
