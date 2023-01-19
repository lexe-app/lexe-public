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
    pub async fn init() -> Self {
        // Init bitcoind
        let bitcoind_exe_path = bitcoind::downloaded_exe_path()
            .expect("Didn't specify bitcoind version in feature flags");
        // dbg!(&bitcoind_exe_path);
        let bitcoind =
            BitcoinD::new(bitcoind_exe_path).expect("Failed to init bitcoind");

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

        regtest
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

    /// Mines 1 block (50 BTC) to the given address, then ensures that the mined
    /// coinbase output is mature by mining another 100 blocks on top.
    ///
    /// Note: BDK won't detect the mined funds until the wallet is `sync()`ed.
    pub async fn fund_address(&self, addr: &Address) -> Vec<BlockHash> {
        self.fund_addresses(&[addr]).await
    }

    /// Mines 1 block (50 BTC) to the given addresses, then ensures that the
    /// mined coinbase outputs are mature by mining another 100 blocks on top.
    ///
    /// Note: BDK won't detect the mined funds until the wallet is `sync()`ed.
    pub async fn fund_addresses(
        &self,
        addresses: &[&Address],
    ) -> Vec<BlockHash> {
        debug!("Funding addresses {addresses:?}");
        let mut hashes = Vec::with_capacity(addresses.len() + 100);

        for addr in addresses {
            hashes.append(&mut self.mine_n_blocks_to_address(1, addr).await);
        }

        // Mine another 100 blocks to ensure the coinbase outputs have matured
        hashes.append(&mut self.mine_n_blocks(100).await);

        hashes
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

        // Trigger electrs-esplora to update
        self.electrsd.trigger().expect("Couldn't trigger electrs");

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
            } else {
                // - When we call .block_headers_subscribe(), electrs-esplora
                //   only returns the latest *cached* header.
                // - Every 5 seconds, electrs-esplora: (1) queries the latest
                //   block, (2) updates its mempool, and (3) updates its
                //   subscribers. Prior to this tick, a subsequent call to
                //   .block_headers_subscribe() will only return the same cached
                //   header.
                // - However, we can call .trigger() to make electrs-esplora
                //   update on-demand, so that the block generation process can
                //   proceed faster. Hence, we keep on retriggering until we've
                //   electrs has indexed the desired # of blocks. More info:
                //   https://github.com/lexe-tech/lexe/pull/85/files#r1080589441
                self.electrsd.trigger().expect("Couldn't trigger electrs");
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
