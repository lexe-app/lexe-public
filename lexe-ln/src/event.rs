use std::collections::HashMap;

use anyhow::Context;
use bitcoin::blockdata::script::Script;
use bitcoin::consensus::encode;
use bitcoin::{secp256k1, Transaction};
use bitcoin_bech32::WitnessProgram;
use common::cli::Network;
use common::hex;
use lightning::util::events::Event;
use tracing::error;

use crate::bitcoind::LexeBitcoind;
use crate::esplora::LexeEsplora;
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePersister};

// TODO(max): Perhaps we should upstream this as a Display impl?
pub fn get_event_name(event: &Event) -> &'static str {
    match event {
        Event::OpenChannelRequest { .. } => "open channel request",
        Event::FundingGenerationReady { .. } => "funding generation ready",
        Event::ChannelReady { .. } => "channel ready",
        Event::PaymentClaimable { .. } => "payment claimable",
        Event::HTLCIntercepted { .. } => "HTLC intercepted",
        Event::PaymentClaimed { .. } => "payment claimed",
        Event::PaymentSent { .. } => "payment sent",
        Event::PaymentFailed { .. } => "payment failed",
        Event::PaymentPathSuccessful { .. } => "payment path successful",
        Event::PaymentPathFailed { .. } => "payment path failed",
        Event::ProbeSuccessful { .. } => "probe successful",
        Event::ProbeFailed { .. } => "probe failed",
        Event::PendingHTLCsForwardable { .. } => "pending HTLCs forwardable",
        Event::SpendableOutputs { .. } => "spendable outputs",
        Event::PaymentForwarded { .. } => "payment forwarded",
        Event::ChannelClosed { .. } => "channel closed",
        Event::DiscardFunding { .. } => "discard funding",
        Event::HTLCHandlingFailed { .. } => "HTLC handling failed",
    }
}

/// Handles a [`Event::FundingGenerationReady`].
#[allow(clippy::too_many_arguments)]
pub async fn handle_funding_generation_ready<CM, PS>(
    channel_manager: CM,
    bitcoind: &LexeBitcoind,
    esplora: &LexeEsplora,
    network: Network,
    test_event_tx: &TestEventSender,

    temporary_channel_id: [u8; 32],
    counterparty_node_id: secp256k1::PublicKey,
    channel_value_satoshis: u64,
    output_script: Script,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Construct the raw transaction with one output, that is paid the
    // amount of the channel.
    let addr = WitnessProgram::from_scriptpubkey(
        &output_script[..],
        bitcoin_bech32::constants::Network::from(network),
    )
    .expect("Lightning funding tx should always be to a SegWit output")
    .to_address();
    let mut outputs = vec![HashMap::with_capacity(1)];
    outputs[0].insert(addr, channel_value_satoshis as f64 / 100_000_000.0);
    let raw_tx = bitcoind
        .create_raw_transaction(outputs)
        .await
        .context("Could not create raw transaction")?;

    // Have your wallet put the inputs into the transaction such that
    // the output is satisfied.
    let funded_tx = bitcoind
        .fund_raw_transaction(raw_tx, esplora)
        .await
        .context("Could not fund raw transaction")?;

    // Sign the final funding transaction and broadcast it.
    let signed_tx = bitcoind
        .sign_raw_transaction_with_wallet(funded_tx.hex)
        .await
        .context("Could not sign raw tx with wallet")?;
    assert!(signed_tx.complete);
    let final_tx: Transaction =
        encode::deserialize(&hex::decode(&signed_tx.hex).unwrap()).unwrap();

    // Give the funding transaction back to LDK for opening the channel.
    match channel_manager.funding_transaction_generated(
        &temporary_channel_id,
        &counterparty_node_id,
        final_tx,
    ) {
        Ok(()) => test_event_tx.send(TestEvent::FundingTxHandled),
        Err(e) => error!(
            "Channel went away before we could fund it. \
            The peer disconnected or refused the channel: {e:?}"
        ),
    }

    // TODO(max): Close the channel if there is an error
    Ok(())
}
