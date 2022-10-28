use bitcoin::hash_types::PubkeyHash;
use bitcoin::hashes::Hash;
use bitcoin::network::constants::Network;
use bitcoin::util::address::{Address, Payload};
use bitcoin::BlockHash;
use bitcoind::bitcoincore_rpc::RpcApi;
use bitcoind::{self, BitcoinD, Conf};
use tracing::debug;

use crate::cli::BitcoindRpcInfo;

/// A wrapper around [`BitcoinD`] which exposes simple methods for launching a
/// bitcoind regtest instance, funding addresses, and generating blocks.
///
/// Note that [`BitcoinD`]'s [`Drop`] impl kills the spawned bitcoind process,
/// so make sure that the [`Regtest`] handle remains in scope for as long as the
/// bitcoind process is needed.
pub struct Regtest(BitcoinD);

impl Regtest {
    pub async fn init() -> (Self, BitcoindRpcInfo) {
        // Construct bitcoin.conf
        let mut conf = Conf::default();
        // This rpcauth string corresponds to user `kek` and password `sadge`
        conf.args.push("-rpcauth=kek:b6c15926aee7ebfbd3669ec8a6515c79$2dba596a7d651187021b1f56d339f0fe465c2ab1b81c37b05e07a320b07822d7");
        let username = "kek".to_owned();
        let password = "sadge".to_owned();

        // Init bitcoind
        let bitcoind_exe_path = bitcoind::downloaded_exe_path()
            .expect("Didn't specify bitcoind version in feature flags");
        let bitcoind = BitcoinD::with_conf(bitcoind_exe_path, &conf)
            .expect("Failed to init bitcoind");
        let host = bitcoind.params.rpc_socket.ip().to_string();
        let port = bitcoind.params.rpc_socket.port();

        // Construct RPC credential info
        let rpc_info = BitcoindRpcInfo {
            username,
            password,
            host,
            port,
        };

        let regtest = Self(bitcoind);

        // Mine some blocks so that chain sync doesn't (unrealistically) see a
        // completely empty history
        regtest.mine_n_blocks(6).await;

        (regtest, rpc_info)
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
        self.0
            .client
            .generate_to_address(num_blocks, address)
            .expect("Failed to generate blocks")
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

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn dummy_address_is_valid() {
        // If the dummy address is valid, we should be able to mine blocks to it
        let (regtest, _rpc_info) = Regtest::init().await;
        regtest.mine_n_blocks(6).await;
        regtest.fund_address(&get_dummy_address()).await;
    }
}
