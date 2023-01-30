use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context};
use common::api::NodePk;
use common::backoff;
use common::ln::peer::ChannelPeer;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

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

pub async fn connect_channel_peer_if_necessary<CM, PM, PS>(
    peer_manager: PM,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
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

pub async fn do_connect_peer<CM, PM, PS>(
    peer_manager: PM,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
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

/// Spawns a task that regularly reconnects to the channel peers in this task's
/// `channel_peers` map, which is initialized with `initial_channel_peers`.
///
/// To reconnect to a node, include it in `initial_channel_peers` during startup
/// or send a [`ChannelPeerUpdate::Add`] anytime to have the task immediately
/// begin reconnect attempts to the given node.
///
/// If you do NOT wish to immediately reconnect to a given channel peer (e.g.
/// LSP should not reconnect to user nodes which are still offline), simply do
/// not send the [`ChannelPeerUpdate::Add`] until the peer (user node) is ready.
pub fn spawn_p2p_reconnector<CM, PM, PS>(
    channel_manager: CM,
    peer_manager: PM,
    initial_channel_peers: Vec<ChannelPeer>,
    mut channel_peer_rx: mpsc::Receiver<ChannelPeerUpdate>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    LxTask::spawn_named("p2p reconnectooor", async move {
        let mut interval = time::interval(P2P_RECONNECT_INTERVAL);

        // The current set of `ChannelPeer`s, indexed by their `NodePk`.
        let mut channel_peers = initial_channel_peers
            .into_iter()
            .map(|cp| (cp.node_pk, cp))
            .collect::<HashMap<NodePk, ChannelPeer>>();

        loop {
            // Retry reconnect when timer ticks or we get a channel peer update
            tokio::select! {
                _ = interval.tick() => (),
                Some(cp_update) = channel_peer_rx.recv() => {
                    // We received a ChannelPeerUpdate; update our HashMap of
                    // current channel peers accordingly.
                    match cp_update {
                        ChannelPeerUpdate::Add(cp) =>
                            channel_peers.insert(cp.node_pk, cp),
                        ChannelPeerUpdate::Remove(cp) =>
                            channel_peers.remove(&cp.node_pk),
                    };
                }
                () = shutdown.recv() => break,
            }

            // Generate futures to reconnect to all disconnected channel peers
            let connected_p2p_peers = peer_manager.get_peer_node_ids();
            let reconnect_futs = channel_manager
                // List our current channels
                .list_channels()
                .into_iter()
                // Get pubkeys of all channel counterparties
                .map(|channel| channel.counterparty.node_id)
                // Filter out channel counterparties we're already connected to
                .filter(|node_id| !connected_p2p_peers.contains(node_id))
                // secp256k1::PublicKey -> NodePk
                .map(NodePk)
                // NodePk -> ChannelPeer (i.e. associate NodePk with SocketAddr)
                .filter_map(|node_pk| channel_peers.get(&node_pk))
                // Produce a future that reconnects to this peer
                .map(|cp| do_connect_peer(peer_manager.clone(), cp.clone()))
                .collect::<Vec<_>>();

            // Do the reconnect(s), quit early if shutting down, log any errors
            let reconnect_results = tokio::select! {
                results = future::join_all(reconnect_futs) => results,
                () = shutdown.recv() => break,
            };
            for res in reconnect_results {
                if let Err(e) = res {
                    warn!("Couldn't reconnect to channel peer: {e:#}");
                }
            }
        }

        info!("LN P2P reconnectooor task complete");
    })
}

/// Given a [`TcpListener`], spawns a task to await on inbound connections,
/// handing off the resultant `TcpStream`s for the `PeerManager` to manage.
pub fn spawn_p2p_listener<CM, PM, PS>(
    listener: TcpListener,
    peer_manager: PM,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    LxTask::spawn_named("p2p listener", async move {
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
                _ = shutdown.recv() => break
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
