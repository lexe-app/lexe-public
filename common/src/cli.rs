use std::fmt::{self, Display};
use std::str::FromStr;

use anyhow::{anyhow, ensure};
use argh::FromArgs;
use lightning_invoice::Currency;

use crate::api::runner::Port;
use crate::api::UserPk;
use crate::enclave::{self, MachineId};

pub const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:5050";

#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand)]
pub enum NodeCommand {
    Start(StartArgs),
    Provision(ProvisionArgs),
}

impl NodeCommand {
    /// Shorthand to get the UserPk from NodeCommand
    pub fn user_pk(&self) -> UserPk {
        match self {
            Self::Start(args) => args.user_pk,
            Self::Provision(args) => args.user_pk,
        }
    }

    /// Overrides the contained user pk. Used during tests.
    pub fn set_user_pk(&mut self, user_pk: UserPk) {
        match self {
            Self::Start(args) => {
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
            Self::Start(args) => {
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
            Self::Start(args) => {
                args.runner_url = runner_url;
            }
            Self::Provision(args) => {
                args.runner_url = runner_url;
            }
        }
    }
}

/// Start the Lexe node
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "start")]
pub struct StartArgs {
    /// bitcoind rpc info, in the format <username>:<password>@<host>:<port>
    #[argh(positional)]
    pub bitcoind_rpc: BitcoindRpcInfo,

    /// the Lexe user pk used in queries to the persistence API
    #[argh(option)]
    pub user_pk: UserPk,

    /// the port warp uses to accept commands and TLS connections.
    #[argh(option)]
    /// Defaults to a port assigned by the OS
    pub warp_port: Option<Port>,

    /// the port on which to accept Lightning P2P connections.
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub peer_port: Option<Port>,

    /// testnet or mainnet. Defaults to testnet.
    #[argh(option, default = "Network::default()")]
    pub network: Network,

    /// this node's Lightning Network alias
    #[argh(option, default = "NodeAlias::default()")]
    pub announced_node_name: NodeAlias,

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

    /// whether to use a mock API client. Only available during development.
    #[argh(switch, short = 'm')]
    pub mock: bool,
}

impl Default for StartArgs {
    /// Non-Option<T> fields are required by the node, with no node defaults.
    /// Option<T> fields are not required by the node, and use node defaults.
    fn default() -> Self {
        Self {
            bitcoind_rpc: BitcoindRpcInfo::default(),
            user_pk: UserPk::from_i64(1), // Test user
            warp_port: None,
            peer_port: None,
            network: Network::default(),
            announced_node_name: NodeAlias::default(),
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 3600,
            repl: false,
            backend_url: DEFAULT_BACKEND_URL.to_owned(),
            runner_url: DEFAULT_RUNNER_URL.to_owned(),
            mock: false,
        }
    }
}

/// Provision a new Lexe node for a user
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionArgs {
    /// identifies the current CPU hardware we're running on. The node enclave
    /// should be able to unseal its own sealed data if this id is the same
    /// (unless we're trying to unseal data sealed with a newer CPUSVN or
    /// different enclave measurement).
    #[argh(option, default = "enclave::machine_id()")]
    pub machine_id: MachineId,

    /// the Lexe user pk to provision the node for
    #[argh(option)]
    pub user_pk: UserPk,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option, default = "String::from(\"localhost\")")]
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
            node_dns_name: "localhost".to_owned(),
            port: None,
            backend_url: DEFAULT_BACKEND_URL.to_owned(),
            runner_url: DEFAULT_RUNNER_URL.to_owned(),
        }
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

// NOTE: NodeAlias isn't meaningfully used anywhere - it's only purpose is to
// provide a Display impl for println! statements. Consider removing
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeAlias([u8; 32]);

impl NodeAlias {
    pub fn new(inner: [u8; 32]) -> Self {
        Self(inner)
    }
}

impl FromStr for NodeAlias {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        ensure!(
            bytes.len() <= 32,
            "node alias can't be longer than 32 bytes"
        );

        let mut alias = [0_u8; 32];
        alias[..bytes.len()].copy_from_slice(bytes);

        Ok(Self(alias))
    }
}

impl Display for NodeAlias {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for b in self.0.iter() {
            let c = *b as char;
            if c == '\0' {
                break;
            }
            if c.is_ascii_graphic() || c == ' ' {
                continue;
            }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for NodeAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
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

#[cfg(test)]
mod test {
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
    fn test_parse_node_alias() {
        let expected = NodeAlias(*b"hello, world - this is lexe\0\0\0\0\0");
        let actual =
            NodeAlias::from_str("hello, world - this is lexe").unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_network_roundtrip() {
        // Mainnet is disabled for now

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
}
