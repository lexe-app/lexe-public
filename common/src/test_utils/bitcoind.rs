use bitcoind::{self, BitcoinD, Conf};

use crate::cli::BitcoindRpcInfo;

/// Helper to initialize a [`BitcoinD`] regtest instance.
///
/// Note that dropping the [`BitcoinD`] kills the process, so be sure that the
/// handle remains in scope for as long as the bitcoind process is needed. A
/// good way to accomplish this is to store the handle in the test harness.
pub fn init_regtest() -> (BitcoinD, BitcoindRpcInfo) {
    // Construct bitcoin.conf
    let mut conf = Conf::default();
    // This rpcauth string corresponds to user `kek` and password `sadge`
    conf.args.push("-rpcauth=kek:b6c15926aee7ebfbd3669ec8a6515c79$2dba596a7d651187021b1f56d339f0fe465c2ab1b81c37b05e07a320b07822d7");
    let username = "kek".to_owned();
    let password = "sadge".to_owned();

    // Init bitcoind
    let exe_path = bitcoind_exe_path();
    let bitcoind =
        BitcoinD::with_conf(exe_path, &conf).expect("Failed to init bitcoind");
    let host = bitcoind.params.rpc_socket.ip().to_string();
    let port = bitcoind.params.rpc_socket.port();

    // Construct RPC credential info
    let rpc_info = BitcoindRpcInfo {
        username,
        password,
        host,
        port,
    };

    (bitcoind, rpc_info)
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

    dbg!(&bitcoind_path);
    dbg!(&crate_dir);
    dbg!(&workspace_dir);
    dbg!(&exe_path);

    exe_path
}
