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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use bitcoin::{constants::ChainHash, secp256k1::PublicKey};
    use lexe_common::secp256k1_ctx::SECP256K1;
    use lexe_ln::logger::LexeTracingLogger;
    use lightning::{
        ln::msgs::UnsignedChannelUpdate,
        routing::{
            gossip::NodeId,
            router::{Path, RouteHop},
        },
        types::features::{ChannelFeatures, NodeFeatures},
    };

    use super::{MIN_ANONYMITY_SET_SIZE, NetworkGraphType, anonymize_path};

    #[test]
    fn anonymize_path_removes_receiver_and_restores_final_value() {
        let graph = NetworkGraphType::new(
            bitcoin::Network::Regtest,
            LexeTracingLogger::new(),
        );

        // original: sender <-> lsp <-> hop <-> receiver
        let _sender = node_pubkey(0);
        let lsp = node_pubkey(1);
        let hop = node_pubkey(2);
        let receiver = node_pubkey(3);
        let off_path = node_pubkey(4);
        // sender<->lsp is un-announced
        // add_channel(&network_graph, 1, sender, lsp);
        add_channel(&graph, 2, lsp, hop);
        add_channel(&graph, 3, hop, receiver);

        add_channel(&graph, 4, hop, off_path);
        add_channel(&graph, 5, off_path, receiver);

        // Add some nodes/channels around `hop` to exercise min anonymity set
        // anonymity set: {hop, lsp, }
        for idx in 0..(MIN_ANONYMITY_SET_SIZE - 4) {
            let scid = 1000 + idx as u64;
            add_channel(&graph, scid, hop, node_pubkey(1000 + idx));
        }

        // Add some nodes/channels around `lsp` and `receiver` just for fun
        for idx in 0..10 {
            let scid = 2000 + idx as u64;
            add_channel(&graph, scid, lsp, node_pubkey(2000 + idx));
        }
        for idx in 0..10 {
            let scid = 3000 + idx as u64;
            add_channel(&graph, scid, receiver, node_pubkey(3000 + idx));
        }

        let final_value_msat = 123_456;
        let path = Path {
            hops: vec![
                route_hop(lsp, 1, 1212),
                route_hop(hop, 2, 6969),
                route_hop(receiver, 3, final_value_msat),
            ],
            blinded_tail: None,
        };

        let anonymized = anonymize_path(&graph, path).expect("anonymizes path");

        // anonymized: sender <-> lsp <-> hop
        assert_eq!(anonymized.hops.len(), 2);
        assert_eq!(anonymized.hops[0].pubkey, lsp);
        assert_eq!(anonymized.hops[0].fee_msat, 1212);
        assert_eq!(anonymized.hops[1].pubkey, hop);
        assert_eq!(anonymized.hops[1].fee_msat, final_value_msat);
        assert_eq!(anonymized.blinded_tail, None);
    }

    fn add_channel(
        network_graph: &NetworkGraphType,
        short_channel_id: u64,
        node_1: PublicKey,
        node_2: PublicKey,
    ) {
        // Need real clock time o/w LDK rejects the channel for being too old
        // or too far in the future
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time is after unix epoch")
            .as_secs()
            .try_into()
            .expect("current time fits in u32");
        network_graph
            .add_channel_from_partial_announcement(
                short_channel_id,
                None,
                u64::from(timestamp),
                ChannelFeatures::empty(),
                NodeId::from_pubkey(&node_1),
                NodeId::from_pubkey(&node_2),
            )
            .unwrap();

        update_channel(network_graph, short_channel_id, timestamp, 0);
        update_channel(network_graph, short_channel_id, timestamp, 1);
    }

    fn node_pubkey(id: usize) -> PublicKey {
        let mut bytes = [1u8; 32];
        bytes[31] = id as u8 + 1;
        let secret_key = bitcoin::secp256k1::SecretKey::from_slice(&bytes)
            .expect("valid secret key");
        PublicKey::from_secret_key(&SECP256K1, &secret_key)
    }

    fn route_hop(
        pubkey: PublicKey,
        short_channel_id: u64,
        fee_msat: u64,
    ) -> RouteHop {
        RouteHop {
            pubkey,
            node_features: NodeFeatures::empty(),
            short_channel_id,
            channel_features: ChannelFeatures::empty(),
            fee_msat,
            cltv_expiry_delta: 144,
            maybe_announced_channel: true,
        }
    }

    fn update_channel(
        network_graph: &NetworkGraphType,
        short_channel_id: u64,
        timestamp: u32,
        channel_flags: u8,
    ) {
        network_graph
            .update_channel_unsigned(&UnsignedChannelUpdate {
                chain_hash: ChainHash::using_genesis_block(
                    bitcoin::Network::Regtest,
                ),
                short_channel_id,
                timestamp,
                message_flags: 1,
                channel_flags,
                cltv_expiry_delta: 144,
                htlc_minimum_msat: 1000,
                htlc_maximum_msat: 1_000_000,
                fee_base_msat: 1000,
                fee_proportional_millionths: 100,
                excess_data: Vec::new(),
            })
            .unwrap();
    }
}
