use std::collections::HashSet;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use common::backoff;
use common::ln::peer::ChannelPeer;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt};
use lightning::ln::msgs::ChannelMessageHandler;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::alias::{LexeChannelManagerType, LexePeerManagerType};
use crate::traits::LexePersister;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const P2P_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);

/// Every time a channel peer is added or removed, a [`ChannelPeerUpdate`] is
/// generated and sent to the [p2p reconnector task] via an [`mpsc`] channel.
/// The [p2p reconnector task] uses this information to update its view of the
/// current set of [`ChannelPeer`]s, obviating the need to repeatedly read the
/// list of channel peers from the DB.
///
/// [p2p reconnector task]: spawn_p2p_reconnector
pub enum ChannelPeerUpdate {
    /// We opened a channel and have a new channel peer.
    Add(ChannelPeer),
    /// We closed a channel and need to remove one of our channel peers.
    Remove(ChannelPeer),
}

pub async fn connect_channel_peer_if_necessary<CHANNEL_MANAGER>(
    peer_manager: Arc<LexePeerManagerType<CHANNEL_MANAGER>>,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CHANNEL_MANAGER: Deref + Send + Sync + 'static,
    CHANNEL_MANAGER::Target: ChannelMessageHandler + Send + Sync,
{
    debug!("Connecting to channel peer {channel_peer}");

    // Return immediately if we're already connected to the peer
    if peer_manager
        .get_peer_node_ids()
        .contains(&channel_peer.node_pk.0)
    {
        debug!("OK: Already connected to channel peer {channel_peer}");
        return Ok(());
    }

    // Otherwise, initiate the connection
    do_connect_peer(peer_manager, channel_peer)
        .await
        .context("Failed to connect to peer")
}

pub async fn do_connect_peer<CHANNEL_MANAGER>(
    peer_manager: Arc<LexePeerManagerType<CHANNEL_MANAGER>>,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CHANNEL_MANAGER: Deref + Send + Sync + 'static,
    CHANNEL_MANAGER::Target: ChannelMessageHandler + Send + Sync,
{
    let stream =
        time::timeout(CONNECT_TIMEOUT, TcpStream::connect(channel_peer.addr))
            .await
            .context("Connect request timed out")?
            .context("TcpStream::connect() failed")?
            .into_std()
            .context("Could not convert tokio TcpStream to std TcpStream")?;

    // NOTE: `setup_outbound()` returns a future which completes when the
    // connection closes, which we do not need to poll because a task was
    // spawned for it. However, in the case of an error, the future returned
    // by `setup_outbound()` completes immediately, and does not propagate
    // the error from `peer_manager.new_outbound_connection()`. So, in order
    // to check that there was no error while establishing the connection we
    // have to manually poll the future, and if it completed, return an
    // error (which we don't have access to because `lightning-net-tokio`
    // failed to surface it to us).
    //
    // On the other hand, since LDK's API doesn't let you know when the
    // connection is established, you have to keep calling
    // `peer_manager.get_peer_node_ids()` to see if the connection has been
    // registered yet.
    //
    // TODO: Rewrite / replace lightning-net-tokio entirely
    let connection_closed_fut = lightning_net_tokio::setup_outbound(
        peer_manager.clone(),
        channel_peer.node_pk.0,
        stream,
    );
    let mut connection_closed_fut = Box::pin(connection_closed_fut);
    // Use exponential backoff when polling so that a stalled connection
    // doesn't keep the node always in memory
    let mut backoff_durations = backoff::get_backoff_iter();
    loop {
        // Check if the connection has been closed
        match futures::poll!(&mut connection_closed_fut) {
            std::task::Poll::Ready(_) => {
                bail!("Failed initial connection to peer - error unknown");
            }
            std::task::Poll::Pending => {}
        }

        // Check if the connection has been established
        if peer_manager
            .get_peer_node_ids()
            .iter()
            .any(|pk| *pk == channel_peer.node_pk.0)
        {
            // Connection confirmed, break and return Ok
            break;
        } else {
            // Connection not confirmed yet, wait before checking again
            tokio::time::sleep(backoff_durations.next().unwrap()).await;
        }
    }

    debug!("Success: Connected to channel peer {channel_peer}");
    Ok(())
}

/// Spawns a task that regularly reconnects to the channel peers stored in DB.
pub fn spawn_p2p_reconnector<CHANNEL_MANAGER, PERSISTER>(
    channel_manager: Arc<LexeChannelManagerType<PERSISTER>>,
    peer_manager: Arc<LexePeerManagerType<CHANNEL_MANAGER>>,
    initial_channel_peers: Vec<ChannelPeer>,
    mut channel_peer_rx: mpsc::Receiver<ChannelPeerUpdate>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CHANNEL_MANAGER: Deref + Send + Sync + 'static,
    CHANNEL_MANAGER::Target: ChannelMessageHandler + Send + Sync,
    PERSISTER: Deref + Send + Sync + 'static,
    PERSISTER::Target: LexePersister + Send + Sync,
{
    LxTask::spawn(async move {
        let mut interval = time::interval(P2P_RECONNECT_INTERVAL);

        let mut channel_peers = initial_channel_peers
            .into_iter()
            .collect::<HashSet<ChannelPeer>>();

        loop {
            tokio::select! {
                // Prevents race condition where we initiate a reconnect *after*
                // a shutdown signal was received, causing this task to hang
                biased;
                _ = shutdown.recv() => break,
                Some(cp_update) = channel_peer_rx.recv() => {
                    // We received a ChannelPeerUpdate; update our HashSet of
                    // current channel peers accordingly.
                    match cp_update {
                        ChannelPeerUpdate::Add(cp) => channel_peers.insert(cp),
                        ChannelPeerUpdate::Remove(cp) => channel_peers.remove(&cp),
                    };
                }
                _ = interval.tick() => {}
            }

            // Find all the peers we've been disconnected from
            let p2p_peers = peer_manager.get_peer_node_ids();
            let disconnected_peers: Vec<_> = channel_manager
                .list_channels()
                .iter()
                .map(|chan| chan.counterparty.node_id)
                .filter(|node_id| !p2p_peers.contains(node_id))
                .collect();

            // Match ids
            let mut connect_futs: Vec<_> =
                Vec::with_capacity(disconnected_peers.len());
            for node_id in disconnected_peers {
                for channel_peer in channel_peers.iter() {
                    if channel_peer.node_pk.0 == node_id {
                        let connect_fut = self::do_connect_peer(
                            peer_manager.clone(),
                            channel_peer.deref().clone(),
                        );
                        connect_futs.push(connect_fut)
                    }
                }
            }

            // Reconnect
            for res in future::join_all(connect_futs).await {
                if let Err(e) = res {
                    warn!("Couldn't neconnect to channel peer: {:#}", e)
                }
            }
        }
    })
}

/// Given a [`TcpListener`], spawns a task to await on inbound connections,
/// handing off the resultant `TcpStream`s for the `PeerManager` to manage.
pub fn spawn_p2p_listener<CHANNEL_MANAGER>(
    listener: TcpListener,
    peer_manager: Arc<LexePeerManagerType<CHANNEL_MANAGER>>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CHANNEL_MANAGER: Deref + 'static + Send + Sync,
    CHANNEL_MANAGER::Target: ChannelMessageHandler + Send + Sync,
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
