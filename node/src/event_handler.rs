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
//! - Event handling must be *idempotent* to varying degrees. Events handled
//!   inline must support that same event getting partially handled and then
//!   replayed after a crash. Events handled in a task must additionally support
//!   that same event getting replayed out-of-order and at a potentially much
//!   later date. See [`NodeEventHandler::get_ldk_handler_future`] for which
//!   events are handled inline vs in a task.
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
};

use anyhow::{anyhow, Context};
use common::{
    api::{test_event::TestEvent, user::NodePk},
    cli::LspInfo,
    debug_panic_release_log,
    ln::{
        channel::LxChannelId,
        payments::{LnClaimId, LxPaymentHash, LxPaymentId},
    },
    rng::{RngExt, ThreadFastRng},
};
use lexe_api::def::NodeLspApi;
use lexe_ln::{
    alias::{NetworkGraphType, ProbabilisticScorerType},
    channel::{ChannelEvent, ChannelEventsBus},
    esplora::FeeEstimates,
    event::{self, EventHandleError, EventHandlerExt, EventId},
    keys_manager::LexeKeysManager,
    payments::outbound::LxOutboundPaymentFailure,
    test_event::TestEventSender,
    traits::{LexeEventHandler, LexePersister},
    tx_broadcaster::TxBroadcaster,
    wallet::LexeWallet,
};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::events::{
    Event, InboundChannelFunds, PaymentFailureReason, ReplayEvent,
};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::{
    alias::PaymentsManagerType, channel_manager::NodeChannelManager,
    persister::NodePersister,
};

#[derive(Clone)]
pub struct NodeEventHandler {
    pub(crate) ctx: Arc<EventCtx>,
}

/// Allows all event handling context to be shared (e.g. spawned into a task)
/// with a single [`Arc`] clone.
pub(crate) struct EventCtx {
    pub lsp: LspInfo,
    pub lsp_api: Arc<dyn NodeLspApi + Send + Sync>,
    pub persister: Arc<NodePersister>,
    pub fee_estimates: Arc<FeeEstimates>,
    pub tx_broadcaster: Arc<TxBroadcaster>,
    pub wallet: LexeWallet,
    pub channel_manager: NodeChannelManager,
    pub keys_manager: Arc<LexeKeysManager>,
    pub network_graph: Arc<NetworkGraphType>,
    pub scorer: Arc<Mutex<ProbabilisticScorerType>>,
    pub payments_manager: PaymentsManagerType,
    pub channel_events_bus: ChannelEventsBus,
    #[allow(dead_code)] // We might need this later
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
        async {
            // All of these events may return `EventHandleError::Replay`, but
            // returning `ReplayEvent` to LDK causes LDK to immediately replay
            // the event. To avoid busylooping and potentially spamming our
            // infrastructure providers, we manage these events in our own
            // queue, and handle event replays with the event replayer task.
            //
            // - Take care that `Payment{Claimable,Claimed,Sent,Failed}` events
            //   are handled idempotently in the `PaymentsManager` and `Payment`
            //   state machines. See the reqs on `Payment`.
            if let Event::PaymentClaimable { .. }
            | Event::PaymentClaimed { .. }
            | Event::PaymentSent { .. }
            | Event::PaymentFailed { .. }
            | Event::SpendableOutputs { .. } = &event
            {
                self.persist_and_spawn_handler(event).await
            } else {
                self.handle_inline(event).await
            }
        }
    }

    fn handle_event(
        &self,
        _event_id: &EventId,
        event: Event,
    ) -> impl Future<Output = Result<(), EventHandleError>> + Send {
        do_handle_event(&self.ctx, event)
    }

    fn persister(&self) -> &impl LexePersister {
        &self.ctx.persister
    }
    fn shutdown(&self) -> &NotifyOnce {
        &self.ctx.shutdown
    }
}

async fn do_handle_event(
    ctx: &Arc<EventCtx>,
    event: Event,
) -> Result<(), EventHandleError> {
    event::handle_network_graph_update(&ctx.network_graph, &event);
    event::handle_scorer_update(&ctx.scorer, &event);

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
            channel_negotiation_type,
            channel_type: _,
            is_announced: _,
            params: _,
        } => {
            let handle_open_channel_request = || {
                // TODO(phlip9): support dual-funded channels
                match channel_negotiation_type {
                    InboundChannelFunds::PushMsat(_) => (),
                    InboundChannelFunds::DualFunded =>
                        return Err(anyhow!(
                            "We don't support dual-funded channels yet"
                        )),
                }

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
            last_local_balance_msat: _,
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
            payment_id,
        } => {
            // TODO(phlip9): unwrap once all replaying PaymentClaimable events
            // drain in prod.
            let claim_id = payment_id.map(LnClaimId::from);
            // NOTE: must be handled idempotently
            ctx.payments_manager
                .payment_claimable(
                    purpose,
                    payment_hash.into(),
                    claim_id,
                    amount_msat,
                )
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
            payment_id,
        } => {
            // TODO(phlip9): unwrap once all replaying PaymentClaimable events
            // drain in prod.
            let claim_id = payment_id.map(LnClaimId::from);
            // NOTE: must be handled idempotently
            ctx.payments_manager
                .payment_claimed(
                    purpose,
                    payment_hash.into(),
                    claim_id,
                    amount_msat,
                )
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

        // We don't enable `UserConfig::manually_handle_bolt12_invoices` so we
        // should never see this event.
        Event::InvoiceReceived { .. } =>
            debug_panic_release_log!("Unexpected InvoiceReceived event"),

        Event::PaymentSent {
            payment_id,
            payment_hash,
            payment_preimage,
            fee_paid_msat,
        } => {
            // NOTE: Err(Replay) ==> must be handled idempotently
            let hash = LxPaymentHash::from(payment_hash);
            let id = LxPaymentId::from_payment_sent(payment_id, hash);
            ctx.payments_manager
                .payment_sent(id, hash, payment_preimage.into(), fee_paid_msat)
                .await
                .context("Error handling PaymentSent")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Replay)?;
        }

        Event::PaymentFailed {
            payment_id,
            reason,
            payment_hash,
        } => {
            let maybe_hash = payment_hash.map(LxPaymentHash::from);
            let id = LxPaymentId::from_payment_failed(payment_id, maybe_hash);

            // NOTE: Err(Replay) ==> must be handled idempotently
            let reason =
                reason.unwrap_or(PaymentFailureReason::RetriesExhausted);
            let failure = LxOutboundPaymentFailure::from(reason);

            ctx.payments_manager
                .payment_failed(id, failure)
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
            prev_node_id,
            next_node_id,
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
                prev_node_id={prev_node_id:?}, \
                next_node_id={next_node_id:?}, \
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

        // The node does in fact forward (its own) HTLCs.
        Event::PendingHTLCsForwardable { .. } =>
            event::handle_pending_htlcs_forwardable(
                ctx.channel_manager.clone(),
                &ctx.eph_tasks_tx,
            ),

        Event::SpendableOutputs {
            outputs,
            channel_id,
        } => {
            // NOTE: Err(Replay) ==> must be handled idempotently
            let channel_id = channel_id.map(LxChannelId::from);
            event::handle_spendable_outputs(
                ctx.channel_manager.clone(),
                &ctx.keys_manager,
                &ctx.fee_estimates,
                &ctx.tx_broadcaster,
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
        Event::OnionMessageIntercepted { peer_node_id, .. } =>
            debug_panic_release_log!(
                "Unexpected `OnionMessageIntercepted` event: \
                 peer_node_id={peer_node_id}"
            ),
        Event::OnionMessagePeerConnected { peer_node_id } =>
            debug_panic_release_log!(
                "Unexpected `OnionMessagePeerConnected`: \
                 peer_node_id={peer_node_id}"
            ),
    }

    Ok(())
}

/// Helpers to anonymize payment paths.
mod anonymize {
    use std::collections::HashSet;

    use common::time::DisplayMs;
    use lightning::{
        events::PathFailure,
        ln::channelmanager::PaymentId,
        routing::{
            gossip::{NetworkUpdate, NodeId, ReadOnlyNetworkGraph},
            router::Path,
        },
        types::payment::PaymentHash,
    };
    use tokio::time::Instant;

    use super::*;

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
    /// least [`MIN_ANONYMITY_SET_SIZE`] (or returns [`None`] if unreachable).
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
        let mut anonymity_set =
            HashSet::<NodeId>::with_capacity(MIN_ANONYMITY_SET_SIZE);
        let mut depth = 1;
        while let Some(departure_hop) = path.hops.last() {
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
                return Some(path);
            }

            path.hops.pop();
            depth += 1;

            // TODO(max): Add `&& depth <= MAX_DEPTH` in the while condition
            // once "if- and while- let chain" syntax is stabilized:
            // https://github.com/rust-lang/rust/issues/53667
            if depth > MAX_DEPTH {
                break;
            }
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
}
