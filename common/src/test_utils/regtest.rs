use bitcoin::hash_types::PubkeyHash;
use bitcoin::hashes::Hash;
use bitcoin::network::constants::Network;
use bitcoin::util::address::{Address, Payload};
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
        let exe_path = bitcoind_exe_path();
        let bitcoind = BitcoinD::with_conf(exe_path, &conf)
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
        regtest.mine_6_blocks().await;

        (regtest, rpc_info)
    }

    /// Mines 6 blocks. Block rewards are sent to a dummy address.
    pub async fn mine_6_blocks(&self) {
        debug!("Mining 6 blocks");
        // `bitcoind.client.generate()` returns a deprecated error, so we use
        // generate_to_address instead.
        self.mine_n_blocks_to_address(6, &get_dummy_address()).await;
    }

    /// Mines 101 blocks to the given address. 101 blocks is needed because
    /// coinbase outputs aren't spendable until after 100 blocks.
    pub async fn fund_address(&self, address: &Address) {
        debug!("Funding address {address} by mining 101 blocks");
        self.mine_n_blocks_to_address(101, address).await;
    }

    /// Mines the given number of blocks to the given [`Address`].
    async fn mine_n_blocks_to_address(
        &self,
        num_blocks: u64,
        address: &Address,
    ) {
        self.0
            .client
            .generate_to_address(num_blocks, address)
            .expect("Failed to generate blocks");
    }
}

/// Hacks around the recurring 'No such file or directory' error when trying to
/// locate the local bitcoind executable.
///
/// <https://github.com/RCasatta/bitcoind/issues/77>
#[rustfmt::skip]
fn bitcoind_exe_path() -> String {
    use std::env;
    // "/Users/fang/lexe/client/target/debug/build/bitcoind-65c3b20abafd4893/out/bitcoin/bitcoin-22.0/bin/bitcoind"
    // The path prior to `target` is wrong, everything after is correct
    let bitcoind_path = bitcoind::downloaded_exe_path()
        .expect("Didn't specify bitcoind version in feature flags");

    // Construct the workspace path based on env::current_dir()
    // "/Users/fang/lexe/dev/client/node"
    let crate_dir = env::current_dir().unwrap();
    // "/Users/fang/lexe/dev/client"
    let workspace_dir = crate_dir.parent().unwrap().to_str().unwrap();

    // Split on `target` to grab the correct half of the bitcoind_path string
    let mut path_halves = bitcoind_path.split("target");
    let _wrong_half = path_halves.next();
    // "/debug/build/bitcoind-65c3b20abafd4893/out/bitcoin/bitcoin-22.0/bin/bitcoind"
    let right_half = path_halves.next().unwrap();

    let exe_path = format!("{workspace_dir}/target{right_half}");

    // Uncomment for debugging when this inevitably breaks again
    // dbg!(&bitcoind_path);
    // dbg!(&crate_dir);
    // dbg!(&workspace_dir);
    // dbg!(&exe_path);

    exe_path
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
        regtest.mine_6_blocks().await;
        regtest.fund_address(&get_dummy_address()).await;
    }
}
