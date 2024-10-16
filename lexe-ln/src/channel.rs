use std::fmt::{self, Display};

use anyhow::{anyhow, ensure, Context};
use common::{
    api::{command::CloseChannelRequest, Empty, NodePk},
    constants::DEFAULT_CHANNEL_SIZE,
    ln::{
        amount::Amount,
        channel::{LxChannelId, LxUserChannelId},
        peer::ChannelPeer,
    },
};
use lightning::{
    events::ClosureReason, ln::ChannelId, util::config::UserConfig,
};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, instrument};

use crate::{
    p2p::{self, ChannelPeerUpdate},
    traits::{
        LexeChannelManager, LexeInnerPersister, LexePeerManager, LexePersister,
    },
};

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
) -> anyhow::Result<Empty>
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
        UserToLsp { lsp_channel_peer } => {
            let ChannelPeer { node_pk, addr } = lsp_channel_peer;
            p2p::connect_peer_if_necessary(peer_manager, &node_pk, &[addr])
                .await
                .context("Could not connect to LSP")?;
        }
        LspToUser { user_node_pk } => ensure!(
            peer_manager.peer_by_node_id(&user_node_pk.0).is_some(),
            "LSP must be connected to user before opening channel",
        ),
        LspToExternal {
            channel_peer,
            persister,
            channel_peer_tx,
        } => {
            let ChannelPeer { node_pk, addr } = channel_peer;
            let addrs = [addr];
            p2p::connect_peer_if_necessary(peer_manager, &node_pk, &addrs)
                .await
                .context("Could not connect to external node")?;

            // Before we actually create the channel, persist the ChannelPeer so
            // that there is no chance of having an open channel without the
            // associated ChannelPeer information.
            // TODO(max): This should be renamed to persist_external_peer
            let [addr] = addrs;
            let channel_peer = ChannelPeer { node_pk, addr };
            persister
                .persist_channel_peer(channel_peer.clone())
                .await
                .context("Failed to persist channel peer")?;

            // Also tell our p2p reconnector to continuously try to reconnect to
            // this channel peer if we disconnect for some reason.
            channel_peer_tx
                .try_send(ChannelPeerUpdate::Add(channel_peer))
                .context("Couldn't tell p2p reconnector of new channel peer")?;
        }
    };

    // Finally, create the channel.
    let push_msat = 0; // No need for this yet
    let temporary_channel_id = None; // No need for this yet
    channel_manager
        .create_channel(
            responder_node_pk.0,
            channel_value.sats_u64(),
            push_msat,
            user_channel_id,
            temporary_channel_id,
            Some(user_config),
        )
        .map_err(|e| anyhow!("Failed to create channel: {e:?}"))?;

    info!("Successfully opened channel");
    Ok(Empty {})
}

/// Initiates a channel close. Supports both cooperative (bilateral) and force
/// (unilateral) channel closes.
pub fn close_channel<CM, PM, PS>(
    req: CloseChannelRequest,
    channel_manager: CM,
    peer_manager: PM,
) -> anyhow::Result<Empty>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    let channel_id = req.channel_id;
    let force_close = req.force_close;
    let maybe_counterparty = req.maybe_counterparty;
    info!(
        %channel_id, %force_close, ?maybe_counterparty,
        "Initiating channel close",
    );

    let counterparty = maybe_counterparty
        .or_else(|| {
            channel_manager
                .list_channels()
                .into_iter()
                .find(|c| c.channel_id.0 == channel_id.0)
                .map(|c| NodePk(c.counterparty.node_id))
        })
        .with_context(|| format!("No channel exists with id {channel_id}"))?;

    let channel_id = ChannelId::from(channel_id);
    if force_close {
        channel_manager
            .force_close_broadcasting_latest_txn(&channel_id, &counterparty.0)
            .map_err(|e| anyhow!("(Force close) LDK returned error: {e:?}"))?;
    } else {
        ensure!(
            peer_manager.peer_by_node_id(&counterparty.0).is_some(),
            "Cannot initiate cooperative close with disconnected peer"
        );

        channel_manager
            .close_channel(&channel_id, &counterparty.0)
            .map_err(|e| anyhow!("(Co-op close) LDK returned error: {e:?}"))?;
    }

    info!(%channel_id, %force_close, "Successfully initiated channel close");
    Ok(Empty {})
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

/// Channel lifecycle events emitted from the node event handler.
///
/// Tail these events using the [`ChannelEventsMonitor`].
#[derive(Clone)]
pub enum ChannelEvent {
    Pending {
        user_channel_id: LxUserChannelId,
        channel_id: LxChannelId,
        funding_txo: bitcoin::OutPoint,
    },
    Ready {
        user_channel_id: LxUserChannelId,
        channel_id: LxChannelId,
    },
    Closed {
        user_channel_id: LxUserChannelId,
        channel_id: LxChannelId,
        reason: ClosureReason,
    },
}

impl ChannelEvent {
    pub fn channel_id(&self) -> &LxChannelId {
        match self {
            Self::Pending { channel_id, .. } => channel_id,
            Self::Ready { channel_id, .. } => channel_id,
            Self::Closed { channel_id, .. } => channel_id,
        }
    }

    pub fn user_channel_id(&self) -> &LxUserChannelId {
        match self {
            Self::Pending {
                user_channel_id, ..
            } => user_channel_id,
            Self::Ready {
                user_channel_id, ..
            } => user_channel_id,
            Self::Closed {
                user_channel_id, ..
            } => user_channel_id,
        }
    }
}

/// The `ChannelEventsMonitor` lets API handlers like `open_channel` and
/// `close_channel` wait on channel lifecycle events (pending, ready, closed)
/// for specific channels.
///
/// We use a [`tokio::sync::broadcast`] channel here because (1) event
/// notification is a noop if there are no waiters, which is common, and (2) we
/// don't need to garbage collect waiters that timeout.
#[derive(Clone)]
pub struct ChannelEventsMonitor {
    event_tx: broadcast::Sender<ChannelEvent>,
}

impl ChannelEventsMonitor {
    pub fn new() -> Self {
        Self {
            event_tx: broadcast::channel(DEFAULT_CHANNEL_SIZE).0,
        }
    }

    /// Called from the event handler, when it observes a channel event.
    ///
    /// See:
    /// * [crate::event::handle_channel_pending]
    /// * [crate::event::handle_channel_ready]
    /// * [crate::event::handle_channel_closed]
    pub fn notify(&self, event: ChannelEvent) {
        // `broadcast::Sender::send` returns an error if there are no active
        // receivers. That's fine in this case.
        let _ = self.event_tx.send(event);
    }

    /// Start listening to all new [`ChannelEvent`]s that get [`Self::notify`]'d
    /// after this point.
    ///
    /// Be sure to start tailing events quickly so they don't queue up and you
    /// don't lose events.
    pub fn subscribe(&self) -> ChannelEventsRx<'_> {
        ChannelEventsRx::subscribe(&self.event_tx)
    }
}

pub struct ChannelEventsRx<'a> {
    // Hold on to this sender handle so the channel can't shutdown while we're
    // waiting.
    _event_tx: &'a broadcast::Sender<ChannelEvent>,
    event_rx: broadcast::Receiver<ChannelEvent>,
}

impl<'a> ChannelEventsRx<'a> {
    fn subscribe(event_tx: &'a broadcast::Sender<ChannelEvent>) -> Self {
        Self {
            _event_tx: event_tx,
            event_rx: event_tx.subscribe(),
        }
    }

    /// Wait for the next [`ChannelEvent`] that makes `filter` return true.
    ///
    /// Will wait indefinitely, so make sure there's a timeout somewhere around
    /// this.
    pub async fn next_filtered(
        &mut self,
        filter: impl Fn(&ChannelEvent) -> bool,
    ) -> ChannelEvent {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match self.event_rx.recv().await {
                Ok(event) =>
                    if filter(&event) {
                        return event;
                    },
                Err(RecvError::Closed) => panic!(
                    "This cannot happen. We currently have a handle to the \
                     `event_tx` sender, so the channel cannot be closed."
                ),
                // We missed some notifications somehow (too slow). Nothing
                // much we can do other than keep going
                // until timeout.
                Err(RecvError::Lagged(_)) => (),
            }
        }
    }
}
