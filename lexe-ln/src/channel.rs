use std::ops::Deref;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use common::ln::peer::ChannelPeer;
use lightning::ln::msgs::ChannelMessageHandler;
use lightning::util::config::UserConfig;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::alias::{LexeChannelManagerType, LexePeerManagerType};
use crate::p2p::{self, ChannelPeerUpdate};
use crate::traits::LexePersister;

/// Handles the full logic of opening a channel, including connecting to the
/// peer, creating the channel, and persisting the newly created channel.
pub async fn open_channel<CHANNEL_MANAGER, PERSISTER>(
    channel_manager: &LexeChannelManagerType<PERSISTER>,
    peer_manager: Arc<LexePeerManagerType<CHANNEL_MANAGER>>,
    persister: PERSISTER,
    channel_peer: ChannelPeer,
    channel_value_sat: u64,
    channel_peer_tx: &mpsc::Sender<ChannelPeerUpdate>,
    user_config: UserConfig,
) -> anyhow::Result<()>
where
    CHANNEL_MANAGER: Deref + Send + Sync + 'static,
    CHANNEL_MANAGER::Target: ChannelMessageHandler + Send + Sync,
    PERSISTER: Deref,
    PERSISTER::Target: LexePersister,
{
    info!("Opening channel with {}", channel_peer);

    // Make sure that we're connected to the channel peer
    p2p::connect_channel_peer_if_necessary(
        peer_manager.clone(),
        channel_peer.clone(),
    )
    .await
    .context("Failed to connect to peer")?;

    // Create the channel
    let user_channel_id = 1; // Not important, just use a default value
    let push_msat = 0; // No need for this yet
    channel_manager
        .create_channel(
            channel_peer.node_pk.0,
            channel_value_sat,
            push_msat,
            user_channel_id,
            Some(user_config),
        )
        // LDK's APIError impls Debug but not Error
        .map_err(|e| anyhow!("Failed to create channel: {e:?}"))?;

    // Persist the channel
    persister
        .persist_channel_peer(channel_peer.clone())
        .await
        .context("Failed to persist channel peer")?;

    info!("Successfully opened channel with {}", channel_peer);
    if let Err(e) =
        channel_peer_tx.try_send(ChannelPeerUpdate::Add(channel_peer))
    {
        error!("Couldn't update p2p reconnector of new channel peer: {e:#}");
    }

    Ok(())
}
