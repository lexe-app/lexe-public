use std::sync::Mutex;

use anyhow::{anyhow, Context};
use bitcoin::{absolute, secp256k1};
use common::{
    ln::channel::{LxChannelId, LxUserChannelId},
    notify,
    rng::{Crng, RngExt, SysRng},
    test_event::TestEvent,
    time::TimestampMs,
};
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        transaction,
    },
    events::{ClosureReason, Event, PathFailure},
    ln::{features::ChannelTypeFeatures, types::ChannelId},
    routing::scoring::ScoreUpdate,
    sign::SpendableOutputDescriptor,
};
use thiserror::Error;
use tracing::{debug, info, info_span, warn};

use crate::{
    alias::{NetworkGraphType, ProbabilisticScorerType},
    channel::{ChannelEvent, ChannelEventsBus},
    esplora::LexeEsplora,
    keys_manager::LexeKeysManager,
    test_event::TestEventSender,
    traits::{LexeChannelManager, LexePersister},
    wallet::LexeWallet,
};

/// Specifies what to do with a [`Event`] after getting this error handling it.
#[derive(Debug, Error)]
pub enum EventHandleError {
    /// Discard the [`Event`], log the error, and move on. Either this event
    /// isn't important, or the event was resolved in some other way.
    ///
    /// NOTE: As of LDK v0.0.124, returning [`ReplayEvent`] to LDK will prevent
    /// any subsequent events from making progress until handling of this event
    /// succeeds. Until that is resolved, we should (re-)persist the event, and
    /// return [`ReplayEvent`] only if persistence fails. See:
    /// <https://github.com/lightningdevkit/rust-lightning/issues/2491#issuecomment-2466036948>
    ///
    /// [`ReplayEvent`]: lightning::events::ReplayEvent
    #[error("EventHandleError (Discard): {0:#}")]
    Discard(anyhow::Error),
    /// We must not lose this unhandled [`Event`].
    /// Keep replaying the event until handling succeeds.
    #[error("EventHandleError (Replay): {0:#}")]
    Replay(anyhow::Error),
}

/// Small extension trait which adds some methods to LDK's [`Event`] type.
pub trait EventExt {
    /// Returns the name of the event.
    fn name(&self) -> &'static str;

    /// Get a unique string ID for this event.
    /// Current format: `<timestamp_ms>-<nonce>-<event_name>`
    fn id(&self) -> String;

    /// A method to call just as we begin to handle an event.
    /// - Logs "Handling event: {name}" at INFO
    /// - Logs the event details at DEBUG if running in debug mode
    /// - Returns the event ID and a [`tracing::Span`] for the event
    fn handle_prelude(&self) -> (String, tracing::Span);

    /// Calls [`Self::handle_prelude`] with an existing event ID.
    fn handle_prelude_with_id(&self, event_id: &str) -> tracing::Span;
}

impl EventExt for Event {
    /// Get the name of the event, without any event details.
    fn name(&self) -> &'static str {
        match self {
            Event::OpenChannelRequest { .. } => "OpenChannelRequest",
            Event::FundingGenerationReady { .. } => "FundingGenerationReady",
            Event::FundingTxBroadcastSafe { .. } => "FundingTxBroadcastSafe",
            Event::ChannelPending { .. } => "ChannelPending",
            Event::ChannelReady { .. } => "ChannelReady",
            Event::ChannelClosed { .. } => "ChannelClosed",
            Event::PaymentClaimable { .. } => "PaymentClaimable",
            Event::PaymentClaimed { .. } => "PaymentClaimed",
            Event::ConnectionNeeded { .. } => "ConnectionNeeded",
            Event::InvoiceReceived { .. } => "InvoiceReceived",
            Event::PaymentSent { .. } => "PaymentSent",
            Event::PaymentFailed { .. } => "PaymentFailed",
            Event::PaymentPathSuccessful { .. } => "PaymentPathSuccessful",
            Event::PaymentPathFailed { .. } => "PaymentPathFailed",
            Event::ProbeSuccessful { .. } => "ProbeSuccessful",
            Event::ProbeFailed { .. } => "ProbeFailed",
            Event::PaymentForwarded { .. } => "PaymentForwarded",
            Event::HTLCIntercepted { .. } => "HTLCIntercepted",
            Event::HTLCHandlingFailed { .. } => "HTLCHandlingFailed",
            Event::PendingHTLCsForwardable { .. } => "PendingHTLCsForwardable",
            Event::SpendableOutputs { .. } => "SpendableOutputs",
            Event::DiscardFunding { .. } => "DiscardFunding",
            Event::BumpTransaction { .. } => "BumpTransaction",
            Event::OnionMessageIntercepted { .. } => "OnionMessageIntercepted",
            Event::OnionMessagePeerConnected { .. } =>
                "OnionMessagePeerConnected",
        }
    }

    fn id(&self) -> String {
        let timestamp_ms = TimestampMs::now().into_u64();
        // Prevents duplicate keys with high probability.
        let nonce = SysRng::new().gen_u32();
        let event_name = self.name();
        format!("{timestamp_ms}-{nonce}-{event_name}")
    }

    fn handle_prelude(&self) -> (String, tracing::Span) {
        let event_id = self.id();
        let span = self.handle_prelude_with_id(&event_id);
        (event_id, span)
    }

    fn handle_prelude_with_id(&self, event_id: &str) -> tracing::Span {
        info!(%event_id, "Handling event: {name}", name = self.name());
        #[cfg(debug_assertions)] // Events contain sensitive info
        debug!(%event_id, "Event details: {self:?}");
        info_span!("(event)", %event_id)
    }
}

/// Handles a [`Event::FundingGenerationReady`].
pub fn handle_funding_generation_ready<CM, PS>(
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
    // Sign the funding tx. This can fail if we just don't have enought on-chain
    // funds, so it's a tolerable error.
    let channel_value = bitcoin::Amount::from_sat(channel_value_satoshis);
    let signed_raw_funding_tx = wallet
        .create_and_sign_funding_tx(output_script, channel_value)
        .context("Failed to create channel funding tx")
        .or_else(|create_err| {
            // Make sure we force close the channel.
            channel_manager
                .force_close_without_broadcasting_txn(
                    &temporary_channel_id,
                    &counterparty_node_id,
                    "Failed to create channel funding transaction".to_owned(),
                )
                .map_err(|close_err| {
                    anyhow!(
                        "Force close failed after funding generation failed: \
                        create_err: {create_err:#}; close_err: {close_err:?}"
                    )
                })
                // Force closing the channel should not fail.
                .map_err(EventHandleError::Replay)?;

            // Failing to build the funding tx is tolerable.
            Err(EventHandleError::Discard(create_err))
        })?;

    use lightning::util::errors::APIError;
    match channel_manager.funding_transaction_generated(
        temporary_channel_id,
        counterparty_node_id,
        signed_raw_funding_tx,
    ) {
        Ok(()) => test_event_tx.send(TestEvent::FundingGenerationHandled),
        Err(APIError::APIMisuseError { err }) =>
            return Err(EventHandleError::Discard(anyhow!(
                "Failed to finish channel funding generation: \
                 LDK API misuse error: {err}"
            ))),
        Err(err) =>
            return Err(EventHandleError::Discard(anyhow!(
                "Failed to handle channel funding generation: {err:?}"
            ))),
    }

    Ok(())
}

/// Handles an [`Event::ChannelPending`]
pub fn handle_channel_pending(
    channel_events_bus: &ChannelEventsBus,
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
    channel_events_bus.notify(ChannelEvent::Pending {
        user_channel_id,
        channel_id,
        funding_txo,
    });
    test_event_tx.send(TestEvent::ChannelPending);
}

/// Handles an [`Event::ChannelReady`]
pub fn handle_channel_ready(
    channel_events_bus: &ChannelEventsBus,
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
    channel_events_bus.notify(ChannelEvent::Ready {
        user_channel_id,
        channel_id,
    });
    test_event_tx.send(TestEvent::ChannelReady);
}

/// Handles an [`Event::ChannelClosed`]
pub fn handle_channel_closed(
    channel_events_bus: &ChannelEventsBus,
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

    channel_events_bus.notify(ChannelEvent::Closed {
        user_channel_id,
        channel_id,
        reason,
    });
    test_event_tx.send(TestEvent::ChannelClosed);
}

/// If the given [`Event`] contains information the [`NetworkGraphType`] should
/// be updated with, updates the network graph accordingly.
///
/// Based on the `handle_network_graph_update` fn in LDK's BGP:
/// <https://github.com/lightningdevkit/rust-lightning/blob/8da30df223d50099c75ba8251615bd2026fcea75/lightning-background-processor/src/lib.rs#L257>
pub fn handle_network_graph_update(
    network_graph: &NetworkGraphType,
    event: &Event,
) {
    if let Event::PaymentPathFailed {
        failure:
            PathFailure::OnPath {
                network_update: Some(ref update),
            },
        ..
    } = event
    {
        network_graph.handle_network_update(update);
    }
}

/// If the given [`Event`] contains information the [`ProbabilisticScorerType`]
/// should be updated with, this fn updates the scorer accordingly and notifies
/// the BGP to re-persist the scorer.
///
/// Based on the `update_scorer` fn in LDK's BGP:
/// <https://github.com/lightningdevkit/rust-lightning/blob/8da30df223d50099c75ba8251615bd2026fcea75/lightning-background-processor/src/lib.rs#L272>
pub fn handle_scorer_update(
    scorer: &Mutex<ProbabilisticScorerType>,
    scorer_persist_tx: &notify::Sender,
    event: &Event,
) {
    let now_since_epoch = TimestampMs::now().into_duration();
    match event {
        Event::PaymentPathFailed {
            ref path,
            short_channel_id: Some(scid),
            ..
        } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.payment_path_failed(path, *scid, now_since_epoch);
            scorer_persist_tx.send();
        }
        Event::PaymentPathFailed {
            ref path,
            payment_failed_permanently: true,
            ..
        } => {
            // This branch is hit if the destination explicitly failed it back.
            // This is treated as a successful probe because the payment made it
            // all the way to the destination with sufficient liquidity.
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.probe_successful(path, now_since_epoch);
            scorer_persist_tx.send();
        }
        Event::PaymentPathSuccessful { path, .. } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.payment_path_successful(path, now_since_epoch);
            scorer_persist_tx.send();
        }
        Event::ProbeSuccessful { path, .. } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.probe_successful(path, now_since_epoch);
            scorer_persist_tx.send();
        }
        Event::ProbeFailed {
            path,
            short_channel_id: Some(scid),
            ..
        } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.probe_failed(path, *scid, now_since_epoch);
            scorer_persist_tx.send();
        }
        _ => (),
    }
}

/// Handles a [`Event::SpendableOutputs`] by spending any non-static outputs to
/// our BDK wallet.
pub async fn handle_spendable_outputs<CM, PS>(
    channel_manager: CM,
    keys_manager: &LexeKeysManager,
    esplora: &LexeEsplora,
    wallet: &LexeWallet,
    test_event_tx: &TestEventSender,
    outputs: Vec<SpendableOutputDescriptor>,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // The tx only includes a 'change' output, which is actually just a
    // new internal address fetched from our wallet.
    // TODO(max): Maybe we should add another output for privacy?
    let spendable_output_descriptors = &outputs.iter().collect::<Vec<_>>();
    let destination_outputs = Vec::new();
    let destination_change_script =
        wallet.get_internal_address().script_pubkey();
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
