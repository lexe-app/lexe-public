use std::str::FromStr;

use bitcoind::{self, BitcoinD};
use common::rng::SysRng;

use crate::cli::StartCommand;
use crate::init;
use crate::types::{BitcoindRpcInfo, Network, NodeAlias};

const DEFAULT_TEST_USER_ID: i64 = 1;

#[allow(dead_code)]
struct OwnerTestHarness {
    bitcoind: BitcoinD,
}

impl OwnerTestHarness {
    async fn init() -> Self {
        // Init bitcoind
        let exe_path = bitcoind::downloaded_exe_path()
            .expect("Didn't specify bitcoind version in feature flags");
        let bitcoind =
            BitcoinD::new(exe_path).expect("Failed to init bitcoind");

        // Construct args to be used in tests
        let rpc_info = BitcoindRpcInfo {
            username: String::new(), // TODO extract from bitcoind params
            password: String::new(), // TODO extract from bitcoind params
            host: String::new(),     // TODO extract from bitcoind params
            port: 6969,              // TODO extract from bitcoind params
        };
        let args = StartCommand {
            bitcoind_rpc: rpc_info,
            user_id: DEFAULT_TEST_USER_ID,
            peer_port: None,
            announced_node_name: NodeAlias::default(),
            network: Network::from_str("regtest").unwrap(),
            warp_port: None,
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 3600,
            repl: false,
        };

        // Init node
        let mut rng = SysRng::new();
        init::start_ldk(&mut rng, args)
            .await
            .expect("Error starting ldk");

        Self { bitcoind }
    }
}

#[tokio::test]
#[should_panic] // TODO remove once that isht works
async fn init() {
    OwnerTestHarness::init().await;
}
