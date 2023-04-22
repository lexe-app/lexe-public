use std::{collections::HashMap, time::Duration};

use anyhow::{bail, Context};
use common::{
    api::NodePk, backoff, ln::peer::ChannelPeer, shutdown::ShutdownChannel,
    task::LxTask,
};
use futures::future;
use tokio::{net::TcpStream, sync::mpsc, time};
use tracing::{debug, info, info_span, warn, Instrument};

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

/// Shorthand to check whether our `PeerManager` registers that we're currently
/// connected to the given [`NodePk`], meaning that we have an active connection
/// and have finished exchanging noise / LN handshake messages. Note that this
/// function is not very efficient; it allocates a `Vec` of all our peers and
/// iterates over it in `O(n)` time.
// We have to take an owned LexePeerManager otherwise there are type issues...
pub fn is_connected<CM, PM, PS>(peer_manager: PM, node_pk: &NodePk) -> bool
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    peer_manager
        .get_peer_node_ids()
        .into_iter()
        .any(|(pk, _maybe_addr)| node_pk.0 == pk)
}

/// Connects to a [`ChannelPeer`], returning early if we were already connected.
pub async fn connect_channel_peer_if_necessary<CM, PM, PS>(
    peer_manager: PM,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    // Initial check to see if we are already connected.
    if is_connected(peer_manager.clone(), &channel_peer.node_pk) {
        return Ok(());
    }

    // We retry a few times to work around an outbound connect race between
    // the reconnector and open_channel which has occasionally been observed.
    let retries = 3;
    for _ in 0..retries {
        // Do the attempt.
        match do_connect_peer(peer_manager.clone(), channel_peer.clone()).await
        {
            Ok(()) => return Ok(()),
            Err(e) => warn!("Failed to connect to peer: {e:#}"),
        }

        // Connect failed; sleep 500ms before the next attempt to give LDK some
        // time to complete the noise / LN handshake. We do NOT need to add a
        // random jitter because LDK's PeerManager already tiebreaks outbound
        // connect races by failing the later attempt.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Right before the next attempt, do another is_connected check in case
        // another task managed to connect while we were sleeping.
        if is_connected(peer_manager.clone(), &channel_peer.node_pk) {
            return Ok(());
        }
    }

    // Do the last attempt.
    do_connect_peer(peer_manager, channel_peer)
        .await
        .context("Failed to connect to peer")
}

async fn do_connect_peer<CM, PM, PS>(
    peer_manager: PM,
    channel_peer: ChannelPeer,
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
    let mut backoff_durations = backoff::iter_with_initial_wait_ms(10);
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
        if is_connected(peer_manager.clone(), &channel_peer.node_pk) {
            // Connection confirmed, log and return Ok
            debug!("Successfully connected to channel peer {channel_peer}");
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
                        let reconnect_fut = async move {
                            let res = do_connect_peer(
                                peer_manager_clone,
                                peer.clone(),
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
