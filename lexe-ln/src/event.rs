use std::{fmt, future::Future, str::FromStr, sync::Mutex, time::Duration};

use anyhow::{Context, anyhow};
use bitcoin::{absolute, consensus::Encodable, secp256k1};
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    api::test_event::TestEvent,
    ln::channel::{LxChannelId, LxUserChannelId},
    rng::{Crng, RngExt, SysRng},
    time::{DisplayMs, TimestampMs},
};
use lexe_api::vfs::{self, Vfs, VfsFile, VfsFileId};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        transaction,
    },
    events::{ClosureReason, Event, PathFailure, ReplayEvent},
    ln::types::ChannelId,
    routing::scoring::ScoreUpdate,
    sign::SpendableOutputDescriptor,
    types::features::ChannelTypeFeatures,
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::{
    TxDisplay,
    alias::{NetworkGraphType, ProbabilisticScorerType},
    esplora::FeeEstimates,
    keys_manager::LexeKeysManager,
    persister::LexePersisterMethods,
    test_event::TestEventSender,
    traits::{LexeChannelManager, LexeEventHandler, LexePersister},
    tx_broadcaster::TxBroadcaster,
    wallet::OnchainWallet,
};

// --- EventHandleError --- //

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
    ///
    /// NOTE: Any [`Event`] kind whose handling may return this variant must be
    /// first persisted and then managed in our own queue! LDK's default
    /// behavior is to immediately replay any failed events, which will cause
    /// us to busyloop and potentially spam our infrastructure providers.
    /// See usages of `persist_and_spawn_handler` for examples.
    #[error("EventHandleError (Replay): {0:#}")]
    Replay(anyhow::Error),
}

/// Small extension trait which adds some methods to LDK's [`Event`] type.
pub trait EventExt {
    /// Returns the name of the event.
    fn name(&self) -> &'static str;

    /// A method to call just as we begin to handle an event.
    /// - Logs "Handling event: {name}" at INFO
    /// - Logs the event details at DEBUG if running in debug mode
    /// - Returns the event ID and a [`tracing::Span`] for the event
    fn handle_prelude(&self) -> (EventId, tracing::Span);

    /// Calls [`Self::handle_prelude`] with an existing event ID.
    fn handle_prelude_with_id(&self, event_id: &EventId) -> tracing::Span;
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

    fn handle_prelude(&self) -> (EventId, tracing::Span) {
        let event_id = EventId::generate(self);
        let span = self.handle_prelude_with_id(&event_id);
        (event_id, span)
    }

    fn handle_prelude_with_id(&self, event_id: &EventId) -> tracing::Span {
        info!(%event_id, "Handling event: {name}", name = self.name());
        #[cfg(debug_assertions)] // Events contain sensitive info
        debug!(%event_id, "Event details: {self:?}");
        info_span!("(event)", %event_id)
    }
}

// --- EventId --- //

/// A unique identifier for an [`Event`].
/// Serialized and displayed as `<timestamp_ms>-<nonce>-<event_name>`.
#[derive(Clone, Debug, PartialEq, SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct EventId {
    pub timestamp: TimestampMs,
    pub nonce: u32,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub name: String,
}

impl EventId {
    /// Generates a new [`EventId`] for the given [`Event`].
    pub fn generate(event: &Event) -> Self {
        let timestamp = TimestampMs::now();
        // Prevents duplicate keys with high probability.
        let nonce = SysRng::new().gen_u32();
        let name = event.name().to_owned();
        Self {
            timestamp,
            nonce,
            name,
        }
    }

    /// A short version of the event ID: `<timestamp_ms>-<nonce>`.
    pub fn short(&self) -> String {
        let timestamp = &self.timestamp;
        let nonce = &self.nonce;
        format!("{timestamp}-{nonce}")
    }
}

impl FromStr for EventId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (timestamp_str, rest) =
            s.split_once('-').context("Missing timestamp")?;
        let timestamp = TimestampMs::from_str(timestamp_str)
            .context("Invalid timestamp")?;
        let (nonce_str, name) =
            rest.split_once('-').context("Missing nonce")?;
        let nonce = u32::from_str(nonce_str).context("Invalid nonce")?;
        Ok(Self {
            timestamp,
            nonce,
            name: name.to_owned(),
        })
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timestamp = &self.timestamp;
        let nonce = &self.nonce;
        let name = &self.name;
        write!(f, "{timestamp}-{nonce}-{name}")
    }
}

// --- Event replayer task --- //

/// A task which periodically replays unhandled events (then deletes them).
pub fn spawn_event_replayer_task(handler: impl LexeEventHandler) -> LxTask<()> {
    /// How often to replay persisted events that weren't successfully
    /// handled. Current: once every 10 minutes.
    const EVENT_REPLAY_INTERVAL: Duration = Duration::from_secs(10 * 60);

    async fn do_event_replays(
        handler: &impl LexeEventHandler,
    ) -> anyhow::Result<()> {
        let ids_and_events = handler
            .persister()
            .read_events()
            .await
            .context("Failed to read events")?;

        for (event_id, event) in ids_and_events {
            handler.replay_event(event_id, event).await;
        }

        // NOTE: Consider switching back to concurrent event replays.
        /*
        let replay_futures = ids_and_events
            .into_iter()
            .map(|(event_id, event)| handler.replay_event(event_id, event));
        futures::future::join_all(replay_futures).await;
        */

        Ok(())
    }

    let mut shutdown = handler.shutdown().clone();

    LxTask::spawn("event replayer", async move {
        let mut replay_timer = tokio::time::interval(EVENT_REPLAY_INTERVAL);

        loop {
            tokio::select! {
                _ = replay_timer.tick() => (),
                () = shutdown.recv() => break,
            }

            let replay_result = tokio::select! {
                res = do_event_replays(&handler) => res,
                () = shutdown.recv() => break,
            };

            if let Err(e) = replay_result {
                error!("Error replaying events: {e:#}");
            }
        }

        info!("Event replayer task shutting down");
    })
}

// --- LexeEventHandlerMethods --- //

/// All event handler methods needed in shared Lexe LN logic.
pub trait LexeEventHandlerMethods: Clone + Send + Sync + 'static {
    // --- Required methods --- //

    /// Given a LDK [`Event`], get a future which handles it.
    /// The BGP passes this future to LDK for async event handling.
    fn get_ldk_handler_future(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> + Send;

    /// Handle an event.
    fn handle_event(
        &self,
        event_id: &EventId,
        event: Event,
    ) -> impl Future<Output = Result<(), EventHandleError>> + Send;

    fn persister(&self) -> &impl LexePersister;

    fn shutdown(&self) -> &NotifyOnce;

    // --- Provided methods --- //

    /// Wraps [`Self::handle_event`]: Handles an event 'inline',
    /// i.e. without spawning off to a task.
    fn handle_inline(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> {
        let (event_id, span) = event.handle_prelude();

        async move {
            match self.handle_event(&event_id, event).await {
                Ok(()) => {
                    info!("Successfully handled event");
                    Ok(())
                }
                Err(EventHandleError::Discard(e)) => {
                    warn!("Tolerable event error, discarding event: {e:#}");
                    Ok(())
                }
                Err(EventHandleError::Replay(e)) => {
                    error!(
                        "Got `EventHandleError::Replay` while handling an \
                         event inline! This event kind should have been \
                         handled with `persist_and_spawn_handler` to avoid \
                         busylooping and potentially spamming our providers in \
                         the case of an error. Initiating shutdown: {e:#}"
                    );
                    // The BGP's shutdown branch will be ready at its next
                    // iteration, so even if we immediately replay this event a
                    // few more times (due to `tokio::select!` choosing randomly
                    // amongst ready futures) that's OK; we'll shut down soon.
                    self.shutdown().send();
                    Err(ReplayEvent())
                }
            }
        }
        // Instrument all logs for this event with the event span
        .instrument(span)
    }

    /// Wraps [`Self::handle_event`]: Persists the event and spawns a task to
    /// handle it. The handler task deletes the event once it is successfully
    /// handled. This fn returns when persistence is complete.
    fn persist_and_spawn_handler(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> + Send {
        async move {
            let (event_id, span) = event.handle_prelude();

            // Notifies the handler task when the persist attempt finishes.
            let (persist_tx, persist_rx) = oneshot::channel();

            // Immediately spawn off the handler, so as to reduce latency a bit.
            LxTask::spawn(event_id.to_string(), {
                let myself = self.clone();
                let event = event.clone();
                let event_id = event_id.clone();
                async move {
                    match myself.handle_event(&event_id, event).await {
                        Ok(()) => {
                            info!(
                                "Successfully handled event; \
                                 removing from Lexe event queue."
                            );
                            // The event was handled; we can delete it now. Wait
                            // for persistence attempt to finish first so we
                            // don't accidentally recreate after deletion.
                            let _ = persist_rx.await;
                            match myself
                                .persister()
                                .remove_event(&event_id)
                                .await
                            {
                                Ok(()) => debug!("Deleted handled event"),
                                Err(e) => warn!("Failed to delete event: {e}"),
                            }
                        }
                        Err(EventHandleError::Discard(e)) => {
                            warn!(
                                "Tolerable error handling spawned event; \
                                 discarding event: {e:#}"
                            );
                            // The error is tolerable; we can delete it now.
                            // Wait for persistence attempt to finish first so
                            // we don't accidentally recreate after deletion.
                            let _ = persist_rx.await;
                            match myself
                                .persister()
                                .remove_event(&event_id)
                                .await
                            {
                                Ok(()) => debug!("Deleted tolerated event"),
                                Err(e) =>
                                    warn!("Failed to delete tol event: {e}"),
                            }
                        }
                        // No need to return `ReplayEvent` here because this
                        // event is managed via Lexe's own event queue. So long
                        // as the event is persisted, our event replayer task
                        // will just keep retrying until the event is handled.
                        Err(EventHandleError::Replay(e)) => error!(
                            "Critical error handling spawned event; \
                             keeping in queue to be replayed later: {e:#}"
                        ),
                    }
                }
                .instrument(span.clone())
            })
            .detach();

            // Ensure the event is persisted before we return.
            let persist_result = async {
                match self.persister().persist_event(&event, &event_id).await {
                    Ok(()) => debug!("Persisted event"),
                    // We failed to persist the event. We don't want to lose the
                    // event, so we return `ReplayEvent`, but we also don't want
                    // to busyloop and spam our infra trying to persist this
                    // event, so we also initiate a shutdown.
                    Err(e) => {
                        error!(
                            "Failed to persist event; initiating shutdown and \
                             returning ReplayEvent to LDK: {e:#}"
                        );
                        // The BGP's shutdown branch will be ready at its next
                        // iteration, so even if we immediately replay this
                        // event a few more times (due to `tokio::select!`
                        // polling branches randomly) that's OK; we'll be
                        // shutting down soon.
                        self.shutdown().send();
                        return Err(ReplayEvent());
                    }
                }

                Ok::<(), ReplayEvent>(())
            }
            .instrument(span)
            .await;

            // Notify the handler regardless of the result.
            let _ = persist_tx.send(());

            persist_result
        }
    }

    /// Wraps [`Self::handle_event`]: Replays a persisted event by re-handling
    /// it, then deletes the persisted event if successful.
    fn replay_event(
        &self,
        event_id: EventId,
        event: Event,
    ) -> impl Future<Output = ()> + Send {
        let span = event.handle_prelude_with_id(&event_id);

        async move {
            match self.handle_event(&event_id, event).await {
                Ok(()) => {
                    info!("Successfully replayed event");
                    match self.persister().remove_event(&event_id).await {
                        Ok(()) => debug!("Deleted replayed event"),
                        Err(e) => warn!("Couldn't delete replayed event: {e}"),
                    }
                }
                Err(EventHandleError::Discard(e)) => {
                    info!(
                        "Tolerable error handling replayed event; \
                         discarding event: {e:#}"
                    );
                    match self.persister().remove_event(&event_id).await {
                        Ok(()) => debug!("Deleted tolerated replayed event"),
                        Err(e) => warn!(
                            "Couldn't delete tolerated replayed event: {e}"
                        ),
                    }
                }
                // No need to do anything further here because this event is
                // already in Lexe's event queue. If the handling fails, we'll
                // just try again at the next replay timer tick.
                Err(EventHandleError::Replay(e)) => warn!(
                    "Critical error handling replayed event; \
                     keeping in queue to be replayed later: {e:#}"
                ),
            }
        }
        // Instrument all logs for this event with the event span
        .instrument(span)
    }
}

// --- Shared handlers --- //

/// Handles a [`Event::FundingGenerationReady`].
pub fn handle_funding_generation_ready<CM, PS>(
    wallet: &OnchainWallet,
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
pub fn log_channel_pending(
    channel_id: LxChannelId,
    user_channel_id: LxUserChannelId,
    counterparty_node_id: secp256k1::PublicKey,
    funding_txo: bitcoin::OutPoint,
    channel_type: Option<ChannelTypeFeatures>,
) {
    let channel_type = channel_type.expect("Launched after 0.0.122");
    info!(
        %channel_id, %user_channel_id, %counterparty_node_id,
        %funding_txo, %channel_type,
        "Channel pending",
    );
}

/// Logs an [`Event::ChannelReady`]
pub fn log_channel_ready(
    channel_id: LxChannelId,
    user_channel_id: LxUserChannelId,
    counterparty_node_id: secp256k1::PublicKey,
    channel_type: ChannelTypeFeatures,
) {
    info!(
        %channel_id, %user_channel_id,
        %counterparty_node_id, %channel_type,
        "Channel ready",
    );
}

/// Logs an [`Event::ChannelClosed`].
pub fn log_channel_closed(
    channel_id: LxChannelId,
    user_channel_id: LxUserChannelId,
    reason: &ClosureReason,
    counterparty_node_id: Option<secp256k1::PublicKey>,
    capacity_sats: Option<u64>,
    funding_txo: Option<transaction::OutPoint>,
) {
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
                network_update: Some(update),
            },
        ..
    } = event
    {
        network_graph.handle_network_update(update);
    }
}

/// If the given [`Event`] contains information the [`ProbabilisticScorerType`]
/// should be updated with, this fn updates the scorer accordingly.
///
/// Based on the `update_scorer` fn in LDK's BGP:
/// <https://github.com/lightningdevkit/rust-lightning/blob/8da30df223d50099c75ba8251615bd2026fcea75/lightning-background-processor/src/lib.rs#L272>
///
/// NOTE: Unlike LDK's BGP, we don't notify the BGP if the scorer was updated,
/// as we already have the scorer on an auto-persist interval, including a final
/// persist at shutdown. It's OK if we lose the last ~5 min of data in a crash.
pub fn handle_scorer_update(
    scorer: &Mutex<ProbabilisticScorerType>,
    event: &Event,
) {
    let now_since_epoch = TimestampMs::now().to_duration();
    match event {
        Event::PaymentPathFailed {
            path,
            short_channel_id: Some(scid),
            ..
        } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.payment_path_failed(path, *scid, now_since_epoch);
        }
        Event::PaymentPathFailed {
            path,
            payment_failed_permanently: true,
            ..
        } => {
            // This branch is hit if the destination explicitly failed it back.
            // This is treated as a successful probe because the payment made it
            // all the way to the destination with sufficient liquidity.
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.probe_successful(path, now_since_epoch);
        }
        Event::PaymentPathSuccessful { path, .. } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.payment_path_successful(path, now_since_epoch);
        }
        Event::ProbeSuccessful { path, .. } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.probe_successful(path, now_since_epoch);
        }
        Event::ProbeFailed {
            path,
            short_channel_id: Some(scid),
            ..
        } => {
            let mut locked_scorer = scorer.lock().unwrap();
            locked_scorer.probe_failed(path, *scid, now_since_epoch);
        }
        _ => (),
    }
}

/// Indicates that a call to `process_pending_htlc_forwards` was complete.
#[derive(Copy, Clone, Debug)]
pub struct HtlcsForwarded;

pub fn handle_pending_htlcs_forwardable<CM, PS>(
    channel_manager: CM,
    htlcs_forwarded_bus: EventsBus<HtlcsForwarded>,
    eph_tasks_tx: &mpsc::Sender<LxTask<()>>,
) where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // According to the API, we are supposed to wait some random time between
    // `[time_forwardable, 5 * time_forwardable]` before forwarding HTLCs to
    // "increase the effort required to correlate payments" (LDK sets
    // `time_forwardable` to 100ms). But this provides minimal privacy benefit
    // for us, so we opted out of this and started forwarding payments
    // immediately. But then this drastically worsened payment latency because
    // the delay allowed us to batch channel monitor persists in the case of MPP
    // and some other cases.
    // TODO(max): We're going with LDK's default of 100ms for now, but that
    // obviously adds payment latency, so we should try to reduce this once we
    // have a better understanding of what we're waiting for.
    let delay = Duration::from_millis(100);
    let delay_ms = DisplayMs(delay);
    const SPAN_NAME: &str = "(pending-htlcs-forwardable)";
    info_span!(SPAN_NAME, %delay_ms).in_scope(|| {
        info!("Sleeping {delay_ms} before forwarding");

        let task = LxTask::spawn_unlogged(SPAN_NAME, async move {
            tokio::time::sleep(delay).await;
            channel_manager.process_pending_htlc_forwards();
            info!("Forwarded pending HTLCs");
            htlcs_forwarded_bus.send(HtlcsForwarded);
        });

        if eph_tasks_tx.try_send(task).is_err() {
            warn!("Couldn't send task");
        }
    });
}

/// Handles a [`Event::SpendableOutputs`] by spending any non-static outputs to
/// our BDK wallet.
//
// Event sources:
// - `EventHandler` -> `Event::SpendableOutputs` (replayable)
// NOTE: Err(Replay) ==> must be handled idempotently
// TODO(phlip9): idempotency audit
// TODO(max): Re idempotency, we may want to first check if the outputs have
// actually been spent in some other way before trying to spend them.
pub async fn handle_spendable_outputs<CM, PS>(
    channel_manager: CM,
    persister: PS,
    fee_estimates: &FeeEstimates,
    keys_manager: &LexeKeysManager,
    test_event_tx: &TestEventSender,
    tx_broadcaster: &TxBroadcaster,
    wallet: &OnchainWallet,
    gdrive_persister_tx: Option<&mpsc::Sender<VfsFile>>,
    event_id: &EventId,
    outputs: Vec<SpendableOutputDescriptor>,
    channel_id: LxChannelId,
) -> Result<(), EventHandleError>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // TODO(phlip9): idempotency: do nothing if outputs already spent

    // The tx only includes a 'change' output, which is actually just a
    // new internal address fetched from our wallet.
    // TODO(max): Maybe we should add another output for privacy?
    let spendable_output_descriptors = &outputs.iter().collect::<Vec<_>>();
    let destination_outputs = Vec::new();
    // + We occasionally experience `SpendableOutputs` replaying unsuccessfully
    //   over and over until we manually resolve them.
    // + We don't want to accidentally reveal a new internal spk for each
    //   failing event replay, leading to many useless internal spks that we
    //   have to track forever (because they're revealed but unused).
    // + On success we immediately broadcast (and thus "use") this spk.
    // ==> Just get the next unused internal address
    let destination_change_script =
        wallet.get_internal_address().script_pubkey();
    let feerate_sat_per_1000_weight = fee_estimates
        .get_est_sat_per_1000_weight(ConfirmationTarget::NonAnchorChannelFee);
    let secp_ctx = SysRng::new().gen_secp256k1_ctx();

    // We set nLockTime to the current height to discourage fee sniping.
    let best_height = channel_manager.current_best_block().height;
    let maybe_locktime = absolute::LockTime::from_height(best_height)
        .inspect_err(|e| warn!(%best_height, "Invalid locktime height: {e:#}"))
        .ok();

    // Returns `None` if all outputs given to us were static outputs already
    // managed by BDK.
    let maybe_sweep_tx = keys_manager
        .spend_spendable_outputs(
            spendable_output_descriptors,
            destination_outputs,
            destination_change_script,
            feerate_sat_per_1000_weight,
            maybe_locktime,
            &secp_ctx,
        )
        .with_context(|| format!("{channel_id}"))
        .context("Error creating spending tx for spendable outputs")
        // Must replay because the outputs are lost if they are not spent.
        .map_err(EventHandleError::Replay)?;

    // Early return if there's nothing to broadcast; event successfully handled.
    let sweep_tx = match maybe_sweep_tx {
        Some(tx) => tx,
        None => {
            test_event_tx.send(TestEvent::SpendableOutputs);
            return Ok(());
        }
    };

    // - Early return with success if broadcast succeeds.
    // - Early return with replay if broadcast fails for a reason OTHER THAN a
    //   spent or missing input.
    // - Continue handling if broadcast fails due to spent or missing inputs.
    debug!("Broadcasting tx to spend spendable outputs");
    match tx_broadcaster.broadcast_transaction(sweep_tx.clone()).await {
        Ok(()) => {
            test_event_tx.send(TestEvent::SpendableOutputs);
            return Ok(());
        }

        Err(e) =>
            if !e.is_spent_or_missing_inputs() {
                return Err(e)
                    .with_context(|| format!("{channel_id}"))
                    .context("Error broadcasting spendable outputs tx")
                    .map_err(EventHandleError::Replay);
            } else {
                // Proceed with further handling below.
            },
    }

    // From here, we know we cannot broadcast this tx because one or more of its
    // inputs are spent or missing. We don't know whether `SpendableOutputs` has
    // been properly handled, but we also don't want to leave the event in our
    // event queue to be replayed over and over again, spamming Esplora with
    // unbroadcastable txs.
    //
    // Instead, we persist the `SpendableOutputs` event and unbroadcastable tx
    // to separate VFS namespaces (and GDrive, if enabled) to be handled
    // inspected and handled later.

    let short_event_id = event_id.short();

    // Persist the `SpendableOutputs` event to the VFS (and GDrive):
    // `unswept_outputs-events/<timestamp>-<nonce>`
    {
        let event = Event::SpendableOutputs {
            outputs,
            channel_id: Some(channel_id.into()),
        };

        let file_id =
            VfsFileId::new(vfs::UNSWEPT_OUTPUTS_EVENTS, short_event_id.clone());
        let file = persister.encrypt_ldk_writeable(file_id, &event);

        // Persist event to GDrive.
        if let Some(gdrive_tx) = gdrive_persister_tx {
            gdrive_tx
                .try_send(file.clone())
                .context("GDrive persister queue full")
                .map_err(EventHandleError::Replay)?;
        }

        // Persist event to VFS.
        let retries = 1;
        persister
            .persist_file(file, retries)
            .await
            .context(
                "Failed to persist unswept outputs event which failed \
                 to broadcast due to spent or missing tx inputs",
            )
            .map_err(EventHandleError::Replay)?;
    }

    let tx_display = TxDisplay(&sweep_tx);
    let tx_context = format!("channel_id={channel_id}, {tx_display}");

    // Persist the unbroadcastable tx to the VFS (and GDrive):
    // `unswept_outputs-txs/<timestamp>-<nonce>-<txid>`
    {
        let raw_tx = {
            let mut buf = Vec::new();
            sweep_tx
                .consensus_encode(&mut buf)
                .with_context(|| tx_context.clone())
                .context("Failed to encode bitcoin tx with bad inputs")
                .map_err(EventHandleError::Replay)?;
            buf
        };

        let txid = sweep_tx.compute_txid();
        let file_id = VfsFileId::new(
            vfs::UNSWEPT_OUTPUTS_TXS,
            format!("{short_event_id}-{txid}"),
        );

        let file = persister.encrypt_bytes(file_id, &raw_tx);

        // Backup unbroadcastable tx to GDrive.
        if let Some(gdrive_tx) = gdrive_persister_tx {
            gdrive_tx
                .try_send(file.clone())
                .context("GDrive persister queue full")
                .map_err(EventHandleError::Replay)?;
        }

        // Persist unbroadcastable tx to VFS.
        let retries = 1;
        persister
            .persist_file(file, retries)
            .await
            .with_context(|| tx_context.clone())
            .context("Failed to persist unbroadcastable tx")
            .map_err(EventHandleError::Replay)?;
    }

    // We successfully moved the unswept outputs event to a different 'queue'.
    // Still return an error, but let the original event be discarded.
    Err(EventHandleError::Discard(anyhow!(
        "An input to the spendable outputs sweep tx was missing or \
         spent. Event and tx persisted to VFS for later handling: \
         short_event_id={short_event_id}, {tx_context}"
    )))
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;
    use proptest::{prop_assert, proptest};

    use super::*;

    /// cargo test -p lexe-ln -- event_id_basic --show-output
    #[test]
    fn event_id_example() {
        let event_id1 =
            EventId::from_str("1733956429163-1598316375-ChannelReady").unwrap();

        let event_id2 = {
            let timestamp = TimestampMs::try_from(1733956429163i64).unwrap();
            let nonce = 1598316375;
            let name = "ChannelReady".to_owned();
            EventId {
                timestamp,
                nonce,
                name,
            }
        };

        assert_eq!(event_id1, event_id2);
    }

    #[test]
    fn event_id_roundtrips() {
        roundtrip::json_string_roundtrip_proptest::<EventId>();
        roundtrip::fromstr_display_roundtrip_proptest::<EventId>();
    }

    /// Proptest: For all valid [`EventId`]s, [`EventId::short`] is a prefix of
    /// [`EventId::to_string`].
    #[test]
    fn prop_event_id_short_is_prefix_of_event_id() {
        proptest!(|(event_id: EventId)| {
            let short = event_id.short();
            let full = event_id.to_string();
            prop_assert!(
                full.starts_with(&short),
                "short() = {short}, to_string() = {full}",
            );
        });
    }
}
