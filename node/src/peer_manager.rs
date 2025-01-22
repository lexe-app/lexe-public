use std::{
    ops::Deref,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use common::{
    api::user::NodePk,
    ln::addr::LxSocketAddress,
    rng::{Crng, RngExt},
};
use lexe_ln::{
    alias::P2PGossipSyncType,
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    p2p::{ConnectionTx, PeerManagerTrait},
};
use lightning::ln::{
    msgs::SocketAddress,
    peer_handler::{IgnoringMessageHandler, MessageHandler, PeerHandleError},
};
use secrecy::zeroize::Zeroizing;

use crate::{
    alias::{OnionMessengerType, PeerManagerType},
    channel_manager::NodeChannelManager,
};

/// An Arc is held internally, so it is fine to clone directly.
#[derive(Clone)]
pub struct NodePeerManager(Arc<PeerManagerType>);

impl Deref for NodePeerManager {
    type Target = PeerManagerType;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl NodePeerManager {
    pub(crate) fn init(
        mut rng: &mut dyn Crng,
        keys_manager: Arc<LexeKeysManager>,
        channel_manager: NodeChannelManager,
        gossip_sync: Arc<P2PGossipSyncType>,
        onion_messenger: Arc<OnionMessengerType>,
        logger: LexeTracingLogger,
    ) -> Self {
        let lightning_msg_handler = MessageHandler {
            chan_handler: channel_manager,
            route_handler: gossip_sync,
            onion_message_handler: onion_messenger,
            custom_message_handler: Arc::new(IgnoringMessageHandler {}),
        };

        // `current_time` is supposed to be monotonically increasing across node
        // restarts, but since secure timekeeping within an enclave is a hard
        // problem, and this field is used to help peers choose between
        // multiple node announcements (it becomes last_node_announcement_serial
        // which then becomes the timestamp field of UnsignedNodeAnnouncement
        // which is specified in BOLT#07), using the system time is fine.
        let current_time: u32 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before Unix epoch")
            .as_secs()
            .try_into()
            .expect("It's the year 2038 and you own nothing");

        let ephemeral_bytes = Zeroizing::new(rng.gen_bytes());
        let peer_manager: PeerManagerType = PeerManagerType::new(
            lightning_msg_handler,
            current_time,
            &ephemeral_bytes,
            logger,
            keys_manager,
        );

        Self(Arc::new(peer_manager))
    }
}

// lexe_ln::p2p::PeerManagerTrait boilerplate
// TODO(phlip9): figure out how to make blanket trait impl work to avoid this
impl PeerManagerTrait for NodePeerManager {
    fn is_connected(&self, node_pk: &NodePk) -> bool {
        self.0.as_ref().peer_by_node_id(&node_pk.0).is_some()
    }

    fn new_outbound_connection(
        &self,
        node_pk: &NodePk,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<Vec<u8>, PeerHandleError> {
        self.0.as_ref().new_outbound_connection(
            node_pk.0,
            conn_tx,
            addr.map(SocketAddress::from),
        )
    }

    fn new_inbound_connection(
        &self,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<(), PeerHandleError> {
        self.0
            .as_ref()
            .new_inbound_connection(conn_tx, addr.map(SocketAddress::from))
    }

    fn socket_disconnected(&self, conn_tx: &ConnectionTx) {
        self.0.as_ref().socket_disconnected(conn_tx)
    }

    fn read_event(
        &self,
        conn_tx: &mut ConnectionTx,
        data: &[u8],
    ) -> Result<bool, PeerHandleError> {
        self.0.as_ref().read_event(conn_tx, data)
    }

    fn process_events(&self) {
        self.0.as_ref().process_events()
    }

    fn write_buffer_space_avail(
        &self,
        conn_tx: &mut ConnectionTx,
    ) -> Result<(), PeerHandleError> {
        self.0.as_ref().write_buffer_space_avail(conn_tx)
    }
}
