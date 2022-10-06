use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use bitcoin::util::address::Address;
use common::api::{NodePk, UserPk};
use common::cli::{Network, RunArgs};
use common::ln::peer::ChannelPeer;
use common::rng::SysRng;
use common::shutdown::ShutdownChannel;
use common::test_utils::regtest::Regtest;
use lexe_ln::alias::NetworkGraphType;
use lexe_ln::{channel, command, logger, p2p};
use tokio::sync::mpsc;

use crate::channel_manager::{NodeChannelManager, USER_CONFIG};
use crate::command::owner;
use crate::peer_manager::NodePeerManager;
use crate::persister::NodePersister;
use crate::run::UserNode;

/// Helper to return a default RunArgs struct for testing.
fn default_args() -> RunArgs {
    default_args_for_user(UserPk::from_i64(1))
}

fn default_args_for_user(user_pk: UserPk) -> RunArgs {
    RunArgs {
        user_pk,
        network: Network::from_str("regtest").unwrap(),
        node_dns_name: "localhost".to_owned(),
        mock: true,
        ..RunArgs::default()
    }
}

struct CommandTestHarness {
    regtest: Regtest,
    node: UserNode,
}

impl CommandTestHarness {
    async fn init(mut args: RunArgs) -> Self {
        logger::init_for_testing();

        // Init bitcoind and update rpc info
        let (regtest, rpc_info) = Regtest::init().await;
        args.bitcoind_rpc = rpc_info;

        // Init node
        let mut rng = SysRng::new();
        let shutdown = ShutdownChannel::new();
        let node = UserNode::init(&mut rng, args, shutdown)
            .await
            .expect("Error during init");

        Self { regtest, node }
    }

    async fn run(self) {
        self.node.run().await.expect("Error while running");
    }

    fn channel_manager(&self) -> NodeChannelManager {
        self.node.channel_manager.clone()
    }

    fn peer_manager(&self) -> NodePeerManager {
        self.node.peer_manager.clone()
    }

    fn persister(&self) -> NodePersister {
        self.node.persister.clone()
    }

    fn network_graph(&self) -> Arc<NetworkGraphType> {
        self.node.network_graph.clone()
    }

    fn node_pk(&self) -> NodePk {
        let mut rng = SysRng::new();
        NodePk(self.node.keys_manager.derive_pk(&mut rng))
    }

    fn p2p_address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.node.peer_port))
    }

    async fn get_new_address(&self) -> Address {
        self.node
            .wallet
            .get_new_address()
            .await
            .expect("Failed to get new address")
    }

    /// Funds the node with some generated blocks,
    /// returning the address the funds were sent to.
    async fn fund_node(&self) -> Address {
        // Coinbase funds can only be spent after 100 blocks
        let address = self.get_new_address().await;
        self.regtest.fund_address(&address).await;
        address
    }
}

/// Tests that a node can initialize, sync, and shutdown.
#[tokio::test]
async fn init_sync_shutdown() {
    let mut args = default_args();
    args.shutdown_after_sync_if_no_activity = true;

    let h = CommandTestHarness::init(args).await;
    h.run().await;
}

/// Tests the node_info handler.
#[tokio::test]
async fn node_info() {
    let args = default_args();
    let h = CommandTestHarness::init(args).await;

    command::node_info(h.channel_manager(), h.peer_manager());
}

/// Tests the list_channels handler.
#[tokio::test]
async fn list_channels() {
    let args = default_args();
    let h = CommandTestHarness::init(args).await;

    owner::list_channels(h.channel_manager(), h.network_graph()).unwrap();
}

/// Tests connecting two nodes to each other.
#[tokio::test]
async fn connect_peer() {
    let args1 = default_args_for_user(UserPk::from_i64(1));
    let args2 = default_args_for_user(UserPk::from_i64(2));
    let (node1, node2) = tokio::join!(
        CommandTestHarness::init(args1),
        CommandTestHarness::init(args2),
    );

    // Build prereqs
    let peer_manager1 = node1.peer_manager();
    let peer_manager2 = node2.peer_manager();
    let channel_peer = ChannelPeer {
        node_pk: node2.node_pk(),
        addr: node2.p2p_address(),
    };

    // Prior to connecting
    let pre_node_info1 =
        command::node_info(node1.channel_manager(), node1.peer_manager());
    assert_eq!(pre_node_info1.num_peers, 0);
    let pre_node_info2 =
        command::node_info(node2.channel_manager(), node2.peer_manager());
    assert_eq!(pre_node_info2.num_peers, 0);
    assert!(peer_manager1.get_peer_node_ids().is_empty());
    assert!(peer_manager2.get_peer_node_ids().is_empty());

    // Connect
    p2p::connect_channel_peer_if_necessary(peer_manager1.clone(), channel_peer)
        .await
        .expect("Failed to connect");

    // After connecting
    let post_node_info1 =
        command::node_info(node1.channel_manager(), node1.peer_manager());
    assert_eq!(post_node_info1.num_peers, 1);
    let post_node_info2 =
        command::node_info(node2.channel_manager(), node2.peer_manager());
    assert_eq!(post_node_info2.num_peers, 1);
    assert_eq!(peer_manager1.get_peer_node_ids().len(), 1);
    assert_eq!(peer_manager2.get_peer_node_ids().len(), 1);
}

/// Tests opening a channel
#[tokio::test]
async fn open_channel() {
    let mut args1 = default_args_for_user(UserPk::from_i64(1));
    let mut args2 = default_args_for_user(UserPk::from_i64(2));
    args1.shutdown_after_sync_if_no_activity = true;
    args2.shutdown_after_sync_if_no_activity = true;

    let (node1, node2) = tokio::join!(
        CommandTestHarness::init(args1),
        CommandTestHarness::init(args2),
    );

    // Fund both nodes
    node1.fund_node().await;
    node2.fund_node().await;

    // Prepare open channel prerequisites
    let channel_peer = ChannelPeer {
        node_pk: node2.node_pk(),
        addr: node2.p2p_address(),
    };
    let channel_value_sat = 1_000_000;
    let (channel_peer_tx, _rx) =
        mpsc::channel(common::constants::DEFAULT_CHANNEL_SIZE);

    // Prior to opening
    let pre_node_info =
        command::node_info(node1.channel_manager(), node1.peer_manager());
    assert_eq!(pre_node_info.num_channels, 0);

    // Open the channel
    println!("Opening channel");

    channel::open_channel(
        node1.channel_manager(),
        node1.peer_manager(),
        node1.persister(),
        channel_peer,
        channel_value_sat,
        &channel_peer_tx,
        USER_CONFIG,
    )
    .await
    .expect("Failed to open channel");

    // After opening
    let post_node_info =
        command::node_info(node1.channel_manager(), node1.peer_manager());
    assert_eq!(post_node_info.num_channels, 1);

    // Wait for a graceful shutdown to complete before exiting this test (and
    // thus dropping BitcoinD which kills the bitcoind process) so that the
    // event handler has enough time to handle the FundingGenerationReady event
    // before BitcoinD is dropped (and `kill`ed), otherwise this test fails.
    node1.run().await;
    node2.run().await;
}
