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
/// The primary source for updates is the
/// `Persist<SignerType>::update_persisted_channel` trait, which is impl'd by a
/// [`LexePersister`] implementor.
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
    shutdown: NotifyOnce,
    monitor_persister_shutdown: NotifyOnce,
    gdrive_persister_shutdown: Option<NotifyOnce>,

    /// Per-channel monitor state
    chanmon_states: HashMap<LxOutPoint, State>,

    /// Active persist operations
    pending_persists: FuturesUnordered<LxTask<Result<LxOutPoint, ()>>>,

    /// The number of in-flight pending persists.
    num_pending_persists: usize,

    /// The maximum number of concurrent channel monitor persists we allow at
    /// any given time.
    ///
    /// If this value is too large, we may overload the backend and persists
    /// will timeout, leading to immediate shutdown. If this value is too
    /// small, we may lose some perf.
    max_pending_persists: usize,
}

/// Tracks the persist state for a specific channel monitor.
struct State {
    /// If a persist is already in-flight but we get another update, we'll
    /// queue it here. Since we persist the full channel monitor each time,
    /// we can coalesce pending writes to the same channel monitor, i.e., we
    /// only need to track the latest pending update id.
    pending_updates: Vec<u64>,
    /// The span of the latest pending update.
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
        max_pending_persists: usize,
    ) -> Self {
        Self {
            persister,
            channel_manager,
            chain_monitor,
            channel_monitor_persister_rx,
            shutdown,
            monitor_persister_shutdown,
            gdrive_persister_shutdown,
            chanmon_states: HashMap::new(),
            pending_persists: FuturesUnordered::new(),
            num_pending_persists: 0,
            max_pending_persists,
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
            tokio::select! {
                Some(update) = self.channel_monitor_persister_rx.recv(),
                    if self.num_pending_persists < self.max_pending_persists =>
                {
                    self.handle_update(update).await;
                }
                Some(result) = self.pending_persists.next(),
                    if !self.pending_persists.is_empty() =>
                {
                    if let Err(()) = self.handle_persist_completion(result).await {
                        // Channel monitor persistence errors are fatal.
                        // Return immediately to prevent further monitor
                        // persists (which may skip the current monitor update
                        // if using incremental persist)
                        return;
                    }
                }
                () = self.monitor_persister_shutdown.recv() => {
                    debug!("channel monitor persister task beginning shutdown");
                    break;
                }
            }
        }

        // Wait a short period for any outstanding channel monitor / channel
        // manager persists to finish up after `monitor_persister_shutdown`
        // triggers.
        self.shutdown_quiescence().await;
    }

    /// Handle a new channel monitor update -> persist channel monitor request.
    async fn handle_update(&mut self, update: LxChannelMonitorUpdate) {
        let funding_txo = update.funding_txo;
        let update_id = update.update_id;
        let span = update.span();

        let state =
            self.chanmon_states
                .entry(funding_txo)
                .or_insert_with(|| State {
                    pending_updates: Vec::new(),
                    span: span.clone(),
                });

        if state.pending_updates.is_empty() {
            // If there's no pending updates, start persisting immediately.
            self.spawn_persist(funding_txo, vec![update_id], span);
        } else {
            // If there's already a persist in-flight, we need to queue this
            // update id.
            state.pending_updates.push(update_id);
            state.span = span;
        }
    }

    /// Spawn a task to persist a single channel monitor and notify the chain
    /// monitor.
    fn spawn_persist(
        &mut self,
        funding_txo: LxOutPoint,
        updates: Vec<u64>,
        span: tracing::Span,
    ) {
        let task = LxTask::spawn_with_span(
            "chanmon-persist",
            span,
            Self::persist_channel_monitor(
                self.persister.clone(),
                self.chain_monitor.clone(),
                funding_txo,
                updates,
            ),
        );
        self.pending_persists.push(task);
        self.num_pending_persists += 1;
    }

    /// Handle the completion of a channel monitor persist task.
    async fn handle_persist_completion(
        &mut self,
        result: Result<Result<LxOutPoint, ()>, tokio::task::JoinError>,
    ) -> Result<(), ()> {
        self.num_pending_persists -= 1;

        // Flatten result
        let result = result.map_err(|_| ()).and_then(|r| r);

        match result {
            Ok(funding_txo) => {
                // Check if there's another queued update for this channel
                if let Some(state) = self.chanmon_states.get_mut(&funding_txo) {
                    if !state.pending_updates.is_empty() {
                        let span = state.span.clone();
                        let updates = mem::take(&mut state.pending_updates);
                        self.spawn_persist(funding_txo, updates, span);
                    }
                }
                Ok(())
            }
            Err(_) => {
                // Persist failed - trigger shutdown
                self.shutdown.send();
                if let Some(shutdown) = &self.gdrive_persister_shutdown {
                    shutdown.send();
                }
                Err(())
            }
        }
    }

    /// After we've received a shutdown signal, ensure both the channel manager
    /// and channel monitors have reached a quiescent state. Wait for channel
    /// monitor updates or channel manager persists until neither do anything
    /// for a full 10ms. The 10ms delay allows other tasks which may trigger
    /// these persists to be scheduled.
    async fn shutdown_quiescence(&mut self) {
        const QUIESCENT_TIMEOUT: Duration = Duration::from_millis(10);

        loop {
            tokio::select! {
                biased;
                // Channel manager persist
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
                Some(update) = self.channel_monitor_persister_rx.recv(),
                    if self.num_pending_persists < self.max_pending_persists =>
                {
                    self.handle_update(update).await;
                }
                Some(result) = self.pending_persists.next(),
                    if !self.pending_persists.is_empty() =>
                {
                    if let Err(()) = self.handle_persist_completion(result).await {
                        // Fatal error during persist - exit immediately
                        return;
                    }
                }
                _ = tokio::time::sleep(QUIESCENT_TIMEOUT) => {
                    if self.pending_persists.is_empty() {
                        info!("Channel mgr and monitors quiescent; shutting down.");
                        break;
                    }
                }
            };
        }

        // For user nodes, trigger the GDrive persister shutdown now that the
        // monitor persister is completely done.
        if let Some(shutdown) = &self.gdrive_persister_shutdown {
            shutdown.send();
        }
    }

    /// Persist a single channel monitor and notify the chain monitor.
    async fn persist_channel_monitor(
        persister: PS,
        chain_monitor: Arc<LexeChainMonitorType<PS>>,
        funding_txo: LxOutPoint,
        updates: Vec<u64>,
    ) -> Result<LxOutPoint, ()> {
        debug!("Handling channel monitor update");

        let result = Self::persist_channel_monitor_inner(
            &persister,
            &chain_monitor,
            funding_txo,
            updates,
        )
        .await;

        match result {
            Ok(()) => {
                info!("Success: persisted monitor");
                Ok(funding_txo)
            }
            Err(e) => {
                error!("Fatal: Monitor persist error: {e:#}");
                Err(())
            }
        }
    }

    async fn persist_channel_monitor_inner(
        persister: &PS,
        chain_monitor: &LexeChainMonitorType<PS>,
        funding_txo: LxOutPoint,
        updates: Vec<u64>,
    ) -> anyhow::Result<()> {
        // Persist the entire channel monitor.
        persister
            .persist_channel_monitor(chain_monitor, &funding_txo)
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
        for update_id in &updates {
            let funding_txo_ldk = OutPoint::from(funding_txo);
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
