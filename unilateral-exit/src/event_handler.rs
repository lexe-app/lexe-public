//! Event handling for the recovery node.
//!
//! The recovery node only closes channels and sweeps funds, so most LDK
//! events are either channel-close bookkeeping, sweep-related, or unexpected.
//! The node is being retired, so inbound HTLCs are failed back, refunding
//! their senders.

use std::{future::Future, sync::Arc};

use lexe_common::ln::channel::ChannelId;
use lexe_ln::{
    channel::ChannelEvent,
    esplora::FeeEstimates,
    event::{
        self, EventExt, EventHandleError, EventId, LexeEventHandlerMethods,
    },
    keys_manager::LexeKeysManager,
    traits::LexePersister,
    tx_broadcaster::TxBroadcaster,
    wallet::OnchainWallet,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::events::{Event, ReplayEvent};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    alias::{BumpTransactionEventHandlerType, ChannelManagerType},
    persister::RecoveryPersister,
};

#[derive(Clone)]
pub struct RecoveryEventHandler {
    pub(crate) ctx: Arc<EventCtx>,
}

/// Allows all event handling context to be shared (e.g. spawned into a task)
/// with a single [`Arc`] clone.
pub(crate) struct EventCtx {
    pub bump_transaction_handler: BumpTransactionEventHandlerType,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub channel_manager: Arc<ChannelManagerType>,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    pub fee_estimates: Arc<FeeEstimates>,
    pub keys_manager: Arc<LexeKeysManager>,
    pub persister: Arc<RecoveryPersister>,
    pub shutdown: NotifyOnce,
    pub tx_broadcaster: TxBroadcaster,
    pub wallet: OnchainWallet,
}

impl LexeEventHandlerMethods for RecoveryEventHandler {
    #[allow(clippy::manual_async_fn)] // Be more explicit re LDK's API.
    fn get_ldk_handler_future(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> {
        async {
            // `SpendableOutputs` must not be lost, so we persist it and handle
            // it from our own replayable event queue.
            if let Event::SpendableOutputs { .. } = &event {
                self.persist_and_spawn_handler(event).await
            } else {
                self.handle_inline(event).await
            }
        }
    }

    fn handle_event(
        &self,
        event_id: &EventId,
        event: Event,
    ) -> impl Future<Output = Result<(), EventHandleError>> + Send {
        do_handle_event(&self.ctx, event_id, event)
    }

    fn eph_tasks_tx(&self) -> &mpsc::Sender<LxTask<()>> {
        &self.ctx.eph_tasks_tx
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
    event_id: &EventId,
    event: Event,
) -> Result<(), EventHandleError> {
    match event {
        Event::OpenChannelRequest {
            temporary_channel_id,
            counterparty_node_id,
            funding_satoshis: _,
            channel_negotiation_type: _,
            channel_type: _,
            is_announced: _,
            params: _,
        } => {
            warn!("Rejecting inbound channel open during recovery");
            ctx.channel_manager
                .force_close_broadcasting_latest_txn(
                    &temporary_channel_id,
                    &counterparty_node_id,
                    "Recovery node doesn't accept channels".to_owned(),
                )
                .map_err(|e| {
                    EventHandleError::Discard(anyhow::anyhow!(
                        "Error rejecting inbound channel: {e:?}"
                    ))
                })?;
        }

        Event::ChannelPending {
            channel_id,
            user_channel_id,
            former_temporary_channel_id: _,
            counterparty_node_id: _,
            funding_txo,
            channel_type: _,
            funding_redeem_script: _,
        } => {
            ctx.channel_events_bus.send(ChannelEvent::Pending {
                user_channel_id: user_channel_id.into(),
                channel_id: channel_id.into(),
                funding_txo,
            });
        }

        Event::ChannelReady {
            channel_id,
            user_channel_id,
            counterparty_node_id: _,
            channel_type: _,
            funding_txo: _,
        } => {
            ctx.channel_events_bus.send(ChannelEvent::Ready {
                user_channel_id: user_channel_id.into(),
                channel_id: channel_id.into(),
            });
        }

        Event::ChannelClosed {
            channel_id,
            user_channel_id,
            reason,
            counterparty_node_id: _,
            channel_capacity_sats: _,
            channel_funding_txo: _,
            last_local_balance_msat: _,
        } => {
            info!(%channel_id, "Channel closed: {reason}");
            ctx.channel_events_bus.send(ChannelEvent::Closed {
                user_channel_id: user_channel_id.into(),
                channel_id: channel_id.into(),
                reason,
            });
        }

        // We're closing all channels, so fail back inbound payments, refunding
        // their senders.
        Event::PaymentClaimable {
            payment_hash,
            amount_msat: _,
            purpose: _,
            counterparty_skimmed_fee_msat: _,
            onion_fields: _,
            receiver_node_id: _,
            receiving_channel_ids: _,
            claim_deadline: _,
            payment_id: _,
        } => {
            warn!("Failing back inbound payment during recovery");
            ctx.channel_manager.fail_htlc_backwards(&payment_hash);
        }

        Event::SpendableOutputs {
            outputs,
            channel_id,
            counterparty_node_id,
        } => {
            let channel_id =
                ChannelId::from(channel_id.expect("Launched after v0.0.107"));
            let test_event_tx = None;
            event::handle_spendable_outputs(
                ctx.channel_manager.clone(),
                ctx.persister.clone(),
                &ctx.fee_estimates,
                &ctx.keys_manager,
                test_event_tx,
                &ctx.tx_broadcaster,
                &ctx.wallet,
                event_id,
                outputs,
                channel_id,
                counterparty_node_id,
            )
            .await?;
        }

        Event::BumpTransaction(event) =>
            ctx.bump_transaction_handler.handle_event(&event).await,

        Event::HTLCHandlingFailed { .. } | Event::DiscardFunding { .. } => (),

        Event::PaymentClaimed {
            receiver_node_id: _,
            payment_hash,
            amount_msat: _,
            purpose: _,
            htlcs: _,
            sender_intended_total_msat: _,
            onion_fields: _,
            payment_id: _,
        } => warn!(%payment_hash, "Ignoring PaymentClaimed during recovery"),
        Event::PaymentSent {
            payment_id: _,
            payment_hash,
            payment_preimage: _,
            fee_paid_msat: _,
            amount_msat: _,
            bolt12_invoice: _,
        } => warn!(%payment_hash, "Ignoring PaymentSent during recovery"),
        Event::PaymentFailed {
            payment_id: _,
            payment_hash: _,
            reason: _,
        } => warn!("Ignoring PaymentFailed during recovery"),

        // The recovery node opens no channels, sends no payments or probes,
        // never forwards, and ignores onion messages, so none of these
        // should fire.
        Event::FundingGenerationReady { .. }
        | Event::FundingTxBroadcastSafe { .. }
        | Event::FundingTransactionReadyForSigning { .. }
        | Event::SpliceNegotiated { .. }
        | Event::SpliceNegotiationFailed { .. }
        | Event::ConnectionNeeded { .. }
        | Event::InvoiceReceived { .. }
        | Event::PaymentPathSuccessful { .. }
        | Event::PaymentPathFailed { .. }
        | Event::ProbeSuccessful { .. }
        | Event::ProbeFailed { .. }
        | Event::PaymentForwarded { .. }
        | Event::HTLCIntercepted { .. }
        | Event::PersistStaticInvoice { .. }
        | Event::StaticInvoiceRequested { .. }
        | Event::OnionMessageIntercepted { .. }
        | Event::OnionMessagePeerConnected { .. } =>
            warn!("Unexpected event during recovery: {}", event.name()),
    }

    Ok(())
}
