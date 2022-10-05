use std::sync::Arc;

use common::ln::channel::LxOutPoint;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::chainmonitor::MonitorUpdateId;
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::alias::LexeChainMonitorType;
use crate::traits::LexePersister;

pub struct LxChannelMonitorUpdate {
    pub funding_txo: LxOutPoint,
    pub update_id: MonitorUpdateId,
}

/// Spawns a task that that lets the persister make calls to the chain monitor.
/// For now, it simply listens on `channel_monitor_updated_rx` and calls
/// `ChainMonitor::channel_monitor_updated()` with any received values. This is
/// required because (a) the chain monitor cannot be initialized without the
/// persister, therefore (b) the persister cannot hold the chain monitor,
/// therefore there needs to be another means of letting the persister notify
/// the channel manager of events.
pub fn spawn_channel_monitor_updated_task<PS: LexePersister>(
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    mut channel_monitor_updated_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    debug!("Starting channel_monitor_updated task");
    LxTask::spawn(async move {
        loop {
            tokio::select! {
                Some(update) = channel_monitor_updated_rx.recv() => {
                    if let Err(e) = chain_monitor.channel_monitor_updated(
                        OutPoint::from(update.funding_txo),
                        update.update_id,
                    ) {
                        // ApiError impls Debug but not std::error::Error
                        error!("channel_monitor_updated returned Err: {:?}", e);
                    }
                }
                _ = shutdown.recv() => {
                    info!("channel_monitor_updated task shutting down");
                    break;
                }
            }
        }
    })
}
