use anyhow::{anyhow, Context};
use bitcoin::{absolute, secp256k1};
use common::{
    ln::{
        channel::{LxChannelId, LxUserChannelId},
        priority::ConfirmationPriority,
    },
    rng::{Crng, SysRng},
    test_event::TestEvent,
};
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        transaction,
    },
    events::{ClosureReason, Event},
    ln::{features::ChannelTypeFeatures, ChannelId},
    sign::SpendableOutputDescriptor,
};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{
    channel::{ChannelEvent, ChannelEventsMonitor},
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
        Event::ConnectionNeeded { .. } => "ConnectionNeeded",
        Event::InvoiceRequestFailed { .. } => "InvoiceRequestFailed",
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
        Event::BumpTransaction { .. } => "BumpTransaction",
    }
}

/// Handles a [`Event::FundingGenerationReady`].
pub async fn handle_funding_generation_ready<CM, PS>(
    wallet: &LexeWallet,
    channel_manager: &CM,
    test_event_tx: &TestEventSender,

    temporary_channel_id: ChannelId,
    counterparty_node_id: secp256k1::PublicKey,
    channel_value_satoshis: u64,
    output_script: bitcoin::ScriptBuf,
) -> Result<(), EventHandleError>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let conf_prio = ConfirmationPriority::Normal;

    // Sign the funding tx.
    // This can fail if we just don't have enought on-chain funds, so it's a
    // tolerable error.
    let signed_raw_funding_tx = wallet
        .create_and_sign_funding_tx(
            output_script,
            channel_value_satoshis,
            conf_prio,
        )
        .await
        .context("Failed to create channel funding tx")
        .map_err(|create_err| {
            // Make sure we force close the channel. Should not fail.
            if let Err(close_err) = channel_manager
                .force_close_broadcasting_latest_txn(
                    &temporary_channel_id,
                    &counterparty_node_id,
                )
                .map_err(|close_err| {
                    anyhow!(
                        "Failed to force close channel after funding generation \
                         fail: {close_err:?}, funding err: {create_err:#}")
                })
            {
                return EventHandleError::Fatal(close_err);
            }

            // Failing to build the funding tx is tolerable.
            EventHandleError::Tolerable(create_err)
        })?;

    use lightning::util::errors::APIError;
    match channel_manager.funding_transaction_generated(
        &temporary_channel_id,
        &counterparty_node_id,
        signed_raw_funding_tx,
    ) {
        Ok(()) => test_event_tx.send(TestEvent::FundingGenerationHandled),
        Err(APIError::APIMisuseError { err }) =>
            return Err(EventHandleError::Fatal(anyhow!(
                "Failed to finish channel funding generation: \
                 LDK API misuse error: {err}"
            ))),
        Err(err) =>
            return Err(EventHandleError::Tolerable(anyhow!(
                "Failed to handle channel funding generation: {err:?}"
            ))),
    }

    Ok(())
}

/// Handles an [`Event::ChannelPending`]
pub fn handle_channel_pending(
    channel_events_monitor: &ChannelEventsMonitor,
    test_event_tx: &TestEventSender,

    channel_id: ChannelId,
    user_channel_id: u128,
    counterparty_node_id: secp256k1::PublicKey,
    funding_txo: bitcoin::OutPoint,
    channel_type: Option<ChannelTypeFeatures>,
) {
    let channel_id = LxChannelId::from(channel_id);
    let user_channel_id = LxUserChannelId::from(user_channel_id);
    let channel_type = channel_type.expect("Launched after 0.0.122");
    info!(
        %channel_id, %user_channel_id, %counterparty_node_id,
        %funding_txo, %channel_type,
        "Channel pending",
    );
    channel_events_monitor.notify(ChannelEvent::Pending {
        user_channel_id,
        channel_id,
        funding_txo,
    });
    test_event_tx.send(TestEvent::ChannelPending);
}

/// Handles an [`Event::ChannelReady`]
pub fn handle_channel_ready(
    channel_events_monitor: &ChannelEventsMonitor,
    test_event_tx: &TestEventSender,

    channel_id: ChannelId,
    user_channel_id: u128,
    counterparty_node_id: secp256k1::PublicKey,
    channel_type: ChannelTypeFeatures,
) {
    let channel_id = LxChannelId::from(channel_id);
    let user_channel_id = LxUserChannelId::from(user_channel_id);
    info!(
        %channel_id, %user_channel_id,
        %counterparty_node_id, %channel_type,
        "Channel ready",
    );
    channel_events_monitor.notify(ChannelEvent::Ready {
        user_channel_id,
        channel_id,
    });
    test_event_tx.send(TestEvent::ChannelReady);
}

/// Handles an [`Event::ChannelClosed`]
pub fn handle_channel_closed(
    channel_events_monitor: &ChannelEventsMonitor,
    test_event_tx: &TestEventSender,

    channel_id: ChannelId,
    user_channel_id: u128,
    reason: ClosureReason,
    counterparty_node_id: Option<secp256k1::PublicKey>,
    capacity_sats: Option<u64>,
    funding_txo: Option<transaction::OutPoint>,
) {
    let channel_id = LxChannelId::from(channel_id);
    let user_channel_id = LxUserChannelId::from(user_channel_id);
    let counterparty_node_id =
        counterparty_node_id.expect("Launched after v0.0.117");
    let capacity_sats = capacity_sats.expect("Launched after v0.0.117");
    // Contrary to the LDK docs, the funding TXO is None when a new
    // channel negotiation fails.
    // let funding_txo = funding_txo.expect("Launched after v0.0.119");

    info!(
        %channel_id, %user_channel_id, ?reason, %counterparty_node_id,
        %capacity_sats, ?funding_txo,
        "Channel is being closed"
    );

    channel_events_monitor.notify(ChannelEvent::Closed {
        user_channel_id,
        channel_id,
        reason,
    });
    test_event_tx.send(TestEvent::ChannelClosed);
}

/// Handles a [`Event::SpendableOutputs`] by spending any non-static outputs to
/// our BDK wallet.
pub async fn handle_spendable_outputs<CM, PS>(
    channel_manager: CM,
    keys_manager: &LexeKeysManager,
    esplora: &LexeEsplora,
    wallet: &LexeWallet,
    outputs: Vec<SpendableOutputDescriptor>,
    test_event_tx: &TestEventSender,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // The tx only includes a 'change' output, which is actually just a
    // new external address fetched from our wallet.
    // TODO(max): Maybe we should add another output for privacy?
    let spendable_output_descriptors = &outputs.iter().collect::<Vec<_>>();
    let destination_outputs = Vec::new();
    let destination_change_script = wallet.get_address().await?.script_pubkey();
    let feerate_sat_per_1000_weight = esplora
        .get_est_sat_per_1000_weight(ConfirmationTarget::NonAnchorChannelFee);
    let secp_ctx = SysRng::new().gen_secp256k1_ctx();

    // We set nLockTime to the current height to discourage fee sniping.
    let best_height = channel_manager.current_best_block().height;
    let maybe_locktime = absolute::LockTime::from_height(best_height)
        .inspect_err(|e| warn!(%best_height, "Invalid locktime height: {e:#}"))
        .ok();

    let maybe_spending_tx = keys_manager.spend_spendable_outputs(
        spendable_output_descriptors,
        destination_outputs,
        destination_change_script,
        feerate_sat_per_1000_weight,
        maybe_locktime,
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
