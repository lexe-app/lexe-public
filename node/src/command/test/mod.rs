#![allow(dead_code)] // TODO remove

use std::str::FromStr;

use bitcoind::{self, BitcoinD, Conf};
use common::rng::SysRng;

use crate::bitcoind_client::BitcoindRpcInfo;
use crate::cli::{StartCommand, DEFAULT_BACKEND_URL};
use crate::init;
use crate::types::{Network, NodeAlias};

mod mock_runner;

const DEFAULT_TEST_USER_ID: i64 = 1;

struct OwnerTestHarness {
    bitcoind: BitcoinD,
}

impl OwnerTestHarness {
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

        // Start mock runner service
        let (runner_addr, runner_fut) = warp::serve(mock_runner::routes())
            // Let the OS assign a port for us
            .bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(async move {
            runner_fut.await;
        });
        let runner_port = runner_addr.port();
        let runner_url = format!("http://127.0.0.1:{}", runner_port);

        // Construct args to be used in tests
        let rpc_info = BitcoindRpcInfo {
            username: String::from("kek"),
            password: String::from("sadge"),
            host,
            port,
        };
        let args = StartCommand {
            bitcoind_rpc: rpc_info,
            user_id: DEFAULT_TEST_USER_ID,
            peer_port: None,
            announced_node_name: NodeAlias::default(),
            network: Network::from_str("regtest").unwrap(),
            warp_port: None,
            shutdown_after_sync_if_no_activity: true, // TODO change to false
            inactivity_timer_sec: 3600,
            repl: false,
            backend_url: DEFAULT_BACKEND_URL.into(),
            runner_url,
        };

        // NOTE: Several refactors needed before this works. The main issue is
        // that start_ldk() currently makes a number of calls to services that
        // are all inaccessible to the node. External API calls need to be
        // mocked so that node functions can be tested in isolation.
        //
        // - DONE Specify `RUNNER_URL` and `BACKEND_URL` using args so that they
        //   can be set here during tests
        // - DONE: Implement KV persistence so that one can more easily create a
        //   mock node backend
        // - DONE: Implement MockRunner
        // - Implement MockNodeBackend

        // Init node
        let mut rng = SysRng::new();
        init::start_ldk(&mut rng, args)
            .await
            .expect("Error starting ldk");

        Self { bitcoind }
    }
}

#[tokio::test]
async fn init() {
    // OwnerTestHarness::init().await;
}
