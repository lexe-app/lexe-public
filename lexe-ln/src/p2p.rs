use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use common::backoff;
use common::ln::peer::ChannelPeer;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use futures::future;
use lightning::ln::msgs::ChannelMessageHandler;
use tokio::net::TcpStream;
use tokio::time;
use tracing::{debug, error, warn};

use crate::alias::{LexeChannelManagerType, LexePeerManagerType};
use crate::traits::LexePersister;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const P2P_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);

pub async fn connect_channel_peer_if_necessary<CHANNELMANAGER>(
    peer_manager: Arc<LexePeerManagerType<CHANNELMANAGER>>,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CHANNELMANAGER: Deref + Send + Sync + 'static,
    CHANNELMANAGER::Target: ChannelMessageHandler + Send + Sync,
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

pub async fn do_connect_peer<CHANNELMANAGER>(
    peer_manager: Arc<LexePeerManagerType<CHANNELMANAGER>>,
    channel_peer: ChannelPeer,
) -> anyhow::Result<()>
where
    CHANNELMANAGER: Deref + Send + Sync + 'static,
    CHANNELMANAGER::Target: ChannelMessageHandler + Send + Sync,
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
pub fn spawn_p2p_reconnector<CHANNELMANAGER, PERSISTER>(
    channel_manager: Arc<LexeChannelManagerType<PERSISTER>>,
    peer_manager: Arc<LexePeerManagerType<CHANNELMANAGER>>,
    persister: PERSISTER,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    CHANNELMANAGER: Deref + Send + Sync + 'static,
    CHANNELMANAGER::Target: ChannelMessageHandler + Send + Sync,
    PERSISTER: Deref + Send + Sync + 'static,
    PERSISTER::Target: LexePersister + Send + Sync,
{
    LxTask::spawn(async move {
        let mut interval = time::interval(P2P_RECONNECT_INTERVAL);

        loop {
            tokio::select! {
                // Prevents race condition where we initiate a reconnect *after*
                // a shutdown signal was received, causing this task to hang
                biased;
                _ = shutdown.recv() => break,
                _ = interval.tick() => {}
            }

            // NOTE: Repeatedly hitting the DB here doesn't seem strictly
            // necessary (a channel for the channel manager to notify this task
            // of a new peer is sufficient), but it is the simplest solution for
            // now. This can be optimized away if it becomes a problem later.
            let channel_peers = match persister.read_channel_peers().await {
                Ok(cp_vec) => cp_vec,
                Err(e) => {
                    error!("Could not read channel peers: {e:#}");
                    continue;
                }
            };

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
