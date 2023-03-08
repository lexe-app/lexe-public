use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

use anyhow::{bail, Context};
use common::api::NodePk;
use common::backoff;
use common::ln::peer::ChannelPeer;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt};
use lightning::ln::msgs::NetAddress;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, info_span, warn, Instrument};

use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The maximum amount of time we'll allow LDK to complete the P2P handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const P2P_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);

/// Every time a channel peer is added or removed, a [`ChannelPeerUpdate`] is
/// generated and sent to the [p2p reconnector task] via an [`mpsc`] channel.
/// The [p2p reconnector task] uses this information to update its view of the
/// current set of [`ChannelPeer`]s, obviating the need to repeatedly read the
/// list of channel peers from the DB.
///
/// [p2p reconnector task]: spawn_p2p_reconnector
#[derive(Debug)]
pub enum ChannelPeerUpdate {
    /// We opened a channel and have a new channel peer.
    Add(ChannelPeer),
    /// We closed a channel and need to remove one of our channel peers.
    Remove(ChannelPeer),
}

/// Converts an LDK [`NetAddress`] to a std [`SocketAddr`] if the [`NetAddress`]
/// is IPv4 or IPv6.
// TODO(max): This can probably be upstreamed as NetAddress::to_sock_addr
pub fn netaddr_to_sockaddr(net_addr: NetAddress) -> Option<SocketAddr> {
    match net_addr {
        NetAddress::IPv4 { addr, port } => {
            let ipv4 = Ipv4Addr::from(addr);
            let sockv4 = SocketAddrV4::new(ipv4, port);
            let sock_addr = SocketAddr::V4(sockv4);
            Some(sock_addr)
        }
        NetAddress::IPv6 { addr, port } => {
            let ipv6 = Ipv6Addr::from(addr);
            let flowinfo = 0;
            let scope_id = 0;
            let sockv6 = SocketAddrV6::new(ipv6, port, flowinfo, scope_id);
            let sock_addr = SocketAddr::V6(sockv6);
            Some(sock_addr)
        }
        _ => None,
    }
}

/// Ensures that we are connected to a given [`ChannelPeer`].
pub async fn connect_channel_peer_if_necessary<CM, PM, PS>(
    peer_manager: PM,
    channel_peer: ChannelPeer,
    test_event_tx: &TestEventSender,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    // Return immediately if we're already connected to the peer
    if peer_manager
        .get_peer_node_ids()
        .into_iter()
        .any(|(pk, _maybe_addr)| channel_peer.node_pk.0 == pk)
    {
        return Ok(());
    }

    // Otherwise, initiate the connection
    do_connect_peer(peer_manager, channel_peer, test_event_tx)
        .await
        .context("Failed to connect to peer")
}

pub async fn do_connect_peer<CM, PM, PS>(
    peer_manager: PM,
    channel_peer: ChannelPeer,
    test_event_tx: &TestEventSender,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    debug!("Connecting to channel peer {channel_peer}");
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
    let p2p_handshake_timeout = tokio::time::sleep(HANDSHAKE_TIMEOUT);
    loop {
        // Check if the connection has been closed.
        match futures::poll!(&mut connection_closed_fut) {
            std::task::Poll::Ready(_) => {
                bail!("Failed initial connection to peer - error unknown");
            }
            std::task::Poll::Pending => {}
        }

        // Check if the connection has been established.
        if peer_manager
            .get_peer_node_ids()
            .into_iter()
            .any(|(pk, _maybe_addr)| channel_peer.node_pk.0 == pk)
        {
            // Connection confirmed, log and return Ok
            debug!("Successfully connected to channel peer {channel_peer}");
            test_event_tx.send(TestEvent::ConnectionInitiated);
            return Ok(());
        }

        // Check if we've timed out waiting to complete the handshake.
        if p2p_handshake_timeout.is_elapsed() {
            bail!("Timed out waiting to complete the noise / P2P handshake");
        }

        // Connection not confirmed yet, wait before checking again
        tokio::time::sleep(backoff_durations.next().unwrap()).await;
    }
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
    peer_manager: PM,
    initial_channel_peers: Vec<ChannelPeer>,
    mut channel_peer_rx: mpsc::Receiver<ChannelPeerUpdate>,
    test_event_tx: TestEventSender,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    LxTask::spawn_named(
        "p2p reconnectooor",
        async move {
            let mut interval = time::interval(P2P_RECONNECT_INTERVAL);

            // The current set of `ChannelPeer`s, indexed by their `NodePk`.
            let mut channel_peers = initial_channel_peers
                .into_iter()
                .map(|cp| (cp.node_pk, cp))
                .collect::<HashMap<NodePk, ChannelPeer>>();

            loop {
                // Retry reconnect when timer ticks or we get an update
                tokio::select! {
                    _ = interval.tick() => (),
                    Some(cp_update) = channel_peer_rx.recv() => {
                        debug!("Received channel peer update: {cp_update:?}");
                        // We received a ChannelPeerUpdate; update our HashMap of
                        // current channel peers accordingly.
                        match cp_update {
                            ChannelPeerUpdate::Add(cp) =>
                                channel_peers.insert(cp.node_pk, cp),
                            ChannelPeerUpdate::Remove(cp) =>
                                channel_peers.remove(&cp.node_pk),
                        };
                        // TODO(max): We should also update the channel peers
                        // that are persisted, but only after differentiating
                        // between channel peer kinds (e.g. we persist external
                        // peers, but not lexe users or the LSP).
                    }
                    () = shutdown.recv() => break,
                }

                // Generate futures to reconnect to all disconnected peers.
                let mut disconnected_peers = channel_peers.clone();
                for (pk, _addr) in peer_manager.get_peer_node_ids() {
                    disconnected_peers.remove(&NodePk(pk));
                }
                let reconnect_futs = disconnected_peers
                    .into_values()
                    .map(|peer| {
                        let peer_manager_clone = peer_manager.clone();
                        let test_event_tx_clone = test_event_tx.clone();
                        let reconnect_fut = async move {
                            let res = do_connect_peer(
                                peer_manager_clone,
                                peer.clone(),
                                &test_event_tx_clone,
                            )
                            .await;
                            if let Err(e) = res {
                                warn!("Couldn't reconnect to {peer}: {e:#}");
                            }
                        };

                        reconnect_fut.in_current_span()
                    })
                    .collect::<Vec<_>>();

                // Do the reconnect(s), quit early if shutting down
                tokio::select! {
                    _ = future::join_all(reconnect_futs) => (),
                    () = shutdown.recv() => break,
                }
            }

            info!("LN P2P reconnectooor task complete");
        }
        .instrument(info_span!("(p2p-reconnector)")),
    )
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
                    let (tcp_stream, peer_addr) = match accept_res {
                        Ok(ts) => ts,
                        Err(e) => {
                            warn!("Failed to accept connection: {e:#}");
                            continue;
                        }
                    };
                    debug!("Accepted connection from {peer_addr}");
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
                _ = shutdown.recv() => break,
            }
        }

        // Wait on all child tasks to finish (i.e. all connections close).
        while let Some(join_res) = child_tasks.next().await {
            if let Err(e) = join_res {
                error!("P2P connection task panicked: {e:#}");
            }
        }

        info!("LN P2P listen task complete");
    }.instrument(info_span!("(p2p-listener)")))
}
