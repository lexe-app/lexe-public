use std::{sync::Arc, time::Instant};

use anyhow::{Context, anyhow};
use futures::{
    FutureExt,
    future::{BoxFuture, Either},
};
use lexe_std::backoff;
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
use lightning::chain::Confirm;
use tokio::{
    sync::{mpsc, oneshot},
    time::{self, Duration},
};
use tracing::{error, info, warn};

use crate::{
    alias::EsploraSyncClientType,
    esplora::LexeEsplora,
    traits::{LexeChainMonitor, LexeChannelManager, LexePersister},
    wallet::OnchainWallet,
};

/// How often the BDK / LDK sync tasks re-sync to the latest chain tip.
// This should be fairly infrequent because both sync using a transaction-based
// API which makes HTTP requests to third party services.
const SYNC_INTERVAL: Duration = Duration::from_secs(60 * 10);

/// The # of times a failed sync attempt is retried after the initial attempt.
//
// (2026-08-11): 0 retries -> 1 retry because `bitreq` only supports HTTP/1.1,
// which doesn't have a mechanism for the server to notify us when they close
// the connection. When a new usernode is scheduled on a long-lived but dormant
// meganode that has a cached Esplora connection, the first sync request
// errors, ultimately causing the entire node boot to fail. More context:
// https://github.com/rust-bitcoin/corepc/issues/587#issuecomment-5261195349
const SYNC_RETRIES: usize = 1;

// TODO(max): The control flow / logic in these two functions are sufficiently
// complex and similar that it's probably a good idea to extract a helper fn.

pub struct BdkSyncRequest {
    pub full_sync: bool,
    pub tx: oneshot::Sender<()>,
}

/// Spawns a task that periodically restarts BDK sync.
pub fn spawn_bdk_sync_task(
    esplora: Arc<LexeEsplora>,
    wallet: OnchainWallet,
    onchain_recv_tx: notify::Sender,
    first_bdk_sync_tx: oneshot::Sender<anyhow::Result<()>>,
    mut bdk_resync_rx: mpsc::Receiver<BdkSyncRequest>,
    mut shutdown: NotifyOnce,
    sync_timeout: Duration,
) -> LxTask<()> {
    LxTask::spawn("bdk sync", async move {
        let mut sync_timer = time::interval(SYNC_INTERVAL);
        let mut maybe_first_bdk_sync_tx = Some(first_bdk_sync_tx);
        // Holds the `oneshot::Sender`s which we'll notify when sync completes.
        let mut synced_txs: Vec<oneshot::Sender<()>> = Vec::new();

        loop {
            // A future which completes when *either* the timer ticks or we
            // receive a signal via bdk_resync_rx.
            let sync_trigger_fut = async {
                let mut is_full_sync = false;

                tokio::select! {
                    _ = sync_timer.tick() => (),
                    Some(req) = bdk_resync_rx.recv() => {
                        is_full_sync |= req.full_sync;
                        synced_txs.push(req.tx);
                    },
                }

                // We're about to sync; clear out any remaining txs
                while let Ok(req) = bdk_resync_rx.try_recv() {
                    is_full_sync |= req.full_sync;
                    synced_txs.push(req.tx);
                }

                is_full_sync
            };

            tokio::select! {
                is_full_sync = sync_trigger_fut => {
                    info!(is_full_sync, "Starting BDK sync");
                    let start = Instant::now();

                    let mut sync = || {
                        let sync_fut = if is_full_sync {
                            Either::Left(wallet.full_sync(&esplora))
                        } else {
                            Either::Right(wallet.sync(&esplora))
                        };
                        async move { sync_fut.await.context("BDK sync failed") }
                            .boxed()
                    };
                    let maybe_sync_res = sync_with_retries(
                        "BDK", sync_timeout, &mut shutdown, &mut sync,
                    ).await;
                    let sync_res = match maybe_sync_res {
                        Some(sync_res) => sync_res,
                        // Shutdown signal received.
                        None => break,
                    };
                    let elapsed_ms = start.elapsed().as_millis();

                    // Return and log the results of the first sync
                    if let Some(sync_tx) = maybe_first_bdk_sync_tx.take() {
                        // 'Clone' the sync result
                        let first_bdk_sync_res = sync_res
                            .as_ref()
                            .map(|_| ())
                            .map_err(|e| anyhow!("{e:#}"));

                        if sync_tx.send(first_bdk_sync_res).is_err() {
                            error!("Could not return result of first BDK sync");
                        }
                    }

                    match sync_res {
                        Ok(sync_stats) => {
                            let is_legacy = false;
                            sync_stats.log_sync_complete(is_legacy, elapsed_ms);
                            onchain_recv_tx.send();
                            for tx in synced_txs.drain(..) {
                                let _ = tx.send(());
                            }
                        }
                        Err(e) => error!("BDK sync error <{elapsed_ms}ms>: {e:#}"),
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
    mut ldk_resync_rx: mpsc::Receiver<oneshot::Sender<()>>,
    mut shutdown: NotifyOnce,
    sync_timeout: Duration,
) -> LxTask<()>
where
    CMAN: LexeChannelManager<PS>,
    CMON: LexeChainMonitor<PS>,
    PS: LexePersister,
{
    LxTask::spawn("ldk sync", async move {
        let mut sync_timer = time::interval(SYNC_INTERVAL);
        let mut maybe_first_ldk_sync_tx = Some(first_ldk_sync_tx);
        // Holds the `oneshot::Sender`s which we'll notify when sync completes.
        let mut synced_txs: Vec<oneshot::Sender<()>> = Vec::new();

        loop {
            // A future which completes when *either* the timer ticks or we
            // receive a signal via ldk_resync_rx.
            let sync_trigger_fut = async {
                tokio::select! {
                    _ = sync_timer.tick() => (),
                    Some(tx) = ldk_resync_rx.recv() => synced_txs.push(tx),
                }

                // We're about to sync; clear out any remaining txs
                while let Ok(tx) = ldk_resync_rx.try_recv() {
                    synced_txs.push(tx);
                }
            };

            tokio::select! {
                () = sync_trigger_fut => {
                    info!("Starting LDK sync");
                    let start = Instant::now();

                    let mut sync = || {
                        let confirmables = vec![
                            channel_manager.deref()
                                as &(dyn Confirm + Send + Sync),
                            chain_monitor.deref()
                                as &(dyn Confirm + Send + Sync),
                        ];
                        let sync_fut = ldk_sync_client.sync(confirmables);
                        async move { sync_fut.await.context("LDK sync failed") }
                            .boxed()
                    };
                    let maybe_sync_res = sync_with_retries(
                        "LDK", sync_timeout, &mut shutdown, &mut sync,
                    ).await;
                    let sync_res = match maybe_sync_res {
                        Some(sync_res) => sync_res,
                        // Shutdown signal received.
                        None => break,
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
                            for tx in synced_txs.drain(..) {
                                let _ = tx.send(());
                            }
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

/// Runs `sync` until success or `timeout` elapses, with up to
/// [`SYNC_RETRIES`] retries.
///
/// - Returns `None` iff a shutdown signal is received.
/// - The timeout bounds the total time across all attempts.
async fn sync_with_retries<'a, T: 'a>(
    kind: &str,
    timeout: Duration,
    shutdown: &mut NotifyOnce,
    sync: &mut (dyn FnMut() -> BoxFuture<'a, anyhow::Result<T>> + Send),
) -> Option<anyhow::Result<T>> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut backoff_durations = backoff::get_backoff_iter();
    let mut attempts = 0;

    loop {
        attempts += 1;
        let sync_result = tokio::select! {
            result = sync() => result,
            () = tokio::time::sleep_until(deadline) => {
                let timeout_secs = timeout.as_secs();
                return Some(Err(anyhow!(
                    "{kind} sync timed out after {timeout_secs}s \
                     (attempt #{attempts})"
                )));
            }
            () = shutdown.recv() => return None,
        };

        let error = match sync_result {
            Ok(value) => return Some(Ok(value)),
            Err(e) => e,
        };

        // Give up if we're out of retries or if the deadline would pass before
        // the next attempt starts.
        let backoff_duration = backoff_durations.next_delay();
        if attempts > SYNC_RETRIES
            || tokio::time::Instant::now() + backoff_duration >= deadline
        {
            return Some(Err(error));
        }

        warn!("{kind} sync attempt #{attempts} failed; retrying: {error:#}");
        tokio::select! {
            () = tokio::time::sleep(backoff_duration) => (),
            () = shutdown.recv() => return None,
        }
    }
}
