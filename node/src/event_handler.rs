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
    api::user::NodePk,
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
use tracing::{error, info, warn, Instrument};

use crate::{alias::PaymentsManagerType, channel_manager::NodeChannelManager};

pub struct NodeEventHandler {
    pub(crate) ctx: Arc<EventCtx>,
}

/// Allows all event handling context to be shared (e.g. spawned into a task)
/// with a single [`Arc`] clone.
pub(crate) struct EventCtx {
    pub(crate) lsp: LspInfo,
    pub(crate) esplora: Arc<LexeEsplora>,
    pub(crate) wallet: LexeWallet,
    pub(crate) channel_manager: NodeChannelManager,
    pub(crate) keys_manager: Arc<LexeKeysManager>,
    pub(crate) network_graph: Arc<NetworkGraphType>,
    pub(crate) scorer: Arc<Mutex<ProbabilisticScorerType>>,
    pub(crate) payments_manager: PaymentsManagerType,
    pub(crate) channel_events_bus: ChannelEventsBus,
    pub(crate) scorer_persist_tx: notify::Sender,
    pub(crate) test_event_tx: TestEventSender,
    pub(crate) shutdown: NotifyOnce,
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

        // Handled by `handle_network_graph_update` and `handle_scorer_update`
        Event::PaymentPathSuccessful { .. } => (),
        Event::PaymentPathFailed { .. } => (),
        Event::ProbeSuccessful { .. } => (),
        Event::ProbeFailed { .. } => (),

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
            let forwarding_channel_manager = ctx.channel_manager.clone();
            let millis_to_sleep = time_forwardable.as_millis() as u64;
            LxTask::spawn(async move {
                tokio::time::sleep(Duration::from_millis(millis_to_sleep))
                    .await;
                forwarding_channel_manager.process_pending_htlc_forwards();
            })
            .detach();
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
