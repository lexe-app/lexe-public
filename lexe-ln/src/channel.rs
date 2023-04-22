use std::fmt::{self, Display};

use anyhow::{anyhow, ensure, Context};
use common::api::NodePk;
use common::hex;
use common::ln::amount::Amount;
use common::ln::peer::ChannelPeer;
use common::rng::Crng;
use lightning::util::config::UserConfig;
use tokio::sync::mpsc;
use tracing::{info, instrument};

use crate::p2p::{self, ChannelPeerUpdate};
use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

/// A newtype for [`ChannelDetails::channel_id`] for semantic clarity.
///
/// [`ChannelDetails::channel_id`]: lightning::ln::channelmanager::ChannelDetails::channel_id
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChannelId(pub [u8; 32]);

/// Specifies the channel initiator-responder relationship. The required
/// parameters and behavior of [`open_channel`] may be different in each case.
pub enum ChannelRelationship<PS: LexePersister> {
    /// A Lexe user node is opening a channel to the LSP.
    /// The LSP's [`ChannelPeer`] must be specified.
    UserToLsp { lsp_channel_peer: ChannelPeer },
    /// Lexe's LSP is opening a channel to a user node.
    /// The user node's [`NodePk`] must be specified.
    LspToUser { user_node_pk: NodePk },
    /// The LSP is opening a channel to an external LN node.
    /// The external LN node's [`ChannelPeer`] must be specified, along with
    /// the utilities required to persist and reconnect to the external peer.
    LspToExternal {
        channel_peer: ChannelPeer,
        persister: PS,
        channel_peer_tx: mpsc::Sender<ChannelPeerUpdate>,
    },
}

/// Generates a random [`u128`] which can be used as a [`user_channel_id`].
///
/// [`user_channel_id`]: lightning::ln::channelmanager::ChannelDetails::user_channel_id
pub fn get_random_u128<R: Crng>(rng: &mut R) -> u128 {
    let mut buf = [0u8; 16];
    rng.fill_bytes(&mut buf);
    u128::from_le_bytes(buf)
}

/// Handles the full logic of opening a channel, including connecting to the
/// peer, creating the channel, and persisting the newly created channel.
#[instrument(skip_all, name = "(open-channel)")]
pub async fn open_channel<CM, PM, PS>(
    channel_manager: CM,
    peer_manager: PM,
    user_channel_id: u128,
    channel_value: Amount,
    relationship: ChannelRelationship<PS>,
    user_config: UserConfig,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    let responder_node_pk = relationship.responder_node_pk();
    info!("Opening a {relationship} channel to {responder_node_pk}");

    // Ensure that we are connected to the channel responder.
    // - In the UserToLsp and LspToExternal cases, we may initiate an outgoing
    //   connection if we are not already connected.
    // - In the LspToUser case, the caller must ensure that we are already
    //   connected to the user prior to open_channel.
    // - If the LSP is opening a channel with an external LN node, we must
    //   ensure that we've persisted the counterparty's ChannelPeer information
    //   so that we can connect with them after restart.
    use ChannelRelationship::*;
    match relationship {
        UserToLsp { lsp_channel_peer } =>
            p2p::connect_channel_peer_if_necessary(
                peer_manager,
                lsp_channel_peer,
            )
            .await
            .context("Could not connect to LSP")?,
        LspToUser { user_node_pk } => ensure!(
            p2p::is_connected(peer_manager, &user_node_pk),
            "LSP must be connected to user before opening channel",
        ),
        LspToExternal {
            channel_peer,
            persister,
            channel_peer_tx,
        } => {
            p2p::connect_channel_peer_if_necessary(
                peer_manager,
                channel_peer.clone(),
            )
            .await
            .context("Could not connect to external node")?;

            // Before we actually create the channel, persist the ChannelPeer so
            // that there is no chance of having an open channel without the
            // associated ChannelPeer information.
            // TODO(max): This should be renamed to persist_external_peer
            persister
                .persist_channel_peer(channel_peer.clone())
                .await
                .context("Failed to persist channel peer")?;

            // Also tell our p2p reconnector to continuously try to reconnect to
            // this channel peer if we disconnect for some reason.
            channel_peer_tx
                .try_send(ChannelPeerUpdate::Add(channel_peer))
                .map_err(|e| anyhow!("{e:#}"))
                .context(
                    "Couldn't update p2p reconnector of new channel peer: {e:#}",
                )?;
        }
    };

    // Finally, create the channel.
    let push_msat = 0; // No need for this yet
    channel_manager
        .create_channel(
            responder_node_pk.0,
            channel_value.sats_u64(),
            push_msat,
            user_channel_id,
            Some(user_config),
        )
        .map_err(|e| anyhow!("Failed to create channel: {e:?}"))?;

    info!("Successfully opened channel");
    Ok(())
}

// --- impl ChannelRelationship --- //

impl<PS: LexePersister> ChannelRelationship<PS> {
    /// Returns the channel responder's [`NodePk`]
    fn responder_node_pk(&self) -> NodePk {
        match self {
            Self::UserToLsp { lsp_channel_peer } => lsp_channel_peer.node_pk,
            Self::LspToUser { user_node_pk } => *user_node_pk,
            Self::LspToExternal { channel_peer, .. } => channel_peer.node_pk,
        }
    }
}

impl<PS: LexePersister> Display for ChannelRelationship<PS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserToLsp { .. } => write!(f, "user to LSP"),
            Self::LspToUser { .. } => write!(f, "LSP to user"),
            Self::LspToExternal { .. } => write!(f, "LSP to external"),
        }
    }
}

// --- impl ChannelId --- //

impl Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(&self.0))
    }
}
