use std::{sync::Arc, time::Instant};

use anyhow::{anyhow, Context};
use common::{notify, shutdown::ShutdownChannel, task::LxTask};
use lightning::chain::Confirm;
use tokio::{
    sync::{broadcast, oneshot},
    time::{self, Duration},
};
use tracing::{error, info};

use crate::{
    alias::EsploraSyncClientType,
    test_event::{TestEvent, TestEventSender},
    traits::{LexeChainMonitor, LexeChannelManager, LexePersister},
    wallet::LexeWallet,
};

/// How often the BDK / LDK sync tasks re-sync to the latest chain tip.
// This should be fairly infrequent because both sync using a transaction-based
// API which makes HTTP requests to third party services.
const SYNC_INTERVAL: Duration = Duration::from_secs(60 * 10);
/// How long BDK / LDK sync can proceed before we consider sync to have failed.
const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawns a task that periodically restarts BDK sync.
pub fn spawn_bdk_sync_task(
    wallet: LexeWallet,
    onchain_recv_tx: notify::Sender,
    first_bdk_sync_tx: oneshot::Sender<anyhow::Result<()>>,
    mut bdk_resync_rx: broadcast::Receiver<()>,
    test_event_tx: TestEventSender,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn_named("bdk sync", async move {
        let mut sync_timer = time::interval(SYNC_INTERVAL);
        let mut maybe_first_bdk_sync_tx = Some(first_bdk_sync_tx);

        loop {
            // A future which completes when *either* the timer ticks or we
            // receive a signal via bdk_resync_rx.
            let sync_trigger_fut = async {
                tokio::select! {
                    _ = sync_timer.tick() => (),
                    Ok(()) = bdk_resync_rx.recv() => (),
                }
            };

            tokio::select! {
                () = sync_trigger_fut => {
                    info!("Starting BDK sync");
                    let start = Instant::now();

                    // Give up if we time out or receive a shutdown signal
                    let timeout = time::sleep(SYNC_TIMEOUT);
                    let sync_res = tokio::select! {
                        res = wallet.sync() => res.context("BDK sync failed"),
                        _ = timeout => Err(anyhow!("BDK sync timed out")),
                        () = shutdown.recv() => break,
                    };
                    let elapsed = start.elapsed().as_millis();

                    // Return and log the results of the first sync
                    if let Some(sync_tx) = maybe_first_bdk_sync_tx.take() {
                        // 'Clone' the sync result
                        let first_bdk_sync_res = sync_res
                            .as_ref()
                            .map(|&()| ())
                            .map_err(|e| anyhow!("{e:#}"));

                        if sync_tx.send(first_bdk_sync_res).is_err() {
                            error!("Could not return result of first BDK sync");
                        }
                    }

                    match sync_res {
                        Ok(()) => {
                            info!("BDK sync completed <{elapsed}ms>");
                            onchain_recv_tx.send();
                            test_event_tx.send(TestEvent::BdkSyncComplete);
                        }
                        Err(e) => error!("BDK sync error <{elapsed}ms>: {e:#}"),
                    }
                }
                () = shutdown.recv() => break,
            }
        }

        info!("BDK sync shutting down");
    })
}

/// Spawns a task that periodically restarts LDK sync via the Esplora client.
pub fn spawn_ldk_sync_task<CMAN, CMON, PS>(
    channel_manager: CMAN,
    chain_monitor: CMON,
    ldk_sync_client: Arc<EsploraSyncClientType>,
    first_ldk_sync_tx: oneshot::Sender<anyhow::Result<()>>,
    mut ldk_resync_rx: broadcast::Receiver<()>,
    test_event_tx: TestEventSender,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CMAN: LexeChannelManager<PS>,
    CMON: LexeChainMonitor<PS>,
    PS: LexePersister,
{
    LxTask::spawn_named("ldk sync", async move {
        let mut sync_timer = time::interval(SYNC_INTERVAL);
        let mut maybe_first_ldk_sync_tx = Some(first_ldk_sync_tx);

        loop {
            // A future which completes when *either* the timer ticks or we
            // receive a signal via ldk_resync_rx.
            let sync_trigger_fut = async {
                tokio::select! {
                    _ = sync_timer.tick() => (),
                    Ok(()) = ldk_resync_rx.recv() => (),
                }
            };

            tokio::select! {
                () = sync_trigger_fut => {
                    info!("Starting LDK sync");
                    let start = Instant::now();

                    let confirmables = vec![
                        channel_manager.deref() as &(dyn Confirm + Send + Sync),
                        chain_monitor.deref() as &(dyn Confirm + Send + Sync),
                    ];

                    // Give up if we time out or receive a shutdown signal
                    let timeout = time::sleep(SYNC_TIMEOUT);
                    let sync_res = tokio::select! {
                        res = ldk_sync_client.sync(confirmables) =>
                            res.context("LDK sync failed"),
                        _ = timeout => Err(anyhow!("LDK sync timed out")),
                        () = shutdown.recv() => break,
                    };
                    let elapsed = start.elapsed().as_millis();

                    // Return and log the results of the first sync
                    if let Some(sync_tx) = maybe_first_ldk_sync_tx.take() {
                        // 'Clone' the sync result
                        let first_ldk_sync_res = sync_res
                            .as_ref()
                            .map(|&()| ())
                            .map_err(|e| anyhow!("{e:#}"));

                        if sync_tx.send(first_ldk_sync_res).is_err() {
                            error!("Could not return result of first LDK sync");
                        }
                    }

                    match sync_res {
                        Ok(()) => {
                            info!("LDK sync completed <{elapsed}ms>");
                            test_event_tx.send(TestEvent::LdkSyncComplete);
                        }
                        Err(e) => error!("LDK sync error <{elapsed}ms>: {e:#}"),
                    }
                }
                () = shutdown.recv() => break,
            }
        }

        info!("LDK sync shutting down");
    })
}
