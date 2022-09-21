use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::encode;
use bitcoin::secp256k1::Secp256k1;
use bitcoin_bech32::WitnessProgram;
use common::cli::Network;
use common::hex;
use common::task::LxTask;
use lexe_ln::alias::NetworkGraphType;
use lexe_ln::bitcoind::LexeBitcoind;
use lexe_ln::keys_manager::LexeKeysManager;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
};
use lightning::routing::gossip::NodeId;
use lightning::util::events::{Event, EventHandler, PaymentPurpose};
use tokio::runtime::Handle;
use tracing::{debug, error};

use crate::lexe::channel_manager::NodeChannelManager;
use crate::types::{
    HTLCStatus, MillisatAmount, PaymentInfo, PaymentInfoStorageType,
};

pub(crate) struct LdkEventHandler {
    network: Network,
    channel_manager: NodeChannelManager,
    keys_manager: LexeKeysManager,
    bitcoind: Arc<LexeBitcoind>,
    network_graph: Arc<NetworkGraphType>,
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
}

impl LdkEventHandler {
    pub(crate) fn new(
        network: Network,
        channel_manager: NodeChannelManager,
        keys_manager: LexeKeysManager,
        bitcoind: Arc<LexeBitcoind>,
        network_graph: Arc<NetworkGraphType>,
        inbound_payments: PaymentInfoStorageType,
        outbound_payments: PaymentInfoStorageType,
    ) -> Self {
        Self {
            network,
            channel_manager,
            keys_manager,
            bitcoind,
            network_graph,
            inbound_payments,
            outbound_payments,
        }
    }
}

impl EventHandler for LdkEventHandler {
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
    fn handle_event(&self, event: &Event) {
        // FIXME: This trait requires that event handling *finishes* before
        // returning from this function. There isn't currently a clean way to do
        // this, so we block the *entire* program (yes, sucks) until event
        // handling is complete, as an inefficient but safe default. Once LDK
        // #1674 (async event handling) is fixed, we can remove the `block_on`.
        Handle::current().block_on(async move {
            handle_event(
                &self.channel_manager,
                &self.bitcoind,
                &self.network_graph,
                &self.keys_manager,
                &self.inbound_payments,
                &self.outbound_payments,
                self.network,
                event,
            )
            .await
        });
    }
}

// TODO(max): Make this non-async by spawning tasks instead
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_event(
    channel_manager: &NodeChannelManager,
    bitcoind: &LexeBitcoind,
    network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    inbound_payments: &PaymentInfoStorageType,
    outbound_payments: &PaymentInfoStorageType,
    network: Network,
    event: &Event,
) {
    let handle_event_res = handle_event_fallible(
        channel_manager,
        bitcoind,
        network_graph,
        keys_manager,
        inbound_payments,
        outbound_payments,
        network,
        event,
    )
    .await;
    match handle_event_res {
        Ok(()) => {}
        Err(e) => {
            error!("Error handling event: {:#}", e);
        }
    }
}

// TODO(max): Make this non-async by spawning tasks instead
#[allow(clippy::too_many_arguments)]
async fn handle_event_fallible(
    channel_manager: &NodeChannelManager,
    bitcoind: &LexeBitcoind,
    network_graph: &NetworkGraphType,
    keys_manager: &LexeKeysManager,
    inbound_payments: &PaymentInfoStorageType,
    outbound_payments: &PaymentInfoStorageType,
    network: Network,
    event: &Event,
) -> anyhow::Result<()> {
    match event {
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            ..
        } => {
            debug!("Handling FundingGenerationReady event");
            // Construct the raw transaction with one output, that is paid the
            // amount of the channel.
            let addr = WitnessProgram::from_scriptpubkey(
                &output_script[..],
                bitcoin_bech32::constants::Network::from(network),
            )
            .expect("Lightning funding tx should always be to a SegWit output")
            .to_address();
            let mut outputs = vec![HashMap::with_capacity(1)];
            outputs[0]
                .insert(addr, *channel_value_satoshis as f64 / 100_000_000.0);
            let raw_tx = bitcoind
                .create_raw_transaction(outputs)
                .await
                .context("Could not create raw transaction")?;

            // Have your wallet put the inputs into the transaction such that
            // the output is satisfied.
            let funded_tx = bitcoind
                .fund_raw_transaction(raw_tx)
                .await
                .context("Could not fund raw transaction")?;

            // Sign the final funding transaction and broadcast it.
            let signed_tx = bitcoind
                .sign_raw_transaction_with_wallet(funded_tx.hex)
                .await
                .context("Could not sign raw tx with wallet")?;
            assert!(signed_tx.complete);
            let final_tx: Transaction =
                encode::deserialize(&hex::decode(&signed_tx.hex).unwrap())
                    .unwrap();
            // Give the funding transaction back to LDK for opening the channel.
            if channel_manager
                .funding_transaction_generated(
                    temporary_channel_id,
                    counterparty_node_id,
                    final_tx,
                )
                .is_err()
            {
                println!(
					"\nERROR: Channel went away before we could fund it. The peer disconnected or refused the channel.");
                print!("> ");
                io::stdout().flush().unwrap();
            }
        }
        Event::PaymentReceived {
            payment_hash,
            purpose,
            amount_msat,
        } => {
            println!(
				"\nEVENT: received payment from payment hash {} of {} millisatoshis",
				hex::encode(&payment_hash.0),
				amount_msat,
			);
            print!("> ");
            io::stdout().flush().unwrap();
            let payment_preimage = match purpose {
                PaymentPurpose::InvoicePayment {
                    payment_preimage, ..
                } => *payment_preimage,
                PaymentPurpose::SpontaneousPayment(preimage) => Some(*preimage),
            };
            channel_manager.claim_funds(payment_preimage.unwrap());
        }
        Event::PaymentClaimed {
            payment_hash,
            purpose,
            amount_msat,
        } => {
            println!(
				"\nEVENT: claimed payment from payment hash {} of {} millisatoshis",
				hex::encode(&payment_hash.0),
				amount_msat,
			);
            print!("> ");
            io::stdout().flush().unwrap();
            let (payment_preimage, payment_secret) = match purpose {
                PaymentPurpose::InvoicePayment {
                    payment_preimage,
                    payment_secret,
                    ..
                } => (*payment_preimage, Some(*payment_secret)),
                PaymentPurpose::SpontaneousPayment(preimage) => {
                    (Some(*preimage), None)
                }
            };
            let mut payments = inbound_payments.lock().unwrap();
            let payment_entry = payments.entry(*payment_hash);
            match payment_entry {
                Entry::Occupied(mut e) => {
                    let payment = e.get_mut();
                    payment.status = HTLCStatus::Succeeded;
                    payment.preimage = payment_preimage;
                    payment.secret = payment_secret;
                }
                Entry::Vacant(e) => {
                    e.insert(PaymentInfo {
                        preimage: payment_preimage,
                        secret: payment_secret,
                        status: HTLCStatus::Succeeded,
                        amt_msat: MillisatAmount(Some(*amount_msat)),
                    });
                }
            }
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
                if *hash == *payment_hash {
                    payment.preimage = Some(*payment_preimage);
                    payment.status = HTLCStatus::Succeeded;
                    println!(
						"\nEVENT: successfully sent payment of {} millisatoshis{} from \
								 payment hash {:?} with preimage {:?}",
						payment.amt_msat,
						if let Some(fee) = fee_paid_msat {
							format!(" (fee {} msat)", fee)
						} else {
							"".to_string()
						},
						hex::encode(&payment_hash.0),
						hex::encode(&payment_preimage.0)
					);
                    print!("> ");
                    io::stdout().flush().unwrap();
                }
            }
        }
        Event::OpenChannelRequest { .. } => {
            // Unreachable, we don't set manually_accept_inbound_channels
        }
        Event::PaymentPathSuccessful { .. } => {}
        Event::PaymentPathFailed { .. } => {}
        Event::ProbeSuccessful { .. } => {}
        Event::ProbeFailed { .. } => {}
        Event::PaymentFailed { payment_hash, .. } => {
            print!(
				"\nEVENT: Failed to send payment to payment hash {:?}: exhausted payment retry attempts",
				hex::encode(&payment_hash.0)
			);
            print!("> ");
            io::stdout().flush().unwrap();

            let mut payments = outbound_payments.lock().unwrap();
            if payments.contains_key(payment_hash) {
                let payment = payments.get_mut(payment_hash).unwrap();
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

            let node_str = |channel_id: &Option<[u8; 32]>| match channel_id {
                None => String::new(),
                Some(channel_id) => {
                    match channels.iter().find(|c| c.channel_id == *channel_id)
                    {
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
            let channel_str = |channel_id: &Option<[u8; 32]>| {
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

            let from_onchain_str = if *claim_from_onchain_tx {
                "from onchain downstream claim"
            } else {
                "from HTLC fulfill message"
            };
            if let Some(fee_earned) = fee_earned_msat {
                println!(
                    "\nEVENT: Forwarded payment{}{}, earning {} msat {}",
                    from_prev_str, to_next_str, fee_earned, from_onchain_str
                );
            } else {
                println!(
                    "\nEVENT: Forwarded payment{}{}, claiming onchain {}",
                    from_prev_str, to_next_str, from_onchain_str
                );
            }
            print!("> ");
            io::stdout().flush().unwrap();
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
            let destination_address = bitcoind
                .get_new_address()
                .await
                .context("Could not get new address")?;
            let output_descriptors = &outputs.iter().collect::<Vec<_>>();
            let tx_feerate = bitcoind
                .get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
            let spending_tx = keys_manager
                .spend_spendable_outputs(
                    output_descriptors,
                    Vec::new(),
                    destination_address.script_pubkey(),
                    tx_feerate,
                    &Secp256k1::new(),
                )
                .unwrap();
            bitcoind.broadcast_transaction(&spending_tx);
        }
        Event::ChannelClosed {
            channel_id,
            reason,
            user_channel_id: _,
        } => {
            println!(
                "\nEVENT: Channel {} closed due to: {:?}",
                hex::encode(channel_id),
                reason
            );
            print!("> ");
            io::stdout().flush().unwrap();
        }
        Event::DiscardFunding { .. } => {
            // A "real" node should probably "lock" the UTXOs spent in funding
            // transactions until the funding transaction either confirms, or
            // this event is generated.
        }
    }

    Ok(())
}
