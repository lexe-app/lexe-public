use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::Confirm;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration};
use tracing::{debug, error, info};

use crate::alias::EsploraSyncClientType;
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChainMonitor, LexeChannelManager, LexePersister};

/// How often the ldk tx sync task re-syncs to the latest chain tip.
const LDK_TX_SYNC_INTERVAL: Duration = Duration::from_secs(60 * 10);

/// Spawns a task that periodically restarts LDK tx sync via the Esplora client.
pub fn spawn_ldk_tx_sync_task<CMAN, CMON, PS>(
    channel_manager: CMAN,
    chain_monitor: CMON,
    ldk_sync_client: Arc<EsploraSyncClientType>,
    initial_sync_tx: oneshot::Sender<anyhow::Result<()>>,
    mut resync_rx: mpsc::Receiver<()>,
    test_event_tx: TestEventSender,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CMAN: LexeChannelManager<PS>,
    CMON: LexeChainMonitor<PS>,
    PS: LexePersister,
{
    LxTask::spawn_named("ldk tx sync", async move {
        let mut sync_timer = time::interval(LDK_TX_SYNC_INTERVAL);
        let mut maybe_initial_sync_tx = Some(initial_sync_tx);

        loop {
            // A future which completes when *either* the timer ticks or we
            // receive a signal via resync_rx.
            let sync_trigger_fut = async {
                tokio::select! {
                    _ = sync_timer.tick() => (),
                    Some(()) = resync_rx.recv() => (),
                }
            };

            tokio::select! {
                () = sync_trigger_fut => {
                    let start = Instant::now();

                    let confirmables = vec![
                        channel_manager.deref() as &(dyn Confirm + Send + Sync),
                        chain_monitor.deref() as &(dyn Confirm + Send + Sync),
                    ];

                    // Give up if we receive shutdown signal during sync
                    let try_tx_sync = tokio::select! {
                        res = ldk_sync_client.sync(confirmables) => res,
                        () = shutdown.recv() => break,
                    };
                    let elapsed = start.elapsed().as_millis();

                    // Return and log the results of the first sync
                    if let Some(sync_tx) = maybe_initial_sync_tx.take() {
                        let initial_sync_result = try_tx_sync
                            .as_ref()
                            .map(|&()| ())
                            // Because TxSyncError doesn't impl Clone
                            .map_err(|e| anyhow!("{e:#}"));

                        if sync_tx.send(initial_sync_result).is_err() {
                            error!("Could not return result of initial sync");
                        }
                    }

                    match try_tx_sync {
                        Ok(()) => {
                            debug!("Tx sync completed <{elapsed}ms>");
                            test_event_tx.send(TestEvent::TxSyncComplete);
                        }
                        Err(e) => error!("Tx sync failed <{elapsed}ms>: {e:#}"),
                    }
                }
                () = shutdown.recv() => break,
            }
        }

        info!("LDK tx sync shutting down");
    })
}
