//! Command tests.
//!
//! Note that all tests which call `CommandTestHarness::init()` must use a
//! multi-threaded runtime, since `LexeContext::init()` starts the
//! `BackgroundProcessor` which requires its own OS thread. A single worker
//! thread should be enough.

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use bitcoin::secp256k1::PublicKey;
use bitcoin::util::address::Address;
use bitcoind::bitcoincore_rpc::RpcApi;
use bitcoind::{self, BitcoinD, Conf};
use common::api::UserPk;
use common::rng::SysRng;

use crate::cli::{
    BitcoindRpcInfo, Network, NodeAlias, StartCommand, DEFAULT_BACKEND_URL,
    DEFAULT_RUNNER_URL,
};
use crate::command::owner;
use crate::init::LexeContext;
use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::peer_manager::{ChannelPeer, LexePeerManager};
use crate::lexe::persister::LexePersister;
use crate::types::NetworkGraphType;

/// Helper to return a default StartCommand struct for testing.
fn default_args() -> StartCommand {
    default_args_for_user(UserPk::new(1))
}

fn default_args_for_user(user_pk: UserPk) -> StartCommand {
    StartCommand {
        bitcoind_rpc: BitcoindRpcInfo {
            username: String::from("kek"),
            password: String::from("sadge"),
            host: String::new(), // Filled in when BitcoinD initializes
            port: 6969,          // Filled in when BitcoinD initializes
        },
        user_pk,
        peer_port: None,
        announced_node_name: NodeAlias::default(),
        network: Network::from_str("regtest").unwrap(),
        warp_port: None,
        shutdown_after_sync_if_no_activity: false,
        inactivity_timer_sec: 3600,
        repl: false,
        backend_url: DEFAULT_BACKEND_URL.into(),
        runner_url: DEFAULT_RUNNER_URL.into(),
        mock: true,
    }
}

struct CommandTestHarness {
    bitcoind: BitcoinD,
    ctx: LexeContext,
}

impl CommandTestHarness {
    async fn init(mut args: StartCommand) -> Self {
        // Construct bitcoin.conf
        let mut conf = Conf::default();
        // This rpcauth string corresponds to user `kek` and password `sadge`
        conf.args.push("-rpcauth=kek:b6c15926aee7ebfbd3669ec8a6515c79$2dba596a7d651187021b1f56d339f0fe465c2ab1b81c37b05e07a320b07822d7");

        // Init bitcoind
        let exe_path = bitcoind::downloaded_exe_path()
            .expect("Didn't specify bitcoind version in feature flags");
        let bitcoind = BitcoinD::with_conf(exe_path, &conf)
            .expect("Failed to init bitcoind");
        let host = bitcoind.params.rpc_socket.ip().to_string();
        let port = bitcoind.params.rpc_socket.port();
        // Update args to include the port
        args.bitcoind_rpc.host = host;
        args.bitcoind_rpc.port = port;

        // Init node
        let mut rng = SysRng::new();
        let ctx = LexeContext::init(&mut rng, args)
            .await
            .expect("Error during init");

        Self { bitcoind, ctx }
    }

    async fn sync(&mut self) {
        self.ctx.sync().await.expect("Error while running");
    }

    async fn run(&mut self) {
        self.ctx.run().await.expect("Error while running");
    }

    fn channel_manager(&self) -> LexeChannelManager {
        self.ctx.channel_manager.clone()
    }

    fn peer_manager(&self) -> LexePeerManager {
        self.ctx.peer_manager.clone()
    }

    fn persister(&self) -> LexePersister {
        self.ctx.persister.clone()
    }

    fn network_graph(&self) -> Arc<NetworkGraphType> {
        self.ctx.network_graph.clone()
    }

    fn pk(&self) -> PublicKey {
        let mut rng = SysRng::new();
        self.ctx.keys_manager.derive_pk(&mut rng)
    }

    fn p2p_address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.ctx.peer_port))
    }

    /// Funds the node with some generated blocks,
    /// returning the address the funds were sent to.
    async fn fund_node(&self) -> Address {
        // Coinbase funds can only be spent after 100 blocks
        let address = self.ctx.wallet.get_new_address().await;
        self.mine_n_blocks_to_address(101, &address).await;
        address
    }

    /// Mines 6 blocks.
    #[allow(dead_code)]
    async fn mine_6_blocks(&self) {
        // Plain bitcoind.client.generate() returns a deprecated error, so we
        // repeat `fund_node()`
        let address = self.ctx.wallet.get_new_address().await;
        self.mine_n_blocks_to_address(6, &address).await;
    }

    async fn mine_n_blocks_to_address(
        &self,
        num_blocks: u64,
        address: &Address,
    ) {
        self.bitcoind
            .client
            .generate_to_address(num_blocks, address)
            .expect("Failed to generate blocks");
    }
}

/// Tests that a node can initialize, sync, and shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn init_sync_shutdown() {
    let mut args = default_args();
    args.shutdown_after_sync_if_no_activity = true;

    let mut h = CommandTestHarness::init(args).await;
    h.sync().await;
    h.run().await;
}

/// Tests the node_info handler.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn node_info() {
    let args = default_args();
    let h = CommandTestHarness::init(args).await;

    owner::node_info(h.channel_manager(), h.peer_manager()).unwrap();
}

/// Tests the list_channels handler.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn list_channels() {
    let args = default_args();
    let h = CommandTestHarness::init(args).await;

    owner::list_channels(h.channel_manager(), h.network_graph()).unwrap();
}

/// Tests connecting two nodes to each other.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn connect_peer() {
    let args1 = default_args_for_user(UserPk::new(1));
    let args2 = default_args_for_user(UserPk::new(2));
    let (node1, node2) = tokio::join!(
        CommandTestHarness::init(args1),
        CommandTestHarness::init(args2),
    );

    // Build prereqs
    let peer_manager1 = node1.peer_manager();
    let peer_manager2 = node2.peer_manager();
    let channel_peer = ChannelPeer {
        pk: node2.pk(),
        addr: node2.p2p_address(),
    };

    // Prior to connecting
    let pre_node_info1 =
        owner::node_info(node1.channel_manager(), node1.peer_manager())
            .unwrap();
    assert_eq!(pre_node_info1.num_peers, 0);
    let pre_node_info2 =
        owner::node_info(node2.channel_manager(), node2.peer_manager())
            .unwrap();
    assert_eq!(pre_node_info2.num_peers, 0);
    assert!(peer_manager1.get_peer_node_ids().is_empty());
    assert!(peer_manager2.get_peer_node_ids().is_empty());

    // Connect
    peer_manager1
        .connect_peer_if_necessary(channel_peer)
        .await
        .expect("Failed to connect");

    // After connecting
    let post_node_info1 =
        owner::node_info(node1.channel_manager(), node1.peer_manager())
            .unwrap();
    assert_eq!(post_node_info1.num_peers, 1);
    let post_node_info2 =
        owner::node_info(node2.channel_manager(), node2.peer_manager())
            .unwrap();
    assert_eq!(post_node_info2.num_peers, 1);
    assert_eq!(peer_manager1.get_peer_node_ids().len(), 1);
    assert_eq!(peer_manager2.get_peer_node_ids().len(), 1);
}

/// Tests opening a channel
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn open_channel() {
    let mut args1 = default_args_for_user(UserPk::new(1));
    let mut args2 = default_args_for_user(UserPk::new(2));
    args1.shutdown_after_sync_if_no_activity = true;
    args2.shutdown_after_sync_if_no_activity = true;

    let (mut node1, mut node2) = tokio::join!(
        CommandTestHarness::init(args1),
        CommandTestHarness::init(args2),
    );

    // Fund both nodes
    node1.fund_node().await;
    node2.fund_node().await;

    // Prepare open channel prerequisites
    let channel_peer = ChannelPeer {
        pk: node2.pk(),
        addr: node2.p2p_address(),
    };
    let channel_value_sat = 1_000_000;

    // Prior to opening
    let pre_node_info =
        owner::node_info(node1.channel_manager(), node1.peer_manager())
            .unwrap();
    assert_eq!(pre_node_info.num_channels, 0);

    // Open the channel
    println!("Opening channel");
    node1
        .channel_manager()
        .open_channel(
            &node1.peer_manager(),
            &node1.persister(),
            channel_peer,
            channel_value_sat,
        )
        .await
        .expect("Failed to open channel");

    // After opening
    let post_node_info =
        owner::node_info(node1.channel_manager(), node1.peer_manager())
            .unwrap();
    assert_eq!(post_node_info.num_channels, 1);

    // Wait for a graceful shutdown to complete before exiting this test (and
    // thus dropping BitcoinD which kills the bitcoind process) so that the
    // event handler has enough time to handle the FundingGenerationReady event
    // before BitcoinD is dropped (and `kill`ed), otherwise this test fails.
    node1.sync().await;
    node1.run().await;
    node2.sync().await;
    node2.run().await;
}
