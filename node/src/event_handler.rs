use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use bitcoin::secp256k1::Secp256k1;
use common::{
    api::NodePk,
    cli::LspInfo,
    hex,
    shutdown::ShutdownChannel,
    task::{BlockingTaskRt, LxTask},
};
use lexe_ln::{
    alias::NetworkGraphType,
    esplora::LexeEsplora,
    event,
    keys_manager::LexeKeysManager,
    test_event::{TestEvent, TestEventSender},
    wallet::LexeWallet,
};
use lightning::{
    chain::chaininterface::{
        BroadcasterInterface, ConfirmationTarget, FeeEstimator,
    },
    routing::gossip::NodeId,
    util::events::{Event, EventHandler},
};
use tracing::{debug, error, info};

use crate::{
    alias::NodePaymentsManagerType, channel_manager::NodeChannelManager,
};

// We pub(crate) all the fields to prevent having to specify each field two more
// times in Self::new parameters and in struct init syntax.
pub struct NodeEventHandler {
    pub(crate) lsp: LspInfo,
    pub(crate) wallet: LexeWallet,
    pub(crate) channel_manager: NodeChannelManager,
    pub(crate) keys_manager: LexeKeysManager,
    pub(crate) esplora: Arc<LexeEsplora>,
    pub(crate) network_graph: Arc<NetworkGraphType>,
    pub(crate) payments_manager: NodePaymentsManagerType,
    pub(crate) test_event_tx: TestEventSender,
    // XXX: remove when `EventHandler` is async
    #[allow(dead_code)]
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
        // handling is supported
        let lsp = self.lsp.clone();
        let wallet = self.wallet.clone();
        let channel_manager = self.channel_manager.clone();
        let esplora = self.esplora.clone();
        let network_graph = self.network_graph.clone();
        let keys_manager = self.keys_manager.clone();
        let payments_manager = self.payments_manager.clone();
        let test_event_tx = self.test_event_tx.clone();
        let shutdown = self.shutdown.clone();

        // XXX(max): The EventHandler contract requires us to have *finished*
        // handling the event before returning from this function. But using the
        // BlockingTaskRt is causing other Lexe services (which are run on the
        // same thread in the integration tests) to be unresponsive when the
        // node handles events. So we hack around it by breaking the contract
        // and just handling the event in a detached task. The long term fix is
        // to move to async event handling, which should be straightforward.
        #[allow(clippy::redundant_async_block)]
        LxTask::spawn(async move {
            handle_event(
                &lsp,
                &wallet,
                &channel_manager,
                &esplora,
                &network_graph,
                &keys_manager,
                &payments_manager,
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
    payments_manager: &NodePaymentsManagerType,
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
        payments_manager,
        test_event_tx,
        shutdown,
        event,
    )
    .await;

    if let Err(e) = handle_event_res {
        error!("Error handling event: {e:#}");
    }
}

async fn handle_event_fallible(
    lsp: &LspInfo,
    wallet: &LexeWallet,
    channel_manager: &NodeChannelManager,
    esplora: &LexeEsplora,
    network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    payments_manager: &NodePaymentsManagerType,
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
            payments_manager
                .payment_claimable(payment_hash, amount_msat, purpose)
                .await
                .context("Error handling PaymentClaimable")?;
        }
        Event::PaymentClaimed {
            payment_hash,
            amount_msat,
            purpose,
            receiver_node_id: _,
        } => {
            payments_manager
                .payment_claimed(payment_hash, amount_msat, purpose)
                .await
                .context("Error handling PaymentClaimed")?;
        }
        Event::PaymentSent {
            payment_id: _,
            payment_hash,
            payment_preimage,
            fee_paid_msat,
        } => {
            payments_manager
                .payment_sent(payment_hash, payment_preimage, fee_paid_msat)
                .await
                .context("Error handling PaymentSent")?;
        }
        Event::PaymentFailed {
            payment_id: _,
            payment_hash,
        } => {
            payments_manager
                .payment_failed(payment_hash)
                .await
                .context("Error handling PaymentFailed")?;
        }
        Event::PaymentPathSuccessful { .. } => {}
        Event::PaymentPathFailed { .. } => {}
        Event::ProbeSuccessful { .. } => {}
        Event::ProbeFailed { .. } => {}
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
            LxTask::spawn(async move {
                tokio::time::sleep(Duration::from_millis(millis_to_sleep))
                    .await;
                forwarding_channel_manager.process_pending_htlc_forwards();
            })
            .detach();
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
