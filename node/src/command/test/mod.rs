//! Command tests.
//!
//! Note that all tests which call `CommandTestHarness::init()` must use a
//! multi-threaded runtime, since `LexeContext::init()` starts the
//! `BackgroundProcessor` which requires its own OS thread. A single worker
//! thread should be enough.

use std::str::FromStr;
use std::sync::Arc;

use bitcoind::{self, BitcoinD, Conf};
use common::rng::SysRng;

use crate::api::mock;
use crate::cli::{StartCommand, DEFAULT_BACKEND_URL, DEFAULT_RUNNER_URL};
use crate::command::owner;
use crate::init::LexeContext;
use crate::lexe::bitcoind::BitcoindRpcInfo;
use crate::lexe::peer_manager::LexePeerManager;
use crate::types::{ChannelManagerType, Network, NodeAlias};

/// Helper to return a default StartCommand struct for testing.
fn default_test_args() -> StartCommand {
    StartCommand {
        bitcoind_rpc: BitcoindRpcInfo {
            username: String::from("kek"),
            password: String::from("sadge"),
            host: String::new(), // Filled in when BitcoinD initializes
            port: 6969,          // Filled in when BitcoinD initializes
        },
        user_id: mock::USER_ID,
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

#[allow(dead_code)] // TODO remove after bitcoind field is read
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

    fn channel_manager(&self) -> Arc<ChannelManagerType> {
        self.ctx.channel_manager.clone()
    }

    fn peer_manager(&self) -> Arc<LexePeerManager> {
        self.ctx.peer_manager.clone()
    }
}

/// Tests that a node can initialize, sync, and shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn init_sync_shutdown() {
    let mut args = default_test_args();
    args.shutdown_after_sync_if_no_activity = true;

    let mut h = CommandTestHarness::init(args).await;
    h.sync().await;
    h.run().await;
}

/// Tests the node_info endpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn node_info1() {
    let args = default_test_args();
    let h = CommandTestHarness::init(args).await;

    owner::node_info(h.channel_manager(), h.peer_manager())
        .await
        .unwrap();
}
