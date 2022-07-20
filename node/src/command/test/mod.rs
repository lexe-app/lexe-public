use std::str::FromStr;

use bitcoind::{self, BitcoinD, Conf};
use common::rng::SysRng;

use crate::api::mock;
use crate::cli::{StartCommand, DEFAULT_BACKEND_URL, DEFAULT_RUNNER_URL};
use crate::init;
use crate::lexe::bitcoind::BitcoindRpcInfo;
use crate::types::{Network, NodeAlias};

#[allow(dead_code)] // TODO remove after bitcoind field is read
struct CommandTestHarness {
    bitcoind: BitcoinD,
}

impl CommandTestHarness {
    async fn init() -> Self {
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

        // Construct args to be used in tests
        let rpc_info = BitcoindRpcInfo {
            username: String::from("kek"),
            password: String::from("sadge"),
            host,
            port,
        };
        let args = StartCommand {
            bitcoind_rpc: rpc_info,
            user_id: mock::USER_ID,
            peer_port: None,
            announced_node_name: NodeAlias::default(),
            network: Network::from_str("regtest").unwrap(),
            warp_port: None,
            shutdown_after_sync_if_no_activity: true, // TODO change to false
            inactivity_timer_sec: 3600,
            repl: false,
            backend_url: DEFAULT_BACKEND_URL.into(),
            runner_url: DEFAULT_RUNNER_URL.into(),
            mock: true,
        };

        // Init node
        let mut rng = SysRng::new();
        init::start_ldk(&mut rng, args)
            .await
            .expect("Error starting ldk");

        Self { bitcoind }
    }
}

// Multi-threaded runtime required due to the background processor
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn init() {
    CommandTestHarness::init().await;
}
