//! Logic to anonymize payment paths before reporting to Lexe LSP.

use std::collections::HashSet;

use lexe_common::{debug_panic_release_log, time::DisplayMs};
use lexe_ln::alias::NetworkGraphType;
use lightning::{
    events::{Event, PathFailure},
    routing::{
        gossip::{NetworkUpdate, NodeId, ReadOnlyNetworkGraph},
        router::Path,
    },
};
use tokio::time::Instant;
use tracing::info;

/// The minimum size of the anonymity set of possible receivers after a
/// payment path has been anonymized.
///
/// Intended to be small enough so that most LSPs can qualify as the (N-1)th
/// hop, but large enough to provide good privacy.
// TODO(max): Increase to 50 or 100 once we have more reliable payments.
const MIN_ANONYMITY_SET_SIZE: usize = 20;
/// The maximum # of hops we'll explore from the departure node.
/// Mostly just a safeguard against a bug causing an infinite loop.
const MAX_DEPTH: usize = 5;

/// Anonymizes a [`Event::PaymentPathSuccessful`].
pub(crate) fn successful_path(
    network_graph: &NetworkGraphType,
    payment_id: lightning::ln::channelmanager::PaymentId,
    payment_hash: Option<lightning::types::payment::PaymentHash>,
    path: Path,
    hold_times: Vec<u32>,
) -> Option<Event> {
    anonymize_path(network_graph, path).map(|path| {
        Event::PaymentPathSuccessful {
            payment_id,
            payment_hash,
            path,
            hold_times,
        }
    })
}

/// Anonymizes a [`Event::PaymentPathFailed`].
pub(crate) fn failed_path(
    network_graph: &NetworkGraphType,
    payment_id: Option<lightning::ln::channelmanager::PaymentId>,
    payment_hash: lightning::types::payment::PaymentHash,
    payment_failed_permanently: bool,
    failure: PathFailure,
    path: Path,
    short_channel_id: Option<u64>,
    hold_times: Vec<u32>,
) -> Option<Event> {
    let path = anonymize_path(network_graph, path)?;

    // So that we don't penalize a subset of the path which was not the
    // cause of the payment failure, as well as to not blow our privacy,
    // ensure that the failed channel or node is on the anonymized path.
    #[allow(clippy::collapsible_match)] // Suggestion is less readable
    if let PathFailure::OnPath { network_update } = &failure
        && let Some(update) = network_update
    {
        match update {
            NetworkUpdate::ChannelFailure {
                short_channel_id, ..
            } => {
                let (node_pk1, node_pk2) = {
                    let network_graph = network_graph.read_only();
                    let channel = network_graph.channel(*short_channel_id)?;
                    let node_pk1 = channel.node_one.as_pubkey().ok()?;
                    let node_pk2 = channel.node_two.as_pubkey().ok()?;
                    (node_pk1, node_pk2)
                };
                path.hops.iter().find(|hop| hop.pubkey == node_pk1)?;
                path.hops.iter().find(|hop| hop.pubkey == node_pk2)?;
            }
            NetworkUpdate::NodeFailure { node_id, .. } => {
                path.hops.iter().find(|hop| hop.pubkey == *node_id)?;
            }
        }
    }

    Some(Event::PaymentPathFailed {
        payment_id,
        payment_hash,
        payment_failed_permanently,
        failure,
        path,
        short_channel_id,
        hold_times,
    })
}

/// Anonymizes a [`Path`] to a receiver by removing hops from the end of the
/// path until the size of the anonymity set of possible receivers is at
/// least [`MIN_ANONYMITY_SET_SIZE`] (or returns [`None`] if unreachable).
fn anonymize_path(
    network_graph: &NetworkGraphType,
    mut path: Path,
) -> Option<Path> {
    // If the tail is already blinded, the receiver is already anonymized;
    // we can send the event to the LSP as is.
    if path.blinded_tail.is_some() {
        return Some(path);
    }
    // From here, we know the path does not have a blinded tail.
    let start = Instant::now();

    // We need to remove the last (Nth) hop, since it is the receiver.
    // TODO(max): Whitelist (don't pop off) custodial nodes like Strike or
    // Coinbase, as their anonymity set is all of their users.
    let receiver_hop = path.hops.pop();

    // Hold on to the path amount (last hop `fee_msat`). We need to restore
    // this on the last hop of the anonymized path in order to update the
    // LSP scorer accurately.
    let path_amount_msat = match receiver_hop {
        Some(h) => h.fee_msat,
        None => {
            debug_panic_release_log!("Path should always have >= 1 hop!");
            return None;
        }
    };

    // Pop off hops and increase our search depth until we either reach the
    // required anonymity set size or run out of hops.
    let network_graph = network_graph.read_only();
    let mut anonymity_set =
        HashSet::<NodeId>::with_capacity(MIN_ANONYMITY_SET_SIZE);
    let mut depth = 1;
    while let Some(departure_hop) = path.hops.last_mut()
        && depth <= MAX_DEPTH
    {
        let departure_node_id = NodeId::from_pubkey(&departure_hop.pubkey);
        let done = explore(
            &network_graph,
            &mut anonymity_set,
            departure_node_id,
            depth,
        );
        if done {
            debug_assert_eq!(anonymity_set.len(), MIN_ANONYMITY_SET_SIZE);
            info!(
                elapsed = %DisplayMs(start.elapsed()),
                anonymity_set = %anonymity_set.len(),
                "Anonymized path: termination depth={depth}"
            );

            // Restore the original path amount in the last hop `fee_msat`.
            departure_hop.fee_msat = path_amount_msat;

            return Some(path);
        }

        path.hops.pop();
        depth += 1;
    }

    info!(
        elapsed = %DisplayMs(start.elapsed()),
        "Failed to anonymize path; skipping. Termination depth={depth}"
    );
    None
}

/// Explores the network graph starting from `node_id` up to a depth of
/// `depth`, accumulating reachable nodes in `anonymity_set`.
///
/// - Returns [`true`] if the anonymity set reaches or exceeds
///   [`MIN_ANONYMITY_SET_SIZE`] during exploration
/// - Otherwise returns `false` after exploring up to the specified depth.
///
/// Uses recursive depth-first search (DFS) to traverse the graph, adding
/// each unvisited node to the anonymity set and terminating early if
/// the set becomes large enough. This is used to determine if a payment
/// path can be anonymized by having a sufficiently large set of
/// possible receivers.
fn explore(
    network_graph: &ReadOnlyNetworkGraph<'_>,
    anonymity_set: &mut HashSet<NodeId>,
    node_id: NodeId,
    depth: usize,
) -> bool {
    // Skip this node if it’s already in the anonymity set, as we've visited
    // it earlier in this DFS. We'll never need to re-`explore` this node
    // because `explore` will never be called on this node at a *higher*
    // depth than the depth it was explored with, due to the depth
    // incrementing only as we also pop off nodes from the path.
    let inserted = anonymity_set.insert(node_id);
    if !inserted {
        return false;
    }

    // If our anonymity set is large enough, we can stop early.
    if anonymity_set.len() >= MIN_ANONYMITY_SET_SIZE {
        return true;
    }

    // Base case: If we've reached the maximum depth, stop exploring.
    if depth == 0 {
        return false;
    }

    // Depth > 1: Explore each of this node's neighbors at depth - 1.
    // Short circuits if exploring any of our neighbors returns `done=true`.
    let node_info = match network_graph.node(&node_id) {
        Some(n) => n,
        None => return false,
    };
    node_info
        .channels
        .iter()
        .filter_map(|scid| network_graph.channel(*scid))
        .filter_map(|channel| channel.as_directed_from(&node_id))
        .map(|(channel, _)| channel.target())
        .any(|neighbor| {
            explore(network_graph, anonymity_set, *neighbor, depth - 1)
        })
}
