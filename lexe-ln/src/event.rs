use anyhow::{anyhow, Context};
use bitcoin::{blockdata::script::Script, secp256k1};
use common::{rng, rng::SysRng, test_event::TestEvent};
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        keysinterface::SpendableOutputDescriptor,
    },
    events::Event,
};
use thiserror::Error;
use tracing::debug;

use crate::{
    esplora::LexeEsplora,
    keys_manager::LexeKeysManager,
    test_event::TestEventSender,
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
        Event::ChannelPending { .. } => "ChannelPending",
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

/// Handles a [`Event::SpendableOutputs`] by spending any non-static outputs to
/// our BDK wallet.
pub async fn handle_spendable_outputs(
    keys_manager: &LexeKeysManager,
    esplora: &LexeEsplora,
    wallet: &LexeWallet,
    outputs: Vec<SpendableOutputDescriptor>,
    test_event_tx: &TestEventSender,
) -> anyhow::Result<()> {
    // The tx only includes a 'change' output, which is actually just a
    // new external address fetched from our wallet.
    // TODO(max): Maybe we should add another output for privacy?
    let spendable_output_descriptors = &outputs.iter().collect::<Vec<_>>();
    let destination_outputs = Vec::new();
    let destination_change_script = wallet.get_address().await?.script_pubkey();
    let feerate_sat_per_1000_weight =
        esplora.get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
    let secp_ctx = rng::get_randomized_secp256k1_ctx(&mut SysRng::new());
    let maybe_spending_tx = keys_manager.spend_spendable_outputs(
        spendable_output_descriptors,
        destination_outputs,
        destination_change_script,
        feerate_sat_per_1000_weight,
        &secp_ctx,
    )?;
    if let Some(spending_tx) = maybe_spending_tx {
        debug!("Broadcasting tx to spend spendable outputs");
        esplora
            .broadcast_tx(&spending_tx)
            .await
            .context("Couldn't spend spendable outputs")?;
    }

    test_event_tx.send(TestEvent::SpendableOutputs);
    Ok(())
}
