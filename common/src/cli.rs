use std::fmt::{self, Display};
#[cfg(all(test, not(target_env = "sgx")))]
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use anyhow::{anyhow, ensure};
use argh::FromArgs;
use lightning_invoice::Currency;
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::arbitrary::{any, Arbitrary};
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::strategy::{BoxedStrategy, Just, Strategy};
#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;

use crate::api::runner::Port;
use crate::api::UserPk;
use crate::constants::{NODE_PROVISION_DNS, NODE_RUN_DNS};
use crate::enclave::{self, MachineId};

pub const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:5050";

#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
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

/// Run the Lexe node
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

    /// testnet or mainnet. Defaults to testnet.
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

    /// protocol://host:port of the node backend.
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

/// Provision a new Lexe node for a user
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionArgs {
    /// the Lexe user pk to provision the node for
    #[argh(option)]
    pub user_pk: UserPk,

    /// identifies the current CPU hardware we're running on. The node enclave
    /// should be able to unseal its own sealed data if this id is the same
    /// (unless we're trying to unseal data sealed with a newer CPUSVN or
    /// different enclave measurement).
    #[argh(option, default = "enclave::machine_id()")]
    pub machine_id: MachineId,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option, default = "NODE_PROVISION_DNS.to_owned()")]
    pub node_dns_name: String,

    /// protocol://host:port of the node backend.
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
            machine_id: enclave::machine_id(),
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
        cmd.arg("provision")
            .arg("--user-pk")
            .arg(&self.user_pk.to_string())
            .arg("--machine-id")
            .arg(&self.machine_id.to_string())
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
        // format: <username>:<password>@<host>:<port>
        let mut parts = s.split(':');
        let (username, pass_host, port) =
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(username), Some(pass_host), Some(port), None) => {
                    (username, pass_host, port)
                }
                _ => return None,
            };

        let mut parts = pass_host.split('@');
        let (password, host) = match (parts.next(), parts.next(), parts.next())
        {
            (Some(password), Some(host), None) => (password, host),
            _ => return None,
        };

        let port = Port::from_str(port).ok()?;

        Some(Self {
            username: username.to_string(),
            password: password.to_string(),
            host: host.to_string(),
            port,
        })
    }

    /// Returns a base64 encoding of "<user>:<pass>" required by the BitcoinD
    /// RPC client.
    pub fn base64_credentials(&self) -> String {
        base64::encode(format!(
            "{}:{}",
            self.username.clone(),
            self.password.clone(),
        ))
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
            // NOTE: bitcoind-rpc parsing currently only supports ipv4
            any::<Ipv4Addr>().prop_map(|x| x.to_string()),
            any::<Port>(),
        )
            .prop_map(|(username, password, host, port)| Self {
                username,
                password,
                host,
                port,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

impl Default for Network {
    fn default() -> Self {
        Self(bitcoin::Network::Testnet)
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
    use proptest::proptest;

    use super::*;

    #[test]
    fn test_parse_bitcoind_rpc_info() {
        let expected = BitcoindRpcInfo {
            username: "hello".to_string(),
            password: "world".to_string(),
            host: "foo.bar".to_string(),
            port: 1234,
        };
        let actual =
            BitcoindRpcInfo::from_str("hello:world@foo.bar:1234").unwrap();
        assert_eq!(expected, actual);
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
