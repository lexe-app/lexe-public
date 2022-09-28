use std::fmt::{self, Display};
use std::net::IpAddr;
#[cfg(all(test, not(target_env = "sgx")))]
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use anyhow::{anyhow, ensure};
use argh::FromArgs;
use bitcoin::blockdata::constants;
use bitcoin::hash_types::BlockHash;
use lightning_invoice::Currency;
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::arbitrary::{any, Arbitrary};
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::strategy::{BoxedStrategy, Just, Strategy};
#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::api::runner::Port;
use crate::api::UserPk;
use crate::constants::{NODE_PROVISION_DNS, NODE_RUN_DNS};

pub const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:5050";

/// Commands accepted by the user node.
#[derive(Clone, Debug, Eq, PartialEq, FromArgs)]
#[argh(subcommand)]
pub enum NodeCommand {
    Run(RunArgs),
    Provision(ProvisionArgs),
}

impl NodeCommand {
    /// Shorthand to get the UserPk from NodeCommand
    pub fn user_pk(&self) -> UserPk {
        match self {
            Self::Run(args) => args.user_pk,
            Self::Provision(args) => args.user_pk,
        }
    }

    /// Overrides the contained user pk. Used during tests.
    pub fn set_user_pk(&mut self, user_pk: UserPk) {
        match self {
            Self::Run(args) => {
                args.user_pk = user_pk;
            }
            Self::Provision(args) => {
                args.user_pk = user_pk;
            }
        }
    }

    /// Overrides the contained backend url. Used during tests.
    pub fn set_backend_url(&mut self, backend_url: String) {
        match self {
            Self::Run(args) => {
                args.backend_url = backend_url;
            }
            Self::Provision(args) => {
                args.backend_url = backend_url;
            }
        }
    }

    /// Overrides the contained runner url. Used during tests.
    pub fn set_runner_url(&mut self, runner_url: String) {
        match self {
            Self::Run(args) => {
                args.runner_url = runner_url;
            }
            Self::Provision(args) => {
                args.runner_url = runner_url;
            }
        }
    }

    /// Shorthand for calling to_cmd() on either RunArgs or ProvisionArgs
    pub fn to_cmd(&self, bin_path: &Path) -> Command {
        match self {
            Self::Run(args) => args.to_cmd(bin_path),
            Self::Provision(args) => args.to_cmd(bin_path),
        }
    }

    /// Shorthand for calling `append_args(cmd)` on the inner variant.
    pub fn append_args<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        match self {
            Self::Run(args) => args.append_args(cmd),
            Self::Provision(args) => args.append_args(cmd),
        }
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for NodeCommand {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        proptest::prop_oneof! {
            any::<RunArgs>().prop_map(Self::Run),
            any::<ProvisionArgs>().prop_map(Self::Provision),
        }
        .boxed()
    }
}

/// Run a user node
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "run")]
pub struct RunArgs {
    /// the Lexe user pk used in queries to the persistence API
    #[argh(option)]
    pub user_pk: UserPk,

    /// bitcoind rpc info, in the format <username>:<password>@<host>:<port>
    #[argh(option)]
    pub bitcoind_rpc: BitcoindRpcInfo,

    /// the port warp uses to accept requests from the owner.
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub owner_port: Option<Port>,

    /// the port warp uses to accept requests from the host (Lexe).
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub host_port: Option<Port>,

    /// the port on which to accept Lightning P2P connections.
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub peer_port: Option<Port>,

    /// bitcoin, testnet, regtest, or signet. Defaults to testnet.
    #[argh(option, default = "Network::default()")]
    pub network: Network,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. This only applies if no activity was detected prior
    /// to the completion of sync (which is usually what happens). Useful when
    /// starting nodes for maintenance purposes. Defaults to false.
    #[argh(switch, short = 's')]
    pub shutdown_after_sync_if_no_activity: bool,

    /// how long the node will stay online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever the node
    /// receives some activity. Defaults to 3600 seconds (1 hour)
    #[argh(option, short = 'i', default = "3600")]
    pub inactivity_timer_sec: u64,

    /// whether to start the REPL, for debugging purposes. Only takes effect if
    /// the node is run outside of SGX.
    #[argh(switch)]
    pub repl: bool,

    /// protocol://host:port of the backend.
    #[argh(option, default = "DEFAULT_BACKEND_URL.to_owned()")]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option, default = "DEFAULT_RUNNER_URL.to_owned()")]
    pub runner_url: String,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option, default = "NODE_RUN_DNS.to_owned()")]
    pub node_dns_name: String,

    /// whether to use a mock API client. Only available during development.
    #[argh(switch, short = 'm')]
    pub mock: bool,
}

impl Default for RunArgs {
    /// Non-Option<T> fields are required by the node, with no node defaults.
    /// Option<T> fields are not required by the node, and use node defaults.
    fn default() -> Self {
        Self {
            bitcoind_rpc: BitcoindRpcInfo::default(),
            user_pk: UserPk::from_i64(1), // Test user
            owner_port: None,
            host_port: None,
            peer_port: None,
            network: Network::default(),
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 3600,
            repl: false,
            node_dns_name: NODE_RUN_DNS.to_owned(),
            backend_url: DEFAULT_BACKEND_URL.to_owned(),
            runner_url: DEFAULT_RUNNER_URL.to_owned(),
            mock: false,
        }
    }
}

impl RunArgs {
    /// Constructs a Command from the contained args, adding no extra options.
    /// Requires the path to the node binary.
    pub fn to_cmd(&self, bin_path: &Path) -> Command {
        let mut cmd = Command::new(bin_path);
        self.append_args(&mut cmd);
        cmd
    }

    /// Serialize and append the args to an existing [`Command`].
    pub fn append_args<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        cmd.arg("run")
            .arg("--user-pk")
            .arg(&self.user_pk.to_string())
            .arg("--bitcoind-rpc")
            .arg(&self.bitcoind_rpc.to_string())
            .arg("-i")
            .arg(&self.inactivity_timer_sec.to_string())
            .arg("--network")
            .arg(&self.network.to_string())
            .arg("--backend-url")
            .arg(&self.backend_url)
            .arg("--runner-url")
            .arg(&self.runner_url)
            .arg("--node-dns-name")
            .arg(&self.node_dns_name);

        if self.shutdown_after_sync_if_no_activity {
            cmd.arg("-s");
        }
        if self.mock {
            cmd.arg("--mock");
        }
        if self.repl {
            cmd.arg("--repl");
        }
        if let Some(owner_port) = self.owner_port {
            cmd.arg("--owner-port").arg(&owner_port.to_string());
        }
        if let Some(host_port) = self.host_port {
            cmd.arg("--host-port").arg(&host_port.to_string());
        }
        if let Some(peer_port) = self.peer_port {
            cmd.arg("--peer-port").arg(&peer_port.to_string());
        }
        cmd
    }
}

/// Provision a new user node
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionArgs {
    /// the Lexe user pk to provision the node for
    #[argh(option)]
    pub user_pk: UserPk,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the which client will expect in its connection
    #[argh(option, default = "NODE_PROVISION_DNS.to_owned()")]
    pub node_dns_name: String,

    /// protocol://host:port of the backend.
    #[argh(option, default = "DEFAULT_BACKEND_URL.to_owned()")]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option, default = "DEFAULT_RUNNER_URL.to_owned()")]
    pub runner_url: String,

    /// the port to use to accept a TLS connection from the client for the
    /// provisioning process.
    #[argh(option)]
    pub port: Option<Port>,
}

impl Default for ProvisionArgs {
    fn default() -> Self {
        Self {
            user_pk: UserPk::from_i64(1), // Test user
            node_dns_name: "provision.lexe.tech".to_owned(),
            port: None,
            backend_url: DEFAULT_BACKEND_URL.to_owned(),
            runner_url: DEFAULT_RUNNER_URL.to_owned(),
        }
    }
}

impl ProvisionArgs {
    /// Constructs a Command from the contained args, adding no extra options.
    /// Requires the path to the node binary.
    pub fn to_cmd(&self, bin_path: &Path) -> Command {
        let mut cmd = Command::new(bin_path);
        self.append_args(&mut cmd);
        cmd
    }

    /// Serialize and append the args to an existing [`Command`].
    pub fn append_args<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        cmd.arg("provision")
            .arg("--user-pk")
            .arg(&self.user_pk.to_string())
            .arg("--node-dns-name")
            .arg(&self.node_dns_name)
            .arg("--backend-url")
            .arg(&self.backend_url)
            .arg("--runner-url")
            .arg(&self.runner_url);
        if let Some(port) = self.port {
            cmd.arg("--port").arg(&port.to_string());
        }
        cmd
    }
}

/// The information required to connect to a bitcoind instance via RPC
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitcoindRpcInfo {
    pub username: String,
    pub password: String,
    /// NOTE: Only ip(v4/v6) addresses will be parsed - no DNS names for now
    pub host: String,
    pub port: Port,
}

impl Default for BitcoindRpcInfo {
    fn default() -> Self {
        Self {
            username: "kek".to_owned(),
            password: "sadge".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: 8332,
        }
    }
}

impl BitcoindRpcInfo {
    fn parse_str(s: &str) -> Option<Self> {
        // expected format: "<username>:<password>@<host>:<port>"

        // ["<username>:<password>", "<host>:<port>"]
        let mut parts = s.split('@');
        let (user_pass, host_port) =
            match (parts.next(), parts.next(), parts.next()) {
                (Some(user_pass), Some(host_port), None) => {
                    (user_pass, host_port)
                }
                _ => return None,
            };

        // ["<username>", "<password>"]
        let mut user_pass = user_pass.split(':');
        let (username, password) =
            match (user_pass.next(), user_pass.next(), user_pass.next()) {
                (Some(username), Some(password), None) => (username, password),
                _ => return None,
            };

        // rsplit_once is necessary because IPv6 addresses can contain ::
        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port)) => (host, port),
            None => return None,
        };

        // Parse host and port
        let host = IpAddr::from_str(host).ok()?;
        let port = Port::from_str(port).ok()?;

        Some(Self {
            username: username.to_owned(),
            password: password.to_owned(),
            host: host.to_string(),
            port,
        })
    }

    /// Returns a base64 encoding of "<user>:<pass>" required by the BitcoinD
    /// RPC client.
    pub fn base64_credentials(&self) -> String {
        let username = &self.username;
        let password = &self.password;
        base64::encode(format!("{username}:{password}"))
    }
}

impl FromStr for BitcoindRpcInfo {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_str(s).ok_or_else(|| anyhow!("Invalid bitcoind rpc URL"))
    }
}

impl Display for BitcoindRpcInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for BitcoindRpcInfo {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        (
            // + denotes "at least 1"
            "[A-Za-z0-9]+",
            "[A-Za-z0-9]+",
            // Only support IP addresses for now; no DNS
            any::<SocketAddr>(),
        )
            .prop_map(|(username, password, socket_addr)| Self {
                username,
                password,
                host: socket_addr.ip().to_string(),
                port: socket_addr.port(),
            })
            .boxed()
    }
}

/// There are slight variations is how the network is represented as strings
/// across bitcoind rpc calls, lightning, etc. For consistency, we use the
/// mapping defined in bitcoin::Network's FromStr impl, which is:
///
/// - Bitcoin <-> "bitcoin"
/// - Testnet <-> "testnet",
/// - Signet <-> "signet",
/// - Regtest <-> "regtest"
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Network(bitcoin::Network);

impl Network {
    pub fn into_inner(self) -> bitcoin::Network {
        self.0
    }

    pub fn to_str(self) -> &'static str {
        match self.into_inner() {
            bitcoin::Network::Bitcoin => "bitcoin",
            bitcoin::Network::Testnet => "testnet",
            bitcoin::Network::Regtest => "regtest",
            bitcoin::Network::Signet => "signet",
        }
    }

    /// Gets the blockhash of the genesis block of this [`Network`]
    pub fn genesis_hash(self) -> BlockHash {
        constants::genesis_block(self.into_inner())
            .header
            .block_hash()
    }
}

impl Default for Network {
    fn default() -> Self {
        Self(bitcoin::Network::Regtest)
    }
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let network = bitcoin::Network::from_str(s)?;
        ensure!(
            !matches!(network, bitcoin::Network::Bitcoin),
            "Mainnet is disabled for now"
        );
        Ok(Self(network))
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl From<Network> for bitcoin_bech32::constants::Network {
    fn from(network: Network) -> Self {
        match network.into_inner() {
            bitcoin::Network::Bitcoin => {
                bitcoin_bech32::constants::Network::Bitcoin
            }
            bitcoin::Network::Testnet => {
                bitcoin_bech32::constants::Network::Testnet
            }
            bitcoin::Network::Regtest => {
                bitcoin_bech32::constants::Network::Regtest
            }
            bitcoin::Network::Signet => {
                bitcoin_bech32::constants::Network::Signet
            }
        }
    }
}

impl From<Network> for Currency {
    fn from(network: Network) -> Self {
        match network.into_inner() {
            bitcoin::Network::Bitcoin => Currency::Bitcoin,
            bitcoin::Network::Testnet => Currency::BitcoinTestnet,
            bitcoin::Network::Regtest => Currency::Regtest,
            bitcoin::Network::Signet => Currency::Signet,
        }
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for Network {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        proptest::prop_oneof! {
            // TODO: Mainnet is disabled for now
            // Just(Network(bitcoin::Network::Bitcoin)),
            Just(Network(bitcoin::Network::Testnet)),
            Just(Network(bitcoin::Network::Regtest)),
            Just(Network(bitcoin::Network::Signet)),
        }
        .boxed()
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use std::net::Ipv4Addr;

    use proptest::{prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn test_parse_bitcoind_rpc_info() {
        let expected = BitcoindRpcInfo {
            username: "hello".to_string(),
            password: "world".to_string(),
            host: Ipv4Addr::new(127, 0, 0, 1).to_string(),
            port: 1234,
        };
        let actual =
            BitcoindRpcInfo::from_str("hello:world@127.0.0.1:1234").unwrap();
        assert_eq!(expected, actual);
    }

    proptest! {
        fn bitcoind_rpc_roundtrip(info1 in any::<BitcoindRpcInfo>()) {
            let info2 = BitcoindRpcInfo::from_str(&info1.to_string()).unwrap();
            prop_assert_eq!(info1, info2);
        }
    }

    #[test]
    fn test_network_roundtrip() {
        // TODO: Mainnet is disabled for now
        // let mainnet1 = Network(bitcoin::Network::Bitcoin);
        let testnet1 = Network(bitcoin::Network::Testnet);
        let regtest1 = Network(bitcoin::Network::Regtest);
        let signet1 = Network(bitcoin::Network::Signet);

        // let mainnet2 = Network::from_str(&mainnet1.to_string()).unwrap();
        let testnet2 = Network::from_str(&testnet1.to_string()).unwrap();
        let regtest2 = Network::from_str(&regtest1.to_string()).unwrap();
        let signet2 = Network::from_str(&signet1.to_string()).unwrap();

        // assert_eq!(mainnet1, mainnet2);
        assert_eq!(testnet1, testnet2);
        assert_eq!(regtest1, regtest2);
        assert_eq!(signet1, signet2);
    }

    proptest! {
        #[test]
        fn proptest_cmd_roundtrip(
            path_str in ".*",
            cmd in any::<NodeCommand>(),
        ) {
            do_cmd_roundtrip(path_str, &cmd);
        }
    }

    fn do_cmd_roundtrip(path_str: String, cmd1: &NodeCommand) {
        let path = Path::new(&path_str);
        // Convert to std::process::Command
        let std_cmd = cmd1.to_cmd(path);
        // Convert to an iterator over &str args
        let mut args_iter = std_cmd.get_args().filter_map(|s| s.to_str());
        // Pop the first arg which contains the subcommand name e.g. 'run'
        let subcommand = args_iter.next().unwrap();
        // Collect the remaining args into a vec
        let cmd_args: Vec<&str> = args_iter.collect();
        dbg!(&cmd_args);
        // Deserialize back into struct
        let cmd2 = NodeCommand::from_args(&[subcommand], &cmd_args).unwrap();
        // Assert
        assert_eq!(*cmd1, cmd2);
    }

    #[test]
    fn test_cmd_regressions() {
        use bitcoin::Network::Testnet;
        use NodeCommand::*;

        // --mock was needed
        let path_str = String::from(".");
        let cmd = Run(RunArgs {
            user_pk: UserPk::from_i64(0),
            bitcoind_rpc: BitcoindRpcInfo {
                username: "0".into(),
                password: "a".into(),
                host: "0.0.0.0".into(),
                port: 0,
            },
            owner_port: None,
            host_port: None,
            peer_port: None,
            network: Network(Testnet),
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 0,
            repl: false,
            backend_url: "".into(),
            runner_url: "".into(),
            node_dns_name: "localhost".to_owned(),
            mock: true,
        });
        do_cmd_roundtrip(path_str, &cmd);

        // --repl was needed
        let path_str = String::from(".");
        let cmd = Run(RunArgs {
            user_pk: UserPk::from_i64(0),
            bitcoind_rpc: BitcoindRpcInfo {
                username: "0".into(),
                password: "A".into(),
                host: "0.0.0.0".into(),
                port: 0,
            },
            owner_port: None,
            host_port: None,
            peer_port: None,
            network: Network(Testnet),
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 0,
            repl: true,
            backend_url: "".into(),
            runner_url: "".into(),
            node_dns_name: "localhost".to_owned(),
            mock: false,
        });
        do_cmd_roundtrip(path_str, &cmd);
    }
}
