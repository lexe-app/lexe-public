use anyhow::{anyhow, Context};
use bitcoin::blockdata::script::Script;
use bitcoin::secp256k1;
use lightning::chain::chaininterface::ConfirmationTarget;
use lightning::util::events::Event;

use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePersister};
use crate::wallet::LexeWallet;

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
pub async fn handle_funding_generation_ready<CM, PS>(
    wallet: &LexeWallet,
    channel_manager: CM,
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
    let conf_target = ConfirmationTarget::Normal;
    let signed_raw_funding_tx = wallet
        .create_and_sign_funding_tx(
            output_script,
            channel_value_satoshis,
            conf_target,
        )
        .context("Could not create and sign funding tx")
        // Force close the pending channel if funding tx generation failed.
        .inspect_err(|_| {
            channel_manager
                .force_close_without_broadcasting_txn(
                    &temporary_channel_id,
                    &counterparty_node_id,
                )
                .expect(
                    "Failed to force close after funding generation failed",
                );
        })?;

    channel_manager
        .funding_transaction_generated(
            &temporary_channel_id,
            &counterparty_node_id,
            signed_raw_funding_tx,
        )
        .inspect(|()| test_event_tx.send(TestEvent::FundingTxHandled))
        .map_err(|e| anyhow!("{e:?}"))
        .context("LDK rejected the signed funding tx")?;

    Ok(())
}
