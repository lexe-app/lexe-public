use std::marker::Send;
use std::ops::Deref;
use std::sync::Arc;

use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use futures::stream::{FuturesUnordered, StreamExt};
use lightning::chain::chainmonitor::Persist;
use lightning::chain::transaction::OutPoint;
use lightning::ln::msgs::ChannelMessageHandler;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::alias::{LexeChainMonitorType, LexePeerManagerType, SignerType};
use crate::channel_monitor::LxChannelMonitorUpdate;

/// Spawns a task that that lets the persister make calls to the chain monitor.
/// For now, it simply listens on `channel_monitor_updated_rx` and calls
/// `ChainMonitor::channel_monitor_updated()` with any received values. This is
/// required because (a) the chain monitor cannot be initialized without the
/// persister, therefore (b) the persister cannot hold the chain monitor,
/// therefore there needs to be another means of letting the persister notify
/// the channel manager of events.
pub fn spawn_channel_monitor_updated_task<PERSISTER>(
    chain_monitor: Arc<LexeChainMonitorType<PERSISTER>>,
    mut channel_monitor_updated_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    PERSISTER: Deref + Send + Sync + 'static,
    PERSISTER::Target: Persist<SignerType> + Send,
{
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

/// Given a [`TcpListener`], spawns a task to await on inbound connections,
/// handing off the resultant `TcpStream`s for the `PeerManager` to manage.
pub fn spawn_p2p_listener<CHANNELMANAGER>(
    listener: TcpListener,
    peer_manager: Arc<LexePeerManagerType<CHANNELMANAGER>>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CHANNELMANAGER: Deref + 'static + Send + Sync,
    CHANNELMANAGER::Target: ChannelMessageHandler + Send + Sync,
{
    LxTask::spawn(async move {
        let mut child_tasks = FuturesUnordered::new();

        loop {
            tokio::select! {
                accept_res = listener.accept() => {
                    // TcpStream boilerplate
                    let (tcp_stream, _peer_addr) = match accept_res {
                        Ok(ts) => ts,
                        Err(e) => {
                            warn!("Failed to accept connection: {e:#}");
                            continue;
                        }
                    };
                    let tcp_stream = match tcp_stream.into_std() {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Couldn't convert to std TcpStream: {e:#}");
                            continue;
                        }
                    };

                    // Spawn a task to await on the connection
                    let peer_manager_clone = peer_manager.clone();
                    let child_task = LxTask::spawn(async move {
                        // `setup_inbound()` returns a future that completes
                        // when the connection is closed. The main thread calls
                        // peer_manager.disconnect_all_peers() once it receives
                        // a shutdown signal so there is no need to pass in a
                        // `shutdown`s here.
                        let connection_closed = lightning_net_tokio::setup_inbound(
                            peer_manager_clone,
                            tcp_stream,
                        );
                        connection_closed.await;
                    });

                    child_tasks.push(child_task);
                }
                // To prevent a memory leak of LxTasks, we select! on the
                // futures unordered so that we can clear out LxTasks for peers
                // that disconnect before the node shuts down.
                Some(join_res) = child_tasks.next() => {
                    if let Err(e) = join_res {
                        error!("P2P connection task panicked: {e:#}");
                    }
                }
                _ = shutdown.recv() =>
                    break info!("LN P2P listen task shutting down"),
            }
        }

        // Wait on all child tasks to finish (i.e. all connections close).
        while let Some(join_res) = child_tasks.next().await {
            if let Err(e) = join_res {
                error!("P2P connection task panicked: {:#}", e);
            }
        }

        info!("LN P2P listen task complete");
    })
}
