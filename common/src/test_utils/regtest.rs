use std::{cmp, env, path::PathBuf, time::Duration};

use anyhow::Context;
use bitcoin::{BlockHash, Network, PubkeyHash};
use bitcoin_hashes::Hash;
use electrsd::{
    bitcoind::{self, bitcoincore_rpc::RpcApi, BitcoinD},
    electrum_client::ElectrumApi,
    ElectrsD,
};
use tracing::{debug, info, instrument};

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
    /// Start a new local dev regtest cluster running `bitcoind` and
    /// `Blockstream/electrsd`.
    ///
    /// `data_dir`: if not None, data will be persisted _across_ runs in this
    ///             directory. Otherwise, both will save data into an ephemeral
    ///             temp. dir.
    #[instrument(skip_all, name = "(regtest)")]
    pub async fn init(data_dir: Option<PathBuf>) -> Self {
        info!("Initializing regtest");

        // Configure bitcoind
        let bitcoind_exe_path = std::env::var("BITCOIND_EXE")
            .or_else(|_| bitcoind::downloaded_exe_path())
            .expect("Didn't specify oneof `$BITCOIND_EXE` or bitcoind version in feature flags");
        debug!(%bitcoind_exe_path);

        let mut bitcoind_conf = bitcoind::Conf::default();
        bitcoind_conf.staticdir = data_dir.as_ref().map(|d| d.join("bitcoind"));

        // Init bitcoind
        let bitcoind = BitcoinD::with_conf(bitcoind_exe_path, &bitcoind_conf)
            .expect("Failed to init bitcoind");

        // Construct electrsd conf
        let electrsd_exe_path = std::env::var("ELECTRS_EXE")
            .ok()
            .or_else(electrsd::downloaded_exe_path)
            .expect("Didn't specify oneof `$ELECTRS_EXE` or electrsd version in feature flags");
        debug!(%electrsd_exe_path);

        let mut electrsd_conf = electrsd::Conf::default();
        electrsd_conf.staticdir = data_dir.as_ref().map(|d| d.join("electrsd"));
        // Expose esplora endpoint
        electrsd_conf.http_enabled = true;

        // Include electrsd stderr if RUST_LOG begins with "trace".
        // This is v helpful to enable if you're having problems with electrsd
        if let Some(log_os_str) = env::var_os("RUST_LOG") {
            let log_str = log_os_str
                .into_string()
                .expect("Could not convert into utf-8")
                .to_lowercase();
            if log_str.starts_with("trace") {
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
        info!("Esplora URL: {esplora_url}");

        let regtest = Self {
            bitcoind,
            electrsd,
            esplora_url,
        };

        // Mine some blocks so that chain sync doesn't (unrealistically) see a
        // completely empty history
        regtest.mine_n_blocks(6).await;

        info!("Successfully initialized regtest");
        regtest
    }

    /// Kills the underlying [`ElectrsD`] and [`BitcoinD`] processes.
    pub fn kill(&mut self) -> anyhow::Result<()> {
        info!("Killing regtest");
        let electrsd_res = self.electrsd.kill();
        let bitcoind_res = self.bitcoind.stop();
        bitcoind_res.context("Could not kill bitcoind")?;
        electrsd_res.context("Could not kill electrsd")?;
        info!("Successfully killed regtest");
        Ok(())
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
    pub async fn fund_address(
        &self,
        addr: &bitcoin::Address,
    ) -> Vec<BlockHash> {
        self.fund_addresses(&[addr]).await
    }

    /// Mines 1 block (50 BTC) to the given addresses, then ensures that the
    /// mined coinbase outputs are mature by mining another 100 blocks on top.
    ///
    /// Note: BDK won't detect the mined funds until the wallet is `sync()`ed.
    pub async fn fund_addresses(
        &self,
        addresses: &[&bitcoin::Address],
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

    /// Mines the given number of blocks to the given [`bitcoin::Address`].
    async fn mine_n_blocks_to_address(
        &self,
        num_blocks: usize,
        address: &bitcoin::Address,
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

        // Poll once a second, for up to 60 seconds, to confirm that esplora has
        // reached the correct new block height. This way, we can ensure the
        // esplora server is up-to-date before telling nodes to resync.
        // There does not appear to be any clean blocking or async API for this.
        let mut poll_timer = tokio::time::interval(Duration::from_secs(1));
        let expected_height = pre_height + num_blocks;
        let mut highest_seen_height = pre_height;
        for i in 0..60 {
            poll_timer.tick().await;
            debug!("Polling for block header notification");
            // We use .block_headers_subscribe() instead of .block_headers_pop()
            // because the latter often fails to actually notify us, causing
            // tests to hang for the full 60 seconds.
            let post_height = self
                .electrsd
                .client
                .block_headers_subscribe()
                .expect("Could not fetch latest block header")
                .height;
            highest_seen_height = cmp::max(highest_seen_height, post_height);
            if post_height >= expected_height {
                debug!(
                    "Got to height {post_height} in {i}s, \
                    expected height {expected_height}"
                );
                return blockhashes;
            } else {
                // - When we call .block_headers_subscribe(), electrs-esplora
                //   only returns the latest *cached* header.
                // - Every 5 secs, Blockstream/electrs: (1) queries the latest
                //   block, (2) updates its mempool, and (3) updates its
                //   subscribers. Prior to this tick, a subsequent call to
                //   .block_headers_subscribe() will only return the same cached
                //   header.
                // - However*, we can call .trigger() to make electrs-esplora
                //   update on-demand, so that the block generation process can
                //   proceed faster. Hence, we keep on retriggering until we've
                //   electrs has indexed the desired # of blocks. More info:
                //   https://github.com/lexe-app/lexe/pull/85/files#r1080589441
                // - *However, Blockstream/electrs currently has a bug where
                //   sending a trigger signal more than once every 5 seconds
                //   will cause Waiter::wait to recurse indefinitely, resulting
                //   in the main thread never getting woken to sync. Thus, the
                //   trigger below is currently commented out. The fix is in
                //   https://github.com/Blockstream/electrs/pull/53. Once it has
                //   been merged to Blockstream/electrs and electrsd has
                //   released a Blockstream/electrs binary (via the "esplora_*"
                //   feature flag) which contains the fix, we should enable this
                //   line again to speed up our tests a bit.
                // self.electrsd.trigger().expect("Couldn't trigger electrs");
            }
        }

        panic!(
            "Failed to mine {num_blocks} blocks to {address}. \
            Expected height {expected_height}, got {highest_seen_height}."
        );
    }
}

/// Helper to get a dummy [`bitcoin::Address`] which blocks can be mined to
fn get_dummy_address() -> bitcoin::Address {
    let pkh = PubkeyHash::from_byte_array([0; 20]);
    let network = Network::Regtest;
    bitcoin::Address::p2pkh(pkh, network)
}
