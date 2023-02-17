use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use bitcoin::secp256k1::Secp256k1;
use common::api::NodePk;
use common::cli::LspInfo;
use common::hex;
use common::shutdown::ShutdownChannel;
use common::task::{BlockingTaskRt, LxTask};
use lexe_ln::alias::{NetworkGraphType, PaymentInfoStorageType};
use lexe_ln::esplora::LexeEsplora;
use lexe_ln::event;
use lexe_ln::invoice::HTLCStatus;
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::test_event::{TestEvent, TestEventSender};
use lexe_ln::wallet::LexeWallet;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
};
use lightning::routing::gossip::NodeId;
use lightning::util::events::{Event, EventHandler};
use tracing::{debug, error, info};

use crate::channel_manager::NodeChannelManager;

// We pub(crate) all the fields to prevent having to specify each field two more
// times in Self::new parameters and in struct init syntax.
pub struct NodeEventHandler {
    pub(crate) lsp: LspInfo,
    pub(crate) wallet: LexeWallet,
    pub(crate) channel_manager: NodeChannelManager,
    pub(crate) keys_manager: LexeKeysManager,
    pub(crate) esplora: Arc<LexeEsplora>,
    pub(crate) network_graph: Arc<NetworkGraphType>,
    pub(crate) outbound_payments: PaymentInfoStorageType,
    pub(crate) test_event_tx: TestEventSender,
    // XXX: remove when `EventHandler` is async
    pub(crate) blocking_task_rt: BlockingTaskRt,
    pub(crate) shutdown: ShutdownChannel,
}

impl EventHandler for NodeEventHandler {
    /// Event handling requirements are documented in the [`EventsProvider`]
    /// doc comments:
    ///
    /// - The handling of an event must *complete* before returning from this
    ///   function. Otherwise, if the channel manager gets persisted and the the
    ///   program crashes ("someone trips on a cable") before event handling is
    ///   complete, the event will be lost forever.
    /// - Event handling must be *idempotent*. It must be okay to handle the
    ///   same event twice, since if an event is handled but the program crashes
    ///   before the channel manager is persisted, the same event will be
    ///   emitted again.
    /// - The event handler must avoid reentrancy by not making direct calls to
    ///   `ChannelManager::process_pending_events` or
    ///   `ChainMonitor::process_pending_events`, otherwise there may be a
    ///   deadlock.
    ///
    /// [`EventsProvider`]: lightning::util::events::EventsProvider
    fn handle_event(&self, event: Event) {
        // XXX: This trait requires that event handling *finishes* before
        // returning from this function, but LDK #1674 (async event handling)
        // isn't implemented yet.
        //
        // As a temporary hack and work-around, we create a new thread which
        // only runs `handle_event` tasks. We use a single-threaded runtime for
        // nodes (plus `sqlx::test` proc-macro also forces a single-threaded
        // rt), so we can't use `rt-multi-threaded`, which would let us use
        // `task::block_in_place`.

        let event_name = lexe_ln::event::get_event_name(&event);
        info!("Handling event: {event_name}");
        debug!("Event details: {event:?}");

        // TODO(max): Should be possible to remove all clone()s once async event
        // handlilng is supported
        let lsp = self.lsp.clone();
        let wallet = self.wallet.clone();
        let channel_manager = self.channel_manager.clone();
        let esplora = self.esplora.clone();
        let network_graph = self.network_graph.clone();
        let keys_manager = self.keys_manager.clone();
        let outbound_payments = self.outbound_payments.clone();
        let test_event_tx = self.test_event_tx.clone();
        let shutdown = self.shutdown.clone();

        // NOTE: this blocks the main node event loop; if `handle_event`
        // depends on anything happening in the normal event loop, the whole
        // program WILL deadlock : )
        self.blocking_task_rt.block_on(async move {
            handle_event(
                &lsp,
                &wallet,
                &channel_manager,
                &esplora,
                &network_graph,
                &keys_manager,
                &outbound_payments,
                &test_event_tx,
                &shutdown,
                event,
            )
            .await
        });
    }
}

// TODO(max): Make this non-async by spawning tasks instead
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_event(
    lsp: &LspInfo,
    wallet: &LexeWallet,
    channel_manager: &NodeChannelManager,
    esplora: &LexeEsplora,
    network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    outbound_payments: &PaymentInfoStorageType,
    test_event_tx: &TestEventSender,
    shutdown: &ShutdownChannel,
    event: Event,
) {
    let handle_event_res = handle_event_fallible(
        lsp,
        wallet,
        channel_manager,
        esplora,
        network_graph,
        keys_manager,
        outbound_payments,
        test_event_tx,
        shutdown,
        event,
    )
    .await;

    if let Err(e) = handle_event_res {
        error!("Error handling event: {e:#}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_event_fallible(
    lsp: &LspInfo,
    wallet: &LexeWallet,
    channel_manager: &NodeChannelManager,
    esplora: &LexeEsplora,
    network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    outbound_payments: &PaymentInfoStorageType,
    test_event_tx: &TestEventSender,
    shutdown: &ShutdownChannel,
    event: Event,
) -> anyhow::Result<()> {
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
                    .context("Couldn't reject channel from unknown LSP")?;

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
                    .map_err(|e| anyhow!("{e:?}"))
                    .context("Zero conf required")?;
            }
        }
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            user_channel_id: _,
        } => {
            event::handle_funding_generation_ready(
                wallet,
                channel_manager.clone(),
                test_event_tx,
                temporary_channel_id,
                counterparty_node_id,
                channel_value_satoshis,
                output_script,
            )
            .await
            .context("Failed to handle funding generation ready")?;
        }
        Event::ChannelReady {
            channel_id: _,
            user_channel_id: _,
            counterparty_node_id: _,
            channel_type: _,
        } => {
            test_event_tx.send(TestEvent::ChannelReady);
        }
        Event::PaymentClaimable {
            payment_hash,
            amount_msat,
            purpose,
            receiver_node_id: _,
            via_channel_id: _,
            via_user_channel_id: _,
        } => {
            event::handle_payment_claimable(
                channel_manager.clone(),
                test_event_tx,
                payment_hash,
                amount_msat,
                purpose,
            )
            .context("Failed to handle payment claimable")?;
        }
        Event::PaymentClaimed {
            payment_hash,
            amount_msat,
            purpose: _,
            receiver_node_id: _,
        } => {
            info!(
                "EVENT: claimed payment from payment hash {} of {} millisatoshis",
                hex::encode(&payment_hash.0),
                amount_msat,
            );

            test_event_tx.send(TestEvent::PaymentClaimed);
        }
        Event::PaymentSent {
            payment_preimage,
            payment_hash,
            fee_paid_msat,
            ..
        } => {
            let mut payments = outbound_payments.lock().unwrap();
            let payments_iter = payments.iter_mut();
            for (hash, payment) in payments_iter {
                if *hash == payment_hash {
                    payment.preimage = Some(payment_preimage);
                    payment.status = HTLCStatus::Succeeded;
                    info!(
                        "EVENT: successfully sent payment of {:?} millisatoshis{:?} from \
                                 payment hash {:?} with preimage {:?}",
                        payment.amt_msat,
                        if let Some(fee) = fee_paid_msat {
                            format!(" (fee {fee} msat)")
                        } else {
                            "".to_string()
                        },
                        hex::encode(&payment_hash.0),
                        hex::encode(&payment_preimage.0)
                    );
                }
            }

            test_event_tx.send(TestEvent::PaymentSent);
        }
        Event::PaymentPathSuccessful { .. } => {}
        Event::PaymentPathFailed { .. } => {}
        Event::ProbeSuccessful { .. } => {}
        Event::ProbeFailed { .. } => {}
        Event::PaymentFailed { payment_hash, .. } => {
            error!(
                "Failed to send payment to payment hash {:?}: exhausted payment retry attempts",
                hex::encode(&payment_hash.0)
            );

            let mut payments = outbound_payments.lock().unwrap();
            if payments.contains_key(&payment_hash) {
                let payment = payments.get_mut(&payment_hash).unwrap();
                payment.status = HTLCStatus::Failed;
            }
        }
        Event::PaymentForwarded {
            prev_channel_id,
            next_channel_id,
            fee_earned_msat,
            claim_from_onchain_tx,
        } => {
            let read_only_network_graph = network_graph.read_only();
            let nodes = read_only_network_graph.nodes();
            let channels = channel_manager.list_channels();

            let node_str = |channel_id: Option<[u8; 32]>| match channel_id {
                None => String::new(),
                Some(channel_id) => {
                    match channels.iter().find(|c| c.channel_id == channel_id) {
                        None => String::new(),
                        Some(channel) => {
                            match nodes.get(&NodeId::from_pubkey(
                                &channel.counterparty.node_id,
                            )) {
                                None => " from private node".to_string(),
                                Some(node) => match &node.announcement_info {
                                    None => " from unnamed node".to_string(),
                                    Some(announcement) => {
                                        format!(
                                            " from node {}",
                                            announcement.alias
                                        )
                                    }
                                },
                            }
                        }
                    }
                }
            };
            let channel_str = |channel_id: Option<[u8; 32]>| {
                channel_id
                    .map(|channel_id| {
                        format!(" with channel {}", hex::encode(&channel_id))
                    })
                    .unwrap_or_default()
            };
            let from_prev_str = format!(
                "{}{}",
                node_str(prev_channel_id),
                channel_str(prev_channel_id)
            );
            let to_next_str = format!(
                "{}{}",
                node_str(next_channel_id),
                channel_str(next_channel_id)
            );

            let from_onchain_str = if claim_from_onchain_tx {
                "from onchain downstream claim"
            } else {
                "from HTLC fulfill message"
            };
            if let Some(fee_earned) = fee_earned_msat {
                info!(
                    "EVENT: Forwarded payment{}{}, earning {} msat {}",
                    from_prev_str, to_next_str, fee_earned, from_onchain_str
                );
            } else {
                info!(
                    "EVENT: Forwarded payment{}{}, claiming onchain {}",
                    from_prev_str, to_next_str, from_onchain_str
                );
            }
        }
        Event::HTLCIntercepted { .. } => {
            unreachable!("accept_intercept_htlcs in UserConfig is false")
        }
        Event::HTLCHandlingFailed { .. } => {}
        Event::PendingHTLCsForwardable { time_forwardable } => {
            let forwarding_channel_manager = channel_manager.clone();
            let millis_to_sleep = time_forwardable.as_millis() as u64;
            let _ = LxTask::spawn(async move {
                tokio::time::sleep(Duration::from_millis(millis_to_sleep))
                    .await;
                forwarding_channel_manager.process_pending_htlc_forwards();
            });
        }
        Event::SpendableOutputs { outputs } => {
            let destination_address = wallet.get_new_address().await?;
            let output_descriptors = &outputs.iter().collect::<Vec<_>>();
            let tx_feerate =
                esplora.get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
            let spending_tx = keys_manager
                .spend_spendable_outputs(
                    output_descriptors,
                    Vec::new(),
                    destination_address.script_pubkey(),
                    tx_feerate,
                    &Secp256k1::new(),
                )
                .unwrap();
            esplora.broadcast_transaction(&spending_tx);
        }
        Event::ChannelClosed {
            channel_id,
            reason,
            user_channel_id: _,
        } => {
            info!(
                "EVENT: Channel {} closed due to: {:?}",
                hex::encode(&channel_id),
                reason
            );
        }
        Event::DiscardFunding { .. } => {
            // A "real" node should probably "lock" the UTXOs spent in funding
            // transactions until the funding transaction either confirms, or
            // this event is generated.
        }
    }

    Ok(())
}
