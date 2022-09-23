use std::cmp::min;
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use common::rng::Crng;
use lexe_ln::alias::{OnionMessengerType, P2PGossipSyncType};
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lexe_ln::peer::ChannelPeer;
use lightning::chain::keysinterface::{KeysInterface, Recipient};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use secrecy::zeroize::Zeroizing;
use tokio::net::TcpStream;
use tokio::time;

use crate::lexe::channel_manager::NodeChannelManager;
use crate::types::PeerManagerType;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// Exponential backoff
const INITIAL_WAIT_MS: u64 = 10;
const MAXIMUM_WAIT_MS: u64 = 30_000;
const EXP_BASE: u64 = 2;

/// An Arc is held internally, so it is fine to clone directly.
#[derive(Clone)]
pub(crate) struct NodePeerManager(Arc<PeerManagerType>);

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
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before Unix epoch")
            .as_secs();

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

    pub(crate) fn as_arc_inner(&self) -> Arc<PeerManagerType> {
        self.0.clone()
    }

    #[allow(dead_code)] // TODO Remove once this fn is used in sgx
    pub(crate) async fn connect_peer_if_necessary(
        &self,
        channel_peer: ChannelPeer,
    ) -> anyhow::Result<()> {
        // Return immediately if we're already connected to the peer
        if self.get_peer_node_ids().contains(&channel_peer.pk) {
            return Ok(());
        }

        // Otherwise, initiate the connection
        self.do_connect_peer(channel_peer)
            .await
            .context("Failed to connect to peer")
    }

    pub(crate) async fn do_connect_peer(
        &self,
        channel_peer: ChannelPeer,
    ) -> anyhow::Result<()> {
        let stream = time::timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect(channel_peer.addr),
        )
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
            self.as_arc_inner(),
            channel_peer.pk,
            stream,
        );
        let mut connection_closed_fut = Box::pin(connection_closed_fut);
        // Use exponential backoff when polling so that a stalled connection
        // doesn't keep the node always in memory
        let mut backoff_durations = (0..)
            .map(|index| INITIAL_WAIT_MS * EXP_BASE.pow(index))
            .map(|wait| min(wait, MAXIMUM_WAIT_MS))
            .map(Duration::from_millis);
        loop {
            // Check if the connection has been closed
            match futures::poll!(&mut connection_closed_fut) {
                std::task::Poll::Ready(_) => {
                    bail!("Failed initial connection to peer - error unknown");
                }
                std::task::Poll::Pending => {}
            }

            // Check if the connection has been established
            if self
                .get_peer_node_ids()
                .iter()
                .any(|pk| *pk == channel_peer.pk)
            {
                // Connection confirmed, break and return Ok
                break;
            } else {
                // Connection not confirmed yet, wait before checking again
                tokio::time::sleep(backoff_durations.next().unwrap()).await;
            }
        }

        Ok(())
    }
}
