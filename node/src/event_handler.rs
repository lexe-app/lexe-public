//! Event handling requirements are outlined in the doc comments for
//! [`EventsProvider::process_pending_events`], which we summarize and expand on
//! here because they are very important to understand clearly.
//!
//! - The channel manager internally contains a `pending_events` queue holding
//!   events which are released to our event handler when our BGP calls
//!   [`ChannelManager::process_pending_events_async`]. If the handler returns
//!   [`Ok(())`], the event is lost when the BGP repersists the channel manager.
//! - On the other hand, if the event handler returns [`Err(ReplayEvent)`], the
//!   event won't be removed from the queue and LDK will automatically replay it
//!   for us, but (as of 2024-11-18) the node won't be able to make progress on
//!   other events until the erroring event is successfully handled.
//! - In practice, depending on the event kind, sometimes we will handle the
//!   event 'inline' (without spawning a task), and sometimes we'll persist the
//!   event in our own queue and handle or replay it later.
//! - Event handling must be *idempotent*. It must be okay to handle the same
//!   event twice, since events may be redundantly replayed due to various race
//!   conditions such as the program crashing before the event is deleted from
//!   the channel manager / Lexe's own event queue.
//! - The event handler must avoid reentrancy by avoiding direct calls to
//!   [`ChannelManager::process_pending_events_async`] or
//!   [`ChainMonitor::process_pending_events_async`]. Otherwise, there may be a
//!   deadlock.
//! - The event handler must not call [`Writeable::write`] on the channel
//!   manager, otherwise there will be a deadlock, because a read lock on the
//!   channel manager's `total_consistency_lock` is held for the duration of the
//!   event handling, and serializing the channel manager requires a write lock.
//!
//! [`EventsProvider::process_pending_events`]: lightning::events::EventsProvider::process_pending_events
//! [`Writeable::write`]: lightning::util::ser::Writeable::write
//! [`ChannelManager::process_pending_events_async`]: lightning::ln::channelmanager::ChannelManager::process_pending_events_async
//! [`ChannelManager::process_pending_events_async`]: lightning::ln::channelmanager::ChannelManager::process_pending_events_async
//! [`ChainMonitor::process_pending_events_async`]: lightning::chain::chainmonitor::ChainMonitor::process_pending_events_async

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Context};
use common::{
    api::{def::NodeLspApi, user::NodePk},
    cli::LspInfo,
    debug_panic_release_log,
    ln::{channel::LxChannelId, payments::LxPaymentHash},
    notify,
    notify_once::NotifyOnce,
    rng::{RngExt, ThreadFastRng},
    task::LxTask,
    test_event::TestEvent,
};
use lexe_ln::{
    alias::{NetworkGraphType, ProbabilisticScorerType},
    channel::{ChannelEvent, ChannelEventsBus},
    esplora::LexeEsplora,
    event::{self, EventExt, EventHandleError},
    keys_manager::LexeKeysManager,
    payments::outbound::LxOutboundPaymentFailure,
    test_event::TestEventSender,
    traits::LexeEventHandler,
    wallet::LexeWallet,
};
use lightning::events::{Event, PaymentFailureReason, ReplayEvent};
use tokio::sync::mpsc;
use tracing::{error, info, info_span, warn, Instrument};

use crate::{alias::PaymentsManagerType, channel_manager::NodeChannelManager};

pub struct NodeEventHandler {
    pub(crate) ctx: Arc<EventCtx>,
}

/// Allows all event handling context to be shared (e.g. spawned into a task)
/// with a single [`Arc`] clone.
pub(crate) struct EventCtx {
    pub lsp: LspInfo,
    pub lsp_api: Arc<dyn NodeLspApi + Send + Sync>,
    pub esplora: Arc<LexeEsplora>,
    pub wallet: LexeWallet,
    pub channel_manager: NodeChannelManager,
    pub keys_manager: Arc<LexeKeysManager>,
    pub network_graph: Arc<NetworkGraphType>,
    pub scorer: Arc<Mutex<ProbabilisticScorerType>>,
    pub payments_manager: PaymentsManagerType,
    pub channel_events_bus: ChannelEventsBus,
    pub scorer_persist_tx: notify::Sender,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    pub test_event_tx: TestEventSender,
    pub shutdown: NotifyOnce,
}

impl LexeEventHandler for NodeEventHandler {
    #[allow(clippy::manual_async_fn)] // Be more explicit re LDK's API.
    fn get_ldk_handler_future(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> {
        self.handle_inline(event)
    }
}

impl NodeEventHandler {
    /// Handles an event 'inline', i.e. without spawning off to a task.
    async fn handle_inline(&self, event: Event) -> Result<(), ReplayEvent> {
        let (_event_id, span) = event.handle_prelude();

        async {
            match do_handle_event(&self.ctx, event).await {
                Ok(()) => Ok(info!("Successfully handled event")),
                Err(EventHandleError::Discard(e)) =>
                    Ok(warn!("Tolerable event error, discarding event: {e:#}")),
                Err(EventHandleError::Replay(e)) => {
                    error!("Critical event error, will replay event: {e:#}");
                    Err(ReplayEvent())
                }
            }
        }
        // Instrument all logs for this event with the event span
        .instrument(span)
        .await
    }
}

async fn do_handle_event(
    ctx: &Arc<EventCtx>,
    event: Event,
) -> Result<(), EventHandleError> {
    event::handle_network_graph_update(&ctx.network_graph, &event);
    event::handle_scorer_update(&ctx.scorer, &ctx.scorer_persist_tx, &event);

    match event {
        // NOTE: This event is received because manually_accept_inbound_channels
        // is set to true. Manually accepting inbound channels is required
        // (1) we may accept zeroconf channels (2) we need to verify that it is
        // Lexe's LSP that is initiating the channel with us. The event MUST be
        // resolved by (a) rejecting the channel open request by calling
        // force_close_without_broadcasting_txn() or (b) accepting the request
        // using accept_inbound_channel() or (c) accepting as trusted zeroconf
        // using accept_inbound_channel_from_trusted_peer_0conf().
        Event::OpenChannelRequest {
            temporary_channel_id,
            counterparty_node_id,
            funding_satoshis: _,
            push_msat: _,
            channel_type: _,
            is_announced: _,
            params: _,
        } => {
            let handle_open_channel_request = || {
                // Only accept inbound channels from Lexe's LSP
                let counterparty_node_pk = NodePk(counterparty_node_id);
                if counterparty_node_pk != ctx.lsp.node_pk {
                    // Lexe's proxy should have prevented non-Lexe nodes from
                    // connecting to us. Log an error and shut down.
                    error!(
                        "Received open channel request from non-Lexe node which \
                        the proxy should have prevented: {counterparty_node_pk}"
                    );

                    // Initiate a shutdown
                    ctx.shutdown.send();

                    return Err(anyhow!(
                        "User only accepts inbound channels from Lexe LSP"
                    ));
                }

                // Checks passed, accept the (probably zero-conf) channel.
                let user_channel_id = ThreadFastRng::new().gen_u128();
                ctx.channel_manager
                    .accept_inbound_channel_from_trusted_peer_0conf(
                        &temporary_channel_id,
                        &counterparty_node_id,
                        user_channel_id,
                    )
                    .inspect(|_| info!("Accepted zeroconf channel from LSP"))
                    .map_err(|e| anyhow!("accept inbound 0conf: {e:?}"))?;

                Ok::<(), anyhow::Error>(())
            };

            // Make sure we clean up and close the channel if something goes
            // wrong accepting the channel.
            if let Err(handle_err) = handle_open_channel_request() {
                ctx.channel_manager
                    .force_close_without_broadcasting_txn(
                        &temporary_channel_id,
                        &counterparty_node_id,
                        handle_err.to_string(),
                    )
                    .map_err(|close_err| {
                        anyhow!(
                            "Error closing channel from failed open request: \
                                 {close_err:?}; handle error: {handle_err:#}"
                        )
                    })
                    .map_err(EventHandleError::Discard)?;
            }
        }

        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            user_channel_id: _,
        } => event::handle_funding_generation_ready(
            &ctx.wallet,
            &ctx.channel_manager,
            &ctx.test_event_tx,
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
        )?,

        // This event is only emitted if we use
        // `ChannelManager::unsafe_manual_funding_transaction_generated`.
        Event::FundingTxBroadcastSafe {
            channel_id,
            funding_txo,
            counterparty_node_id,
            ..
        } => error!(
            %channel_id, %funding_txo, %counterparty_node_id,
            "Somehow received FundingTxBroadcastSafe"
        ),

        Event::ChannelPending {
            channel_id,
            user_channel_id,
            former_temporary_channel_id: _,
            counterparty_node_id,
            funding_txo,
            channel_type,
        } => {
            event::log_channel_pending(
                channel_id.into(),
                user_channel_id.into(),
                counterparty_node_id,
                funding_txo,
                channel_type,
            );
            ctx.channel_events_bus.notify(ChannelEvent::Pending {
                user_channel_id: user_channel_id.into(),
                channel_id: channel_id.into(),
                funding_txo,
            });
            ctx.test_event_tx.send(TestEvent::ChannelPending);
        }

        Event::ChannelReady {
            channel_id,
            user_channel_id,
            counterparty_node_id,
            channel_type,
        } => {
            event::log_channel_ready(
                channel_id.into(),
                user_channel_id.into(),
                counterparty_node_id,
                channel_type,
            );
            ctx.channel_events_bus.notify(ChannelEvent::Ready {
                user_channel_id: user_channel_id.into(),
                channel_id: channel_id.into(),
            });
            ctx.test_event_tx.send(TestEvent::ChannelReady);
        }

        Event::ChannelClosed {
            channel_id,
            user_channel_id,
            reason,
            counterparty_node_id,
            channel_capacity_sats,
            channel_funding_txo,
        } => {
            event::log_channel_closed(
                channel_id.into(),
                user_channel_id.into(),
                &reason,
                counterparty_node_id,
                channel_capacity_sats,
                channel_funding_txo,
            );
            ctx.channel_events_bus.notify(ChannelEvent::Closed {
                user_channel_id: user_channel_id.into(),
                channel_id: channel_id.into(),
                reason,
            });
            ctx.test_event_tx.send(TestEvent::ChannelClosed);
        }

        Event::PaymentClaimable {
            payment_hash,
            amount_msat,
            purpose,
            // TODO(max): Use this
            counterparty_skimmed_fee_msat: _,
            onion_fields: _,
            receiver_node_id: _,
            via_channel_id: _,
            via_user_channel_id: _,
            claim_deadline: _,
        } => {
            ctx.payments_manager
                .payment_claimable(payment_hash.into(), amount_msat, purpose)
                .await
                .context("Error handling PaymentClaimable")
                // Want to ensure we always claim funds
                .map_err(EventHandleError::Replay)?;
        }

        Event::PaymentClaimed {
            receiver_node_id: _,
            payment_hash,
            amount_msat,
            purpose,
            htlcs: _,
            // TODO(max): We probably want to use this to get JIT on-chain fees?
            sender_intended_total_msat: _,
            onion_fields: _,
        } => {
            ctx.payments_manager
                .payment_claimed(payment_hash.into(), amount_msat, purpose)
                .await
                .context("Error handling PaymentClaimed")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Replay)?;
        }

        Event::ConnectionNeeded { node_id, addresses } => {
            // The only connection the user node should need is to the LSP.
            // Ignore the event but log an error.
            let node_pk = NodePk(node_id);
            let addrs = addresses
                .into_iter()
                .map(|addr| format!("{addr}"))
                .collect::<Vec<String>>();
            debug_panic_release_log!(
                "Unexpected `ConnectionNeeded` event: \
                node_pk={node_pk}, addrs={addrs:?}"
            );
        }

        // TODO(max): Revisit for BOLT 12
        Event::InvoiceReceived { payment_id, .. } =>
            error!(%payment_id, "Somehow received InvoiceReceived"),

        Event::PaymentSent {
            payment_id: _,
            payment_hash,
            payment_preimage,
            fee_paid_msat,
        } => {
            ctx.payments_manager
                .payment_sent(
                    payment_hash.into(),
                    payment_preimage.into(),
                    fee_paid_msat,
                )
                .await
                .context("Error handling PaymentSent")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Replay)?;
        }

        Event::PaymentFailed {
            payment_id: _,
            reason,
            payment_hash,
        } => {
            let reason =
                reason.unwrap_or(PaymentFailureReason::RetriesExhausted);
            let failure = LxOutboundPaymentFailure::from(reason);
            warn!("Payment failed: {failure:?}");
            // TODO(max): Remove .expect() for BOLT 12, handle bolt 12 client id
            let hash = payment_hash.expect("Only None for BOLT 12");
            let hash = LxPaymentHash::from(hash);
            ctx.test_event_tx.send(TestEvent::PaymentFailed);
            ctx.payments_manager
                .payment_failed(hash.into(), failure)
                .await
                .context("Error handling PaymentFailed")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Replay)?;
        }

        // `handle_network_graph_update` and `handle_scorer_update` applied the
        // payment updates to our local network graph and scorer, respectively.
        // Here, we anonymize this info, then send it to the LSP.
        Event::PaymentPathSuccessful {
            payment_id,
            payment_hash,
            path,
        } => {
            let maybe_event =
                anonymize::successful_path(ctx, payment_id, payment_hash, path);
            match maybe_event {
                Some(event) => {
                    info!("Sending anonymized succ path to LSP");
                    ctx.lsp_api
                        .payment_path(&event)
                        .await
                        .context("Failed to call /payment_path")
                        .map_err(EventHandleError::Discard)?;
                }
                None => info!("Failed to anonymize successful path; skipping"),
            }
        }
        Event::PaymentPathFailed {
            payment_id,
            payment_hash,
            payment_failed_permanently,
            failure,
            path,
            short_channel_id,
        } => {
            let maybe_event = anonymize::failed_path(
                ctx,
                payment_id,
                payment_hash,
                payment_failed_permanently,
                failure,
                path,
                short_channel_id,
            );
            match maybe_event {
                Some(event) => {
                    info!("Sending anonymized failed path to LSP");
                    ctx.lsp_api
                        .payment_path(&event)
                        .await
                        .context("Failed to call /payment_path")
                        .map_err(EventHandleError::Discard)?;
                }
                None => info!("Failed to anonymize failed path; skipping"),
            }
        }

        // The node doesn't send probes
        Event::ProbeSuccessful { .. } =>
            debug_panic_release_log!("Somehow received ProbeSuccessful"),
        Event::ProbeFailed { .. } =>
            debug_panic_release_log!("Somehow received ProbeFailed"),

        Event::PaymentForwarded {
            prev_channel_id,
            next_channel_id,
            prev_user_channel_id,
            next_user_channel_id,
            total_fee_earned_msat,
            skimmed_fee_msat,
            claim_from_onchain_tx,
            outbound_amount_forwarded_msat,
        } => {
            let prev_channel_id =
                prev_channel_id.expect("Launched after v0.0.107");
            let next_channel_id =
                next_channel_id.expect("Launched after v0.0.107");
            let prev_user_channel_id =
                prev_user_channel_id.expect("Launched after v0.0.122");

            // The user node doesn't forward payments
            debug_panic_release_log!(
                "Somehow received a PaymentForwarded event: \
                prev_channel_id={prev_channel_id}, \
                next_channel_id={next_channel_id}, \
                prev_user_channel_id={prev_user_channel_id}, \
                next_user_channel_id={next_user_channel_id:?}, \
                total_fee_earned_msat={total_fee_earned_msat:?}, \
                skimmed_fee_msat={skimmed_fee_msat:?}, \
                claim_from_onchain_tx={claim_from_onchain_tx}, \
                outbound_amount_forwarded_msat={outbound_amount_forwarded_msat:?}"
            );
        }

        Event::HTLCIntercepted { .. } => {
            unreachable!("accept_intercept_htlcs in UserConfig is false")
        }

        Event::HTLCHandlingFailed { .. } => {}

        Event::PendingHTLCsForwardable { time_forwardable } => {
            let channel_manager = ctx.channel_manager.clone();
            let time_to_sleep =
                Duration::from_millis(time_forwardable.as_millis() as u64);
            let task = LxTask::spawn_with_span(
                "PendingHTLCsForwardable handler",
                info_span!("(pending-htlc-fwd)"),
                async move {
                    tokio::time::sleep(time_to_sleep).await;
                    channel_manager.process_pending_htlc_forwards();
                    info!("Processed pending HTLC forwards");
                },
            );
            if ctx.eph_tasks_tx.try_send(task).is_err() {
                warn!("(PendingHTLCsForwardable) Couldn't send task");
            }
        }

        Event::SpendableOutputs {
            outputs,
            channel_id,
        } => {
            let channel_id = channel_id.map(LxChannelId::from);
            event::handle_spendable_outputs(
                ctx.channel_manager.clone(),
                &ctx.keys_manager,
                &ctx.esplora,
                &ctx.wallet,
                &ctx.test_event_tx,
                outputs,
            )
            .await
            .with_context(|| format!("{channel_id:?}"))
            .context("Error handling SpendableOutputs")
            // Must replay because the outputs are lost if they aren't swept.
            .map_err(EventHandleError::Replay)?;
        }

        Event::DiscardFunding { .. } => {
            // A "real" node should probably "lock" the UTXOs spent in funding
            // transactions until the funding transaction either confirms, or
            // this event is generated.
        }

        Event::BumpTransaction(_) => {
            // TODO(max): Implement this once we support anchor outputs
        }

        // We don't use this
        Event::OnionMessageIntercepted { peer_node_id, .. } => error!(
            %peer_node_id,
            "Somehow received OnionMessageIntercepted"
        ),
        // We don't use this
        Event::OnionMessagePeerConnected { peer_node_id } => error!(
            %peer_node_id,
            "Somehow received OnionMessagePeerConnected"
        ),
    }

    Ok(())
}

/// Helpers to anonymize payment paths.
mod anonymize {
    use std::collections::HashSet;

    use lexe_api::trace::DisplayMs;
    use lightning::{
        events::PathFailure,
        ln::{channelmanager::PaymentId, PaymentHash},
        routing::{
            gossip::{NetworkUpdate, NodeId, ReadOnlyNetworkGraph},
            router::Path,
        },
    };
    use tokio::time::Instant;

    use super::*;

    /// The minimum size of the anonymity set of possible receivers after a
    /// payment path has been anonymized.
    ///
    /// Intended to be small enough so that most LSPs can qualify as the (N-1)th
    /// hop, but large enough to provide good privacy.
    // TODO(max): Increase to 50 or 100 once we have more reliable payments.
    const MIN_ANONYMITY_SET: usize = 20;

    /// Anonymizes a [`Event::PaymentPathSuccessful`].
    pub(super) fn successful_path(
        ctx: &EventCtx,
        payment_id: PaymentId,
        payment_hash: Option<PaymentHash>,
        path: Path,
    ) -> Option<Event> {
        anonymize_path(ctx, path).map(|path| Event::PaymentPathSuccessful {
            payment_id,
            payment_hash,
            path,
        })
    }

    /// Anonymizes a [`Event::PaymentPathFailed`].
    pub(super) fn failed_path(
        ctx: &EventCtx,
        payment_id: Option<PaymentId>,
        payment_hash: PaymentHash,
        payment_failed_permanently: bool,
        failure: PathFailure,
        path: Path,
        short_channel_id: Option<u64>,
    ) -> Option<Event> {
        let path = anonymize_path(ctx, path)?;

        // So that we don't penalize a subset of the path which was not the
        // cause of the payment failure, as well as to not blow our privacy,
        // ensure that the failed channel or node is on the anonymized path.
        let network_graph = ctx.network_graph.read_only();
        #[allow(clippy::collapsible_match)] // Suggestion is less readable
        if let PathFailure::OnPath { network_update } = &failure {
            if let Some(update) = network_update {
                match update {
                    NetworkUpdate::ChannelFailure {
                        short_channel_id, ..
                    } => {
                        let channel =
                            network_graph.channel(*short_channel_id)?;
                        let node_pk1 = channel.node_one.as_pubkey().ok()?;
                        let node_pk2 = channel.node_two.as_pubkey().ok()?;
                        path.hops.iter().find(|hop| hop.pubkey == node_pk1)?;
                        path.hops.iter().find(|hop| hop.pubkey == node_pk2)?;
                    }
                    NetworkUpdate::NodeFailure { node_id, .. } => {
                        path.hops.iter().find(|hop| hop.pubkey == *node_id)?;
                    }
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
        })
    }

    /// Anonymizes a [`Path`] to a receiver by removing hops from the end of the
    /// path until the size of the anonymity set of possible receivers is at
    /// least [`MIN_ANONYMITY_SET`] (or returns [`None`] if unreachable).
    fn anonymize_path(ctx: &EventCtx, mut path: Path) -> Option<Path> {
        // If the tail is already blinded, the receiver is already anonymized;
        // we can send the event to the LSP as is.
        if path.blinded_tail.is_some() {
            info!("Anonymized path: Blinded tail already anonymized");
            return Some(path);
        }
        // From here, we know the path does not have a blinded tail.
        let start = Instant::now();

        // We need to remove the last (Nth) hop, since it is the receiver.
        // TODO(max): Whitelist (don't pop off) custodial nodes like Strike or
        // Coinbase, as their anonymity set is all of their users.
        let receiver_hop = path.hops.pop();
        if receiver_hop.is_none() {
            debug_panic_release_log!("Path should always have at >= 1 hop!");
            return None;
        };

        // Pop off hops and increase our search depth until we either reach the
        // required anonymity set size or run out of hops.
        let network_graph = ctx.network_graph.read_only();
        let mut depth = 1;
        while let Some(departure_hop) = path.hops.last() {
            let departure_node_id = NodeId::from_pubkey(&departure_hop.pubkey);
            let anonymity_set =
                compute_anonymity_set(&network_graph, departure_node_id, depth);

            if anonymity_set >= MIN_ANONYMITY_SET {
                let elapsed = DisplayMs(start.elapsed());
                info!(
                    "Anonymized path: Anonymity set size = {anonymity_set}, \
                     termination depth={depth}, elapsed={elapsed}"
                );
                return Some(path);
            }

            path.hops.pop();
            depth += 1;
        }

        info!(
            elapsed = %DisplayMs(start.elapsed()),
            "Failed to anonymize path; skipping"
        );
        None
    }

    /// Computes the size of the anonymity set of receivers reachable from
    /// `departure_node_id` within `depth` hops.
    ///
    /// Uses cumulative BFS to spread out from `departure_node_id`, accumulating
    /// anonymity set members in a [`HashSet`] to ensure no duplicate nodes.
    fn compute_anonymity_set(
        network_graph: &ReadOnlyNetworkGraph<'_>,
        departure_node_id: NodeId,
        depth: usize,
    ) -> usize {
        // Start with departure node in both the `seen` and `frontier_ids`
        // The `seen_ids` set ensures we don't do duplicate work.
        let mut seen_ids = HashSet::from([departure_node_id]);
        let mut frontier_ids = HashSet::from([departure_node_id]);

        // Outer loop iterates `depth` times. At each iteration, it expands the
        // current frontier to include all direct neighbors not seen before.
        'outer: for _ in 0..depth {
            let mut next_frontier_ids = HashSet::new();

            // For all nodes in the current frontier:
            'inner: for node_id in frontier_ids.drain() {
                let node = match network_graph.node(&node_id) {
                    Some(n) => n,
                    None => continue 'inner,
                };

                // For each of this node's neighbors, add the neighbor to
                // `next_frontier_ids` if it hasn't been seen before.
                node.channels
                    .iter()
                    .filter_map(|scid| network_graph.channel(*scid))
                    .flat_map(|channel| [channel.node_one, channel.node_two])
                    .for_each(|neighbor| {
                        let newly_seen = seen_ids.insert(neighbor);
                        if newly_seen {
                            next_frontier_ids.insert(neighbor);
                        }
                    });
            }

            // If no new nodes were discovered, we can stop early.
            if next_frontier_ids.is_empty() {
                break 'outer;
            }

            frontier_ids = next_frontier_ids;
        }

        seen_ids.len()
    }
}
