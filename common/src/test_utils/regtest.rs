use std::env;
use std::time::Duration;

use bitcoin::hash_types::PubkeyHash;
use bitcoin::hashes::Hash;
use bitcoin::network::constants::Network;
use bitcoin::util::address::{Address, Payload};
use bitcoin::BlockHash;
use electrsd::bitcoind::bitcoincore_rpc::RpcApi;
use electrsd::bitcoind::{self, BitcoinD};
use electrsd::electrum_client::ElectrumApi;
use electrsd::ElectrsD;
use tracing::{debug, trace};

use crate::cli::BitcoindRpcInfo;

/// A wrapper around [`BitcoinD`] and [`ElectrsD`] which exposes simple methods
/// for launching a bitcoind regtest instance and esplora server, funding
/// addresses, and generating blocks.
///
/// Note that [`BitcoinD`] / [`ElectrsD`]'s [`Drop`] impl kills the spawned
/// bitcoind process, so make sure that the [`Regtest`] handle remains in scope
/// for as long as the bitcoind process is needed.
pub struct Regtest {
    bitcoind: BitcoinD,
    electrsd: ElectrsD,
    esplora_url: String,
}

impl Regtest {
    pub async fn init() -> (Self, BitcoindRpcInfo) {
        // Construct bitcoin.conf
        let mut bitcoind_conf = bitcoind::Conf::default();
        // This rpcauth string corresponds to user `kek` and password `sadge`
        bitcoind_conf.args.push("-rpcauth=kek:b6c15926aee7ebfbd3669ec8a6515c79$2dba596a7d651187021b1f56d339f0fe465c2ab1b81c37b05e07a320b07822d7");
        let username = "kek".to_owned();
        let password = "sadge".to_owned();

        // Init bitcoind
        let bitcoind_exe_path = bitcoind::downloaded_exe_path()
            .expect("Didn't specify bitcoind version in feature flags");
        // dbg!(&bitcoind_exe_path);
        let bitcoind = BitcoinD::with_conf(bitcoind_exe_path, &bitcoind_conf)
            .expect("Failed to init bitcoind");
        let host = bitcoind.params.rpc_socket.ip().to_string();
        let port = bitcoind.params.rpc_socket.port();
        let bitcoind_rpc_info = BitcoindRpcInfo {
            username,
            password,
            host,
            port,
        };
        // dbg!(&bitcoind_rpc_info);

        // Construct electrsd conf
        let electrsd_exe_path = electrsd::downloaded_exe_path()
            .expect("Didn't specify electrsd version in feature flags");
        // dbg!(&electrsd_exe_path);
        let mut electrsd_conf = electrsd::Conf::default();
        // Expose esplora endpoint
        electrsd_conf.http_enabled = true;
        // Include electrsd stderr if RUST_LOG begins with "debug" or "trace"
        // This is v helpful to enable if you're having problems with electrsd
        if let Some(log_os_str) = env::var_os("RUST_LOG") {
            let log_str = log_os_str
                .into_string()
                .expect("Could not convert into utf-8")
                .to_lowercase();
            if log_str.starts_with("debug") || log_str.starts_with("trace") {
                electrsd_conf.view_stderr = true;
            }
        }

        // Init electrsd
        let electrsd =
            ElectrsD::with_conf(electrsd_exe_path, &bitcoind, &electrsd_conf)
                .expect("Failed to init electrsd");
        let esplora_url = match electrsd.esplora_url {
            Some(ref url) => format!("http://{url}"),
            None => panic!("Missing esplora feature or not enabled in Conf"),
        };
        // dbg!(&esplora_url);

        let regtest = Self {
            bitcoind,
            electrsd,
            esplora_url,
        };

        // Mine some blocks so that chain sync doesn't (unrealistically) see a
        // completely empty history
        regtest.mine_n_blocks(6).await;

        (regtest, bitcoind_rpc_info)
    }

    /// Get the esplora URL, e.g. `http://0.0.0.0:59416`
    pub fn esplora_url(&self) -> String {
        self.esplora_url.clone()
    }

    /// Mines n blocks. Block rewards are sent to a dummy address.
    pub async fn mine_n_blocks(&self, n: usize) -> Vec<BlockHash> {
        debug!("Mining {n} blocks");
        self.mine_n_blocks_to_address(n, &get_dummy_address()).await
    }

    /// Mines 101 blocks to the given address. 101 blocks is needed because
    /// coinbase outputs aren't spendable until after 100 blocks.
    ///
    /// [`mine_n_blocks`]: Self::mine_n_blocks
    pub async fn fund_address(&self, address: &Address) -> Vec<BlockHash> {
        debug!("Funding address {address} by mining 101 blocks");
        self.mine_n_blocks_to_address(101, address).await
    }

    /// Mines the given number of blocks to the given [`Address`].
    async fn mine_n_blocks_to_address(
        &self,
        num_blocks: usize,
        address: &Address,
    ) -> Vec<BlockHash> {
        let pre_height = self
            .electrsd
            .client
            .block_headers_subscribe()
            .expect("Could not fetch latest block header")
            .height;
        debug!("Starting height: {pre_height}");

        let blockhashes = self
            .bitcoind
            .client
            // Weird that this is u64 but ok
            .generate_to_address(num_blocks as u64, address)
            .expect("Failed to generate blocks");

        // Trigger electrsd sync.
        self.electrsd
            .trigger()
            .expect("Couldn't trigger electrsd sync");

        // Poll once a second, for up to a minute, to confirm that esplora has
        // reached the correct new block height. This way, we can ensure the
        // esplora server is up-to-date before telling nodes to resync.
        // There does not appear to be any clean blocking or async API for this.
        let mut poll_timer = tokio::time::interval(Duration::from_secs(1));
        for _ in 0..60 {
            poll_timer.tick().await;
            trace!("Polling for block header notification");
            let expected = pre_height + num_blocks;
            // We use .block_headers_subscribe() instead of .block_headers_pop()
            // because the latter often fails to actually notify us, causing
            // tests to hang for the full 60 seconds.
            let post_height = self
                .electrsd
                .client
                .block_headers_subscribe()
                .expect("Could not fetch latest block header")
                .height;
            if post_height >= expected {
                debug!("Got to height {post_height}, expected {expected}");
                break;
            }
        }

        blockhashes
    }
}

/// Helper to get a dummy [`Address`] which blocks can be mined to
fn get_dummy_address() -> Address {
    let hash = Hash::from_inner([0; 20]);
    let pkh = PubkeyHash::from_hash(hash);
    let payload = Payload::PubkeyHash(pkh);

    let network = Network::Regtest;
    Address { payload, network }
}
