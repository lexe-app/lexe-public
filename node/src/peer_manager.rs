use std::ops::Deref;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use common::rng::Crng;
use lexe_ln::alias::{OnionMessengerType, P2PGossipSyncType};
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lexe_ln::traits::ArcInner;
use lightning::chain::keysinterface::{NodeSigner, Recipient};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use secrecy::zeroize::Zeroizing;

use crate::alias::PeerManagerType;
use crate::channel_manager::NodeChannelManager;

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
        rng: &mut dyn Crng,
        keys_manager: &LexeKeysManager,
        channel_manager: NodeChannelManager,
        gossip_sync: Arc<P2PGossipSyncType>,
        onion_messenger: Arc<OnionMessengerType>,
        logger: LexeTracingLogger,
    ) -> Self {
        let mut ephemeral_bytes = Zeroizing::new([0u8; 32]);
        rng.fill_bytes(ephemeral_bytes.as_mut_slice());

        let lightning_msg_handler = MessageHandler {
            chan_handler: channel_manager,
            route_handler: gossip_sync,
            onion_message_handler: onion_messenger,
        };
        let node_secret = keys_manager
            .get_node_secret(Recipient::Node)
            .expect("Always succeeds when called with Recipient::Node");

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

        let peer_manager: PeerManagerType = PeerManagerType::new(
            lightning_msg_handler,
            node_secret,
            current_time,
            &ephemeral_bytes,
            logger,
            Arc::new(IgnoringMessageHandler {}),
        );

        Self(Arc::new(peer_manager))
    }
}

impl ArcInner<PeerManagerType> for NodePeerManager {
    fn arc_inner(&self) -> Arc<PeerManagerType> {
        self.0.clone()
    }
}
