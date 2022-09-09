use std::fmt::{self, Display};
use std::net::SocketAddr;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use bitcoin::secp256k1::PublicKey;
use common::rng::Crng;
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lightning::chain::keysinterface::{KeysInterface, Recipient};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use secrecy::zeroize::Zeroizing;
use tokio::net::TcpStream;
use tokio::time;

use crate::lexe::channel_manager::NodeChannelManager;
use crate::types::{P2PGossipSyncType, PeerManagerType};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// An Arc is held internally, so it is fine to clone directly.
#[derive(Clone)]
pub struct LexePeerManager(Arc<PeerManagerType>);

impl Deref for LexePeerManager {
    type Target = PeerManagerType;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl LexePeerManager {
    pub fn init(
        rng: &mut dyn Crng,
        keys_manager: &LexeKeysManager,
        channel_manager: NodeChannelManager,
        gossip_sync: Arc<P2PGossipSyncType>,
        logger: LexeTracingLogger,
    ) -> Self {
        let mut ephemeral_bytes = Zeroizing::new([0u8; 32]);
        rng.fill_bytes(ephemeral_bytes.as_mut_slice());

        let lightning_msg_handler = MessageHandler {
            chan_handler: channel_manager,
            route_handler: gossip_sync,
        };
        let node_secret = keys_manager
            .get_node_secret(Recipient::Node)
            .expect("Always succeeds when called with Recipient::Node");

        let peer_manager: PeerManagerType = PeerManagerType::new(
            lightning_msg_handler,
            node_secret,
            &ephemeral_bytes,
            logger,
            Arc::new(IgnoringMessageHandler {}),
        );

        Self(Arc::new(peer_manager))
    }

    pub fn as_arc_inner(&self) -> Arc<PeerManagerType> {
        self.0.clone()
    }

    #[allow(dead_code)] // TODO Remove once this fn is used in sgx
    pub async fn connect_peer_if_necessary(
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

    pub async fn do_connect_peer(
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
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct ChannelPeer {
    pub pk: PublicKey,
    pub addr: SocketAddr,
}

impl ChannelPeer {
    pub fn new(pk: PublicKey, addr: SocketAddr) -> Self {
        Self { pk, addr }
    }
}

/// <pk>@<addr>
impl FromStr for ChannelPeer {
    type Err = anyhow::Error;
    fn from_str(pk_at_addr: &str) -> anyhow::Result<Self> {
        // vec![<pk>, <addr>]
        let mut pk_and_addr = pk_at_addr.split('@');
        let pk_str = pk_and_addr
            .next()
            .context("Missing <pk> in <pk>@<addr> peer address")?;
        let addr_str = pk_and_addr
            .next()
            .context("Missing <addr> in <pk>@<addr> peer address")?;

        let pk = PublicKey::from_str(pk_str)
            .context("Could not deserialize PublicKey from LowerHex")?;
        let addr = SocketAddr::from_str(addr_str)
            .context("Could not parse socket address from string")?;

        Ok(Self { pk, addr })
    }
}

/// <pk>@<addr>
impl Display for ChannelPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pk, self.addr)
    }
}
