#![allow(dead_code)] // TODO remove

use std::str::FromStr;

use bitcoind::{self, BitcoinD, Conf};
use common::hex;
use common::rng::SysRng;

use crate::bitcoind_client::BitcoindRpcInfo;
use crate::cli::StartCommand;
use crate::types::{EnclaveId, InstanceId, Network, NodeAlias};
use crate::{convert, init};

mod mock_backend;
mod mock_runner;

// --- Consts used in tests ---

pub const USER_ID: i64 = 1;
pub const PUBKEY: &str =
    "02692f6894d5cb51bb785cc3c54f457889faf674fedea54a906f7ec99e88832d18";
pub const MEASUREMENT: &str = "default";
pub const HEX_SEED: &str =
    "39ee00e3e23a9cd7e6509f56ff66daaf021cb5502e4ab3c6c393b522a6782d03";
pub const CPU_ID: &str = "my_cpu_id";
pub fn instance_id() -> InstanceId {
    format!("{}_{}", PUBKEY, MEASUREMENT)
}
pub fn seed() -> Vec<u8> {
    hex::decode(HEX_SEED).unwrap()
}
pub fn enclave_id() -> EnclaveId {
    convert::get_enclave_id(instance_id().as_str(), CPU_ID)
}

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

        // Start mock backend service
        let (backend_addr, backend_fut) = warp::serve(mock_backend::routes())
            // Let the OS assign a port for us
            .bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(async move {
            backend_fut.await;
        });
        let backend_port = backend_addr.port();
        let backend_url = format!("http://127.0.0.1:{}", backend_port);

        // Construct args to be used in tests
        let rpc_info = BitcoindRpcInfo {
            username: String::from("kek"),
            password: String::from("sadge"),
            host,
            port,
        };
        let args = StartCommand {
            bitcoind_rpc: rpc_info,
            user_id: USER_ID,
            peer_port: None,
            announced_node_name: NodeAlias::default(),
            network: Network::from_str("regtest").unwrap(),
            warp_port: None,
            shutdown_after_sync_if_no_activity: true, // TODO change to false
            inactivity_timer_sec: 3600,
            repl: false,
            backend_url,
            runner_url,
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
    OwnerTestHarness::init().await;
}
