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
    p2p::{spawn_process_events_task, ConnectionTx, PeerManagerTrait},
};
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
use lightning::ln::{
    msgs::SocketAddress,
    peer_handler::{IgnoringMessageHandler, MessageHandler, PeerHandleError},
};
use secrecy::zeroize::Zeroizing;

use crate::{
    alias::{OnionMessengerType, PeerManagerType},
    channel_manager::NodeChannelManager,
};

#[derive(Clone)]
pub struct NodePeerManager(Arc<Inner>);

struct Inner {
    peer_manager: PeerManagerType,
    process_events_tx: notify::Sender,
}

impl Deref for NodePeerManager {
    type Target = PeerManagerType;
    fn deref(&self) -> &Self::Target {
        &self.0.peer_manager
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
        shutdown: NotifyOnce,
    ) -> (Self, LxTask<()>) {
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

        let (process_events_tx, process_events_rx) = notify::channel();
        let node_peer_manager = Self(Arc::new(Inner {
            peer_manager,
            process_events_tx,
        }));

        // Spawn task that calls `PeerManager::process_events` on
        // `process_events_tx` notification.
        let process_events_task = spawn_process_events_task(
            node_peer_manager.clone(),
            process_events_rx,
            shutdown,
        );

        (node_peer_manager, process_events_task)
    }
}

// lexe_ln::p2p::PeerManagerTrait boilerplate
// TODO(phlip9): figure out how to make blanket trait impl work to avoid this
impl PeerManagerTrait for NodePeerManager {
    fn is_connected(&self, node_pk: &NodePk) -> bool {
        self.0.peer_manager.peer_by_node_id(&node_pk.0).is_some()
    }

    fn notify_process_events_task(&self) {
        self.0.process_events_tx.send();
    }

    fn new_outbound_connection(
        &self,
        node_pk: &NodePk,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<Vec<u8>, PeerHandleError> {
        self.0.peer_manager.new_outbound_connection(
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
            .peer_manager
            .new_inbound_connection(conn_tx, addr.map(SocketAddress::from))
    }

    fn socket_disconnected(&self, conn_tx: &ConnectionTx) {
        self.0.peer_manager.socket_disconnected(conn_tx)
    }

    fn read_event(
        &self,
        conn_tx: &mut ConnectionTx,
        data: &[u8],
    ) -> Result<bool, PeerHandleError> {
        self.0.peer_manager.read_event(conn_tx, data)
    }

    fn process_events(&self) {
        self.0.peer_manager.process_events()
    }

    fn write_buffer_space_avail(
        &self,
        conn_tx: &mut ConnectionTx,
    ) -> Result<(), PeerHandleError> {
        self.0.peer_manager.write_buffer_space_avail(conn_tx)
    }
}
