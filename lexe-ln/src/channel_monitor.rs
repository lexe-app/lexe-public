use std::{
    collections::HashMap,
    fmt::{self, Display},
    mem,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context};
use common::ln::channel::LxOutPoint;
use futures::{stream::FuturesUnordered, StreamExt};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span};

use crate::{
    alias::LexeChainMonitorType,
    traits::{LexeChannelManager, LexeInnerPersister, LexePersister},
};

/// An actor which persists channel monitors. Channel monitors are persisted
/// serially per-channel, and concurrently across channels.
///
/// Updates to a single channel monitor are coalesced, meaning that if multiple
/// updates are queued for the same channel funding_txo, we only persist the
/// channel monitor once, though we still have to notify the chain monitor for
/// each update_id in the batch.
///
/// The primary source for updates is the
/// `Persist<SignerType>::update_persisted_channel` trait, which is impl'd by a
/// [`LexePersister`] implementor. We receive these updates via the
/// `channel_monitor_persister_rx` channel.
///
/// # Shutdown
///
/// The shutdown sequence for this task is special. LDK has noted that it may be
/// possible to generate monitor updates to be persisted after disconnecting
/// from a peer. However, we also disconnect from all peers in our peer
/// connector task in response to a shutdown signal, meaning that if the monitor
/// persister task is scheduled first and shuts down immediately, it won't be
/// around anymore when those monitor updates are queued. Thus, we trigger
/// `monitor_persister_shutdown` only *after* the BGP has completed its shutdown
/// sequence (during which it repersists the channel manager).
///
/// <https://discord.com/channels/915026692102316113/1367736643100086374/1367952226269663262>
///
/// Since the user node's GDrive persister task must live at least as long as
/// this task, we trigger it only once the monitor persister task has shut down.
///
/// To summarize, the *typical* (not always!) trigger order of shutdowns is:
///
/// 1) `shutdown`
/// 2) `monitor_persister_shutdown`
/// 3) `gdrive_persister_shutdown`
pub struct ChannelMonitorPersister<CM, PS>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    persister: PS,
    channel_manager: CM,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    rx_is_closed: bool,
    shutdown: NotifyOnce,
    monitor_persister_shutdown: NotifyOnce,
    gdrive_persister_shutdown: Option<NotifyOnce>,

    /// Used to receive a batch of `LxChannelMonitorUpdate` from
    /// `channel_monitor_persister_rx`.
    updates_buf: Vec<LxChannelMonitorUpdate>,

    /// Per-channel monitor persist state.
    chanmon_persist_states: HashMap<LxOutPoint, MonitorPersistState>,

    /// Active persist operations that are currently in-flight.
    active_persists: FuturesUnordered<LxTask<Result<LxOutPoint, FatalError>>>,

    /// The number of in-flight pending persists. This is
    /// `active_persists.len()` but doesn't require traversing the whole queue.
    num_active_persists: u32,

    /// The maximum number of concurrent channel monitor persists we allow at
    /// any given time.
    ///
    /// If this value is too large, we may overload the backend and persists
    /// will timeout, leading to immediate shutdown. If this value is too
    /// small, we may lose some perf.
    max_active_persists: u32,
}

/// Tracks the persist state for a specific channel monitor.
struct MonitorPersistState {
    /// Whether a persist operation is currently in-flight for this monitor.
    is_persisting: bool,
    /// If a persist is already in-flight but we get another update, we'll
    /// queue it here. Since we persist the full channel monitor each time,
    /// we can coalesce pending writes to the same channel monitor. We do still
    /// need to notify the chain monitor for each individual update id.
    queued_update_ids: Vec<u64>,
    /// The span of the latest queued update.
    span: tracing::Span,
}

/// A batch of channel monitor updates for a single channel. Since we persist
/// the entire channel monitor, we can persist once and notify the chain monitor
/// for all updates in the batch.
struct UpdateBatch {
    funding_txo: LxOutPoint,
    update_ids: Vec<u64>,
    span: tracing::Span,
}

/// Represents a channel monitor update requested by the `LexePersister`.
pub struct LxChannelMonitorUpdate {
    #[allow(dead_code)] // Conceptually part of the update.
    kind: ChannelMonitorUpdateKind,
    funding_txo: LxOutPoint,
    /// The ID of the channel monitor update, given by
    /// [`ChannelMonitorUpdate::update_id`] or
    /// [`ChannelMonitor::get_latest_update_id`].
    ///
    /// [`ChannelMonitorUpdate::update_id`]: lightning::chain::channelmonitor::ChannelMonitorUpdate::update_id
    /// [`ChannelMonitor::get_latest_update_id`]: lightning::chain::channelmonitor::ChannelMonitor::get_latest_update_id
    update_id: u64,
    span: tracing::Span,
}

/// Whether the [`LxChannelMonitorUpdate`] represents a new or updated channel.
#[derive(Copy, Clone, Debug)]
pub enum ChannelMonitorUpdateKind {
    New,
    Updated,
}

/// A fatal error that occurs during channel monitor persistence.
struct FatalError;

// --- impl ChannelMonitorPersister --- //

impl<CM, PS> ChannelMonitorPersister<CM, PS>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    pub fn new(
        persister: PS,
        channel_manager: CM,
        chain_monitor: Arc<LexeChainMonitorType<PS>>,
        channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
        shutdown: NotifyOnce,
        monitor_persister_shutdown: NotifyOnce,
        gdrive_persister_shutdown: Option<NotifyOnce>,
        max_active_persists: u32,
    ) -> Self {
        assert!(max_active_persists > 0);

        Self {
            persister,
            channel_manager,
            chain_monitor,
            channel_monitor_persister_rx,
            rx_is_closed: false,
            shutdown,
            monitor_persister_shutdown,
            gdrive_persister_shutdown,
            chanmon_persist_states: HashMap::new(),
            updates_buf: Vec::with_capacity(max_active_persists as usize),
            active_persists: FuturesUnordered::new(),
            num_active_persists: 0,
            max_active_persists,
        }
    }

    pub fn spawn(mut self) -> LxTask<()> {
        debug!("Starting channel monitor persister task");
        const SPAN_NAME: &str = "(chan-monitor-persister)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            self.run().await;
        })
    }

    async fn run(&mut self) {
        loop {
            let available_slots = self.available_slots();
            tokio::select! {
                num_updates = self.channel_monitor_persister_rx.recv_many(
                    &mut self.updates_buf,
                    available_slots,
                ), if self.rx_ready() => {
                    self.handle_updates(num_updates).await;
                }

                Some(res) = self.active_persists.next(),
                    if !self.active_persists.is_empty() =>
                {
                    let res = self.handle_persist_completion(res).await;
                    if let Err(FatalError) = res {
                        // Channel monitor persistence errors are fatal.
                        // Return immediately to prevent further monitor
                        // persists (which may skip the current monitor update
                        // if using incremental persist)
                        self.shutdown();
                        return;
                    }
                }

                () = self.monitor_persister_shutdown.recv() => {
                    debug!("channel monitor persister task shutting down");
                    break;
                }
            }
        }

        // Wait a short period for any outstanding channel monitor / channel
        // manager persists to finish up after `monitor_persister_shutdown`
        // triggers.
        self.shutdown_quiescence().await;
    }

    /// Returns the max number of updates we'll accept off
    /// `channel_monitor_persister_rx` in the next recv.
    #[inline]
    fn available_slots(&self) -> usize {
        (self.max_active_persists - self.num_active_persists) as usize
    }

    /// Returns `true` if we should poll `channel_monitor_persister_rx` for more
    /// updates.
    #[inline]
    fn rx_ready(&self) -> bool {
        !self.rx_is_closed && self.available_slots() > 0
    }

    /// Handle all channel monitor updates received on the channel. We batch
    /// and coalesce updates to the same `funding_txo` so that we only persist
    /// the channel monitor once per `funding_txo`, even if there are multiple
    /// updates for the same channel.
    async fn handle_updates(&mut self, num_updates: usize) {
        // recv_many returns 0 if the channel is closed and there are no more
        // updates queued => stop polling it
        if num_updates == 0 {
            self.rx_is_closed = true;
            return;
        }

        let mut updates_buf = mem::take(&mut self.updates_buf);

        let num_updates = updates_buf.len();
        let mut num_persists = 0;

        // Group and batch updates by funding_txo
        for batch in helpers::iter_update_batches(&mut updates_buf) {
            num_persists += 1;
            self.handle_update(batch);
        }

        debug!(
            "spawned channel monitor persists: \
             {num_persists}/{num_updates} (persists/updates)"
        );

        // Clear and reuse the allocation
        updates_buf.clear();
        self.updates_buf = updates_buf;
    }

    /// Handle a single channel monitor update batch -> persist channel monitor
    /// request.
    fn handle_update(&mut self, batch: UpdateBatch) {
        let state = self
            .chanmon_persist_states
            .entry(batch.funding_txo)
            .or_insert_with(|| MonitorPersistState {
                is_persisting: false,
                queued_update_ids: Vec::new(),
                span: batch.span.clone(),
            });

        if !state.is_persisting {
            // If there's no persist in-flight, start persisting immediately.
            state.is_persisting = true;
            self.spawn_persist(batch);
        } else {
            // If there's already a persist in-flight, we need to queue these
            // update ids.
            state.queued_update_ids.extend(batch.update_ids);
            state.span = batch.span;
        }
    }

    /// Spawn a task to persist a single channel monitor and notify the chain
    /// monitor.
    fn spawn_persist(&mut self, batch: UpdateBatch) {
        let task = LxTask::spawn_with_span(
            "chanmon-persist",
            batch.span.clone(),
            Self::persist_monitor_batch(
                self.persister.clone(),
                self.chain_monitor.clone(),
                batch,
            ),
        );
        self.active_persists.push(task);
        self.num_active_persists += 1;
    }

    /// Handle the completion of a channel monitor persist task.
    async fn handle_persist_completion(
        &mut self,
        result: Result<Result<LxOutPoint, FatalError>, tokio::task::JoinError>,
    ) -> Result<(), FatalError> {
        self.num_active_persists -= 1;

        let funding_txo = result.map_err(|_| FatalError).and_then(|r| r)?;

        // If there's another queued update for this channel, spawn another
        // persist task for it. Otherwise, reset `is_persisting` back to false.
        if let Some(state) = self.chanmon_persist_states.get_mut(&funding_txo) {
            if !state.queued_update_ids.is_empty() {
                let span = state.span.clone();
                let updates = mem::take(&mut state.queued_update_ids);
                let batch = UpdateBatch {
                    funding_txo,
                    update_ids: updates,
                    span,
                };
                // Keep is_persisting true since we're spawning another persist
                self.spawn_persist(batch);
            } else {
                state.is_persisting = false;
            }
        }
        Ok(())
    }

    /// After we've received a shutdown signal, ensure both the channel manager
    /// and channel monitors have reached a quiescent state. Wait for channel
    /// monitor updates or channel manager persists until neither do anything
    /// for a full 10ms. The 10ms delay allows other tasks which may trigger
    /// these persists to be scheduled.
    async fn shutdown_quiescence(&mut self) {
        const QUIESCENT_TIMEOUT: Duration = Duration::from_millis(10);

        loop {
            let available_slots = self.available_slots();
            tokio::select! {
                biased;

                () = self.channel_manager
                         .get_event_or_persistence_needed_future() =>
                {
                    if self.channel_manager.get_and_clear_needs_persistence() {
                        let try_persist = self.persister
                            .persist_manager(self.channel_manager.deref())
                            .await;
                        if let Err(e) = try_persist {
                            error!("(Quiescence) manager persist error: {e:#}");
                            // Nothing to do if persist fails, so just shutdown.
                            break;
                        }
                    }
                }

                num_updates = self.channel_monitor_persister_rx.recv_many(
                    &mut self.updates_buf,
                    available_slots,
                ), if self.rx_ready() => {
                    self.handle_updates(num_updates).await;
                }

                Some(res) = self.active_persists.next(),
                    if !self.active_persists.is_empty() =>
                {
                    let res = self.handle_persist_completion(res).await;
                    if let Err(FatalError) = res {
                        // Fatal error during persist - exit immediately
                        break;
                    }
                }

                _ = tokio::time::sleep(QUIESCENT_TIMEOUT) => {
                    if self.active_persists.is_empty() {
                        info!("chanmgr and monitors quiescent; shutting down.");
                        break;
                    }
                }
            };
        }

        self.shutdown();
    }

    fn shutdown(&self) {
        self.shutdown.send();

        // For user nodes, trigger the GDrive persister shutdown now that the
        // monitor persister is completely done.
        // TODO(phlip9): OwnedLxTask.into_shutdown().await when that exists
        if let Some(shutdown) = &self.gdrive_persister_shutdown {
            shutdown.send();
        }
    }

    /// Persist a single channel monitor update batch and notify the chain
    /// monitor.
    async fn persist_monitor_batch(
        persister: PS,
        chain_monitor: Arc<LexeChainMonitorType<PS>>,
        batch: UpdateBatch,
    ) -> Result<LxOutPoint, FatalError> {
        debug!("Handling channel monitor update");

        let result = Self::persist_monitor_batch_inner(
            &persister,
            &chain_monitor,
            &batch,
        )
        .await;

        match result {
            Ok(()) => {
                info!("Success: persisted monitor");
                Ok(batch.funding_txo)
            }
            Err(e) => {
                error!("Fatal: Monitor persist error: {e:#}");
                Err(FatalError)
            }
        }
    }

    async fn persist_monitor_batch_inner(
        persister: &PS,
        chain_monitor: &LexeChainMonitorType<PS>,
        batch: &UpdateBatch,
    ) -> anyhow::Result<()> {
        // Persist the entire channel monitor.
        persister
            .persist_channel_monitor(chain_monitor, &batch.funding_txo)
            .await
            .context("persist_channel_monitor failed")?;

        // Notify the chain monitor for _each monitor update id separately_.
        // - This should trigger a log like "Completed off-chain monitor update"
        // - NOTE: After this update, there may still be more updates to
        //   persist. The LDK log message will say "all off-chain updates
        //   complete" or "still have pending off-chain updates" (common during
        //   payments)
        // - NOTE: Only after *all* channel monitor updates are handled will the
        //   channel be reenabled and the BGP woken to process events via the
        //   chain monitor future.
        for update_id in &batch.update_ids {
            let funding_txo_ldk = OutPoint::from(batch.funding_txo);
            chain_monitor
                .channel_monitor_updated(funding_txo_ldk, *update_id)
                .map_err(|e| {
                    anyhow!("channel_monitor_updated returned Err: {e:?}")
                })?;
        }

        Ok(())
    }
}

// --- impl LxChannelMonitorUpdate --- //

impl LxChannelMonitorUpdate {
    pub fn new(
        kind: ChannelMonitorUpdateKind,
        funding_txo: LxOutPoint,
        update_id: u64,
    ) -> Self {
        let span =
            info_span!("(monitor-update)", %kind, %funding_txo, %update_id);

        Self {
            kind,
            funding_txo,
            update_id,
            span,
        }
    }

    /// The span for this update which includes the full monitor update context.
    ///
    /// Logs related to this monitor update should be logged inside this span,
    /// to ensure the log information is associated with this update.
    pub fn span(&self) -> tracing::Span {
        self.span.clone()
    }
}

// --- impl ChannelMonitorUpdateKind --- //

impl Display for ChannelMonitorUpdateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New => write!(f, "new"),
            Self::Updated => write!(f, "updated"),
        }
    }
}

// --- helpers --- //

mod helpers {
    use super::*;

    /// Return an iterator over batched updates grouped by funding_txo
    pub(super) fn iter_update_batches(
        updates: &mut [LxChannelMonitorUpdate],
    ) -> impl Iterator<Item = UpdateBatch> + '_ {
        // Group by funding_txo. Technically we don't need to sort the
        // update_ids, but it probably helps keep us on the happy path.
        updates.sort_unstable_by_key(|u| (u.funding_txo, u.update_id));
        updates
            .chunk_by(|a, b| a.funding_txo == b.funding_txo)
            .map(|chunk| {
                let last = chunk.last().expect("chunk is non-empty");
                let funding_txo = last.funding_txo;
                let span = last.span();
                let update_ids: Vec<u64> =
                    chunk.iter().map(|u| u.update_id).collect();

                UpdateBatch {
                    funding_txo,
                    update_ids,
                    span,
                }
            })
    }
}
