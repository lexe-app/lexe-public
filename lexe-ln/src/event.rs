use anyhow::{anyhow, Context};
use bitcoin::{blockdata::script::Script, secp256k1};
use lightning::{
    chain::chaininterface::ConfirmationTarget, util::events::Event,
};
use thiserror::Error;

use crate::{
    test_event::{TestEvent, TestEventSender},
    traits::{LexeChannelManager, LexePersister},
    wallet::LexeWallet,
};

/// Errors that can occur while handling [`Event`]s.
#[derive(Debug, Error)]
pub enum EventHandleError {
    /// We encountered an tolerable error; log it and move on.
    #[error("Tolerable event handle error: {0:#}")]
    Tolerable(anyhow::Error),
    /// We encountered a fatal error and the node must shut down without losing
    /// the unhandled [`Event`] (i.e. without repersisting the channel manager)
    #[error("Fatal event handle error: {0:#}")]
    Fatal(anyhow::Error),
}

pub fn get_event_name(event: &Event) -> &'static str {
    match event {
        Event::OpenChannelRequest { .. } => "OpenChannelRequest",
        Event::FundingGenerationReady { .. } => "FundingGenerationReady",
        Event::ChannelReady { .. } => "ChannelReady",
        Event::PaymentClaimable { .. } => "PaymentClaimable",
        Event::HTLCIntercepted { .. } => "HTLCIntercepted",
        Event::PaymentClaimed { .. } => "PaymentClaimed",
        Event::PaymentSent { .. } => "PaymentSent",
        Event::PaymentFailed { .. } => "PaymentFailed",
        Event::PaymentPathSuccessful { .. } => "PaymentPathSuccessful",
        Event::PaymentPathFailed { .. } => "PaymentPathFailed",
        Event::ProbeSuccessful { .. } => "ProbeSuccessful",
        Event::ProbeFailed { .. } => "ProbeFailed",
        Event::PendingHTLCsForwardable { .. } => "PendingHTLCsForwardable",
        Event::SpendableOutputs { .. } => "SpendableOutputs",
        Event::PaymentForwarded { .. } => "PaymentForwarded",
        Event::ChannelClosed { .. } => "ChannelClosed",
        Event::DiscardFunding { .. } => "DiscardFunding",
        Event::HTLCHandlingFailed { .. } => "HTLCHandlingFailed",
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
        .await
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
        .inspect(|()| test_event_tx.send(TestEvent::FundingGenerationHandled))
        .map_err(|e| anyhow!("LDK rejected the signed funding tx: {e:?}"))?;

    Ok(())
}
