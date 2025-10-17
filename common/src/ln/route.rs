use std::{fmt, ops::Deref};

use lightning::{
    routing::{
        gossip::{NetworkGraph, NodeId},
        router::{BlindedTail, Path, Route, RouteHop},
    },
    util::logger::Logger,
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use super::{amount::Amount, node_alias::LxNodeAlias};
use crate::api::user::{NodePk, Scid};

/// Newtype for [`Route`].
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxRoute {
    /// The [`LxPath`]s taken for a single (possibly multi-path) payment.
    ///
    /// If no [`LxBlindedTail`]s are present, then the pubkey of the last
    /// [`LxRouteHop`] in each path must be the same.
    pub paths: Vec<LxPath>,
}

impl LxRoute {
    pub fn from_ldk<L: Deref<Target: Logger>>(
        route: Route,
        network_graph: &NetworkGraph<L>,
    ) -> Self {
        let mut paths = route
            .paths
            .into_iter()
            .map(LxPath::from)
            .collect::<Vec<_>>();

        // Annotate all hops with node aliases known from our network graph.
        {
            let locked_graph = network_graph.read_only();

            let lookup_alias = |node_pk: &NodePk| -> Option<LxNodeAlias> {
                let node_id = NodeId::from_pubkey(&node_pk.0);
                locked_graph
                    .node(&node_id)
                    .and_then(|node_info| node_info.announcement_info.as_ref())
                    .map(|ann_info| LxNodeAlias::from(*ann_info.alias()))
            };

            for path in paths.iter_mut() {
                for hop in path.hops.iter_mut() {
                    hop.node_alias = lookup_alias(&hop.node_pk);
                }
            }
        }

        Self { paths }
    }

    /// Return the total amount paid on this [`LxRoute`], excluding the fees.
    pub fn amount(&self) -> Amount {
        self.paths.iter().map(LxPath::amount).sum()
    }

    /// Return the total fees on this [`LxRoute`].
    pub fn fees(&self) -> Amount {
        self.paths.iter().map(LxPath::fees).sum()
    }
}

impl fmt::Display for LxRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num_paths = self.paths.len();
        for (i, path) in self.paths.iter().enumerate() {
            write!(f, "{path}")?;
            if i != num_paths - 1 {
                write!(f, " | ")?;
            }
        }
        Ok(())
    }
}

/// Newtype for [`Path`].
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxPath {
    /// The unblinded hops in this [`Path`]. Must be at least length one.
    pub hops: Vec<LxRouteHop>,
    /// The blinded path at which this path terminates, if present.
    pub blinded_tail: Option<LxBlindedTail>,
}

impl From<Path> for LxPath {
    fn from(path: Path) -> Self {
        LxPath {
            hops: path.hops.into_iter().map(LxRouteHop::from).collect(),
            blinded_tail: path.blinded_tail.map(LxBlindedTail::from),
        }
    }
}

impl fmt::Display for LxPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let amount = self.amount();
        let fees = self.fees();
        write!(f, "[{amount} sat, {fees} fees: ")?;
        let num_hops = self.hops.len();
        for (i, hop) in self.hops.iter().enumerate() {
            let hop_node_pk = hop.node_pk;
            match hop.node_alias {
                Some(alias) => write!(f, "({alias}) {hop_node_pk}")?,
                None => write!(f, "{hop_node_pk}")?,
            }
            if i != num_hops - 1 {
                write!(f, " -> ")?;
            }
        }
        if let Some(tail) = &self.blinded_tail {
            let num_hops = tail.num_hops;
            write!(f, " -> blinded tail with {num_hops} hops")?;
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl LxPath {
    /// Return the amount paid on this [`LxPath`], excluding the fees.
    pub fn amount(&self) -> Amount {
        match self.blinded_tail.as_ref() {
            Some(tail) => tail.final_value,
            None => self
                .hops
                .last()
                .map_or(Amount::ZERO, |hop| hop.fee_or_amount),
        }
    }

    /// Gets the fees on this [`Path`], excluding any excess fees paid to the
    /// recipient.
    pub fn fees(&self) -> Amount {
        match &self.blinded_tail {
            // There is a blinded tail:
            // - Non-last hops are fees
            // - Last hop is the fee for the entire blinded path.
            // => Sum `fee_or_amount` over all hops.
            Some(_) => self
                .hops
                .iter()
                .map(|hop| hop.fee_or_amount)
                .sum::<Amount>(),
            // There is no blinded tail:
            // - Non-last hops are fees
            // - Last hop is the amount paid, so it should be ignored
            None => match self.hops.split_last() {
                Some((_last, non_last)) =>
                    non_last.iter().map(|hop| hop.fee_or_amount).sum::<Amount>(),
                None => Amount::ZERO,
            },
        }
    }
}

/// Newtype for [`RouteHop`].
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxRouteHop {
    /// The node_id of the node at this hop.
    pub node_pk: NodePk,
    /// The alias of the node at this hop, if known.
    pub node_alias: Option<LxNodeAlias>,
    /// The channel used from the previous hop to reach this node.
    pub scid: Scid,
    /// If this is NOT the last hop in [`LxPath::hops`], this is the fee taken
    /// on this hop (for paying for the use of the *next* channel in the path).
    ///
    /// If this IS the last hop in [`LxPath::hops`]:
    /// - If we're sending to a blinded payment path, this is the fee paid for
    ///   use of the entire blinded path.
    /// - Otherwise, this is the amount of this [`LxPath`]'s part of the
    ///   payment.
    pub fee_or_amount: Amount,
    /// Whether we believe this channel is announced in the public graph.
    pub announced: bool,
}

impl From<RouteHop> for LxRouteHop {
    fn from(hop: RouteHop) -> Self {
        Self {
            node_pk: NodePk(hop.pubkey),
            node_alias: None,
            scid: Scid(hop.short_channel_id),
            fee_or_amount: Amount::from_msat(hop.fee_msat),
            announced: hop.maybe_announced_channel,
        }
    }
}

/// Newtype for [`BlindedTail`].
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxBlindedTail {
    pub num_hops: usize,
    /// The total amount paid on this [`LxPath`], excluding the fees.
    pub final_value: Amount,
}

impl From<BlindedTail> for LxBlindedTail {
    fn from(tail: BlindedTail) -> Self {
        Self {
            num_hops: tail.hops.len(),
            final_value: Amount::from_msat(tail.final_value_msat),
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::prelude::any;

    use super::*;
    use crate::{
        rng::{FastRng, SysRng},
        test_utils::arbitrary,
    };

    /// Check the [`fmt::Display`] implementation of [`LxRoute`].
    ///
    /// $ cargo test -p common display_route -- --ignored --nocapture
    #[ignore]
    #[test]
    fn display_route() {
        let mut rng = FastRng::from_sysrng(&mut SysRng::new());
        let mut route = arbitrary::gen_value(&mut rng, any::<LxRoute>());
        route.paths.truncate(3);
        for path in route.paths.iter_mut() {
            path.hops.truncate(3);
        }
        println!("Route: {route}");
    }
}
