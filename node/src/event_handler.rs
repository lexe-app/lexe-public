use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Context};
use common::{
    api::NodePk, cli::LspInfo, ln::channel::LxChannelId,
    shutdown::ShutdownChannel, task::LxTask,
};
use lexe_ln::{
    alias::NetworkGraphType,
    channel::ChannelEventsMonitor,
    esplora::LexeEsplora,
    event::{self, EventHandleError},
    keys_manager::LexeKeysManager,
    payments::outbound::LxOutboundPaymentFailure,
    test_event::TestEventSender,
    wallet::LexeWallet,
};
use lightning::events::{Event, EventHandler, PaymentFailureReason};
use tracing::{error, info, warn};

use crate::{alias::PaymentsManagerType, channel_manager::NodeChannelManager};

// We pub(crate) all the fields to prevent having to specify each field two more
// times in Self::new parameters and in struct init syntax.
pub struct NodeEventHandler {
    pub(crate) lsp: LspInfo,
    pub(crate) wallet: LexeWallet,
    pub(crate) channel_manager: NodeChannelManager,
    pub(crate) keys_manager: Arc<LexeKeysManager>,
    pub(crate) esplora: Arc<LexeEsplora>,
    pub(crate) network_graph: Arc<NetworkGraphType>,
    pub(crate) payments_manager: PaymentsManagerType,
    pub(crate) fatal_event: Arc<AtomicBool>,
    pub(crate) channel_events_monitor: ChannelEventsMonitor,
    pub(crate) test_event_tx: TestEventSender,
    pub(crate) shutdown: ShutdownChannel,
}

// TODO(max): Revisit with async handling in mind, docs likely out of date
/// Event handling requirements are outlined in the doc comments for
/// [`EventsProvider`], `ChannelManager::process_pending_events`, and
/// `ChainMonitor::process_pending_events`, which we summarize and expand on
/// here because they are very important to understand clearly.
///
/// - The docs state that the handling of an event must *succeed* before
///   returning from this function. Otherwise, if the background processor
///   repersists the channel manager and the program crashes before event
///   handling succeeds, the event (which is queued up and persisted in the
///   channel manager) will be lost forever.
///   - In practice, we accomplish this by sending a notification to the BGP if
///     a fatal [`EventHandleError`] occurs. The BGP checks for a notification
///     just after the call to `process_pending_events[_async]` and skips the
///     channel manager persist and any I/O if a notification was received.
/// - Event handling must be *idempotent*. It must be okay to handle the same
///   event twice, since if an event is handled but another event produced a
///   fatal error, or the program crashes before the channel manager can be
///   repersisted, the event will be replayed upon next boot.
/// - The event handler must avoid reentrancy by avoiding direct calls to
///   `ChannelManager::process_pending_events` or
///   `ChainMonitor::process_pending_events` (or their async variants).
///   Otherwise, there may be a deadlock.
/// - The event handler must not call [`Writeable::write`] on the channel
///   manager, otherwise there will be a deadlock, because the channel manager's
///   `total_consistency_lock` is held for the duration of the event handling.
///
/// [`EventsProvider`]: lightning::events::EventsProvider
/// [`Writeable::write`]: lightning::util::ser::Writeable::write
impl EventHandler for NodeEventHandler {
    fn handle_event(&self, event: Event) {
        let event_name = lexe_ln::event::get_event_name(&event);
        info!("Handling event: {event_name}");
        #[cfg(debug_assertions)] // Events contain sensitive info
        tracing::trace!("Event details: {event:?}");

        // TODO(max): Remove all clone()s when async handling is implemented
        let lsp = self.lsp.clone();
        let wallet = self.wallet.clone();
        let channel_manager = self.channel_manager.clone();
        let esplora = self.esplora.clone();
        let network_graph = self.network_graph.clone();
        let keys_manager = self.keys_manager.clone();
        let payments_manager = self.payments_manager.clone();
        let fatal_event = self.fatal_event.clone();
        let channel_events_monitor = self.channel_events_monitor.clone();
        let test_event_tx = self.test_event_tx.clone();
        let shutdown = self.shutdown.clone();

        // XXX(max): We are currently breaking the EventHandler contract because
        // spawning off the event handling in a task means that it is possible
        // for the BGP to repersist the channel manager (thus losing any events)
        // prior to finding out that one of the events in the batch produced a
        // fatal error and must be replayed upon the next boot.
        //
        // An attempt to hack around this using the BlockingTaskRt caused other
        // Lexe services (which are run on the same thread in integration tests)
        // to be unresponsive while events were being handled, which in turn
        // prevented the event handler from completing, so we gave up for now.
        //
        // Once we move to async event handling, the BGP will repersist the
        // channel manager only after it `.await`s for the async event handler
        // to complete, so the contract will be upheld once more.
        #[allow(clippy::redundant_async_block)]
        LxTask::spawn(async move {
            handle_event(
                &lsp,
                &wallet,
                &channel_manager,
                &esplora,
                &network_graph,
                keys_manager.as_ref(),
                &payments_manager,
                fatal_event.as_ref(),
                &channel_events_monitor,
                &test_event_tx,
                &shutdown,
                event,
            )
            .await
        })
        .detach();
    }
}

// TODO(max): Make this non-async by spawning tasks instead
pub(crate) async fn handle_event(
    lsp: &LspInfo,
    wallet: &LexeWallet,
    channel_manager: &NodeChannelManager,
    esplora: &LexeEsplora,
    network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    payments_manager: &PaymentsManagerType,
    fatal_event: &AtomicBool,
    channel_events_monitor: &ChannelEventsMonitor,
    test_event_tx: &TestEventSender,
    shutdown: &ShutdownChannel,
    event: Event,
) {
    let event_name = lexe_ln::event::get_event_name(&event);
    let handle_event_res = handle_event_fallible(
        lsp,
        wallet,
        channel_manager,
        esplora,
        network_graph,
        keys_manager,
        payments_manager,
        channel_events_monitor,
        test_event_tx,
        shutdown,
        event,
    )
    .await;

    match handle_event_res {
        Ok(()) => info!("Successfully handled {event_name}"),
        Err(EventHandleError::Tolerable(e)) =>
            warn!("Tolerable error handling {event_name}: {e:#}"),
        Err(EventHandleError::Fatal(e)) => {
            error!("Fatal error handling {event_name}: {e:#}");
            shutdown.send();
            // Notify our BGP that a fatal event handling error has occurred and
            // that the current batch of events MUST not be lost.
            fatal_event.store(true, Ordering::Release);
        }
    }
}

async fn handle_event_fallible(
    lsp: &LspInfo,
    wallet: &LexeWallet,
    channel_manager: &NodeChannelManager,
    esplora: &LexeEsplora,
    // TODO(max): Remove this?
    _network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    payments_manager: &PaymentsManagerType,
    channel_events_monitor: &ChannelEventsMonitor,
    test_event_tx: &TestEventSender,
    shutdown: &ShutdownChannel,
    event: Event,
) -> Result<(), EventHandleError> {
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
        } => {
            // Only accept inbound channels from Lexe's LSP
            let counterparty_node_pk = NodePk(counterparty_node_id);
            if counterparty_node_pk != lsp.node_pk {
                // Lexe's proxy should have prevented non-Lexe nodes from
                // connecting to us. Log an error and shut down.
                error!(
                    "Received open channel request from non-Lexe node which \
                    the proxy should have prevented: {counterparty_node_pk}"
                );

                // Reject the channel
                channel_manager
                    .force_close_without_broadcasting_txn(
                        &temporary_channel_id,
                        &counterparty_node_id,
                    )
                    .map_err(|e| anyhow!("{e:?}"))
                    .context("Couldn't reject channel from unknown LSP")
                    .map_err(EventHandleError::Tolerable)?;

                // Initiate a shutdown
                shutdown.send();
            } else {
                // Checks passed, accept the (possible zero-conf) channel.

                // No need for a user channel id at the moment
                let user_channel_id = 0;
                channel_manager
                    .accept_inbound_channel_from_trusted_peer_0conf(
                        &temporary_channel_id,
                        &counterparty_node_id,
                        user_channel_id,
                    )
                    .inspect(|_| info!("Accepted zeroconf channel from LSP"))
                    .map_err(|e| anyhow!("Zero conf required: {e:?}"))
                    .map_err(EventHandleError::Tolerable)?;
            }
        }

        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            user_channel_id: _,
        } =>
            event::handle_funding_generation_ready(
                wallet,
                channel_manager,
                test_event_tx,
                temporary_channel_id,
                counterparty_node_id,
                channel_value_satoshis,
                output_script,
            )
            .await?,

        Event::ChannelPending {
            channel_id,
            user_channel_id,
            former_temporary_channel_id: _,
            counterparty_node_id,
            funding_txo,
            channel_type,
        } => event::handle_channel_pending(
            channel_events_monitor,
            test_event_tx,
            channel_id,
            user_channel_id,
            counterparty_node_id,
            funding_txo,
            channel_type,
        ),

        Event::ChannelReady {
            channel_id,
            user_channel_id,
            counterparty_node_id,
            channel_type,
        } => event::handle_channel_ready(
            channel_events_monitor,
            test_event_tx,
            channel_id,
            user_channel_id,
            counterparty_node_id,
            channel_type,
        ),

        Event::ChannelClosed {
            channel_id,
            user_channel_id,
            reason,
            counterparty_node_id,
            channel_capacity_sats,
            channel_funding_txo,
        } => event::handle_channel_closed(
            channel_events_monitor,
            test_event_tx,
            channel_id,
            user_channel_id,
            reason,
            counterparty_node_id,
            channel_capacity_sats,
            channel_funding_txo,
        ),

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
            payments_manager
                .payment_claimable(payment_hash.into(), amount_msat, purpose)
                .await
                .context("Error handling PaymentClaimable")
                // Want to ensure we always claim funds
                .map_err(EventHandleError::Fatal)?;
        }

        Event::PaymentClaimed {
            receiver_node_id: _,
            payment_hash,
            amount_msat,
            purpose,
            htlcs: _,
            // TODO(max): We probably want to use this to get JIT on-chain fees?
            sender_intended_total_msat: _,
        } => {
            payments_manager
                .payment_claimed(payment_hash.into(), amount_msat, purpose)
                .await
                .context("Error handling PaymentClaimed")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Fatal)?;
        }

        Event::ConnectionNeeded { node_id, addresses } => {
            // The only connection the user node should need is to the LSP.
            // Ignore the event but log an error.
            let node_pk = NodePk(node_id);
            let addrs = addresses
                .into_iter()
                .map(|addr| format!("{addr}"))
                .collect::<Vec<String>>();
            error!(%node_pk, ?addrs, "Unexpected `ConnectionNeeded` event");
            debug_assert!(false);
        }

        Event::InvoiceRequestFailed { payment_id } => {
            // TODO(max): Revisit once we implement BOLT 12
            error!(%payment_id, "Invoice request failed");
        }

        Event::PaymentSent {
            payment_id: _,
            payment_hash,
            payment_preimage,
            fee_paid_msat,
        } => {
            payments_manager
                .payment_sent(
                    payment_hash.into(),
                    payment_preimage.into(),
                    fee_paid_msat,
                )
                .await
                .context("Error handling PaymentSent")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Fatal)?;
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
            payments_manager
                .payment_failed(payment_hash.into(), failure)
                .await
                .context("Error handling PaymentFailed")
                // Don't want to end up with a 'hung' payment state
                .map_err(EventHandleError::Fatal)?;
        }

        Event::PaymentPathSuccessful { .. } => {}

        Event::PaymentPathFailed { .. } => {}

        Event::ProbeSuccessful { .. } => {}

        Event::ProbeFailed { .. } => {}

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
            error!(
                %prev_channel_id, %next_channel_id,
                %prev_user_channel_id, ?next_user_channel_id,
                ?total_fee_earned_msat, ?skimmed_fee_msat,
                %claim_from_onchain_tx, ?outbound_amount_forwarded_msat,
                "Somehow received a PaymentForwarded event??"
            );
            debug_assert!(false);
        }

        Event::HTLCIntercepted { .. } => {
            unreachable!("accept_intercept_htlcs in UserConfig is false")
        }

        Event::HTLCHandlingFailed { .. } => {}

        Event::PendingHTLCsForwardable { time_forwardable } => {
            let forwarding_channel_manager = channel_manager.clone();
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
                channel_manager.clone(),
                keys_manager,
                esplora,
                wallet,
                outputs,
                test_event_tx,
            )
            .await
            .with_context(|| format!("{channel_id:?}"))
            .context("Error handling SpendableOutputs")
            // This is fatal because the outputs are lost if they aren't swept.
            .map_err(EventHandleError::Fatal)?;
        }

        Event::DiscardFunding { .. } => {
            // A "real" node should probably "lock" the UTXOs spent in funding
            // transactions until the funding transaction either confirms, or
            // this event is generated.
        }

        Event::BumpTransaction(_) => {
            // TODO(max): Implement this once we support anchor outputs
        }
    }

    Ok(())
}
