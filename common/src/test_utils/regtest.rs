use bitcoin::hash_types::PubkeyHash;
use bitcoin::hashes::Hash;
use bitcoin::network::constants::Network;
use bitcoin::util::address::{Address, Payload};
use bitcoin::BlockHash;
use electrsd::bitcoind::bitcoincore_rpc::RpcApi;
use electrsd::bitcoind::{self, BitcoinD};
use electrsd::ElectrsD;
use tracing::debug;

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
        // NOTE: Uncomment the following if electrsd is failing to start up;
        // gives helpful information for debugging
        // electrsd_conf.view_stderr = true;

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
    ///
    /// NOTE: If you mine more than 3 blocks at once without givinng nodes that
    /// have completed sync a chance to finish persisting their updated channel
    /// monitors, the ChainMonitor will error with "A ChannelMonitor sync took
    /// longer than 3 blocks to complete." The correct ways to mine more than 3
    /// blocks in an integration test is:
    ///
    /// 1) To mine them before the node has completed its `sync()` stage
    /// 2) To mine them three blocks at a time, waiting for channel monitors to
    ///   persist in between.
    pub async fn mine_n_blocks(&self, n: u64) -> Vec<BlockHash> {
        debug!("Mining {n} blocks");
        self.mine_n_blocks_to_address(n, &get_dummy_address()).await
    }

    /// Mines 101 blocks to the given address. 101 blocks is needed because
    /// coinbase outputs aren't spendable until after 100 blocks.
    ///
    /// NOTE: Due to the limitations documented in [`mine_n_blocks`] above, this
    /// function should only be called *before* the node has reached the sync()
    /// stage.
    ///
    /// [`mine_n_blocks`]: Self::mine_n_blocks
    pub async fn fund_address(&self, address: &Address) -> Vec<BlockHash> {
        debug!("Funding address {address} by mining 101 blocks");
        self.mine_n_blocks_to_address(101, address).await
    }

    /// Mines the given number of blocks to the given [`Address`].
    async fn mine_n_blocks_to_address(
        &self,
        num_blocks: u64,
        address: &Address,
    ) -> Vec<BlockHash> {
        let blockhashes = self
            .bitcoind
            .client
            .generate_to_address(num_blocks, address)
            .expect("Failed to generate blocks");

        // Trigger electrsd sync.
        self.electrsd.trigger().expect("Could sync electrsd");

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
