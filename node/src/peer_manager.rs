use std::{
    ops::Deref,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use common::rng::{Crng, RngExt};
use lexe_ln::{
    alias::P2PGossipSyncType, keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
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
