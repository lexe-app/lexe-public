use std::{collections::HashMap, time::Duration};

use anyhow::{bail, ensure, Context};
use common::{
    api::{Empty, NodePk},
    backoff,
    ln::{addr::LxSocketAddress, peer::ChannelPeer},
    shutdown::ShutdownChannel,
    task::LxTask,
    Apply,
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
/// generated and sent to the [p2p connector task] via an [`mpsc`] channel.
/// The [p2p connector task] uses this information to update its view of the
/// current set of [`ChannelPeer`]s, obviating the need to repeatedly read the
/// list of channel peers from the DB.
///
/// [p2p connector task]: spawn_p2p_connector
#[derive(Debug)]
pub enum ChannelPeerUpdate {
    /// We opened a channel and have a new channel peer.
    Add(ChannelPeer),
    /// We closed a channel and need to remove one of our channel peers.
    Remove(ChannelPeer),
}

/// Connects to a LN peer, returning early if we were already connected.
/// Cycles through the given addresses until we run out of connect attempts.
pub async fn connect_peer_if_necessary<CM, PM, PS>(
    peer_manager: PM,
    node_pk: &NodePk,
    addrs: &[LxSocketAddress],
) -> anyhow::Result<Empty>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    ensure!(!addrs.is_empty(), "No addrs were provided");

    // Early return if we're already connected
    if peer_manager.peer_by_node_id(&node_pk.0).is_some() {
        return Ok(Empty {});
    }

    // Cycle the given addresses in order
    let mut addrs = addrs.iter().cycle();

    // Retry at least a couple times to mitigate an outbound connect race
    // between the reconnector and open_channel which has been observed.
    let retries = 5;
    for _ in 0..retries {
        let addr = addrs.next().expect("Cycling through a non-empty slice");

        match do_connect_peer(peer_manager.clone(), node_pk, addr).await {
            Ok(()) => return Ok(Empty {}),
            Err(e) => warn!("Failed to connect to peer: {e:#}"),
        }

        // Connect failed; sleep 500ms before the next attempt to give LDK some
        // time to complete the noise / LN handshake. We do NOT need to add a
        // random jitter because LDK's PeerManager already tiebreaks outbound
        // connect races by failing the later attempt.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Right before the next attempt, check again whether we're connected in
        // case another task managed to connect while we were sleeping.
        if peer_manager.peer_by_node_id(&node_pk.0).is_some() {
            return Ok(Empty {});
        }
    }

    // Do the last attempt.
    let addr = addrs.next().expect("Cycling through a non-empty slice");
    do_connect_peer(peer_manager, node_pk, addr)
        .await
        .context("Failed to connect to peer")?;

    Ok(Empty {})
}

async fn do_connect_peer<CM, PM, PS>(
    peer_manager: PM,
    node_pk: &NodePk,
    addr: &LxSocketAddress,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    debug!(%node_pk, %addr, "Starting do_connect_peer");

    // TcpStream::connect takes a `String` in SGX.
    let addr_str = addr.to_string();
    let stream = TcpStream::connect(addr_str)
        .apply(|fut| time::timeout(CONNECT_TIMEOUT, fut))
        .await
        .context("Connect request timed out")?
        .context("TcpStream::connect() failed")?
        .into_std()
        .context("Couldn't convert to std TcpStream")?;

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
        node_pk.0,
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
        if peer_manager.peer_by_node_id(&node_pk.0).is_some() {
            // Connection confirmed, log and return Ok
            debug!(%node_pk, %addr, "Successfully connected to peer");
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
/// Upon shutdown, this task will also disconnect from all peers.
///
/// To reconnect to a node, include it in `initial_channel_peers` during startup
/// or send a [`ChannelPeerUpdate::Add`] anytime to have the task immediately
/// begin reconnect attempts to the given node.
///
/// If you do NOT wish to immediately reconnect to a given channel peer (e.g.
/// LSP should not reconnect to user nodes which are still offline), simply do
/// not send the [`ChannelPeerUpdate::Add`] until the peer (user node) is ready.
pub fn spawn_p2p_connector<CM, PM, PS>(
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
    const SPAN_NAME: &str = "(p2p-connector)";
    LxTask::spawn_named_with_span(
        SPAN_NAME,
        info_span!(SPAN_NAME),
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
                        info!("Received channel peer update: {cp_update:?}");
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
                for details in peer_manager.list_peers() {
                    let connected_peer_pk =
                        NodePk(details.counterparty_node_id);
                    disconnected_peers.remove(&connected_peer_pk);
                }
                let reconnect_futs = disconnected_peers
                    .into_values()
                    .map(|peer| {
                        let peer_manager_clone = peer_manager.clone();
                        let reconnect_fut = async move {
                            let res = do_connect_peer(
                                peer_manager_clone,
                                &peer.node_pk,
                                &peer.addr,
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

            info!("Received shutdown; disconnecting all peers");
            // This ensures we don't continue updating our channel data after
            // the background processor has stopped.
            peer_manager.disconnect_all_peers();

            info!("LN P2P connector task complete");
        },
    )
}
