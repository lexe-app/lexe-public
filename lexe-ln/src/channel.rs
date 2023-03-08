use std::fmt::{self, Display};

use anyhow::{anyhow, Context};
use common::hex;
use common::ln::peer::ChannelPeer;
use common::rng::Crng;
use lightning::util::config::UserConfig;
use tokio::sync::mpsc;
use tracing::{debug, info, instrument};

use crate::p2p::{self, ChannelPeerUpdate};
use crate::test_event::TestEventSender;
use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

/// A newtype for [`ChannelDetails::channel_id`] for semantic clarity.
///
/// [`ChannelDetails::channel_id`]: lightning::ln::channelmanager::ChannelDetails::channel_id
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChannelId(pub [u8; 32]);

/// A smol enum which specifies whether our channel counterparty is an external
/// LN node, a Lexe user node, or Lexe's LSP. The required parameters and
/// behavior of [`open_channel`] may be different in each case.
pub enum CounterpartyKind<PS: LexePersister> {
    /// External LN node. We need to save their [`ChannelPeer`] in this case.
    External {
        persister: PS,
        channel_peer_tx: mpsc::Sender<ChannelPeerUpdate>,
    },
    /// Lexe user node.
    UserNode,
    /// Lexe's LSP.
    Lsp,
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
#[allow(clippy::too_many_arguments)]
#[instrument(skip_all, name = "(open-channel)")]
pub async fn open_channel<CM, PM, PS>(
    channel_manager: CM,
    peer_manager: PM,
    user_channel_id: u128,
    channel_peer: ChannelPeer,
    channel_value_sat: u64,
    counterparty: CounterpartyKind<PS>,
    user_config: UserConfig,
    test_event_tx: &TestEventSender,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    info!("Opening channel with {counterparty} {channel_peer}");

    // Make sure that we're connected to the channel peer
    p2p::connect_channel_peer_if_necessary(
        peer_manager,
        channel_peer.clone(),
        test_event_tx,
    )
    .await
    .context("Failed to connect to peer")?;

    // Create the channel.
    let push_msat = 0; // No need for this yet
    channel_manager
        .create_channel(
            channel_peer.node_pk.0,
            channel_value_sat,
            push_msat,
            user_channel_id,
            Some(user_config),
        )
        .map_err(|e| anyhow!("Failed to create channel: {e:?}"))?;
    debug!("Created channel");

    // If we opened a channel with an external LN node, we need to save their
    // ChannelPeer info so that we can reconnect to them after restart, and tell
    // our p2p reconnector to continuously try reconnecting if we disconnected.
    if let CounterpartyKind::External {
        channel_peer_tx,
        persister,
    } = counterparty
    {
        // TODO(max): This should be renamed to persist_external_peer
        persister
            .persist_channel_peer(channel_peer.clone())
            .await
            .context("Failed to persist channel peer")?;

        channel_peer_tx
            .try_send(ChannelPeerUpdate::Add(channel_peer))
            .map_err(|e| anyhow!("{e:#}"))
            .context(
                "Couldn't update p2p reconnector of new channel peer: {e:#}",
            )?;
    }

    info!("Successfully opened channel");
    Ok(())
}

// --- Display impls --- //

impl<PS: LexePersister> Display for CounterpartyKind<PS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::External { .. } => write!(f, "external"),
            Self::UserNode => write!(f, "user node"),
            Self::Lsp => write!(f, "LSP"),
        }
    }
}

impl Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(&self.0))
    }
}
