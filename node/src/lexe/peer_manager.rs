use std::fmt::{self, Display};
use std::net::SocketAddr;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bitcoin::secp256k1::PublicKey;
use common::rng::Crng;
use lightning::chain::keysinterface::{KeysInterface, Recipient};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use secrecy::zeroize::Zeroizing;
use tokio::net::TcpStream;
use tokio::time;

use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
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
        channel_manager: LexeChannelManager,
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
        if self.get_peer_node_ids().contains(&channel_peer.pubkey) {
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

        // `setup_outbound()` returns a future which completes when the
        // connection closes, which we do not need to poll because a
        // task was spawned for it.
        let _ = lightning_net_tokio::setup_outbound(
            self.as_arc_inner(),
            channel_peer.pubkey,
            stream,
        );

        Ok(())
    }
}

#[derive(Clone)]
pub struct ChannelPeer {
    pub pubkey: PublicKey,
    pub addr: SocketAddr,
}

impl From<(PublicKey, SocketAddr)> for ChannelPeer {
    fn from((pubkey, addr): (PublicKey, SocketAddr)) -> Self {
        Self { pubkey, addr }
    }
}

/// <pubkey>@<addr>
impl FromStr for ChannelPeer {
    type Err = anyhow::Error;
    fn from_str(pubkey_at_addr: &str) -> anyhow::Result<Self> {
        // vec![<pubkey>, <addr>]
        let mut pubkey_and_addr = pubkey_at_addr.split('@');
        let pubkey_str = pubkey_and_addr
            .next()
            .context("Missing <pubkey> in <pubkey>@<addr> peer address")?;
        let addr_str = pubkey_and_addr
            .next()
            .context("Missing <addr> in <pubkey>@<addr> peer address")?;

        let pubkey = PublicKey::from_str(pubkey_str)
            .context("Could not deserialize PublicKey from LowerHex")?;
        let addr = SocketAddr::from_str(addr_str)
            .context("Could not parse socket address from string")?;

        Ok(Self { pubkey, addr })
    }
}

/// <pubkey>@<addr>
impl Display for ChannelPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pubkey, self.addr)
    }
}
